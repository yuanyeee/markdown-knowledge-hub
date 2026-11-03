use anyhow::Result;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use tantivy::{
    Index, IndexReader, TantivyDocument, Term,
    collector::TopDocs,
    doc,
    query::{BooleanQuery, BoostQuery, PhraseQuery, Query, QueryParser, RegexQuery, TermQuery},
    schema::*,
    tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer},
};

pub struct Search {
    index: Index,
    path: Field,
    title: Field,
    body: Field,
    reader: Mutex<Option<IndexReader>>,
    title_one: Option<Field>,
    body_one: Option<Field>,
    results: Mutex<VecDeque<(QueryKey, Vec<String>)>>,
}
#[derive(PartialEq)]
struct QueryKey {
    text: String,
    source: String,
    offset: usize,
    limit: usize,
}
type Cache = VecDeque<(PathBuf, Arc<Search>)>;
static READERS: OnceLock<Mutex<Cache>> = OnceLock::new();
impl Search {
    pub(crate) fn current(dir: &Path) -> Result<bool> {
        Ok(!dir.join("meta.json").exists()
            || Index::open_in_dir(dir)?
                .schema()
                .get_field("title_one")
                .is_ok())
    }
    pub fn cached(dir: &Path) -> Result<Arc<Self>> {
        let cache = READERS.get_or_init(Default::default);
        if let Some((_, index)) = cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(p, _)| p == dir)
        {
            return Ok(index.clone());
        }
        let index = Arc::new(Self::open(dir)?);
        let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.push_back((dir.to_path_buf(), index.clone()));
        while cache.len() > 8 {
            cache.pop_front();
        }
        Ok(index)
    }
    pub(crate) fn evict(prefix: &Path) {
        if let Some(cache) = READERS.get() {
            cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(p, _)| !p.starts_with(prefix));
        }
    }
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let mut schema = Schema::builder();
        let path = schema.add_text_field("path", STRING | STORED);
        let options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("chars2")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let title = schema.add_text_field("title", options.clone());
        let body = schema.add_text_field("body", options);
        let one = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("chars1")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        schema.add_text_field("title_one", one.clone());
        schema.add_text_field("body_one", one);
        let schema = schema.build();
        let index = if dir.join("meta.json").exists() {
            Index::open_in_dir(dir)?
        } else {
            Index::create_in_dir(dir, schema)?
        };
        index.tokenizers().register(
            "cjk",
            TextAnalyzer::builder(NgramTokenizer::new(1, 2, false)?)
                .filter(LowerCaser)
                .build(),
        );
        for width in [1, 2] {
            index.tokenizers().register(
                &format!("chars{width}"),
                TextAnalyzer::builder(crate::tokenizer::PositionedNgram(width))
                    .filter(LowerCaser)
                    .build(),
            );
        }
        let title_one = index.schema().get_field("title_one").ok();
        let body_one = index.schema().get_field("body_one").ok();
        Ok(Self {
            index,
            path,
            title,
            body,
            reader: Mutex::new(None),
            title_one,
            body_one,
            results: Mutex::new(VecDeque::new()),
        })
    }
    pub(crate) fn document_count(&self) -> Result<u64> {
        Ok(self.index.reader()?.searcher().num_docs())
    }
    pub fn update(
        &self,
        reset: bool,
        deleted: &[String],
        documents: impl IntoIterator<Item = Result<(String, String, String)>>,
    ) -> Result<()> {
        let mut writer = self.index.writer::<TantivyDocument>(50_000_000)?;
        // Keep foreground deltas small. Explicit rebuild performs compaction.
        if !reset
            && self
                .index
                .searchable_segment_metas()?
                .iter()
                .any(|m| m.num_docs() > 0)
        {
            writer.set_merge_policy(Box::new(tantivy::merge_policy::NoMergePolicy));
        }
        if reset {
            writer.delete_all_documents()?;
        }
        for p in deleted {
            writer.delete_term(Term::from_field_text(self.path, p));
        }
        for row in documents {
            let (p, title, body) = row?;
            writer.delete_term(Term::from_field_text(self.path, &p));
            // One-character queries use dedicated fields; multi-character queries use positioned bigrams.
            if let (Some(title_one), Some(body_one)) = (self.title_one, self.body_one) {
                writer.add_document(doc!(self.path=>p.as_str(),self.title=>title.as_str(),self.body=>body.as_str(),title_one=>title.as_str(),body_one=>body.as_str()))?;
            } else {
                writer.add_document(
                    doc!(self.path=>p.as_str(),self.title=>title.as_str(),self.body=>body.as_str()),
                )?;
            }
        }
        writer.commit()?;
        writer.wait_merging_threads()?;
        Ok(())
    }
    pub fn query(&self, q: &str) -> Result<Vec<String>> {
        self.query_page(q, 0, 50)
    }
    pub fn query_page(&self, q: &str, offset: usize, limit: usize) -> Result<Vec<String>> {
        self.query_page_scoped(q, "", offset, limit)
    }

    pub fn query_page_scoped(
        &self,
        q: &str,
        source: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<String>> {
        if q.trim().is_empty() {
            return Ok(vec![]);
        }
        let key = QueryKey {
            text: q.to_string(),
            source: source.to_owned(),
            offset,
            limit,
        };
        if let Some((_, result)) = self
            .results
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(k, _)| k == &key)
        {
            return Ok(result.clone());
        }
        let reader = {
            let mut cached = self.reader.lock().unwrap_or_else(|e| e.into_inner());
            if cached.is_none() {
                *cached = Some(self.index.reader()?);
            }
            cached.as_ref().expect("initialized reader").clone()
        };
        let searcher = reader.searcher();
        let query: Box<dyn Query> =
            if let (Some(title_one), Some(body_one)) = (self.title_one, self.body_one) {
                let mut words = vec![];
                for word in q.split_whitespace() {
                    let word = word.to_lowercase();
                    let width = if word.chars().count() > 1 { 2 } else { 1 };
                    let title = if width == 1 { title_one } else { self.title };
                    let body = if width == 1 { body_one } else { self.body };
                    let make = |field| -> Box<dyn Query> {
                        let chars = word.chars().collect::<Vec<_>>();
                        let terms = chars
                            .windows(width)
                            .enumerate()
                            .map(|(position, gram)| {
                                (
                                    position,
                                    Term::from_field_text(field, &gram.iter().collect::<String>()),
                                )
                            })
                            .collect::<Vec<_>>();
                        if terms.len() == 1 {
                            Box::new(TermQuery::new(
                                terms[0].1.clone(),
                                IndexRecordOption::WithFreqsAndPositions,
                            ))
                        } else {
                            Box::new(PhraseQuery::new_with_offset(terms))
                        }
                    };
                    words.push(Box::new(BooleanQuery::union(vec![
                        Box::new(BoostQuery::new(make(title), 2.0)),
                        make(body),
                    ])) as Box<dyn Query>);
                }
                Box::new(BooleanQuery::intersection(words))
            } else {
                let mut parser = QueryParser::for_index(&self.index, vec![self.title, self.body]);
                parser.set_conjunction_by_default();
                // Treat user input as literal words rather than field/query syntax.
                let literal = q
                    .split_whitespace()
                    .map(|w| format!("\"{}\"", w.replace('\\', "\\\\").replace('"', "\\\"")))
                    .collect::<Vec<_>>()
                    .join(" ");
                parser.parse_query(&literal)?
            };
        let query = if source.is_empty() {
            query
        } else {
            let folder = regex::escape(source.trim_end_matches(".md"));
            let scope = BooleanQuery::union(vec![
                Box::new(TermQuery::new(
                    Term::from_field_text(self.path, source),
                    IndexRecordOption::Basic,
                )),
                Box::new(RegexQuery::from_pattern(
                    &format!("{folder}/.*"),
                    self.path,
                )?),
            ]);
            Box::new(BooleanQuery::intersection(vec![
                query,
                Box::new(BoostQuery::new(Box::new(scope), 0.0)),
            ]))
        };
        let mut paths = vec![];
        for (_, address) in searcher.search(
            &query,
            &TopDocs::with_limit(limit)
                .and_offset(offset)
                .order_by_score(),
        )? {
            let document: TantivyDocument = searcher.doc(address)?;
            if let Some(p) = document.get_first(self.path).and_then(|v| v.as_str()) {
                paths.push(p.to_string());
            }
        }
        let mut cache = self.results.lock().unwrap_or_else(|e| e.into_inner());
        cache.push_back((key, paths.clone()));
        while cache.len() > 32 {
            cache.pop_front();
        }
        Ok(paths)
    }
}
