use crate::editor::{load_file, save_file};
use crate::highlight::{Highlighter, StyledLine};
use crate::project::scan_project;
use crate::symbols::{index_python_files, index_rust_files, Symbol};
use crate::terminal::TerminalSession;
use anyhow::Context;
use chrono::Local;
use eframe::egui;
use serde::{Deserialize, Serialize};
use egui::text::CCursor;
use egui::text::{LayoutJob, TextFormat};
use std::collections::{BTreeMap, BTreeSet};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ── Design tokens (aligned with codex style guide) ──────────────────────────
const BG_BASE: egui::Color32 = egui::Color32::from_rgb(18, 18, 18);
const BG_SIDEBAR: egui::Color32 = egui::Color32::from_rgb(24, 24, 24);
const BG_PANEL: egui::Color32 = egui::Color32::from_rgb(28, 28, 28);
const BG_HOVER: egui::Color32 = egui::Color32::from_rgb(38, 38, 42);
const BG_SELECTED: egui::Color32 = egui::Color32::from_rgb(30, 50, 68);
const BG_ACTIVE: egui::Color32 = egui::Color32::from_rgb(40, 60, 80);
const BG_INPUT: egui::Color32 = egui::Color32::from_rgb(32, 32, 36);
const BG_BUTTON: egui::Color32 = egui::Color32::from_rgb(44, 44, 50);
const BG_BUTTON_HOVER: egui::Color32 = egui::Color32::from_rgb(58, 58, 66);

const FG_PRIMARY: egui::Color32 = egui::Color32::from_rgb(220, 220, 220);
const FG_SECONDARY: egui::Color32 = egui::Color32::from_rgb(140, 140, 148);
const FG_DIM: egui::Color32 = egui::Color32::from_rgb(88, 88, 96);
const FG_ACCENT: egui::Color32 = egui::Color32::from_rgb(0, 200, 220);
const FG_SELECTED: egui::Color32 = egui::Color32::from_rgb(100, 210, 255);
const FG_SUCCESS: egui::Color32 = egui::Color32::from_rgb(80, 200, 120);
const FG_ERROR: egui::Color32 = egui::Color32::from_rgb(240, 80, 80);
const FG_DIR: egui::Color32 = egui::Color32::from_rgb(180, 180, 190);
const FG_FILE: egui::Color32 = egui::Color32::from_rgb(160, 160, 170);

const BORDER_SUBTLE: egui::Color32 = egui::Color32::from_rgb(42, 42, 48);
const BORDER_NORMAL: egui::Color32 = egui::Color32::from_rgb(58, 58, 66);

const TREE_ROW_HEIGHT: f32 = 22.0;
const TREE_INDENT: f32 = 16.0;
const TREE_ICON_W: f32 = 16.0;
const LINE_NUMBER_CHAR_W: f32 = 8.0;
const LINE_NUMBER_PAD_X: f32 = 14.0;

const DEFAULT_CODE_FONT_SIZE: f32 = 13.0;
const DEFAULT_LINE_HEIGHT: f32 = 1.25;

// ── Main view mode ────────────────────────────────────────────────────────────

#[derive(Default, PartialEq, Clone, Copy)]
enum MainView {
    #[default]
    Code,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum EditorFontFamily {
    Monospace,
    Proportional,
}

impl EditorFontFamily {
    const ALL: [Self; 2] = [Self::Monospace, Self::Proportional];

    fn label(self) -> &'static str {
        match self {
            Self::Monospace => "Monospace",
            Self::Proportional => "Proportional",
        }
    }

    fn egui_family(self) -> egui::FontFamily {
        match self {
            Self::Monospace => egui::FontFamily::Monospace,
            Self::Proportional => egui::FontFamily::Proportional,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct EditorFormatSettings {
    font_size: f32,
    font_family: EditorFontFamily,
    line_height: f32,
    wrap_lines: bool,
}

impl Default for EditorFormatSettings {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_CODE_FONT_SIZE,
            font_family: EditorFontFamily::Monospace,
            line_height: DEFAULT_LINE_HEIGHT,
            wrap_lines: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct EditorPreferences {
    global: EditorFormatSettings,
    files: BTreeMap<String, EditorFormatSettings>,
}

impl Default for EditorPreferences {
    fn default() -> Self {
        Self {
            global: EditorFormatSettings::default(),
            files: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct HighlightCache {
    file_name: String,
    source_hash: u64,
    lines: Arc<Vec<StyledLine>>,
}

// ── Tree node ────────────────────────────────────────────────────────────────

enum TreeNode {
    Dir {
        path: PathBuf,
        depth: usize,
        is_open: bool,
    },
    File {
        path: PathBuf,
        depth: usize,
    },
}

#[derive(Default)]
struct DirEntry {
    dirs: BTreeSet<PathBuf>,
    files: Vec<PathBuf>,
}

// ── App state ────────────────────────────────────────────────────────────────

pub struct EditorApp {
    project_root: Option<PathBuf>,
    files: Vec<PathBuf>,
    expanded_dirs: BTreeSet<PathBuf>,
    selected_file: Option<PathBuf>,
    project_symbols: Vec<Symbol>,
    pending_scroll_line: Option<usize>,
    /// Raw source text of the currently viewed file.
    editor_text: String,
    original_text: String,
    status: String,
    status_kind: StatusKind,
    dirty: bool,
    hovered_path: Option<PathBuf>,
    highlighter: Highlighter,
    editor_preferences: EditorPreferences,
    editor_preferences_path: PathBuf,
    highlight_cache: Option<HighlightCache>,
    // ── Quick-open / search overlays ─────────────────────────────────────
    /// Cmd+P: file picker overlay
    file_picker_open: bool,
    file_picker_query: String,
    /// Cmd+Shift+F: full-text search overlay
    text_search_open: bool,
    text_search_query: String,
    /// Cached results for the text search (path, line_no, line_text)
    text_search_results: Vec<(PathBuf, usize, String)>,
    /// Whether the text search results are stale and need recomputing
    text_search_dirty: bool,

    // ── Terminal ──────────────────────────────────────────────────────────
    main_view: MainView,
    /// Lazily-created PTY shell session.
    terminal: Option<TerminalSession>,
    /// Parsed display lines: each line is a list of (color, text) spans.
    term_lines: Vec<Vec<(egui::Color32, String)>>,
    /// Spans being built for the current (not-yet-newlined) line.
    term_current_spans: Vec<(egui::Color32, String)>,
    /// Current ANSI foreground color while parsing.
    term_cur_color: egui::Color32,
    /// Incomplete UTF-8 bytes carried across PTY reads.
    term_pending_utf8: Vec<u8>,
    /// Whether the last byte was \r (to detect \r\n pairs).
    term_last_was_cr: bool,
    /// Current input line being typed.
    term_input: String,
    /// Command history (oldest first).
    term_history: Vec<String>,
    /// Index into history when browsing with ↑/↓ (-1 = not browsing).
    term_history_idx: i32,
    /// Saved draft input when browsing history.
    term_history_draft: String,
    /// Whether the terminal output needs to scroll to the bottom.
    term_scroll_bottom: bool,
    /// Whether the terminal input should request focus this frame.
    term_focus_input: bool,
}

#[derive(Default, PartialEq)]
enum StatusKind {
    #[default]
    Info,
    Success,
    Error,
}

impl Default for EditorApp {
    fn default() -> Self {
        let editor_preferences_path = Self::editor_preferences_path();
        let editor_preferences = Self::load_editor_preferences_from(&editor_preferences_path)
            .unwrap_or_default();

        Self {
            project_root: None,
            files: Vec::new(),
            expanded_dirs: Default::default(),
            selected_file: None,
            project_symbols: Vec::new(),
            pending_scroll_line: None,
            editor_text: String::new(),
            original_text: String::new(),
            status: "Ready".to_string(),
            status_kind: StatusKind::Info,
            dirty: false,
            hovered_path: None,
            highlighter: Highlighter::new(),
            editor_preferences,
            editor_preferences_path,
            highlight_cache: None,
            file_picker_open: false,
            file_picker_query: String::new(),
            text_search_open: false,
            text_search_query: String::new(),
            text_search_results: Vec::new(),
            text_search_dirty: false,
            main_view: MainView::Code,
            terminal: None,
            term_lines: Vec::new(),
            term_current_spans: Vec::new(),
            term_cur_color: FG_PRIMARY,
            term_pending_utf8: Vec::new(),
            term_last_was_cr: false,
            term_input: String::new(),
            term_history: Vec::new(),
            term_history_idx: -1,
            term_history_draft: String::new(),
            term_scroll_bottom: false,
            term_focus_input: false,
        }
    }
}

// ── Theme helpers ─────────────────────────────────────────────────────────────

fn apply_ui_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(11.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(DEFAULT_CODE_FONT_SIZE, egui::FontFamily::Monospace),
    );
    style.spacing.item_spacing = egui::vec2(6.0, 2.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.window_margin = egui::Margin::same(0.0);
    style.spacing.indent = TREE_INDENT;
    style.visuals.window_rounding = egui::Rounding::same(4.0);
    style.visuals.menu_rounding = egui::Rounding::same(4.0);
    ctx.set_style(style);
}

fn apply_visuals(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG_BASE;
    v.window_fill = BG_PANEL;
    v.extreme_bg_color = BG_INPUT;
    v.faint_bg_color = BG_SIDEBAR;

    v.widgets.noninteractive.bg_fill = BG_BASE;
    v.widgets.noninteractive.weak_bg_fill = BG_BASE;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, FG_SECONDARY);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER_SUBTLE);

    v.widgets.inactive.bg_fill = BG_BUTTON;
    v.widgets.inactive.weak_bg_fill = BG_BUTTON;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, FG_PRIMARY);
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER_NORMAL);
    v.widgets.inactive.rounding = egui::Rounding::same(3.0);

    v.widgets.hovered.bg_fill = BG_BUTTON_HOVER;
    v.widgets.hovered.weak_bg_fill = BG_BUTTON_HOVER;
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, FG_PRIMARY);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, FG_ACCENT);
    v.widgets.hovered.rounding = egui::Rounding::same(3.0);

    v.widgets.active.bg_fill = BG_ACTIVE;
    v.widgets.active.weak_bg_fill = BG_ACTIVE;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, FG_ACCENT);
    v.widgets.active.rounding = egui::Rounding::same(3.0);

    v.selection.bg_fill = BG_SELECTED;
    v.selection.stroke = egui::Stroke::new(1.0, FG_ACCENT);
    v.hyperlink_color = FG_ACCENT;
    v.override_text_color = Some(FG_PRIMARY);

    ctx.set_visuals(v);
}

// ── EditorApp impl ────────────────────────────────────────────────────────────

impl EditorApp {
    fn open_project(&mut self, path: PathBuf) {
        match scan_project(&path) {
            Ok(files) => {
                self.project_root = Some(path);
                let mut symbols = Vec::new();
                let mut index_err: Option<String> = None;
                match index_rust_files(&files) {
                    Ok(s) => symbols.extend(s),
                    Err(err) => index_err = Some(err.to_string()),
                }
                match index_python_files(&files) {
                    Ok(s) => symbols.extend(s),
                    Err(err) => {
                        if index_err.is_none() {
                            index_err = Some(err.to_string());
                        }
                    }
                }
                if let Some(err) = index_err {
                    self.set_status(
                        format!("Project loaded, symbol index failed: {err}"),
                        StatusKind::Error,
                    );
                }
                self.project_symbols = symbols;
                self.files = files;
                self.expanded_dirs.clear();
                if let Some(root) = &self.project_root {
                    self.expanded_dirs.insert(root.clone());
                }
                if self.status.starts_with("Project loaded, symbol index failed") {
                    return;
                }
                self.set_status(
                    format!(
                        "Project loaded — {} symbols indexed",
                        self.project_symbols.len()
                    ),
                    StatusKind::Success,
                );
            }
            Err(err) => {
                self.set_status(format!("Open failed: {err}"), StatusKind::Error);
            }
        }
    }

    fn open_file(&mut self, path: PathBuf) {
        match load_file(&path) {
            Ok(text) => {
                // Index symbols for this file if it's Python (Rust symbols are
                // indexed project-wide on open_project; Python files opened
                // individually also get their symbols added here).
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if matches!(ext, "py" | "pyw") {
                    // Remove any stale symbols for this file, then re-index.
                    self.project_symbols.retain(|s| s.path != path);
                    use crate::symbols::parse_python_symbols;
                    self.project_symbols
                        .extend(parse_python_symbols(path.clone(), &text));
                }

                self.original_text = text.clone();
                self.editor_text = text;
                self.selected_file = Some(path);
                self.highlight_cache = None;
                self.dirty = false;
                self.set_status("File loaded".to_string(), StatusKind::Success);
            }
            Err(err) => {
                self.set_status(format!("Read failed: {err}"), StatusKind::Error);
            }
        }
    }

    #[allow(dead_code)]
    fn save_current(&mut self) {
        let Some(path) = self.selected_file.clone() else {
            self.set_status("No file selected".to_string(), StatusKind::Error);
            return;
        };
        match save_file(&path, &self.editor_text) {
            Ok(()) => {
                self.original_text = self.editor_text.clone();
                self.dirty = false;
                self.reindex_current_python_file();
                self.set_status("Saved".to_string(), StatusKind::Success);
            }
            Err(err) => {
                self.set_status(format!("Save failed: {err}"), StatusKind::Error);
            }
        }
    }

    fn set_status(&mut self, msg: String, kind: StatusKind) {
        self.status = msg;
        self.status_kind = kind;
    }

    fn source_hash(source: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        hasher.finish()
    }

    fn current_file_name(&self) -> String {
        self.selected_file
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("file.txt")
            .to_string()
    }

    fn highlighted_lines(&mut self, file_name: &str, source_hash: u64) -> Arc<Vec<StyledLine>> {
        if let Some(cache) = &self.highlight_cache {
            if cache.file_name == file_name && cache.source_hash == source_hash {
                return Arc::clone(&cache.lines);
            }
        }

        let lines = Arc::new(self.highlighter.highlight(&self.editor_text, file_name));
        self.highlight_cache = Some(HighlightCache {
            file_name: file_name.to_string(),
            source_hash,
            lines: Arc::clone(&lines),
        });
        lines
    }

    fn reindex_current_python_file(&mut self) {
        let Some(path) = self.selected_file.clone() else {
            return;
        };
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "py" | "pyw") {
            return;
        }

        self.project_symbols.retain(|symbol| symbol.path != path);
        self.project_symbols
            .extend(crate::symbols::parse_python_symbols(path, &self.editor_text));
    }

    fn project_label(path: &Path) -> String {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .unwrap_or_else(|| path.display().to_string())
    }

    fn editor_title_label(path: &Path) -> String {
        path.display().to_string()
    }

    fn python_navigation_symbols(&self) -> Vec<Symbol> {
        let Some(selected) = self.selected_file.as_ref() else {
            return Vec::new();
        };
        if selected.extension().and_then(|ext| ext.to_str()) != Some("py")
            && selected.extension().and_then(|ext| ext.to_str()) != Some("pyw")
        {
            return Vec::new();
        }

        self.project_symbols
            .iter()
            .filter(|symbol| {
                symbol.path == *selected
                    && matches!(symbol.kind, crate::symbols::SymbolKind::Method | crate::symbols::SymbolKind::Function)
            })
            .cloned()
            .collect()
    }

    fn char_index_for_line(text: &str, line_no: usize) -> usize {
        if line_no <= 1 {
            return 0;
        }

        let mut char_index = 0;
        let mut current_line = 1;
        for ch in text.chars() {
            if ch == '\n' {
                current_line += 1;
                if current_line == line_no {
                    return char_index + 1;
                }
            }
            char_index += 1;
        }

        char_index
    }

    fn line_number_gutter_width(line_count: usize) -> f32 {
        let digits = line_count.max(1).to_string().len() as f32;
        digits * LINE_NUMBER_CHAR_W + LINE_NUMBER_PAD_X * 2.0
    }

    fn build_tree(&self) -> Vec<TreeNode> {
        let Some(root) = self.project_root.as_ref() else {
            return Vec::new();
        };

        let mut tree: BTreeMap<PathBuf, DirEntry> = BTreeMap::new();
        for file in &self.files {
            let parent = file.parent().unwrap_or(root.as_path()).to_path_buf();
            tree.entry(parent.clone())
                .or_default()
                .files
                .push(file.clone());

            let mut current = root.clone();
            let Ok(relative_parent) = parent.strip_prefix(root) else {
                continue;
            };
            for part in relative_parent {
                let next = current.join(part);
                tree.entry(current.clone())
                    .or_default()
                    .dirs
                    .insert(next.clone());
                tree.entry(next.clone()).or_default();
                current = next;
            }
        }

        let mut nodes = Vec::new();
        self.push_dir_nodes(root.as_path(), 0, &tree, &mut nodes);
        nodes
    }

    fn push_dir_nodes(
        &self,
        dir: &Path,
        depth: usize,
        tree: &BTreeMap<PathBuf, DirEntry>,
        nodes: &mut Vec<TreeNode>,
    ) {
        let is_open = self.expanded_dirs.contains(dir);
        nodes.push(TreeNode::Dir {
            path: dir.to_path_buf(),
            depth,
            is_open,
        });
        if !is_open {
            return;
        }
        let Some(entry) = tree.get(dir) else {
            return;
        };
        for child_dir in &entry.dirs {
            self.push_dir_nodes(child_dir, depth + 1, tree, nodes);
        }
        for file in &entry.files {
            nodes.push(TreeNode::File {
                path: file.clone(),
                depth: depth + 1,
            });
        }
    }

    fn toggle_dir(&mut self, path: &Path) {
        if !self.expanded_dirs.remove(path) {
            self.expanded_dirs.insert(path.to_path_buf());
        }
    }

    fn editor_preferences_path() -> PathBuf {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".config")
                .join("deskagent")
                .join("editor_prefs.json");
        }
        PathBuf::from("editor_prefs.json")
    }

    fn load_editor_preferences_from(path: &Path) -> anyhow::Result<EditorPreferences> {
        let text = fs::read_to_string(path).context("read editor preferences failed")?;
        Ok(serde_json::from_str(&text).context("parse editor preferences failed")?)
    }

    fn save_editor_preferences(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.editor_preferences_path.parent() {
            fs::create_dir_all(parent).context("create editor preferences dir failed")?;
        }
        let text = serde_json::to_string_pretty(&self.editor_preferences)
            .context("serialize editor preferences failed")?;
        fs::write(&self.editor_preferences_path, text).context("write editor preferences failed")?;
        Ok(())
    }

    fn current_editor_settings(&self) -> EditorFormatSettings {
        if let Some(selected) = self.selected_file.as_ref() {
            if let Some(settings) = self.editor_preferences.files.get(&selected.display().to_string()) {
                return settings.clone();
            }
        }
        self.editor_preferences.global.clone()
    }

    fn current_editor_settings_mut(&mut self) -> &mut EditorFormatSettings {
        if let Some(selected) = self.selected_file.as_ref() {
            let key = selected.display().to_string();
            return self
                .editor_preferences
                .files
                .entry(key)
                .or_insert_with(|| self.editor_preferences.global.clone());
        }
        &mut self.editor_preferences.global
    }

    #[allow(dead_code)]
    fn jump_to_python_symbol_line(&mut self, line_no: usize) -> bool {
        let Some(selected) = self.selected_file.as_ref() else {
            return false;
        };

        let Some(symbol) = self
            .project_symbols
            .iter()
            .find(|symbol| {
                symbol.path == *selected
                    && symbol.line == line_no
                    && matches!(symbol.kind, crate::symbols::SymbolKind::Method | crate::symbols::SymbolKind::Function)
            })
            .cloned()
        else {
            return false;
        };

        self.pending_scroll_line = Some(line_no);
        self.set_status(format!("Jumped to {}:{}", symbol.name, line_no), StatusKind::Info);
        true
    }

    // ── File explorer ─────────────────────────────────────────────────────────

    fn render_file_tree(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("EXPLORER")
                    .size(10.5)
                    .color(FG_DIM)
                    .strong(),
            );
        });
        ui.add_space(4.0);

        let sep_rect =
            egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 1.0));
        ui.painter().rect_filled(sep_rect, 0.0, BORDER_SUBTLE);
        ui.add_space(1.0);

        if self.project_root.is_none() {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("Open a project to browse files")
                        .size(12.0)
                        .color(FG_DIM),
                );
            });
            return;
        }

        let nodes = self.build_tree();
        let mut file_to_open: Option<PathBuf> = None;
        let mut dir_to_toggle: Option<PathBuf> = None;
        let mut new_hovered: Option<PathBuf> = None;

        egui::ScrollArea::vertical()
            .id_source("file_tree_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                for node in &nodes {
                    match node {
                        TreeNode::Dir {
                            path,
                            depth,
                            is_open,
                        } => {
                            let indent = 8.0 + TREE_INDENT * (*depth as f32);
                            let label = Self::project_label(path);
                            let is_hovered =
                                self.hovered_path.as_deref() == Some(path.as_path());
                            let bg = if is_hovered {
                                BG_HOVER
                            } else {
                                egui::Color32::TRANSPARENT
                            };

                            let (row_rect, row_resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), TREE_ROW_HEIGHT),
                                egui::Sense::click(),
                            );
                            if row_resp.hovered() {
                                new_hovered = Some(path.clone());
                            }

                            ui.painter().rect_filled(row_rect, 0.0, bg);

                            if *is_open {
                                let bar = egui::Rect::from_min_size(
                                    row_rect.min,
                                    egui::vec2(2.0, row_rect.height()),
                                );
                                ui.painter().rect_filled(bar, 0.0, FG_ACCENT);
                            }

                            let arrow = if *is_open { "▾" } else { "▸" };
                            let arrow_color = if *is_open { FG_ACCENT } else { FG_SECONDARY };
                            ui.painter().text(
                                egui::pos2(row_rect.min.x + indent, row_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                arrow,
                                egui::FontId::new(11.0, egui::FontFamily::Proportional),
                                arrow_color,
                            );

                            let text_color = if is_hovered { FG_PRIMARY } else { FG_DIR };
                            ui.painter().text(
                                egui::pos2(
                                    row_rect.min.x + indent + TREE_ICON_W,
                                    row_rect.center().y,
                                ),
                                egui::Align2::LEFT_CENTER,
                                &label,
                                egui::FontId::new(13.0, egui::FontFamily::Proportional),
                                text_color,
                            );

                            if row_resp.clicked() {
                                dir_to_toggle = Some(path.clone());
                            }
                        }

                        TreeNode::File { path, depth } => {
                            let indent = 8.0 + TREE_INDENT * (*depth as f32);
                            let label = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("")
                                .to_string();

                            let is_selected = self
                                .selected_file
                                .as_deref()
                                .map(|s| s == path.as_path())
                                .unwrap_or(false);
                            let is_hovered =
                                self.hovered_path.as_deref() == Some(path.as_path());

                            let bg = if is_selected {
                                BG_SELECTED
                            } else if is_hovered {
                                BG_HOVER
                            } else {
                                egui::Color32::TRANSPARENT
                            };

                            let (row_rect, row_resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), TREE_ROW_HEIGHT),
                                egui::Sense::click(),
                            );
                            if row_resp.hovered() {
                                new_hovered = Some(path.clone());
                            }

                            ui.painter().rect_filled(row_rect, 0.0, bg);

                            if is_selected {
                                let bar = egui::Rect::from_min_size(
                                    row_rect.min,
                                    egui::vec2(2.0, row_rect.height()),
                                );
                                ui.painter().rect_filled(bar, 0.0, FG_ACCENT);
                            }

                            let icon_color = if is_selected {
                                FG_ACCENT
                            } else if is_hovered {
                                FG_SECONDARY
                            } else {
                                FG_DIM
                            };
                            ui.painter().circle_filled(
                                egui::pos2(
                                    row_rect.min.x + indent + 6.0,
                                    row_rect.center().y,
                                ),
                                2.5,
                                icon_color,
                            );

                            let text_color = if is_selected {
                                FG_SELECTED
                            } else if is_hovered {
                                FG_PRIMARY
                            } else {
                                FG_FILE
                            };
                            ui.painter().text(
                                egui::pos2(
                                    row_rect.min.x + indent + TREE_ICON_W,
                                    row_rect.center().y,
                                ),
                                egui::Align2::LEFT_CENTER,
                                &label,
                                egui::FontId::new(13.0, egui::FontFamily::Proportional),
                                text_color,
                            );

                            if row_resp.clicked() {
                                file_to_open = Some(path.clone());
                            }
                        }
                    }
                }
            });

        self.hovered_path = new_hovered;
        if let Some(path) = dir_to_toggle {
            self.toggle_dir(&path);
        }
        if let Some(path) = file_to_open {
            self.open_file(path);
        }
    }

    // ── Read-only syntax-highlighted code viewer ──────────────────────────────

    fn render_editor_toolbar(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let mut save_as_global = false;
        let mut reset_file = false;
        let is_file_override = self
            .selected_file
            .as_ref()
            .map(|path| self.editor_preferences.files.contains_key(&path.display().to_string()))
            .unwrap_or(false);

        egui::Frame::none()
            .fill(egui::Color32::from_rgb(22, 22, 26))
            .inner_margin(egui::Margin::symmetric(10.0, 5.0))
            .stroke(egui::Stroke::new(1.0, BORDER_SUBTLE))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let settings = self.current_editor_settings_mut();
                    ui.label(egui::RichText::new("Font").size(11.0).color(FG_DIM));
                    egui::ComboBox::from_id_source("editor_font_family")
                        .selected_text(settings.font_family.label())
                        .width(118.0)
                        .show_ui(ui, |ui| {
                            for family in EditorFontFamily::ALL {
                                changed |= ui
                                    .selectable_value(&mut settings.font_family, family, family.label())
                                    .changed();
                            }
                        });

                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Size").size(11.0).color(FG_DIM));
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut settings.font_size)
                                .speed(0.5)
                                .range(10.0..=28.0),
                        )
                        .changed();

                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Line").size(11.0).color(FG_DIM));
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut settings.line_height)
                                .speed(0.05)
                                .range(1.0..=2.2),
                        )
                        .changed();

                    ui.add_space(8.0);
                    changed |= ui.checkbox(&mut settings.wrap_lines, "Wrap").changed();

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Set Global").clicked() {
                            save_as_global = true;
                        }
                        if is_file_override && ui.button("Reset File").clicked() {
                            reset_file = true;
                        }
                    });
                });
            });

        if save_as_global {
            self.editor_preferences.global = self.current_editor_settings();
            changed = true;
        }
        if reset_file {
            if let Some(selected) = self.selected_file.as_ref() {
                self.editor_preferences.files.remove(&selected.display().to_string());
                changed = true;
            }
        }
        if changed {
            match self.save_editor_preferences() {
                Ok(()) => self.set_status("Editor format saved".to_string(), StatusKind::Success),
                Err(err) => self.set_status(format!("Editor format save failed: {err}"), StatusKind::Error),
            }
        }
    }

    fn render_editor(&mut self, ui: &mut egui::Ui) {
        if self.editor_text.is_empty() {
            ui.add_space(32.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("Select a file to view")
                        .size(14.0)
                        .color(FG_DIM),
                );
            });
            return;
        }

        self.render_editor_toolbar(ui);
        ui.add_space(2.0);

        let code_editor_id = ui.make_persistent_id("code_viewer_text_edit");
        if let Some(target_line) = self.pending_scroll_line.take() {
            let char_index = Self::char_index_for_line(&self.editor_text, target_line);
            let mut state =
                egui::TextEdit::load_state(ui.ctx(), code_editor_id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(CCursor::new(char_index))));
            state.store(ui.ctx(), code_editor_id);
            ui.memory_mut(|memory| memory.request_focus(code_editor_id));
        }

        let settings = self.current_editor_settings();
        let editor_font = egui::FontId::new(settings.font_size, settings.font_family.egui_family());
        let row_height = settings.font_size * settings.line_height;
        let file_name = self.current_file_name();
        let source_hash = Self::source_hash(&self.editor_text);
        let highlight_lines = self.highlighted_lines(&file_name, source_hash);
        let line_count = highlight_lines.len().max(1);
        let editor_line_count = self.editor_text.lines().count().max(1);
        let gutter_width = Self::line_number_gutter_width(line_count);
        let python_symbols = self.python_navigation_symbols();
        let python_symbol_lines: BTreeMap<usize, String> = python_symbols
            .iter()
            .map(|symbol| (symbol.line, symbol.name.clone()))
            .collect();

        egui::ScrollArea::both()
            .id_source("code_viewer_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut layouter = |ui: &egui::Ui, text: &str, wrap_width: f32| {
                    let mut job = LayoutJob::default();
                    job.wrap.max_width = if settings.wrap_lines {
                        wrap_width
                    } else {
                        f32::INFINITY
                    };
                    job.break_on_newline = true;

                    let lines: Vec<&str> = text.lines().collect();
                    for (line_idx, line) in lines.iter().enumerate() {
                        if let Some(spans) = highlight_lines.get(line_idx) {
                            for (color, fragment) in spans {
                                job.append(
                                    fragment,
                                    0.0,
                                    TextFormat {
                                        font_id: editor_font.clone(),
                                        color: *color,
                                        extra_letter_spacing: 0.0,
                                        line_height: Some(row_height),
                                        ..Default::default()
                                    },
                                );
                            }
                        } else {
                            job.append(
                                line,
                                0.0,
                                TextFormat {
                                    font_id: editor_font.clone(),
                                    color: FG_PRIMARY,
                                    extra_letter_spacing: 0.0,
                                    line_height: Some(row_height),
                                    ..Default::default()
                                },
                            );
                        }
                        if line_idx + 1 < lines.len() {
                            job.append(
                                "\n",
                                0.0,
                                TextFormat {
                                    font_id: editor_font.clone(),
                                    color: FG_PRIMARY,
                                    extra_letter_spacing: 0.0,
                                    line_height: Some(row_height),
                                    ..Default::default()
                                },
                            );
                        }
                    }

                    ui.fonts(|fonts| fonts.layout_job(job))
                };

                let editor = egui::TextEdit::multiline(&mut self.editor_text)
                    .id(code_editor_id)
                    .code_editor()
                    .margin(egui::Margin {
                        left: gutter_width,
                        right: 6.0,
                        top: 4.0,
                        bottom: 4.0,
                    })
                    .desired_width(f32::INFINITY)
                    .desired_rows(editor_line_count)
                    .font(egui::TextStyle::Monospace)
                    .text_color(FG_PRIMARY)
                    .layouter(&mut layouter)
                    .cursor_at_end(false);
                let output = editor.show(ui);

                if output.response.changed() {
                    self.dirty = self.editor_text != self.original_text;
                    self.highlight_cache = None;
                    if matches!(
                        self.selected_file.as_ref().and_then(|path| path.extension().and_then(|e| e.to_str())),
                        Some("py" | "pyw")
                    ) {
                        self.reindex_current_python_file();
                    }
                }

                let painter = ui.painter();
                let text_clip = output.text_clip_rect;
                let galley = output.galley;
                let gutter_left = text_clip.left() - gutter_width;
                let gutter_right = text_clip.left() - 8.0;
                let start_y = output.galley_pos.y;
                for (line_idx, row) in galley.rows.iter().enumerate() {
                    let y_center = start_y + row.rect.center().y;
                    let line_no = line_idx + 1;
                    let line_num = format!("{}", line_no);
                    let line_rect = egui::Rect::from_min_max(
                        egui::pos2(gutter_left, start_y + row.rect.min.y),
                        egui::pos2(text_clip.left(), start_y + row.rect.max.y),
                    );
                    let response = ui.interact(
                        line_rect,
                        code_editor_id.with(("line_number", line_no)),
                        egui::Sense::click(),
                    );
                    if response.clicked() && python_symbol_lines.contains_key(&line_no) {
                        self.pending_scroll_line = Some(line_no);
                        let char_index = Self::char_index_for_line(&self.editor_text, line_no);
                        let mut state = egui::TextEdit::load_state(ui.ctx(), code_editor_id)
                            .unwrap_or_default();
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                            CCursor::new(char_index),
                        )));
                        state.store(ui.ctx(), code_editor_id);
                        ui.memory_mut(|memory| memory.request_focus(code_editor_id));
                    }
                    if let Some(symbol_name) = python_symbol_lines.get(&line_no) {
                        response.on_hover_text(format!("Jump to {symbol_name}:{line_no}"));
                        painter.text(
                            egui::pos2(gutter_right, y_center),
                            egui::Align2::RIGHT_CENTER,
                            line_num,
                            editor_font.clone(),
                            FG_ACCENT,
                        );
                    } else {
                        painter.text(
                            egui::pos2(gutter_right, y_center),
                            egui::Align2::RIGHT_CENTER,
                            line_num,
                            editor_font.clone(),
                            FG_DIM,
                        );
                    }
                }

                painter.line_segment(
                    [
                        egui::pos2(gutter_left + gutter_width - 1.0, text_clip.top()),
                        egui::pos2(gutter_left + gutter_width - 1.0, text_clip.bottom()),
                    ],
                    egui::Stroke::new(1.0, BORDER_SUBTLE),
                );
            });
    }

    // ── File picker overlay (Cmd+P) ───────────────────────────────────────────

    fn render_file_picker(&mut self, ctx: &egui::Context) {
        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.file_picker_open = false;
            return;
        }

        let query = self.file_picker_query.to_lowercase();

        // Fuzzy-filter: every character in query must appear in order in the
        // candidate string (subsequence match), case-insensitive.
        let matches: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|p| {
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if query.is_empty() {
                    return true;
                }
                fuzzy_match(&name, &query)
            })
            .take(20)
            .cloned()
            .collect();

        let mut file_to_open: Option<PathBuf> = None;
        let mut close = false;

        egui::Window::new("file_picker_window")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
            .fixed_size(egui::vec2(520.0, 0.0))
            .frame(
                egui::Frame::none()
                    .fill(BG_PANEL)
                    .stroke(egui::Stroke::new(1.0, FG_ACCENT))
                    .rounding(egui::Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(0.0)),
            )
            .show(ctx, |ui| {
                // ── Search input ──────────────────────────────────────────────
                let input_frame = egui::Frame::none()
                    .fill(BG_INPUT)
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0));
                input_frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("⌘P")
                                .size(11.0)
                                .color(FG_ACCENT)
                                .monospace(),
                        );
                        ui.add_space(6.0);
                        let edit = egui::TextEdit::singleline(&mut self.file_picker_query)
                            .desired_width(f32::INFINITY)
                            .hint_text(
                                egui::RichText::new("搜索文件名…").color(FG_DIM),
                            )
                            .font(egui::TextStyle::Monospace)
                            .text_color(FG_PRIMARY)
                            .frame(false);
                        let resp = ui.add(edit);
                        // Auto-focus the input when the overlay first opens
                        resp.request_focus();
                    });
                });

                // Separator
                let sep = egui::Rect::from_min_size(
                    ui.cursor().min,
                    egui::vec2(ui.available_width(), 1.0),
                );
                ui.painter().rect_filled(sep, 0.0, BORDER_SUBTLE);

                // ── Results list ──────────────────────────────────────────────
                if matches.is_empty() && !self.file_picker_query.is_empty() {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new("无匹配文件")
                                .size(12.0)
                                .color(FG_DIM),
                        );
                    });
                    ui.add_space(12.0);
                } else {
                    egui::ScrollArea::vertical()
                        .id_source("file_picker_results")
                        .max_height(320.0)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            for path in &matches {
                                let name = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("");
                                let dir = path
                                    .parent()
                                    .and_then(|p| p.to_str())
                                    .unwrap_or("");

                                let (row_rect, row_resp) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 30.0),
                                    egui::Sense::click(),
                                );
                                let bg = if row_resp.hovered() {
                                    BG_HOVER
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                ui.painter().rect_filled(row_rect, 0.0, bg);

                                // File name (left, prominent)
                                ui.painter().text(
                                    egui::pos2(row_rect.min.x + 14.0, row_rect.center().y - 6.0),
                                    egui::Align2::LEFT_CENTER,
                                    name,
                                    egui::FontId::new(13.0, egui::FontFamily::Monospace),
                                    FG_PRIMARY,
                                );
                                // Directory path (below, dim)
                                ui.painter().text(
                                    egui::pos2(row_rect.min.x + 14.0, row_rect.center().y + 7.0),
                                    egui::Align2::LEFT_CENTER,
                                    dir,
                                    egui::FontId::new(10.5, egui::FontFamily::Proportional),
                                    FG_DIM,
                                );

                                if row_resp.clicked() {
                                    file_to_open = Some(path.clone());
                                    close = true;
                                }
                            }
                        });
                }

                // Footer hint
                let footer_frame = egui::Frame::none()
                    .fill(egui::Color32::from_rgb(22, 22, 26))
                    .inner_margin(egui::Margin::symmetric(12.0, 5.0));
                footer_frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("↵ 打开   Esc 关闭")
                                .size(10.5)
                                .color(FG_DIM),
                        );
                    });
                });
            });

        if let Some(path) = file_to_open {
            self.open_file(path);
        }
        if close {
            self.file_picker_open = false;
        }
    }

    // ── Full-text search overlay (Cmd+Shift+F) ────────────────────────────────

    fn render_text_search(&mut self, ctx: &egui::Context) {
        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.text_search_open = false;
            return;
        }

        // Recompute results when query changes
        if self.text_search_dirty {
            self.text_search_dirty = false;
            let q = self.text_search_query.to_lowercase();
            self.text_search_results.clear();
            if !q.is_empty() {
                'outer: for path in &self.files {
                    if let Ok(source) = std::fs::read_to_string(path) {
                        for (line_idx, line) in source.lines().enumerate() {
                            if fuzzy_match(&line.to_lowercase(), &q) {
                                self.text_search_results
                                    .push((path.clone(), line_idx + 1, line.to_string()));
                                if self.text_search_results.len() >= 200 {
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut jump_to: Option<(PathBuf, usize)> = None;
        let mut close = false;

        egui::Window::new("text_search_window")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
            .fixed_size(egui::vec2(600.0, 0.0))
            .frame(
                egui::Frame::none()
                    .fill(BG_PANEL)
                    .stroke(egui::Stroke::new(1.0, FG_ACCENT))
                    .rounding(egui::Rounding::same(6.0))
                    .inner_margin(egui::Margin::same(0.0)),
            )
            .show(ctx, |ui| {
                // ── Search input ──────────────────────────────────────────────
                let input_frame = egui::Frame::none()
                    .fill(BG_INPUT)
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0));
                input_frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("⌘⇧F")
                                .size(11.0)
                                .color(FG_ACCENT)
                                .monospace(),
                        );
                        ui.add_space(6.0);
                        let prev_query = self.text_search_query.clone();
                        let edit = egui::TextEdit::singleline(&mut self.text_search_query)
                            .desired_width(f32::INFINITY)
                            .hint_text(
                                egui::RichText::new("搜索关键字…").color(FG_DIM),
                            )
                            .font(egui::TextStyle::Monospace)
                            .text_color(FG_PRIMARY)
                            .frame(false);
                        let resp = ui.add(edit);
                        resp.request_focus();
                        if self.text_search_query != prev_query {
                            self.text_search_dirty = true;
                        }
                    });
                });

                // Separator
                let sep = egui::Rect::from_min_size(
                    ui.cursor().min,
                    egui::vec2(ui.available_width(), 1.0),
                );
                ui.painter().rect_filled(sep, 0.0, BORDER_SUBTLE);

                // ── Results ───────────────────────────────────────────────────
                let q = self.text_search_query.clone();
                if q.is_empty() {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new("输入关键字开始搜索")
                                .size(12.0)
                                .color(FG_DIM),
                        );
                    });
                    ui.add_space(12.0);
                } else if self.text_search_results.is_empty() {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new("无匹配结果")
                                .size(12.0)
                                .color(FG_DIM),
                        );
                    });
                    ui.add_space(12.0);
                } else {
                    let result_count = self.text_search_results.len();
                    // Header: result count
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        let label = if result_count >= 200 {
                            format!("200+ 条结果（已截断）")
                        } else {
                            format!("{result_count} 条结果")
                        };
                        ui.label(
                            egui::RichText::new(label)
                                .size(10.5)
                                .color(FG_DIM),
                        );
                    });

                    egui::ScrollArea::vertical()
                        .id_source("text_search_results")
                        .max_height(360.0)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            // Clone to avoid borrow conflict
                            let results = self.text_search_results.clone();
                            for (path, line_no, line_text) in &results {
                                let file_name = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("");
                                let display_line = line_text.trim();
                                // Truncate long lines
                                let display_line = if display_line.len() > 80 {
                                    &display_line[..80]
                                } else {
                                    display_line
                                };

                                let (row_rect, row_resp) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 32.0),
                                    egui::Sense::click(),
                                );
                                let bg = if row_resp.hovered() {
                                    BG_HOVER
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                ui.painter().rect_filled(row_rect, 0.0, bg);

                                // File:line badge
                                let badge_text = format!("{file_name}:{line_no}");
                                ui.painter().text(
                                    egui::pos2(row_rect.min.x + 14.0, row_rect.center().y - 6.0),
                                    egui::Align2::LEFT_CENTER,
                                    &badge_text,
                                    egui::FontId::new(11.0, egui::FontFamily::Monospace),
                                    FG_ACCENT,
                                );
                                // Line content
                                ui.painter().text(
                                    egui::pos2(row_rect.min.x + 14.0, row_rect.center().y + 7.0),
                                    egui::Align2::LEFT_CENTER,
                                    display_line,
                                    egui::FontId::new(11.5, egui::FontFamily::Monospace),
                                    FG_SECONDARY,
                                );

                                if row_resp.clicked() {
                                    jump_to = Some((path.clone(), *line_no));
                                    close = true;
                                }
                            }
                        });
                }

                // Footer hint
                let footer_frame = egui::Frame::none()
                    .fill(egui::Color32::from_rgb(22, 22, 26))
                    .inner_margin(egui::Margin::symmetric(12.0, 5.0));
                footer_frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("↵ 跳转   Esc 关闭")
                                .size(10.5)
                                .color(FG_DIM),
                        );
                    });
                });
            });

        if let Some((path, line_no)) = jump_to {
            self.open_file(path.clone());
            self.pending_scroll_line = Some(line_no);
            self.set_status(
                format!(
                    "跳转到 {}:{}",
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(""),
                    line_no
                ),
                StatusKind::Info,
            );
        }
        if close {
            self.text_search_open = false;
        }
    }

    // ── Terminal: ANSI byte-stream parser ────────────────────────────────────
    //
    // Converts raw PTY bytes into a list of colored display lines.
    // Handles the most common SGR sequences (colors 30-37, 90-97, 256-color,
    // bold, reset) and strips all other escape sequences silently.

    fn ingest_raw_bytes(&mut self, raw: Vec<u8>) {
        self.term_pending_utf8.extend(raw);

        let mut decoded = String::new();
        loop {
            match std::str::from_utf8(&self.term_pending_utf8) {
                Ok(text) => {
                    decoded.push_str(text);
                    self.term_pending_utf8.clear();
                    break;
                }
                Err(err) => {
                    let valid_up_to = err.valid_up_to();
                    if valid_up_to > 0 {
                        decoded.push_str(
                            std::str::from_utf8(&self.term_pending_utf8[..valid_up_to])
                                .expect("valid utf-8 prefix"),
                        );
                        self.term_pending_utf8.drain(..valid_up_to);
                    }
                    match err.error_len() {
                        Some(len) => {
                            decoded.push('\u{fffd}');
                            let drain_len = len.min(self.term_pending_utf8.len());
                            self.term_pending_utf8.drain(..drain_len);
                        }
                        None => break,
                    }
                }
            }
        }

        let mut chars = decoded.chars().peekable();

        while let Some(ch) = chars.next() {
            // ── Handle \r\n as a single newline ──────────────────────────
            // PTY always sends \r\n for line endings.  We must NOT clear the
            // current line on \r; instead we flush on \n (and ignore the \r
            // that immediately precedes it).
            if ch == '\r' {
                self.term_last_was_cr = true;
                continue;
            }

            match ch {
                // ESC – ANSI escape sequence
                '\x1b' => {
                    self.term_last_was_cr = false;
                    match chars.peek() {
                        Some('[') => {
                            chars.next(); // consume '['
                            let mut params = String::new();
                            loop {
                                match chars.peek() {
                                    // Parameter bytes 0x30-0x3F
                                    Some(&c) if ('\x30'..='\x3f').contains(&c) => {
                                        params.push(c);
                                        chars.next();
                                    }
                                    // Intermediate bytes 0x20-0x2F – skip
                                    Some(&c) if ('\x20'..='\x2f').contains(&c) => {
                                        chars.next();
                                    }
                                    _ => break,
                                }
                            }
                            let final_byte = chars.next();
                            if final_byte == Some('m') {
                                self.apply_sgr(&params);
                            }
                            // All other CSI sequences (cursor move, erase, etc.) discarded
                        }
                        Some(']') => {
                            // OSC – consume until BEL or ESC-backslash
                            chars.next();
                            loop {
                                match chars.next() {
                                    None | Some('\x07') => break,
                                    Some('\x1b') => { chars.next(); break; }
                                    _ => {}
                                }
                            }
                        }
                        _ => { chars.next(); } // unknown two-char sequence
                    }
                }

                // Newline – flush current spans as a completed line
                '\n' => {
                    // If preceded by \r (i.e. \r\n), the \r was already
                    // consumed above without clearing anything – correct.
                    self.term_last_was_cr = false;
                    let mut spans = std::mem::take(&mut self.term_current_spans);
                    // Remove trailing empty spans
                    while spans.last().map(|(_, t): &(_, String)| t.is_empty()).unwrap_or(false) {
                        spans.pop();
                    }
                    self.term_lines.push(spans);
                    // Reset color to default at line boundary (matches terminal behaviour)
                    self.term_cur_color = FG_PRIMARY;
                    // Keep buffer bounded (~5000 lines)
                    if self.term_lines.len() > 5000 {
                        self.term_lines.drain(0..500);
                    }
                }

                // Backspace
                '\x08' => {
                    self.term_last_was_cr = false;
                    // Remove last character from the last non-empty span
                    if let Some(last) = self.term_current_spans.last_mut() {
                        last.1.pop();
                    }
                }

                // Bell – ignore
                '\x07' => { self.term_last_was_cr = false; }

                // Printable character – append to current span
                c => {
                    self.term_last_was_cr = false;
                    let color = self.term_cur_color;
                    // Reuse the last span if it has the same color, else start a new one
                    match self.term_current_spans.last_mut() {
                        Some(last) if last.0 == color => last.1.push(c),
                        _ => self.term_current_spans.push((color, c.to_string())),
                    }
                }
            }
        }
    }

    /// Parse SGR (Select Graphic Rendition) parameters and update `term_cur_color`.
    fn apply_sgr(&mut self, params: &str) {
        if params.is_empty() || params == "0" {
            self.term_cur_color = FG_PRIMARY;
            return;
        }
        let nums: Vec<u32> = params
            .split(';')
            .filter_map(|s| s.parse().ok())
            .collect();
        let mut i = 0;
        while i < nums.len() {
            match nums[i] {
                0 => self.term_cur_color = FG_PRIMARY,
                1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 => {} // attributes – ignore
                // Standard foreground 30-37
                30 => self.term_cur_color = egui::Color32::from_rgb(  0,   0,   0),
                31 => self.term_cur_color = egui::Color32::from_rgb(205,  49,  49),
                32 => self.term_cur_color = egui::Color32::from_rgb( 13, 188, 121),
                33 => self.term_cur_color = egui::Color32::from_rgb(229, 229,  16),
                34 => self.term_cur_color = egui::Color32::from_rgb( 36, 114, 200),
                35 => self.term_cur_color = egui::Color32::from_rgb(188,  63, 188),
                36 => self.term_cur_color = egui::Color32::from_rgb( 17, 168, 205),
                37 => self.term_cur_color = egui::Color32::from_rgb(229, 229, 229),
                // Bright foreground 90-97
                90 => self.term_cur_color = egui::Color32::from_rgb(102, 102, 102),
                91 => self.term_cur_color = egui::Color32::from_rgb(241,  76,  76),
                92 => self.term_cur_color = egui::Color32::from_rgb( 35, 209, 139),
                93 => self.term_cur_color = egui::Color32::from_rgb(245, 245,  67),
                94 => self.term_cur_color = egui::Color32::from_rgb( 59, 142, 234),
                95 => self.term_cur_color = egui::Color32::from_rgb(214, 112, 214),
                96 => self.term_cur_color = egui::Color32::from_rgb( 41, 184, 219),
                97 => self.term_cur_color = egui::Color32::from_rgb(229, 229, 229),
                // 38;5;n – 256-color
                38 if i + 2 < nums.len() && nums[i + 1] == 5 => {
                    self.term_cur_color = ansi_256_color(nums[i + 2]);
                    i += 2;
                }
                // 38;2;r;g;b – true-color
                38 if i + 4 < nums.len() && nums[i + 1] == 2 => {
                    self.term_cur_color = egui::Color32::from_rgb(
                        nums[i + 2] as u8,
                        nums[i + 3] as u8,
                        nums[i + 4] as u8,
                    );
                    i += 4;
                }
                39 => self.term_cur_color = FG_PRIMARY, // default fg
                // Background codes 40-49, 100-107 – ignore
                40..=49 | 100..=107 => {}
                _ => {}
            }
            i += 1;
        }
    }

    // ── Terminal pane UI ──────────────────────────────────────────────────────
    // Called from inside the outer CentralPanel's ui closure.
    // Uses manual layout (allocate_ui_with_layout) instead of nested Panels
    // to avoid egui's layout height debug_assert when the window is small.

    fn terminal_display_cwd(&self) -> Option<String> {
        for spans in self.term_lines.iter().rev() {
            let line: String = spans.iter().map(|(_, text)| text.as_str()).collect();
            let mut parts = line.split_whitespace();
            let first = parts.next()?;
            let second = parts.next()?;
            if first.contains('@')
                && (second.starts_with('/') || second.starts_with('~') || second == "." || second == "..")
            {
                return Some(second.to_string());
            }
        }

        self.terminal.as_ref().map(|sess| sess.cwd.clone())
    }

    fn render_terminal(&mut self, ui: &mut egui::Ui) {
        // ── Drain PTY output and parse ────────────────────────────────────
        let raw = self
            .terminal
            .as_ref()
            .map(|s| s.drain_raw())
            .unwrap_or_default();
        if !raw.is_empty() {
            self.ingest_raw_bytes(raw);
            self.term_scroll_bottom = true;
        }

        // Request repaint frequently so output appears promptly
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));

        // Total available rect from the outer CentralPanel
        let total_rect = ui.available_rect_before_wrap();
        let total_h = total_rect.height();
        if total_h < 2.0 {
            return; // window too small, nothing to draw
        }

        const TITLE_H: f32 = 28.0;
        let output_h = (total_h - TITLE_H).max(0.0);

        // ── Title bar ─────────────────────────────────────────────────────
        let title_rect = egui::Rect::from_min_size(
            total_rect.min,
            egui::vec2(total_rect.width(), TITLE_H),
        );
        ui.allocate_ui_at_rect(title_rect, |ui| {
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(20, 20, 22))
                .inner_margin(egui::Margin::symmetric(14.0, 4.0))
                .stroke(egui::Stroke::new(1.0, BORDER_SUBTLE))
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(ui.available_width(), TITLE_H - 2.0));
                    ui.horizontal(|ui| {
                        // Traffic-light dots (macOS style)
                        let base = ui.cursor().min;
                        ui.painter().circle_filled(
                            base + egui::vec2(6.0, 8.0),
                            5.0,
                            egui::Color32::from_rgb(255, 95, 86),
                        );
                        ui.add_space(14.0);
                        ui.painter().circle_filled(
                            base + egui::vec2(12.0, 8.0),
                            5.0,
                            egui::Color32::from_rgb(255, 189, 46),
                        );
                        ui.add_space(14.0);
                        ui.painter().circle_filled(
                            base + egui::vec2(28.0, 8.0),
                            5.0,
                            egui::Color32::from_rgb(39, 201, 63),
                        );
                        ui.add_space(24.0);
                        let cwd_label = self
                            .terminal
                            .as_ref()
                            .map(|sess| sess.cwd.as_str())
                            .unwrap_or("~");
                        ui.label(
                            egui::RichText::new(cwd_label)
                                .size(12.0)
                                .color(FG_SECONDARY)
                                .monospace(),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new("Clear")
                                                .size(11.0)
                                                .color(FG_SECONDARY),
                                        )
                                        .fill(egui::Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::new(1.0, BORDER_NORMAL))
                                        .rounding(egui::Rounding::same(3.0)),
                                    )
                                    .clicked()
                                {
                                    self.term_lines.clear();
                                    self.term_current_spans.clear();
                                    self.term_cur_color = FG_PRIMARY;
                                    if let Some(sess) = &mut self.terminal {
                                        sess.write_raw(&[0x0c]);
                                    }
                                }
                            },
                        );
                    });
                });
        });

        // ── Output area ───────────────────────────────────────────────────
        if output_h > 1.0 {
            let output_rect = egui::Rect::from_min_size(
                total_rect.min + egui::vec2(0.0, TITLE_H),
                egui::vec2(total_rect.width(), output_h),
            );
            // Paint background directly — no Frame wrapper to avoid rect/size mismatch
            ui.painter().rect_filled(
                output_rect,
                0.0,
                egui::Color32::from_rgb(12, 12, 14),
            );
            ui.allocate_ui_at_rect(output_rect, |ui| {
                // Do NOT pass max_height — let ScrollArea fill the rect naturally.
                // Passing max_height > available height triggers layout.rs:663.
                let scroll = egui::ScrollArea::vertical()
                    .id_source("term_output_scroll")
                    .auto_shrink([false, false])
                    .stick_to_bottom(true);

                if self.term_scroll_bottom {
                    self.term_scroll_bottom = false;
                }

                scroll.show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
                    let line_font =
                        egui::FontId::new(13.0, egui::FontFamily::Monospace);

                    let lines = self.term_lines.clone();
                    for spans in &lines {
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            for (color, text) in spans {
                                if text.is_empty() { continue; }
                                ui.label(
                                    egui::RichText::new(text)
                                        .font(line_font.clone())
                                        .color(*color),
                                );
                            }
                        });
                    }

                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        let cwd_label = self
                            .terminal_display_cwd()
                            .unwrap_or_else(|| "~".to_string());
                        ui.label(
                            egui::RichText::new(cwd_label)
                                .font(line_font.clone())
                                .color(FG_SECONDARY),
                        );
                        ui.add_space(8.0);

                        let input_id = egui::Id::new("term_input_field");
                        let input_edit = egui::TextEdit::singleline(&mut self.term_input)
                            .id(input_id)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace)
                            .text_color(FG_PRIMARY)
                            .hint_text(egui::RichText::new("输入命令…").color(FG_DIM))
                            .frame(false);

                        let resp = ui.add(input_edit);
                        if self.term_focus_input {
                            resp.request_focus();
                            self.term_focus_input = false;
                        } else {
                            resp.request_focus();
                        }

                        // ── History ↑ / ↓ ─────────────────────────────────
                        let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                        let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                        if up && !self.term_history.is_empty() {
                            if self.term_history_idx < 0 {
                                self.term_history_draft = self.term_input.clone();
                                self.term_history_idx = self.term_history.len() as i32 - 1;
                            } else if self.term_history_idx > 0 {
                                self.term_history_idx -= 1;
                            }
                            self.term_input = self.term_history[self.term_history_idx as usize].clone();
                        }
                        if down && self.term_history_idx >= 0 {
                            self.term_history_idx += 1;
                            if self.term_history_idx >= self.term_history.len() as i32 {
                                self.term_history_idx = -1;
                                self.term_input = self.term_history_draft.clone();
                            } else {
                                self.term_input = self.term_history[self.term_history_idx as usize].clone();
                            }
                        }

                        // ── Enter ──────────────────────────────────────────
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if enter {
                            let cmd = self.term_input.trim().to_string();
                            if !cmd.is_empty() {
                                if self.term_history.last().map(|s| s.as_str()) != Some(&cmd) {
                                    self.term_history.push(cmd.clone());
                                    if self.term_history.len() > 500 {
                                        self.term_history.remove(0);
                                    }
                                }
                                self.term_history_idx = -1;
                                self.term_history_draft.clear();
                                if let Some(sess) = &mut self.terminal {
                                    sess.send_line(&cmd);
                                }
                                self.term_input.clear();
                                self.term_scroll_bottom = true;
                            }
                            self.term_focus_input = true;
                        }

                        // ── Ctrl+C ─────────────────────────────────────────
                        let ctrl_c = ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::C));
                        if ctrl_c {
                            if let Some(sess) = &mut self.terminal {
                                sess.write_raw(&[0x03]);
                            }
                            self.term_input.clear();
                            self.term_history_idx = -1;
                            self.term_focus_input = true;
                        }

                        // ── Ctrl+L ─────────────────────────────────────────
                        let ctrl_l = ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::L));
                        if ctrl_l {
                            self.term_lines.clear();
                            self.term_current_spans.clear();
                            self.term_cur_color = FG_PRIMARY;
                            if let Some(sess) = &mut self.terminal {
                                sess.write_raw(&[0x0c]);
                            }
                            self.term_focus_input = true;
                        }
                    });
                    ui.add_space(4.0);
                });
            });
        }

    }
}

fn toolbar_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(12.5).color(FG_PRIMARY))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0, BORDER_NORMAL))
            .rounding(egui::Rounding::same(3.0)),
    )
}

// ── eframe::App ───────────────────────────────────────────────────────────────

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_ui_theme(ctx);
        apply_visuals(ctx);

        // ── Top bar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top_bar")
            .frame(
                egui::Frame::none()
                    .fill(BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .stroke(egui::Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("DeskAgent")
                            .size(13.5)
                            .color(FG_ACCENT)
                            .strong(),
                    );
                    ui.add_space(12.0);
                    ui.add(egui::Separator::default().vertical().spacing(8.0));
                    ui.add_space(4.0);

                    if toolbar_button(ui, "Open Project").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.open_project(path);
                        }
                    }

                    ui.add_space(6.0);
                    ui.add(egui::Separator::default().vertical().spacing(8.0));
                    ui.add_space(4.0);

                    // ── View / Shell toggle ───────────────────────────────
                    let view_active = self.main_view == MainView::Code;
                    let shell_active = self.main_view == MainView::Terminal;

                    let view_fill = if view_active {
                        egui::Color32::from_rgb(0, 40, 55)
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let view_stroke = if view_active {
                        egui::Stroke::new(1.0, FG_ACCENT)
                    } else {
                        egui::Stroke::new(1.0, BORDER_NORMAL)
                    };
                    let view_color = if view_active { FG_ACCENT } else { FG_PRIMARY };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("⊞ View").size(12.5).color(view_color),
                            )
                            .fill(view_fill)
                            .stroke(view_stroke)
                            .rounding(egui::Rounding::same(3.0)),
                        )
                        .clicked()
                    {
                        self.main_view = MainView::Code;
                    }

                    let shell_fill = if shell_active {
                        egui::Color32::from_rgb(0, 40, 55)
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let shell_stroke = if shell_active {
                        egui::Stroke::new(1.0, FG_ACCENT)
                    } else {
                        egui::Stroke::new(1.0, BORDER_NORMAL)
                    };
                    let shell_color = if shell_active { FG_ACCENT } else { FG_PRIMARY };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("$ Shell").size(12.5).color(shell_color),
                            )
                            .fill(shell_fill)
                            .stroke(shell_stroke)
                            .rounding(egui::Rounding::same(3.0)),
                        )
                        .clicked()
                    {
                        // Lazily spawn the terminal session on first switch
                        if self.terminal.is_none() {
                            match crate::terminal::TerminalSession::new() {
                                Ok(sess) => {
                                    self.terminal = Some(sess);
                                }
                                Err(e) => {
                                    self.set_status(
                                        format!("Terminal error: {e}"),
                                        StatusKind::Error,
                                    );
                                }
                            }
                        }
                        self.main_view = MainView::Terminal;
                    }

                    ui.add_space(8.0);
                    if let Some(root) = &self.project_root {
                        let name = root
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("project");
                        ui.label(
                            egui::RichText::new(format!("  {name}"))
                                .size(12.0)
                                .color(FG_SECONDARY),
                        );
                    }

                    // ── Clock (right-aligned) ─────────────────────────────
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let now = Local::now();
                            let time_str = now.format("%H:%M:%S").to_string();
                            let date_str = now.format("%Y-%m-%d %Z").to_string();
                            ui.label(
                                egui::RichText::new(&date_str)
                                    .size(11.0)
                                    .color(FG_DIM)
                                    .monospace(),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(&time_str)
                                    .size(13.0)
                                    .color(FG_ACCENT)
                                    .monospace()
                                    .strong(),
                            );
                        },
                    );
                });
            });

        // ── Shortcut hint bar ────────────────────────────────────────────────
        egui::TopBottomPanel::top("shortcut_bar")
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(22, 22, 26))
                    .inner_margin(egui::Margin::symmetric(12.0, 3.0))
                    .stroke(egui::Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("⌘P")
                            .size(10.5)
                            .color(FG_ACCENT)
                            .monospace(),
                    );
                    ui.label(
                        egui::RichText::new(" 文件搜索")
                            .size(10.5)
                            .color(FG_DIM),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new("⌘⇧F")
                            .size(10.5)
                            .color(FG_ACCENT)
                            .monospace(),
                    );
                    ui.label(
                        egui::RichText::new(" 全文搜索")
                            .size(10.5)
                            .color(FG_DIM),
                    );
                });
            });

        // ── Keyboard shortcuts ───────────────────────────────────────────────
        {
            let cmd = ctx.input(|i| i.modifiers.command);
            let shift = ctx.input(|i| i.modifiers.shift);
            let pressed_p = ctx.input(|i| i.key_pressed(egui::Key::P));
            let pressed_f = ctx.input(|i| i.key_pressed(egui::Key::F));

            if cmd && !shift && pressed_p {
                self.file_picker_open = true;
                self.file_picker_query.clear();
            }
            if cmd && shift && pressed_f {
                self.text_search_open = true;
                self.text_search_query.clear();
                self.text_search_results.clear();
                self.text_search_dirty = false;
            }
        }

        // ── File picker overlay (Cmd+P) ──────────────────────────────────────
        if self.file_picker_open {
            self.render_file_picker(ctx);
        }

        // ── Text search overlay (Cmd+Shift+F) ────────────────────────────────
        if self.text_search_open {
            self.render_text_search(ctx);
        }

        // ── Status bar ───────────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                egui::Frame::none()
                    .fill(BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(12.0, 4.0))
                    .stroke(egui::Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (icon, color) = match self.status_kind {
                        StatusKind::Success => ("✓", FG_SUCCESS),
                        StatusKind::Error => ("✗", FG_ERROR),
                        StatusKind::Info => ("›", FG_SECONDARY),
                    };
                    ui.label(egui::RichText::new(icon).size(12.0).color(color));
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(&self.status)
                            .size(12.0)
                            .color(FG_SECONDARY),
                    );

                    if let Some(file) = &self.selected_file {
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                // Language tag
                                let lang = detect_language(file);
                                ui.label(
                                    egui::RichText::new(lang)
                                        .size(11.0)
                                        .color(FG_ACCENT),
                                );
                                ui.add_space(8.0);
                                let name = file
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("");
                                ui.label(
                                    egui::RichText::new(name).size(11.5).color(FG_DIM),
                                );
                            },
                        );
                    }
                });
            });

        // ── Left sidebar ─────────────────────────────────────────────────────
        egui::SidePanel::left("files")
            .default_width(220.0)
            .min_width(160.0)
            .frame(
                egui::Frame::none()
                    .fill(BG_SIDEBAR)
                    .stroke(egui::Stroke::new(1.0, BORDER_SUBTLE)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                self.render_file_tree(ui);
            });

        // ── Central panel: Terminal or Code viewer ───────────────────────────
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG_BASE)
                    .inner_margin(egui::Margin::same(0.0)),
            )
            .show(ctx, |ui| {
                match self.main_view {
                    MainView::Terminal => {
                        // render_terminal uses manual rect layout — no nested Panels
                        self.render_terminal(ui);
                    }
                    MainView::Code => {
                        // File title bar
                        egui::TopBottomPanel::top("editor_title")
                            .frame(
                                egui::Frame::none()
                                    .fill(BG_PANEL)
                                    .inner_margin(egui::Margin::symmetric(12.0, 6.0))
                                    .stroke(egui::Stroke::new(1.0, BORDER_SUBTLE)),
                            )
                            .show_inside(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if let Some(file) = &self.selected_file {
                                        let title = Self::editor_title_label(file);
                                        ui.label(
                                            egui::RichText::new(title)
                                                .size(12.5)
                                                .color(FG_PRIMARY),
                                        );
                                        // Read-only badge
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new("READ ONLY")
                                                        .size(10.0)
                                                        .color(FG_DIM),
                                                );
                                            },
                                        );
                                    } else {
                                        ui.label(
                                            egui::RichText::new("No file open")
                                                .size(12.5)
                                                .color(FG_DIM),
                                        );
                                    }
                                });
                            });

                        // Code viewer body — must NOT nest CentralPanel inside CentralPanel
                        egui::Frame::none()
                            .fill(BG_BASE)
                            .inner_margin(egui::Margin::same(0.0))
                            .show(ui, |ui| {
                                self.render_editor(ui);
                            });
                    }
                }
            });

        // Repaint every second so the clock stays current
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

/// Return a short language label for the status bar.
fn detect_language(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "rs" => "Rust",
        "py" | "pyw" => "Python",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TSX",
        "jsx" => "JSX",
        "go" => "Go",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" => "C++",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "rb" => "Ruby",
        "sh" | "bash" | "zsh" => "Shell",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "json" => "JSON",
        "md" | "markdown" => "Markdown",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "sql" => "SQL",
        "xml" => "XML",
        _ => "Plain Text",
    }
}

// ── Fuzzy matching ────────────────────────────────────────────────────────────

/// Subsequence fuzzy match: every character in `pattern` must appear in
/// `text` in order (case-insensitive comparison expected from caller).
fn fuzzy_match(text: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    let mut pattern_chars = pattern.chars();
    let mut current = match pattern_chars.next() {
        Some(c) => c,
        None => return true,
    };
    for ch in text.chars() {
        if ch == current {
            match pattern_chars.next() {
                Some(next) => current = next,
                None => return true, // all pattern chars matched
            }
        }
    }
    false
}

// ── ANSI 256-color palette ────────────────────────────────────────────────────

/// Convert an xterm 256-color index to an egui Color32.
fn ansi_256_color(n: u32) -> egui::Color32 {
    match n {
        // Standard colors 0-15 (same as 30-37 / 90-97)
        0  => egui::Color32::from_rgb(0,   0,   0),
        1  => egui::Color32::from_rgb(128, 0,   0),
        2  => egui::Color32::from_rgb(0,   128, 0),
        3  => egui::Color32::from_rgb(128, 128, 0),
        4  => egui::Color32::from_rgb(0,   0,   128),
        5  => egui::Color32::from_rgb(128, 0,   128),
        6  => egui::Color32::from_rgb(0,   128, 128),
        7  => egui::Color32::from_rgb(192, 192, 192),
        8  => egui::Color32::from_rgb(128, 128, 128),
        9  => egui::Color32::from_rgb(255, 0,   0),
        10 => egui::Color32::from_rgb(0,   255, 0),
        11 => egui::Color32::from_rgb(255, 255, 0),
        12 => egui::Color32::from_rgb(0,   0,   255),
        13 => egui::Color32::from_rgb(255, 0,   255),
        14 => egui::Color32::from_rgb(0,   255, 255),
        15 => egui::Color32::from_rgb(255, 255, 255),
        // 6×6×6 color cube (16-231)
        16..=231 => {
            let idx = n - 16;
            let b = idx % 6;
            let g = (idx / 6) % 6;
            let r = idx / 36;
            let c = |v: u32| if v == 0 { 0u8 } else { (55 + v * 40) as u8 };
            egui::Color32::from_rgb(c(r), c(g), c(b))
        }
        // Grayscale ramp (232-255)
        232..=255 => {
            let v = (8 + (n - 232) * 10) as u8;
            egui::Color32::from_rgb(v, v, v)
        }
        _ => egui::Color32::from_rgb(220, 220, 220),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::SymbolKind;

    #[test]
    fn terminal_view_renders_without_layout_panic() {
        let ctx = egui::Context::default();
        let mut app = EditorApp::default();
        app.term_lines.push(vec![(FG_PRIMARY, "ready".to_string())]);
        app.term_scroll_bottom = true;

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(960.0, 540.0),
            )),
            ..Default::default()
        };

        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.render_terminal(ui);
            });
        });
    }

    #[test]
    fn editor_title_label_uses_full_file_path() {
        let path = PathBuf::from("/tmp/workspace/src/main.rs");

        assert_eq!(
            EditorApp::editor_title_label(&path),
            "/tmp/workspace/src/main.rs"
        );
        assert_ne!(EditorApp::editor_title_label(&path), "main.rs");
    }

    #[test]
    fn python_navigation_symbols_only_include_current_python_file() {
        let python_file = PathBuf::from("src/example.py");
        let rust_file = PathBuf::from("src/lib.rs");
        let mut app = EditorApp::default();
        app.selected_file = Some(python_file.clone());
        app.project_symbols = vec![
            Symbol {
                path: python_file.clone(),
                name: "Worker.run".to_string(),
                kind: SymbolKind::Method,
                line: 12,
            },
            Symbol {
                path: rust_file,
                name: "main".to_string(),
                kind: SymbolKind::Function,
                line: 1,
            },
        ];

        let symbols = app.python_navigation_symbols();

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Worker.run");
        assert_eq!(symbols[0].line, 12);
    }


    #[test]
    fn editor_preferences_use_file_override_before_global_default() {
        let mut app = EditorApp::default();
        let file = PathBuf::from("/tmp/project/main.py");
        app.selected_file = Some(file.clone());
        app.editor_preferences.global.font_size = 15.0;
        app.editor_preferences.global.line_height = 1.4;
        app.editor_preferences.global.wrap_lines = false;
        app.editor_preferences.files.insert(
            file.display().to_string(),
            EditorFormatSettings {
                font_size: 18.0,
                font_family: EditorFontFamily::Proportional,
                line_height: 1.8,
                wrap_lines: true,
            },
        );

        let settings = app.current_editor_settings();

        assert_eq!(settings.font_size, 18.0);
        assert_eq!(settings.font_family, EditorFontFamily::Proportional);
        assert_eq!(settings.line_height, 1.8);
        assert!(settings.wrap_lines);
    }

    #[test]
    fn jump_to_python_symbol_line_updates_pending_scroll_and_cursor_line() {
        let python_file = PathBuf::from("src/example.py");
        let mut app = EditorApp::default();
        app.selected_file = Some(python_file.clone());
        app.editor_text = "one\ndef run():\n    pass\n".to_string();
        app.project_symbols = vec![Symbol {
            path: python_file,
            name: "run".to_string(),
            kind: SymbolKind::Function,
            line: 2,
        }];

        assert!(app.jump_to_python_symbol_line(2));

        assert_eq!(app.pending_scroll_line, Some(2));
        assert_eq!(app.status, "Jumped to run:2");
    }

    #[test]
    fn char_index_for_line_returns_line_start_character_index() {
        let text = "one\n二三\nfour";

        assert_eq!(EditorApp::char_index_for_line(text, 1), 0);
        assert_eq!(EditorApp::char_index_for_line(text, 2), 4);
        assert_eq!(EditorApp::char_index_for_line(text, 3), 7);
        assert_eq!(EditorApp::char_index_for_line(text, 99), text.chars().count());
    }

    #[test]
    fn line_number_gutter_width_scales_with_digit_count() {
        assert_eq!(EditorApp::line_number_gutter_width(9), 36.0);
        assert_eq!(EditorApp::line_number_gutter_width(100), 52.0);
    }

    #[test]
    fn ingest_raw_bytes_preserves_multibyte_utf8_across_chunks() {
        let mut app = EditorApp::default();
        app.ingest_raw_bytes(vec![0xe4, 0xbd]);
        assert!(app.term_lines.is_empty());
        assert_eq!(app.term_pending_utf8, vec![0xe4, 0xbd]);

        app.ingest_raw_bytes(vec![0xa0, 0xe5, 0xa5, 0xbd, b'\n']);
        assert!(app.term_pending_utf8.is_empty());
        assert_eq!(app.term_lines.len(), 1);
        let rendered: String = app.term_lines[0].iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(rendered, "你好");
    }
}
