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

/// Parse Python `def` and `class` definitions from source text.
///
/// Handles:
/// - Top-level `def foo(...):`
/// - Top-level `class Foo:`
/// - Methods inside a class (indented `def`)
/// - Async functions (`async def`)
pub fn parse_python_symbols(path: PathBuf, source: &str) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    // Track the current class name so methods can be prefixed with it.
    // We use indentation level to detect when we leave a class body.
    let mut current_class: Option<(String, usize)> = None; // (name, indent_level)

    for (idx, raw_line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim_start();
        let indent = raw_line.len() - trimmed.len();

        // If we have a current class and the indent is back to class level or less,
        // we've left the class body.
        if let Some((_, class_indent)) = &current_class {
            if !trimmed.is_empty() && indent <= *class_indent {
                current_class = None;
            }
        }

        // Match `class Foo:` or `class Foo(Base):`
        if let Some(rest) = trimmed
            .strip_prefix("class ")
        {
            let name = rest
                .split(|c: char| c == '(' || c == ':' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .trim();
            if !name.is_empty() {
                symbols.push(Symbol {
                    path: path.clone(),
                    name: name.to_string(),
                    kind: SymbolKind::Function, // treat class as top-level symbol
                    line: line_no,
                });
                current_class = Some((name.to_string(), indent));
            }
            continue;
        }

        // Match `def foo(...)` or `async def foo(...)`
        let def_rest = if let Some(r) = trimmed.strip_prefix("async def ") {
            Some(r)
        } else {
            trimmed.strip_prefix("def ")
        };

        if let Some(rest) = def_rest {
            let name = rest
                .split(|c: char| c == '(' || c == ':' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .trim();
            if name.is_empty() {
                continue;
            }

            let full_name = if let Some((class_name, class_indent)) = &current_class {
                if indent > *class_indent {
                    format!("{class_name}.{name}")
                } else {
                    name.to_string()
                }
            } else {
                name.to_string()
            };

            let kind = if current_class.as_ref().map(|(_, ci)| indent > *ci).unwrap_or(false) {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };

            symbols.push(Symbol {
                path: path.clone(),
                name: full_name,
                kind,
                line: line_no,
            });
        }
    }

    symbols
}

pub fn index_python_files(paths: &[PathBuf]) -> anyhow::Result<Vec<Symbol>> {
    let mut symbols = Vec::new();
    for path in paths {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("py") | Some("pyw") => {}
            _ => continue,
        }
        let source = std::fs::read_to_string(path)?;
        symbols.extend(parse_python_symbols(path.clone(), &source));
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
