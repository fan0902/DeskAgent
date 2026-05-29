use deskagent::editor::{load_file, save_file};

#[test]
fn load_and_save_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();

    let text = load_file(&path).unwrap();
    assert_eq!(text, "fn main() {}\n");

    save_file(&path, "fn main() { println!(\"hi\"); }\n").unwrap();
    let round_trip = std::fs::read_to_string(&path).unwrap();
    assert_eq!(round_trip, "fn main() { println!(\"hi\"); }\n");
}
