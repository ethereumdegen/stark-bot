//! Read Symbol tool - extracts specific function/struct/class definitions from files
//!
//! Instead of reading an entire 2000-line file, the agent can request just the
//! definition of a specific symbol. Language-aware parsing for Rust, TypeScript/JS, Python, Go.

use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use crate::tools::ToolSafetyLevel;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Tool for extracting specific symbol definitions from source files
pub struct ReadSymbolTool {
    definition: ToolDefinition,
}

impl ReadSymbolTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();

        properties.insert(
            "path".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Path to the source file (relative to workspace).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "symbol".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Name of the symbol to extract (function, struct, class, enum, impl, etc.).".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        properties.insert(
            "kind".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "Symbol kind hint: 'function', 'struct', 'class', 'enum', 'impl', 'type', 'const', 'interface'. Auto-detected if omitted.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ReadSymbolTool {
            definition: ToolDefinition {
                name: "read_symbol".to_string(),
                description: "Extract a specific symbol definition (function, struct, class, enum, impl) from a source file. More efficient than reading the entire file when you only need one definition. Supports Rust, TypeScript, JavaScript, Python, and Go.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["path".to_string(), "symbol".to_string()],
                },
                group: ToolGroup::Development,
            },
        }
    }

    /// Detect language from file extension
    fn detect_language(path: &Path) -> Language {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Language::Rust,
            Some("ts") | Some("tsx") | Some("mts") => Language::TypeScript,
            Some("js") | Some("jsx") | Some("mjs") => Language::JavaScript,
            Some("py") => Language::Python,
            Some("go") => Language::Go,
            _ => Language::Unknown,
        }
    }

    /// Extract a symbol from source code
    fn extract_symbol(content: &str, symbol: &str, kind: Option<&str>, lang: Language) -> Option<SymbolExtraction> {
        match lang {
            Language::Rust => Self::extract_rust_symbol(content, symbol, kind),
            Language::TypeScript | Language::JavaScript => Self::extract_ts_symbol(content, symbol, kind),
            Language::Python => Self::extract_python_symbol(content, symbol, kind),
            Language::Go => Self::extract_go_symbol(content, symbol, kind),
            Language::Unknown => Self::extract_generic_symbol(content, symbol),
        }
    }

    /// Extract a Rust symbol (fn, struct, enum, impl, type, const, trait)
    fn extract_rust_symbol(content: &str, symbol: &str, kind: Option<&str>) -> Option<SymbolExtraction> {
        let lines: Vec<&str> = content.lines().collect();

        // Build search patterns based on kind hint
        let patterns: Vec<String> = if let Some(k) = kind {
            match k {
                "function" | "fn" => vec![
                    format!("fn {}(", symbol),
                    format!("fn {}<", symbol),
                    format!("async fn {}(", symbol),
                    format!("async fn {}<", symbol),
                    format!("pub fn {}(", symbol),
                    format!("pub async fn {}(", symbol),
                    format!("pub(crate) fn {}(", symbol),
                    format!("pub(crate) async fn {}(", symbol),
                ],
                "struct" => vec![
                    format!("struct {} ", symbol),
                    format!("struct {}<", symbol),
                    format!("pub struct {} ", symbol),
                    format!("pub struct {}<", symbol),
                    format!("struct {} {{", symbol),
                    format!("pub struct {} {{", symbol),
                ],
                "enum" => vec![
                    format!("enum {} ", symbol),
                    format!("pub enum {} ", symbol),
                    format!("enum {} {{", symbol),
                    format!("pub enum {} {{", symbol),
                ],
                "impl" => vec![
                    format!("impl {} ", symbol),
                    format!("impl {} {{", symbol),
                    format!("impl<") , // will check for symbol after <...>
                ],
                "trait" => vec![
                    format!("trait {} ", symbol),
                    format!("pub trait {} ", symbol),
                    format!("trait {} {{", symbol),
                    format!("pub trait {} {{", symbol),
                ],
                _ => Self::rust_all_patterns(symbol),
            }
        } else {
            Self::rust_all_patterns(symbol)
        };

        // Find the start line
        let mut start_line = None;
        let mut doc_start = None;
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            for pattern in &patterns {
                if trimmed.contains(pattern.as_str()) {
                    // For 'impl', specifically check it matches the symbol after generics
                    if kind == Some("impl") && pattern.starts_with("impl<") {
                        if !trimmed.contains(&format!(" {} ", symbol)) && !trimmed.contains(&format!(" {} {{", symbol)) {
                            continue;
                        }
                    }
                    start_line = Some(i);
                    // Walk backwards to find doc comments (/// or //!)
                    doc_start = Some(i);
                    let mut j = i;
                    while j > 0 {
                        j -= 1;
                        let prev = lines[j].trim();
                        if prev.starts_with("///") || prev.starts_with("//!") || prev.starts_with("#[") {
                            doc_start = Some(j);
                        } else if prev.is_empty() && j + 1 < i {
                            // Allow one blank line between doc and definition
                            if j + 1 == doc_start.unwrap_or(i) {
                                continue;
                            }
                            break;
                        } else {
                            break;
                        }
                    }
                    break;
                }
            }
            if start_line.is_some() {
                break;
            }
        }

        let start = doc_start?;
        let def_start = start_line?;

        // Find the end by brace counting
        let end = Self::find_brace_end(&lines, def_start)?;

        Some(SymbolExtraction {
            start_line: start + 1, // 1-indexed
            end_line: end + 1,
            content: lines[start..=end].join("\n"),
            symbol_name: symbol.to_string(),
        })
    }

    fn rust_all_patterns(symbol: &str) -> Vec<String> {
        vec![
            format!("fn {}(", symbol),
            format!("fn {}<", symbol),
            format!("async fn {}(", symbol),
            format!("async fn {}<", symbol),
            format!("pub fn {}(", symbol),
            format!("pub async fn {}(", symbol),
            format!("pub(crate) fn {}(", symbol),
            format!("pub(crate) async fn {}(", symbol),
            format!("struct {} ", symbol),
            format!("struct {}<", symbol),
            format!("struct {} {{", symbol),
            format!("pub struct {} ", symbol),
            format!("pub struct {}<", symbol),
            format!("pub struct {} {{", symbol),
            format!("enum {} ", symbol),
            format!("enum {} {{", symbol),
            format!("pub enum {} ", symbol),
            format!("pub enum {} {{", symbol),
            format!("impl {} ", symbol),
            format!("impl {} {{", symbol),
            format!("trait {} ", symbol),
            format!("trait {} {{", symbol),
            format!("pub trait {} ", symbol),
            format!("pub trait {} {{", symbol),
            format!("type {} ", symbol),
            format!("pub type {} ", symbol),
            format!("const {}", symbol),
            format!("pub const {}", symbol),
            format!("static {}", symbol),
            format!("pub static {}", symbol),
        ]
    }

    /// Extract a TypeScript/JavaScript symbol
    fn extract_ts_symbol(content: &str, symbol: &str, kind: Option<&str>) -> Option<SymbolExtraction> {
        let lines: Vec<&str> = content.lines().collect();

        let patterns: Vec<String> = if let Some(k) = kind {
            match k {
                "function" => vec![
                    format!("function {}(", symbol),
                    format!("function {}<", symbol),
                    format!("async function {}(", symbol),
                    format!("export function {}(", symbol),
                    format!("export async function {}(", symbol),
                    format!("export default function {}(", symbol),
                    format!("const {} = (", symbol),
                    format!("const {} = async (", symbol),
                    format!("export const {} = (", symbol),
                    format!("export const {} = async (", symbol),
                    format!("const {} = <", symbol),
                ],
                "class" => vec![
                    format!("class {} ", symbol),
                    format!("class {} {{", symbol),
                    format!("export class {} ", symbol),
                    format!("export default class {} ", symbol),
                ],
                "interface" => vec![
                    format!("interface {} ", symbol),
                    format!("interface {} {{", symbol),
                    format!("export interface {} ", symbol),
                ],
                "type" => vec![
                    format!("type {} ", symbol),
                    format!("export type {} ", symbol),
                ],
                _ => Self::ts_all_patterns(symbol),
            }
        } else {
            Self::ts_all_patterns(symbol)
        };

        let mut start_line = None;
        let mut doc_start = None;
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            for pattern in &patterns {
                if trimmed.contains(pattern.as_str()) {
                    start_line = Some(i);
                    doc_start = Some(i);
                    // Walk backwards for JSDoc comments
                    let mut j = i;
                    while j > 0 {
                        j -= 1;
                        let prev = lines[j].trim();
                        if prev.starts_with("/**") || prev.starts_with("*") || prev.starts_with("*/")
                            || prev.starts_with("//") || prev.starts_with("@")
                            || prev.starts_with("export") || prev.starts_with("declare")
                        {
                            doc_start = Some(j);
                        } else if prev.is_empty() {
                            break;
                        } else {
                            break;
                        }
                    }
                    break;
                }
            }
            if start_line.is_some() {
                break;
            }
        }

        let start = doc_start?;
        let def_start = start_line?;
        let end = Self::find_brace_end(&lines, def_start)?;

        Some(SymbolExtraction {
            start_line: start + 1,
            end_line: end + 1,
            content: lines[start..=end].join("\n"),
            symbol_name: symbol.to_string(),
        })
    }

    fn ts_all_patterns(symbol: &str) -> Vec<String> {
        vec![
            format!("function {}(", symbol),
            format!("function {}<", symbol),
            format!("async function {}(", symbol),
            format!("export function {}(", symbol),
            format!("export async function {}(", symbol),
            format!("export default function {}(", symbol),
            format!("const {} = (", symbol),
            format!("const {} = async (", symbol),
            format!("export const {} = (", symbol),
            format!("export const {} = async (", symbol),
            format!("let {} = (", symbol),
            format!("class {} ", symbol),
            format!("class {} {{", symbol),
            format!("export class {} ", symbol),
            format!("export default class {} ", symbol),
            format!("interface {} ", symbol),
            format!("interface {} {{", symbol),
            format!("export interface {} ", symbol),
            format!("type {} ", symbol),
            format!("export type {} ", symbol),
            format!("const {} =", symbol),
            format!("export const {} =", symbol),
            format!("enum {} ", symbol),
            format!("export enum {} ", symbol),
        ]
    }

    /// Extract a Python symbol (def, class)
    fn extract_python_symbol(content: &str, symbol: &str, kind: Option<&str>) -> Option<SymbolExtraction> {
        let lines: Vec<&str> = content.lines().collect();

        let patterns: Vec<String> = if let Some(k) = kind {
            match k {
                "function" | "method" => vec![
                    format!("def {}(", symbol),
                    format!("async def {}(", symbol),
                ],
                "class" => vec![
                    format!("class {}(", symbol),
                    format!("class {}:", symbol),
                ],
                _ => vec![
                    format!("def {}(", symbol),
                    format!("async def {}(", symbol),
                    format!("class {}(", symbol),
                    format!("class {}:", symbol),
                ],
            }
        } else {
            vec![
                format!("def {}(", symbol),
                format!("async def {}(", symbol),
                format!("class {}(", symbol),
                format!("class {}:", symbol),
            ]
        };

        let mut start_line = None;
        let mut doc_start = None;
        let mut def_indent = 0usize;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            for pattern in &patterns {
                if trimmed.starts_with(pattern.as_str()) || trimmed.starts_with(&format!("@")) {
                    // For decorators, don't match here but use as doc_start
                    if trimmed.starts_with('@') {
                        continue;
                    }
                    start_line = Some(i);
                    def_indent = line.len() - line.trim_start().len();
                    doc_start = Some(i);
                    // Walk backwards for decorators and comments
                    let mut j = i;
                    while j > 0 {
                        j -= 1;
                        let prev = lines[j].trim();
                        if prev.starts_with('@') || prev.starts_with('#') || prev.starts_with("\"\"\"") {
                            doc_start = Some(j);
                        } else if prev.is_empty() {
                            break;
                        } else {
                            break;
                        }
                    }
                    break;
                }
            }
            if start_line.is_some() {
                break;
            }
        }

        let start = doc_start?;
        let def_start = start_line?;

        // Find the end by indentation (Python uses indentation, not braces)
        let mut end = def_start;
        let mut body_started = false;
        for i in (def_start + 1)..lines.len() {
            let line = lines[i];
            if line.trim().is_empty() {
                continue; // Skip blank lines
            }
            let indent = line.len() - line.trim_start().len();
            if indent > def_indent {
                body_started = true;
                end = i;
            } else if body_started {
                break;
            }
        }

        Some(SymbolExtraction {
            start_line: start + 1,
            end_line: end + 1,
            content: lines[start..=end].join("\n"),
            symbol_name: symbol.to_string(),
        })
    }

    /// Extract a Go symbol
    fn extract_go_symbol(content: &str, symbol: &str, kind: Option<&str>) -> Option<SymbolExtraction> {
        let lines: Vec<&str> = content.lines().collect();

        let patterns: Vec<String> = if let Some(k) = kind {
            match k {
                "function" => vec![
                    format!("func {}(", symbol),
                    format!("func ({}", symbol),  // method receiver
                ],
                "struct" => vec![
                    format!("type {} struct", symbol),
                ],
                "interface" => vec![
                    format!("type {} interface", symbol),
                ],
                _ => Self::go_all_patterns(symbol),
            }
        } else {
            Self::go_all_patterns(symbol)
        };

        let mut start_line = None;
        let mut doc_start = None;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            for pattern in &patterns {
                if trimmed.contains(pattern.as_str()) {
                    start_line = Some(i);
                    doc_start = Some(i);
                    // Walk backwards for Go doc comments
                    let mut j = i;
                    while j > 0 {
                        j -= 1;
                        let prev = lines[j].trim();
                        if prev.starts_with("//") {
                            doc_start = Some(j);
                        } else if prev.is_empty() {
                            break;
                        } else {
                            break;
                        }
                    }
                    break;
                }
            }
            if start_line.is_some() {
                break;
            }
        }

        let start = doc_start?;
        let def_start = start_line?;
        let end = Self::find_brace_end(&lines, def_start)?;

        Some(SymbolExtraction {
            start_line: start + 1,
            end_line: end + 1,
            content: lines[start..=end].join("\n"),
            symbol_name: symbol.to_string(),
        })
    }

    fn go_all_patterns(symbol: &str) -> Vec<String> {
        vec![
            format!("func {}(", symbol),
            format!("type {} struct", symbol),
            format!("type {} interface", symbol),
            format!("type {} ", symbol),
            format!("var {} ", symbol),
            format!("const {} ", symbol),
        ]
    }

    /// Generic symbol extraction for unknown languages
    fn extract_generic_symbol(content: &str, symbol: &str) -> Option<SymbolExtraction> {
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.contains(symbol) && (line.contains('{') || line.contains('(')) {
                let end = Self::find_brace_end(&lines, i).unwrap_or(i);
                return Some(SymbolExtraction {
                    start_line: i + 1,
                    end_line: end + 1,
                    content: lines[i..=end].join("\n"),
                    symbol_name: symbol.to_string(),
                });
            }
        }
        None
    }

    /// Find the closing brace that matches the opening brace on/after start_line
    fn find_brace_end(lines: &[&str], start_line: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut found_open = false;

        for i in start_line..lines.len() {
            for ch in lines[i].chars() {
                if ch == '{' {
                    depth += 1;
                    found_open = true;
                } else if ch == '}' {
                    depth -= 1;
                    if found_open && depth == 0 {
                        return Some(i);
                    }
                }
            }
            // For one-liners without braces (e.g., type aliases, const)
            if i == start_line && !found_open && lines[i].contains(';') {
                return Some(i);
            }
        }

        // If we found braces but didn't close, return last line we checked (truncated)
        if found_open {
            Some((start_line + 200).min(lines.len() - 1))
        } else if start_line < lines.len() {
            // No braces at all — single-line definition
            Some(start_line)
        } else {
            None
        }
    }

    /// List all top-level symbols in a file
    fn list_symbols(content: &str, lang: Language) -> Vec<SymbolSummary> {
        let mut symbols = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let (kind, name) = match lang {
                Language::Rust => Self::parse_rust_symbol_line(trimmed),
                Language::TypeScript | Language::JavaScript => Self::parse_ts_symbol_line(trimmed),
                Language::Python => Self::parse_python_symbol_line(trimmed),
                Language::Go => Self::parse_go_symbol_line(trimmed),
                Language::Unknown => (None, None),
            };

            if let (Some(k), Some(n)) = (kind, name) {
                symbols.push(SymbolSummary {
                    name: n,
                    kind: k,
                    line: i + 1,
                });
            }
        }
        symbols
    }

    fn parse_rust_symbol_line(line: &str) -> (Option<String>, Option<String>) {
        let prefixes = [
            ("pub async fn ", "function"),
            ("pub(crate) async fn ", "function"),
            ("pub fn ", "function"),
            ("pub(crate) fn ", "function"),
            ("async fn ", "function"),
            ("fn ", "function"),
            ("pub struct ", "struct"),
            ("struct ", "struct"),
            ("pub enum ", "enum"),
            ("enum ", "enum"),
            ("pub trait ", "trait"),
            ("trait ", "trait"),
            ("impl ", "impl"),
            ("pub type ", "type"),
            ("type ", "type"),
            ("pub const ", "const"),
            ("const ", "const"),
        ];

        for (prefix, kind) in prefixes {
            if line.starts_with(prefix) {
                let rest = &line[prefix.len()..];
                let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if !name.is_empty() {
                    return (Some(kind.to_string()), Some(name));
                }
            }
        }
        (None, None)
    }

    fn parse_ts_symbol_line(line: &str) -> (Option<String>, Option<String>) {
        let prefixes = [
            ("export default async function ", "function"),
            ("export default function ", "function"),
            ("export async function ", "function"),
            ("export function ", "function"),
            ("async function ", "function"),
            ("function ", "function"),
            ("export default class ", "class"),
            ("export class ", "class"),
            ("class ", "class"),
            ("export interface ", "interface"),
            ("interface ", "interface"),
            ("export type ", "type"),
            ("type ", "type"),
            ("export enum ", "enum"),
            ("enum ", "enum"),
            ("export const ", "const"),
            ("const ", "const"),
        ];

        for (prefix, kind) in prefixes {
            if line.starts_with(prefix) {
                let rest = &line[prefix.len()..];
                let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
                if !name.is_empty() {
                    return (Some(kind.to_string()), Some(name));
                }
            }
        }
        (None, None)
    }

    fn parse_python_symbol_line(line: &str) -> (Option<String>, Option<String>) {
        // Only top-level (no indentation)
        if line.starts_with(' ') || line.starts_with('\t') {
            return (None, None);
        }
        let prefixes = [
            ("async def ", "function"),
            ("def ", "function"),
            ("class ", "class"),
        ];
        for (prefix, kind) in prefixes {
            if line.starts_with(prefix) {
                let rest = &line[prefix.len()..];
                let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if !name.is_empty() {
                    return (Some(kind.to_string()), Some(name));
                }
            }
        }
        (None, None)
    }

    fn parse_go_symbol_line(line: &str) -> (Option<String>, Option<String>) {
        let prefixes = [
            ("func ", "function"),
            ("type ", "type"),
            ("var ", "var"),
            ("const ", "const"),
        ];
        for (prefix, kind) in prefixes {
            if line.starts_with(prefix) {
                let rest = &line[prefix.len()..];
                // Skip receiver for methods: func (r *Receiver) Name(
                let rest = if prefix == "func " && rest.starts_with('(') {
                    if let Some(close) = rest.find(')') {
                        rest[close + 1..].trim_start()
                    } else {
                        rest
                    }
                } else {
                    rest
                };
                let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if !name.is_empty() {
                    return (Some(kind.to_string()), Some(name));
                }
            }
        }
        (None, None)
    }
}

impl Default for ReadSymbolTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Unknown,
}

#[derive(Debug)]
struct SymbolExtraction {
    start_line: usize,
    end_line: usize,
    content: String,
    symbol_name: String,
}

#[derive(Debug)]
struct SymbolSummary {
    name: String,
    kind: String,
    line: usize,
}

#[derive(Debug, Deserialize)]
struct ReadSymbolParams {
    path: String,
    symbol: String,
    kind: Option<String>,
}

#[async_trait]
impl Tool for ReadSymbolTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ReadSymbolParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let requested_path = Path::new(&params.path);
        let full_path = if requested_path.is_absolute() {
            requested_path.to_path_buf()
        } else {
            workspace.join(requested_path)
        };

        if !full_path.exists() || !full_path.is_file() {
            return ToolResult::error(format!("File not found: {}", params.path));
        }

        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read file: {}", e)),
        };

        let lang = Self::detect_language(&full_path);

        // If symbol is "*" or "list", return all symbols
        if params.symbol == "*" || params.symbol == "list" {
            let symbols = Self::list_symbols(&content, lang);
            if symbols.is_empty() {
                return ToolResult::success(format!("No top-level symbols found in {}", params.path));
            }
            let mut output = format!("## Symbols in {}\n\n", params.path);
            for s in &symbols {
                output.push_str(&format!("- **{}** `{}` (line {})\n", s.kind, s.name, s.line));
            }
            return ToolResult::success(output);
        }

        match Self::extract_symbol(&content, &params.symbol, params.kind.as_deref(), lang) {
            Some(extraction) => {
                let output = format!(
                    "## {} in {} (lines {}-{})\n\n```\n{}\n```",
                    extraction.symbol_name,
                    params.path,
                    extraction.start_line,
                    extraction.end_line,
                    extraction.content,
                );

                ToolResult::success(output).with_metadata(json!({
                    "symbol": extraction.symbol_name,
                    "start_line": extraction.start_line,
                    "end_line": extraction.end_line,
                    "lines": extraction.end_line - extraction.start_line + 1,
                }))
            }
            None => {
                // Symbol not found — list available symbols to help
                let symbols = Self::list_symbols(&content, lang);
                let mut msg = format!("Symbol '{}' not found in {}.", params.symbol, params.path);
                if !symbols.is_empty() {
                    msg.push_str("\n\nAvailable symbols:\n");
                    for s in symbols.iter().take(20) {
                        msg.push_str(&format!("- {} `{}` (line {})\n", s.kind, s.name, s.line));
                    }
                    if symbols.len() > 20 {
                        msg.push_str(&format!("... and {} more\n", symbols.len() - 20));
                    }
                }
                ToolResult::error(msg)
            }
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::ReadOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust_function() {
        let code = r#"
use std::io;

/// Add two numbers together
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {
    println!("{}", add(1, 2));
}
"#;
        let result = ReadSymbolTool::extract_rust_symbol(code, "add", None).unwrap();
        assert!(result.content.contains("fn add(a: i32, b: i32)"));
        assert!(result.content.contains("Add two numbers"));
        assert!(result.content.contains("a + b"));
    }

    #[test]
    fn test_extract_rust_struct() {
        let code = r#"
/// A point in 2D space
#[derive(Debug, Clone)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}
"#;
        let result = ReadSymbolTool::extract_rust_symbol(code, "Point", Some("struct")).unwrap();
        assert!(result.content.contains("pub struct Point"));
        assert!(result.content.contains("pub x: f64"));
        assert!(result.content.contains("#[derive(Debug, Clone)]"));
    }

    #[test]
    fn test_extract_python_function() {
        let code = r#"import os

def process_data(data):
    """Process the given data."""
    result = []
    for item in data:
        result.append(item * 2)
    return result

def main():
    data = [1, 2, 3]
    print(process_data(data))
"#;
        let result = ReadSymbolTool::extract_python_symbol(code, "process_data", None).unwrap();
        assert!(result.content.contains("def process_data(data)"));
        assert!(result.content.contains("return result"));
    }

    #[test]
    fn test_list_rust_symbols() {
        let code = r#"pub struct Foo {
    x: i32,
}

pub enum Bar {
    A,
    B,
}

pub fn baz() -> i32 {
    42
}

impl Foo {
    pub fn new() -> Self {
        Self { x: 0 }
    }
}
"#;
        let symbols = ReadSymbolTool::list_symbols(code, Language::Rust);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"));
        assert!(names.contains(&"Bar"));
        assert!(names.contains(&"baz"));
    }

    #[test]
    fn test_find_brace_end() {
        let lines = vec![
            "fn foo() {",
            "    if true {",
            "        bar();",
            "    }",
            "}",
            "",
            "fn next() {",
        ];
        let end = ReadSymbolTool::find_brace_end(&lines, 0).unwrap();
        assert_eq!(end, 4);
    }
}
