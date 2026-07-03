<!-- Keywords: code search, semantic code search, AI agent, LLM, BM25, embeddings, tree-sitter, AST, dependency graph, impact analysis, Rust, CLI, Claude Code, Codex, Cursor, grep replacement, token reduction, potion-code, model2vec, hybrid search, RRF, build output digest, CI log compression, korean code search, 한글 코드 검색 -->

<h2 align="center"> semble_rs<br/> Fast and Accurate Code Search for Agents — in Rust<br/> <sub>Replaces grep / cat / read / ls and compresses build & CI output. Up to <b>-99%</b> tokens.</sub> </h2>

<div align="center">

<p> <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a> <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.75%2B-orange.svg" alt="Rust"></a> <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue.svg" alt="Platform"> <a href="#benchmarks"><img src="https://img.shields.io/badge/agent%20tokens-up%20to%20--99%25-brightgreen.svg" alt="Token savings"></a> <a href="./README.ko.md"><img src="https://img.shields.io/badge/%ED%95%9C%EA%B5%AD%EC%96%B4-README.ko.md-blue.svg" alt="한국어"></a> </p>

<p> <a href="#quickstart">Quickstart</a> • <a href="#search">Search</a> • <a href="#tree">Tree</a> • <a href="#digest">Digest</a> • <a href="#dependency-graph">Deps / Impact</a> • <a href="#mcp-server">MCP</a> • <a href="#how-it-works">How it works</a> • <a href="#benchmarks">Benchmarks</a> </p>

</div>

`semble_rs` is a Rust port and superset of [MinishLab/semble](https://github.com/MinishLab/semble) built for AI coding agents. It returns the exact code chunks an agent needs, prints a token-cheap codebase tree instead of `ls -R`, and compresses 3 MB CI logs into 35 KB. One single binary, no daemon, no API keys, no GPU. Hybrid BM25 + [Model2Vec](https://github.com/MinishLab/model2vec) static embeddings with code-aware reranking, plus a dependency graph, AST chunking, and a `digest` pipeline for build / test / CI output.

## Quickstart

```bash
# Install Rust if needed, then:
git clone https://github.com/johunsang/semble_rs.git && cd semble_rs
cargo install --path .
```

The binary lands at `~/.cargo/bin/semble_rs`. On first run, the default embedding model `minishlab/potion-code-16M` (\~60 MB) is downloaded from HuggingFace.

```bash
# Map the codebase (replaces ls -R)
semble_rs tree ./my-project --symbols

# Find code by what it does (replaces grep + cat)
semble_rs search "how is auth handled" ./my-project --outline

# Compress build / CI output before reading it
cargo build 2>&1 | semble_rs digest
gh run view <id> --log-failed | semble_rs digest
```

For agent integration (Claude Code, Codex, Cursor), see [Agent integration](#agent-integration).

## Main Features

- **Fast**: indexes the local repo (22 files) in \~150 ms, \~10 s on 1,600 files. Static embedder — no transformer forward pass at query time.
- **Token-efficient**: `tree` collapses `ls -R` by **4×–747×**; `--outline` is **-47%** vs full output; `digest` reaches **-98.9%** on real GitHub Actions logs.
- **Hybrid retrieval**: BM25 + Model2Vec embeddings fused with RRF, then reranked with definition / identifier-stem / file-coherence boosts and noise penalties.
- **Dependency graph**: `deps` / `impact` show what a file imports, defines, and what changes if you touch it. Optional Graphviz `--dot` output.
- **Build / CI compression**: `digest` auto-detects cargo, pnpm/npm/yarn/bun, tsc, pytest, go test, gradle, ruff, mypy, clang/gcc/cmake/make/swiftc, GitHub Actions.
- **MCP server**: `semble_rs serve` exposes all search tools over stdio [MCP](https://modelcontextprotocol.io) for Claude Code, Cursor, and any MCP host.
- **Single binary**: no Python, no daemon, no API keys. Runs on CPU.

## Search

```bash
semble_rs search "auth flow" ./my-project --outline    # pass 1: structural overview
semble_rs search "loginWithEmail" ./my-project --compact   # pass 2: matching lines
semble_rs search "save model" https://github.com/MinishLab/model2vec   # git URL
```

`path` defaults to the current directory; git URLs are accepted (cloned shallow).

### Output modes

| Mode             | Output                                                       | Token cost vs `--compact` | When to use                    |
|------------------|--------------------------------------------------------------|---------------------------|--------------------------------|
| `--outline`      | One signature line per chunk                                 | **-47%**                  | First-pass structural scan     |
| `--group`        | Directory grouping + match lines capped at 3 (`+N` overflow) | \-47%                     | Many match lines per chunk     |
| `--compact`      | Score + path + every matching line                           | baseline                  | Precision scan                 |
| `--json --strip` | Chunk bodies (comments stripped)                             | +800%                     | Tooling / pipeline integration |
| `--json`         | Chunk bodies (raw)                                           | +900%                     | Tooling / pipeline integration |

**Recommended:** `--outline` to overview → `--compact` to narrow → `--json --strip` only if the chunk body itself is needed.

### `find-related`

Given a `file:line` from a previous search result, returns chunks semantically similar to that location.

```bash
semble_rs find-related src/auth.rs 42 ./my-project
```

### `plan`

When the agent doesn't know where to start, `plan` runs a small search and prints a recommended sequence of `--outline` / `--group` / `--compact` / `deps` / `impact` commands.

```bash
semble_rs plan "fix auth flow bug" ./my-project -k 5
```

`plan` is a guardrail, not an oracle: low-confidence candidates are leads, not facts. Skip it when the symbol or feature name is already known.

### `--model`

All search-side commands accept `--model <hf-repo-or-local-path>` to override the default embedder. Also honours the `SEMBLE_MODEL_PATH` environment variable.

## Tree

`semble_rs tree` prints the codebase file tree using the same gitignore-aware index as `search`. It exists because `ls -R` on a real project explodes into tens or hundreds of thousands of tokens (`.git/`, `target/`, `node_modules/` all included). Measured on real repos:

| Project                      | `semble_rs tree` | `ls -R`   | Reduction |
|------------------------------|------------------|-----------|-----------|
| this repo (Rust + `target/`) | **533 B**        | 398,101 B | **747×**  |
| 6,693-file Python backend    | **3,950 B**      | 254,066 B | **64×**   |
| 325-file ML training repo    | 838 B            | 7,522 B   | 9×        |

```bash
semble_rs tree                              # current directory
semble_rs tree -d                           # directories only
semble_rs tree --max-depth 2                # cap depth
semble_rs tree --symbols                    # append top-level symbols per file
semble_rs tree --lang rust,python           # filter by language
```

## Digest

`semble_rs digest` collapses build / test / install / CI output. Errors, file:line:col, tracebacks, panic stacks, and failed-step bodies are always preserved — only progress lines collapse to counts.

```bash
cargo build 2>&1            | semble_rs digest
pnpm install 2>&1           | semble_rs digest
pytest 2>&1                 | semble_rs digest
gh run view <id> --log-failed | semble_rs digest
```

Measured on 15 real-world fixtures:

| Fixture                                                 | Raw → digest        | Savings      |
|---------------------------------------------------------|---------------------|--------------|
| `cargo build` (clean, 218 crates)                       | 7,611 B → 59 B      | **-99.2%**   |
| `cargo test` (45 passing)                               | 3,368 B → 369 B     | \-89.0%      |
| `pnpm install`                                          | 1,323 B → 349 B     | \-73.6%      |
| `tsc` (13 errors, 5 codes)                              | 1,085 B → 648 B     | \-40.3%      |
| `pytest` (4 failures)                                   | 2,762 B → 2,330 B   | \-15.6%      |
| **GitHub Actions log (rust-lang/rust failed CI, real)** | **3.3 MB → 35 KB**  | **-98.9%** ⭐ |
| `go test` (with panic + stack)                          | 1,034 B → 475 B     | \-54.1%      |
| `gradle test` (2 failures)                              | 1,232 B → 522 B     | \-57.6%      |
| `ruff` / `mypy` / `clang` / `cmake` / `swift`           | varies              | \-3% to -30% |
| **TOTAL (15 fixtures)**                                 | **3.33 MB → 43 KB** | **-98.7%**   |

Auto-detection covers cargo, pnpm/npm/yarn/bun, tsc, pytest, go test, gradle, ruff, mypy, clang/gcc/cmake/make/swiftc, GitHub Actions. Force a handler with `--format <name>`; inspect with `--show-format`.

## Dependency graph

```bash
semble_rs deps   src/auth.rs ./my-project                  # what this file imports / defines (flat)
semble_rs deps   src/auth.rs ./my-project --tree           # transitive imports as ASCII tree
semble_rs deps   src/auth.rs ./my-project --tree --max-depth 3
semble_rs deps   src/auth.rs ./my-project --dot | dot -Tpng > deps.png
semble_rs impact src/auth.rs ./my-project                  # who depends on this file (flat list)
semble_rs impact src/auth.rs ./my-project --tree           # reverse-dependency tree
semble_rs impact src/auth.rs ./my-project --dot | dot -Tpng > impact.png
```

`--tree` (v0.9.1+) renders forward (`deps`) or reverse (`impact`) dependencies as an ASCII tree with cycle detection (repeated nodes marked `(cycle)`) and `--max-depth N` truncation (`…`). No external tool required, agent-readable.

`impact` is intended to be run before edits to a shared module to avoid surprises.

### `find-pattern`

Thin wrapper around `ast-grep` for structural queries that semantic search can't express:

```bash
semble_rs find-pattern 'fn $name($$$)' . --lang rust --compact
```

Requires `ast-grep` installed (`brew install ast-grep` or `cargo install ast-grep`).

## Encode

`semble_rs encode` exposes the embedding model as a CLI for scripting and debugging:

```bash
semble_rs encode "search result scoring"            # one vector → JSON array
echo -e "auth\nlogin\ntoken" | semble_rs encode     # stdin, one sentence per line
semble_rs encode "x" --model minishlab/potion-multilingual-128M
```

## Agent integration

Append a snippet like the following to your project-root `CLAUDE.md` or `AGENTS.md`. It works for Claude Code, Codex, Cursor (`.cursorrules`), Aider, and OpenHands.

```markdown
## Code search and exploration

Use `semble_rs` instead of `ls -R`, `grep`, `cat`:

​```bash
semble_rs tree . --symbols                         # codebase map (cheap)
semble_rs search "<feature or symbol>" . --outline # pass 1
semble_rs search "<feature or symbol>" . --compact # pass 2
semble_rs deps   <file> .                          # what file imports / defines
semble_rs impact <file> .                          # files affected by changes
​```

Compress noisy command output before reading it:

​```bash
cargo build 2>&1 | semble_rs digest
pnpm install 2>&1 | semble_rs digest
gh run view <id> --log-failed | semble_rs digest
​```
```

`semble_rs savings` shows estimated tokens saved across past searches.

### MCP server

`semble_rs serve` runs a stdio [MCP](https://modelcontextprotocol.io) server so agents see semble as native, self-describing tools instead of shell commands — no prompt nudging, no shell quoting, and read-only tool calls that hosts can auto-approve. It exposes `search`, `tree`, `deps`, `impact`, `find_pattern`, `find_related`, and `plan` with the same stateless behavior as the CLI: every call re-indexes the target path fresh. `digest` and `savings` stay CLI-only (`digest` is a pipe; by the time an MCP tool could see the log it would already be in context).

Claude Code:

```bash
claude mcp add semble -- semble_rs serve
```

Any MCP host with JSON config (Crush, Cursor, Windsurf, ...):

```json
{
  "mcpServers": {
    "semble": {
      "command": "semble_rs",
      "args": ["serve"]
    }
  }
}
```

Relative `path` arguments resolve against the server's working directory (set by the host, usually the project root); absolute paths and git URLs work too. The embedding model is downloaded and cached at startup if needed. Use `serve --model <repo-or-path>` or the `SEMBLE_MODEL_PATH` env var for a custom or local model.

## How it works

`semble_rs` chunks every file with `tree-sitter` at function / class / module boundaries (line-based fallback for unsupported languages), then scores every query with two complementary retrievers: static [Model2Vec](https://github.com/MinishLab/model2vec) embeddings (default `minishlab/potion-code-16M`) for semantic similarity, and BM25 for lexical matches on identifiers and API names. Score lists are fused with Reciprocal Rank Fusion.

After fusion, results are reranked with code-aware signals:

<details> <summary><b>Ranking signals</b></summary>

- **Adaptive weighting.** Symbol-like queries (`Foo::bar`, `_private`, `getUserById`) get more lexical weight; natural-language queries stay balanced.
- **Definition boosts.** Chunks that define the queried symbol (a `class`, `def`, `func`, etc.) outrank chunks that merely reference it.
- **Identifier stems.** Query tokens are stemmed and matched against identifier stems. Querying `parse config` boosts chunks containing `parseConfig`, `ConfigParser`, or `config_parser`.
- **File coherence.** When multiple chunks of a file match, the file is boosted so the top result reflects file-level relevance.
- **Sibling-chunk boost.** Chunks adjacent to a top hit get a small boost — definitions and their helpers usually cluster.
- **Dependency boost.** Chunks in files imported by a top hit get boosted so call-chain context surfaces.
- **Noise penalties.** Test files, `compat/` / `legacy/` shims, example code, and `.d.ts` declaration stubs are down-ranked so canonical implementations surface first.

</details>

The embedder is fully static (vocab embedding lookup → mean pool → SIF weighting → L2 normalize). All of this runs in milliseconds on CPU.

## Benchmarks

### Retrieval quality — 100-query benchmark (this repo)

100 hand-labelled queries across 5 categories: exact symbol names, natural-language feature descriptions, scenarios, acronyms, and Korean queries. Default model `minishlab/potion-code-16M`.

| Metric         | Score                 |
|----------------|-----------------------|
| Recall@1       | 70%                   |
| Recall@5       | 90%                   |
| Recall@10      | 95%                   |
| MRR            | 0.78                  |
| Median latency | 150 ms / query (cold) |

| Category     | n  | R@1 | R@5  | R@10 | MRR  |
|--------------|----|-----|------|------|------|
| exact_symbol | 30 | 93% | 100% | 100% | 0.96 |
| nl_feature   | 40 | 75% | 98%  | 100% | 0.83 |
| scenario     | 10 | 70% | 100% | 100% | 0.77 |
| acronym      | 10 | 50% | 70%  | 70%  | 0.56 |
| korean       | 10 | 10% | 60%  | 80%  | 0.27 |

Query set: `docs/eval_set_100.json` · per-miss analysis: `docs/benchmark_100.md`.

### Indexing and query latency by repo size

The index is rebuilt every run (no persistent cache).

| Repo size (code files) | Indexing + first query |
|------------------------|------------------------|
| 22 (this repo)         | **\~0.15 s**           |
| 57–120                 | \~0.3–0.7 s            |
| 1,600                  | \~10 s                 |

`digest` is independent of repo size: 3.3 MB CI log → 35 KB in **\~20 ms**.

### Token efficiency vs native shell tools

Measured on real projects:

| Operation                                                | `semble_rs`        | Native          | Reduction    |
|----------------------------------------------------------|--------------------|-----------------|--------------|
| **Codebase map** (this repo)                             | `tree` **533 B**   | `ls -R` 398 KB  | **747×**     |
| **Codebase map** (6,693-file Python backend)             | `tree` **3,950 B** | `ls -R` 254 KB  | **64×**      |
| **Codebase map** (325-file Python repo)                  | `tree` 838 B       | `ls -R` 7,522 B | 9×           |
| **Code chunk lookup** (`--outline` vs `--compact`)       | \-47%              | baseline        | \-47%        |
| **Build log** (`cargo build` clean)                      | `digest` 59 B      | raw 7,611 B     | **-99.2%**   |
| **CI failure log** (real GitHub Actions, rust-lang/rust) | `digest` 35 KB     | raw 3.3 MB      | **-98.9%** ⭐ |
| **15-fixture aggregate**                                 | `digest` 43 KB     | raw 3.33 MB     | **-98.7%**   |

> Agents using `grep + cat + ls -R` spend most of their context window on irrelevant code and noise. `semble_rs` returns only what matters and compresses the rest.

## Supported languages

| Language                  | Search | AST chunking | Dependency graph |
|---------------------------|--------|--------------|------------------|
| Rust                      | ✓      | ✓            | ✓                |
| Python                    | ✓      | ✓            | ✓                |
| JavaScript / TypeScript   | ✓      | ✓            | ✓                |
| Go                        | ✓      | ✓            | ✓                |
| Java                      | ✓      | ✓            | ✓                |
| C / C++                   | ✓      | ✓            | ✓                |
| Kotlin                    | ✓      | ✓            | ✓                |
| Ruby                      | ✓      | ✓            | ✓                |
| PHP                       | ✓      | ✓            | ✓                |
| Swift                     | ✓      | ✓            | ✓                |
| HTML / CSS / Vue / Svelte | ✓      | line-based   | partial          |
| Other                     | ✓      | line-based   | —                |

## License

MIT

## Acknowledgements

- [MinishLab/semble](https://github.com/MinishLab/semble) — original Python implementation by Stéphan Tulkens and Thomas van Dongen. `semble_rs` is a Rust port + superset of their work.
- [Model2Vec](https://github.com/MinishLab/model2vec) and [model2vec-rs](https://github.com/MinishLab/model2vec-rs) — static distillation framework powering the embedder.
- Embedding model: `minishlab/potion-code-16M`.