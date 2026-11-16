//! Convert selected office and text files into an Open Knowledge Format v0.2 bundle.
//! Original files are only read. Generated Markdown is a separate Wiki input.

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_FILE: u64 = 20 * 1024 * 1024;
const MAX_ENTRY: u64 = 8 * 1024 * 1024;
const TEMPLATE_TYPES: [&str; 3] = ["文書", "表", "スライド"];
pub(crate) const CONVERTED_PREFIX: &str = "file-convert";

#[derive(Debug, Serialize)]
pub struct ConvertReport {
    pub pages: usize,
    pub paths: Vec<String>,
}

struct Page {
    path: String,
    title: String,
    markdown: String,
    source_name: String,
}

pub fn compile_file_wiki(files: &[PathBuf], destination: &Path) -> Result<ConvertReport> {
    ensure!(!destination.exists(), "生成先が既にあります");
    let pages = build_pages(files, &BTreeSet::new())?;
    fs::create_dir_all(destination)?;
    let result = (|| {
        write_pages(destination, &pages)?;
        write_index(destination, &pages)?;
        write_log(destination, &pages, true)?;
        fs::write(
            destination.join("file-wiki.json"),
            b"{\"format\":1,\"converter\":\"file-convert\"}\n",
        )?;
        Ok(ConvertReport {
            pages: pages.len(),
            paths: pages.iter().map(|page| page.path.clone()).collect(),
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

pub fn append_file_wiki(files: &[PathBuf], destination: &Path) -> Result<ConvertReport> {
    ensure!(
        destination.join("file-wiki.json").is_file(),
        "このフォルダーは変換Wikiではありません"
    );
    let mut used = existing_page_names(destination)?;
    let pages = build_pages(files, &used)?;
    for page in &pages {
        used.insert(page.path.clone());
    }
    write_pages(destination, &pages)?;
    let all = load_pages(destination)?;
    write_index(destination, &all)?;
    write_log(destination, &pages, false)?;
    Ok(ConvertReport {
        pages: pages.len(),
        paths: pages.iter().map(|page| page.path.clone()).collect(),
    })
}

pub fn validate_okf_bundle(root: &Path) -> Result<Vec<String>> {
    ensure!(root.is_dir(), "Wikiフォルダーを指定してください");
    let mut messages = Vec::new();
    let index_path = root.join("index.md");
    let index = fs::read_to_string(&index_path).unwrap_or_default();
    match frontmatter(&index) {
        Ok(Some(value)) => {
            match value.get("okf_version").and_then(|item| item.as_str()) {
                Some(version) if !version.trim().is_empty() => {}
                _ => messages.push("index.md に okf_version がありません".to_string()),
            }
            if value.get("type").is_some() {
                messages.push("index.md に type は置きません".to_string());
            }
        }
        Ok(None) => messages.push("index.md に frontmatter がありません".to_string()),
        Err(error) => messages.push(format!("index.md の frontmatter を読めません: {error}")),
    }
    let log_text = fs::read_to_string(root.join("log.md")).unwrap_or_default();
    if log_text.is_empty() {
        messages.push("log.md がありません".to_string());
    } else if let Ok(Some(value)) = frontmatter(&log_text)
        && value.get("type").is_some()
    {
        messages.push("log.md に type は置きません".to_string());
    }
    for line in log_text.lines() {
        if let Some(rest) = line.strip_prefix("- ")
            && !rest.contains("T")
        {
            messages.push(format!("log.md の記録に時刻がありません: {line}"));
        }
    }
    let mut linked = BTreeSet::new();
    for line in index.lines() {
        if let Some(start) = line.find("](pages/") {
            let rest = &line[start + 2..];
            if let Some(end) = rest.find(')') {
                linked.insert(rest[..end].to_string());
            }
        }
    }
    let pages_dir = root.join("pages");
    if pages_dir.is_dir() {
        for entry in fs::read_dir(&pages_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.ends_with(".md") {
                continue;
            }
            let rel = format!("pages/{name}");
            if !linked.contains(&rel) {
                messages.push(format!("{rel} が index.md からリンクされていません"));
            }
            let text = fs::read_to_string(entry.path()).unwrap_or_default();
            match frontmatter(&text) {
                Ok(Some(value)) => check_concept(&rel, &value, &mut messages),
                Ok(None) => messages.push(format!("{rel} に frontmatter がありません")),
                Err(error) => messages.push(format!("{rel} の frontmatter を読めません: {error}")),
            }
        }
    }
    for rel in &linked {
        if !root.join(rel).is_file() {
            messages.push(format!("index.md のリンク先がありません: {rel}"));
        }
    }
    Ok(messages)
}

fn check_concept(path: &str, value: &serde_yaml_ng::Value, messages: &mut Vec<String>) {
    let kind = value
        .get("type")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    if !TEMPLATE_TYPES.contains(&kind) {
        messages.push(format!(
            "{path} の type は 文書、表、スライド のいずれかです"
        ));
    }
    if let Some(mapping) = value.as_mapping() {
        for key in mapping.keys() {
            let name = key.as_str().unwrap_or("");
            if !matches!(
                name,
                "type" | "title" | "description" | "generated" | "sources"
            ) {
                messages.push(format!("{path} の {name} はテンプレートにありません"));
            }
        }
    }
    let generated_by = value
        .get("generated")
        .and_then(|item| item.get("by"))
        .and_then(|item| item.as_str())
        .unwrap_or("");
    if generated_by != "process:file-convert" {
        messages.push(format!(
            "{path} の generated.by は process:file-convert です"
        ));
    }
    let at = value
        .get("generated")
        .and_then(|item| item.get("at"))
        .and_then(|item| item.as_str())
        .unwrap_or("");
    if crate::okf::normalize_instant(at).is_none() {
        messages.push(format!("{path} の generated.at の日時を読めません"));
    }
}

fn frontmatter(text: &str) -> Result<Option<serde_yaml_ng::Value>, String> {
    let Some(rest) = text.strip_prefix("---\n") else {
        return Ok(None);
    };
    let Some(end) = rest.find("\n---") else {
        return Err("終端がありません".to_string());
    };
    serde_yaml_ng::from_str(&rest[..end]).map_err(|error| error.to_string())
}

impl crate::Hub {
    pub fn prepare_file_wiki(&self, id: &str, files: &[PathBuf], parent: &Path) -> Result<PathBuf> {
        let destination = self.generated_input_path(id, parent, "File-Wiki")?;
        compile_file_wiki(files, &destination)?;
        Ok(destination)
    }

    pub fn add_converted_files(&mut self, id: &str, files: &[PathBuf]) -> Result<ConvertReport> {
        let project = self
            .projects()?
            .into_iter()
            .find(|project| project.id == id)
            .context("プロジェクトが見つかりません")?;
        let root = PathBuf::from(project.root);
        if root.join("file-wiki.json").is_file() {
            let report = append_file_wiki(files, &root)?;
            self.scan(id)?;
            return Ok(report);
        }
        ensure!(
            !root.join(CONVERTED_PREFIX).exists(),
            "入力フォルダーに file-convert があるため、変換ページを追加できません"
        );
        let destination = self.dir(id)?.join("converted");
        let report = if destination.join("file-wiki.json").is_file() {
            append_file_wiki(files, &destination)?
        } else {
            ensure!(!destination.exists(), "変換ページの保存先が不正です");
            compile_file_wiki(files, &destination)?
        };
        self.scan(id)?;
        Ok(report)
    }
}

pub(crate) fn converted_markdown_relative(relative: &str) -> bool {
    match relative.split('/').collect::<Vec<_>>().as_slice() {
        ["index.md"] | ["log.md"] => true,
        ["pages", name] => valid_converted_page(name),
        _ => false,
    }
}

pub(crate) fn valid_converted_page(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    !stem.is_empty()
        && stem.chars().count() <= 120
        && stem
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_')
}

fn build_pages(files: &[PathBuf], used: &BTreeSet<String>) -> Result<Vec<Page>> {
    ensure!(!files.is_empty(), "変換するファイルを選択してください");
    let mut names = used.clone();
    let mut pages = Vec::new();
    let at = crate::okf::utc_now();
    for path in files {
        let path = path
            .canonicalize()
            .with_context(|| format!("ファイルが見つかりません: {}", path.display()))?;
        ensure!(path.is_file(), "ファイルを選択してください");
        let len = path.metadata()?.len();
        ensure!(len <= MAX_FILE, "20MBを超えるファイルは変換しません");
        let source_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("ファイル名を読めません")?
            .to_string();
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let (kind, body) = convert_file(&path, &extension)?;
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("page");
        let file_name = allocate_name(stem, &mut names);
        let title = first_heading(&body).unwrap_or_else(|| stem.to_string());
        let description = plain_excerpt(&body);
        let markdown = format!(
            "---\ntype: {}\ntitle: {}\ndescription: {}\ngenerated:\n  by: process:file-convert\n  at: {}\nsources:\n  - title: {}\n    id: {}\n---\n\n# {}\n\n{body}\n\n## 出典\n\n- {source_name}\n",
            yaml_quote(kind),
            yaml_quote(&title),
            yaml_quote(&description),
            yaml_quote(&at),
            yaml_quote(&source_name),
            yaml_quote(&source_name),
            title.trim()
        );
        pages.push(Page {
            path: format!("pages/{file_name}.md"),
            title,
            markdown,
            source_name,
        });
    }
    Ok(pages)
}

fn convert_file(path: &Path, extension: &str) -> Result<(&'static str, String)> {
    match extension {
        "txt" => Ok(("文書", text_body(&read_text(path)?)?)),
        "html" | "htm" => Ok(("文書", html_to_markdown(&read_text(path)?))),
        "csv" => Ok(("表", csv_to_markdown(&read_text(path)?))),
        "docx" => Ok(("文書", docx_to_markdown(path)?)),
        "xlsx" => Ok(("表", xlsx_to_markdown(path)?)),
        "pptx" => Ok(("スライド", pptx_to_markdown(path)?)),
        _ => bail!("変換できるのは docx、xlsx、pptx、html、txt、csv です"),
    }
}

fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    String::from_utf8(bytes).context("UTF-8のテキストを選択してください")
}

fn text_body(text: &str) -> Result<String> {
    let body = text.replace("\r\n", "\n").trim().to_string();
    ensure!(!body.is_empty(), "空のファイルは変換しません");
    Ok(body)
}

fn html_to_markdown(html: &str) -> String {
    let without = strip_blocks(html, "script");
    let without = strip_blocks(&without, "style");
    let mut out = String::new();
    let mut rest = without.as_str();
    let mut table: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut in_table = false;
    let mut in_cell = false;
    while let Some(start) = rest.find('<') {
        push_text(&mut cell, &rest[..start], in_cell, &mut out);
        rest = &rest[start + 1..];
        if let Some(stripped) = rest.strip_prefix("!--") {
            if let Some(end) = stripped.find("-->") {
                rest = &stripped[end + 3..];
            } else {
                break;
            }
            continue;
        }
        let end = rest.find('>').unwrap_or(rest.len());
        let raw = &rest[..end];
        rest = rest.get(end + 1..).unwrap_or("");
        let closing = raw.starts_with('/');
        let name = raw
            .trim_start_matches('/')
            .split(|ch: char| ch.is_whitespace() || ch == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if closing {
            match name.as_str() {
                "td" | "th" if in_cell => {
                    row.push(cell.trim().to_string());
                    cell.clear();
                    in_cell = false;
                }
                "tr" if in_table => {
                    if !row.is_empty() {
                        table.push(std::mem::take(&mut row));
                    }
                }
                "table" => {
                    out.push_str(&render_table(&table));
                    table.clear();
                    in_table = false;
                }
                "p" | "div" | "h1" | "h2" | "h3" | "h4" | "li" => out.push('\n'),
                _ => {}
            }
        } else {
            match name.as_str() {
                "table" => in_table = true,
                "tr" => row.clear(),
                "td" | "th" => in_cell = true,
                "br" => out.push('\n'),
                "h1" => out.push_str("\n# "),
                "h2" => out.push_str("\n## "),
                "h3" => out.push_str("\n### "),
                "li" => out.push_str("\n- "),
                _ => {}
            }
        }
    }
    if !rest.is_empty() {
        push_text(&mut cell, rest, in_cell, &mut out);
    }
    collapse_blank_lines(&decode_entities(&out))
}

fn push_text(cell: &mut String, text: &str, in_cell: bool, out: &mut String) {
    let decoded = decode_entities(text);
    if in_cell {
        cell.push_str(&decoded);
    } else if !decoded.trim().is_empty() {
        out.push_str(&decoded);
    }
}

fn strip_blocks(html: &str, tag: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    loop {
        let Some(start) = rest.to_ascii_lowercase().find(&open) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let after = &rest[start + open.len()..];
        let lower = after.to_ascii_lowercase();
        if let Some(end) = lower.find(&close) {
            let tail = &after[end + close.len()..];
            rest = tail.find('>').map(|pos| &tail[pos + 1..]).unwrap_or("");
        } else {
            break;
        }
    }
    out
}

fn csv_to_markdown(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let delimiter = sniff_delimiter(&text);
    let rows = parse_delimited(&text, delimiter);
    render_table(&rows)
}

fn sniff_delimiter(text: &str) -> char {
    let line = text.lines().next().unwrap_or("");
    let commas = line.matches(',').count();
    let tabs = line.matches('\t').count();
    let semis = line.matches(';').count();
    if tabs > commas && tabs >= semis {
        '\t'
    } else if semis > commas {
        ';'
    } else {
        ','
    }
}

fn parse_delimited(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(ch);
            }
        } else if ch == '"' && field.is_empty() {
            quoted = true;
        } else if ch == delimiter {
            row.push(std::mem::take(&mut field));
        } else if ch == '\n' {
            row.push(std::mem::take(&mut field));
            if row.iter().any(|cell| !cell.is_empty()) {
                rows.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        } else if ch != '\r' {
            field.push(ch);
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        if row.iter().any(|cell| !cell.is_empty()) {
            rows.push(row);
        }
    }
    rows
}

fn render_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let width = rows.iter().map(|row| row.len()).max().unwrap_or(0).min(32);
    let mut out = String::new();
    for (index, row) in rows.iter().take(2000).enumerate() {
        let cells: Vec<String> = (0..width)
            .map(|column| {
                row.get(column)
                    .map(|cell| cell.replace('|', "\\|").replace('\n', " "))
                    .unwrap_or_default()
            })
            .collect();
        out.push_str("| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |\n");
        if index == 0 {
            out.push('|');
            for _ in 0..width {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
    }
    out
}

fn docx_to_markdown(path: &Path) -> Result<String> {
    let xml = read_zip_text(path, "word/document.xml")?.context("文書本文が見つかりません")?;
    Ok(word_xml_to_markdown(&xml))
}

fn word_xml_to_markdown(xml: &str) -> String {
    let mut out = String::new();
    let mut paragraph = String::new();
    let mut cell = String::new();
    let mut row = Vec::new();
    let mut table = Vec::new();
    let mut in_table = 0i32;
    for event in xml_events(xml) {
        match event {
            XmlEvent::Text(text) => {
                if in_table > 0 {
                    cell.push_str(&text);
                } else {
                    paragraph.push_str(&text);
                }
            }
            XmlEvent::ParagraphEnd => {
                if in_table > 0 {
                    if !cell.is_empty() && !cell.ends_with(' ') {
                        cell.push(' ');
                    }
                } else if !paragraph.trim().is_empty() {
                    out.push_str(paragraph.trim());
                    out.push_str("\n\n");
                    paragraph.clear();
                } else {
                    paragraph.clear();
                }
            }
            XmlEvent::CellEnd => {
                row.push(cell.trim().to_string());
                cell.clear();
            }
            XmlEvent::RowEnd => {
                if row.iter().any(|item| !item.is_empty()) {
                    table.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            XmlEvent::TableStart => in_table += 1,
            XmlEvent::TableEnd => {
                in_table -= 1;
                out.push_str(&render_table(&table));
                out.push('\n');
                table.clear();
            }
        }
    }
    if !paragraph.trim().is_empty() {
        out.push_str(paragraph.trim());
        out.push('\n');
    }
    collapse_blank_lines(&out)
}

enum XmlEvent {
    Text(String),
    ParagraphEnd,
    CellEnd,
    RowEnd,
    TableStart,
    TableEnd,
}

fn xml_events(xml: &str) -> Vec<XmlEvent> {
    let mut events = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        rest = rest.get(end + 1..).unwrap_or("");
        let name = tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("");
        let local = name.rsplit(':').next().unwrap_or(name);
        let closing = tag.starts_with('/');
        if !closing && (local == "t" || local == "instrText") {
            if let Some(text_end) = rest.find('<') {
                events.push(XmlEvent::Text(xml_unescape(&rest[..text_end])));
            }
        } else if closing && local == "p" {
            events.push(XmlEvent::ParagraphEnd);
        } else if closing && local == "tc" {
            events.push(XmlEvent::CellEnd);
        } else if closing && local == "tr" {
            events.push(XmlEvent::RowEnd);
        } else if !closing && local == "tbl" {
            events.push(XmlEvent::TableStart);
        } else if closing && local == "tbl" {
            events.push(XmlEvent::TableEnd);
        }
    }
    events
}

fn xlsx_to_markdown(path: &Path) -> Result<String> {
    let shared = read_zip_text(path, "xl/sharedStrings.xml")?
        .map(|xml| shared_strings(&xml))
        .unwrap_or_default();
    let names = zip_names(path)?;
    let mut sheets: Vec<String> = names
        .into_iter()
        .filter(|name| name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml"))
        .collect();
    sheets.sort();
    ensure!(!sheets.is_empty(), "表が見つかりません");
    let mut out = String::new();
    for (index, name) in sheets.iter().enumerate() {
        let xml = read_zip_text(path, name)?.unwrap_or_default();
        out.push_str(&format!("## Sheet {}\n\n", index + 1));
        out.push_str(&render_table(&sheet_rows(&xml, &shared)));
        out.push('\n');
    }
    Ok(out)
}

fn shared_strings(xml: &str) -> Vec<String> {
    let mut strings = Vec::new();
    for item in xml.split("<si").skip(1) {
        let block = item.split("</si>").next().unwrap_or("");
        let mut text = String::new();
        for piece in block.split("<t").skip(1) {
            let after = piece.split_once('>').map(|(_, rest)| rest).unwrap_or("");
            let raw = after.split("</t>").next().unwrap_or("");
            text.push_str(&xml_unescape(raw));
        }
        strings.push(text);
    }
    strings
}

fn sheet_rows(xml: &str, shared: &[String]) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for row_xml in xml.split("<row").skip(1) {
        let block = row_xml.split("</row>").next().unwrap_or("");
        let mut cells = Vec::new();
        for cell in block.split("<c ").skip(1) {
            let head = cell.split('>').next().unwrap_or("");
            let column = head
                .split("r=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .map(column_index)
                .unwrap_or(cells.len());
            let inline = head.contains("t=\"inlineStr\"") || head.contains("t=\"str\"");
            let shared_ref = head.contains("t=\"s\"");
            let value = if inline {
                cell.split("<t")
                    .nth(1)
                    .and_then(|piece| piece.split_once('>'))
                    .map(|(_, rest)| rest.split("</t>").next().unwrap_or(""))
                    .map(xml_unescape)
                    .unwrap_or_default()
            } else {
                let raw = cell
                    .split("<v>")
                    .nth(1)
                    .and_then(|piece| piece.split("</v>").next())
                    .unwrap_or("");
                if shared_ref {
                    raw.parse::<usize>()
                        .ok()
                        .and_then(|index| shared.get(index))
                        .cloned()
                        .unwrap_or_default()
                } else {
                    xml_unescape(raw)
                }
            };
            while cells.len() < column {
                cells.push(String::new());
            }
            if cells.len() == column {
                cells.push(value);
            }
        }
        if cells.iter().any(|cell| !cell.is_empty()) {
            rows.push(cells);
        }
    }
    rows
}

fn column_index(reference: &str) -> usize {
    reference
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .fold(0usize, |total, ch| {
            total * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize + 1)
        })
        .saturating_sub(1)
}

fn pptx_to_markdown(path: &Path) -> Result<String> {
    let mut slides: Vec<String> = zip_names(path)?
        .into_iter()
        .filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        .collect();
    slides.sort_by_key(|name| {
        name.trim_start_matches("ppt/slides/slide")
            .trim_end_matches(".xml")
            .parse::<u32>()
            .unwrap_or(0)
    });
    ensure!(!slides.is_empty(), "スライドが見つかりません");
    let mut out = String::new();
    for (index, name) in slides.iter().enumerate() {
        let xml = read_zip_text(path, name)?.unwrap_or_default();
        out.push_str(&format!("## スライド {}\n\n", index + 1));
        let text = xml
            .split("<a:t")
            .skip(1)
            .filter_map(|piece| piece.split_once('>'))
            .map(|(_, rest)| xml_unescape(rest.split("</a:t>").next().unwrap_or("")))
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        out.push_str(&text);
        out.push_str("\n\n");
    }
    Ok(out)
}

fn zip_names(path: &Path) -> Result<Vec<String>> {
    let file = fs::File::open(path)?;
    let archive = zip::ZipArchive::new(file).context("Officeファイルを開けません")?;
    Ok(archive
        .file_names()
        .map(|name| name.replace('\\', "/"))
        .collect())
}

fn read_zip_text(path: &Path, name: &str) -> Result<Option<String>> {
    let file = fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file).context("Officeファイルを開けません")?;
    let index = match archive
        .file_names()
        .position(|entry| entry.replace('\\', "/") == name)
    {
        Some(index) => index,
        None => return Ok(None),
    };
    let entry = archive.by_index(index)?;
    ensure!(entry.size() <= MAX_ENTRY, "展開後の本文が大きすぎます");
    let mut text = String::new();
    entry
        .take(MAX_ENTRY)
        .read_to_string(&mut text)
        .context("本文の文字コードを読めません")?;
    Ok(Some(text))
}

fn write_pages(destination: &Path, pages: &[Page]) -> Result<()> {
    let directory = destination.join("pages");
    fs::create_dir_all(&directory)?;
    for page in pages {
        let target = destination.join(&page.path);
        ensure!(!target.exists(), "同じ名前のページが既にあります");
        fs::write(target, &page.markdown)?;
    }
    Ok(())
}

fn write_index(destination: &Path, pages: &[Page]) -> Result<()> {
    let mut body = String::from("---\nokf_version: \"0.2\"\n---\n\n# Wiki\n\n## ページ\n\n");
    for page in pages {
        body.push_str(&format!(
            "- [{}]({})\n",
            page.title.replace(']', ""),
            page.path
        ));
    }
    fs::write(destination.join("index.md"), body)?;
    Ok(())
}

fn write_log(destination: &Path, pages: &[Page], fresh: bool) -> Result<()> {
    let path = destination.join("log.md");
    if fresh || !path.exists() {
        fs::write(&path, "---\nokf_version: \"0.2\"\n---\n\n# 記録\n\n")?;
    }
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    let at = crate::okf::utc_now();
    for page in pages {
        writeln!(file, "- {at} {} を {} へ変換", page.source_name, page.path)?;
    }
    Ok(())
}

fn existing_page_names(destination: &Path) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let directory = destination.join("pages");
    if directory.is_dir() {
        for entry in fs::read_dir(directory)? {
            let name = entry?.file_name();
            names.insert(format!("pages/{}", name.to_string_lossy()));
        }
    }
    Ok(names)
}

fn load_pages(destination: &Path) -> Result<Vec<Page>> {
    let mut pages = Vec::new();
    let directory = destination.join("pages");
    let mut paths: Vec<PathBuf> = fs::read_dir(&directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();
    for path in paths {
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            let markdown = fs::read_to_string(&path)?;
            let title = frontmatter(&markdown)
                .ok()
                .flatten()
                .and_then(|value| {
                    value
                        .get("title")
                        .and_then(|item| item.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("page")
                        .to_string()
                });
            pages.push(Page {
                path: format!(
                    "pages/{}",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("page.md")
                ),
                title,
                markdown,
                source_name: String::new(),
            });
        }
    }
    Ok(pages)
}

fn allocate_name(stem: &str, used: &mut BTreeSet<String>) -> String {
    let mut clean: String = stem
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    clean = clean.trim_matches('-').chars().take(80).collect();
    if clean.is_empty() {
        clean = "page".to_string();
    }
    let mut candidate = clean.clone();
    let mut suffix = 2u32;
    while used.contains(&format!("pages/{candidate}.md")) {
        candidate = format!("{clean}-{suffix}");
        suffix += 1;
    }
    used.insert(format!("pages/{candidate}.md"));
    candidate
}

fn first_heading(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| {
        let trimmed = line.trim();
        let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=6).contains(&hashes)
            && trimmed
                .as_bytes()
                .get(hashes)
                .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            let text = trimmed[hashes..].trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        } else {
            None
        }
    })
}

fn plain_excerpt(markdown: &str) -> String {
    let mut text = String::new();
    for line in markdown.lines() {
        let trimmed = line.trim().trim_start_matches('#').trim();
        if trimmed.is_empty() || trimmed.starts_with('|') {
            continue;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(trimmed);
        if text.chars().count() >= 160 {
            break;
        }
    }
    text.chars().take(160).collect()
}

fn yaml_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn xml_unescape(text: &str) -> String {
    decode_entities(text)
}

fn decode_entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + 1..];
        if let Some(end) = rest.find(';') {
            let token = &rest[..end];
            rest = &rest[end + 1..];
            match token {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ if let Some(code) = token.strip_prefix('#') => {
                    let code = code.strip_prefix('x').map_or_else(
                        || code.parse::<u32>().ok(),
                        |hex| u32::from_str_radix(hex, 16).ok(),
                    );
                    if let Some(ch) = code.and_then(char::from_u32) {
                        out.push(ch);
                    }
                }
                _ => {
                    out.push('&');
                    out.push_str(token);
                    out.push(';');
                }
            }
        } else {
            out.push('&');
            out.push_str(rest);
            return out;
        }
    }
    out.push_str(rest);
    out
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank <= 1 && !out.is_empty() {
                out.push('\n');
            }
        } else {
            blank = 0;
            out.push_str(line.trim());
            out.push('\n');
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn write_office(path: &Path, entries: &[(&str, &str)]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, body) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn converts_text_table_and_office_into_an_okf_bundle() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input");
        fs::create_dir(&input).unwrap();
        fs::write(input.join("notes.txt"), "# メモ\n\n今日の記録です。\n").unwrap();
        fs::write(input.join("rows.csv"), "名前,値\nAlpha,1\n").unwrap();
        fs::write(
            input.join("page.html"),
            "<html><style>.x{}</style><script>alert(1)</script><h1>画面</h1><p>本文 &amp; 続き</p><table><tr><th>列</th></tr><tr><td>値</td></tr></table></html>",
        )
        .unwrap();
        write_office(
            &input.join("brief.docx"),
            &[(
                "word/document.xml",
                r#"<w:document><w:body><w:p><w:r><w:t>決定事項</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>項目</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
            )],
        );
        write_office(
            &input.join("grid.xlsx"),
            &[
                (
                    "xl/sharedStrings.xml",
                    r#"<sst><si><t>見出し</t></si><si><t>数値</t></si></sst>"#,
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<worksheet><sheetData><row><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row><c r="A2"><v>10</v></c></row></sheetData></worksheet>"#,
                ),
            ],
        );
        write_office(
            &input.join("deck.pptx"),
            &[(
                "ppt/slides/slide1.xml",
                r#"<p:sld><a:t>表紙</a:t><a:t>要点</a:t></p:sld>"#,
            )],
        );
        let originals: Vec<Vec<u8>> = [
            "notes.txt",
            "rows.csv",
            "page.html",
            "brief.docx",
            "grid.xlsx",
            "deck.pptx",
        ]
        .iter()
        .map(|name| fs::read(input.join(name)).unwrap())
        .collect();
        let wiki = temp.path().join("wiki");
        let report = compile_file_wiki(
            &[
                input.join("notes.txt"),
                input.join("rows.csv"),
                input.join("page.html"),
                input.join("brief.docx"),
                input.join("grid.xlsx"),
                input.join("deck.pptx"),
            ],
            &wiki,
        )
        .unwrap();
        assert_eq!(report.pages, 6);
        assert!(
            validate_okf_bundle(&wiki).unwrap().is_empty(),
            "{:?}",
            validate_okf_bundle(&wiki)
        );
        let notes = fs::read_to_string(wiki.join("pages/notes.md")).unwrap();
        assert!(notes.contains("type: \"文書\""));
        assert!(notes.contains("process:file-convert"));
        assert!(notes.contains("今日の記録です。"));
        let table = fs::read_to_string(wiki.join("pages/rows.md")).unwrap();
        assert!(table.contains("type: \"表\""));
        assert!(table.contains("| Alpha |"));
        let html = fs::read_to_string(wiki.join("pages/page.md")).unwrap();
        assert!(html.contains("本文 & 続き"));
        assert!(!html.contains("alert"));
        assert!(
            fs::read_to_string(wiki.join("pages/brief.md"))
                .unwrap()
                .contains("決定事項")
        );
        assert!(
            fs::read_to_string(wiki.join("pages/grid.md"))
                .unwrap()
                .contains("見出し")
        );
        assert!(
            fs::read_to_string(wiki.join("pages/deck.md"))
                .unwrap()
                .contains("type: \"スライド\"")
        );
        assert!(
            fs::read_to_string(wiki.join("index.md"))
                .unwrap()
                .contains("okf_version: \"0.2\"")
        );
        for (name, bytes) in [
            "notes.txt",
            "rows.csv",
            "page.html",
            "brief.docx",
            "grid.xlsx",
            "deck.pptx",
        ]
        .iter()
        .zip(originals)
        {
            assert_eq!(fs::read(input.join(name)).unwrap(), bytes);
        }
        let extra = input.join("more.txt");
        fs::write(&extra, "追加ページ\n").unwrap();
        append_file_wiki(&[extra], &wiki).unwrap();
        assert!(validate_okf_bundle(&wiki).unwrap().is_empty());
        assert!(
            fs::read_to_string(wiki.join("log.md"))
                .unwrap()
                .contains("more.txt")
        );
        let mut hub = crate::Hub::open(&temp.path().join("settings")).unwrap();
        hub.register("demo", "Demo", &wiki).unwrap();
        hub.scan("demo").unwrap();
        assert!(
            hub.search("demo", "今日の記録")
                .unwrap()
                .iter()
                .any(|hit| hit.path == "pages/notes.md")
        );
        let graph = hub.graph("demo", None, 20).unwrap();
        assert!(
            graph
                .nodes
                .iter()
                .all(|node| node.path != "index.md" && node.path != "log.md")
        );
        let broken = wiki.join("pages/notes.md");
        fs::write(&broken, "---\ntype: \"別\"\n---\n# 別\n").unwrap();
        let messages = validate_okf_bundle(&wiki).unwrap();
        assert!(messages.iter().any(|message| message.contains("type")));
    }

    #[test]
    fn adds_pages_to_an_existing_project_without_changing_its_markdown() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("notes");
        fs::create_dir(&source)?;
        let original = source.join("keep.md");
        fs::write(&original, "# Keep\n\n既存の本文\n")?;
        let bytes = fs::read(&original)?;
        let mut hub = crate::Hub::open(&temp.path().join("settings"))?;
        hub.register("demo", "Demo", &source)?;
        hub.scan("demo")?;
        let extra = temp.path().join("extra.txt");
        fs::write(&extra, "変換された記録\n")?;
        let report = hub.add_converted_files("demo", &[extra])?;
        assert_eq!(report.pages, 1);
        assert_eq!(fs::read(&original)?, bytes);
        assert!(!source.join("converted").exists());
        assert!(!source.join(CONVERTED_PREFIX).exists());
        let page = "file-convert/pages/extra.md";
        let document = hub.document("demo", page)?;
        assert!(document.markdown.contains("type: \"文書\""));
        assert!(document.markdown.contains("変換された記録"));
        assert!(
            hub.search("demo", "既存の本文")?
                .iter()
                .any(|hit| hit.path == "keep.md")
        );
        hub.rebuild("demo")?;
        assert!(!hub.document("demo", page)?.title.is_empty());
        assert!(
            hub.document("demo", page)?
                .markdown
                .contains("変換された記録")
        );
        let more = temp.path().join("more.txt");
        fs::write(&more, "二枚目\n")?;
        hub.add_converted_files("demo", &[more])?;
        assert!(
            hub.document("demo", "file-convert/pages/more.md")?
                .markdown
                .contains("二枚目")
        );
        assert!(
            hub.document("demo", page)?
                .markdown
                .contains("変換された記録")
        );
        fs::write(&original, "# Keep\n\n既存の本文を更新\n")?;
        hub.scan_paths("demo", &["keep.md".into()])?;
        assert!(
            hub.document("demo", "keep.md")?
                .markdown
                .contains("既存の本文を更新")
        );
        assert!(
            hub.document("demo", page)?
                .markdown
                .contains("変換された記録")
        );
        let moved = temp.path().join("moved");
        fs::create_dir(&moved)?;
        hub.move_output("demo", &moved)?;
        hub.rebuild("demo")?;
        assert!(
            hub.document("demo", page)?
                .markdown
                .contains("変換された記録")
        );
        assert!(
            hub.document("demo", "file-convert/pages/more.md")?
                .markdown
                .contains("二枚目")
        );
        assert!(fs::read_to_string(&original)?.contains("既存の本文を更新"));
        let output = hub.dir("demo")?;
        crate::outputs::validate_tree(&output)?;
        fs::write(output.join("converted").join("stray.txt"), "x")?;
        assert!(crate::outputs::validate_tree(&output).is_err());
        let blocked = temp.path().join("blocked");
        fs::create_dir_all(blocked.join(CONVERTED_PREFIX))?;
        fs::write(blocked.join("keep.md"), "# Keep\n")?;
        hub.register("other", "Other", &blocked)?;
        let rejected = temp.path().join("rejected.txt");
        fs::write(&rejected, "入らない\n")?;
        assert!(hub.add_converted_files("other", &[rejected]).is_err());
        Ok(())
    }
}
