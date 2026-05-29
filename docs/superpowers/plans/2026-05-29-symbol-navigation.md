# Symbol Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add line numbers, bold editor text, current-file method navigation, and cross-file Rust symbol navigation to the egui editor.

**Architecture:** Add a lightweight Rust symbol parser in `src/symbols.rs`, expose it through `src/lib.rs`, and keep UI state in `EditorApp`. The editor renders line numbers beside the text area and a symbol list above it; clicking a symbol opens its file and scrolls to the symbol line.

**Tech Stack:** Rust 2024, eframe/egui 0.28, existing cargo test suite.

---

### Task 1: Symbol Parser

**Files:**
- Create: `src/symbols.rs`
- Modify: `src/lib.rs`
- Test: `tests/symbols.rs`

- [ ] Write failing tests for extracting free functions and impl methods from Rust source.
- [ ] Run `cargo test --test symbols` and confirm it fails because `helloworld::symbols` does not exist.
- [ ] Implement `Symbol`, `SymbolKind`, `parse_rust_symbols`, and `index_rust_files`.
- [ ] Run `cargo test --test symbols` and confirm it passes.

### Task 2: App Symbol State

**Files:**
- Modify: `src/app.rs`

- [ ] Store project symbols in `EditorApp`.
- [ ] Rebuild symbols when opening a project.
- [ ] Add a `pending_scroll_line` field for symbol jump requests.
- [ ] Add a helper that opens a symbol target file and records the target line.

### Task 3: Editor UI

**Files:**
- Modify: `src/app.rs`

- [ ] Add a symbol navigation strip above the editor, grouped as current file first and project symbols after it.
- [ ] Render line numbers in a fixed-width gutter beside the multiline editor.
- [ ] Make editor/body fonts bold via `RichText::strong` where possible and text style sizing where not.
- [ ] Scroll the editor viewport to the requested line after a symbol is clicked.

### Task 4: Verification

**Files:**
- Existing test suite only.

- [ ] Run `cargo test`.
- [ ] Run `cargo check`.
- [ ] Inspect `git diff` to confirm the change is scoped to symbol navigation, line numbers, and font weight.
