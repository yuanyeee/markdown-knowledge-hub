mod parser;
mod search;
mod store;
pub use store::*;

#[cfg(test)]
mod tests {
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
