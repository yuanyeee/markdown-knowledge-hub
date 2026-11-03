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
    references: Vec<(String, usize, bool)>,
}

fn parse(path: PathBuf, source: String) -> Option<Spec> {
    let mut name = None;
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
                    name = title
                        .strip_suffix(" モジュール仕様書")
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
            if let Some(spec) = parse(relative, fs::read_to_string(entry.path())?) {
                specs.push(spec);
            }
        }
    }
    ensure!(
        !specs.is_empty(),
        "モジュール見出しのあるMarkdownが見つかりません"
    );

    let mut by_name = BTreeMap::<&str, Vec<usize>>::new();
    for (id, spec) in specs.iter().enumerate() {
        by_name.entry(&spec.name).or_default().push(id);
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
        .map(|spec| crate::oml_elements::extract(&spec.source))
        .collect();
    let mut actions = BTreeMap::<(usize, &str), Vec<usize>>::new();
    for (id, elements) in extracted.iter().enumerate() {
        for (number, element) in elements.iter().enumerate() {
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
        for (number, element) in elements.iter().enumerate() {
            for anchor in &element.anchors {
                anchors.entry(anchor.clone()).or_default().push(number + 1);
            }
        }
        if !elements.is_empty() {
            let module_page = page(id);
            let folder = module_page.trim_end_matches(".md");
            fs::create_dir(destination.join(folder))?;
            body.push_str("\n## 要素別Wiki\n\n");
            for (number, element) in elements.iter().enumerate() {
                let title = format!("{} · {}", element.kind, literal(&element.title));
                let relative = format!("{folder}/{}.md", number + 1);
                body.push_str(&format!("- [{title}]({relative})\n"));
                let mut content = format!(
                    "# {title}\n\n[元仕様書ページ](../{module_page}) · {} {} 行\n\n{}",
                    literal(&spec.path.to_string_lossy()),
                    element.line,
                    crate::oml_elements::route_fragments(element.body, &anchors, "")
                );
                content.push_str("\n<a id=\"oml-call\"></a>\n");
                if element.kind == "画面"
                    || element.kind.contains("アクション")
                    || element.kind == "関数"
                {
                    let calls = crate::oml_elements::calls(element.body);
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
                fs::write(destination.join(relative), content)?;
                report.elements += 1;
            }
        }
        body.push_str("\n## 元仕様書\n\n");
        let module_page = page(id);
        let prefix = format!("{}/", module_page.trim_end_matches(".md"));
        body.push_str(&crate::oml_elements::route_fragments(
            &spec.source,
            &anchors,
            &prefix,
        ));
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
