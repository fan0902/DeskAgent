# Rust AI Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a minimal Rust desktop editor that opens a project, edits one file, and supports AI-driven explanation and confirmed rewrite.

**Architecture:** Keep the first version deliberately small: one app state, one project scanner, one text editor buffer, one AI adapter, and one diff/preview path. The editor should remain usable even if AI is unavailable, because the core value is the file editing loop.  

**Tech Stack:** Rust 2024, `eframe`/`egui`, `walkdir`, `rfd`, `serde`, `toml`, `similar`, `tempfile`, `anyhow`.

---

### Task 1: App shell and project scanning

**Files:**
- Modify: `Cargo.toml`
- Create: `src/app.rs`
- Create: `src/project.rs`
- Create: `src/main.rs`
- Test: `tests/project_scan.rs`

- [ ] **Step 1: Write the failing test**

```rust
use helloworld::project::scan_project;

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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test scan_project_lists_only_files --test project_scan`
Expected: fail because `scan_project` is not implemented yet.

- [ ] **Step 3: Write minimal implementation**

```rust
pub fn scan_project(root: impl AsRef<Path>) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry?;
        if entry.file_type().is_file() {
            files.push(entry.path().to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test scan_project_lists_only_files --test project_scan`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/main.rs src/app.rs src/project.rs tests/project_scan.rs
git commit -m "feat: add app shell and project scanning"
```

### Task 2: Single-file editor state and disk IO

**Files:**
- Create: `src/editor.rs`
- Modify: `src/app.rs`
- Test: `tests/editor_io.rs`

- [ ] **Step 1: Write the failing test**

```rust
use helloworld::editor::{load_file, save_file};

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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test load_and_save_round_trip --test editor_io`
Expected: fail because `load_file` and `save_file` do not exist yet.

- [ ] **Step 3: Write minimal implementation**

```rust
pub fn load_file(path: impl AsRef<Path>) -> anyhow::Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

pub fn save_file(path: impl AsRef<Path>, text: &str) -> anyhow::Result<()> {
    std::fs::write(path, text)?;
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test load_and_save_round_trip --test editor_io`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/editor.rs src/app.rs tests/editor_io.rs
git commit -m "feat: add file editor io"
```

### Task 3: AI adapter and preview/apply flow

**Files:**
- Create: `src/ai.rs`
- Create: `src/diff.rs`
- Modify: `src/app.rs`
- Test: `tests/diff.rs`

- [ ] **Step 1: Write the failing test**

```rust
use helloworld::diff::make_preview;

#[test]
fn preview_shows_original_and_replacement() {
    let preview = make_preview("old\n", "new\n");
    assert!(preview.contains("-old"));
    assert!(preview.contains("+new"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test preview_shows_original_and_replacement --test diff`
Expected: fail because `make_preview` is missing.

- [ ] **Step 3: Write minimal implementation**

```rust
pub fn make_preview(old: &str, new: &str) -> String {
    let diff = similar::TextDiff::from_lines(old, new);
    diff.unified_diff().to_string()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test preview_shows_original_and_replacement --test diff`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ai.rs src/diff.rs src/app.rs tests/diff.rs
git commit -m "feat: add ai preview diff"
```

### Task 4: Config and app polish

**Files:**
- Create: `src/config.rs`
- Modify: `src/app.rs`
- Test: `tests/config.rs`

- [ ] **Step 1: Write the failing test**

```rust
use helloworld::config::AppConfig;

#[test]
fn config_round_trips_through_toml() {
    let config = AppConfig { provider: "openai".into(), api_key: Some("abc".into()) };
    let text = toml::to_string(&config).unwrap();
    let parsed: AppConfig = toml::from_str(&text).unwrap();
    assert_eq!(parsed.provider, "openai");
    assert_eq!(parsed.api_key.as_deref(), Some("abc"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test config_round_trips_through_toml --test config`
Expected: fail because `AppConfig` is missing.

- [ ] **Step 3: Write minimal implementation**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub provider: String,
    pub api_key: Option<String>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test config_round_trips_through_toml --test config`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/app.rs tests/config.rs
git commit -m "feat: add config support"
```
