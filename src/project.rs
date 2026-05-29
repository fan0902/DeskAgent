use std::path::{Path, PathBuf};

const SKIPPED_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".next",
    "dist",
    "build",
    ".venv",
    "venv",
];

pub fn scan_project(root: impl AsRef<Path>) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let walker = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| {
            !entry.file_type().is_dir()
                || entry
                    .file_name()
                    .to_str()
                    .map(|name| !SKIPPED_DIRS.contains(&name))
                    .unwrap_or(true)
        });

    for entry in walker {
        let entry = entry?;
        if entry.file_type().is_file() {
            files.push(entry.path().to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}
