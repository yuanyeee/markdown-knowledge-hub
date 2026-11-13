use anyhow::{Context, Result, bail};
use hub_core::Hub;
use serde_json::{Value, json};
use std::path::Path;

fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("{key}が指定されていません"))
}
fn number(args: &Value, key: &str, default: usize) -> Result<usize> {
    match args.get(key) {
        None => Ok(default),
        Some(Value::String(s)) => Ok(s.parse()?),
        Some(v) => Ok(usize::try_from(
            v.as_u64().context("ページ指定が不正です")?,
        )?),
    }
}
/// Explicit UI operations only; never accepts file reads or executable commands.
pub fn call(hub: &mut Hub, operation: &str, args: &Value) -> Result<Value> {
    Ok(match operation {
        "projects" => serde_json::to_value(hub.projects()?)?,
        "link-target" => serde_json::to_value(hub.link_target(
            text(args, "project")?,
            text(args, "path")?,
            text(args, "target")?,
        )?)?,
        "search-page" => serde_json::to_value(hub.search_page_scoped(
            text(args, "project")?,
            args.get("q").and_then(Value::as_str).unwrap_or(""),
            args.get("source").and_then(Value::as_str).unwrap_or(""),
            number(args, "offset", 0)?,
            number(args, "limit", 50)?,
        )?)?,
        "oml-catalog" => serde_json::to_value(hub.oml_catalog(
            text(args, "project")?,
            args.get("path").and_then(Value::as_str).unwrap_or(""),
        )?)?,
        "wiki-view" => {
            let project = text(args, "project")?;
            let path = text(args, "path")?;
            json!({
                "document": hub.reading_document(project, path, args.get("fragment").and_then(Value::as_str).unwrap_or(""))?,
                "backlinks": hub.backlink_details(project, path, 0, 100)?,
                "links": hub.links_page(project, path, 0, 100)?,
            })
        }
        "backlink-details" => serde_json::to_value(hub.backlink_details(
            text(args, "project")?,
            text(args, "path")?,
            number(args, "offset", 0)?,
            number(args, "limit", 100)?,
        )?)?,
        "backlinks-page" => serde_json::to_value(hub.backlinks_page(
            text(args, "project")?,
            text(args, "path")?,
            number(args, "offset", 0)?,
            number(args, "limit", 100)?,
        )?)?,
        "search" => serde_json::to_value(hub.search(
            text(args, "project")?,
            args.get("q").and_then(Value::as_str).unwrap_or(""),
        )?)?,
        "document" => {
            serde_json::to_value(hub.document(text(args, "project")?, text(args, "path")?)?)?
        }
        "backlinks" => {
            serde_json::to_value(hub.backlinks(text(args, "project")?, text(args, "path")?)?)?
        }
        "document-links" => serde_json::to_value(hub.links_page(
            text(args, "project")?,
            text(args, "path")?,
            number(args, "offset", 0)?,
            number(args, "limit", 100)?,
        )?)?,
        "stats" => serde_json::to_value(hub.stats(text(args, "project")?)?)?,
        "diagnostics-page" => serde_json::to_value(hub.diagnostics_page(
            text(args, "project")?,
            text(args, "kind")?,
            number(args, "offset", 0)?,
            number(args, "limit", 100)?,
        )?)?,
        "graph-summary" => serde_json::to_value(hub.graph_summary(text(args, "project")?)?)?,
        "graph" => serde_json::to_value(
            hub.graph(
                text(args, "project")?,
                args.get("path")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty()),
                200,
            )?,
        )?,
        "admin/create" => {
            let id = text(args, "id")?;
            let original = Path::new(text(args, "root")?);
            let output = args.get("output").and_then(Value::as_str).map(Path::new);
            let generated = if let Some(files) = args.get("md_files") {
                let files: Vec<std::path::PathBuf> = serde_json::from_value(files.clone())?;
                Some(hub.prepare_markdown_selection(
                    id,
                    &files,
                    output.context("出力の親フォルダーを選択してください")?,
                )?)
            } else if args.get("oml").and_then(Value::as_bool) == Some(true) {
                let parent = output.context("出力の親フォルダーを選択してください")?;
                Some(hub.prepare_oml_wiki(id, original, parent)?)
            } else {
                None
            };
            let root = generated.as_deref().unwrap_or(original);
            if let Some(output) = output {
                hub.register_with_output(id, text(args, "name")?, root, output)?;
            } else {
                hub.register(id, text(args, "name")?, root)?;
            }
            match hub.scan(id) {
                Ok(report) => json!({"id":id,"report":report}),
                Err(error) => {
                    if let Err(cleanup) = hub.remove(id) {
                        eprintln!("作成取消に失敗: {cleanup:#}");
                    }
                    return Err(error);
                }
            }
        }
        "output-settings" => json!({"last_output": hub.last_output_parent()?}),
        "retired-outputs" => serde_json::to_value(hub.retired_outputs()?)?,
        "admin/move-output" => serde_json::to_value(
            hub.move_output(text(args, "project")?, Path::new(text(args, "output")?))?,
        )?,
        "admin/open-output" => json!({"id": hub.import_output(Path::new(text(args, "output")?))?}),
        "admin/change-input" => {
            hub.change_input(text(args, "project")?, Path::new(text(args, "root")?))?;
            json!({"id": text(args, "project")?})
        }
        "admin/cleanup-output" => {
            hub.cleanup_retired(text(args, "key")?)?;
            json!({"removed": text(args, "key")?})
        }
        "admin/rebuild" => serde_json::to_value(hub.rebuild(text(args, "project")?)?)?,
        "admin/remove" => {
            let id = text(args, "project")?;
            hub.remove(id)?;
            json!({"removed":id})
        }
        _ => bail!("未対応の操作です: {operation}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn selected_markdown_keeps_relative_links_and_excludes_unselected_files() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let input = temp.path().join("source");
        let output = temp.path().join("output");
        std::fs::create_dir_all(input.join("sub"))?;
        std::fs::create_dir(&output)?;
        let first = input.join("a.md");
        let second = input.join("sub/b.md");
        std::fs::write(&first, "# Alpha\n[Beta](sub/b.md)")?;
        std::fs::write(&second, "# Beta")?;
        std::fs::write(input.join("excluded.md"), "# Excluded")?;
        let mut hub = Hub::open(&temp.path().join("settings"))?;
        call(
            &mut hub,
            "admin/create",
            &json!({
                "id": "selected", "name": "Selection", "root": "", "output": output,
                "md_files": [first, second]
            }),
        )?;
        assert_eq!(hub.backlinks("selected", "sub/b.md")?, vec!["a.md"]);
        assert!(hub.search("selected", "Excluded")?.is_empty());
        Ok(())
    }

    #[test]
    fn oml_creation_registers_the_generated_wiki() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let originals = temp.path().join("originals");
        let output = temp.path().join("output");
        std::fs::create_dir(&originals)?;
        std::fs::create_dir(&output)?;
        let source = "# Orders モジュール仕様書";
        std::fs::write(originals.join("orders.md"), source)?;
        let mut hub = Hub::open(&temp.path().join("settings"))?;
        call(
            &mut hub,
            "admin/create",
            &json!({
                "id": "oml", "name": "OML Wiki", "root": originals, "output": output, "oml": true
            }),
        )?;
        assert!(
            hub.search("oml", "Orders")?
                .iter()
                .any(|hit| hit.title == "Orders")
        );
        assert_eq!(
            std::fs::read_to_string(originals.join("orders.md"))?,
            source
        );
        Ok(())
    }

    #[test]
    fn external_output_lifecycle_keeps_cached_documents_without_source() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# Cached document").unwrap();
        let mut hub = Hub::open(state.path()).unwrap();
        call(
            &mut hub,
            "admin/create",
            &json!({"id":"demo","name":"Demo","root":source.path(),"output":first.path()}),
        )
        .unwrap();
        assert!(call(&mut hub, "output-settings", &json!({})).unwrap()["last_output"].is_string());
        let report = call(
            &mut hub,
            "admin/move-output",
            &json!({"project":"demo","output":second.path()}),
        )
        .unwrap();
        let output = report["new_output"].as_str().unwrap();
        assert!(Path::new(output).exists());
        let retired = call(&mut hub, "retired-outputs", &json!({})).unwrap();
        assert_eq!(retired.as_array().unwrap().len(), 1);
        call(
            &mut hub,
            "admin/cleanup-output",
            &json!({"key":retired[0]["key"]}),
        )
        .unwrap();
        assert!(hub.retired_outputs().unwrap().is_empty());
        drop(source);
        let imported_state = tempfile::tempdir().unwrap();
        let mut imported = Hub::open(imported_state.path()).unwrap();
        assert_eq!(
            call(
                &mut imported,
                "admin/open-output",
                &json!({"output":output})
            )
            .unwrap()["id"],
            "demo"
        );
        assert_eq!(
            call(
                &mut imported,
                "document",
                &json!({"project":"demo","path":"a.md"})
            )
            .unwrap()["title"],
            "Cached document"
        );
        let replacement = tempfile::tempdir().unwrap();
        std::fs::write(replacement.path().join("a.md"), "# Replacement").unwrap();
        call(
            &mut imported,
            "admin/change-input",
            &json!({"project":"demo","root":replacement.path()}),
        )
        .unwrap();
        call(&mut imported, "admin/rebuild", &json!({"project":"demo"})).unwrap();
        assert_eq!(
            call(
                &mut imported,
                "document",
                &json!({"project":"demo","path":"a.md"})
            )
            .unwrap()["title"],
            "Replacement"
        );
    }
    #[test]
    fn desktop_operations_share_core_and_preserve_sources() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# 注文\n[[b]]").unwrap();
        std::fs::write(source.path().join("b.md"), "# 顧客").unwrap();
        let mut hub = hub_core::Hub::open(state.path()).unwrap();
        call(
            &mut hub,
            "admin/create",
            &json!({"id":"demo","name":"Demo","root":source.path()}),
        )
        .unwrap();
        assert_eq!(
            call(&mut hub, "projects", &json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            call(&mut hub, "search", &json!({"project":"demo","q":"注文"})).unwrap()[0]["path"],
            "a.md"
        );
        assert_eq!(
            call(
                &mut hub,
                "document",
                &json!({"project":"demo","path":"a.md"})
            )
            .unwrap()["title"],
            "注文"
        );
        assert_eq!(
            call(
                &mut hub,
                "document-links",
                &json!({"project":"demo","path":"a.md","offset":"0","limit":"100"})
            )
            .unwrap()["items"][0]["resolved"],
            "b.md"
        );
        call(&mut hub, "admin/rebuild", &json!({"project":"demo"})).unwrap();
        call(&mut hub, "admin/remove", &json!({"project":"demo"})).unwrap();
        assert!(call(&mut hub, "shell", &json!({"command":"anything"})).is_err());
        assert!(source.path().join("a.md").exists());
    }
}
