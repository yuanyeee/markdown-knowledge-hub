use crate::{Hub, parser::heading_id};
use anyhow::{Result, bail};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::Serialize;
use std::sync::{Arc, Mutex};

const LARGE_DOCUMENT: usize = 256 * 1024;
const MAX_SECTION: usize = 96 * 1024;

#[derive(Clone, Serialize)]
pub struct Chapter {
    pub title: String,
    pub start: usize,
    #[serde(skip)]
    end: usize,
    #[serde(skip)]
    anchors: Vec<String>,
}

#[derive(Serialize)]
pub struct ReadingDocument {
    path: String,
    title: String,
    markdown: String,
    chapters: Option<Vec<Chapter>>,
    chapter: Option<usize>,
}

type CachedChapters = Option<(String, Arc<Vec<Chapter>>)>;
static CHAPTERS: Mutex<CachedChapters> = Mutex::new(None);

fn sections(raw: &str) -> Vec<Chapter> {
    let mut boundaries = vec![(0, "概要".to_owned(), Vec::new())];
    let mut heading = None;
    for (event, span) in Parser::new_ext(raw, Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => heading = Some((span.start, String::new())),
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, title)) = &mut heading {
                    title.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((start, title)) = heading.take() {
                    let mut anchors = vec![heading_id(&title)];
                    if let Some(id) = raw[..start]
                        .trim_end()
                        .lines()
                        .next_back()
                        .and_then(|line| line.trim().strip_prefix("<a id=\""))
                        .and_then(|line| line.strip_suffix("\"></a>"))
                    {
                        anchors.push(id.to_owned());
                    }
                    if start == 0 {
                        boundaries.clear();
                    }
                    boundaries.push((start, title.chars().take(160).collect(), anchors));
                }
            }
            _ => {}
        }
    }
    let mut chapters = Vec::new();
    for (index, (start, title, anchors)) in boundaries.iter().enumerate() {
        let end = boundaries.get(index + 1).map_or(raw.len(), |item| item.0);
        let mut cursor = *start;
        let mut part = 1;
        while cursor < end {
            let mut next = (cursor + MAX_SECTION).min(end);
            while !raw.is_char_boundary(next) {
                next -= 1;
            }
            if next < end
                && let Some(line) = raw[cursor..next].rfind('\n')
                && line > MAX_SECTION / 2
            {
                next = cursor + line + 1;
            }
            chapters.push(Chapter {
                title: if part == 1 {
                    title.clone()
                } else {
                    format!("{title}（続き {part}）")
                },
                start: cursor,
                end: next,
                anchors: if part == 1 {
                    anchors.clone()
                } else {
                    Vec::new()
                },
            });
            part += 1;
            cursor = next;
        }
    }
    chapters
}

impl Hub {
    pub fn reading_document(
        &self,
        id: &str,
        path: &str,
        fragment: &str,
    ) -> Result<ReadingDocument> {
        let _lock = self.project_lock(id, true)?;
        let db = self.db(id)?;
        let (title, hash, size): (String, String, i64) = db.query_row(
            "SELECT title,hash,length(CAST(raw AS BLOB)) FROM docs WHERE path=?1",
            [path],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let load_raw = || {
            db.query_row("SELECT raw FROM docs WHERE path=?1", [path], |row| {
                row.get::<_, String>(0)
            })
        };
        if size <= LARGE_DOCUMENT as i64 {
            return Ok(ReadingDocument {
                path: path.to_owned(),
                title,
                markdown: load_raw()?,
                chapters: None,
                chapter: None,
            });
        }
        let cached = CHAPTERS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|(key, _)| *key == hash)
            .map(|(_, value)| value.clone());
        let chapters = match cached {
            Some(value) => value,
            None => {
                let value = Arc::new(sections(&load_raw()?));
                *CHAPTERS.lock().unwrap_or_else(|error| error.into_inner()) =
                    Some((hash, value.clone()));
                value
            }
        };
        let decoded = percent_encoding::percent_decode_str(fragment).decode_utf8_lossy();
        let selected = if let Some(offset) = decoded.strip_prefix("chapter:") {
            let start: usize = offset.parse()?;
            let Some(index) = chapters.iter().position(|chapter| chapter.start == start) else {
                bail!("章が見つかりません。目次を開き直してください");
            };
            Some(index)
        } else if !decoded.is_empty() {
            chapters.iter().position(|chapter| {
                chapter
                    .anchors
                    .iter()
                    .any(|anchor| anchor == decoded.as_ref())
            })
        } else {
            None
        };
        let markdown = match selected {
            Some(index) => {
                let section = &chapters[index];
                let bytes: Vec<u8> = db.query_row(
                    "SELECT substr(CAST(raw AS BLOB), ?2, ?3) FROM docs WHERE path=?1",
                    rusqlite::params![
                        path,
                        (section.start + 1) as i64,
                        (section.end - section.start) as i64
                    ],
                    |row| row.get(0),
                )?;
                String::from_utf8(bytes)?
            }
            None => String::new(),
        };
        Ok(ReadingDocument {
            path: path.to_owned(),
            title,
            markdown,
            chapters: Some(chapters.as_ref().clone()),
            chapter: selected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_documents_open_as_toc_and_load_only_the_requested_utf8_section() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let raw = format!(
            "# 概要\n\n{}\n\n<a id=\"action-a\"></a>\n\n## 保存\n\n本文\n",
            "長い説明です。\n".repeat(15000)
        );
        std::fs::write(source.path().join("large.md"), &raw).unwrap();
        std::fs::write(source.path().join("small.md"), "# 小さい文書").unwrap();
        let mut hub = Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();

        let toc = hub.reading_document("demo", "large.md", "").unwrap();
        assert!(toc.markdown.is_empty());
        assert!(toc.chapter.is_none());
        let chapters = toc.chapters.unwrap();
        assert!(chapters.len() > 2);
        for chapter in chapters {
            let selected = hub
                .reading_document("demo", "large.md", &format!("chapter:{}", chapter.start))
                .unwrap();
            assert_eq!(selected.markdown, raw[chapter.start..chapter.end]);
            assert!(selected.markdown.len() <= MAX_SECTION);
        }
        let selected = hub
            .reading_document("demo", "large.md", "action-a")
            .unwrap();
        assert_eq!(selected.markdown, "## 保存\n\n本文\n");
        let small = hub.reading_document("demo", "small.md", "").unwrap();
        assert_eq!(small.markdown, "# 小さい文書");
        assert!(small.chapters.is_none());
    }
}
