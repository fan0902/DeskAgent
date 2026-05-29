use deskagent::project::scan_project;

#[test]
fn scan_project_lists_only_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn main() {}").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/b.txt"), "hello").unwrap();

    let files = scan_project(dir.path()).unwrap();

    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|p| p.ends_with("a.rs")));
    assert!(files.iter().any(|p| p.ends_with("b.txt")));
}

#[test]
fn scan_project_skips_common_build_dirs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("src.rs"), "fn main() {}").unwrap();
    std::fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    std::fs::write(dir.path().join("target/debug/generated.rs"), "fn main() {}").unwrap();
    std::fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
    std::fs::write(dir.path().join(".git/objects/ignored"), "x").unwrap();

    let files = scan_project(dir.path()).unwrap();

    assert_eq!(files.len(), 1);
    assert!(files.iter().any(|p| p.ends_with("src.rs")));
    assert!(!files.iter().any(|p| p.ends_with("generated.rs")));
    assert!(!files.iter().any(|p| p.ends_with("ignored")));
}
