use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

pub struct Element<'a> {
    pub kind: &'static str,
    pub title: String,
    pub line: usize,
    pub body: &'a str,
    pub anchors: Vec<String>,
}

fn detail_heading(title: &str) -> bool {
    let title = title.split(['(', '（']).next().unwrap_or(title).trim();
    matches!(
        title,
        "入力パラメータ"
            | "出力パラメータ"
            | "入出力パラメータ"
            | "ローカル変数"
            | "処理フロー"
            | "処理フロー図"
            | "処理詳細"
            | "パラメータ"
            | "変数"
            | "説明"
            | "概要"
            | "属性"
    )
}

pub fn extract(source: &str) -> Vec<Element<'_>> {
    let mut headings = Vec::new();
    let mut heading = None;
    for (event, span) in Parser::new(source).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading = Some((level, span.start, String::new()));
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, _, title)) = &mut heading {
                    title.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = heading.take() {
                    headings.push(heading);
                }
            }
            _ => {}
        }
    }
    let mut section = "";
    let mut elements = Vec::new();
    let newlines: Vec<_> = source
        .match_indices('\n')
        .map(|(offset, _)| offset)
        .collect();
    for (index, (level, offset, title)) in headings.iter().enumerate() {
        if *level <= HeadingLevel::H2 {
            section = title;
        }
        if detail_heading(title) {
            continue;
        }
        let prefixes = [
            ("画面:", "画面"),
            ("サーバーアクション:", "サーバーアクション"),
            ("クライアントアクション:", "クライアントアクション"),
            ("エンティティ:", "Entity"),
            ("[エンティティ]", "Entity"),
            ("[アクション]", "アクション"),
            ("[関数]", "関数"),
        ];
        let classified = prefixes
            .iter()
            .find_map(|(prefix, kind)| title.strip_prefix(prefix).map(|name| (*kind, name.trim())))
            .or_else(|| {
                if *level != HeadingLevel::H3 {
                    return None;
                }
                if section.starts_with("スクリーンアクション")
                    || section.starts_with("画面アクション")
                {
                    Some(("画面アクション", title.as_str()))
                } else if section.starts_with("エンティティ") {
                    Some(("Entity", title.as_str()))
                } else if section.starts_with("サーバーアクション") {
                    Some(("サーバーアクション", title.as_str()))
                } else {
                    None
                }
            });
        let slug = crate::parser::heading_id(title);
        let prior = source[..*offset].trim_end();
        let explicit = prior
            .lines()
            .next_back()
            .and_then(|line| line.trim().strip_prefix("<a id=\""))
            .and_then(|line| line.strip_suffix("\"></a>"));
        let mut anchors = vec![slug];
        if let Some(id) = explicit.filter(|id| *id != anchors[0]) {
            anchors.push(id.to_owned());
        }
        let Some((kind, title)) = classified.filter(|(_, title)| !title.is_empty()) else {
            continue;
        };
        let end = headings[index + 1..]
            .iter()
            .find(|(next, _, title)| next < level || (next == level && !detail_heading(title)))
            .map_or(source.len(), |(_, offset, _)| *offset);
        elements.push(Element {
            kind,
            title: title.to_owned(),
            line: newlines.partition_point(|position| position < offset) + 1,
            body: &source[*offset..end],
            anchors,
        });
    }
    elements
}

/// Only explicit action execution rows in OML flow tables establish a call.
pub fn calls(markdown: &str) -> std::collections::BTreeSet<String> {
    use pulldown_cmark::Options;
    let mut columns = None;
    let mut cells = Vec::new();
    let mut cell = None;
    let mut result = std::collections::BTreeSet::new();
    for event in Parser::new_ext(markdown, Options::ENABLE_TABLES) {
        match event {
            Event::Start(Tag::Table(_)) => columns = None,
            Event::Start(Tag::TableHead | Tag::TableRow) => cells.clear(),
            Event::Start(Tag::TableCell) => cell = Some(String::new()),
            Event::Text(text) | Event::Code(text) => {
                if let Some(cell) = &mut cell {
                    cell.push_str(&text);
                }
            }
            Event::End(TagEnd::TableCell) => {
                cells.push(cell.take().unwrap_or_default().trim().to_owned())
            }
            Event::End(TagEnd::TableHead) => {
                columns = cells
                    .iter()
                    .position(|v| v == "種別")
                    .zip(cells.iter().position(|v| v == "処理"))
                    .filter(|_| cells.iter().any(|v| v == "次へ"));
            }
            Event::End(TagEnd::TableRow) => {
                if let Some((kind, label)) = columns
                    && cells.get(kind).is_some_and(|v| v == "アクション実行")
                    && let Some(value) = cells.get(label).filter(|v| !v.is_empty())
                {
                    result.insert(value.clone());
                }
            }
            _ => {}
        }
    }
    result
}

pub fn route_fragments(
    markdown: &str,
    anchors: &std::collections::BTreeMap<String, Vec<usize>>,
    prefix: &str,
) -> String {
    use pulldown_cmark::Options;
    let mut result = String::new();
    let mut end = 0;
    for (event, span) in Parser::new_ext(markdown, Options::ENABLE_TABLES).into_offset_iter() {
        let Event::Start(Tag::Link { dest_url, .. }) = event else {
            continue;
        };
        let Some(fragment) = dest_url.strip_prefix('#') else {
            continue;
        };
        let Ok(fragment) = percent_encoding::percent_decode_str(fragment).decode_utf8() else {
            continue;
        };
        let Some([number]) = anchors.get(fragment.as_ref()).map(Vec::as_slice) else {
            continue;
        };
        let original = &markdown[span.clone()];
        let Some(label_end) = original.rfind("](") else {
            continue;
        };
        if span.start < end || !original.starts_with('[') || !original.ends_with(')') {
            continue;
        }
        result.push_str(&markdown[end..span.start]);
        result.push_str(&original[..label_end + 2]);
        result.push_str(&format!("{prefix}{number}.md)"));
        end = span.end;
    }
    result.push_str(&markdown[end..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_detail_headings_stay_inside_the_action() {
        let markdown = "# 画面: Edit\n\n## スクリーンアクション\n\n### Save\n\n### 入力パラメータ (1)\n\nInput\n\n### ローカル変数\n\nLocal\n\n### 処理フロー\n\nFlow\n\n### Cancel\n\nCancel body\n";
        let elements = extract(markdown);
        assert_eq!(
            elements
                .iter()
                .map(|element| element.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Edit", "Save", "Cancel"]
        );
        for heading in ["入力パラメータ", "ローカル変数", "処理フロー"] {
            assert!(elements[1].body.contains(heading));
        }
        assert!(!elements[1].body.contains("Cancel body"));
    }
}
