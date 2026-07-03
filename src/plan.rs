use serde::Serialize;

use crate::outline::extract_signature_near;
use crate::types::SearchResult;

#[derive(Debug, Serialize)]
pub struct PlanReport {
    pub task: String,
    pub path: String,
    pub confidence: String,
    pub steps: Vec<PlanStep>,
    pub candidates: Vec<PlanCandidate>,
}

#[derive(Debug, Serialize)]
pub struct PlanStep {
    pub title: String,
    pub command: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct PlanCandidate {
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub score: f64,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

pub fn build_plan(task: &str, path: &str, top_k: usize, results: &[SearchResult]) -> PlanReport {
    let query_terms = query_terms(task);
    let mut ranked_candidates: Vec<(PlanCandidate, f64)> = results
        .iter()
        .map(|r| {
            let match_nums: Vec<usize> = r.match_lines.iter().map(|m| m.line).collect();
            let signature =
                extract_signature_near(&r.chunk.content, r.chunk.start_line, &match_nums)
                    .unwrap_or_else(|| {
                        format!("(lines {}-{})", r.chunk.start_line, r.chunk.end_line)
                    });

            let evidence = best_evidence_line(r, &query_terms);
            let plan_score = adjusted_plan_score(
                r.score,
                &r.chunk.file_path,
                &signature,
                evidence.as_deref(),
                &query_terms,
            );

            (
                PlanCandidate {
                    file_path: r.chunk.file_path.clone(),
                    start_line: r.chunk.start_line,
                    end_line: r.chunk.end_line,
                    score: r.score,
                    signature,
                    evidence,
                },
                plan_score,
            )
        })
        .collect();

    ranked_candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let top_plan_score = ranked_candidates
        .first()
        .map(|(_, score)| *score)
        .unwrap_or(0.0);
    let candidates: Vec<PlanCandidate> = ranked_candidates
        .into_iter()
        .map(|(candidate, _)| candidate)
        .collect();

    let mut steps = vec![
        PlanStep {
            title: "Start broad".to_string(),
            command: format!(
                "semble_rs search {} {} --outline -k {}",
                shell_quote(task),
                shell_quote(path),
                top_k
            ),
            reason: "Find the smallest set of relevant signatures before reading source."
                .to_string(),
        },
        PlanStep {
            title: "Group if noisy".to_string(),
            command: format!(
                "semble_rs search {} {} --group -k {}",
                shell_quote(task),
                shell_quote(path),
                top_k
            ),
            reason: "Collapse many matches into directory-level clusters.".to_string(),
        },
        PlanStep {
            title: "Narrow precisely".to_string(),
            command: format!(
                "semble_rs search {} {} --compact -k {}",
                shell_quote(task),
                shell_quote(path),
                top_k.min(8)
            ),
            reason: "Inspect matching lines only after the candidate area is known.".to_string(),
        },
    ];

    for file_path in unique_code_candidate_files(&candidates).into_iter().take(3) {
        steps.push(PlanStep {
            title: "Check dependencies".to_string(),
            command: format!(
                "semble_rs deps {} {}",
                shell_quote(&file_path),
                shell_quote(path)
            ),
            reason: "Read imports, defined symbols, and direct users before editing.".to_string(),
        });
        steps.push(PlanStep {
            title: "Check impact".to_string(),
            command: format!(
                "semble_rs impact {} {}",
                shell_quote(&file_path),
                shell_quote(path)
            ),
            reason: "Estimate the blast radius for changes in this file.".to_string(),
        });
    }

    PlanReport {
        task: task.to_string(),
        path: path.to_string(),
        confidence: confidence_label(top_plan_score).to_string(),
        steps,
        candidates,
    }
}

pub fn format_plan(report: &PlanReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("Plan for: {}\n", report.task));
    out.push_str(&format!("Path: {}\n", report.path));
    out.push_str(&format!("Confidence: {}\n", report.confidence));
    if report.confidence == "low" {
        out.push_str("Low-confidence matches: treat candidates as leads, not facts.\n");
    }
    out.push('\n');

    out.push_str("Recommended flow:\n");
    for (idx, step) in report.steps.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", idx + 1, step.title));
        out.push_str(&format!("   {}\n", step.command));
        out.push_str(&format!("   {}\n", step.reason));
    }

    out.push('\n');
    if report.candidates.is_empty() {
        out.push_str("No candidate chunks found yet. Try broader natural-language wording or --include-text-files.\n");
    } else {
        out.push_str("Likely candidates:\n");
        for c in &report.candidates {
            out.push_str(&format!(
                "{:.4} {}:{}-{}\n",
                c.score, c.file_path, c.start_line, c.end_line
            ));
            if let Some(evidence) = &c.evidence {
                out.push_str(&format!("  {}\n", evidence));
                if evidence != &c.signature {
                    out.push_str(&format!("  signature: {}\n", c.signature));
                }
            } else {
                out.push_str(&format!("  {}\n", c.signature));
            }
        }
    }
    out
}

pub fn print_plan(report: &PlanReport) {
    print!("{}", format_plan(report));
}

fn unique_code_candidate_files(candidates: &[PlanCandidate]) -> Vec<String> {
    let mut files = Vec::new();
    for candidate in candidates {
        if is_code_path(&candidate.file_path) && !files.contains(&candidate.file_path) {
            files.push(candidate.file_path.clone());
        }
    }
    files
}

fn confidence_label(top_plan_score: f64) -> &'static str {
    if top_plan_score >= 0.10 {
        "high"
    } else if top_plan_score >= 0.05 {
        "medium"
    } else {
        "low"
    }
}

fn is_code_path(path: &str) -> bool {
    matches!(
        path.rsplit('.').next().unwrap_or(""),
        "rs" | "py"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "go"
            | "java"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "kt"
            | "kts"
            | "swift"
            | "rb"
            | "php"
    )
}

fn best_evidence_line(result: &SearchResult, query_terms: &[String]) -> Option<String> {
    result
        .match_lines
        .iter()
        .filter_map(|line| {
            let cleaned = clean_evidence_line(&line.content)?;
            let overlap = overlap_count(&cleaned, query_terms);
            let is_structural = looks_structural(&cleaned);

            if overlap == 0 || (is_structural && overlap < 2) {
                return None;
            }

            Some((cleaned, overlap, is_structural))
        })
        .max_by_key(|(_, overlap, is_structural)| (*overlap, !*is_structural))
        .map(|(cleaned, _, _)| cleaned)
}

fn adjusted_plan_score(
    base_score: f64,
    file_path: &str,
    signature: &str,
    evidence: Option<&str>,
    query_terms: &[String],
) -> f64 {
    let file_overlap = overlap_count(file_path, query_terms).min(3) as f64;
    let signature_overlap = overlap_count(signature, query_terms).min(3) as f64;
    let evidence_overlap = evidence
        .map(|line| overlap_count(line, query_terms).min(4) as f64)
        .unwrap_or(0.0);

    let generic_penalty = if evidence.is_none() && looks_generic_signature(signature) {
        0.01
    } else {
        0.0
    };

    base_score + file_overlap * 0.010 + signature_overlap * 0.008 + evidence_overlap * 0.014
        - generic_penalty
}

fn query_terms(query: &str) -> Vec<String> {
    tokenize(query)
        .into_iter()
        .filter(|term| !is_stop_word(term))
        .collect()
}

fn overlap_count(text: &str, query_terms: &[String]) -> usize {
    let text_terms = tokenize(text);
    query_terms
        .iter()
        .filter(|term| text_terms.contains(term))
        .count()
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter_map(|part| {
            let term = part.trim().to_ascii_lowercase();
            if term.len() >= 3 {
                Some(term)
            } else {
                None
            }
        })
        .collect()
}

fn is_stop_word(term: &str) -> bool {
    matches!(
        term,
        "the"
            | "and"
            | "for"
            | "from"
            | "with"
            | "this"
            | "that"
            | "into"
            | "onto"
            | "when"
            | "then"
            | "than"
            | "what"
            | "where"
            | "which"
            | "while"
    )
}

fn clean_evidence_line(line: &str) -> Option<String> {
    let cleaned = line
        .trim()
        .trim_start_matches("///")
        .trim_start_matches("//!")
        .trim_start_matches("//")
        .trim_start_matches('#')
        .trim_start_matches("/*")
        .trim_start_matches('*')
        .trim_start_matches("\"\"\"")
        .trim_start_matches("'''")
        .trim_end_matches("\"\"\"")
        .trim_end_matches("'''")
        .trim_end_matches("*/")
        .trim()
        .trim_matches('`')
        .trim()
        .to_string();

    if cleaned.len() < 8 {
        None
    } else {
        Some(cleaned)
    }
}

fn looks_structural(line: &str) -> bool {
    let lower = line.trim_start().to_ascii_lowercase();
    lower.starts_with("use ")
        || lower.starts_with("import ")
        || lower.starts_with("from ")
        || lower.starts_with("const ")
        || lower.starts_with("let ")
        || lower.starts_with("var ")
        || lower.starts_with("pub fn ")
        || lower.starts_with("fn ")
        || lower.starts_with("def ")
        || lower.starts_with("function ")
        || lower.starts_with("class ")
}

fn looks_generic_signature(signature: &str) -> bool {
    let lower = signature.to_ascii_lowercase();
    lower.starts_with("fn main")
        || lower.starts_with("def main")
        || lower.starts_with("pub fn main")
        || lower.starts_with("fn new")
        || lower.starts_with("pub fn new")
        || lower.starts_with("def __init__")
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '_' | '-'))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use crate::types::{Chunk, MatchLine};

    use super::*;

    #[test]
    fn plan_starts_broad_then_narrows() {
        let results = vec![SearchResult {
            chunk: Chunk::new(
                "pub fn search(query: &str) {}\n".to_string(),
                "src/search.rs".to_string(),
                10,
                12,
                Some("rust".to_string()),
            ),
            score: 0.42,
            match_lines: vec![MatchLine {
                line: 10,
                content: "pub fn search(query: &str) {}".to_string(),
            }],
        }];

        let report = build_plan("search flow", ".", 10, &results);

        assert_eq!(report.candidates.len(), 1);
        assert!(report.steps[0].command.contains("--outline"));
        assert!(report.steps[1].command.contains("--group"));
        assert!(report.steps[2].command.contains("--compact"));
        assert!(report
            .steps
            .iter()
            .any(|s| s.command == "semble_rs deps src/search.rs ."));
        assert!(report
            .steps
            .iter()
            .any(|s| s.command == "semble_rs impact src/search.rs ."));
    }

    #[test]
    fn docstring_evidence_explains_module_level_match() {
        let results = vec![SearchResult {
            chunk: Chunk::new(
                "\"\"\"Render a video from an EDL.\n\nUsage examples.\n\"\"\"\n\ndef get_preset(name: str) -> str:\n    return name\n".to_string(),
                "helpers/render.py".to_string(),
                1,
                8,
                Some("python".to_string()),
            ),
            score: 0.05,
            match_lines: vec![MatchLine {
                line: 1,
                content: "\"\"\"Render a video from an EDL.".to_string(),
            }],
        }];

        let report = build_plan("render video EDL", ".", 3, &results);

        assert_eq!(
            report.candidates[0].evidence.as_deref(),
            Some("Render a video from an EDL.")
        );
        assert!(report.candidates[0].signature.contains("get_preset"));
    }

    #[test]
    fn evidence_can_lift_more_relevant_candidates() {
        let results = vec![
            SearchResult {
                chunk: Chunk::new(
                    "def main() -> None:\n    pass\n".to_string(),
                    "helpers/other.py".to_string(),
                    1,
                    2,
                    Some("python".to_string()),
                ),
                score: 0.10,
                match_lines: vec![],
            },
            SearchResult {
                chunk: Chunk::new(
                    "\"\"\"Create filmstrip waveform timeline image.\"\"\"\n\ndef render_timeline() -> None:\n    pass\n".to_string(),
                    "helpers/timeline_view.py".to_string(),
                    1,
                    4,
                    Some("python".to_string()),
                ),
                score: 0.08,
                match_lines: vec![MatchLine {
                    line: 1,
                    content: "\"\"\"Create filmstrip waveform timeline image.\"\"\"".to_string(),
                }],
            },
        ];

        let report = build_plan("create filmstrip waveform timeline image", ".", 3, &results);

        assert_eq!(report.candidates[0].file_path, "helpers/timeline_view.py");
    }

    #[test]
    fn shell_quotes_task_with_spaces() {
        assert_eq!(shell_quote("auth failure"), "'auth failure'");
        assert_eq!(shell_quote("src/main.rs"), "src/main.rs");
        assert_eq!(shell_quote("owner's task"), "'owner'\\''s task'");
    }
}
