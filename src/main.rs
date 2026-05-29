use deskagent::app::EditorApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "DeskAgent",
        options,
        Box::new(|_cc| Ok(Box::<EditorApp>::default())),
    )
}
