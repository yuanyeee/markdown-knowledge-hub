use anyhow::{Context, Result, ensure};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

struct Spec {
    name: String,
    path: PathBuf,
    source: String,
    supplemental: bool,
    details: Vec<(PathBuf, String)>,
    references: Vec<(String, usize, bool)>,
}

fn parse(path: PathBuf, source: String) -> Option<Spec> {
    let mut name = None;
    let mut supplemental = false;
    let mut section = String::new();

    let mut line_text = String::new();
    let mut cells = Vec::new();
    let mut dependency_table = None;
    let mut capture = false;
    let newlines: Vec<_> = source
        .match_indices('\n')
        .map(|(offset, _)| offset)
        .collect();
    let mut references = Vec::new();
    for (event, span) in Parser::new_ext(&source, Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                capture = true;
                line_text.clear();
            }
            Event::End(TagEnd::Heading(level)) => {
                if level == HeadingLevel::H1 && name.is_none() {
                    let title = line_text.trim();
                    supplemental = title.ends_with(" データ構造");
                    name = title
                        .strip_suffix(" モジュール仕様書")
                        .or_else(|| title.strip_suffix(" データ構造"))
                        .or_else(|| title.strip_prefix("Extension 仕様書: "))
                        .map(str::to_owned);
                }
                capture = false;
                if level <= HeadingLevel::H2 {
                    section = line_text.trim().to_owned();
                }
            }
            Event::Start(Tag::Table(_)) => dependency_table = None,
            Event::Start(Tag::TableHead | Tag::TableRow) => cells.clear(),
            Event::Start(Tag::TableCell) => {
                capture = true;
                line_text.clear();
            }
            Event::Text(value) | Event::Code(value) if capture => line_text.push_str(&value),
            Event::End(TagEnd::TableCell) => {
                capture = false;
                cells.push(line_text.trim().to_owned());
            }
            Event::End(TagEnd::TableHead) => {
                dependency_table = match cells.first().map(String::as_str) {
                    Some("モジュール") if section.starts_with("使っているモジュール") => {
                        Some(true)
                    }
                    Some("モジュール名") if section == "参照モジュール" => Some(false),
                    _ => None,
                };
            }
            Event::End(TagEnd::TableRow) => {
                if let (Some(reverse), Some(value)) = (
                    dependency_table,
                    cells.first().filter(|value| !value.is_empty()),
                ) {
                    let line = newlines.partition_point(|offset| *offset < span.start) + 1;
                    references.push((value.clone(), line, reverse));
                }
            }
            Event::End(TagEnd::Table) => dependency_table = None,
            _ => {}
        }
    }
    let name = name.filter(|value| !value.is_empty())?;
    Some(Spec {
        name,
        path,
        source,
        supplemental,
        details: Vec::new(),
        references,
    })
}

#[derive(Serialize)]
pub struct OmlReport {
    pub modules: usize,
    pub elements: usize,
    pub resolved_references: usize,
    pub unresolved_references: usize,
}

fn literal(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| {
            let prefix = if ch.is_ascii_punctuation() {
                Some('\\')
            } else {
                None
            };
            prefix.into_iter().chain(Some(ch))
        })
        .collect()
}

/// Compile OML Utilities Markdown exports into a separate Wiki input directory.
pub fn compile_oml_wiki(input: &Path, destination: &Path) -> Result<OmlReport> {
    let input = input
        .canonicalize()
        .context("OML仕様書フォルダーが見つかりません")?;
    ensure!(input.is_dir(), "OML仕様書フォルダーを指定してください");
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let destination = parent.join(destination.file_name().context("生成先の名前が必要です")?);
    ensure!(
        !crate::outputs::overlaps(&input, &destination),
        "原本と生成先を分けてください"
    );
    ensure!(!destination.exists(), "未使用の生成先を指定してください");

    let mut specs = Vec::new();
    let mut details = Vec::new();
    for entry in walkdir::WalkDir::new(&input)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry?;
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            let relative = entry.path().strip_prefix(&input)?.to_owned();
            let folder = relative.parent().unwrap_or(Path::new(""));
            let owner = match relative.file_name().and_then(|name| name.to_str()) {
                Some("rest.md" | "data.md") => Some(folder.join("index.md")),
                _ if folder
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| matches!(name, "screens" | "webblocks" | "actions")) =>
                {
                    folder.parent().map(|parent| parent.join("index.md"))
                }
                _ => None,
            };
            let source = fs::read_to_string(entry.path())?;
            if let Some(owner) = owner {
                details.push((owner, relative, source));
            } else if let Some(spec) = parse(relative, source) {
                specs.push(spec);
            }
        }
    }
    let owners: BTreeMap<_, _> = specs
        .iter()
        .enumerate()
        .filter(|(_, spec)| !spec.supplemental)
        .map(|(index, spec)| (spec.path.clone(), index))
        .collect();
    for (owner, relative, source) in details {
        if let Some(&index) = owners.get(&owner) {
            specs[index].details.push((relative, source));
        } else if let Some(spec) = parse(relative, source) {
            specs.push(spec);
        }
    }
    ensure!(
        !specs.is_empty(),
        "モジュール見出しのあるMarkdownが見つかりません"
    );

    let mut by_name = BTreeMap::<&str, Vec<usize>>::new();
    for (id, spec) in specs.iter().enumerate() {
        if !spec.supplemental {
            by_name.entry(&spec.name).or_default().push(id);
        }
    }
    let page = |id: usize| {
        format!(
            "oml-{}.md",
            blake3::hash(specs[id].path.to_string_lossy().as_bytes()).to_hex()
        )
    };
    let mut report = OmlReport {
        modules: specs.len(),
        elements: 0,
        resolved_references: 0,
        unresolved_references: 0,
    };
    let mut dependencies = vec![String::new(); specs.len()];
    for (id, spec) in specs.iter().enumerate() {
        for (target, line, reverse) in &spec.references {
            let evidence = format!("{} {} 行", literal(&spec.path.to_string_lossy()), line);
            match by_name.get(target.as_str()).map(Vec::as_slice) {
                Some([target_id]) => {
                    // The extension table lists the modules that consume it.
                    let (from, to) = if *reverse {
                        (*target_id, id)
                    } else {
                        (id, *target_id)
                    };
                    dependencies[from].push_str(&format!(
                        "- [{}]({}) — {}\n",
                        literal(&specs[to].name),
                        page(to),
                        evidence
                    ));
                    report.resolved_references += 1;
                }
                _ => {
                    dependencies[id].push_str(&format!(
                        "- 未解決の{}　{} — {}\n",
                        if *reverse { "利用元" } else { "参照先" },
                        literal(target),
                        evidence
                    ));
                    report.unresolved_references += 1;
                }
            }
        }
    }
    let extracted: Vec<_> = specs
        .iter()
        .map(|spec| {
            std::iter::once((&spec.path, &spec.source))
                .chain(spec.details.iter().map(|(path, source)| (path, source)))
                .flat_map(|(path, source)| {
                    crate::oml_elements::extract(source)
                        .into_iter()
                        .map(move |element| (path, element))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let mut types = BTreeMap::<(&str, &str), Vec<(usize, usize)>>::new();
    let mut actions = BTreeMap::<(usize, &str), Vec<usize>>::new();
    for (id, elements) in extracted.iter().enumerate() {
        for (number, (_, element)) in elements.iter().enumerate() {
            if matches!(element.kind, "Structure" | "Entity") {
                types
                    .entry((&specs[id].name, &element.title))
                    .or_default()
                    .push((id, number + 1));
            }
            if element.kind.contains("アクション") || element.kind == "関数" {
                actions
                    .entry((id, &element.title))
                    .or_default()
                    .push(number + 1);
            }
        }
    }
    fs::create_dir(&destination)?;
    let mut index = String::from("# OML Wiki\n\n## モジュール一覧\n\n");
    for (id, spec) in specs.iter().enumerate() {
        index.push_str(&format!("- [{}]({})\n", literal(&spec.name), page(id)));
        let mut body = format!(
            "# {}\n\n出典　{}\n\n## モジュール依存\n\n{}",
            literal(&spec.name),
            literal(&spec.path.to_string_lossy()),
            dependencies[id]
        );
        let elements = &extracted[id];
        let mut anchors = BTreeMap::<String, Vec<usize>>::new();
        let mut detail_pages = BTreeMap::<&Path, Vec<usize>>::new();
        for (number, (source, element)) in elements.iter().enumerate() {
            for anchor in &element.anchors {
                anchors.entry(anchor.clone()).or_default().push(number + 1);
            }
            if *source != &spec.path {
                detail_pages
                    .entry(source.as_path())
                    .or_default()
                    .push(number + 1);
            }
        }
        let module_page = page(id);
        let folder = module_page.trim_end_matches(".md");
        let route_file = |source: &Path, url: &str, prefix: &str| {
            let (url, fragment) = url.split_once('#').map_or((url, ""), |pair| pair);
            if url.is_empty() || url.contains(':') || url.starts_with('/') {
                return None;
            }
            let url = percent_encoding::percent_decode_str(url)
                .decode_utf8()
                .ok()?;
            let mut path = source.parent()?.to_path_buf();
            for component in Path::new(url.as_ref()).components() {
                match component {
                    std::path::Component::Normal(part) => path.push(part),
                    std::path::Component::ParentDir if path.pop() => {}
                    std::path::Component::CurDir => {}
                    _ => return None,
                }
            }
            if path == spec.path {
                return Some(format!("{prefix}{module_page}"));
            }
            let numbers = detail_pages.get(path.as_path())?;
            if !fragment.is_empty() {
                let fragment = percent_encoding::percent_decode_str(fragment)
                    .decode_utf8()
                    .ok()?;
                let mut matches = numbers.iter().filter(|&&number| {
                    elements[number - 1]
                        .1
                        .anchors
                        .iter()
                        .any(|anchor| anchor == fragment.as_ref())
                });
                if let (Some(number), None) = (matches.next(), matches.next()) {
                    return Some(format!("{prefix}{folder}/{number}.md"));
                }
            }
            if let [number] = numbers.as_slice() {
                return Some(format!("{prefix}{folder}/{number}.md"));
            }
            let detail = spec.details.iter().find(|(detail, _)| detail == &path)?;
            let title = detail.1.lines().find_map(|line| line.strip_prefix("# "))?;
            let anchor = crate::parser::heading_id(title);
            Some(format!(
                "{prefix}{module_page}#{}",
                percent_encoding::utf8_percent_encode(&anchor, percent_encoding::NON_ALPHANUMERIC)
            ))
        };
        if !elements.is_empty() {
            fs::create_dir(destination.join(folder))?;
            body.push_str("\n## 要素別Wiki\n\n");
            for (number, (source, element)) in elements.iter().enumerate() {
                let title = format!("{} · {}", element.kind, literal(&element.title));
                let relative = format!("{folder}/{}.md", number + 1);
                body.push_str(&format!("- [{title}]({relative})\n"));
                let fragment = element
                    .anchors
                    .iter()
                    .rev()
                    .find(|anchor| {
                        anchors
                            .get(*anchor)
                            .is_some_and(|matches| matches.len() == 1)
                    })
                    .map(|anchor| {
                        format!(
                            "#{}",
                            percent_encoding::utf8_percent_encode(
                                anchor,
                                percent_encoding::NON_ALPHANUMERIC
                            )
                        )
                    })
                    .unwrap_or_default();
                let mut content = format!(
                    "# {title}\n\n[元仕様書ページ](../{module_page}{fragment}) · {} {} 行\n\n{}",
                    literal(&source.to_string_lossy()),
                    element.line,
                    crate::oml_elements::route_links(
                        &crate::oml_elements::route_fragments(&element.body, &anchors, ""),
                        |url| route_file(source, url, "../")
                    )
                );
                content.push_str("\n<a id=\"oml-call\"></a>\n");
                if element.kind == "画面"
                    || element.kind.contains("アクション")
                    || matches!(
                        element.kind,
                        "関数" | "Webブロック" | "REST" | "SOAP" | "REST / SOAP" | "タイマー"
                    )
                {
                    let mut calls = crate::oml_elements::calls(&element.body);
                    calls.extend(element.timer_action.iter().cloned());
                    if !calls.is_empty() {
                        content.push_str("\n## 処理フローの呼び出し先\n\n");
                    }
                    for call in calls {
                        let (module, action) = call.split_once('.').map_or(
                            (Some(id), call.as_str()),
                            |(module, action)| {
                                let matched =
                                    by_name
                                        .get(module)
                                        .and_then(|items| match items.as_slice() {
                                            [module] => Some(*module),
                                            _ => None,
                                        });
                                (matched, action)
                            },
                        );
                        let target = module.and_then(|module| {
                            actions.get(&(module, action)).map(|items| (module, items))
                        });
                        if let Some((module, numbers)) = target
                            && let [number] = numbers.as_slice()
                        {
                            let target = page(module);
                            let folder = target.trim_end_matches(".md");
                            content.push_str(&format!(
                                "- [{}](../{folder}/{number}.md#oml-call)\n",
                                literal(&call)
                            ));
                        } else {
                            content.push_str(&format!("- 未解決　{}\n", literal(&call)));
                        }
                    }
                }
                let mut type_links = String::new();
                for name in crate::oml_elements::declared_types(&element.body) {
                    let (module, target) = name.rsplit_once('.').unwrap_or((&spec.name, &name));
                    if let Some([(target_id, target_number)]) =
                        types.get(&(module, target)).map(Vec::as_slice)
                        && (*target_id != id || *target_number != number + 1)
                    {
                        let target_page = page(*target_id);
                        let target_folder = target_page.trim_end_matches(".md");
                        type_links.push_str(&format!(
                            "- [{}](../{target_folder}/{target_number}.md)\n",
                            literal(&name)
                        ));
                    } else if name.contains('.') {
                        type_links.push_str(&format!("- 未解決　{}\n", literal(&name)));
                    }
                }
                if !type_links.is_empty() {
                    content.push_str("\n## 使用するデータ型\n\n");
                    content.push_str(&type_links);
                }
                if element.kind == "Structure"
                    && !element
                        .body
                        .lines()
                        .any(|line| line.trim_start().starts_with("| 公開"))
                {
                    content.push_str("\n公開範囲：仕様書に記載なし\n");
                }
                fs::write(destination.join(relative), content)?;
                report.elements += 1;
            }
        }
        body.push_str("\n## 元仕様書\n\n");
        let prefix = format!("{folder}/");
        for (source, raw) in std::iter::once((&spec.path, &spec.source))
            .chain(spec.details.iter().map(|(path, raw)| (path, raw)))
        {
            let rendered = crate::oml_elements::route_fragments(raw, &anchors, &prefix);
            if spec.details.is_empty() {
                body.push_str(&rendered);
            } else {
                body.push_str(&crate::oml_elements::route_links(&rendered, |url| {
                    route_file(source, url, "")
                }));
            }
            body.push_str("\n\n");
        }
        fs::write(destination.join(page(id)), body)?;
    }
    fs::write(destination.join("index.md"), index)?;
    Ok(report)
}

impl crate::Hub {
    /// Prepare a separate Markdown input for the normal project registration flow.
    pub fn prepare_oml_wiki(&self, id: &str, input: &Path, parent: &Path) -> Result<PathBuf> {
        let destination = self.generated_input_path(id, parent, "OML-Wiki")?;
        compile_oml_wiki(input, &destination)?;
        Ok(destination)
    }
    fn generated_input_path(&self, id: &str, parent: &Path, prefix: &str) -> Result<PathBuf> {
        Self::validate_id(id)?;
        let parent = parent.canonicalize()?;
        let destination = parent.join(format!("{prefix}-{id}-{}", uuid::Uuid::new_v4()));
        self.validate_input(&destination)?;
        for project in self.projects()? {
            ensure!(
                !crate::outputs::overlaps(Path::new(&project.root), &destination),
                "既存プロジェクトの入力領域には生成できません"
            );
        }
        Ok(destination)
    }

    pub fn prepare_markdown_selection(
        &self,
        id: &str,
        files: &[PathBuf],
        parent: &Path,
    ) -> Result<PathBuf> {
        ensure!(!files.is_empty(), "Markdownファイルを選択してください");
        let mut selected = std::collections::BTreeSet::new();
        for path in files {
            let path = path.canonicalize()?;
            ensure!(
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("md")),
                "有効な.mdファイルを選択してください"
            );
            selected.insert(path);
        }
        let first = selected
            .first()
            .context("Markdownファイルを選択してください")?;
        let common = first
            .ancestors()
            .skip(1)
            .find(|root| selected.iter().all(|file| file.starts_with(root)))
            .context("同じドライブにあるMarkdownファイルを選択してください")?;
        let destination = self.generated_input_path(id, parent, "Markdown-Selection")?;
        fs::create_dir(&destination)?;
        for file in &selected {
            let target = destination.join(file.strip_prefix(common)?);
            fs::create_dir_all(target.parent().context("保存先が不正です")?)?;
            fs::copy(file, target)?;
        }
        Ok(destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Hub;

    #[test]
    fn connects_oml_parameter_to_xif_structure_and_indexes_split_oml_data() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let input = temp.path().join("input");
        let wiki = temp.path().join("wiki");
        fs::create_dir(&input)?;
        fs::write(
            input.join("Orders.md"),
            "# Orders モジュール仕様書\n\n# サーバーアクション: Save\n\n## 出力パラメータ\n\n| 名前 | 型 |\n|---|---|\n| Response | List of Shared.Result |\n",
        )?;
        fs::write(
            input.join("Shared.md"),
            "# Extension 仕様書: Shared\n\n## 構造体 (1)\n\n### Result\n\n結果の属性",
        )?;
        fs::write(
            input.join("data.md"),
            "# Orders データ構造\n\n## 構造体 (Structure)\n\n### OrderInput\n\n| 項目 | 値 |\n|---|---|\n| 公開 | Yes |",
        )?;
        assert_eq!(compile_oml_wiki(&input, &wiki)?.elements, 3);
        let mut hub = Hub::open(&temp.path().join("settings"))?;
        hub.register("demo", "Demo", &wiki)?;
        hub.scan("demo")?;
        let result = hub
            .search("demo", "Result")?
            .into_iter()
            .find(|hit| hit.title == "Structure · Result")
            .context("XIF structure not indexed")?;
        assert!(
            hub.backlink_details("demo", &result.path, 0, 20)?
                .items
                .iter()
                .any(|hit| hit.title == "サーバーアクション · Save")
        );
        assert!(
            hub.search("demo", "OrderInput")?
                .iter()
                .any(|hit| hit.title == "Structure · OrderInput")
        );
        Ok(())
    }

    #[test]
    fn split_oml_details_are_grouped_and_linked_to_their_module() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let input = temp.path().join("input");
        let wiki = temp.path().join("wiki");
        for folder in ["screens", "webblocks", "actions"] {
            fs::create_dir_all(input.join(folder))?;
        }
        fs::write(
            input.join("index.md"),
            "# Orders モジュール仕様書\n\n[画面](screens/Edit.md) / [連携](rest.md)\n\n## サイトプロパティ(2)\n\n| 名前 | 型 |\n|---|---|\n| Enabled | Boolean |\n\n## セッション変数 （3）\n\n| 名前 | 型 |\n|---|---|\n| Cart | Text |\n\n## その他のモジュール定義\n\n### タイマー\n\n| 名前 | 説明 | 詳細 |\n|---|---|---|\n| Nightly | 日次処理 | Action: Save<br>Schedule: Daily |\n| Weekly | 週次処理 | Action: Save |\n\n### ロール\n\n| 名前 | 説明 | 詳細 |\n|---|---|---|\n| Admin | 管理者 | |\n",
        )?;
        fs::write(
            input.join("screens/Edit.md"),
            "# 画面: Edit\n\n[← 目次](../index.md)\n\n[Save](../actions/Save.md)",
        )?;
        fs::write(
            input.join("webblocks/Cart.md"),
            "# Web ブロック: Cart\n\n## スクリーンアクション\n\n### Clear\n\n画面内処理",
        )?;
        fs::write(
            input.join("actions/Save.md"),
            "# サーバーアクション: Save\n\n保存処理",
        )?;
        fs::write(
            input.join("rest.md"),
            "# Orders REST / SOAP 連携\n\n## PublicApi (REST (公開))\n\n### PostOrder\n\n#### 処理フロー\n\nAPIの詳細\n\n## Legacy (SOAP (消費))\n\n### Send\n\nSOAPの詳細",
        )?;
        fs::write(
            input.join("data.md"),
            "# Orders データ構造\n\n## 構造体 (Structure)\n\n### Request\n\n入力",
        )?;

        let report = compile_oml_wiki(&input, &wiki)?;
        assert_eq!(report.modules, 1);
        assert_eq!(report.elements, 10);
        let mut hub = Hub::open(&temp.path().join("state"))?;
        hub.register("oml", "OML", &wiki)?;
        hub.scan("oml")?;
        let sources = hub.oml_catalog("oml", "")?;
        let pages = hub.oml_catalog("oml", &sources[0].path)?;
        let page = |title: &str| -> Result<&str> {
            pages
                .iter()
                .find(|item| item.title == title)
                .map(|item| item.path.as_str())
                .with_context(|| title.to_owned())
        };
        let timer = hub.document("oml", page("タイマー · Nightly")?)?;
        assert!(timer.markdown.contains("Schedule: Daily"));
        assert!(!timer.markdown.contains("Weekly"));
        let save = page("サーバーアクション · Save")?;
        assert!(
            hub.backlink_details("oml", save, 0, 20)?
                .items
                .iter()
                .any(|hit| hit.path == timer.path && hit.call)
        );
        assert!(
            hub.backlink_details("oml", save, 0, 20)?
                .items
                .iter()
                .any(|hit| hit.title == "画面 · Edit")
        );
        assert!(
            hub.document("oml", page("REST · PublicApi")?)?
                .markdown
                .contains("PostOrder")
        );
        for title in [
            "SOAP · Legacy",
            "タイマー · Weekly",
            "Webブロック · Cart",
            "Structure · Request",
            "サイトプロパティ · 一覧",
            "セッション変数 · 一覧",
        ] {
            page(title)?;
        }
        Ok(())
    }

    #[test]
    fn elements_are_grouped_by_source_and_backlinks_have_titles() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("source");
        let wiki = temp.path().join("wiki");
        fs::create_dir(&source)?;
        fs::write(
            source.join("orders.md"),
            "# Orders モジュール仕様書\n\n[保存へ](#store-id)\n\n# 画面: EditOrder\n\n## スクリーンアクション\n\n### Save\n\n画面処理\n\n| # | 種別 | 処理 | 次へ |\n|---|---|---|---|\n| 1 | アクション実行 | Store | → 2 |\n| 2 | 終了 | End | |\n\n<a id=\"store-id\"></a>\n\n# サーバーアクション: Store\n\nサーバー処理\n\n## エンティティ\n\n### Order\n\n注文データ",
        )?;
        assert_eq!(compile_oml_wiki(&source, &wiki)?.elements, 3);
        let mut hub = Hub::open(&temp.path().join("settings"))?;
        hub.register("oml", "OML", &wiki)?;
        hub.scan("oml")?;
        let sources = hub.oml_catalog("oml", "")?;
        assert_eq!(sources.len(), 1);
        let folder = sources[0].path.trim_end_matches(".md");
        let summary = hub.graph_summary("oml")?;
        assert!(
            summary
                .nodes
                .iter()
                .any(|node| node.path == folder && node.title == "Orders (3文書)")
        );
        assert!(
            summary
                .edges
                .iter()
                .any(|edge| edge.source == folder || edge.target == folder)
        );
        let pages = hub.oml_catalog("oml", &sources[0].path)?;
        assert_eq!(pages.len(), 4);
        let screen = pages
            .iter()
            .find(|hit| hit.title == "画面 · EditOrder")
            .context("screen page")?;
        let backlinks = hub.backlink_details("oml", &screen.path, 0, 20)?;
        assert_eq!(backlinks.items[0].title, "Orders");
        assert!(backlinks.items.iter().all(|hit| hit.path != screen.path));
        let store = pages
            .iter()
            .find(|hit| hit.title == "サーバーアクション · Store")
            .context("store page")?;
        let callers = hub.backlink_details("oml", &store.path, 0, 20)?;
        assert!(
            callers
                .items
                .iter()
                .any(|hit| hit.path == screen.path && hit.call)
        );
        assert!(
            hub.graph("oml", Some(&store.path), 20)?
                .edges
                .iter()
                .any(|edge| edge.source == screen.path && edge.target == store.path && edge.call)
        );
        let module = hub.document("oml", &sources[0].path)?;
        assert!(
            module
                .markdown
                .contains(&format!("[保存へ]({})", store.path))
        );
        Ok(())
    }

    #[test]
    fn compiled_dependencies_are_searchable_and_have_backlinks() -> Result<()> {
        let workspace = tempfile::tempdir()?;
        let input = workspace.path().join("original");
        let destination = workspace.path().join("generated");
        fs::create_dir(&input)?;
        let source = "# Orders モジュール仕様書\n\n## 参照モジュール\n\n| モジュール名 | アクション |\n|---|---|\n| Shared | Save |\n| Missing | Run |\n";
        fs::write(input.join("Orders.md"), source)?;
        fs::write(
            input.join("Shared.md"),
            "# Extension 仕様書: Shared\n\n## 使っているモジュール (1)\n\n| モジュール | 使っているアクション |\n|---|---|\n| Orders | Save |",
        )?;
        assert!(compile_oml_wiki(&input, &input.join("unsafe")).is_err());
        let report = compile_oml_wiki(&input, &destination)?;
        assert_eq!(
            (
                report.modules,
                report.resolved_references,
                report.unresolved_references
            ),
            (2, 2, 1)
        );
        assert_eq!(fs::read_to_string(input.join("Orders.md"))?, source);
        assert!(compile_oml_wiki(&input, &destination).is_err());

        let mut hub = Hub::open(&workspace.path().join("state"))?;
        hub.register("oml", "OML Wiki", &destination)?;
        hub.scan("oml")?;
        let shared = hub
            .search("oml", "Shared")?
            .into_iter()
            .find(|hit| hit.title == "Shared")
            .context("Shared Wiki page missing")?;
        assert_eq!(hub.backlinks("oml", &shared.path)?.len(), 2);
        assert_eq!(hub.graph("oml", Some(&shared.path), 50)?.edges.len(), 3);
        Ok(())
    }
}
