//! String renderers shared by the CLI (`main.rs`) and the MCP server (`mcp.rs`).
//!
//! The MCP stdio transport reserves stdout for protocol messages, so every
//! output format must be buildable as a `String` instead of printed directly.

use std::collections::{BTreeMap, HashSet};

use crate::filter::smart_strip;
use crate::graph::DependencyGraph;
use crate::outline::extract_signature_near;
use crate::types::{Chunk, SearchResult};

/// Compact search output: score, location, and matching lines.
pub fn compact(results: &[SearchResult]) -> String {
    let mut out = String::new();
    for r in results {
        out.push_str(&format!(
            "{:.4}\t{}:{}-{}\n",
            r.score, r.chunk.file_path, r.chunk.start_line, r.chunk.end_line
        ));
        for ml in &r.match_lines {
            out.push_str(&format!(
                "  L{}:\t{}\n",
                ml.line,
                truncate_line(&ml.content, 120)
            ));
        }
    }
    out
}

/// Outline search output: one signature line per chunk (smallest footprint).
pub fn outline(results: &[SearchResult]) -> String {
    let mut out = String::new();
    for r in results {
        let match_nums: Vec<usize> = r.match_lines.iter().map(|m| m.line).collect();
        let sig = extract_signature_near(&r.chunk.content, r.chunk.start_line, &match_nums)
            .unwrap_or_else(|| format!("(lines {}-{})", r.chunk.start_line, r.chunk.end_line));
        let match_suffix = if r.match_lines.is_empty() {
            String::new()
        } else {
            format!(" [{}m]", r.match_lines.len())
        };
        out.push_str(&format!(
            "{:.4} {}:{}-{}{}\n  {}\n",
            r.score, r.chunk.file_path, r.chunk.start_line, r.chunk.end_line, match_suffix, sig
        ));
    }
    out
}

/// Directory-grouped search output with match lines capped at 3 per chunk.
pub fn grouped(results: &[SearchResult]) -> String {
    let mut by_dir: BTreeMap<String, (f64, Vec<&SearchResult>)> = BTreeMap::new();
    for r in results {
        let dir = std::path::Path::new(&r.chunk.file_path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();
        let entry = by_dir.entry(dir).or_insert((f64::NEG_INFINITY, Vec::new()));
        if r.score > entry.0 {
            entry.0 = r.score;
        }
        entry.1.push(r);
    }
    let mut dirs: Vec<(&String, &(f64, Vec<&SearchResult>))> = by_dir.iter().collect();
    dirs.sort_by(|a, b| {
        b.1 .0
            .partial_cmp(&a.1 .0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    const MAX_MATCH_LINES: usize = 3;
    let mut out = String::new();
    for (dir, (_, group)) in dirs {
        let has_dir = !dir.is_empty();
        if has_dir {
            out.push_str(&format!("{dir}/\n"));
        }
        for r in group {
            let fname = std::path::Path::new(&r.chunk.file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(r.chunk.file_path.as_str());
            let indent = if has_dir { "  " } else { "" };
            out.push_str(&format!(
                "{indent}{:.4} {fname}:{}-{}\n",
                r.score, r.chunk.start_line, r.chunk.end_line
            ));
            let total = r.match_lines.len();
            for ml in r.match_lines.iter().take(MAX_MATCH_LINES) {
                out.push_str(&format!(
                    "{indent}  L{}: {}\n",
                    ml.line,
                    truncate_line(&ml.content, 100)
                ));
            }
            if total > MAX_MATCH_LINES {
                out.push_str(&format!("{indent}  ... (+{})\n", total - MAX_MATCH_LINES));
            }
        }
    }
    out
}

/// JSON search output (single line, no trailing newline).
pub fn json(results: &[SearchResult]) -> String {
    serde_json::to_string(results).unwrap_or_else(|_| "[]".to_string())
}

/// JSON search output with comments stripped from chunk bodies.
pub fn json_stripped(results: &[SearchResult]) -> String {
    let stripped: Vec<SearchResult> = results
        .iter()
        .map(|r| {
            let lang = r.chunk.language.as_deref();
            SearchResult {
                chunk: Chunk::new(
                    smart_strip(&r.chunk.content, lang),
                    r.chunk.file_path.clone(),
                    r.chunk.start_line,
                    r.chunk.end_line,
                    r.chunk.language.clone(),
                ),
                score: r.score,
                match_lines: r.match_lines.clone(),
            }
        })
        .collect();
    serde_json::to_string(&stripped).unwrap_or_else(|_| "[]".to_string())
}

/// ASCII dependency tree rooted at `root`. `reverse = false` walks imports
/// (deps), `reverse = true` walks dependents (impact). Cycle-aware.
pub fn dep_tree(
    graph: &DependencyGraph,
    root: &str,
    max_depth: Option<usize>,
    reverse: bool,
) -> String {
    let mut out = String::new();
    out.push_str(root);
    out.push('\n');
    let mut visited = HashSet::new();
    visited.insert(root.to_string());
    let children = next_files(graph, root, reverse);
    let mut prefix = String::new();
    walk_dep_tree(
        graph,
        &children,
        &mut visited,
        &mut prefix,
        &mut out,
        1,
        max_depth,
        reverse,
    );
    out
}

/// Human-readable deps report: symbols, direct imports, and direct users.
/// Returns `None` if the file is not in the dependency graph.
pub fn deps_summary(graph: &DependencyGraph, file_path: &str) -> Option<String> {
    let node = graph.deps(file_path)?;
    let mut out = String::new();
    out.push_str(&format!("File: {file_path}\n\n"));
    if !node.symbols.is_empty() {
        out.push_str(&format!("Symbols ({}):\n", node.symbols.len()));
        for sym in &node.symbols {
            out.push_str(&format!("  {} {} (line {})\n", sym.kind, sym.name, sym.line));
        }
        out.push('\n');
    }
    if !node.depends_on.is_empty() {
        out.push_str(&format!("Depends on ({}):\n", node.depends_on.len()));
        for dep in &node.depends_on {
            out.push_str(&format!("  {dep}\n"));
        }
        out.push('\n');
    }
    let dependents = graph.dependents(file_path);
    if !dependents.is_empty() {
        out.push_str(&format!("Used by ({}):\n", dependents.len()));
        for dep in &dependents {
            out.push_str(&format!("  {dep}\n"));
        }
    }
    if node.symbols.is_empty() && node.depends_on.is_empty() && dependents.is_empty() {
        out.push_str("No dependencies or symbols found.\n");
    }
    Some(out)
}

/// Human-readable impact report: all files transitively affected by a change.
pub fn impact_summary(graph: &DependencyGraph, file_path: &str) -> String {
    let affected = graph.impact(file_path);
    if affected.is_empty() {
        format!("No files affected by changes to {file_path}.\n")
    } else {
        let mut out = format!("Impact of {file_path} ({} files affected):\n\n", affected.len());
        for f in &affected {
            out.push_str(&format!("  {f}\n"));
        }
        out
    }
}

fn next_files(graph: &DependencyGraph, file: &str, reverse: bool) -> Vec<String> {
    if reverse {
        graph
            .dependents(file)
            .into_iter()
            .map(String::from)
            .collect()
    } else {
        graph
            .deps(file)
            .map(|n| n.depends_on.clone())
            .unwrap_or_default()
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_dep_tree(
    graph: &DependencyGraph,
    items: &[String],
    visited: &mut HashSet<String>,
    prefix: &mut String,
    out: &mut String,
    depth: usize,
    max_depth: Option<usize>,
    reverse: bool,
) {
    let last_idx = items.len().saturating_sub(1);
    for (i, item) in items.iter().enumerate() {
        let is_last = i == last_idx;
        let connector = if is_last { "└── " } else { "├── " };
        out.push_str(prefix);
        out.push_str(connector);
        out.push_str(item);

        let cycle = visited.contains(item);
        if cycle {
            out.push_str("  (cycle)\n");
            continue;
        }
        let depth_exceeded = max_depth.is_some_and(|m| depth >= m);
        let children = if depth_exceeded {
            vec![]
        } else {
            next_files(graph, item, reverse)
        };
        if depth_exceeded && !next_files(graph, item, reverse).is_empty() {
            out.push_str("  …\n");
            continue;
        }
        out.push('\n');

        if !children.is_empty() {
            visited.insert(item.clone());
            let push = if is_last { "    " } else { "│   " };
            prefix.push_str(push);
            walk_dep_tree(
                graph,
                &children,
                visited,
                prefix,
                out,
                depth + 1,
                max_depth,
                reverse,
            );
            prefix.truncate(prefix.len() - push.len());
            visited.remove(item);
        }
    }
}

fn truncate_line(line: &str, max_len: usize) -> String {
    let trimmed = line.trim();
    if trimmed.len() <= max_len {
        return trimmed.to_string();
    }
    let s: String = trimmed.chars().take(max_len - 3).collect();
    format!("{s}...")
}
