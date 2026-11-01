//! Synthetic smoke benchmark. Not a substitute for production corpus validation.
use anyhow::Result;
use hub_core::Hub;
use std::time::Instant;
fn elapsed(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
fn percentile(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[(samples.len() * 95).div_ceil(100) - 1]
}
fn main() -> Result<()> {
    let count: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "10000".into())
        .parse()?;
    anyhow::ensure!(count > 0 && count <= 100000, "1..100000を指定してください");
    let target_bytes: usize = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "8192".into())
        .parse()?;
    anyhow::ensure!(
        (512..=65536).contains(&target_bytes),
        "文書サイズは512～65536バイトです"
    );
    let data = tempfile::tempdir()?;
    let source = tempfile::tempdir()?;
    let mut bytes = 0;
    for i in 0..count {
        let mut raw = format!(
            "# 文書 {i}\n\n注文管理と客户信息。English knowledge system.\n\n[[doc-{}.md|次の仕様]]\n\n{}",
            (i + 1) % count,
            "商品情報 customer order reference document. 東京 上海 Osaka.\n".repeat(5)
        );
        let mut section = 0;
        while raw.len() < target_bytes {
            raw.push_str(&format!("Section {section} specification document {i} customer order processing validation feature number {}.\n",(i*31+section)%10007));
            section += 1;
        }
        raw.truncate(target_bytes);
        bytes += raw.len();
        std::fs::write(source.path().join(format!("doc-{i}.md")), raw)?;
    }
    let mut hub = Hub::open(data.path())?;
    hub.register("bench", "Benchmark", source.path())?;
    let start = Instant::now();
    let first = hub.scan("bench")?;
    let initial_ms = elapsed(start);
    let start = Instant::now();
    let noop = hub.scan("bench")?;
    let unchanged_ms = elapsed(start);
    assert_eq!(first.changed, count);
    assert_eq!(noop.changed, 0);
    let mut search = vec![];
    let mut back = vec![];
    let mut first_queries = vec![];
    for i in 0..100 {
        let start = Instant::now();
        assert!(
            !hub.search("bench", ["注文", "客户", "customer"][i % 3])?
                .is_empty()
        );
        let ms = elapsed(start);
        if i < 3 {
            first_queries.push(ms);
        }
        search.push(ms);
        let start = Instant::now();
        assert_eq!(
            hub.backlinks("bench", &format!("doc-{}.md", i % count))?
                .len(),
            1
        );
        back.push(elapsed(start));
    }
    std::fs::write(
        source.path().join("doc-0.md"),
        "# 更新\n差分更新の実測 [[doc-1.md]]",
    )?;
    let start = Instant::now();
    assert_eq!(hub.scan("bench")?.changed, 1);
    let delta_ms = elapsed(start);
    let mut event_samples = vec![];
    for n in 0..10 {
        std::fs::write(
            source.path().join("doc-0.md"),
            format!("# 更新 {n}\nイベント差分の実測 [[doc-1.md]]"),
        )?;
        let start = Instant::now();
        assert_eq!(hub.scan_paths("bench", &["doc-0.md".into()])?.changed, 1);
        event_samples.push(elapsed(start));
    }
    let index_bytes = walkdir::WalkDir::new(data.path())
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum::<u64>();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "corpus":"synthetic, padded; not production documents","target_document_bytes":target_bytes,"files":count,"source_bytes":bytes,"index_bytes":index_bytes,"initial_ms":initial_ms,"unchanged_scan_ms":unchanged_ms,
            "single_delta_ms":delta_ms,"event_delta_p95_ms":percentile(event_samples.clone()),"event_sample_ms":event_samples,"event_samples":10,"search_p95_ms":percentile(search),"backlinks_p95_ms":percentile(back),
            "first_three_uncached_query_ms":first_queries,"queries":100,"reader":"reader cache up to 8 generations; result cache up to 32 pages; repeated three queries, cache warm after first pass","profile":if cfg!(debug_assertions){"debug"}else{"release"}
        }))?
    );
    Ok(())
}
