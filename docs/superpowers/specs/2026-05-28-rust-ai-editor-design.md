# Rust AI Editor Design

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a minimal Rust desktop code editor with an AI side panel for single-file explanation, completion, and confirmed rewrite.

**Architecture:** The app is a single `eframe` desktop window with three state owners: project browsing, current file editing, and AI interaction. The file tree only discovers text files and loads one active file into a plain text editor; the AI panel receives the active file text plus a short instruction, then returns a proposed replacement that the user must review before applying.

**Tech Stack:** Rust 2024, `eframe`/`egui`, `walkdir`, `rfd`, `serde`, `toml`, `reqwest`-ready AI abstraction, `similar`, `tempfile` for tests.

---

## Scope

First version:
- open a project directory
- browse files in a left sidebar
- edit one file in the center
- send the active file and a user instruction to AI
- preview the AI rewrite before applying it
- save the edited file back to disk

Out of scope:
- multi-file agent edits
- LSP integration
- Git integration
- terminal panel
- plugin system
- real-time collaboration

## UI Layout

- Top bar: open project, save file, current path, AI model/config status
- Left sidebar: project file tree
- Center: plain text editor for the active file
- Right sidebar: AI instruction box, action buttons, result preview
- Bottom bar: dirty state, request state, short errors

## Data Model

- `ProjectState`: root path, discovered files, selected file
- `EditorState`: active path, text buffer, dirty flag, save/load helpers
- `AiState`: instruction, request status, last response, preview buffer
- `AppConfig`: model/provider settings and optional API key

## AI Flow

1. User selects a file.
2. User enters an instruction like "explain this function" or "rewrite this for clarity".
3. The app sends the active file content plus the instruction to the AI layer.
4. The AI layer returns either a textual answer or a full-file replacement.
5. For rewrite actions, the app shows the proposed content in a preview area.
6. The user explicitly applies the change before it replaces the editor buffer.

## Error Handling

- Directory scan errors stay local to the file tree and do not crash the app.
- File read/write errors show in the bottom bar and keep the prior buffer intact.
- AI request failures preserve the current editor content and show a retryable error.
- Invalid or empty instructions are rejected before network work starts.

## Testing Strategy

- unit test project file discovery excludes binary-like entries and captures text files
- unit test save/load behavior on a temp file
- unit test diff generation between original and AI-proposed content
- unit test config serialization round-trips

## Milestones

1. App shell and project loading
2. Single-file editor and save path
3. AI abstraction and preview/apply flow
4. Basic config and error state polish
