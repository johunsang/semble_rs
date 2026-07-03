use std::io::Read;
use std::process;

use clap::{Parser, Subcommand};

use semble::digest::{self, Format};
use semble::encoder::StaticEncoder;
use semble::index::SembleIndex;
use semble::plan::{build_plan, print_plan};
use semble::render;
use semble::stats::format_savings_report;
use semble::tree::{render as render_tree, TreeOptions};
use semble::utils::{format_results, is_git_url, resolve_chunk};

#[derive(Parser)]
#[command(
    name = "semble_rs",
    version,
    about = "Fast and Accurate Code Search for Agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search a codebase with keyword/symbol query
    Search {
        /// Keyword, symbol, or function name to search for
        query: String,
        /// Local path or git URL (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Number of results
        #[arg(short = 'k', long = "top-k", default_value = "10")]
        top_k: usize,
        /// Also index non-code text files (.md, .yaml, .json, etc.)
        #[arg(long)]
        include_text_files: bool,
        /// Output as JSON (for agent/tool integration)
        #[arg(long)]
        json: bool,
        /// Compact output: file paths, scores, and match lines only (minimal tokens)
        #[arg(long)]
        compact: bool,
        /// Strip comments from code chunks in JSON output to reduce tokens
        #[arg(long)]
        strip: bool,
        /// Outline output: one signature line per chunk (smallest token footprint)
        #[arg(long)]
        outline: bool,
        /// Group results by directory + cap match lines at 3 per chunk
        #[arg(long)]
        group: bool,
        /// Embedding model (HF repo id or local path).
        /// Overrides SEMBLE_MODEL_PATH; default: minishlab/potion-code-16M.
        #[arg(long)]
        model: Option<String>,
    },
    /// Find code similar to a specific location
    FindRelated {
        /// File path as shown in search results
        file_path: String,
        /// Line number (1-indexed)
        line: usize,
        /// Local path or git URL (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Number of results
        #[arg(short = 'k', long = "top-k", default_value = "10")]
        top_k: usize,
        /// Also index non-code text files
        #[arg(long)]
        include_text_files: bool,
        /// Output as JSON (for agent/tool integration)
        #[arg(long)]
        json: bool,
        /// Embedding model (HF repo id or local path).
        #[arg(long)]
        model: Option<String>,
    },
    /// Show what a file depends on and what symbols it defines
    Deps {
        /// File path (relative to project root)
        file_path: String,
        /// Local path (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Output as Graphviz DOT (pipe into `dot -Tpng > graph.png`)
        #[arg(long)]
        dot: bool,
        /// Output as ASCII dependency tree (transitive imports)
        #[arg(long)]
        tree: bool,
        /// Max tree depth (with --tree)
        #[arg(long)]
        max_depth: Option<usize>,
    },
    /// Show all files affected if a file changes (transitive)
    Impact {
        /// File path (relative to project root)
        file_path: String,
        /// Local path (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Output as Graphviz DOT
        #[arg(long)]
        dot: bool,
        /// Output as ASCII reverse-dependency tree (who depends on this)
        #[arg(long)]
        tree: bool,
        /// Max tree depth (with --tree)
        #[arg(long)]
        max_depth: Option<usize>,
    },
    /// AST pattern match — wraps `ast-grep` for "find every `fn $name($$$)`"
    /// style structural queries that semantic search can't express.
    FindPattern {
        /// ast-grep pattern, e.g. `"fn $name($$$)"`
        pattern: String,
        /// Local path (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Language hint passed to ast-grep (rust, python, javascript, ...)
        #[arg(long)]
        lang: Option<String>,
        /// Compact one-line-per-match output
        #[arg(long)]
        compact: bool,
    },
    /// Recommend a token-efficient exploration flow for a task
    Plan {
        /// Natural-language task or feature to investigate
        task: String,
        /// Local path or git URL (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Number of candidate chunks to use
        #[arg(short = 'k', long = "top-k", default_value = "8")]
        top_k: usize,
        /// Also index non-code text files (.md, .yaml, .json, etc.)
        #[arg(long)]
        include_text_files: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Embedding model (HF repo id or local path).
        #[arg(long)]
        model: Option<String>,
    },
    /// Show token savings and usage stats
    Savings {
        /// Show usage breakdown by call type
        #[arg(long)]
        verbose: bool,
    },
    /// Print the codebase file tree (gitignore-aware, no `ls -R` token explosion)
    Tree {
        /// Local path or git URL (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Show directories only
        #[arg(short = 'd', long)]
        dirs_only: bool,
        /// Limit tree depth
        #[arg(long)]
        max_depth: Option<usize>,
        /// Append top-level symbols (fn, struct, class, enum, ...) per file
        #[arg(long)]
        symbols: bool,
        /// Filter languages (comma-separated, e.g. rust,python)
        #[arg(long, value_delimiter = ',')]
        lang: Option<Vec<String>>,
        /// Also index non-code text files
        #[arg(long)]
        include_text_files: bool,
    },
    /// Encode text to a Model2Vec embedding vector (JSON output)
    Encode {
        /// Text to encode. If omitted, reads sentences from --file or stdin (one per line).
        text: Option<String>,
        /// Read sentences from a file (one per line).
        #[arg(long)]
        file: Option<String>,
        /// Override SEMBLE_MODEL_PATH / default model (HF repo id or local path).
        #[arg(long)]
        model: Option<String>,
    },
    /// Compress build/test/install/CI output (cargo, pnpm, tsc, pytest, GitHub Actions)
    Digest {
        /// Input file. If omitted, reads from stdin.
        file: Option<String>,
        /// Force a specific format (auto-detects if omitted).
        /// Values: cargo, pnpm, tsc, pytest, ci.
        #[arg(long, default_value = "auto")]
        format: String,
        /// Print the detected format on stderr.
        #[arg(long)]
        show_format: bool,
    },
    /// Start a stdio MCP server exposing search, tree, deps, impact,
    /// find-pattern, find-related, and plan as tools for coding agents
    Serve {
        /// Embedding model (HF repo id or local path).
        /// Overrides SEMBLE_MODEL_PATH; default: minishlab/potion-code-16M.
        #[arg(long)]
        model: Option<String>,
    },
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Tree {
            path,
            dirs_only,
            max_depth,
            symbols,
            lang,
            include_text_files,
        } => {
            let index = build_index(&path, include_text_files, None);
            let opts = TreeOptions {
                dirs_only,
                max_depth,
                symbols,
                langs: lang.as_deref(),
            };
            let out = render_tree(index.chunks(), index.graph(), &opts);
            print!("{out}");
        }
        Commands::Encode { text, file, model } => {
            let encoder = StaticEncoder::load(model.as_deref()).unwrap_or_else(|e| {
                eprintln!("Failed to load model: {e}");
                process::exit(1);
            });
            let inputs: Vec<String> = if let Some(t) = text {
                vec![t]
            } else {
                let buf = if let Some(f) = file {
                    std::fs::read_to_string(&f).unwrap_or_else(|e| {
                        eprintln!("Error reading {f}: {e}");
                        process::exit(1);
                    })
                } else {
                    let mut s = String::new();
                    if let Err(e) = std::io::stdin().read_to_string(&mut s) {
                        eprintln!("Error reading stdin: {e}");
                        process::exit(1);
                    }
                    s
                };
                let lines: Vec<String> = buf
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|s| s.to_string())
                    .collect();
                if lines.is_empty() {
                    eprintln!("No input text.");
                    process::exit(1);
                }
                lines
            };
            let arr = encoder.encode_batch(&inputs).unwrap_or_else(|e| {
                eprintln!("Encoding failed: {e}");
                process::exit(1);
            });
            let rows: Vec<Vec<f32>> = arr.outer_iter().map(|r| r.to_vec()).collect();
            let json = if rows.len() == 1 {
                serde_json::to_string(&rows[0])
            } else {
                serde_json::to_string(&rows)
            }
            .unwrap_or_else(|e| {
                eprintln!("Serialization failed: {e}");
                process::exit(1);
            });
            println!("{json}");
        }
        Commands::Digest {
            file,
            format,
            show_format,
        } => {
            let text = match file {
                Some(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| {
                    eprintln!("Error reading {path}: {e}");
                    process::exit(1);
                }),
                None => {
                    let mut buf = String::new();
                    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                        eprintln!("Error reading stdin: {e}");
                        process::exit(1);
                    }
                    buf
                }
            };
            let fmt = if format == "auto" {
                digest::detect(&text)
            } else {
                Format::parse(&format).unwrap_or_else(|| {
                    eprintln!("Unknown --format value: {format}. Valid: cargo, pnpm, tsc, pytest, ci, auto.");
                    process::exit(1);
                })
            };
            if show_format {
                eprintln!("[digest] format={}", fmt.as_str());
            }
            let out = digest::digest(&text, fmt);
            println!("{out}");
        }
        Commands::FindPattern {
            pattern,
            path,
            lang,
            compact,
        } => {
            // Thin wrapper around `ast-grep` for structural pattern matching.
            // Falls back to a clear hint if ast-grep isn't installed.
            let mut cmd = std::process::Command::new("ast-grep");
            cmd.arg("--pattern").arg(&pattern).arg(&path);
            if let Some(l) = lang.as_deref() {
                cmd.arg("--lang").arg(l);
            }
            if compact {
                cmd.arg("--json=stream");
            }
            match cmd.spawn() {
                Ok(mut child) => {
                    let _ = child.wait();
                }
                Err(_) => {
                    eprintln!(
                        "ast-grep is not installed. semble_rs find-pattern is a thin wrapper around it.\n\
                         Install with `brew install ast-grep` or `cargo install ast-grep` and re-run."
                    );
                    process::exit(1);
                }
            }
        }
        Commands::Savings { verbose } => {
            print!("{}", format_savings_report(verbose));
        }
        Commands::Serve { model } => {
            if let Err(e) = semble::mcp::serve(model.as_deref()) {
                eprintln!("MCP server error: {e:?}");
                process::exit(1);
            }
        }
        Commands::Deps {
            file_path,
            path,
            json,
            dot,
            tree,
            max_depth,
        } => {
            let index = build_index(&path, false, None);
            let graph = index.graph();

            if dot {
                println!("{}", graph.deps_dot(&file_path));
                return;
            }
            if tree {
                if graph.deps(&file_path).is_none() {
                    eprintln!("File not found in graph: {file_path}");
                    process::exit(1);
                }
                print!("{}", render::dep_tree(graph, &file_path, max_depth, false));
                return;
            }
            if json {
                match graph.deps(&file_path) {
                    Some(node) => {
                        println!(
                            "{}",
                            serde_json::to_string(node).unwrap_or_else(|_| "{}".to_string())
                        );
                    }
                    None => {
                        println!("{{}}");
                    }
                }
            } else {
                match render::deps_summary(graph, &file_path) {
                    Some(out) => print!("{out}"),
                    None => {
                        eprintln!("File not found in graph: {file_path}");
                        process::exit(1);
                    }
                }
            }
        }
        Commands::Impact {
            file_path,
            path,
            json,
            dot,
            tree,
            max_depth,
        } => {
            let index = build_index(&path, false, None);
            let graph = index.graph();

            if dot {
                println!("{}", graph.impact_dot(&file_path));
                return;
            }
            if tree {
                if graph.deps(&file_path).is_none() && graph.dependents(&file_path).is_empty() {
                    eprintln!("File not found in graph: {file_path}");
                    process::exit(1);
                }
                print!("{}", render::dep_tree(graph, &file_path, max_depth, true));
                return;
            }

            if json {
                let affected = graph.impact(&file_path);
                println!(
                    "{}",
                    serde_json::to_string(&affected).unwrap_or_else(|_| "[]".to_string())
                );
            } else {
                print!("{}", render::impact_summary(graph, &file_path));
            }
        }
        Commands::Plan {
            task,
            path,
            top_k,
            include_text_files,
            json,
            model,
        } => {
            let index = build_index(&path, include_text_files, model.as_deref());
            let results = index.search(task.as_str(), top_k, None, None, None);
            let report = build_plan(&task, &path, top_k, &results);

            if json {
                println!(
                    "{}",
                    serde_json::to_string(&report).unwrap_or_else(|_| "{}".to_string())
                );
            } else {
                print_plan(&report);
            }
        }
        Commands::Search {
            query,
            path,
            top_k,
            include_text_files,
            json,
            compact,
            strip,
            outline,
            group,
            model,
        } => {
            let index = build_index(&path, include_text_files, model.as_deref());

            let results = index.search(query.as_str(), top_k, None, None, None);
            if outline {
                print!("{}", render::outline(&results));
            } else if group {
                print!("{}", render::grouped(&results));
            } else if compact {
                print!("{}", render::compact(&results));
            } else if json && strip {
                println!("{}", render::json_stripped(&results));
            } else if json {
                println!("{}", render::json(&results));
            } else if results.is_empty() {
                println!("No results found.");
            } else {
                println!(
                    "{}",
                    format_results(&format!("Search results for: {query:?}"), &results)
                );
            }
        }
        Commands::FindRelated {
            file_path,
            line,
            path,
            top_k,
            include_text_files,
            json,
            model,
        } => {
            let index = build_index(&path, include_text_files, model.as_deref());

            let chunk = match resolve_chunk(index.chunks(), &file_path, line) {
                Some(c) => c.clone(),
                None => {
                    eprintln!("No chunk found at {file_path}:{line}.");
                    process::exit(1);
                }
            };

            let results = index.find_related(&chunk, top_k);
            if json {
                println!("{}", render::json(&results));
            } else if results.is_empty() {
                println!("No related chunks found for {file_path}:{line}.");
            } else {
                println!(
                    "{}",
                    format_results(&format!("Chunks related to {file_path}:{line}"), &results)
                );
            }
        }
    }
}

fn build_index(path: &str, include_text_files: bool, model: Option<&str>) -> SembleIndex {
    let encoder = model.map(|m| {
        StaticEncoder::load(Some(m)).unwrap_or_else(|e| {
            eprintln!("Failed to load model {m:?}: {e}");
            process::exit(1);
        })
    });
    let result = if is_git_url(path) {
        SembleIndex::from_git(path, None, encoder, None, None, include_text_files)
    } else {
        SembleIndex::from_path(path, encoder, None, None, include_text_files)
    };

    match result {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("Error: {e:?}");
            process::exit(1);
        }
    }
}
