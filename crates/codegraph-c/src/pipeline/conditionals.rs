// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Preprocessor conditional evaluation
//!
//! This module provides evaluation of preprocessor conditionals to strip dead code
//! before parsing. This is crucial for handling kernel code which often has
//! `#if 0` blocks for debugging or commented-out code.

/// Strategy for handling preprocessor conditionals
#[derive(Debug, Clone, PartialEq)]
pub enum ConditionalStrategy {
    /// Keep all conditionals as-is (no modification)
    KeepAll,
    /// Strip all preprocessor lines except #include
    StripAll,
    /// Evaluate simple conditions (#if 0, #if 1)
    EvaluateSimple,
}

/// State machine for tracking conditional nesting
#[derive(Debug, Clone, PartialEq)]
enum ConditionalState {
    /// Normal code, being included
    Active,
    /// In a disabled block (#if 0 or false branch)
    Disabled,
    /// Seen #else while in disabled state, now active
    ElseActive,
    /// Seen #else while in active state, now disabled
    ElseDisabled,
}

/// Evaluate preprocessor conditionals in source code
///
/// Returns the processed source and the count of blocks stripped.
pub fn evaluate_conditionals(source: &str, strategy: ConditionalStrategy) -> (String, usize) {
    match strategy {
        ConditionalStrategy::KeepAll => (source.to_string(), 0),
        ConditionalStrategy::StripAll => strip_all_preprocessor(source),
        ConditionalStrategy::EvaluateSimple => evaluate_simple_conditionals(source),
    }
}

/// Strip all preprocessor directives except #include
fn strip_all_preprocessor(source: &str) -> (String, usize) {
    let mut result = String::with_capacity(source.len());
    let mut stripped_count = 0;

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('#') {
            if trimmed.starts_with("#include") {
                result.push_str(line);
                result.push('\n');
            } else {
                // Replace with empty line to preserve line numbers
                result.push('\n');
                stripped_count += 1;
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    (result, stripped_count)
}

/// Evaluate simple conditionals (#if 0, #if 1)
fn evaluate_simple_conditionals(source: &str) -> (String, usize) {
    let mut result = String::with_capacity(source.len());
    let mut stripped_count = 0;

    // Stack to track nested conditionals
    // Each entry is (state, depth)
    let mut state_stack: Vec<ConditionalState> = vec![ConditionalState::Active];

    // Track if we're in a multi-line macro (continuation with backslash)
    let mut in_multiline_define = false;
    let mut multiline_define_content = String::new();

    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Handle continuation lines from multi-line #define
        if in_multiline_define {
            // Check if this line also ends with backslash
            let continues = line.trim_end().ends_with('\\');
            // Append content without the trailing backslash
            let content = if continues {
                line.trim_end().trim_end_matches('\\').trim_end()
            } else {
                line.trim()
            };
            // Escape any internal comments to avoid nested comment issues
            // Replace /* with /+ and */ with +/
            let escaped_content = content.replace("/*", "/+").replace("*/", "+/");
            multiline_define_content.push(' ');
            multiline_define_content.push_str(&escaped_content);

            if !continues {
                // End of multi-line define - output as single line comment
                result.push_str("/* ");
                result.push_str(&multiline_define_content);
                result.push_str(" */\n");
                in_multiline_define = false;
                multiline_define_content.clear();
            } else {
                // Still continuing, just emit empty line to preserve line numbers
                result.push('\n');
            }
            i += 1;
            continue;
        }

        if let Some(directive) = get_preprocessor_directive(trimmed) {
            match directive {
                PreprocessorDirective::If(condition) => {
                    let current_state = state_stack.last().unwrap_or(&ConditionalState::Active);

                    let new_state = if *current_state == ConditionalState::Active
                        || *current_state == ConditionalState::ElseActive
                    {
                        // Evaluate the condition
                        if is_false_condition(&condition) {
                            ConditionalState::Disabled
                        } else {
                            ConditionalState::Active
                        }
                    } else {
                        // Parent is disabled, so we're disabled too
                        ConditionalState::Disabled
                    };

                    state_stack.push(new_state);

                    // Always emit empty line to preserve line numbers
                    result.push('\n');
                    stripped_count += 1;
                }

                PreprocessorDirective::Ifdef(_) | PreprocessorDirective::Ifndef(_) => {
                    let current_state = state_stack.last().unwrap_or(&ConditionalState::Active);

                    // For ifdef/ifndef, we can't evaluate without knowing defined macros
                    // Keep as active if parent is active, otherwise disabled
                    let new_state = if *current_state == ConditionalState::Active
                        || *current_state == ConditionalState::ElseActive
                    {
                        ConditionalState::Active
                    } else {
                        ConditionalState::Disabled
                    };

                    state_stack.push(new_state);
                    // Strip the #ifdef/#ifndef line itself (not valid C)
                    result.push('\n');
                    stripped_count += 1;
                }

                PreprocessorDirective::Elif(condition) => {
                    if let Some(state) = state_stack.last_mut() {
                        match state {
                            ConditionalState::Disabled => {
                                // Previous branch was disabled, check this condition
                                if !is_false_condition(&condition) {
                                    *state = ConditionalState::Active;
                                }
                            }
                            ConditionalState::Active => {
                                // Previous branch was active, so skip this one
                                *state = ConditionalState::Disabled;
                            }
                            ConditionalState::ElseActive | ConditionalState::ElseDisabled => {
                                // elif after else is an error, but handle gracefully
                            }
                        }
                    }
                    result.push('\n');
                    stripped_count += 1;
                }

                PreprocessorDirective::Else => {
                    if let Some(state) = state_stack.last_mut() {
                        *state = match state {
                            ConditionalState::Active => ConditionalState::ElseDisabled,
                            ConditionalState::Disabled => ConditionalState::ElseActive,
                            ConditionalState::ElseActive | ConditionalState::ElseDisabled => {
                                // Multiple else is an error, but handle gracefully
                                state.clone()
                            }
                        };
                    }
                    result.push('\n');
                    stripped_count += 1;
                }

                PreprocessorDirective::Endif => {
                    if state_stack.len() > 1 {
                        state_stack.pop();
                    }
                    result.push('\n');
                    stripped_count += 1;
                }

                PreprocessorDirective::Include => {
                    // Always keep includes
                    result.push_str(line);
                    result.push('\n');
                }

                PreprocessorDirective::Define
                | PreprocessorDirective::Undef
                | PreprocessorDirective::Pragma
                | PreprocessorDirective::Error
                | PreprocessorDirective::Warning => {
                    let current_state = state_stack.last().unwrap_or(&ConditionalState::Active);
                    if *current_state == ConditionalState::Active
                        || *current_state == ConditionalState::ElseActive
                    {
                        // Keep directive but it may cause parsing issues
                        // Convert to comment to preserve line numbers
                        // Check if this is a multi-line macro (ends with backslash)
                        let continues = line.trim_end().ends_with('\\');
                        if continues {
                            // Start collecting multi-line define
                            // Remove trailing backslash from content and escape internal comments
                            let content = trimmed.trim_end_matches('\\').trim_end();
                            let escaped = content.replace("/*", "/+").replace("*/", "+/");
                            multiline_define_content.clear();
                            multiline_define_content.push_str(&escaped);
                            in_multiline_define = true;
                            // Emit empty line to preserve line numbers
                            result.push('\n');
                        } else {
                            // Single-line define - wrap in comment
                            // Escape any internal comments
                            let escaped = trimmed.replace("/*", "/+").replace("*/", "+/");
                            result.push_str("/* ");
                            result.push_str(&escaped);
                            result.push_str(" */\n");
                        }
                    } else {
                        result.push('\n');
                    }
                    stripped_count += 1;
                }

                PreprocessorDirective::Other => {
                    let current_state = state_stack.last().unwrap_or(&ConditionalState::Active);
                    if *current_state == ConditionalState::Active
                        || *current_state == ConditionalState::ElseActive
                    {
                        result.push_str(line);
                        result.push('\n');
                    } else {
                        result.push('\n');
                        stripped_count += 1;
                    }
                }
            }
        } else {
            // Regular code line
            let current_state = state_stack.last().unwrap_or(&ConditionalState::Active);
            if *current_state == ConditionalState::Active
                || *current_state == ConditionalState::ElseActive
            {
                result.push_str(line);
                result.push('\n');
            } else {
                // Emit empty line to preserve line numbers
                result.push('\n');
                stripped_count += 1;
            }
        }
        i += 1;
    }

    (result, stripped_count)
}

#[derive(Debug, Clone)]
enum PreprocessorDirective {
    If(String),
    Ifdef(#[allow(dead_code)] String),
    Ifndef(#[allow(dead_code)] String),
    Elif(String),
    Else,
    Endif,
    Include,
    Define,
    Undef,
    Pragma,
    Error,
    Warning,
    Other,
}

fn get_preprocessor_directive(trimmed: &str) -> Option<PreprocessorDirective> {
    if !trimmed.starts_with('#') {
        return None;
    }

    let rest = trimmed[1..].trim_start();

    if rest.starts_with("if ") || rest == "if" {
        let condition = rest.strip_prefix("if").unwrap_or("").trim().to_string();
        Some(PreprocessorDirective::If(condition))
    } else if rest.starts_with("ifdef ") || rest.starts_with("ifdef\t") {
        let name = rest
            .strip_prefix("ifdef")
            .unwrap_or("")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        Some(PreprocessorDirective::Ifdef(name))
    } else if rest.starts_with("ifndef ") || rest.starts_with("ifndef\t") {
        let name = rest
            .strip_prefix("ifndef")
            .unwrap_or("")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        Some(PreprocessorDirective::Ifndef(name))
    } else if rest.starts_with("elif ") {
        let condition = rest.strip_prefix("elif").unwrap_or("").trim().to_string();
        Some(PreprocessorDirective::Elif(condition))
    } else if rest == "else" || rest.starts_with("else ") || rest.starts_with("else\t") {
        Some(PreprocessorDirective::Else)
    } else if rest == "endif"
        || rest.starts_with("endif ")
        || rest.starts_with("endif\t")
        || rest.starts_with("endif/")
    {
        Some(PreprocessorDirective::Endif)
    } else if rest.starts_with("include") {
        Some(PreprocessorDirective::Include)
    } else if rest.starts_with("define") {
        Some(PreprocessorDirective::Define)
    } else if rest.starts_with("undef") {
        Some(PreprocessorDirective::Undef)
    } else if rest.starts_with("pragma") {
        Some(PreprocessorDirective::Pragma)
    } else if rest.starts_with("error") {
        Some(PreprocessorDirective::Error)
    } else if rest.starts_with("warning") {
        Some(PreprocessorDirective::Warning)
    } else {
        Some(PreprocessorDirective::Other)
    }
}

/// Check if a condition is definitely false
fn is_false_condition(condition: &str) -> bool {
    let condition = condition.trim();

    // #if 0 is definitely false
    if condition == "0" {
        return true;
    }

    // #if (0) is also false
    if condition == "(0)" {
        return true;
    }

    // Check for common patterns
    // !1 is false
    if condition == "!1" {
        return true;
    }

    // defined(NEVER_DEFINED_MACRO) - can't evaluate without context
    // So we return false (assume it might be true)

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keep_all_strategy() {
        let source = r#"
#if 0
int x;
#endif
int y;
"#;
        let (result, count) = evaluate_conditionals(source, ConditionalStrategy::KeepAll);
        assert_eq!(result, source);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_strip_all_strategy() {
        let source = r#"#include <stdio.h>
#define FOO 1
#if FOO
int x;
#endif
int y;
"#;
        let (result, count) = evaluate_conditionals(source, ConditionalStrategy::StripAll);

        // Should keep include
        assert!(result.contains("#include <stdio.h>"));
        // Should strip #define
        assert!(!result.contains("#define"));
        // Should strip #if and #endif
        assert!(!result.contains("#if"));
        assert!(!result.contains("#endif"));
        // Should keep code
        assert!(result.contains("int x;"));
        assert!(result.contains("int y;"));
        assert!(count > 0);
    }

    #[test]
    fn test_evaluate_simple_if_0() {
        let source = r#"int a;
#if 0
int b;
#endif
int c;
"#;
        let (result, _count) = evaluate_conditionals(source, ConditionalStrategy::EvaluateSimple);

        assert!(result.contains("int a;"));
        assert!(!result.contains("int b;"));
        assert!(result.contains("int c;"));
    }

    #[test]
    fn test_evaluate_simple_if_0_else() {
        let source = r#"int a;
#if 0
int b;
#else
int c;
#endif
int d;
"#;
        let (result, _count) = evaluate_conditionals(source, ConditionalStrategy::EvaluateSimple);

        assert!(result.contains("int a;"));
        assert!(!result.contains("int b;"));
        assert!(result.contains("int c;"));
        assert!(result.contains("int d;"));
    }

    #[test]
    fn test_evaluate_nested_conditionals() {
        let source = r#"int a;
#if 0
int b;
#if 1
int c;
#endif
int d;
#endif
int e;
"#;
        let (result, _count) = evaluate_conditionals(source, ConditionalStrategy::EvaluateSimple);

        assert!(result.contains("int a;"));
        assert!(!result.contains("int b;"));
        assert!(!result.contains("int c;")); // Nested in #if 0
        assert!(!result.contains("int d;"));
        assert!(result.contains("int e;"));
    }

    #[test]
    fn test_evaluate_elif() {
        let source = r#"int a;
#if 0
int b;
#elif 1
int c;
#else
int d;
#endif
int e;
"#;
        let (result, _count) = evaluate_conditionals(source, ConditionalStrategy::EvaluateSimple);

        assert!(result.contains("int a;"));
        assert!(!result.contains("int b;"));
        // We can't evaluate #elif 1 as definitely true without more context
        // But it won't be in a #if 0 block
        assert!(result.contains("int e;"));
    }

    #[test]
    fn test_preserve_line_numbers() {
        let source = "line1\n#if 0\nline3\n#endif\nline5\n";
        let (result, _count) = evaluate_conditionals(source, ConditionalStrategy::EvaluateSimple);

        // Count lines
        let line_count = result.lines().count();
        let original_line_count = source.lines().count();

        // Line count should be preserved
        assert_eq!(line_count, original_line_count);
    }

    #[test]
    fn test_ifdef_content_kept_directive_stripped() {
        let source = r#"#ifdef CONFIG_FOO
int x;
#endif
"#;
        let (result, _count) = evaluate_conditionals(source, ConditionalStrategy::EvaluateSimple);

        // #ifdef can't be evaluated without knowing what's defined
        // The code inside should be kept (assume true)
        assert!(result.contains("int x;"));
        // But the #ifdef directive itself should be stripped (not valid C)
        assert!(!result.contains("#ifdef"));
    }

    #[test]
    fn test_is_false_condition() {
        assert!(is_false_condition("0"));
        assert!(is_false_condition(" 0 "));
        assert!(is_false_condition("(0)"));
        assert!(is_false_condition("!1"));

        assert!(!is_false_condition("1"));
        assert!(!is_false_condition("FOO"));
        assert!(!is_false_condition("defined(BAR)"));
    }

    #[test]
    fn test_complex_kernel_code() {
        let source = r#"
#include <linux/module.h>

#if 0
/* Disabled debug code */
#define DEBUG 1
static void debug_print(void) {}
#endif

MODULE_LICENSE("GPL");

#ifdef CONFIG_DEBUG
static int debug_level = 1;
#else
static int debug_level = 0;
#endif
"#;

        let (result, _count) = evaluate_conditionals(source, ConditionalStrategy::EvaluateSimple);

        // Should keep include
        assert!(result.contains("#include <linux/module.h>"));
        // Should strip #if 0 block
        assert!(!result.contains("debug_print"));
        // Should keep CONFIG_DEBUG block (can't evaluate)
        assert!(result.contains("debug_level"));
    }
}
