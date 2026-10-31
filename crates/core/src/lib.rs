mod parser;
mod search;
mod store;
mod watch;
pub use store::*;

#[cfg(test)]
mod tests {
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
        let db =
            rusqlite::Connection::open(state.path().join("indexes/demo/metadata.sqlite")).unwrap();
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
        std::fs::write(source.path().join("sub/b.md"), "nested").unwrap();
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
