mod backup;
mod generations;
mod monitor;
mod pages;
mod parser;
mod tokenizer;
pub use monitor::*;
mod projects;
mod search;
mod store;
mod watch;
pub use pages::*;
pub use projects::*;
pub use store::*;

#[cfg(test)]
mod tests {
    #[test]
    fn targeted_update_upgrades_the_entire_legacy_metadata() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "[[BKey]]").unwrap();
        std::fs::write(source.path().join("b.md"), "---\naliases: [BKey]\n---\n# B").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        {
            let db = hub.write_db("demo").unwrap();
            db.execute_batch("UPDATE state SET value=0 WHERE key='parser_version'; DELETE FROM aliases; DELETE FROM headings; DELETE FROM link_keys;").unwrap();
        }
        std::fs::write(source.path().join("a.md"), "# A\n[[BKey]]").unwrap();
        assert_eq!(hub.scan_paths("demo", &["a.md".into()]).unwrap().changed, 2);
        assert_eq!(
            hub.link_target("demo", "a.md", "BKey")
                .unwrap()
                .unwrap()
                .resolved
                .as_deref(),
            Some("b.md")
        );
    }
    #[test]
    fn literal_search_does_not_match_scattered_characters() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# Match\nCustomer 東京注文").unwrap();
        std::fs::write(
            source.path().join("b.md"),
            "# Other\ncusto tomer 東の京 注の文",
        )
        .unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        assert_eq!(hub.search("demo", "customer").unwrap().len(), 1);
        assert_eq!(hub.search("demo", "東京").unwrap().len(), 1);
        assert_eq!(hub.search("demo", "customer 注文").unwrap().len(), 1);
        assert_eq!(hub.search("demo", "東").unwrap().len(), 2);
    }
    #[test]
    fn backlink_pages_and_folder_summary_cover_all_documents() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        for folder in ["x", "y"] {
            std::fs::create_dir(source.path().join(folder)).unwrap();
        }
        std::fs::write(source.path().join("x/a.md"), "# Target").unwrap();
        std::fs::write(source.path().join("y/a.md"), "# Isolated").unwrap();
        for i in 0..503 {
            std::fs::write(source.path().join(format!("x/doc-{i}.md")), "[[a]]").unwrap();
        }
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        assert_eq!(
            hub.backlinks_page("demo", "x/a.md", 400, 200)
                .unwrap()
                .items
                .len(),
            103
        );
        assert!(
            hub.backlinks_page("demo", "x/a.md", 400, 200)
                .unwrap()
                .next_offset
                .is_none()
        );
        let stats = hub.stats("demo").unwrap();
        assert_eq!(stats.duplicate_names, 1);
        assert_eq!(stats.isolated, 1);
        let summary = hub.graph_summary("demo").unwrap();
        assert_eq!(summary.nodes.len(), 2);
        assert_eq!(summary.edges.len(), 1);
        assert_eq!(
            hub.link_target("demo", "x/doc-502.md", "a")
                .unwrap()
                .unwrap()
                .resolved
                .as_deref(),
            Some("x/a.md")
        );
    }
    #[test]
    fn generated_empty_html_anchors_are_recognized_without_enabling_html() {
        let parsed =
            crate::parser::parse("<a id=\"screen-login\"></a>\n\n# Login", "a.md").unwrap();
        assert!(parsed.headings.contains(&"screen-login".to_string()));
        let parsed =
            crate::parser::parse("<a id=\"unsafe\" onclick=\"doSomething()\"></a>", "a.md")
                .unwrap();
        assert!(!parsed.headings.contains(&"unsafe".to_string()));
    }
    #[test]
    fn aliases_and_heading_targets_are_resolved_and_validated() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(
            source.path().join("a.md"),
            "---\naliases: [顧客マスター, 'Customer Master']\n---\n# 顧客\n## 詳細情報\n",
        )
        .unwrap();
        std::fs::write(
            source.path().join("b.md"),
            "[[顧客マスター#詳細情報]] [[Customer Master]] [[a#unknown]] [[#local]]\n## local",
        )
        .unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        let links = hub.links("demo").unwrap();
        assert_eq!(links.iter().filter(|l| l.status == "resolved").count(), 3);
        assert_eq!(
            links
                .iter()
                .find(|l| l.target == "a#unknown")
                .unwrap()
                .status,
            "missing-heading"
        );
        std::fs::write(source.path().join("a.md"), "# 顧客\n## 削除後").unwrap();
        hub.scan("demo").unwrap();
        assert_eq!(
            hub.links("demo")
                .unwrap()
                .iter()
                .find(|l| l.target == "Customer Master")
                .unwrap()
                .status,
            "missing"
        );
    }
    #[test]
    fn publication_survives_process_exit_without_cleanup() {
        if let Ok(state) = std::env::var("HUB_CRASH_TEST_STATE") {
            let mut hub = super::Hub::open(std::path::Path::new(&state)).unwrap();
            let _ = hub.scan_before_publish("demo", || std::process::exit(77));
            panic!("公開前停止フックが呼ばれていません");
        }
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# BeforeCrash").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        std::fs::write(source.path().join("a.md"), "# AfterCrash").unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::publication_survives_process_exit_without_cleanup",
                "--nocapture",
            ])
            .env("HUB_CRASH_TEST_STATE", state.path())
            .status()
            .unwrap();
        assert_eq!(result.code(), Some(77));
        assert_eq!(hub.search("demo", "BeforeCrash").unwrap().len(), 1);
        hub.scan("demo").unwrap();
        assert_eq!(hub.search("demo", "AfterCrash").unwrap().len(), 1);
    }
    #[test]
    fn publication_keeps_old_generation_until_both_indexes_are_ready() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# 旧文書").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        std::fs::write(source.path().join("a.md"), "# 新文書").unwrap();
        let before_publish = || -> anyhow::Result<()> {
            let reader = super::Hub::open(state.path())?;
            assert_eq!(reader.document("demo", "a.md")?.title, "旧文書");
            assert_eq!(reader.search("demo", "旧文書")?.len(), 1);
            anyhow::bail!("公開前の異常終了を模擬")
        };
        assert!(hub.scan_before_publish("demo", before_publish).is_err());
        drop(hub);
        let mut hub = super::Hub::open(state.path()).unwrap();
        assert_eq!(hub.search("demo", "旧文書").unwrap().len(), 1);
        hub.scan("demo").unwrap();
        assert_eq!(hub.document("demo", "a.md").unwrap().title, "新文書");
        assert!(hub.search("demo", "旧文書").unwrap().is_empty());
    }

    #[test]
    fn search_and_backlink_pages_have_no_hard_truncation() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        for i in 0..63 {
            std::fs::write(
                source.path().join(format!("{i:03}.md")),
                "# ページ試験\n[[000]]",
            )
            .unwrap();
        }
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        let first = hub.search_page("demo", "", 0, 50).unwrap();
        assert_eq!(first.items.len(), 50);
        let next = hub
            .search_page("demo", "", first.next_offset.unwrap(), 50)
            .unwrap();
        assert_eq!(next.items.len(), 13);
        assert!(next.next_offset.is_none());
        assert_eq!(
            hub.search_page("demo", "ページ試験", 50, 50)
                .unwrap()
                .items
                .len(),
            13
        );
        assert_eq!(
            hub.backlinks_page("demo", "000.md", 50, 50)
                .unwrap()
                .items
                .len(),
            13
        );
        assert!(hub.search_page("demo", "", usize::MAX, 1).is_err());
        assert!(hub.backlinks_page("demo", "000.md", 0, 0).is_err());
    }
    #[test]
    fn project_lifecycle_preserves_sources_and_recreation_is_fresh() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let raw = "# 注文\n[[customer]] [[missing]]";
        std::fs::write(source.path().join("order.md"), raw).unwrap();
        std::fs::write(source.path().join("customer.md"), "# 顧客").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        assert_eq!(hub.stats("demo").unwrap().documents, 2);
        assert_eq!(hub.stats("demo").unwrap().missing, 1);
        let graph = hub.graph("demo", Some("order.md"), 2).unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
        assert!(hub.graph("demo", None, 0).is_err());
        assert_eq!(hub.graph("demo", None, 1).unwrap().nodes.len(), 1);
        assert!(hub.graph("demo", None, 1).unwrap().truncated);
        assert!(hub.graph("demo", Some("absent.md"), 10).is_err());
        hub.rebuild("demo").unwrap();
        hub.remove("demo").unwrap();
        assert!(hub.projects().unwrap().is_empty());
        assert!(!state.path().join("indexes/demo").exists());
        assert_eq!(
            std::fs::read_to_string(source.path().join("order.md")).unwrap(),
            raw
        );
        std::fs::remove_file(source.path().join("customer.md")).unwrap();
        hub.register("demo", "再作成", source.path()).unwrap();
        hub.scan("demo").unwrap();
        assert!(hub.search("demo", "顧客").unwrap().is_empty());
        assert_eq!(hub.stats("demo").unwrap().documents, 1);
        assert!(hub.remove("../escape").is_err());
    }
    #[cfg(windows)]
    #[test]
    fn targeted_paths_cannot_duplicate_a_document_with_different_casing() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# A").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        assert!(hub.scan_paths("demo", &["A.md".into()]).is_err());
        assert_eq!(hub.search("demo", "").unwrap().len(), 1);
    }
    #[test]
    fn file_events_verify_hash_even_when_metadata_is_unchanged() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let path = source.path().join("a.md");
        std::fs::write(&path, "aaaa").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, "bbbb").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(stamp))
            .unwrap();
        assert_eq!(hub.scan("demo").unwrap().changed, 0);
        assert_eq!(hub.scan_paths("demo", &["a.md".into()]).unwrap().changed, 1);
        assert_eq!(hub.document("demo", "a.md").unwrap().markdown, "bbbb");
    }
    #[test]
    fn targeted_updates_preserve_unrelated_documents_and_refresh_ambiguity() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        for dir in ["x", "y", "z"] {
            std::fs::create_dir(source.path().join(dir)).unwrap();
        }
        std::fs::write(source.path().join("z/a.md"), "# 初期\n[[b]]").unwrap();
        std::fs::write(source.path().join("x/b.md"), "# 顧客").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        std::fs::write(source.path().join("bad.md"), [255]).unwrap();
        std::fs::write(source.path().join("z/a.md"), "# 変更\n[[b]]").unwrap();
        let report = hub
            .scan_paths("demo", &["z/a.md".into(), "z/a.md".into()])
            .unwrap();
        assert_eq!(report.changed, 1);
        assert_eq!(report.unchanged, 0);
        assert_eq!(hub.search("demo", "変更").unwrap().len(), 1);
        assert_eq!(hub.backlinks("demo", "x/b.md").unwrap(), vec!["z/a.md"]);
        std::fs::write(source.path().join("y/b.md"), "# 同名").unwrap();
        hub.scan_paths("demo", &["y/b.md".into()]).unwrap();
        assert!(hub.backlinks("demo", "x/b.md").unwrap().is_empty());
        assert_eq!(hub.links("demo").unwrap()[0].status, "ambiguous");
        std::fs::remove_file(source.path().join("y/b.md")).unwrap();
        assert_eq!(
            hub.scan_paths("demo", &["y/b.md".into()]).unwrap().deleted,
            1
        );
        assert_eq!(hub.backlinks("demo", "x/b.md").unwrap(), vec!["z/a.md"]);
        assert!(hub.scan_paths("demo", &["../escape.md".into()]).is_err());
        assert!(hub.scan_paths("demo", &["x".into()]).is_err());
        assert!(hub.scan("demo").is_err());
    }

    #[test]
    fn scoped_link_pages_reach_documents_after_the_global_cap() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "[[b]]\n".repeat(5001)).unwrap();
        std::fs::write(source.path().join("z.md"), "[[a]] [[b]] [[missing]]").unwrap();
        std::fs::write(source.path().join("b.md"), "# B").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        assert!(
            !hub.links("demo")
                .unwrap()
                .iter()
                .any(|l| l.source == "z.md")
        );
        let page = hub.links_page("demo", "z.md", 0, 2).unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.next_offset, Some(2));
        assert_eq!(page.items[0].resolved.as_deref(), Some("a.md"));
        let next = hub
            .links_page("demo", "z.md", page.next_offset.unwrap(), 2)
            .unwrap();
        assert_eq!(next.items.len(), 1);
        assert_eq!(next.next_offset, None);
        assert!(hub.links_page("demo", "z.md", 0, 0).is_err());
        assert!(hub.links_page("demo", "z.md", 0, 201).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn targeted_updates_reject_symlink_ancestors() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        std::fs::write(external.path().join("secret.md"), "secret").unwrap();
        std::os::unix::fs::symlink(external.path(), source.path().join("outside")).unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        assert!(
            hub.scan_paths("demo", &["outside/secret.md".into()])
                .is_err()
        );
        assert!(hub.search("demo", "secret").unwrap().is_empty());
    }
    #[test]
    fn recovery_and_failed_read_preserve_existing_data() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.md"), "# 中文\n客户信息 order").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        assert_eq!(hub.search("demo", "客户").unwrap().len(), 1);
        assert_eq!(hub.search("demo", "order").unwrap().len(), 1);
        let db = hub.write_db("demo").unwrap();
        db.execute("UPDATE state SET value=1 WHERE key='dirty'", [])
            .unwrap();
        assert!(hub.scan("demo").unwrap().recovered);
        std::fs::write(source.path().join("bad.md"), [255, 254]).unwrap();
        assert!(hub.scan("demo").is_err());
        assert_eq!(hub.search("demo", "客户").unwrap().len(), 1);
        std::fs::remove_file(source.path().join("bad.md")).unwrap();
        assert_eq!(hub.scan_verified("demo", true).unwrap().changed, 0);
    }
    #[test]
    fn ambiguous_and_code_links() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        for dir in ["x", "y", "z"] {
            std::fs::create_dir(source.path().join(dir)).unwrap();
        }
        for path in ["x/b.md", "y/b.md"] {
            std::fs::write(source.path().join(path), "# b").unwrap();
        }
        std::fs::write(
            source.path().join("z/a.md"),
            "[[b]]\n```md\n[[ghost]]\n```\n`[[inline]]`\n[[missing|]]",
        )
        .unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("demo", "Demo", source.path()).unwrap();
        hub.scan("demo").unwrap();
        let links = hub.links("demo").unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links.iter().filter(|l| l.status == "ambiguous").count(), 1);
        assert!(hub.backlinks("demo", "x/b.md").unwrap().is_empty());
    }
    #[test]
    fn incremental_search_and_backlinks() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::write(
            source.path().join("a.md"),
            "# 注文\n東京の注文管理 [[b|説明]] `[[ghost]]` [外部](https://example.com)",
        )
        .unwrap();
        std::fs::write(source.path().join("b.md"), "# 顧客\n顧客情報").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        hub.register("sample", "サンプル", source.path()).unwrap();
        assert_eq!(hub.scan("sample").unwrap().changed, 2);
        assert_eq!(hub.search("sample", "注文").unwrap().len(), 1);
        assert_eq!(hub.backlinks("sample", "b.md").unwrap(), vec!["a.md"]);
        assert_eq!(hub.links("sample").unwrap().len(), 1);
        assert_eq!(hub.scan("sample").unwrap().changed, 0);
        drop(hub);
        let mut hub = super::Hub::open(state.path()).unwrap();
        assert_eq!(hub.projects().unwrap().len(), 1);
        assert_eq!(hub.search("sample", "注文").unwrap().len(), 1);
        std::fs::write(source.path().join("a.md"), "# 更新\n商品配送").unwrap();
        assert_eq!(hub.scan("sample").unwrap().changed, 1);
        assert!(hub.search("sample", "注文").unwrap().is_empty());
        assert!(hub.backlinks("sample", "b.md").unwrap().is_empty());
        std::fs::remove_file(source.path().join("b.md")).unwrap();
        assert_eq!(hub.scan("sample").unwrap().deleted, 1);
        assert_eq!(
            std::fs::read_to_string(source.path().join("a.md")).unwrap(),
            "# 更新\n商品配送"
        );
    }
    #[test]
    fn isolation_ambiguity_and_path_safety() {
        let source = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join("sub")).unwrap();
        std::fs::write(
            source.path().join("a.md"),
            "[[b]] [[missing]] [顧客](sub/b.md#見出し)",
        )
        .unwrap();
        std::fs::write(source.path().join("b.md"), "root").unwrap();
        std::fs::write(source.path().join("sub/b.md"), "nested\n## 見出し").unwrap();
        let mut hub = super::Hub::open(state.path()).unwrap();
        assert!(hub.register("../bad", "bad", source.path()).is_err());
        assert!(hub.register("Demo", "bad", source.path()).is_err());
        assert!(hub.register("con", "bad", source.path()).is_err());
        assert!(
            hub.register("nested", "bad", &state.path().join("source"))
                .is_err()
        );
        hub.register("one", "One", source.path()).unwrap();
        hub.register("two", "Two", source.path()).unwrap();
        hub.scan("one").unwrap();
        assert!(hub.search("two", "root").unwrap().is_empty());
        assert!(hub.document("one", "../secret.md").is_err());
        let links = hub.links("one").unwrap();
        assert_eq!(links.iter().filter(|l| l.status == "resolved").count(), 2);
        assert_eq!(links.iter().filter(|l| l.status == "missing").count(), 1);
    }
}
