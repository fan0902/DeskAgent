use deskagent::diff::make_preview;

#[test]
fn preview_shows_original_and_replacement() {
    let preview = make_preview("old\n", "new\n");
    assert!(preview.contains("-old"));
    assert!(preview.contains("+new"));
}
