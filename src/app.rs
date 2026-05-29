use crate::ai::{AiClient, AiMode, AiRequest, AiResponse, StubAiClient};
use crate::config::AppConfig;
use crate::diff::make_preview;
use crate::editor::{load_file, save_file};
use crate::project::scan_project;
use crate::symbols::{index_rust_files, Symbol};
use eframe::egui;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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

pub struct EditorApp {
    project_root: Option<PathBuf>,
    files: Vec<PathBuf>,
    expanded_dirs: BTreeSet<PathBuf>,
    selected_file: Option<PathBuf>,
    project_symbols: Vec<Symbol>,
    pending_scroll_line: Option<usize>,
    editor_text: String,
    original_text: String,
    ai_instruction: String,
    ai_preview: String,
    ai_response: Option<AiResponse>,
    status: String,
    dirty: bool,
    config: AppConfig,
    ai_client: Box<dyn AiClient + Send + Sync>,
}

impl Default for EditorApp {
    fn default() -> Self {
        Self {
            project_root: None,
            files: Vec::new(),
            expanded_dirs: Default::default(),
            selected_file: None,
            project_symbols: Vec::new(),
            pending_scroll_line: None,
            editor_text: String::new(),
            original_text: String::new(),
            ai_instruction: String::new(),
            ai_preview: String::new(),
            ai_response: None,
            status: "Ready".to_string(),
            dirty: false,
            config: AppConfig::default(),
            ai_client: Box::new(StubAiClient),
        }
    }
}

fn apply_ui_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(19.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(16.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(16.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);
}

impl EditorApp {
    fn open_project(&mut self, path: PathBuf) {
        match scan_project(&path) {
            Ok(files) => {
                self.project_root = Some(path);
                self.project_symbols = match index_rust_files(&files) {
                    Ok(symbols) => symbols,
                    Err(err) => {
                        self.status = format!("Project loaded, symbol index failed: {err}");
                        Vec::new()
                    }
                };
                self.files = files;
                self.expanded_dirs.clear();
                if let Some(root) = &self.project_root {
                    self.expanded_dirs.insert(root.clone());
                }
                if self.status.starts_with("Project loaded, symbol index failed") {
                    return;
                }
                self.status = format!("Project loaded, {} symbols indexed", self.project_symbols.len());
            }
            Err(err) => {
                self.status = format!("Open failed: {err}");
            }
        }
    }

    fn open_file(&mut self, path: PathBuf) {
        match load_file(&path) {
            Ok(text) => {
                self.original_text = text.clone();
                self.editor_text = text;
                self.selected_file = Some(path);
                self.dirty = false;
                self.status = "File loaded".to_string();
            }
            Err(err) => {
                self.status = format!("Read failed: {err}");
            }
        }
    }

    fn jump_to_symbol(&mut self, symbol: &Symbol) {
        self.open_file(symbol.path.clone());
        self.pending_scroll_line = Some(symbol.line);
        self.status = format!("Jumped to {}:{}", symbol.name, symbol.line);
    }

    fn save_current(&mut self) {
        let Some(path) = self.selected_file.clone() else {
            self.status = "No file selected".to_string();
            return;
        };
        match save_file(&path, &self.editor_text) {
            Ok(()) => {
                self.original_text = self.editor_text.clone();
                self.dirty = false;
                self.status = "Saved".to_string();
            }
            Err(err) => {
                self.status = format!("Save failed: {err}");
            }
        }
    }

    fn request_ai(&mut self, mode: AiMode) {
        let instruction = self.ai_instruction.trim().to_string();
        if instruction.is_empty() {
            self.status = "Instruction required".to_string();
            return;
        }
        let request = AiRequest {
            mode: mode.clone(),
            instruction,
            content: self.editor_text.clone(),
        };
        match self.ai_client.request(&self.config, &request) {
            Ok(response) => {
                self.ai_preview = response
                    .rewritten_content
                    .as_deref()
                    .map(|new| make_preview(&self.editor_text, new))
                    .unwrap_or_else(|| response.text.clone());
                self.ai_response = Some(response);
                self.status = "AI finished".to_string();
            }
            Err(err) => {
                self.status = format!("AI failed: {err}");
            }
        }
    }

    fn apply_ai_change(&mut self) {
        if let Some(response) = &self.ai_response {
            if let Some(new_text) = &response.rewritten_content {
                self.editor_text = new_text.clone();
                self.dirty = self.editor_text != self.original_text;
                self.status = "AI change applied".to_string();
            }
        }
    }

    fn project_label(path: &Path) -> String {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
            .unwrap_or_else(|| path.display().to_string())
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

    fn render_file_tree(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("FOLDERS")
                    .color(egui::Color32::from_gray(130))
                    .strong(),
            );
            ui.add_space(6.0);

            if self.project_root.is_none() {
                ui.label(
                    egui::RichText::new("Open a project to browse files")
                        .color(egui::Color32::from_gray(120)),
                );
                return;
            }

            let nodes = self.build_tree();
            let mut file_to_open = None;

            for node in nodes {
                match node {
                    TreeNode::Dir {
                        path,
                        depth,
                        is_open,
                    } => {
                        let indent = 14.0 * depth as f32;
                        let label = Self::project_label(&path);
                        let arrow = if is_open { "▾" } else { "▸" };
                        let row = ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 22.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add_space(indent);
                                ui.add_sized(
                                    [14.0, 20.0],
                                    egui::Label::new(
                                        egui::RichText::new(arrow)
                                            .color(egui::Color32::from_gray(210)),
                                    ),
                                );
                                ui.add_sized(
                                    [ui.available_width(), 20.0],
                                    egui::Button::new(
                                        egui::RichText::new(label)
                                            .color(egui::Color32::from_gray(210)),
                                    )
                                    .frame(false)
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .sense(egui::Sense::click()),
                                )
                            },
                        );
                        let response = row.inner;
                        if response.clicked() {
                            self.toggle_dir(&path);
                        }
                    }
                    TreeNode::File { path, depth } => {
                        let indent = 14.0 * depth as f32;
                        let label = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(|name| name.to_string())
                            .unwrap_or_else(|| path.display().to_string());
                        let selected = self
                            .selected_file
                            .as_ref()
                            .map(|selected| selected.as_path() == path.as_path())
                            .unwrap_or(false);
                        let fill = if selected {
                            egui::Color32::from_rgb(58, 65, 78)
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let row = ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 22.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add_space(indent);
                                ui.add_sized(
                                    [14.0, 20.0],
                                    egui::Label::new(
                                        egui::RichText::new("▸")
                                            .color(egui::Color32::TRANSPARENT),
                                    ),
                                );
                                ui.add_sized(
                                    [ui.available_width(), 20.0],
                                    egui::Button::new(
                                        egui::RichText::new(label).color(
                                            if selected {
                                                egui::Color32::WHITE
                                            } else {
                                                egui::Color32::from_gray(195)
                                            },
                                        ),
                                    )
                                    .frame(false)
                                    .fill(fill)
                                    .stroke(egui::Stroke::NONE)
                                    .sense(egui::Sense::click()),
                                )
                            },
                        );
                        let response = row.inner;
                        if response.clicked() {
                            file_to_open = Some(path);
                        }
                    }
                }
            }

            if let Some(path) = file_to_open {
                self.open_file(path);
            }
        });
    }

    fn render_symbol_nav(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("SYMBOLS")
                    .color(egui::Color32::from_gray(130))
                    .strong(),
            );

            let current_file = self.selected_file.clone();
            let mut target = None;

            if let Some(current_file) = current_file.as_ref() {
                for symbol in self
                    .project_symbols
                    .iter()
                    .filter(|symbol| symbol.path.as_path() == current_file.as_path())
                {
                    if symbol_button(ui, symbol, true).clicked() {
                        target = Some(symbol.clone());
                    }
                }
            }

            let mut shown_project_label = false;
            for symbol in self.project_symbols.iter().filter(|symbol| {
                current_file
                    .as_ref()
                    .map(|current| current.as_path() != symbol.path.as_path())
                    .unwrap_or(true)
            }) {
                if !shown_project_label {
                    ui.separator();
                    ui.label(
                        egui::RichText::new("PROJECT")
                            .color(egui::Color32::from_gray(130))
                            .strong(),
                    );
                    shown_project_label = true;
                }
                if symbol_button(ui, symbol, false).clicked() {
                    target = Some(symbol.clone());
                }
            }

            if self.project_symbols.is_empty() {
                ui.label(
                    egui::RichText::new("No Rust symbols indexed")
                        .color(egui::Color32::from_gray(120))
                        .strong(),
                );
            }

            if let Some(symbol) = target {
                self.jump_to_symbol(&symbol);
            }
        });
    }

    fn render_editor(&mut self, ui: &mut egui::Ui) {
        let line_count = self.editor_text.lines().count().max(1);
        let line_number_width = (line_count.to_string().len() as f32 * 9.0).max(34.0) + 12.0;
        let text_edit_id = ui.id().with("main_editor");

        egui::ScrollArea::both()
            .id_source("main_editor_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let numbers = (1..=line_count)
                        .map(|line| format!("{line:>width$}", width = line_count.to_string().len()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.add_sized(
                        [line_number_width, ui.available_height()],
                        egui::Label::new(
                            egui::RichText::new(numbers)
                                .monospace()
                                .strong()
                                .color(egui::Color32::from_gray(125)),
                        ),
                    );

                    let available = ui.available_size();
                    let output = egui::TextEdit::multiline(&mut self.editor_text)
                        .id(text_edit_id)
                        .font(egui::TextStyle::Body)
                        .desired_width(available.x.max(600.0))
                        .desired_rows(line_count.max(1))
                        .lock_focus(true)
                        .show(ui);

                    if output.response.changed() {
                        self.dirty = self.editor_text != self.original_text;
                    }

                    if let Some(line) = self.pending_scroll_line.take() {
                        let row_index = line.saturating_sub(1);
                        if let Some(row) = output.galley.rows.get(row_index) {
                            ui.scroll_to_rect(row.rect.translate(output.galley_pos.to_vec2()), Some(egui::Align::Center));
                        }
                    }
                });
            });
    }
}

fn symbol_button(ui: &mut egui::Ui, symbol: &Symbol, current_file: bool) -> egui::Response {
    let label = if current_file {
        format!("{}:{}", symbol.name, symbol.line)
    } else {
        let file = symbol
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file");
        format!("{file} · {}:{}", symbol.name, symbol.line)
    };

    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .strong()
                .color(egui::Color32::from_gray(220)),
        )
        .frame(false)
        .fill(egui::Color32::from_rgb(48, 52, 60))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 76, 88))),
    )
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_ui_theme(ctx);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(37, 37, 38);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(37, 37, 38);
        visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_gray(210);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(62, 62, 64);
        visuals.widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(62, 62, 64);
        visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
        ctx.set_visuals(visuals);

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open Project").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        self.open_project(path);
                    }
                }
                if ui.button("Save").clicked() {
                    self.save_current();
                }
                ui.label(format!(
                    "Project: {}",
                    self.project_root
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "-".to_string())
                ));
            });
        });

        egui::SidePanel::left("files").show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            self.render_file_tree(ui);
        });

        egui::SidePanel::right("ai").show(ctx, |ui| {
            ui.heading("AI");
            ui.label("Instruction");
            ui.text_edit_multiline(&mut self.ai_instruction);
            if ui.button("Explain").clicked() {
                self.request_ai(AiMode::Explain);
            }
            if ui.button("Rewrite").clicked() {
                self.request_ai(AiMode::Rewrite);
            }
            if ui.button("Apply Rewrite").clicked() {
                self.apply_ai_change();
            }
            ui.separator();
            ui.label("Preview");
            ui.text_edit_multiline(&mut self.ai_preview);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(
                self.selected_file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "No file open".to_string()),
            );
            self.render_symbol_nav(ui);
            ui.separator();
            self.render_editor(ui);
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                if self.dirty {
                    ui.label("Modified");
                }
            });
        });
    }
}
