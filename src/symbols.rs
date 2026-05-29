use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub path: PathBuf,
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
}

pub fn parse_rust_symbols(path: PathBuf, source: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index].trim_start();

        if let Some(impl_name) = parse_impl_header(line) {
            index += 1;
            while index < lines.len() {
                let body_line = lines[index].trim_start();
                if body_line.starts_with('}') {
                    break;
                }
                if let Some(name) = parse_function_name(body_line) {
                    symbols.push(Symbol {
                        path: path.clone(),
                        name: format!("{impl_name}::{name}"),
                        kind: SymbolKind::Method,
                        line: index + 1,
                    });
                }
                index += 1;
            }
            continue;
        }

        if let Some(trait_name) = parse_trait_header(line) {
            index += 1;
            while index < lines.len() {
                let body_line = lines[index].trim_start();
                if body_line.starts_with('}') {
                    break;
                }
                if let Some(name) = parse_function_name(body_line) {
                    symbols.push(Symbol {
                        path: path.clone(),
                        name: format!("{trait_name}::{name}"),
                        kind: SymbolKind::Method,
                        line: index + 1,
                    });
                }
                index += 1;
            }
            continue;
        }

        if let Some(name) = parse_function_name(line) {
            symbols.push(Symbol {
                path: path.clone(),
                name: name.to_string(),
                kind: SymbolKind::Function,
                line: index + 1,
            });
            index += 1;
            continue;
        }

        index += 1;
    }

    symbols
}

pub fn index_rust_files(paths: &[PathBuf]) -> anyhow::Result<Vec<Symbol>> {
    let mut symbols = Vec::new();
    for path in paths {
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(path)?;
        symbols.extend(parse_rust_symbols(path.clone(), &source));
    }
    Ok(symbols)
}

fn parse_function_name(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let line = line.strip_prefix("pub(crate) ").unwrap_or(line);
    let line = line.strip_prefix("pub(super) ").unwrap_or(line);
    let line = line.strip_prefix("pub ").unwrap_or(line);
    let line = line.strip_prefix("async ").unwrap_or(line);
    let line = line.strip_prefix("const ").unwrap_or(line);
    let line = line.strip_prefix("unsafe ").unwrap_or(line);

    let rest = line.strip_prefix("fn ")?;
    let name = rest
        .split_once('(')
        .map(|(name, _)| name.trim())
        .filter(|name| !name.is_empty())?;
    Some(name)
}

fn parse_impl_header(line: &str) -> Option<String> {
    let line = line.trim_start();
    let line = line.strip_prefix("impl ")?;
    let target = line
        .split_whitespace()
        .take_while(|part| *part != "for")
        .collect::<Vec<_>>()
        .join(" ");
    let name = target
        .split("::")
        .last()
        .unwrap_or(&target)
        .trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '_');
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn parse_trait_header(line: &str) -> Option<String> {
    let line = line.trim_start();
    let line = line.strip_prefix("trait ")?;
    let name = line
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '_');
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}
