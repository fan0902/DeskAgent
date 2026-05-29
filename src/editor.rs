use std::path::Path;

pub fn load_file(path: impl AsRef<Path>) -> anyhow::Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

pub fn save_file(path: impl AsRef<Path>, text: &str) -> anyhow::Result<()> {
    std::fs::write(path, text)?;
    Ok(())
}
