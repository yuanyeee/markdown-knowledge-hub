use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub struct Parsed {
    pub title: String,
    pub text: String,
    pub links: Vec<String>,
}

pub fn parse(raw: &str, fallback: &str) -> Parsed {
    let mut out = Parsed {
        title: String::new(),
        text: String::new(),
        links: vec![],
    };
    let mut heading = false;
    let mut first_heading = false;
    let mut code = false;
    // Use the maintained parser extension, so fenced/inline code cannot create links.
    for event in Parser::new_ext(
        raw,
        Options::ENABLE_WIKILINKS | Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
    ) {
        match event {
            Event::Start(Tag::Heading { .. }) if !first_heading => {
                heading = true;
                first_heading = true;
            }
            Event::End(TagEnd::Heading(_)) => heading = false,
            Event::Start(Tag::CodeBlock(_)) => code = true,
            Event::End(TagEnd::CodeBlock) => code = false,
            Event::Start(Tag::Link { dest_url, .. }) if !code => {
                let target = dest_url.to_string();
                if !target.contains(':') && !target.starts_with('#') && !target.starts_with("//") {
                    out.links.push(target);
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if heading {
                    out.title.push_str(&t);
                }
                out.text.push_str(&t);
                out.text.push(' ');
            }
            _ => {}
        }
    }
    if out.title.is_empty() {
        out.title = fallback.to_string();
    }
    out
}
