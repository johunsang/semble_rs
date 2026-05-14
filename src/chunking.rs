use tree_sitter::{Language, Node, Parser};

use crate::types::Chunk;

const DESIRED_CHUNK_LENGTH_CHARS: usize = 1500;

struct ChunkBoundary {
    start: usize,
    end: usize,
}

fn get_language(name: &str) -> Option<Language> {
    let lang_fn = match name {
        "rust" => tree_sitter_rust::LANGUAGE,
        "python" => tree_sitter_python::LANGUAGE,
        "javascript" => tree_sitter_javascript::LANGUAGE,
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "go" => tree_sitter_go::LANGUAGE,
        "java" => tree_sitter_java::LANGUAGE,
        "c" => tree_sitter_c::LANGUAGE,
        "cpp" => tree_sitter_cpp::LANGUAGE,
        "kotlin" => tree_sitter_kotlin_ng::LANGUAGE,
        _ => return None,
    };
    Some(Language::from(lang_fn))
}

fn is_definition_node(language: &str, node: &Node) -> bool {
    let kind = node.kind();
    match language {
        "rust" => matches!(
            kind,
            "function_item"
                | "impl_item"
                | "struct_item"
                | "enum_item"
                | "trait_item"
                | "mod_item"
                | "const_item"
                | "static_item"
                | "type_item"
                | "macro_definition"
                | "attribute_item"
        ),
        "python" => matches!(
            kind,
            "function_definition" | "class_definition" | "decorated_definition"
        ),
        "javascript" => matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "export_statement"
                | "lexical_declaration"
                | "variable_declaration"
        ),
        "typescript" => matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "interface_declaration"
                | "type_alias_declaration"
                | "enum_declaration"
                | "export_statement"
                | "lexical_declaration"
                | "variable_declaration"
        ),
        "go" => matches!(
            kind,
            "function_declaration" | "method_declaration" | "type_declaration"
        ),
        "java" => matches!(
            kind,
            "class_declaration"
                | "method_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "constructor_declaration"
                | "record_declaration"
        ),
        "c" => matches!(
            kind,
            "function_definition" | "struct_specifier" | "enum_specifier" | "declaration"
        ),
        "cpp" => matches!(
            kind,
            "function_definition"
                | "class_specifier"
                | "struct_specifier"
                | "enum_specifier"
                | "declaration"
                | "namespace_definition"
                | "template_declaration"
        ),
        "kotlin" => matches!(
            kind,
            "class_declaration"
                | "object_declaration"
                | "function_declaration"
                | "property_declaration"
                | "type_alias"
                | "companion_object"
                | "secondary_constructor"
        ),
        _ => false,
    }
}

fn chunk_with_tree_sitter(source: &str, language: &str) -> Option<Vec<ChunkBoundary>> {
    let ts_lang = get_language(language)?;
    let mut parser = Parser::new();
    parser.set_language(&ts_lang).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();

    let mut def_starts: Vec<usize> = Vec::new();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if is_definition_node(language, &child) {
            def_starts.push(child.start_byte());
        }
    }

    if def_starts.is_empty() {
        return None;
    }

    let mut boundaries = Vec::new();

    for (i, &start) in def_starts.iter().enumerate() {
        let end = if i + 1 < def_starts.len() {
            def_starts[i + 1]
        } else {
            source.len()
        };

        // For the first definition, include any leading content (imports, comments)
        let actual_start = if i == 0 { 0 } else { start };

        if actual_start < end {
            boundaries.push(ChunkBoundary {
                start: actual_start,
                end,
            });
        }
    }

    if boundaries.is_empty() {
        return None;
    }

    Some(merge_adjacent_chunks(
        &boundaries,
        DESIRED_CHUNK_LENGTH_CHARS,
    ))
}

fn merge_adjacent_chunks(chunks: &[ChunkBoundary], desired_length: usize) -> Vec<ChunkBoundary> {
    if chunks.is_empty() {
        return Vec::new();
    }

    let mut merged = Vec::new();
    let mut current_start = chunks[0].start;
    let mut current_end = chunks[0].end;
    let mut current_length = current_end - current_start;

    for group in &chunks[1..] {
        let length = group.end - group.start;

        if current_length + length > desired_length {
            merged.push(ChunkBoundary {
                start: current_start,
                end: current_end,
            });
            current_start = group.start;
            current_end = group.end;
            current_length = length;
            continue;
        }

        current_end = group.end;
        current_length += length;
    }

    merged.push(ChunkBoundary {
        start: current_start,
        end: current_end,
    });

    merged
}

fn chunk_lines(text: &str, desired_length: usize) -> Vec<ChunkBoundary> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let mut lines_as_groups = Vec::new();
    let mut index = 0;
    for line in text.split_inclusive('\n') {
        lines_as_groups.push(ChunkBoundary {
            start: index,
            end: index + line.len(),
        });
        index += line.len();
    }
    if index < text.len() {
        lines_as_groups.push(ChunkBoundary {
            start: index,
            end: text.len(),
        });
    }

    merge_adjacent_chunks(&lines_as_groups, desired_length)
}

pub fn chunk_source(source: &str, file_path: &str, language: Option<&str>) -> Vec<Chunk> {
    if source.trim().is_empty() {
        return Vec::new();
    }

    let boundaries = language
        .and_then(|lang| chunk_with_tree_sitter(source, lang))
        .unwrap_or_else(|| chunk_lines(source, DESIRED_CHUNK_LENGTH_CHARS));

    let mut chunks = Vec::new();
    for boundary in &boundaries {
        let end_index = boundary.end.max(boundary.start);
        let text = &source[boundary.start..end_index];

        let start_line = source[..boundary.start].matches('\n').count() + 1;
        let end_line = if end_index > 0 {
            source[..end_index].matches('\n').count() + 1
        } else {
            1
        };

        chunks.push(Chunk::new(
            text.to_string(),
            file_path.to_string(),
            start_line,
            end_line,
            language.map(String::from),
        ));
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_tree_sitter_chunking_small() {
        let source = r#"
use std::collections::HashMap;

fn foo() {
    println!("foo");
}

struct MyStruct {
    field: i32,
}
"#;
        let chunks = chunk_source(source, "test.rs", Some("rust"));
        assert!(!chunks.is_empty());
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("fn foo"));
        assert!(all_content.contains("struct MyStruct"));
        assert!(all_content.contains("use std::collections"));
    }

    #[test]
    fn test_rust_tree_sitter_splits_large() {
        let long_body = "    let x = 1;\n".repeat(100);
        let source = format!(
            "fn foo() {{\n{long_body}}}\n\nfn bar() {{\n{long_body}}}\n\nfn baz() {{\n{long_body}}}\n"
        );
        let chunks = chunk_source(&source, "test.rs", Some("rust"));
        assert!(
            chunks.len() >= 2,
            "large source should split: got {} chunks",
            chunks.len()
        );
    }

    #[test]
    fn test_python_tree_sitter_chunking() {
        let long_body = "    x = 1\n".repeat(100);
        let source =
            format!("import os\n\nclass MyClass:\n{long_body}\ndef standalone():\n{long_body}\n");
        let chunks = chunk_source(&source, "test.py", Some("python"));
        assert!(
            chunks.len() >= 2,
            "large python source should split: got {} chunks",
            chunks.len()
        );
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("class MyClass"));
        assert!(all_content.contains("def standalone"));
    }

    #[test]
    fn test_fallback_for_unknown_language() {
        let source = "line1\nline2\nline3\n";
        let chunks = chunk_source(source, "test.xyz", None);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_javascript_tree_sitter_chunking() {
        let source = r#"
const x = require('something');

function hello() {
    console.log("hello");
}

class Greeter {
    greet() {
        return "hi";
    }
}
"#;
        let chunks = chunk_source(source, "test.js", Some("javascript"));
        assert!(!chunks.is_empty());
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("function hello"));
        assert!(all_content.contains("class Greeter"));
    }

    #[test]
    fn test_go_tree_sitter_chunking() {
        let source = r#"
package main

import "fmt"

func main() {
    fmt.Println("hello")
}

func helper() int {
    return 42
}
"#;
        let chunks = chunk_source(source, "test.go", Some("go"));
        assert!(!chunks.is_empty());
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("func main"));
        assert!(all_content.contains("func helper"));
    }

    #[test]
    fn test_kotlin_tree_sitter_chunking_small() {
        let source = r#"
package com.example

import kotlin.collections.List

class Foo {
    fun bar() = 42
}

object Singleton {
    val x: Int = 1
}

fun topLevel(): String = "hi"

typealias Name = String
"#;
        let chunks = chunk_source(source, "Foo.kt", Some("kotlin"));
        assert!(!chunks.is_empty());
        let all_content: String = chunks.iter().map(|c| c.content.as_str()).collect();
        assert!(all_content.contains("class Foo"));
        assert!(all_content.contains("object Singleton"));
        assert!(all_content.contains("fun topLevel"));
        assert!(all_content.contains("typealias Name"));
    }

    #[test]
    fn test_kotlin_tree_sitter_uses_definition_boundaries() {
        let body = "    val x = 1\n".repeat(80);
        let source =
            format!("class A {{\n{body}}}\n\nclass B {{\n{body}}}\n\nclass C {{\n{body}}}\n");
        let chunks = chunk_source(&source, "Big.kt", Some("kotlin"));
        assert!(
            chunks.len() >= 3,
            "large kotlin source should split by class: got {} chunks",
            chunks.len()
        );
        assert!(chunks[0].content.contains("class A"));
        assert!(
            !chunks[0].content.contains("class B"),
            "first chunk should end at the next top-level definition"
        );
        assert!(chunks[1].content.trim_start().starts_with("class B"));
    }
}
