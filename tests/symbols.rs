use deskagent::symbols::{index_rust_files, parse_rust_symbols, SymbolKind};

#[test]
fn parse_rust_symbols_finds_free_functions_and_methods() {
    let source = r#"
pub fn top_level() {}

impl EditorApp {
    fn open_file(&mut self) {}
    pub(crate) async fn request_ai(&mut self) {}
}

trait Runner {
    fn run(&self);
}
"#;

    let symbols = parse_rust_symbols("src/app.rs".into(), source);
    let names: Vec<_> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();

    assert_eq!(names, vec!["top_level", "EditorApp::open_file", "EditorApp::request_ai", "Runner::run"]);
    assert_eq!(symbols[0].kind, SymbolKind::Function);
    assert_eq!(symbols[1].kind, SymbolKind::Method);
    assert_eq!(symbols[2].line, 6);
    assert_eq!(symbols[3].line, 10);
}

#[test]
fn index_rust_files_ignores_non_rust_files() {
    let dir = tempfile::tempdir().unwrap();
    let rust_file = dir.path().join("lib.rs");
    let text_file = dir.path().join("notes.txt");
    std::fs::write(&rust_file, "fn library() {}\n").unwrap();
    std::fs::write(&text_file, "fn not_rust() {}\n").unwrap();

    let symbols = index_rust_files(&[rust_file.clone(), text_file]).unwrap();

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "library");
    assert_eq!(symbols[0].path, rust_file);
    assert_eq!(symbols[0].line, 1);
}
