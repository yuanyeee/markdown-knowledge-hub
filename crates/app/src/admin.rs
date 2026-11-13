use crate::ApiResult;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct AdminState {
    data: PathBuf,
    serial: Arc<Mutex<()>>,
    port: u16,
}
fn denied() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"error":"ローカル管理画面から操作してください"})),
    )
}
fn authorize(headers: &HeaderMap, port: u16) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if headers.get("x-hub-admin").and_then(|v| v.to_str().ok()) != Some("1") {
        return Err(denied());
    }
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(denied)?;
    let allowed = [
        "http://127.0.0.1:5173".to_string(),
        "http://localhost:5173".to_string(),
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
    ];
    if !allowed.iter().any(|candidate| candidate == origin) {
        return Err(denied());
    }
    Ok(())
}
// These read-only endpoints use a custom header to prevent cross-origin simple
// requests. Same-origin browser GET requests need not include an Origin header.
fn authorize_read(
    headers: &HeaderMap,
    port: u16,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if headers.get("x-hub-admin").and_then(|v| v.to_str().ok()) != Some("1") {
        return Err(denied());
    }
    if headers.contains_key("origin") {
        authorize(headers, port)?;
    }
    Ok(())
}
#[cfg(test)]
pub fn router(data: PathBuf) -> Router {
    router_with_port(data, 3847)
}
pub fn router_with_port(data: PathBuf, port: u16) -> Router {
    Router::new()
        .route("/api/admin/{operation}", post(manage))
        .route("/api/output-settings", get(output_settings))
        .route("/api/retired-outputs", get(retired_outputs))
        .route("/api/admin/pick-folder", post(pick_folder))
        .with_state(AdminState {
            data,
            serial: Arc::new(Mutex::new(())),
            port,
        })
}
async fn service(state: AdminState, operation: String, args: serde_json::Value) -> ApiResult {
    let result = tokio::task::spawn_blocking(move || {
        let _guard = state
            .serial
            .lock()
            .map_err(|_| anyhow::anyhow!("management lock unavailable"))?;
        let mut hub = crate::Hub::open(&state.data)?;
        hub_service::call(&mut hub, &operation, &args)
    })
    .await;
    match result {
        Ok(Ok(value)) => Ok(Json(value)),
        Ok(Err(error)) => {
            eprintln!("management API: {error:#}");
            let message = crate::client_error_message(
                &error,
                "フォルダーとプロジェクト設定を確認してください",
            );
            Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": message})),
            ))
        }
        Err(error) => {
            eprintln!("management worker: {error}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error":"処理を完了できませんでした"})),
            ))
        }
    }
}
async fn manage(
    State(state): State<AdminState>,
    Path(operation): Path<String>,
    headers: HeaderMap,
    Json(args): Json<serde_json::Value>,
) -> ApiResult {
    authorize(&headers, state.port)?;
    if ![
        "create",
        "remove",
        "rebuild",
        "move-output",
        "open-output",
        "change-input",
        "cleanup-output",
    ]
    .contains(&operation.as_str())
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":"Unknown operation"})),
        ));
    }
    service(state, format!("admin/{operation}"), args).await
}
async fn output_settings(State(state): State<AdminState>, headers: HeaderMap) -> ApiResult {
    authorize_read(&headers, state.port)?;
    service(state, "output-settings".into(), serde_json::json!({})).await
}
async fn retired_outputs(State(state): State<AdminState>, headers: HeaderMap) -> ApiResult {
    authorize_read(&headers, state.port)?;
    service(state, "retired-outputs".into(), serde_json::json!({})).await
}
async fn pick_folder(State(state): State<AdminState>, headers: HeaderMap) -> ApiResult {
    authorize(&headers, state.port)?;
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
    async fn output_routes_require_management_authorization() {
        let data = tempfile::tempdir().unwrap();
        let app = router(data.path().to_path_buf());
        for uri in ["/api/output-settings", "/api/retired-outputs"] {
            for authorized in [false, true] {
                let mut request = Request::get(uri).header("origin", "http://127.0.0.1:5173");
                if authorized {
                    request = request.header("x-hub-admin", "1");
                }
                let response = app
                    .clone()
                    .oneshot(request.body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    if authorized {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    }
                );
            }
            for (admin_header, origin, expected) in [
                (true, None, StatusCode::OK),
                (false, None, StatusCode::FORBIDDEN),
                (true, Some("https://evil.example"), StatusCode::FORBIDDEN),
            ] {
                let mut request = Request::get(uri);
                if admin_header {
                    request = request.header("x-hub-admin", "1");
                }
                if let Some(origin) = origin {
                    request = request.header("origin", origin);
                }
                let response = app
                    .clone()
                    .oneshot(request.body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), expected);
            }
            let response = crate::app_router(data.path().to_path_buf(), false)
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        for op in [
            "move-output",
            "open-output",
            "change-input",
            "cleanup-output",
        ] {
            for origin in ["https://evil.example", "http://127.0.0.1:5173"] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::post(format!("/api/admin/{op}"))
                            .header("origin", origin)
                            .header("x-hub-admin", "1")
                            .header("content-type", "application/json")
                            .body(Body::from("{}"))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    if origin.starts_with("https") {
                        StatusCode::FORBIDDEN
                    } else {
                        StatusCode::BAD_REQUEST
                    }
                );
            }
        }
    }
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

    #[tokio::test]
    async fn custom_serve_port_authorizes_matching_origin_and_surfaces_validation_errors() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# 原本").unwrap();
        let app = crate::app_router_with_port(data.path().to_path_buf(), true, 4915);
        let payload =
            serde_json::json!({"id":"demo","name":"Demo","root":source.path()}).to_string();

        let wrong_port = app
            .clone()
            .oneshot(
                Request::post("/api/admin/create")
                    .header("content-type", "application/json")
                    .header("origin", "http://127.0.0.1:3847")
                    .header("x-hub-admin", "1")
                    .body(Body::from(payload.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wrong_port.status(), StatusCode::FORBIDDEN);

        let custom_port_ok = app
            .clone()
            .oneshot(
                Request::post("/api/admin/create")
                    .header("content-type", "application/json")
                    .header("origin", "http://127.0.0.1:4915")
                    .header("x-hub-admin", "1")
                    .body(Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(custom_port_ok.status(), StatusCode::OK);

        let invalid_id_payload =
            serde_json::json!({"id":"BAD_ID","name":"Demo","root":source.path()}).to_string();
        let validation_err = app
            .oneshot(
                Request::post("/api/admin/create")
                    .header("content-type", "application/json")
                    .header("origin", "http://localhost:4915")
                    .header("x-hub-admin", "1")
                    .body(Body::from(invalid_id_payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(validation_err.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value = serde_json::from_slice(
            &to_bytes(validation_err.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(body["error"].as_str().unwrap().contains("プロジェクトID"));
    }
}
