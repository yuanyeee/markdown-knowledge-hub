//! Convert selected office and text files into an Open Knowledge Format v0.2 bundle.
//! Original files are only read. Generated Markdown is a separate Wiki input.

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_FILE: u64 = 20 * 1024 * 1024;
const MAX_ENTRY: u64 = 8 * 1024 * 1024;
const MAX_TABLE_CHARS: usize = 1_000_000;
const TEMPLATE_TYPES: [&str; 4] = ["文書", "表", "スライド", "用語"];
const PREVIEW_ROWS: usize = 5;
const LARGE_TABLE_ROWS: usize = 20;
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
    /// 0 = source page, 1 = summary, 2 = term. Index lists them in this order.
    rank: u8,
    log_line: String,
}

struct Built {
    created: Vec<Page>,
    updated: Vec<Page>,
}

struct TermLink {
    stem: String,
    wikilink: bool,
}

pub fn compile_file_wiki(files: &[PathBuf], destination: &Path) -> Result<ConvertReport> {
    ensure!(!destination.exists(), "生成先が既にあります");
    let built = build_pages(files, None, &BTreeSet::new())?;
    fs::create_dir_all(destination)?;
    let result = (|| {
        write_pages(destination, &built.created)?;
        write_index(destination, &built.created)?;
        write_log(destination, &built.created, true)?;
        fs::write(
            destination.join("file-wiki.json"),
            b"{\"format\":1,\"converter\":\"file-convert\"}\n",
        )?;
        Ok(ConvertReport {
            pages: built.created.len(),
            paths: built.created.iter().map(|page| page.path.clone()).collect(),
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
    let used = existing_page_names(destination)?;
    let built = build_pages(files, Some(destination), &used)?;
    write_pages(destination, &built.created)?;
    overwrite_pages(destination, &built.updated)?;
    let all = load_pages(destination)?;
    write_index(destination, &all)?;
    write_log(destination, &built.created, false)?;
    Ok(ConvertReport {
        pages: built.created.len(),
        paths: built.created.iter().map(|page| page.path.clone()).collect(),
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
            "{path} の type は 文書、表、スライド、用語 のいずれかです"
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
    if !matches!(
        generated_by,
        "process:file-convert" | "process:wiki-compile"
    ) {
        messages.push(format!(
            "{path} の generated.by は process:file-convert または process:wiki-compile です"
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

struct SourceDoc {
    source_name: String,
    stem: String,
    kind: &'static str,
    preamble: String,
    units: Vec<Unit>,
}

struct Unit {
    title: String,
    body: UnitBody,
}

enum UnitBody {
    Prose(String),
    Table {
        rows: Vec<Vec<String>>,
        truncated: bool,
    },
}

struct StoredPage {
    path: String,
    stem: String,
    title: String,
    markdown: String,
    kind: String,
    generated_by: String,
}

fn build_pages(
    files: &[PathBuf],
    existing_dir: Option<&Path>,
    used: &BTreeSet<String>,
) -> Result<Built> {
    ensure!(!files.is_empty(), "変換するファイルを選択してください");
    let mut names = used.clone();
    let at = crate::okf::utc_now();
    let mut docs = Vec::new();
    for path in files {
        docs.push(extract_source(path).with_context(|| {
            format!(
                "{} を変換できません",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file")
            )
        })?);
    }
    let stored = existing_dir
        .map(read_stored_pages)
        .transpose()?
        .unwrap_or_default();
    let mut counts = BTreeMap::new();
    for page in stored
        .iter()
        .filter(|page| page.generated_by == "process:file-convert" && page.kind != "用語")
    {
        collect_text_keys(&page.markdown, &mut counts);
    }
    for doc in &docs {
        for unit in &doc.units {
            count_unit(unit, &mut counts);
        }
    }
    let terms = select_terms(&counts);
    let mut term_stems = BTreeMap::new();
    for page in stored.iter().filter(|page| page.kind == "用語") {
        term_stems.insert(page.title.clone(), page.stem.clone());
    }
    let mut plans = Vec::new();
    for doc in &docs {
        let slots = allocate_doc(doc, &mut names);
        let summary_stem = allocate_name(&format!("{}--summary", doc.stem), &mut names);
        plans.push((slots, summary_stem));
    }
    let links = term_links(&terms, &mut names, &term_stems);

    let mut created = Vec::new();
    let mut updated = Vec::new();
    let mut mentions: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    for (doc, (slots, summary_stem)) in docs.iter().zip(plans) {
        for slot in &slots {
            let body = render_slot(doc, slot, &slots, &links);
            remember_mentions(&body, &links, &slot.stem, &slot.title, &mut mentions);
            created.push(make_page(
                &slot.stem,
                &slot.title,
                doc.kind,
                "process:file-convert",
                &at,
                &doc.source_name,
                &body,
                0,
                &format!("{} を pages/{}.md へ変換", doc.source_name, slot.stem),
            ));
        }
        let summary_title = if slots.len() == 1 {
            format!("{} の概要", slots[0].title)
        } else {
            format!("{} の概要", doc.stem)
        };
        created.push(make_page(
            &summary_stem,
            &summary_title,
            doc.kind,
            "process:wiki-compile",
            &at,
            &doc.source_name,
            &summary_body(doc, &slots),
            1,
            &format!(
                "{} の概要を pages/{summary_stem}.md へ作成",
                doc.source_name
            ),
        ));
    }

    for page in &stored {
        if page.generated_by != "process:file-convert" || page.kind == "用語" {
            continue;
        }
        let linked = link_markdown_tables(&page.markdown, &links);
        remember_mentions(&linked, &links, &page.stem, &page.title, &mut mentions);
        if linked != page.markdown {
            updated.push(Page {
                path: page.path.clone(),
                title: page.title.clone(),
                markdown: linked,
                rank: 0,
                log_line: format!("用語リンクを {} で更新", page.path),
            });
        }
    }

    for term in &terms {
        let Some(link) = links.get(term) else {
            continue;
        };
        let mut listed: Vec<(String, String)> = mentions
            .get(term)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        listed.sort();
        if listed.is_empty() {
            continue;
        }
        let sources = listed
            .iter()
            .map(|(_, title)| title.clone())
            .take(8)
            .collect::<Vec<_>>();
        let page = Page {
            path: format!("pages/{}.md", link.stem),
            title: term.clone(),
            markdown: page_markdown(
                "用語",
                term,
                &format!("{} 件のページに出る語", listed.len()),
                "process:file-convert",
                &at,
                &sources,
                &term_body(&listed),
            ),
            rank: 2,
            log_line: format!("用語 {term} を pages/{}.md へ作成", link.stem),
        };
        if stored.iter().any(|stored| stored.stem == link.stem) {
            updated.push(page);
        } else {
            created.push(page);
        }
    }
    Ok(Built { created, updated })
}

struct Slot {
    stem: String,
    title: String,
    /// `None` is the parent page that only lists the parts.
    unit: Option<usize>,
}

fn extract_source(path: &Path) -> Result<SourceDoc> {
    let path = path
        .canonicalize()
        .with_context(|| format!("ファイルが見つかりません: {}", path.display()))?;
    ensure!(path.is_file(), "ファイルを選択してください");
    let source_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("ファイル名を読めません")?
        .to_string();
    let len = path.metadata()?.len();
    ensure!(
        len <= MAX_FILE,
        "{source_name} は20MBを超えるため変換しません"
    );
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("page")
        .to_string();
    let (kind, preamble, units) = match extension.as_str() {
        "txt" => prose_source("文書", &text_body(&read_text(&path)?)?, &stem),
        "html" | "htm" => prose_source("文書", &html_to_markdown(&read_text(&path)?), &stem),
        "csv" => {
            let text = read_text(&path)?.replace("\r\n", "\n");
            let rows = parse_delimited(&text, sniff_delimiter(&text));
            (
                "表",
                String::new(),
                vec![Unit {
                    title: stem.clone(),
                    body: UnitBody::Table {
                        rows,
                        truncated: false,
                    },
                }],
            )
        }
        "docx" => prose_source("文書", &docx_to_markdown(&path)?, &stem),
        "xlsx" => {
            let sheets = read_sheets(&path)?;
            (
                "表",
                String::new(),
                sheets
                    .into_iter()
                    .map(|sheet| Unit {
                        title: sheet.name,
                        body: UnitBody::Table {
                            rows: sheet.rows,
                            truncated: sheet.truncated,
                        },
                    })
                    .collect(),
            )
        }
        "pptx" => ("スライド", String::new(), pptx_units(&path)?),
        _ => bail!("変換できるのは docx、xlsx、pptx、html、txt、csv です"),
    };
    ensure!(!units.is_empty(), "{source_name} からページを作れません");
    Ok(SourceDoc {
        source_name,
        stem,
        kind,
        preamble,
        units,
    })
}

fn prose_source(
    kind: &'static str,
    markdown: &str,
    stem: &str,
) -> (&'static str, String, Vec<Unit>) {
    if let Some((preamble, units)) = split_units(markdown) {
        return (kind, preamble, units);
    }
    let title = first_heading(markdown).unwrap_or_else(|| stem.to_string());
    (
        kind,
        String::new(),
        vec![Unit {
            title,
            body: UnitBody::Prose(markdown.trim().to_string()),
        }],
    )
}

fn split_units(markdown: &str) -> Option<(String, Vec<Unit>)> {
    let mut preamble = String::new();
    let mut sections = Vec::new();
    let mut title: Option<String> = None;
    let mut buf = String::new();
    for line in markdown.lines() {
        if let Some(heading) = heading_text(line, 2) {
            if let Some(previous) = title.take() {
                sections.push(Unit {
                    title: previous,
                    body: UnitBody::Prose(buf.trim().to_string()),
                });
            } else {
                preamble = buf.trim().to_string();
            }
            buf.clear();
            title = Some(heading);
        } else {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if let Some(previous) = title.take() {
        sections.push(Unit {
            title: previous,
            body: UnitBody::Prose(buf.trim().to_string()),
        });
    }
    (sections.len() >= 2).then_some((preamble, sections))
}

fn allocate_doc(doc: &SourceDoc, names: &mut BTreeSet<String>) -> Vec<Slot> {
    if doc.units.len() == 1 {
        return vec![Slot {
            stem: allocate_name(&doc.stem, names),
            title: doc.units[0].title.clone(),
            unit: Some(0),
        }];
    }
    let parent = allocate_name(&doc.stem, names);
    let mut slots = vec![Slot {
        stem: parent,
        title: doc.stem.clone(),
        unit: None,
    }];
    for (index, unit) in doc.units.iter().enumerate() {
        slots.push(Slot {
            stem: allocate_name(&format!("{}--{}", doc.stem, unit.title), names),
            title: unit.title.clone(),
            unit: Some(index),
        });
    }
    slots
}

fn render_slot(
    doc: &SourceDoc,
    slot: &Slot,
    slots: &[Slot],
    links: &BTreeMap<String, TermLink>,
) -> String {
    let Some(index) = slot.unit else {
        let mut body = match doc.kind {
            "表" => format!("シートは {} 件です。\n\n", doc.units.len()),
            "スライド" => format!("スライドは {} 枚です。\n\n", doc.units.len()),
            _ => format!("見出しは {} 件です。\n\n", doc.units.len()),
        };
        if !doc.preamble.is_empty() {
            body.push_str(&doc.preamble);
            body.push_str("\n\n");
        }
        for child in slots.iter().filter(|item| item.unit.is_some()) {
            body.push_str(&format!(
                "- [{}]({}.md)\n",
                link_label(&child.title),
                child.stem
            ));
        }
        return body;
    };
    let unit = &doc.units[index];
    let rendered = match &unit.body {
        UnitBody::Prose(text) => link_markdown_tables(text, links),
        UnitBody::Table { rows, truncated } => render_sheet(rows, *truncated, links),
    };
    if slots.len() == 1 {
        return rendered;
    }
    let parent = &slots[0];
    format!(
        "[{}]({}.md)\n\n{rendered}",
        link_label(&format!("← {}", parent.title)),
        parent.stem
    )
}

fn summary_body(doc: &SourceDoc, slots: &[Slot]) -> String {
    let mut text = match doc.kind {
        "表" => {
            let mut line = format!("表は {} 件です。", doc.units.len());
            for unit in doc.units.iter().take(3) {
                if let UnitBody::Table { rows, truncated } = &unit.body {
                    let headers = row_cells(rows.first());
                    let sample = rows
                        .iter()
                        .skip(1)
                        .flat_map(|row| row.iter())
                        .filter(|cell| !cell.trim().is_empty())
                        .take(4)
                        .cloned()
                        .collect::<Vec<_>>();
                    let columns = if headers.is_empty() {
                        "（列名なし）".to_string()
                    } else {
                        headers.join("、")
                    };
                    line.push_str(&format!(
                        "「{}」の列は {} で、データは {} 行です。",
                        unit.title,
                        columns,
                        rows.len().saturating_sub(1)
                    ));
                    if !sample.is_empty() {
                        line.push_str(&format!("先頭は {} です。", sample.join("、")));
                    }
                    if *truncated {
                        line.push_str("続きは出典の表に残しています。");
                    }
                }
            }
            line
        }
        "スライド" => {
            let titles = doc
                .units
                .iter()
                .take(6)
                .map(|unit| unit.title.as_str())
                .collect::<Vec<_>>()
                .join("、");
            format!(
                "スライドは {} 枚です。見出しは {titles} です。",
                doc.units.len()
            )
        }
        _ => {
            let titles = doc
                .units
                .iter()
                .take(6)
                .map(|unit| unit.title.as_str())
                .collect::<Vec<_>>()
                .join("、");
            let opening = doc
                .units
                .iter()
                .find_map(|unit| match &unit.body {
                    UnitBody::Prose(text) => {
                        let excerpt = opening_sentence(text);
                        (!excerpt.is_empty()).then_some(excerpt)
                    }
                    UnitBody::Table { .. } => None,
                })
                .or_else(|| (!doc.preamble.is_empty()).then(|| opening_sentence(&doc.preamble)))
                .unwrap_or_default();
            let mut line = format!("見出しは {titles} です。");
            if !opening.is_empty() {
                line.push_str(&format!("冒頭は {opening} です。"));
            }
            line
        }
    };
    text.push_str("\n\n## ページ\n\n");
    for slot in slots {
        text.push_str(&format!(
            "- [{}]({}.md)\n",
            link_label(&slot.title),
            slot.stem
        ));
    }
    text
}

fn render_sheet(
    rows: &[Vec<String>],
    truncated: bool,
    links: &BTreeMap<String, TermLink>,
) -> String {
    let headers = row_cells(rows.first());
    let data_rows = rows.len().saturating_sub(1);
    let mut out = String::new();
    if headers.is_empty() {
        out.push_str(&format!("データ行: {data_rows}。\n\n"));
    } else {
        out.push_str(&format!(
            "列: {}。データ行: {data_rows}。\n\n",
            headers.join("、")
        ));
    }
    let large = truncated || data_rows > LARGE_TABLE_ROWS;
    if large {
        let preview = rows
            .iter()
            .take(1 + PREVIEW_ROWS)
            .cloned()
            .collect::<Vec<_>>();
        out.push_str("先頭の行だけを示します。\n\n");
        out.push_str(&render_table_capped(&preview, usize::MAX, links).0);
        out.push_str("\n\n## 出典の表\n\n");
    }
    let (table, cut) = render_table_capped(rows, MAX_TABLE_CHARS, links);
    out.push_str(&table);
    if truncated || cut {
        out.push_str("\n\n> この表は大きいため、先頭部分だけを収録しました。\n");
    }
    out
}

fn row_cells(row: Option<&Vec<String>>) -> Vec<String> {
    row.map(|cells| {
        cells
            .iter()
            .filter(|cell| !cell.trim().is_empty())
            .take(8)
            .cloned()
            .collect()
    })
    .unwrap_or_default()
}

fn term_body(pages: &[(String, String)]) -> String {
    let mut out = String::from("この語が出るページです。\n\n");
    for (stem, title) in pages.iter().take(20) {
        out.push_str(&format!("- [{}]({stem}.md)\n", link_label(title)));
    }
    if pages.len() > 20 {
        out.push_str("\nほかのページにも出ます。\n");
    }
    out
}

fn term_links(
    terms: &[String],
    names: &mut BTreeSet<String>,
    existing: &BTreeMap<String, String>,
) -> BTreeMap<String, TermLink> {
    let mut links = BTreeMap::new();
    for term in terms {
        let stem = if let Some(stem) = existing.get(term) {
            stem.clone()
        } else {
            allocate_name(term, names)
        };
        links.insert(
            term.clone(),
            TermLink {
                wikilink: stem == *term,
                stem,
            },
        );
    }
    links
}

fn select_terms(counts: &BTreeMap<String, usize>) -> Vec<String> {
    let mut ranked: Vec<(&String, &usize)> =
        counts.iter().filter(|(_, count)| **count >= 2).collect();
    ranked.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
    ranked.truncate(40);
    let mut terms: Vec<String> = ranked.into_iter().map(|(term, _)| term.clone()).collect();
    terms.sort();
    terms
}

fn count_unit(unit: &Unit, counts: &mut BTreeMap<String, usize>) {
    // The page title counts once. The same heading inside the body is not a second place.
    bump(counts, &unit.title);
    let title = normalize_term(&unit.title);
    match &unit.body {
        UnitBody::Prose(text) => collect_text_keys_skipping(text, counts, title.as_deref()),
        UnitBody::Table { rows, .. } => {
            for row in rows {
                for cell in row {
                    bump(counts, cell);
                }
            }
        }
    }
}

fn collect_text_keys(markdown: &str, counts: &mut BTreeMap<String, usize>) {
    collect_text_keys_skipping(markdown, counts, None);
}

fn collect_text_keys_skipping(
    markdown: &str,
    counts: &mut BTreeMap<String, usize>,
    skip: Option<&str>,
) {
    for line in markdown.lines() {
        if let Some(title) = heading_text(line, 6) {
            let normalized = normalize_term(&title);
            if !skip_heading(&title) && normalized.as_deref() != skip {
                bump(counts, &title);
            }
            continue;
        }
        if is_table_row(line) && !is_separator_row(line) {
            for cell in split_row(line) {
                bump(counts, &plain_cell(&cell));
            }
        }
    }
}

fn remember_mentions(
    markdown: &str,
    links: &BTreeMap<String, TermLink>,
    stem: &str,
    title: &str,
    mentions: &mut BTreeMap<String, BTreeSet<(String, String)>>,
) {
    let mut counts = BTreeMap::new();
    collect_text_keys(markdown, &mut counts);
    bump(&mut counts, title);
    for term in links.keys() {
        if counts.contains_key(term) {
            mentions
                .entry(term.clone())
                .or_default()
                .insert((stem.to_string(), title.to_string()));
        }
    }
}

fn bump(counts: &mut BTreeMap<String, usize>, raw: &str) {
    if let Some(term) = normalize_term(raw) {
        *counts.entry(term).or_default() += 1;
    }
}

fn normalize_term(raw: &str) -> Option<String> {
    let text = plain_cell(raw);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = text.chars().count();
    if !(2..=40).contains(&count) {
        return None;
    }
    if text.contains(['|', '[', ']', '#', '<', '>', '`']) {
        return None;
    }
    if !text.chars().any(|ch| ch.is_alphabetic()) {
        return None;
    }
    const SKIP: &[&str] = &[
        "true",
        "false",
        "yes",
        "no",
        "null",
        "none",
        "n/a",
        "na",
        "なし",
        "無し",
        "あり",
        "未設定",
        "不明",
    ];
    if SKIP.iter().any(|item| text.eq_ignore_ascii_case(item)) {
        return None;
    }
    Some(text)
}

fn skip_heading(title: &str) -> bool {
    matches!(title, "出典" | "出典の表" | "ページ" | "記録" | "Wiki") || title.ends_with("の概要")
}

fn plain_cell(cell: &str) -> String {
    let cell = cell.trim();
    if let Some(inner) = cell
        .strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
    {
        return inner.split('|').next().unwrap_or(inner).trim().to_string();
    }
    if let Some(rest) = cell.strip_prefix('[')
        && let Some(end) = rest.find("](")
    {
        return rest[..end].to_string();
    }
    cell.to_string()
}

fn linkify_cell(cell: &str, links: &BTreeMap<String, TermLink>) -> String {
    let key = normalize_term(cell);
    let Some(key) = key else {
        return cell.to_string();
    };
    let Some(link) = links.get(&key) else {
        return cell.to_string();
    };
    if link.wikilink {
        format!("[[{key}]]")
    } else {
        format!("[{}]({}.md)", link_label(&key), link.stem)
    }
}

fn link_markdown_tables(markdown: &str, links: &BTreeMap<String, TermLink>) -> String {
    if links.is_empty() {
        return markdown.to_string();
    }
    let mut out = String::new();
    for line in markdown.lines() {
        if is_table_row(line) && !is_separator_row(line) {
            let cells: Vec<String> = split_row(line)
                .into_iter()
                .map(|cell| linkify_cell(cell.trim(), links))
                .collect();
            out.push_str(&format!("| {} |\n", cells.join(" | ")));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim_end_matches('\n').to_string()
}

fn is_table_row(line: &str) -> bool {
    line.trim().starts_with('|')
}

fn is_separator_row(line: &str) -> bool {
    let cells = split_row(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let trimmed = cell.trim();
            !trimmed.is_empty() && trimmed.chars().all(|ch| matches!(ch, '-' | ':'))
        })
}

fn split_row(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = line.trim().trim_matches('|').chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'|') {
            cell.push('|');
            chars.next();
        } else if ch == '|' {
            cells.push(std::mem::take(&mut cell));
        } else {
            cell.push(ch);
        }
    }
    cells.push(cell);
    cells
}

fn link_label(text: &str) -> String {
    text.replace(['[', ']'], "")
}

fn opening_sentence(text: &str) -> String {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with('|')
            || trimmed.starts_with('>')
        {
            continue;
        }
        return trimmed.chars().take(80).collect();
    }
    String::new()
}

#[allow(clippy::too_many_arguments)]
fn make_page(
    stem: &str,
    title: &str,
    kind: &str,
    generated_by: &str,
    at: &str,
    source: &str,
    body: &str,
    rank: u8,
    log_line: &str,
) -> Page {
    let sources = [source.to_string()];
    Page {
        path: format!("pages/{stem}.md"),
        title: title.to_string(),
        markdown: page_markdown(
            kind,
            title,
            &plain_excerpt(body),
            generated_by,
            at,
            &sources,
            body,
        ),
        rank,
        log_line: log_line.to_string(),
    }
}

fn page_markdown(
    kind: &str,
    title: &str,
    description: &str,
    generated_by: &str,
    at: &str,
    sources: &[String],
    body: &str,
) -> String {
    let mut sources_yaml = String::from("sources:\n");
    let listed: Vec<&String> = if sources.is_empty() {
        Vec::new()
    } else {
        sources.iter().take(8).collect()
    };
    if listed.is_empty() {
        sources_yaml.push_str("  - title: \"変換\"\n    id: \"変換\"\n");
    } else {
        for source in &listed {
            sources_yaml.push_str(&format!(
                "  - title: {}\n    id: {}\n",
                yaml_quote(source),
                yaml_quote(source)
            ));
        }
    }
    format!(
        "---\ntype: {}\ntitle: {}\ndescription: {}\ngenerated:\n  by: {generated_by}\n  at: {}\n{sources_yaml}---\n\n# {}\n\n{body}\n\n## 出典\n\n- {}\n",
        yaml_quote(kind),
        yaml_quote(title),
        yaml_quote(description),
        yaml_quote(at),
        title.trim(),
        listed.first().map(|item| item.as_str()).unwrap_or("変換")
    )
}

fn read_stored_pages(destination: &Path) -> Result<Vec<StoredPage>> {
    let directory = destination.join("pages");
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut pages = Vec::new();
    let mut paths: Vec<PathBuf> = fs::read_dir(&directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    paths.sort();
    for path in paths {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        let markdown = fs::read_to_string(&path)?;
        let value = frontmatter(&markdown).ok().flatten();
        let title = value
            .as_ref()
            .and_then(|item| item.get("title"))
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string();
        let kind = value
            .as_ref()
            .and_then(|item| item.get("type"))
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string();
        let generated_by = value
            .as_ref()
            .and_then(|item| item.get("generated"))
            .and_then(|item| item.get("by"))
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string();
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("page")
            .to_string();
        pages.push(StoredPage {
            path: format!("pages/{stem}.md"),
            stem,
            title,
            markdown,
            kind,
            generated_by,
        });
    }
    Ok(pages)
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
    render_table_capped(rows, usize::MAX, &BTreeMap::new()).0
}

fn render_table_capped(
    rows: &[Vec<String>],
    max_chars: usize,
    links: &BTreeMap<String, TermLink>,
) -> (String, bool) {
    if rows.is_empty() {
        return (String::new(), false);
    }
    let width = rows.iter().map(|row| row.len()).max().unwrap_or(0).min(32);
    let mut out = String::new();
    let mut truncated = rows.len() > 2000;
    for (index, row) in rows.iter().take(2000).enumerate() {
        let cells: Vec<String> = (0..width)
            .map(|column| {
                row.get(column)
                    .map(|cell| {
                        let linked = linkify_cell(cell, links);
                        let flat = linked.replace('|', "\\|").replace('\n', " ");
                        let mut clipped: String = flat.chars().take(500).collect();
                        if flat.chars().count() > 500 {
                            clipped.push('…');
                        }
                        clipped
                    })
                    .unwrap_or_default()
            })
            .collect();
        let mut line = format!("| {} |\n", cells.join(" | "));
        if index == 0 {
            line.push('|');
            for _ in 0..width {
                line.push_str(" --- |");
            }
            line.push('\n');
        }
        if out.len().saturating_add(line.len()) > max_chars && !out.is_empty() {
            truncated = true;
            break;
        }
        out.push_str(&line);
    }
    (out, truncated)
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
            XmlEvent::ParagraphEnd { heading } => {
                if in_table > 0 {
                    if !cell.is_empty() && !cell.ends_with(' ') {
                        cell.push(' ');
                    }
                } else if !paragraph.trim().is_empty() {
                    if let Some(level) = heading {
                        out.push_str(&"#".repeat(level as usize));
                        out.push(' ');
                    }
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
    ParagraphEnd { heading: Option<u8> },
    CellEnd,
    RowEnd,
    TableStart,
    TableEnd,
}

fn xml_events(xml: &str) -> Vec<XmlEvent> {
    let mut events = Vec::new();
    let mut rest = xml;
    let mut heading = None;
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
        } else if !closing && local == "p" {
            heading = None;
        } else if !closing && (local == "pStyle" || local == "outlineLvl") {
            if let Some(level) = style_heading(local, tag) {
                heading = Some(level);
            }
        } else if closing && local == "p" {
            events.push(XmlEvent::ParagraphEnd { heading });
            heading = None;
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

struct SheetData {
    name: String,
    rows: Vec<Vec<String>>,
    truncated: bool,
}

#[cfg(test)]
fn xlsx_to_markdown(path: &Path) -> Result<String> {
    let sheets = read_sheets(path)?;
    let mut out = String::new();
    let mut truncated = false;
    for sheet in &sheets {
        if out.len() >= MAX_TABLE_CHARS {
            truncated = true;
            break;
        }
        truncated |= sheet.truncated;
        let (table, cut) =
            render_table_capped(&sheet.rows, MAX_TABLE_CHARS - out.len(), &BTreeMap::new());
        truncated |= cut;
        out.push_str(&format!("## {}\n\n{table}\n", sheet.name));
    }
    if truncated {
        out.push_str("\n> この表は大きいため、先頭部分だけを収録しました。\n");
    }
    ensure!(!out.trim().is_empty(), "表が見つかりません");
    Ok(out)
}

fn read_sheets(path: &Path) -> Result<Vec<SheetData>> {
    let shared_xml = read_zip_prefix(path, "xl/sharedStrings.xml", MAX_ENTRY)?;
    let shared_cut = shared_xml.as_ref().is_some_and(|(_, cut)| *cut);
    let shared = shared_xml
        .map(|(xml, _)| shared_strings(&xml))
        .unwrap_or_default();
    let ordered = ordered_sheets(path)?;
    ensure!(!ordered.is_empty(), "表が見つかりません");
    let mut sheets = Vec::new();
    for (index, (name, display)) in ordered.iter().enumerate() {
        let Some((xml, cut)) = read_zip_prefix(path, name, MAX_ENTRY)? else {
            continue;
        };
        let rows = sheet_rows(&xml, &shared);
        if rows.is_empty() && !cut {
            continue;
        }
        let title = if display.trim().is_empty() {
            format!("Sheet {}", index + 1)
        } else {
            display.clone()
        };
        sheets.push(SheetData {
            name: title,
            rows,
            truncated: cut || shared_cut,
        });
    }
    ensure!(!sheets.is_empty(), "表が見つかりません");
    Ok(sheets)
}

fn ordered_sheets(path: &Path) -> Result<Vec<(String, String)>> {
    if let Some(named) = workbook_sheets(path)
        && !named.is_empty()
    {
        return Ok(named);
    }
    let mut sheets: Vec<String> = zip_names(path)?
        .into_iter()
        .filter(|name| name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml"))
        .collect();
    sheets.sort_by_key(|name| sheet_ordinal(name));
    Ok(sheets
        .into_iter()
        .map(|name| (name, String::new()))
        .collect())
}

fn workbook_sheets(path: &Path) -> Option<Vec<(String, String)>> {
    let book = read_zip_text(path, "xl/workbook.xml").ok().flatten()?;
    let rels = read_zip_text(path, "xl/_rels/workbook.xml.rels")
        .ok()
        .flatten()?;
    let mut targets = BTreeMap::new();
    for tag in rels.split('<').skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        let Some(id) = xml_attr(tag, "Id") else {
            continue;
        };
        let Some(target) = xml_attr(tag, "Target") else {
            continue;
        };
        targets.insert(id.to_string(), sheet_zip_path(target));
    }
    let mut sheets = Vec::new();
    for tag in book.split('<').skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        if tag.starts_with('/') || tag.starts_with('!') {
            continue;
        }
        let local = tag
            .split_whitespace()
            .next()
            .unwrap_or("")
            .rsplit(':')
            .next()
            .unwrap_or("");
        if local != "sheet" {
            continue;
        }
        let name = xml_attr(tag, "name").unwrap_or("").to_string();
        let Some(id) = xml_attr(tag, "r:id").or_else(|| xml_attr(tag, "id")) else {
            continue;
        };
        let Some(path) = targets.get(id) else {
            continue;
        };
        sheets.push((path.clone(), name));
    }
    (!sheets.is_empty()).then_some(sheets)
}

fn sheet_zip_path(target: &str) -> String {
    let target = target.replace('\\', "/");
    let target = target.trim_start_matches('/');
    if let Some(stripped) = target.strip_prefix("./") {
        return sheet_zip_path(stripped);
    }
    if target.starts_with("xl/") {
        target.to_string()
    } else {
        format!("xl/{target}")
    }
}

fn sheet_ordinal(name: &str) -> u32 {
    name.trim_start_matches("xl/worksheets/sheet")
        .trim_end_matches(".xml")
        .parse()
        .unwrap_or(0)
}

fn xml_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let rest = tag.split(&needle).nth(1)?;
    rest.split('"').next()
}

fn style_heading(local: &str, tag: &str) -> Option<u8> {
    let value = xml_attr(tag, "w:val").or_else(|| xml_attr(tag, "val"))?;
    if local == "outlineLvl" {
        let level = value.parse::<u8>().ok()?;
        return (level < 3).then_some(level + 1);
    }
    let compact: String = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    let folded = compact.to_ascii_lowercase();
    let number = folded
        .strip_prefix("heading")
        .or_else(|| compact.strip_prefix("見出し"))?;
    let level = number.parse::<u8>().ok()?;
    (1..=3).contains(&level).then_some(level)
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

fn pptx_units(path: &Path) -> Result<Vec<Unit>> {
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
    let mut units = Vec::new();
    for (index, name) in slides.iter().enumerate() {
        let xml = read_zip_text(path, name)?.unwrap_or_default();
        let lines = xml
            .split("<a:t")
            .skip(1)
            .filter_map(|piece| piece.split_once('>'))
            .map(|(_, rest)| xml_unescape(rest.split("</a:t>").next().unwrap_or("")))
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            continue;
        }
        let title = lines
            .first()
            .filter(|line| !line.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| format!("スライド {}", index + 1));
        units.push(Unit {
            title,
            body: UnitBody::Prose(lines.join("\n\n")),
        });
    }
    ensure!(!units.is_empty(), "スライドが見つかりません");
    Ok(units)
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
    let Some((text, truncated)) = read_zip_prefix(path, name, MAX_ENTRY)? else {
        return Ok(None);
    };
    ensure!(!truncated, "展開後の本文が大きすぎます");
    Ok(Some(text))
}

/// Read at most `limit` uncompressed bytes. Oversized spreadsheet parts are truncated
/// instead of failing the whole selection.
fn read_zip_prefix(path: &Path, name: &str, limit: u64) -> Result<Option<(String, bool)>> {
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
    let size = entry.size();
    let mut bytes = Vec::new();
    entry
        .take(limit)
        .read_to_end(&mut bytes)
        .context("本文を読み取れません")?;
    let truncated = size > limit || (size == 0 && bytes.len() as u64 == limit);
    Ok(Some((
        String::from_utf8_lossy(&bytes).into_owned(),
        truncated,
    )))
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

fn overwrite_pages(destination: &Path, pages: &[Page]) -> Result<()> {
    for page in pages {
        fs::write(destination.join(&page.path), &page.markdown)?;
    }
    Ok(())
}

fn write_index(destination: &Path, pages: &[Page]) -> Result<()> {
    let mut ordered: Vec<&Page> = pages.iter().collect();
    ordered.sort_by(|left, right| left.rank.cmp(&right.rank).then(left.path.cmp(&right.path)));
    let mut body = String::from("---\nokf_version: \"0.2\"\n---\n\n# Wiki\n\n## ページ\n\n");
    for page in ordered {
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
        writeln!(file, "- {at} {}", page.log_line)?;
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
            let value = frontmatter(&markdown).ok().flatten();
            let title = value
                .as_ref()
                .and_then(|item| item.get("title"))
                .and_then(|item| item.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("page")
                        .to_string()
                });
            let kind = value
                .as_ref()
                .and_then(|item| item.get("type"))
                .and_then(|item| item.as_str())
                .unwrap_or("");
            let generated_by = value
                .as_ref()
                .and_then(|item| item.get("generated"))
                .and_then(|item| item.get("by"))
                .and_then(|item| item.as_str())
                .unwrap_or("");
            let rank = if kind == "用語" {
                2
            } else if generated_by == "process:wiki-compile" {
                1
            } else {
                0
            };
            pages.push(Page {
                path: format!(
                    "pages/{}",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("page.md")
                ),
                title,
                markdown,
                rank,
                log_line: String::new(),
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

fn heading_text(line: &str, max_level: usize) -> Option<String> {
    let trimmed = line.trim();
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=max_level).contains(&hashes)
        || !trimmed
            .as_bytes()
            .get(hashes)
            .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        return None;
    }
    let text = trimmed[hashes..].trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn first_heading(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| heading_text(line, 6))
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
        assert_eq!(report.pages, 12);
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
        let grid = fs::read_to_string(wiki.join("pages/grid.md")).unwrap();
        assert!(grid.contains("見出し"));
        assert!(!grid.contains("先頭部分だけを収録"));
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
        assert_eq!(report.pages, 2);
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

    #[test]
    fn oversized_excel_keeps_the_leading_rows() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large.xlsx");
        let row = "<row><c r=\"A1\" t=\"inlineStr\"><is><t>記録</t></is></c></row>";
        let mut sheet = String::from("<worksheet><sheetData>");
        while sheet.len() <= 8 * 1024 * 1024 {
            sheet.push_str(row);
        }
        sheet.push_str("</sheetData></worksheet>");
        write_office(
            &path,
            &[
                ("xl/sharedStrings.xml", "<sst><si><t>見出し</t></si></sst>"),
                ("xl/worksheets/sheet1.xml", &sheet),
                (
                    "xl/worksheets/sheet2.xml",
                    r#"<worksheet><sheetData><row><c r="A1" t="inlineStr"><is><t>後半</t></is></c></row></sheetData></worksheet>"#,
                ),
            ],
        );
        let markdown = xlsx_to_markdown(&path).unwrap();
        assert!(markdown.contains("記録"));
        assert!(markdown.contains("後半"));
        assert!(markdown.contains("先頭部分だけを収録"));
        assert!(markdown.len() < 16 * 1024 * 1024);
    }

    #[test]
    fn splits_files_into_linked_pages_and_terms() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input");
        fs::create_dir(&input).unwrap();
        write_office(
            &input.join("grid.xlsx"),
            &[
                (
                    "xl/workbook.xml",
                    r#"<workbook><sheets><sheet name="売上" sheetId="1" r:id="rId1"/><sheet name="拠点" sheetId="2" r:id="rId2"/></sheets></workbook>"#,
                ),
                (
                    "xl/_rels/workbook.xml.rels",
                    r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="worksheets/sheet2.xml"/></Relationships>"#,
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    r#"<worksheet><sheetData><row><c r="A1" t="inlineStr"><is><t>顧客</t></is></c><c r="B1" t="inlineStr"><is><t>金額</t></is></c></row><row><c r="A2" t="inlineStr"><is><t>東京</t></is></c><c r="B2"><v>1</v></c></row><row><c r="A3" t="inlineStr"><is><t>東京</t></is></c><c r="B3"><v>2</v></c></row></sheetData></worksheet>"#,
                ),
                (
                    "xl/worksheets/sheet2.xml",
                    r#"<worksheet><sheetData><row><c r="A1" t="inlineStr"><is><t>顧客</t></is></c><c r="B1" t="inlineStr"><is><t>地域</t></is></c></row><row><c r="A2" t="inlineStr"><is><t>大阪</t></is></c><c r="B2" t="inlineStr"><is><t>西</t></is></c></row></sheetData></worksheet>"#,
                ),
            ],
        );
        write_office(
            &input.join("brief.docx"),
            &[(
                "word/document.xml",
                r#"<w:document><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>背景</w:t></w:r></w:p><w:p><w:r><w:t>説明文</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>結論</w:t></w:r></w:p><w:p><w:r><w:t>決めた</w:t></w:r></w:p></w:body></w:document>"#,
            )],
        );
        write_office(
            &input.join("deck.pptx"),
            &[
                (
                    "ppt/slides/slide1.xml",
                    r#"<p:sld><a:t>表紙</a:t><a:t>要点</a:t></p:sld>"#,
                ),
                (
                    "ppt/slides/slide2.xml",
                    r#"<p:sld><a:t>詳細</a:t><a:t>補足</a:t></p:sld>"#,
                ),
            ],
        );
        let wiki = temp.path().join("wiki");
        compile_file_wiki(
            &[
                input.join("grid.xlsx"),
                input.join("brief.docx"),
                input.join("deck.pptx"),
            ],
            &wiki,
        )
        .unwrap();
        assert!(
            validate_okf_bundle(&wiki).unwrap().is_empty(),
            "{:?}",
            validate_okf_bundle(&wiki)
        );
        let sales = fs::read_to_string(wiki.join("pages/grid--売上.md")).unwrap();
        assert!(sales.contains("[[東京]]"));
        assert!(sales.contains("[[顧客]]"));
        assert!(sales.contains("列: 顧客、金額"));
        assert!(sales.contains("[← grid](grid.md)"));
        let parent = fs::read_to_string(wiki.join("pages/grid.md")).unwrap();
        assert!(parent.contains("シートは 2 件です。"));
        assert!(parent.contains("(grid--拠点.md)"));
        let summary = fs::read_to_string(wiki.join("pages/grid--summary.md")).unwrap();
        assert!(summary.contains("process:wiki-compile"));
        assert!(summary.contains("金額"));
        assert!(summary.contains("先頭は 東京、1、東京、2 です。"));
        let term = fs::read_to_string(wiki.join("pages/東京.md")).unwrap();
        assert!(term.contains("type: \"用語\""));
        assert!(term.contains("(grid--売上.md)"));
        let background = fs::read_to_string(wiki.join("pages/brief--背景.md")).unwrap();
        assert!(background.contains("説明文"));
        assert!(background.contains("(brief.md)"));
        assert!(
            fs::read_to_string(wiki.join("pages/brief.md"))
                .unwrap()
                .contains("(brief--結論.md)")
        );
        assert!(
            fs::read_to_string(wiki.join("pages/deck.md"))
                .unwrap()
                .contains("スライドは 2 枚です。")
        );
        assert!(
            fs::read_to_string(wiki.join("pages/deck--詳細.md"))
                .unwrap()
                .contains("補足")
        );
        let mut hub = crate::Hub::open(&temp.path().join("settings")).unwrap();
        hub.register("demo", "Demo", &wiki).unwrap();
        hub.scan("demo").unwrap();
        let graph = hub.graph("demo", None, 40).unwrap();
        assert!(graph.edges.iter().any(|edge| {
            let pair = [&edge.source, &edge.target];
            pair.iter().any(|path| path.ends_with("東京.md"))
                && pair.iter().any(|path| path.contains("売上"))
        }));
        let extra = input.join("more.csv");
        fs::write(&extra, "地域,件数\n大阪,3\n").unwrap();
        append_file_wiki(&[extra], &wiki).unwrap();
        let updated = fs::read_to_string(wiki.join("pages/grid--拠点.md")).unwrap();
        assert!(updated.contains("[[大阪]]"), "{updated}");
        assert!(
            validate_okf_bundle(&wiki).unwrap().is_empty(),
            "{:?}",
            validate_okf_bundle(&wiki)
        );
    }
}
