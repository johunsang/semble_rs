//! Stdio MCP (Model Context Protocol) server exposing semble's code-search
//! tools to coding agents.
//!
//! Design notes:
//! - Stateless by design, like the CLI: every tool call re-indexes the target
//!   path from scratch. No daemon state, no persisted index, no staleness.
//! - stdout carries protocol messages only (newline-delimited JSON-RPC 2.0);
//!   all logging goes to stderr.
//! - The embedding model is warmed up (downloaded/cached) in a background
//!   thread at startup so the first tool call doesn't hit a host timeout;
//!   tool calls join that thread before indexing.
//! - `digest` and `savings` stay CLI-only: `digest` is a pipe (the log would
//!   already be in the agent's context before an MCP call could compress it)
//!   and `savings` is a human-facing report.

use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::thread::JoinHandle;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::encoder::StaticEncoder;
use crate::index::SembleIndex;
use crate::plan::{build_plan, format_plan};
use crate::render;
use crate::tree::{render as render_tree, TreeOptions};
use crate::utils::{format_results, is_git_url, resolve_chunk};

const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

const JSONRPC_PARSE_ERROR: i64 = -32700;
const JSONRPC_INVALID_REQUEST: i64 = -32600;
const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;
const JSONRPC_INVALID_PARAMS: i64 = -32602;

const SERVER_INSTRUCTIONS: &str = "Token-efficient code search over local paths and git URLs. \
Stateless: every call re-indexes the target path fresh (fast; no staleness). \
Typical flow: tree (map an unfamiliar repo) -> search with mode=outline -> escalate mode only \
when needed (compact -> group -> full) -> deps/impact before editing shared files. \
Use plan when unsure where to start. Relative paths resolve against the server's working directory.";

/// Run the stdio MCP server until stdin closes.
///
/// `model` overrides the embedding model for all tool calls (same semantics
/// as the CLI `--model` flag: explicit value > SEMBLE_MODEL_PATH > default).
pub fn serve(model: Option<&str>) -> Result<()> {
    let mut server = McpServer::new(model.map(String::from));

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.context("Failed to read from stdin")?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle_line(&line) {
            let payload =
                serde_json::to_string(&response).context("Failed to serialize response")?;
            let mut out = stdout.lock();
            out.write_all(payload.as_bytes())?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

struct McpServer {
    model: Option<String>,
    warmup: Option<JoinHandle<()>>,
}

impl McpServer {
    fn new(model: Option<String>) -> Self {
        let warmup_model = model.clone();
        let warmup = std::thread::spawn(move || {
            eprintln!("[semble mcp] warming up embedding model...");
            match StaticEncoder::load(warmup_model.as_deref()) {
                Ok(_) => eprintln!("[semble mcp] embedding model ready"),
                Err(e) => eprintln!(
                    "[semble mcp] model warm-up failed (tool calls will retry): {e:#}"
                ),
            }
        });
        Self {
            model,
            warmup: Some(warmup),
        }
    }

    /// Handle one incoming line. Returns a response value for requests,
    /// `None` for notifications and client responses.
    fn handle_line(&mut self, line: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(error_response(
                    Value::Null,
                    JSONRPC_PARSE_ERROR,
                    &format!("Parse error: {e}"),
                ))
            }
        };
        if !msg.is_object() {
            return Some(error_response(
                Value::Null,
                JSONRPC_INVALID_REQUEST,
                "Invalid request: expected a JSON-RPC object (batching is not supported)",
            ));
        }

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str);
        match (method, id) {
            // Client response to a server request — we never send any; ignore.
            (None, _) => None,
            // Notification: no response allowed.
            (Some(m), None) => {
                self.handle_notification(m);
                None
            }
            (Some(m), Some(id)) => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                Some(self.handle_request(m, &params, id))
            }
        }
    }

    fn handle_notification(&mut self, method: &str) {
        match method {
            "notifications/initialized" | "notifications/cancelled" => {}
            other => eprintln!("[semble mcp] ignoring notification: {other}"),
        }
    }

    fn handle_request(&mut self, method: &str, params: &Value, id: Value) -> Value {
        match method {
            "initialize" => {
                let requested = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(LATEST_PROTOCOL_VERSION);
                let negotiated = if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
                    requested
                } else {
                    LATEST_PROTOCOL_VERSION
                };
                result_response(
                    id,
                    json!({
                        "protocolVersion": negotiated,
                        "capabilities": { "tools": {} },
                        "serverInfo": {
                            "name": "semble",
                            "version": env!("CARGO_PKG_VERSION"),
                        },
                        "instructions": SERVER_INSTRUCTIONS,
                    }),
                )
            }
            "ping" => result_response(id, json!({})),
            "tools/list" => result_response(id, json!({ "tools": tool_definitions() })),
            "tools/call" => self.handle_tool_call(params, id),
            other => error_response(
                id,
                JSONRPC_METHOD_NOT_FOUND,
                &format!("Method not found: {other}"),
            ),
        }
    }

    fn handle_tool_call(&mut self, params: &Value, id: Value) -> Value {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return error_response(id, JSONRPC_INVALID_PARAMS, "Missing tool name");
        };
        if !tool_exists(name) {
            return error_response(
                id,
                JSONRPC_INVALID_PARAMS,
                &format!("Unknown tool: {name}"),
            );
        }
        let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

        // Make sure the startup model download finished before indexing.
        if let Some(handle) = self.warmup.take() {
            let _ = handle.join();
        }

        let model = self.model.clone();
        let outcome = catch_unwind(AssertUnwindSafe(|| run_tool(name, &args, model.as_deref())));
        let tool_result = match outcome {
            Ok(Ok(text)) => json!({
                "content": [{ "type": "text", "text": text }],
                "isError": false,
            }),
            Ok(Err(e)) => json!({
                "content": [{ "type": "text", "text": format!("Error: {e:#}") }],
                "isError": true,
            }),
            Err(_) => json!({
                "content": [{ "type": "text", "text": "Error: internal panic while running the tool" }],
                "isError": true,
            }),
        };
        result_response(id, tool_result)
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

const TOOL_NAMES: &[&str] = &[
    "search",
    "tree",
    "deps",
    "impact",
    "find_pattern",
    "find_related",
    "plan",
];

fn tool_exists(name: &str) -> bool {
    TOOL_NAMES.contains(&name)
}

fn tool_definitions() -> Value {
    let path_prop = json!({
        "type": "string",
        "description": "Directory to index, or a git URL (shallow-cloned). Default: server working directory",
        "default": ".",
    });
    let include_text_files_prop = json!({
        "type": "boolean",
        "description": "Also index non-code text files (.md, .yaml, .json, ...)",
        "default": false,
    });

    json!([
        {
            "name": "search",
            "description": "Hybrid lexical + semantic code search over AST chunks. Describe the feature or behavior in natural language (beats guessing symbol names). Returns a ranked, trimmed set of matches. Escalate mode only when the cheaper one is insufficient: outline -> compact -> group -> full.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural-language feature description, symbol, or keyword",
                    },
                    "path": path_prop,
                    "top_k": {
                        "type": "integer",
                        "description": "Max results",
                        "default": 10,
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["outline", "compact", "group", "full"],
                        "description": "outline: one signature line per match (cheapest). compact: path + matching lines. group: directory-grouped, max 3 match lines per chunk. full: whole chunk bodies.",
                        "default": "outline",
                    },
                    "include_text_files": include_text_files_prop,
                },
                "required": ["query"],
            },
        },
        {
            "name": "tree",
            "description": "Gitignore-aware codebase file tree at a fraction of `ls -R` tokens. Set symbols=true to append top-level symbols (fn, struct, class, ...) per file. Best first call on an unfamiliar repo.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": path_prop,
                    "symbols": {
                        "type": "boolean",
                        "description": "Append top-level symbols per file",
                        "default": false,
                    },
                    "dirs_only": {
                        "type": "boolean",
                        "description": "Show directories only",
                        "default": false,
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Limit tree depth",
                    },
                    "lang": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Filter to languages, e.g. [\"rust\", \"python\"]",
                    },
                    "include_text_files": include_text_files_prop,
                },
                "required": [],
            },
        },
        {
            "name": "deps",
            "description": "Show what a file depends on: defined symbols, direct imports, and direct users. mode=tree renders the transitive import tree (cycle-aware). Use before editing shared files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "File path relative to the indexed root, as shown in search results",
                    },
                    "path": path_prop,
                    "mode": {
                        "type": "string",
                        "enum": ["summary", "tree"],
                        "description": "summary: symbols + direct imports + direct users. tree: transitive import tree.",
                        "default": "summary",
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Max tree depth (mode=tree)",
                    },
                },
                "required": ["file"],
            },
        },
        {
            "name": "impact",
            "description": "Blast radius: all files transitively affected if the given file changes. mode=tree renders the reverse-dependency tree. Use before editing shared files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "File path relative to the indexed root, as shown in search results",
                    },
                    "path": path_prop,
                    "mode": {
                        "type": "string",
                        "enum": ["summary", "tree"],
                        "description": "summary: flat list of affected files. tree: reverse-dependency tree.",
                        "default": "summary",
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Max tree depth (mode=tree)",
                    },
                },
                "required": ["file"],
            },
        },
        {
            "name": "find_pattern",
            "description": "Exhaustive structural AST pattern search via ast-grep (must be installed), e.g. pattern \"fn $NAME($$$)\" with lang \"rust\". Use for every-occurrence structural matches that ranked semantic search cannot express.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "ast-grep pattern, e.g. \"fn $NAME($$$)\"",
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search (git URLs are not supported by this tool). Default: server working directory",
                        "default": ".",
                    },
                    "lang": {
                        "type": "string",
                        "description": "Language hint (rust, python, javascript, ...)",
                    },
                    "compact": {
                        "type": "boolean",
                        "description": "One JSON object per match (ast-grep --json=stream)",
                        "default": false,
                    },
                },
                "required": ["pattern"],
            },
        },
        {
            "name": "find_related",
            "description": "Find code semantically similar to the chunk at a specific file:line (e.g. locate duplicated logic or sibling implementations).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "File path as shown in search results",
                    },
                    "line": {
                        "type": "integer",
                        "description": "Line number (1-indexed)",
                    },
                    "path": path_prop,
                    "top_k": {
                        "type": "integer",
                        "description": "Max results",
                        "default": 10,
                    },
                    "include_text_files": include_text_files_prop,
                },
                "required": ["file", "line"],
            },
        },
        {
            "name": "plan",
            "description": "Recommend a token-efficient exploration flow for a task: ranked candidate files plus a suggested command sequence. Use when unsure where to start; treat 'Confidence: low' candidates as leads, not facts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Natural-language task or feature to investigate",
                    },
                    "path": path_prop,
                    "top_k": {
                        "type": "integer",
                        "description": "Number of candidate chunks to use",
                        "default": 8,
                    },
                    "include_text_files": include_text_files_prop,
                },
                "required": ["task"],
            },
        },
    ])
}

// ---------------------------------------------------------------------------
// Tool execution
// ---------------------------------------------------------------------------

fn run_tool(name: &str, args: &Value, model: Option<&str>) -> Result<String> {
    match name {
        "search" => tool_search(args, model),
        "tree" => tool_tree(args, model),
        "deps" => tool_deps(args, model),
        "impact" => tool_impact(args, model),
        "find_pattern" => tool_find_pattern(args),
        "find_related" => tool_find_related(args, model),
        "plan" => tool_plan(args, model),
        other => bail!("Unknown tool: {other}"),
    }
}

fn tool_search(args: &Value, model: Option<&str>) -> Result<String> {
    let query = req_str(args, "query")?;
    let path = opt_str(args, "path")?.unwrap_or(".");
    let top_k = opt_usize(args, "top_k")?.unwrap_or(10);
    let mode = opt_str(args, "mode")?.unwrap_or("outline");
    let include_text_files = opt_bool(args, "include_text_files")?.unwrap_or(false);

    let index = build_index(path, include_text_files, model)?;
    let results = index.search(query, top_k, None, None, None);
    if results.is_empty() {
        return Ok("No results found.".to_string());
    }
    match mode {
        "outline" => Ok(render::outline(&results)),
        "compact" => Ok(render::compact(&results)),
        "group" => Ok(render::grouped(&results)),
        "full" => Ok(format_results(
            &format!("Search results for: {query:?}"),
            &results,
        )),
        other => bail!("Unknown mode: {other:?} (expected outline, compact, group, or full)"),
    }
}

fn tool_tree(args: &Value, model: Option<&str>) -> Result<String> {
    let path = opt_str(args, "path")?.unwrap_or(".");
    let symbols = opt_bool(args, "symbols")?.unwrap_or(false);
    let dirs_only = opt_bool(args, "dirs_only")?.unwrap_or(false);
    let max_depth = opt_usize(args, "max_depth")?;
    let include_text_files = opt_bool(args, "include_text_files")?.unwrap_or(false);
    let langs = opt_string_vec(args, "lang")?;

    let index = build_index(path, include_text_files, model)?;
    let opts = TreeOptions {
        dirs_only,
        max_depth,
        symbols,
        langs: langs.as_deref(),
    };
    Ok(render_tree(index.chunks(), index.graph(), &opts))
}

fn tool_deps(args: &Value, model: Option<&str>) -> Result<String> {
    let file = req_str(args, "file")?;
    let path = opt_str(args, "path")?.unwrap_or(".");
    let mode = opt_str(args, "mode")?.unwrap_or("summary");
    let max_depth = opt_usize(args, "max_depth")?;

    let index = build_index(path, false, model)?;
    let graph = index.graph();
    match mode {
        "tree" => {
            if graph.deps(file).is_none() {
                bail!("File not found in graph: {file}");
            }
            Ok(render::dep_tree(graph, file, max_depth, false))
        }
        "summary" => render::deps_summary(graph, file)
            .ok_or_else(|| anyhow!("File not found in graph: {file}")),
        other => bail!("Unknown mode: {other:?} (expected summary or tree)"),
    }
}

fn tool_impact(args: &Value, model: Option<&str>) -> Result<String> {
    let file = req_str(args, "file")?;
    let path = opt_str(args, "path")?.unwrap_or(".");
    let mode = opt_str(args, "mode")?.unwrap_or("summary");
    let max_depth = opt_usize(args, "max_depth")?;

    let index = build_index(path, false, model)?;
    let graph = index.graph();
    match mode {
        "tree" => {
            if graph.deps(file).is_none() && graph.dependents(file).is_empty() {
                bail!("File not found in graph: {file}");
            }
            Ok(render::dep_tree(graph, file, max_depth, true))
        }
        "summary" => Ok(render::impact_summary(graph, file)),
        other => bail!("Unknown mode: {other:?} (expected summary or tree)"),
    }
}

fn tool_find_pattern(args: &Value) -> Result<String> {
    let pattern = req_str(args, "pattern")?;
    let path = opt_str(args, "path")?.unwrap_or(".");
    let lang = opt_str(args, "lang")?;
    let compact = opt_bool(args, "compact")?.unwrap_or(false);

    let mut cmd = std::process::Command::new("ast-grep");
    cmd.arg("--pattern").arg(pattern).arg(path);
    if let Some(l) = lang {
        cmd.arg("--lang").arg(l);
    }
    if compact {
        cmd.arg("--json=stream");
    }
    let output = cmd.output().map_err(|_| {
        anyhow!(
            "ast-grep is not installed. find_pattern is a thin wrapper around it. \
             Install with `brew install ast-grep` or `cargo install ast-grep` and retry."
        )
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // ast-grep exits non-zero on zero matches with no output; that is not an error.
    if !output.status.success() && stdout.trim().is_empty() && !stderr.trim().is_empty() {
        bail!("ast-grep failed: {}", stderr.trim());
    }
    if stdout.trim().is_empty() {
        Ok("No matches found.".to_string())
    } else {
        Ok(stdout.into_owned())
    }
}

fn tool_find_related(args: &Value, model: Option<&str>) -> Result<String> {
    let file = req_str(args, "file")?;
    let line = opt_usize(args, "line")?.ok_or_else(|| anyhow!("Missing required argument: line"))?;
    let path = opt_str(args, "path")?.unwrap_or(".");
    let top_k = opt_usize(args, "top_k")?.unwrap_or(10);
    let include_text_files = opt_bool(args, "include_text_files")?.unwrap_or(false);

    let index = build_index(path, include_text_files, model)?;
    let chunk = resolve_chunk(index.chunks(), file, line)
        .cloned()
        .ok_or_else(|| anyhow!("No chunk found at {file}:{line}."))?;

    let results = index.find_related(&chunk, top_k);
    if results.is_empty() {
        Ok(format!("No related chunks found for {file}:{line}."))
    } else {
        Ok(format_results(
            &format!("Chunks related to {file}:{line}"),
            &results,
        ))
    }
}

fn tool_plan(args: &Value, model: Option<&str>) -> Result<String> {
    let task = req_str(args, "task")?;
    let path = opt_str(args, "path")?.unwrap_or(".");
    let top_k = opt_usize(args, "top_k")?.unwrap_or(8);
    let include_text_files = opt_bool(args, "include_text_files")?.unwrap_or(false);

    let index = build_index(path, include_text_files, model)?;
    let results = index.search(task, top_k, None, None, None);
    let report = build_plan(task, path, top_k, &results);
    Ok(format_plan(&report))
}

fn build_index(path: &str, include_text_files: bool, model: Option<&str>) -> Result<SembleIndex> {
    let encoder = match model {
        Some(m) => Some(
            StaticEncoder::load(Some(m))
                .with_context(|| format!("Failed to load model {m:?}"))?,
        ),
        None => None,
    };
    if is_git_url(path) {
        SembleIndex::from_git(path, None, encoder, None, None, include_text_files)
    } else {
        SembleIndex::from_path(path, encoder, None, None, include_text_files)
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC helpers
// ---------------------------------------------------------------------------

fn result_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    opt_str(args, key)?.ok_or_else(|| anyhow!("Missing required argument: {key}"))
}

fn opt_str<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(other) => bail!("Argument {key:?} must be a string, got: {other}"),
    }
}

fn opt_usize(args: &Value, key: &str) -> Result<Option<usize>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(|n| Some(n as usize))
            .ok_or_else(|| anyhow!("Argument {key:?} must be a non-negative integer, got: {v}")),
    }
}

fn opt_bool(args: &Value, key: &str) -> Result<Option<bool>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => bail!("Argument {key:?} must be a boolean, got: {other}"),
    }
}

fn opt_string_vec(args: &Value, key: &str) -> Result<Option<Vec<String>>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item.as_str() {
                    Some(s) => out.push(s.to_string()),
                    None => bail!("Argument {key:?} must be an array of strings"),
                }
            }
            Ok(Some(out))
        }
        // Accept a comma-separated string too, mirroring the CLI flag.
        Some(Value::String(s)) => Ok(Some(
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
        )),
        Some(other) => bail!("Argument {key:?} must be an array of strings, got: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(server: &mut McpServer, line: &str) -> Value {
        server.handle_line(line).expect("expected a response")
    }

    #[test]
    fn initialize_negotiates_known_version() {
        let mut s = McpServer::new(None);
        let resp = request(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        );
        assert_eq!(resp["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(resp["result"]["serverInfo"]["name"], "semble");
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn initialize_falls_back_to_latest_version() {
        let mut s = McpServer::new(None);
        let resp = request(
            &mut s,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        );
        assert_eq!(resp["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    }

    #[test]
    fn tools_list_exposes_all_tools() {
        let mut s = McpServer::new(None);
        let resp = request(&mut s, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let tools = resp["result"]["tools"].as_array().expect("tools array");
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("tool name"))
            .collect();
        assert_eq!(names, *TOOL_NAMES);
        for tool in tools {
            assert!(tool["description"].as_str().is_some_and(|d| !d.is_empty()));
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn notifications_get_no_response() {
        let mut s = McpServer::new(None);
        assert!(s
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());
    }

    #[test]
    fn unknown_method_returns_error() {
        let mut s = McpServer::new(None);
        let resp = request(&mut s, r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#);
        assert_eq!(resp["error"]["code"], JSONRPC_METHOD_NOT_FOUND);
    }

    #[test]
    fn parse_error_returns_null_id() {
        let mut s = McpServer::new(None);
        let resp = request(&mut s, "not json");
        assert_eq!(resp["error"]["code"], JSONRPC_PARSE_ERROR);
        assert!(resp["id"].is_null());
    }

    #[test]
    fn unknown_tool_is_invalid_params() {
        let mut s = McpServer::new(None);
        let resp = request(
            &mut s,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#,
        );
        assert_eq!(resp["error"]["code"], JSONRPC_INVALID_PARAMS);
    }

    #[test]
    fn tool_error_is_result_with_is_error() {
        let mut s = McpServer::new(None);
        // Missing required argument -> tool-level error, not protocol error.
        let resp = request(
            &mut s,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"search","arguments":{}}}"#,
        );
        assert!(resp.get("error").is_none());
        assert_eq!(resp["result"]["isError"], true);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("query"));
    }

    #[test]
    fn argument_helpers_validate_types() {
        let args = json!({"s": "x", "n": 3, "b": true, "arr": ["a", "b"], "csv": "a, b"});
        assert_eq!(req_str(&args, "s").unwrap(), "x");
        assert!(req_str(&args, "missing").is_err());
        assert_eq!(opt_usize(&args, "n").unwrap(), Some(3));
        assert!(opt_usize(&args, "s").is_err());
        assert_eq!(opt_bool(&args, "b").unwrap(), Some(true));
        assert!(opt_bool(&args, "n").is_err());
        assert_eq!(
            opt_string_vec(&args, "arr").unwrap(),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(
            opt_string_vec(&args, "csv").unwrap(),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(opt_string_vec(&args, "missing").unwrap(), None);
    }
}
