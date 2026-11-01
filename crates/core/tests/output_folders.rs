use hub_core::Hub;
use std::path::Path;
fn write_source(root: &Path) {
    std::fs::write(root.join("a.md"), "# 注文\n[[b]]").unwrap();
    std::fs::write(root.join("b.md"), "# 顧客").unwrap();
}

#[test]
fn external_output_preserves_parent_files_and_reopens() {
    let source = tempfile::tempdir().unwrap();
    let settings = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    write_source(source.path());
    std::fs::write(output.path().join("keep.txt"), "unrelated").unwrap();
    let mut hub = Hub::open(settings.path()).unwrap();
    hub.register_with_output("demo", "Demo", source.path(), output.path())
        .unwrap();
    hub.scan("demo").unwrap();
    let project = hub.projects().unwrap().remove(0);
    assert_eq!(
        Path::new(&project.output),
        output
            .path()
            .canonicalize()
            .unwrap()
            .join("MarkdownKnowledgeHub/demo")
    );
    assert!(!settings.path().join("indexes/demo").exists());
    drop(hub);
    let mut hub = Hub::open(settings.path()).unwrap();
    assert_eq!(hub.search("demo", "注文").unwrap().len(), 1);
    hub.remove("demo").unwrap();
    assert!(!Path::new(&project.output).exists());
    assert_eq!(
        std::fs::read_to_string(output.path().join("keep.txt")).unwrap(),
        "unrelated"
    );
    assert_eq!(
        std::fs::read_to_string(source.path().join("a.md")).unwrap(),
        "# 注文\n[[b]]"
    );
}

#[test]
fn legacy_output_moves_and_can_be_imported_and_backed_up() {
    let source = tempfile::tempdir().unwrap();
    let settings = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let backups = tempfile::tempdir().unwrap();
    write_source(source.path());
    let mut hub = Hub::open(settings.path()).unwrap();
    hub.register("demo", "Demo", source.path()).unwrap();
    hub.scan("demo").unwrap();
    let report = hub.move_output("demo", output.path()).unwrap();
    assert!(Path::new(&report.old_output).exists());
    assert_eq!(hub.search("demo", "注文").unwrap().len(), 1);
    assert_eq!(hub.retired_outputs().unwrap().len(), 1);
    let mut second = Hub::open(other.path()).unwrap();
    second.import_output(Path::new(&report.new_output)).unwrap();
    assert_eq!(second.backlinks("demo", "b.md").unwrap(), vec!["a.md"]);
    assert!(second.import_output(Path::new(&report.new_output)).is_err());
    let backup = backups.path().join("backup");
    hub.backup(&backup).unwrap();
    assert!(
        hub.backup(&Path::new(&report.new_output).join("nested-backup"))
            .is_err()
    );
    let restored = Hub::open(&backup).unwrap();
    assert!(restored.retired_outputs().unwrap().is_empty());
    assert_eq!(
        Path::new(&restored.projects().unwrap().remove(0).output),
        backup.canonicalize().unwrap().join("indexes/demo")
    );
    assert_eq!(restored.search("demo", "注文").unwrap().len(), 1);
    let old = hub.retired_outputs().unwrap().remove(0);
    hub.cleanup_retired(&old.key).unwrap();
    assert!(!Path::new(&report.old_output).exists());
    assert!(Path::new(&report.new_output).exists());
}

#[test]
fn conflicting_paths_and_unknown_files_are_never_removed() {
    let source = tempfile::tempdir().unwrap();
    let settings = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    write_source(source.path());
    let mut hub = Hub::open(settings.path()).unwrap();
    assert!(
        hub.register_with_output("bad", "Bad", source.path(), source.path())
            .is_err()
    );
    hub.register_with_output("demo", "Demo", source.path(), output.path())
        .unwrap();
    hub.scan("demo").unwrap();
    let path = hub.projects().unwrap().remove(0).output;
    std::fs::write(Path::new(&path).join("customer-note.txt"), "keep").unwrap();
    assert!(hub.remove("demo").is_err());
    assert!(Path::new(&path).join("customer-note.txt").exists());
    assert_eq!(hub.projects().unwrap().len(), 1);
    let mut other = Hub::open(settings.path()).unwrap();
    assert!(
        other
            .register_with_output("another", "Another", Path::new(&path), output.path())
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(source.path().join("a.md")).unwrap(),
        "# 注文\n[[b]]"
    );
}

#[test]
fn missing_input_remains_browsable_and_can_be_reconnected() {
    let source = tempfile::tempdir().unwrap();
    let settings = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let replacement = tempfile::tempdir().unwrap();
    write_source(source.path());
    write_source(replacement.path());
    let mut h = Hub::open(settings.path()).unwrap();
    h.register_with_output("demo", "Demo", source.path(), output.path())
        .unwrap();
    h.scan("demo").unwrap();
    let path = h.projects().unwrap().remove(0).output;
    source.close().unwrap();
    assert_eq!(h.search("demo", "注文").unwrap().len(), 1);
    assert!(h.scan("demo").is_err());
    let mut second = Hub::open(other.path()).unwrap();
    second.import_output(Path::new(&path)).unwrap();
    assert_eq!(second.search("demo", "注文").unwrap().len(), 1);
    second.change_input("demo", replacement.path()).unwrap();
    assert_eq!(
        h.root("demo").unwrap(),
        replacement.path().canonicalize().unwrap()
    );
    h.scan("demo").unwrap();
}
#[test]
fn namespace_locks_cannot_be_created_inside_an_input() {
    let parent = tempfile::tempdir().unwrap();
    let settings = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let input = parent.path().join("MarkdownKnowledgeHub").join(".locks");
    std::fs::create_dir_all(&input).unwrap();
    write_source(&input);
    write_source(source.path());
    let mut h = Hub::open(settings.path()).unwrap();
    h.register("original", "Original", &input).unwrap();
    assert!(
        h.register_with_output("new", "New", source.path(), parent.path())
            .is_err()
    );
    assert!(!parent.path().join("MarkdownKnowledgeHub/new").exists());
    assert_eq!(std::fs::read_dir(&input).unwrap().count(), 2);
}
#[test]
fn invalid_import_does_not_register_or_create_an_index() {
    let source = tempfile::tempdir().unwrap();
    let settings = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    write_source(source.path());
    let mut h = Hub::open(settings.path()).unwrap();
    h.register_with_output("demo", "Demo", source.path(), output.path())
        .unwrap();
    h.scan("demo").unwrap();
    let path = std::path::PathBuf::from(h.projects().unwrap().remove(0).output);
    let manifest = path.join("project.json");
    let original = std::fs::read(&manifest).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    value["input"] = "../unsafe".into();
    std::fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut second = Hub::open(other.path()).unwrap();
    assert!(second.import_output(&path).is_err());
    assert!(second.projects().unwrap().is_empty());
    std::fs::write(&manifest, original).unwrap();
    std::fs::write(path.join("customer.txt"), "preserve").unwrap();
    assert!(second.import_output(&path).is_err());
    assert!(second.projects().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(path.join("customer.txt")).unwrap(),
        "preserve"
    );
}

#[test]
fn replacing_input_verifies_all_files_even_when_metadata_matches() {
    let source = tempfile::tempdir().unwrap();
    let settings = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let replacement = tempfile::tempdir().unwrap();
    for name in ["a.md", "b.md"] {
        std::fs::write(source.path().join(name), "# Before").unwrap();
        std::fs::write(replacement.path().join(name), "# After!").unwrap();
        let modified = std::fs::metadata(source.path().join(name))
            .unwrap()
            .modified()
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(replacement.path().join(name))
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }
    let mut h = Hub::open(settings.path()).unwrap();
    h.register_with_output("demo", "Demo", source.path(), output.path())
        .unwrap();
    h.scan("demo").unwrap();
    h.change_input("demo", replacement.path()).unwrap();
    assert_eq!(h.search("demo", "Before").unwrap().len(), 2);
    h.scan_paths("demo", &["a.md".to_string()]).unwrap();
    assert_eq!(h.search("demo", "After").unwrap().len(), 2);
    assert!(h.search("demo", "Before").unwrap().is_empty());
    let locks = output.path().join("MarkdownKnowledgeHub/.locks");
    assert!(h.change_input("demo", &locks).is_err());
}

#[test]
fn rescanning_does_not_collect_unknown_files_in_retired_generations() {
    let source = tempfile::tempdir().unwrap();
    let settings = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    write_source(source.path());
    let mut h = Hub::open(settings.path()).unwrap();
    h.register_with_output("demo", "Demo", source.path(), output.path())
        .unwrap();
    h.scan("demo").unwrap();
    let base = std::path::PathBuf::from(h.projects().unwrap().remove(0).output);
    let generation = std::fs::read_dir(base.join("generations"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let note = generation.join("customer-note.txt");
    std::fs::write(&note, "preserve").unwrap();
    for text in ["# Changed once", "# Changed twice"] {
        std::fs::write(source.path().join("a.md"), text).unwrap();
        h.scan_verified("demo", true).unwrap();
    }
    assert_eq!(std::fs::read_to_string(note).unwrap(), "preserve");
}
