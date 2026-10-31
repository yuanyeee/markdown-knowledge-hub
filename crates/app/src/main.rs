use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
use clap::{Parser, Subcommand};
use hub_core::Hub;
use serde::Deserialize;
use std::path::PathBuf;
mod admin;

#[derive(Parser)]
#[command(about = "読み取り専用Markdown索引基盤")]
struct Args {
    #[arg(long, default_value = ".hub")]
    data: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Add {
        id: String,
        name: String,
        root: PathBuf,
    },
    Projects,
    Scan {
        project: String,
        #[arg(long)]
        verify: bool,
    },
    Update {
        project: String,
        #[arg(required = true)]
        paths: Vec<String>,
    },
    Search {
        project: String,
        query: String,
    },
    Backlinks {
        project: String,
        path: String,
    },
    Watch {
        project: String,
    },
    Serve {
        #[arg(long)]
        admin: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut hub = Hub::open(&args.data)?;
    let value = match args.command {
        Command::Add { id, name, root } => {
            hub.register(&id, &name, &root)?;
            serde_json::json!({"registered":id})
        }
        Command::Projects => serde_json::to_value(hub.projects()?)?,
        Command::Scan { project, verify } => {
            serde_json::to_value(hub.scan_verified(&project, verify)?)?
        }
        Command::Update { project, paths } => {
            serde_json::to_value(hub.scan_paths(&project, &paths)?)?
        }
        Command::Search { project, query } => serde_json::to_value(hub.search(&project, &query)?)?,
        Command::Backlinks { project, path } => {
            serde_json::to_value(hub.backlinks(&project, &path)?)?
        }
        Command::Watch { project } => {
            hub.watch(&project)?;
            return Ok(());
        }
        Command::Serve { admin } => {
            drop(hub);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:3847").await?;
            println!("閲覧API: http://127.0.0.1:3847 （loopback限定）");
            axum::serve(listener, app_router(args.data, admin)).await?;
            return Ok(());
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn router(data: PathBuf) -> Router {
    Router::new()
        .route("/api/projects", get(projects))
        .route("/api/search", get(search))
        .route("/api/document", get(document))
        .route("/api/backlinks", get(backlinks))
        .route("/api/links", get(links))
        .route("/api/document-links", get(document_links))
        .route("/api/stats", get(stats))
        .route("/api/graph", get(graph))
        .with_state(data)
}
fn app_router(data: PathBuf, admin_enabled: bool) -> Router {
    let app = router(data.clone()).route(
        "/api/capabilities",
        get(move || async move {
            Json(serde_json::json!({"admin":admin_enabled,"folder_picker":cfg!(windows)}))
        }),
    );
    if admin_enabled {
        app.merge(admin::router(data))
    } else {
        app
    }
}
async fn stats(State(data): State<PathBuf>, Query(p): Query<Parameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(h.stats(&p.project)?)?)
    })
    .await
}
async fn graph(State(data): State<PathBuf>, Query(p): Query<Parameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(h.graph(
            &p.project,
            if p.path.is_empty() {
                None
            } else {
                Some(&p.path)
            },
            200,
        )?)?)
    })
    .await
}
#[derive(Deserialize)]
struct Parameters {
    project: String,
    #[serde(default)]
    q: String,
    #[serde(default)]
    path: String,
}
#[derive(Deserialize)]
struct LinkParameters {
    project: String,
    path: String,
    #[serde(default)]
    offset: usize,
    #[serde(default = "default_link_limit")]
    limit: usize,
}
fn default_link_limit() -> usize {
    100
}
type ApiResult = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;
async fn run(
    data: PathBuf,
    operation: impl FnOnce(Hub) -> Result<serde_json::Value> + Send + 'static,
) -> ApiResult {
    match tokio::task::spawn_blocking(move || operation(Hub::open(&data)?)).await {
        Ok(Ok(value)) => Ok(Json(value)),
        Ok(Err(error)) => {
            eprintln!("API: {error:#}");
            Err((
                StatusCode::BAD_REQUEST,
                Json(
                    serde_json::json!({"error":"指定されたプロジェクト・文書・検索条件を確認してください"}),
                ),
            ))
        }
        Err(error) => {
            eprintln!("worker: {error}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error":"処理を完了できませんでした"})),
            ))
        }
    }
}
async fn projects(State(data): State<PathBuf>) -> ApiResult {
    run(data, |h| Ok(serde_json::to_value(h.projects()?)?)).await
}
async fn search(State(data): State<PathBuf>, Query(p): Query<Parameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(h.search(&p.project, &p.q)?)?)
    })
    .await
}
async fn document(State(data): State<PathBuf>, Query(p): Query<Parameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(h.document(&p.project, &p.path)?)?)
    })
    .await
}
async fn backlinks(State(data): State<PathBuf>, Query(p): Query<Parameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(h.backlinks(&p.project, &p.path)?)?)
    })
    .await
}
async fn links(State(data): State<PathBuf>, Query(p): Query<Parameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(h.links(&p.project)?)?)
    })
    .await
}
async fn document_links(State(data): State<PathBuf>, Query(p): Query<LinkParameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(
            h.links_page(&p.project, &p.path, p.offset, p.limit)?,
        )?)
    })
    .await
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
    async fn default_server_does_not_expose_management_routes() {
        let data = tempfile::tempdir().unwrap();
        let app = app_router(data.path().to_path_buf(), false);
        let response = app
            .oneshot(
                Request::post("/api/admin/remove")
                    .header("origin", "http://127.0.0.1:5173")
                    .header("x-hub-admin", "1")
                    .header("content-type", "application/json")
                    .body(Body::from("{\"project\":\"demo\"}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    #[tokio::test]
    async fn read_api_and_invalid_path() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# API\n注文 [[b]]").unwrap();
        std::fs::write(source.path().join("b.md"), "# 顧客").unwrap();
        let mut hub = Hub::open(data.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        let app = router(data.path().to_path_buf());
        for uri in [
            "/api/projects",
            "/api/search?project=demo&q=API",
            "/api/document?project=demo&path=a.md",
            "/api/backlinks?project=demo&path=b.md",
            "/api/links?project=demo",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            assert!(serde_json::from_slice::<serde_json::Value>(&body).is_ok());
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/document?project=demo&path=../secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
    #[tokio::test]
    async fn document_links_are_scoped_and_paginated() {
        let source = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "[[b]] [[c]]").unwrap();
        std::fs::write(source.path().join("b.md"), "[[a]]").unwrap();
        let mut hub = Hub::open(data.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        let app = router(data.path().to_path_buf());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/document-links?project=demo&path=a.md&limit=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(value["items"].as_array().unwrap().len(), 1);
        assert_eq!(value["items"][0]["source"], "a.md");
        assert_eq!(value["next_offset"], 1);
        for query in [
            "limit=0",
            "limit=201",
            "offset=-1",
            "offset=18446744073709551615",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!(
                            "/api/document-links?project=demo&path=a.md&{query}"
                        ))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
    }
}
