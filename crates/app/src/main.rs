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
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Projects,
    MoveOutput {
        project: String,
        output: PathBuf,
    },
    OpenOutput {
        output: PathBuf,
    },
    ChangeInput {
        project: String,
        root: PathBuf,
    },
    RetiredOutputs,
    CleanupOutput {
        key: String,
    },
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
    Rebuild {
        project: String,
    },
    Backup {
        destination: PathBuf,
    },
    Restore {
        backup: PathBuf,
        destination: PathBuf,
    },
    Serve {
        #[arg(long, default_value_t = 3847)]
        port: u16,
        #[arg(long)]
        admin: bool,
        #[arg(long)]
        web_dir: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut hub = Hub::open(&args.data)?;
    let value = match args.command {
        Command::Add {
            id,
            name,
            root,
            output,
        } => {
            if let Some(output) = output {
                hub.register_with_output(&id, &name, &root, &output)?;
            } else {
                hub.register(&id, &name, &root)?;
            }
            serde_json::json!({"registered":id})
        }
        Command::MoveOutput { project, output } => {
            serde_json::to_value(hub.move_output(&project, &output)?)?
        }
        Command::OpenOutput { output } => serde_json::json!({"id": hub.import_output(&output)?}),
        Command::ChangeInput { project, root } => {
            hub.change_input(&project, &root)?;
            serde_json::json!({"id": project})
        }
        Command::RetiredOutputs => serde_json::to_value(hub.retired_outputs()?)?,
        Command::CleanupOutput { key } => {
            hub.cleanup_retired(&key)?;
            serde_json::json!({"removed": key})
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
        Command::Rebuild { project } => serde_json::to_value(hub.rebuild(&project)?)?,
        Command::Backup { destination } => {
            hub.backup(&destination)?;
            serde_json::json!({"backup":destination})
        }
        Command::Restore {
            backup,
            destination,
        } => {
            Hub::open(&backup)?.backup(&destination)?;
            serde_json::json!({"restored":destination})
        }
        Command::Serve {
            admin,
            web_dir,
            port,
        } => {
            drop(hub);
            let mut app = app_router(args.data.clone(), admin);
            if let Some(path) = web_dir {
                anyhow::ensure!(
                    path.join("index.html").is_file(),
                    "Web本番ビルドのindex.htmlがありません"
                );
                app = app.fallback_service(tower_http::services::ServeDir::new(path));
            }
            let listener =
                tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
            println!("閲覧API: http://127.0.0.1:{port} （loopback限定）");
            axum::serve(listener, app).await?;
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
        .route("/api/search-page", get(search_page))
        .route("/api/backlinks-page", get(backlinks_page))
        .route("/api/document", get(document))
        .route("/api/backlinks", get(backlinks))
        .route("/api/links", get(links))
        .route("/api/link-target", get(link_target))
        .route("/api/document-links", get(document_links))
        .route("/api/stats", get(stats))
        .route("/api/graph", get(graph))
        .route("/api/graph-summary", get(graph_summary))
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
async fn graph_summary(State(data): State<PathBuf>, Query(p): Query<Parameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(h.graph_summary(&p.project)?)?)
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
    #[serde(default)]
    target: String,
}
#[derive(Deserialize)]
struct LinkParameters {
    project: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    q: String,
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
async fn link_target(State(data): State<PathBuf>, Query(p): Query<Parameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(
            h.link_target(&p.project, &p.path, &p.target)?,
        )?)
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
async fn search_page(State(data): State<PathBuf>, Query(p): Query<LinkParameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(
            h.search_page(&p.project, &p.q, p.offset, p.limit)?,
        )?)
    })
    .await
}
async fn backlinks_page(State(data): State<PathBuf>, Query(p): Query<LinkParameters>) -> ApiResult {
    run(data, move |h| {
        Ok(serde_json::to_value(
            h.backlinks_page(&p.project, &p.path, p.offset, p.limit)?,
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
            "/api/search-page?project=demo&q=API&limit=1",
            "/api/backlinks-page?project=demo&path=b.md&limit=1",
            "/api/link-target?project=demo&path=a.md&target=b",
            "/api/graph-summary?project=demo",
            "/api/stats?project=demo",
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
