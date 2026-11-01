//! Read-only production-corpus assessment. Reports aggregates, never document text.
use anyhow::{Result, ensure};
use hub_core::Hub;
use std::{path::Path, time::Instant};
fn fingerprint(root: &Path) -> Result<(String, usize, u64)> {
    let mut paths = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    paths.retain(|p| {
        p.file_type().is_file()
            && p.path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
    });
    paths.sort_by_key(|p| p.path().to_path_buf());
    let mut hash = blake3::Hasher::new();
    let mut bytes = 0;
    for path in &paths {
        let raw = std::fs::read(path.path())?;
        bytes += raw.len() as u64;
        hash.update(path.path().strip_prefix(root)?.to_string_lossy().as_bytes());
        hash.update(&[0]);
        hash.update(blake3::hash(&raw).as_bytes());
    }
    Ok((hash.finalize().to_hex().to_string(), paths.len(), bytes))
}
fn p95(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[(samples.len() * 95).div_ceil(100) - 1]
}
fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    ensure!(args.len() == 4, "corpus ROOT DATA REPORT.json");
    let root = Path::new(&args[1]);
    let data = Path::new(&args[2]);
    let output = Path::new(&args[3]);
    let before = fingerprint(root)?;
    let mut hub = Hub::open(data)?;
    if !hub.projects()?.iter().any(|p| p.id == "real") {
        hub.register("real", "実文書評価", root)?;
    }
    let start = Instant::now();
    let initial = hub.scan("real")?;
    let initial_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    let unchanged = hub.scan("real")?;
    let unchanged_ms = start.elapsed().as_secs_f64() * 1000.;
    ensure!(unchanged.changed == 0, "原本の変更がありました");
    let mut offset = 0;
    let mut docs = vec![];
    loop {
        let page = hub.search_page("real", "", offset, 200)?;
        docs.extend(page.items);
        if let Some(next) = page.next_offset {
            offset = next;
        } else {
            break;
        }
    }
    let mut searches = vec![];
    let mut backs = vec![];
    let mut exact = 0;
    for doc in docs.iter().take(50) {
        let q = doc.title.chars().take(32).collect::<String>();
        let start = Instant::now();
        let hits = hub.search("real", &q)?;
        searches.push(start.elapsed().as_secs_f64() * 1000.);
        exact += usize::from(hits.iter().any(|hit| hit.path == doc.path));
        let start = Instant::now();
        hub.backlinks("real", &doc.path)?;
        backs.push(start.elapsed().as_secs_f64() * 1000.);
    }
    ensure!(!searches.is_empty(), "文書がありません");
    let stats = hub.stats("real")?;
    let graph = hub.graph_summary("real")?;
    let after = fingerprint(root)?;
    ensure!(before == after, "評価中に原本が変化しました");
    let report = serde_json::json!({"corpus":"user-provided, original documents; no source writes","files":before.1,"source_bytes":before.2,"source_fingerprint_before":before.0,"source_fingerprint_after":after.0,"initial_changed":initial.changed,"initial_ms":initial_ms,"unchanged_scan_ms":unchanged_ms,"search_p95_ms":p95(searches),"backlinks_p95_ms":p95(backs),"weak_relevance_test":{"method":"first 32 title characters, document in top 50; not expert relevance labels","queries":docs.len().min(50),"self_document_hits":exact},"stats":stats,"aggregate_graph_nodes":graph.nodes.len(),"aggregate_graph_edges":graph.edges.len()});
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
