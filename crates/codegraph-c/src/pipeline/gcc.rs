// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! GCC extension neutralization
//!
//! This module provides transformation of GCC-specific extensions into
//! standard C that tree-sitter can parse. It tracks all transformations
//! for position mapping back to the original source.

use regex::Regex;
use std::sync::LazyLock;

/// Type of transformation applied
#[derive(Debug, Clone, PartialEq)]
pub enum TransformKind {
    /// __attribute__((...)) removal
    Attribute,
    /// __extension__ removal
    Extension,
    /// typeof → int replacement
    Typeof,
    /// Statement expression ({ ... }) → (0)
    StatementExpression,
    /// __asm__ removal
    Asm,
    /// __restrict removal
    Restrict,
    /// __inline__ removal
    Inline,
    /// __volatile removal
    Volatile,
    /// __typeof__ → int replacement
    TypeofUnderscore,
    /// alignof/sizeof handling
    AlignofSizeof,
}

/// Record of a transformation
#[derive(Debug, Clone)]
pub struct Transformation {
    /// Byte offset in original source where transformation started
    pub original_start: usize,
    /// Byte length in original source that was transformed
    pub original_length: usize,
    /// Byte offset in transformed source
    pub transformed_start: usize,
    /// Byte length in transformed source
    pub transformed_length: usize,
    /// Kind of transformation
    pub kind: TransformKind,
    /// Original text that was transformed
    pub original_text: String,
}

/// Result of neutralization
#[derive(Debug)]
pub struct NeutralizedSource {
    /// Transformed code
    pub code: String,
    /// List of transformations applied
    pub transformations: Vec<Transformation>,
}

/// GCC extension neutralizer
pub struct GccNeutralizer {
    // Patterns are defined as static LazyLock regexes
    // This struct is kept for future extensibility
}

// Regex patterns compiled once
static RE_ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__attribute__\s*\(\(").unwrap());
static RE_EXTENSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"__extension__\s*").unwrap());
static RE_TYPEOF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"typeof\s*\(").unwrap());
static RE_TYPEOF_UNDERSCORE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__typeof__\s*\(").unwrap());
static RE_TYPEOF_SINGLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"__typeof\s*\(").unwrap());
static RE_ASM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__asm__\s*(?:volatile\s*)?\(").unwrap());
static RE_ASM_VOLATILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__asm\s+volatile\s*\(").unwrap());
static RE_RESTRICT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"__restrict\s+").unwrap());
static RE_RESTRICT_UNDERSCORE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__restrict__\s+").unwrap());
static RE_INLINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"__inline__\s+").unwrap());
static RE_INLINE_SINGLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"__inline\s+").unwrap());
static RE_VOLATILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"__volatile__\s+").unwrap());
static RE_VOLATILE_SINGLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__volatile\s+").unwrap());
static RE_STATEMENT_EXPR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(\{").unwrap());
static RE_BUILTIN_OFFSETOF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__builtin_offsetof\s*\(").unwrap());
static RE_BUILTIN_TYPES_COMPATIBLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"__builtin_types_compatible_p\s*\(").unwrap());

impl Default for GccNeutralizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GccNeutralizer {
    pub fn new() -> Self {
        Self {}
    }

    /// Neutralize GCC extensions in source code
    pub fn neutralize(&self, source: &str) -> NeutralizedSource {
        let mut code = source.to_string();
        let mut transformations = Vec::new();

        // Process each pattern type
        // Order matters - some patterns may be nested

        // 1. __attribute__((...))
        while let Some(m) = RE_ATTRIBUTE.find(&code) {
            let start = m.start();
            if let Some((end, _original)) = Self::find_double_paren_end(&code, start + m.len()) {
                let original_text = code[start..end].to_string();
                transformations.push(Transformation {
                    original_start: start,
                    original_length: end - start,
                    transformed_start: start,
                    transformed_length: 0,
                    kind: TransformKind::Attribute,
                    original_text,
                });
                code = format!("{}{}", &code[..start], &code[end..]);
            } else {
                break;
            }
        }

        // 2. __extension__
        while let Some(m) = RE_EXTENSION.find(&code) {
            let start = m.start();
            let end = m.end();
            transformations.push(Transformation {
                original_start: start,
                original_length: end - start,
                transformed_start: start,
                transformed_length: 0,
                kind: TransformKind::Extension,
                original_text: code[start..end].to_string(),
            });
            code = format!("{}{}", &code[..start], &code[end..]);
        }

        // 3. typeof(...) → int
        for regex in [&*RE_TYPEOF, &*RE_TYPEOF_UNDERSCORE, &*RE_TYPEOF_SINGLE] {
            while let Some(m) = regex.find(&code) {
                let start = m.start();
                if let Some((end, _)) = Self::find_matching_paren(&code, m.end() - 1) {
                    let original_text = code[start..end].to_string();
                    transformations.push(Transformation {
                        original_start: start,
                        original_length: end - start,
                        transformed_start: start,
                        transformed_length: 3, // "int"
                        kind: TransformKind::Typeof,
                        original_text,
                    });
                    code = format!("{}int{}", &code[..start], &code[end..]);
                } else {
                    break;
                }
            }
        }

        // 4. __asm__ volatile(...) - replace entire statement with empty
        for regex in [&*RE_ASM, &*RE_ASM_VOLATILE] {
            while let Some(m) = regex.find(&code) {
                let start = m.start();
                if let Some((end, _)) = Self::find_matching_paren(&code, m.end() - 1) {
                    // Find the semicolon after the asm statement
                    let stmt_end = code[end..].find(';').map(|i| end + i + 1).unwrap_or(end);
                    let original_text = code[start..stmt_end].to_string();
                    transformations.push(Transformation {
                        original_start: start,
                        original_length: stmt_end - start,
                        transformed_start: start,
                        transformed_length: 4, // "0 ; "
                        kind: TransformKind::Asm,
                        original_text,
                    });
                    // Replace with a simple expression statement
                    code = format!("{}0{}", &code[..start], &code[stmt_end..]);
                } else {
                    break;
                }
            }
        }

        // 5. __restrict / __restrict__
        for regex in [&*RE_RESTRICT, &*RE_RESTRICT_UNDERSCORE] {
            while let Some(m) = regex.find(&code) {
                let start = m.start();
                let end = m.end();
                transformations.push(Transformation {
                    original_start: start,
                    original_length: end - start,
                    transformed_start: start,
                    transformed_length: 0,
                    kind: TransformKind::Restrict,
                    original_text: code[start..end].to_string(),
                });
                code = format!("{}{}", &code[..start], &code[end..]);
            }
        }

        // 6. __inline__ / __inline
        for regex in [&*RE_INLINE, &*RE_INLINE_SINGLE] {
            while let Some(m) = regex.find(&code) {
                let start = m.start();
                let end = m.end();
                transformations.push(Transformation {
                    original_start: start,
                    original_length: end - start,
                    transformed_start: start,
                    transformed_length: 0,
                    kind: TransformKind::Inline,
                    original_text: code[start..end].to_string(),
                });
                code = format!("{}{}", &code[..start], &code[end..]);
            }
        }

        // 7. __volatile__ / __volatile
        for regex in [&*RE_VOLATILE, &*RE_VOLATILE_SINGLE] {
            while let Some(m) = regex.find(&code) {
                let start = m.start();
                let end = m.end();
                transformations.push(Transformation {
                    original_start: start,
                    original_length: end - start,
                    transformed_start: start,
                    transformed_length: 0,
                    kind: TransformKind::Volatile,
                    original_text: code[start..end].to_string(),
                });
                code = format!("{}{}", &code[..start], &code[end..]);
            }
        }

        // 8. Statement expressions ({ ... }) → (0)
        while let Some(m) = RE_STATEMENT_EXPR.find(&code) {
            let start = m.start();
            if let Some((end, _)) = Self::find_statement_expr_end(&code, start) {
                let original_text = code[start..end].to_string();
                transformations.push(Transformation {
                    original_start: start,
                    original_length: end - start,
                    transformed_start: start,
                    transformed_length: 3, // "(0)"
                    kind: TransformKind::StatementExpression,
                    original_text,
                });
                code = format!("{}(0){}", &code[..start], &code[end..]);
            } else {
                break;
            }
        }

        // 9. __builtin_offsetof → 0
        while let Some(m) = RE_BUILTIN_OFFSETOF.find(&code) {
            let start = m.start();
            if let Some((end, _)) = Self::find_matching_paren(&code, m.end() - 1) {
                let original_text = code[start..end].to_string();
                transformations.push(Transformation {
                    original_start: start,
                    original_length: end - start,
                    transformed_start: start,
                    transformed_length: 1, // "0"
                    kind: TransformKind::AlignofSizeof,
                    original_text,
                });
                code = format!("{}0{}", &code[..start], &code[end..]);
            } else {
                break;
            }
        }

        // 10. __builtin_types_compatible_p → 0
        while let Some(m) = RE_BUILTIN_TYPES_COMPATIBLE.find(&code) {
            let start = m.start();
            if let Some((end, _)) = Self::find_matching_paren(&code, m.end() - 1) {
                let original_text = code[start..end].to_string();
                transformations.push(Transformation {
                    original_start: start,
                    original_length: end - start,
                    transformed_start: start,
                    transformed_length: 1, // "0"
                    kind: TransformKind::AlignofSizeof,
                    original_text,
                });
                code = format!("{}0{}", &code[..start], &code[end..]);
            } else {
                break;
            }
        }

        NeutralizedSource {
            code,
            transformations,
        }
    }

    /// Find the end of a double-parenthesis expression like __attribute__((...))
    fn find_double_paren_end(code: &str, start: usize) -> Option<(usize, String)> {
        let bytes = code.as_bytes();
        let mut depth = 2; // Already inside "(("
        let mut i = start;

        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b'"' => {
                    // Skip string literal
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'\'' => {
                    // Skip char literal
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }

        if depth == 0 {
            Some((i, code[start..i].to_string()))
        } else {
            None
        }
    }

    /// Find the end of a parenthesized expression
    fn find_matching_paren(code: &str, start: usize) -> Option<(usize, String)> {
        let bytes = code.as_bytes();
        if start >= bytes.len() || bytes[start] != b'(' {
            return None;
        }

        let mut depth = 1;
        let mut i = start + 1;

        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b'"' => {
                    // Skip string literal
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'\'' => {
                    // Skip char literal
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }

        if depth == 0 {
            Some((i, code[start..i].to_string()))
        } else {
            None
        }
    }

    /// Find the end of a statement expression ({ ... })
    fn find_statement_expr_end(code: &str, start: usize) -> Option<(usize, String)> {
        let bytes = code.as_bytes();
        if start + 1 >= bytes.len() || bytes[start] != b'(' || bytes[start + 1] != b'{' {
            return None;
        }

        let mut paren_depth = 1;
        let mut brace_depth = 1;
        let mut i = start + 2;

        while i < bytes.len() && (paren_depth > 0 || brace_depth > 0) {
            match bytes[i] {
                b'(' => paren_depth += 1,
                b')' => paren_depth -= 1,
                b'{' => brace_depth += 1,
                b'}' => brace_depth -= 1,
                b'"' => {
                    // Skip string literal
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                b'\'' => {
                    // Skip char literal
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }

        if paren_depth == 0 && brace_depth == 0 {
            Some((i, code[start..i].to_string()))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neutralize_attribute() {
        let neutralizer = GccNeutralizer::new();
        let source = "void __attribute__((packed)) foo(void) {}";
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("__attribute__"));
        assert!(result.code.contains("void  foo(void) {}"));
        assert!(!result.transformations.is_empty());
        assert_eq!(result.transformations[0].kind, TransformKind::Attribute);
    }

    #[test]
    fn test_neutralize_attribute_nested() {
        let neutralizer = GccNeutralizer::new();
        let source = "void __attribute__((unused, aligned(16))) bar(void) {}";
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("__attribute__"));
        assert!(result.code.contains("void  bar(void) {}"));
    }

    #[test]
    fn test_neutralize_extension() {
        let neutralizer = GccNeutralizer::new();
        let source = "__extension__ union { int x; float y; };";
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("__extension__"));
        assert!(result.code.contains("union { int x; float y; };"));
    }

    #[test]
    fn test_neutralize_typeof() {
        let neutralizer = GccNeutralizer::new();
        let source = "typeof(foo) bar;";
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("typeof"));
        assert!(result.code.contains("int bar;"));
    }

    #[test]
    fn test_neutralize_typeof_underscore() {
        let neutralizer = GccNeutralizer::new();
        let source = "__typeof__(x) y;";
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("__typeof__"));
        assert!(result.code.contains("int y;"));
    }

    #[test]
    fn test_neutralize_asm() {
        let neutralizer = GccNeutralizer::new();
        let source = "void foo(void) { __asm__ volatile(\"nop\"); }";
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("__asm__"));
        // The asm statement should be replaced with a simple statement
        assert!(result.code.contains("{ 0 }"));
    }

    #[test]
    fn test_neutralize_restrict() {
        let neutralizer = GccNeutralizer::new();
        let source = "void foo(int * __restrict p) {}";
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("__restrict"));
        assert!(result.code.contains("int * p"));
    }

    #[test]
    fn test_neutralize_inline() {
        let neutralizer = GccNeutralizer::new();
        let source = "__inline__ void foo(void) {}";
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("__inline__"));
        assert!(result.code.contains("void foo(void)"));
    }

    #[test]
    fn test_neutralize_statement_expression() {
        let neutralizer = GccNeutralizer::new();
        let source = "int x = ({ int y = 5; y + 1; });";
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("({"));
        assert!(result.code.contains("int x = (0);"));
    }

    #[test]
    fn test_neutralize_builtin_offsetof() {
        let neutralizer = GccNeutralizer::new();
        let source = "int off = __builtin_offsetof(struct foo, bar);";
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("__builtin_offsetof"));
        assert!(result.code.contains("int off = 0;"));
    }

    #[test]
    fn test_neutralize_multiple() {
        let neutralizer = GccNeutralizer::new();
        let source = r#"
__extension__ struct {
    __attribute__((packed)) int x;
} __attribute__((aligned(16)));
"#;
        let result = neutralizer.neutralize(source);

        assert!(!result.code.contains("__extension__"));
        assert!(!result.code.contains("__attribute__"));
    }

    #[test]
    fn test_transformation_tracking() {
        let neutralizer = GccNeutralizer::new();
        let source = "__attribute__((unused)) int x;";
        let result = neutralizer.neutralize(source);

        assert!(!result.transformations.is_empty());
        let trans = &result.transformations[0];
        assert_eq!(trans.kind, TransformKind::Attribute);
        assert!(trans.original_text.contains("__attribute__"));
    }

    #[test]
    fn test_no_false_positives() {
        let neutralizer = GccNeutralizer::new();
        let source = "int attribute_count; int typeof_var;";
        let result = neutralizer.neutralize(source);

        // Should not transform regular identifiers that contain the keywords
        assert!(result.code.contains("attribute_count"));
        // typeof_var contains "typeof" but not as "typeof("
        assert!(result.code.contains("typeof_var"));
    }

    #[test]
    fn test_string_literal_handling() {
        let neutralizer = GccNeutralizer::new();
        // Note: The current regex-based implementation may transform patterns
        // inside string literals. This is a known limitation that could be
        // addressed with a more sophisticated tokenizer.
        let source = "char *s = \"test\";";
        let result = neutralizer.neutralize(source);

        // Simple strings without __attribute__ should be preserved
        assert!(result.code.contains("\"test\""));
    }
}
