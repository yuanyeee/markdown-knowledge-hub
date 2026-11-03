use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::borrow::Cow;

pub struct Element<'a> {
    pub kind: &'static str,
    pub title: String,
    pub line: usize,
    pub body: Cow<'a, str>,
    pub timer_action: Option<String>,
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

fn without_count(title: &str) -> &str {
    for (open, close) in [('(', ')'), ('（', '）')] {
        if let Some((name, count)) = title
            .strip_suffix(close)
            .and_then(|text| text.rsplit_once(open))
        {
            let count = count.trim();
            if !count.is_empty() && count.chars().all(|character| character.is_ascii_digit()) {
                return name.trim_end();
            }
        }
    }
    title
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
    let mut rest = false;
    let mut module_definitions = false;
    let mut elements = Vec::new();
    let newlines: Vec<_> = source
        .match_indices('\n')
        .map(|(offset, _)| offset)
        .collect();
    for (index, (level, offset, title)) in headings.iter().enumerate() {
        if *level == HeadingLevel::H1 {
            rest = title.ends_with(" REST / SOAP 連携");
        }
        if *level <= HeadingLevel::H2 {
            module_definitions = title == "その他のモジュール定義";
            section = title;
        }
        if detail_heading(title) {
            continue;
        }
        let prefixes = [
            ("画面:", "画面"),
            ("Web ブロック:", "Webブロック"),
            ("サービスアクション:", "サービスアクション"),
            ("タイマー:", "タイマー"),
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
                if rest && *level == HeadingLevel::H2 {
                    return [("REST", "REST"), ("SOAP", "SOAP"), ("連携", "REST / SOAP")]
                        .into_iter()
                        .find_map(|(protocol, kind)| {
                            let marker = format!(" ({protocol}");
                            let (name, suffix) = title.strip_suffix(')')?.rsplit_once(&marker)?;
                            matches!(suffix, "" | " (公開)" | " (消費)").then_some((kind, name))
                        });
                }
                if *level == HeadingLevel::H2 {
                    return match without_count(title) {
                        "サイトプロパティ" => Some(("サイトプロパティ", "一覧")),
                        "セッション変数" => Some(("セッション変数", "一覧")),
                        _ => None,
                    };
                }
                if *level != HeadingLevel::H3 {
                    return None;
                }
                if section.starts_with("エンティティ") {
                    Some(("Entity", title.as_str()))
                } else if section.starts_with("構造体") {
                    Some(("Structure", title.as_str()))
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
        let end = headings[index + 1..]
            .iter()
            .find(|(next, _, title)| next < level || (next == level && !detail_heading(title)))
            .map_or(source.len(), |(_, offset, _)| *offset);
        let definition_kind = match title.as_str() {
            "タイマー" => Some("タイマー"),
            "公開 Web サービス (SOAP)" => Some("公開SOAP"),
            _ => None,
        };
        if module_definitions
            && *level == HeadingLevel::H3
            && let Some(kind) = definition_kind
        {
            let markdown = &source[*offset..end];
            let mut cells = Vec::new();
            for (event, span) in
                Parser::new_ext(markdown, Options::ENABLE_TABLES).into_offset_iter()
            {
                match event {
                    Event::Start(Tag::TableRow) => cells.clear(),
                    Event::End(TagEnd::TableCell) => cells.push(markdown[span].trim()),
                    Event::End(TagEnd::TableRow) if cells.len() == 3 && !cells[0].is_empty() => {
                        let action = (kind == "タイマー")
                            .then(|| {
                                cells[2]
                                    .split("<br>")
                                    .find_map(|part| part.trim().strip_prefix("Action: "))
                            })
                            .flatten();
                        elements.push(Element {
                            kind,
                            title: cells[0].replace("\\", ""),
                            line: newlines
                                .partition_point(|position| *position < offset + span.start)
                                + 1,
                            body: Cow::Owned(format!(
                                "| 名前 | 説明 | 詳細 |\n|---|---|---|\n{}\n",
                                markdown[span].trim()
                            )),
                            timer_action: action.map(str::to_owned),
                            anchors: vec![],
                        });
                    }
                    _ => {}
                }
            }
        }
        let Some((kind, title)) = classified.filter(|(_, title)| !title.is_empty()) else {
            continue;
        };
        elements.push(Element {
            kind,
            title: title.to_owned(),
            line: newlines.partition_point(|position| position < offset) + 1,
            body: Cow::Borrowed(&source[*offset..end]),
            timer_action: None,
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

/// Extract declared types from the OML/XIF parameter and attribute tables.
pub fn declared_types(markdown: &str) -> std::collections::BTreeSet<String> {
    use pulldown_cmark::Options;
    let mut column = None;
    let mut cells = Vec::new();
    let mut cell = None;
    let mut types = std::collections::BTreeSet::new();
    for event in Parser::new_ext(markdown, Options::ENABLE_TABLES) {
        match event {
            Event::Start(Tag::Table(_)) => column = None,
            Event::Start(Tag::TableHead | Tag::TableRow) => cells.clear(),
            Event::Start(Tag::TableCell) => cell = Some(String::new()),
            Event::Text(text) | Event::Code(text) => {
                if let Some(value) = &mut cell {
                    value.push_str(&text);
                }
            }
            Event::End(TagEnd::TableCell) => {
                cells.push(cell.take().unwrap_or_default().trim().to_owned())
            }
            Event::End(TagEnd::TableHead)
                if cells.iter().any(|value| {
                    matches!(value.as_str(), "名前" | "属性" | "属性名" | "パラメータ名")
                }) =>
            {
                column = cells
                    .iter()
                    .position(|value| matches!(value.as_str(), "型" | "データ型"));
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(value) = column.and_then(|index| cells.get(index)) {
                    let name = value.strip_prefix("List of ").unwrap_or(value).trim();
                    if !name.is_empty() {
                        types.insert(name.to_owned());
                    }
                }
            }
            _ => {}
        }
    }
    types
}

pub fn route_fragments(
    markdown: &str,
    anchors: &std::collections::BTreeMap<String, Vec<usize>>,
    prefix: &str,
) -> String {
    route_links(markdown, |url| {
        let fragment = percent_encoding::percent_decode_str(url.strip_prefix('#')?)
            .decode_utf8()
            .ok()?;
        let [number] = anchors.get(fragment.as_ref())?.as_slice() else {
            return None;
        };
        Some(format!("{prefix}{number}.md"))
    })
}

pub fn route_links(markdown: &str, mut target: impl FnMut(&str) -> Option<String>) -> String {
    let mut result = String::new();
    let mut end = 0;
    for (event, span) in Parser::new_ext(markdown, Options::ENABLE_TABLES).into_offset_iter() {
        let Event::Start(Tag::Link { dest_url, .. }) = event else {
            continue;
        };
        let Some(target) = target(&dest_url) else {
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
        result.push_str(&target);
        result.push(')');
        end = span.end;
    }
    result.push_str(&markdown[end..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_actions_stay_in_the_screen_wiki() {
        let markdown = "# 画面: Edit\n\n## スクリーンアクション\n\n### Save\n\n### 入力パラメータ\n\nInput\n\n### 処理フロー\n\nFlow\n\n### Cancel\n\nCancel body\n\n# サーバーアクション: Store\n";
        let elements = extract(markdown);
        assert_eq!(
            elements
                .iter()
                .map(|element| element.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Edit", "Store"]
        );
        for heading in ["Save", "入力パラメータ", "処理フロー", "Cancel"] {
            assert!(elements[0].body.contains(heading));
        }
    }
}
