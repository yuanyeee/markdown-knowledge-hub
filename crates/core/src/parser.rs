use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub struct Parsed {
    pub title: String,
    pub text: String,
    pub links: Vec<String>,
    pub aliases: Vec<String>,
    pub headings: Vec<String>,
    pub okf: crate::okf::Record,
}

pub fn parse(raw: &str, fallback: &str) -> anyhow::Result<Parsed> {
    let normalized = raw.replace("\r\n", "\n");
    let (metadata, content, frontmatter_error) = split_frontmatter(&normalized)?;
    let mut okf = crate::okf::Record {
        concept: !crate::okf::reserved_path(fallback),
        error: frontmatter_error,
        ..crate::okf::Record::default()
    };
    if let Some(metadata) = &metadata {
        crate::okf::apply(&mut okf, metadata);
    }
    let aliases = match metadata.as_ref().map(read_aliases).transpose() {
        Ok(Some(aliases)) => aliases,
        Ok(None) => Vec::new(),
        Err(message) => {
            if okf.error.is_none() {
                okf.error = Some(message);
            }
            Vec::new()
        }
    };
    let mut out = Parsed {
        title: okf.title.clone().unwrap_or_default(),
        text: String::new(),
        links: vec![],
        aliases,
        headings: vec![],
        okf,
    };
    let mut heading = false;
    let mut first_heading = !out.title.is_empty();
    let mut code = false;
    let mut current_heading: Option<String> = None;
    let mut heading_counts = std::collections::HashMap::<String, usize>::new();
    static ANCHOR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = ANCHOR.get_or_init(|| {
        regex::Regex::new(
            r#"(?i)^\s*<a\s+(?:id|name)\s*=\s*["']([^"'<>\s]+)["']\s*>\s*(?:</a>\s*)?$"#,
        )
        .expect("constant anchor regex")
    });
    // Use the maintained parser extension, so fenced/inline code cannot create links.
    for event in Parser::new_ext(
        content,
        Options::ENABLE_WIKILINKS
            | Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_HEADING_ATTRIBUTES,
    ) {
        match event {
            Event::Html(html) | Event::InlineHtml(html) => {
                if let Some(capture) = pattern.captures(&html) {
                    out.headings.push(capture[1].to_string());
                }
            }
            Event::Start(Tag::Heading { id, .. }) => {
                current_heading = Some(String::new());
                if let Some(id) = id {
                    out.headings.push(id.to_string());
                }
                if !first_heading {
                    heading = true;
                    first_heading = true;
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                heading = false;
                if let Some(text) = current_heading.take() {
                    let slug = heading_id(&text);
                    let count = heading_counts.entry(slug.clone()).or_default();
                    out.headings.push(if *count == 0 {
                        slug.clone()
                    } else {
                        format!("{slug}-{count}")
                    });
                    *count += 1;
                    out.headings.push(text);
                }
            }
            Event::Start(Tag::CodeBlock(_)) => code = true,
            Event::End(TagEnd::CodeBlock) => code = false,
            Event::Start(Tag::Link { dest_url, .. }) if !code => {
                let target = dest_url.to_string();
                if !target.contains(':') && !target.starts_with("//") {
                    out.links.push(target);
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(text) = &mut current_heading {
                    text.push_str(&t);
                }
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
    Ok(out)
}

fn split_frontmatter(
    normalized: &str,
) -> anyhow::Result<(Option<serde_yaml_ng::Value>, &str, Option<String>)> {
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return Ok((None, normalized, None));
    };
    let Some((metadata, content)) = rest.split_once("\n---\n") else {
        return Ok((None, normalized, None));
    };
    anyhow::ensure!(
        metadata.len() <= 64 * 1024,
        "frontmatterは64KiB以下で指定してください"
    );
    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(metadata) {
        Ok(value) if matches!(value, serde_yaml_ng::Value::Mapping(_)) => {
            Ok((Some(value), content, None))
        }
        Ok(_) => Ok((
            None,
            content,
            Some("frontmatterはキーと値で指定してください".to_string()),
        )),
        Err(_) => Ok((
            None,
            content,
            Some("frontmatterを読み取れません".to_string()),
        )),
    }
}

fn read_aliases(metadata: &serde_yaml_ng::Value) -> Result<Vec<String>, String> {
    let Some(value) = metadata.get("aliases") else {
        return Ok(Vec::new());
    };
    if let Some(alias) = value.as_str() {
        return Ok(vec![alias.to_string()]);
    }
    let Some(values) = value.as_sequence() else {
        return Err("aliasesには文字列または配列を指定してください".to_string());
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "aliasesには文字列を指定してください".to_string())
        })
        .collect()
}

pub fn heading_id(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_whitespace() {
                Some('-')
            } else if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else {
                None
            }
        })
        .collect()
}
