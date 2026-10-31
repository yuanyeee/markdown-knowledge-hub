use anyhow::Result;
use std::path::Path;
use tantivy::{
    Index, TantivyDocument, Term,
    collector::TopDocs,
    doc,
    query::QueryParser,
    schema::*,
    tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer},
};

pub struct Search {
    index: Index,
    path: Field,
    title: Field,
    body: Field,
}
impl Search {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let mut schema = Schema::builder();
        let path = schema.add_text_field("path", STRING | STORED);
        let options = TextOptions::default().set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("cjk")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );
        let title = schema.add_text_field("title", options.clone());
        let body = schema.add_text_field("body", options);
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
        Ok(Self {
            index,
            path,
            title,
            body,
        })
    }
    pub fn update(
        &self,
        reset: bool,
        deleted: &[String],
        documents: &[(String, String, String)],
    ) -> Result<()> {
        let mut writer = self.index.writer::<TantivyDocument>(50_000_000)?;
        if reset {
            writer.delete_all_documents()?;
        }
        for p in deleted {
            writer.delete_term(Term::from_field_text(self.path, p));
        }
        for (p, title, body) in documents {
            writer.delete_term(Term::from_field_text(self.path, p));
            writer.add_document(doc!(self.path => p.as_str(), self.title => title.as_str(), self.body => body.as_str()))?;
        }
        writer.commit()?;
        writer.wait_merging_threads()?;
        Ok(())
    }
    pub fn query(&self, q: &str) -> Result<Vec<String>> {
        if q.trim().is_empty() {
            return Ok(vec![]);
        }
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        let mut parser = QueryParser::for_index(&self.index, vec![self.title, self.body]);
        parser.set_conjunction_by_default();
        // Treat user input as literal words rather than field/query syntax.
        let literal = q
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(" ");
        let query = parser.parse_query(&literal)?;
        let mut paths = vec![];
        for (_, address) in searcher.search(&query, &TopDocs::with_limit(50).order_by_score())? {
            let document: TantivyDocument = searcher.doc(address)?;
            if let Some(p) = document.get_first(self.path).and_then(|v| v.as_str()) {
                paths.push(p.to_string());
            }
        }
        Ok(paths)
    }
}
