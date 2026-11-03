use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

pub struct Element<'a> {
    pub kind: &'static str,
    pub title: String,
    pub line: usize,
    pub body: &'a str,
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
        let Some((kind, title)) = classified.filter(|(_, title)| !title.is_empty()) else {
            continue;
        };
        let end = headings[index + 1..]
            .iter()
            .find(|(next, _, _)| next <= level)
            .map_or(source.len(), |(_, offset, _)| *offset);
        elements.push(Element {
            kind,
            title: title.to_owned(),
            line: newlines.partition_point(|position| position < offset) + 1,
            body: &source[*offset..end],
        });
    }
    elements
}
