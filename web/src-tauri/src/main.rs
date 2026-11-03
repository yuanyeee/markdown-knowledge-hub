#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{Manager, State};
mod oml_import;
struct DesktopState {
    data: Arc<PathBuf>,
    watches: Arc<Mutex<HashMap<String, hub_core::WatchHandle>>>,
}
fn data_override(args: &[String]) -> Result<Option<PathBuf>, std::io::Error> {
    let Some(index) = args.iter().position(|a| a == "--data-dir") else {
        return Ok(None);
    };
    let path = args
        .get(index + 1)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "--data-dirには保存先の絶対パスを指定してください",
            )
        })?;
    Ok(Some(path))
}

#[tauri::command]
async fn hub_request(
    window: tauri::WebviewWindow,
    state: State<'_, DesktopState>,
    operation: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = window.url().map_err(|e| e.to_string())?;
    let bundled = url.scheme() == "tauri"
        || (matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost"));
    let development =
        cfg!(debug_assertions) && url.host_str() == Some("127.0.0.1") && url.port() == Some(5173);
    if window.label() != "main" || (!bundled && !development) {
        return Err("アプリ内の画面から操作してください".into());
    }
    let data = state.data.clone();
    let watches = state.watches.clone();
    tauri::async_runtime::spawn_blocking(move||{
        if operation=="admin/pick-folder" {
            let purpose=args.get("purpose").and_then(|v|v.as_str()).unwrap_or("フォルダー");
            return Ok(serde_json::json!({"path":rfd::FileDialog::new().set_title(format!("{purpose}を選択してください")).pick_folder().map(|p|p.to_string_lossy().into_owned())}));
        }
        if operation=="admin/pick-md-files" {
            return Ok(serde_json::json!({"paths":rfd::FileDialog::new().set_title("Markdownファイルを選択").add_filter("Markdown", &["md"]).pick_files()}));
        }
        if operation=="admin/pick-oml-files" {
            return Ok(serde_json::json!({"paths":rfd::FileDialog::new().set_title("OMLファイルを選択").add_filter("OutSystems OML", &["oml"]).pick_files()}));
        }
        let path=data.as_ref();
        if operation=="capabilities" {
            return Ok(serde_json::json!({"admin":true,"folder_picker":true,"desktop":true}));
        }
        if operation=="watch/status" || operation=="admin/watch-start" || operation=="admin/watch-stop" || operation=="admin/remove" {
            let id=args.get("project").and_then(|v|v.as_str()).ok_or("プロジェクトを指定してください")?;
            let mut handles=watches.lock().map_err(|_|"監視操作を再試行してください")?;
            if operation=="admin/watch-stop" || operation=="admin/remove" { handles.remove(id); }
            if operation=="admin/watch-start" && !handles.get(id).is_some_and(|h|h.status().running) {
                handles.remove(id);
                handles.insert(id.to_string(),hub_core::WatchHandle::start(path,id).map_err(|e|e.to_string())?);
            }
            if operation!="admin/remove" {
                return Ok(handles.get(id).map(|h|serde_json::to_value(h.status()).unwrap()).unwrap_or(serde_json::json!({"running":false,"error":null,"updates":0})));
            }
        }
        let mut hub=hub_core::Hub::open(path).map_err(|e|e.to_string())?;
        if operation=="admin/create" && let Some(files)=args.get("oml_files") {
            return oml_import::create(&mut hub, args.clone(), files).map_err(|e|format!("{e:#}"));
        }
        if operation=="admin/move-output" || operation=="admin/change-input" {
            let id=args.get("project").and_then(|v|v.as_str()).ok_or("プロジェクトを指定してください")?;
            // Keep the watcher control mutex until the switch and restart finish, so
            // a concurrent start cannot reopen the old location mid-migration.
            let mut handles=watches.lock().map_err(|_|"監視操作を再試行してください")?;
            let restart=handles.get(id).is_some_and(|h|h.status().running);
            handles.remove(id);
            let result=hub_service::call(&mut hub,&operation,&args).map_err(|e|e.to_string());
            if restart {
                let handle=hub_core::WatchHandle::start(path,id).map_err(|e|format!("監視を再開できません。監視を開始し直してください: {e}"));
                match handle {
                    Ok(handle)=>{handles.insert(id.to_string(),handle);},
                    Err(error)=>{
                        // The output operation may already have committed. Preserve
                        // that result and make a restart failure visible to the UI.
                        return match result {Ok(mut value)=>{value["warning"]=serde_json::json!(error);Ok(value)},Err(original)=>Err(format!("{original} / {error}"))};
                    }
                }
            }
            return result;
        }
        hub_service::call(&mut hub,&operation,&args).map_err(|e|e.to_string())
    }).await.map_err(|e|e.to_string())?
}

fn main() {
    let result = tauri::Builder::default()
        .setup(|app| {
            let args: Vec<_> = std::env::args().collect();
            let data = data_override(&args)?.unwrap_or(app.path().app_local_data_dir()?);
            hub_core::Hub::open(&data)?;
            app.manage(DesktopState {
                data: Arc::new(data),
                watches: Arc::new(Mutex::new(HashMap::new())),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![hub_request])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        rfd::MessageDialog::new()
            .set_title("Markdown Knowledge Hub 起動エラー")
            .set_description(error.to_string())
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storage_override_requires_an_explicit_absolute_path() {
        assert!(data_override(&["app.exe".into()]).unwrap().is_none());
        assert!(data_override(&["app.exe".into(), "--data-dir".into()]).is_err());
        assert!(data_override(&["--data-dir".into(), "relative".into()]).is_err());
        assert_eq!(
            data_override(&["--data-dir".into(), r"D:\HubState".into()]).unwrap(),
            Some(PathBuf::from(r"D:\HubState"))
        );
    }
}
