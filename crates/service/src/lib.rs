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
            hub.register(id, text(args, "name")?, Path::new(text(args, "root")?))?;
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
