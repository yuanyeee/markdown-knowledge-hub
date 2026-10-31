use crate::{ApiResult, run};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct AdminState {
    data: PathBuf,
    serial: Arc<Mutex<()>>,
}
#[derive(Deserialize)]
struct Create {
    id: String,
    name: String,
    root: String,
}
#[derive(Deserialize)]
struct Target {
    project: String,
}
fn denied() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"error":"ローカル管理画面から操作してください"})),
    )
}
fn authorize(headers: &HeaderMap) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if headers.get("x-hub-admin").and_then(|v| v.to_str().ok()) != Some("1") {
        return Err(denied());
    }
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(denied)?;
    if ![
        "http://127.0.0.1:5173",
        "http://localhost:5173",
        "http://127.0.0.1:3847",
        "http://localhost:3847",
    ]
    .contains(&origin)
    {
        return Err(denied());
    }
    Ok(())
}
pub fn router(data: PathBuf) -> Router {
    Router::new()
        .route("/api/admin/create", post(create))
        .route("/api/admin/remove", post(remove))
        .route("/api/admin/rebuild", post(rebuild))
        .route("/api/admin/pick-folder", post(pick_folder))
        .with_state(AdminState {
            data,
            serial: Arc::new(Mutex::new(())),
        })
}
async fn create(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(p): Json<Create>,
) -> ApiResult {
    authorize(&headers)?;
    run(state.data, move |mut h| {
        let _guard = state
            .serial
            .lock()
            .map_err(|_| anyhow::anyhow!("管理操作を再試行してください"))?;
        h.register(&p.id, &p.name, &PathBuf::from(p.root))?;
        match h.scan(&p.id) {
            Ok(report) => Ok(serde_json::json!({"id":p.id,"report":report})),
            Err(e) => {
                if let Err(cleanup) = h.remove(&p.id) {
                    eprintln!("作成取消に失敗: {cleanup:#}");
                }
                Err(e)
            }
        }
    })
    .await
}
async fn remove(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(p): Json<Target>,
) -> ApiResult {
    authorize(&headers)?;
    run(state.data, move |mut h| {
        let _guard = state
            .serial
            .lock()
            .map_err(|_| anyhow::anyhow!("管理操作を再試行してください"))?;
        h.remove(&p.project)?;
        Ok(serde_json::json!({"removed":p.project}))
    })
    .await
}
async fn rebuild(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(p): Json<Target>,
) -> ApiResult {
    authorize(&headers)?;
    run(state.data, move |mut h| {
        let _guard = state
            .serial
            .lock()
            .map_err(|_| anyhow::anyhow!("管理操作を再試行してください"))?;
        Ok(serde_json::to_value(h.rebuild(&p.project)?)?)
    })
    .await
}
async fn pick_folder(headers: HeaderMap) -> ApiResult {
    authorize(&headers)?;
    let result = tokio::task::spawn_blocking(choose_folder).await;
    match result {
        Ok(Ok(path)) => Ok(Json(serde_json::json!({"path":path}))),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error":"フォルダ選択が利用できません。パスを直接入力してください"}),
            ),
        )),
    }
}
#[cfg(windows)]
fn choose_folder() -> anyhow::Result<Option<String>> {
    Ok(rfd::FileDialog::new()
        .set_title("Markdownフォルダを選択してください")
        .pick_folder()
        .map(|path| path.to_string_lossy().into_owned()))
}
#[cfg(not(windows))]
fn choose_folder() -> anyhow::Result<Option<String>> {
    anyhow::bail!("このOSではパスを入力してください")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    #[tokio::test]
    async fn failed_creation_rolls_back_derived_data() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("bad.md"), [255]).unwrap();
        let payload =
            serde_json::json!({"id":"demo","name":"Demo","root":source.path()}).to_string();
        let response = router(data.path().to_path_buf())
            .oneshot(
                Request::post("/api/admin/create")
                    .header("content-type", "application/json")
                    .header("origin", "http://127.0.0.1:5173")
                    .header("x-hub-admin", "1")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            crate::Hub::open(data.path())
                .unwrap()
                .projects()
                .unwrap()
                .is_empty()
        );
        assert!(source.path().join("bad.md").exists());
        assert!(!data.path().join("indexes/demo").exists());
    }
    #[tokio::test]
    async fn managed_create_rebuild_remove_and_origin_protection() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# 原本").unwrap();
        let app = router(data.path().to_path_buf());
        let payload =
            serde_json::json!({"id":"demo","name":"Demo","root":source.path()}).to_string();
        let missing_header = app
            .clone()
            .oneshot(
                Request::post("/api/admin/create")
                    .header("content-type", "application/json")
                    .header("origin", "http://127.0.0.1:5173")
                    .body(Body::from(payload.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_header.status(), StatusCode::FORBIDDEN);
        for origin in ["https://evil.example", "null", ""] {
            let response = app
                .clone()
                .oneshot(
                    Request::post("/api/admin/create")
                        .header("content-type", "application/json")
                        .header("origin", origin)
                        .header("x-hub-admin", "1")
                        .body(Body::from(payload.clone()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        for (endpoint, body) in [
            ("create", payload),
            ("rebuild", "{\"project\":\"demo\"}".into()),
            ("remove", "{\"project\":\"demo\"}".into()),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::post(format!("/api/admin/{endpoint}"))
                        .header("content-type", "application/json")
                        .header("origin", "http://127.0.0.1:5173")
                        .header("x-hub-admin", "1")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{endpoint}: {:?}",
                to_bytes(response.into_body(), 1024 * 1024).await.unwrap()
            );
        }
        assert_eq!(
            std::fs::read_to_string(source.path().join("a.md")).unwrap(),
            "# 原本"
        );
    }
}
