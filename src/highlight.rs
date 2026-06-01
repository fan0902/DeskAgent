//! Syntax highlighting via syntect.
//!
//! Converts source text into a list of `StyledLine`s, each containing
//! `(color, text_fragment)` pairs that can be rendered with egui.

use eframe::egui;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// A single highlighted fragment: (color, text).
pub type Span = (egui::Color32, String);

/// One line of highlighted spans.
pub type StyledLine = Vec<Span>;

/// Lazily-initialised highlight state (syntax set + theme).
pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    /// Highlight `source` using the best-matching syntax for `file_name`.
    /// Falls back to plain text when no syntax is found.
    ///
    /// Returns one `StyledLine` per source line (including empty lines).
    pub fn highlight(&self, source: &str, file_name: &str) -> Vec<StyledLine> {
        // Pick theme — "base16-ocean.dark" is a clean dark theme bundled with syntect
        let theme = self
            .theme_set
            .themes
            .get("base16-ocean.dark")
            .or_else(|| self.theme_set.themes.values().next())
            .expect("at least one theme is always bundled");

        // Find syntax by extension, fall back to plain text
        let syntax = self
            .syntax_set
            .find_syntax_for_file(file_name)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let mut h = HighlightLines::new(syntax, theme);
        let mut result: Vec<StyledLine> = Vec::new();

        for line in LinesWithEndings::from(source) {
            let ranges = h
                .highlight_line(line, &self.syntax_set)
                .unwrap_or_default();

            let styled: StyledLine = ranges
                .into_iter()
                .map(|(style, text)| (syntect_color_to_egui(style), text.to_string()))
                .collect();

            result.push(styled);
        }

        // Ensure we always have at least one line
        if result.is_empty() {
            result.push(vec![]);
        }

        result
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

fn syntect_color_to_egui(style: Style) -> egui::Color32 {
    let c = style.foreground;
    egui::Color32::from_rgb(c.r, c.g, c.b)
}
