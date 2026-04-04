// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Layered processing pipeline for C source code
//!
//! This module provides a multi-stage processing pipeline that transforms C source code
//! to make it more parseable by tree-sitter, while preserving semantic information.
//!
//! ## Pipeline Stages
//!
//! 1. **Platform Detection** - Identify target platform (Linux, FreeBSD, Darwin)
//! 2. **Header Stub Injection** - Inject type definitions for known headers
//! 3. **Conditional Evaluation** - Strip `#if 0` blocks, optionally evaluate simple conditions
//! 4. **GCC Neutralization** - Remove/replace GCC extensions
//! 5. **Attribute Stripping** - Remove platform-specific attributes
//!
//! After these stages, the code is ready for tree-sitter parsing.

mod conditionals;
mod gcc;
mod macros;

pub use conditionals::{evaluate_conditionals, ConditionalStrategy};
pub use gcc::{GccNeutralizer, NeutralizedSource, TransformKind, Transformation};
pub use macros::{MacroNeutralizer, MacroStats};

use crate::platform::{DetectionResult, HeaderStubs, PlatformRegistry};

/// Configuration for the processing pipeline
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Whether to inject header stubs
    pub inject_stubs: bool,
    /// Strategy for handling preprocessor conditionals
    pub conditional_strategy: ConditionalStrategy,
    /// Whether to neutralize GCC extensions
    pub neutralize_gcc: bool,
    /// Whether to strip platform-specific attributes
    pub strip_attributes: bool,
    /// Whether to neutralize kernel macros
    pub neutralize_macros: bool,
    /// Optional platform ID to force (bypasses detection)
    pub force_platform: Option<String>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            inject_stubs: true,
            conditional_strategy: ConditionalStrategy::EvaluateSimple,
            neutralize_gcc: true,
            strip_attributes: true,
            neutralize_macros: true,
            force_platform: None,
        }
    }
}

impl PipelineConfig {
    /// Configuration for minimal processing
    pub fn minimal() -> Self {
        Self {
            inject_stubs: false,
            conditional_strategy: ConditionalStrategy::KeepAll,
            neutralize_gcc: false,
            strip_attributes: false,
            neutralize_macros: false,
            force_platform: None,
        }
    }

    /// Configuration optimized for kernel code
    pub fn for_kernel_code() -> Self {
        Self {
            inject_stubs: true,
            conditional_strategy: ConditionalStrategy::EvaluateSimple,
            neutralize_gcc: true,
            strip_attributes: true,
            neutralize_macros: true,
            force_platform: Some("linux".to_string()),
        }
    }
}

/// Result of pipeline processing
#[derive(Debug)]
pub struct PipelineResult {
    /// Processed source code ready for parsing
    pub source: String,
    /// Detected or forced platform
    pub platform: DetectionResult,
    /// GCC transformation records (for position mapping)
    pub transformations: Vec<Transformation>,
    /// Statistics about processing
    pub stats: PipelineStats,
}

/// Statistics about pipeline processing
#[derive(Debug, Default, Clone)]
pub struct PipelineStats {
    /// Number of header stubs injected
    pub stubs_injected: usize,
    /// Number of conditional blocks stripped
    pub conditionals_stripped: usize,
    /// Number of GCC extensions neutralized
    pub gcc_neutralized: usize,
    /// Number of attributes stripped
    pub attributes_stripped: usize,
    /// Statistics about macro neutralization
    pub macro_stats: MacroStats,
    /// Original source length
    pub original_length: usize,
    /// Processed source length
    pub processed_length: usize,
}

/// The processing pipeline
pub struct Pipeline {
    registry: PlatformRegistry,
    gcc_neutralizer: GccNeutralizer,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            registry: PlatformRegistry::new(),
            gcc_neutralizer: GccNeutralizer::new(),
        }
    }

    /// Process source code through the pipeline
    pub fn process(&self, source: &str, config: &PipelineConfig) -> PipelineResult {
        let mut stats = PipelineStats {
            original_length: source.len(),
            ..Default::default()
        };

        // Step 1: Platform detection
        let platform = if let Some(ref forced) = config.force_platform {
            DetectionResult {
                platform_id: forced.clone(),
                confidence: 1.0,
                matched_patterns: vec!["forced".to_string()],
            }
        } else {
            self.registry.detect(source)
        };

        let platform_module = self.registry.get(&platform.platform_id);

        // Step 2: Header stub injection
        let mut processed = source.to_string();
        if config.inject_stubs {
            if let Some(module) = platform_module {
                let stubs = module.header_stubs().get_for_includes(source);
                if !stubs.is_empty() {
                    stats.stubs_injected = stubs.lines().filter(|l| l.contains("typedef")).count();
                    processed = format!("{stubs}\n{processed}");
                }
            }
        }

        // Step 3: Conditional evaluation
        let (processed, conditionals_stripped) =
            evaluate_conditionals(&processed, config.conditional_strategy.clone());
        stats.conditionals_stripped = conditionals_stripped;

        // Step 4: GCC extension neutralization
        let (processed, transformations) = if config.neutralize_gcc {
            let result = self.gcc_neutralizer.neutralize(&processed);
            stats.gcc_neutralized = result.transformations.len();
            (result.code, result.transformations)
        } else {
            (processed, Vec::new())
        };

        // Step 5: Attribute stripping
        let processed = if config.strip_attributes {
            if let Some(module) = platform_module {
                let (stripped, count) =
                    Self::strip_attributes(&processed, module.attributes_to_strip());
                stats.attributes_stripped = count;
                stripped
            } else {
                processed
            }
        } else {
            processed
        };

        // Step 6: Macro neutralization (kernel macros like likely/unlikely, BUILD_BUG_ON, etc.)
        let processed = if config.neutralize_macros {
            let mut macro_neutralizer = MacroNeutralizer::new();
            let result = macro_neutralizer.neutralize(&processed);
            stats.macro_stats = macro_neutralizer.stats().clone();
            result
        } else {
            processed
        };

        stats.processed_length = processed.len();

        PipelineResult {
            source: processed,
            platform,
            transformations,
            stats,
        }
    }

    /// Strip platform-specific attributes from source
    fn strip_attributes(source: &str, attributes: &[&str]) -> (String, usize) {
        let mut result = source.to_string();
        let mut count = 0;

        for attr in attributes {
            // Count occurrences before stripping
            let before_count = result.matches(attr).count();

            // Handle both plain attributes and function-like attributes
            let patterns = [format!("{attr} "), format!("{attr}\t"), format!("{attr}(")];

            for pattern in &patterns {
                while result.contains(pattern.as_str()) {
                    if pattern.ends_with('(') {
                        // For function-like attributes, find and remove with parentheses
                        if let Some(start) = result.find(attr) {
                            if let Some(paren_start) = result[start..].find('(') {
                                let abs_paren = start + paren_start;
                                let mut depth = 1;
                                let mut end = abs_paren + 1;
                                for (i, c) in result[abs_paren + 1..].char_indices() {
                                    match c {
                                        '(' => depth += 1,
                                        ')' => {
                                            depth -= 1;
                                            if depth == 0 {
                                                end = abs_paren + 1 + i + 1;
                                                break;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                result = format!("{}{}", &result[..start], &result[end..]);
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    } else {
                        result = result.replacen(pattern, "", 1);
                    }
                }
            }

            let after_count = result.matches(attr).count();
            count += before_count.saturating_sub(after_count);
        }

        (result, count)
    }

    /// Get the platform registry for direct access
    pub fn registry(&self) -> &PlatformRegistry {
        &self.registry
    }

    /// Get header stubs for a platform
    pub fn get_stubs(&self, platform_id: &str) -> Option<&HeaderStubs> {
        self.registry.get(platform_id).map(|p| p.header_stubs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_default_config() {
        let config = PipelineConfig::default();
        assert!(config.inject_stubs);
        assert!(config.neutralize_gcc);
        assert!(config.strip_attributes);
    }

    #[test]
    fn test_pipeline_minimal_config() {
        let config = PipelineConfig::minimal();
        assert!(!config.inject_stubs);
        assert!(!config.neutralize_gcc);
        assert!(!config.strip_attributes);
    }

    #[test]
    fn test_pipeline_kernel_config() {
        let config = PipelineConfig::for_kernel_code();
        assert!(config.inject_stubs);
        assert!(config.neutralize_gcc);
        assert_eq!(config.force_platform, Some("linux".to_string()));
    }

    #[test]
    fn test_pipeline_basic_processing() {
        let pipeline = Pipeline::new();
        let config = PipelineConfig::minimal();

        let source = "int main() { return 0; }";
        let result = pipeline.process(source, &config);

        assert_eq!(result.source, source);
        assert_eq!(result.stats.original_length, source.len());
    }

    #[test]
    fn test_pipeline_with_linux_detection() {
        let pipeline = Pipeline::new();
        let config = PipelineConfig::default();

        let source = r#"
#include <linux/module.h>
#include <linux/kernel.h>

MODULE_LICENSE("GPL");

static int __init my_init(void) {
    printk(KERN_INFO "Hello\n");
    return 0;
}
"#;

        let result = pipeline.process(source, &config);

        // Should detect Linux platform
        assert_eq!(result.platform.platform_id, "linux");
        assert!(result.platform.confidence > 0.5);

        // Verify stubs were available for the headers
        let stubs = pipeline.get_stubs("linux");
        assert!(stubs.is_some());
        let stubs = stubs.unwrap();
        assert!(stubs.has_stub("linux/module.h"));
        assert!(stubs.has_stub("linux/kernel.h"));

        // Processed source should have type definitions from stubs
        // Note: stubs_injected counts typedef lines
        assert!(
            result.source.contains("extern int printk")
                || result.source.contains("typedef")
                || result.stats.stubs_injected > 0
        );

        // Should have stripped __init attribute
        assert!(!result.source.contains("__init"));
    }

    #[test]
    fn test_pipeline_conditional_stripping() {
        let pipeline = Pipeline::new();
        let config = PipelineConfig {
            inject_stubs: false,
            conditional_strategy: ConditionalStrategy::EvaluateSimple,
            neutralize_gcc: false,
            strip_attributes: false,
            neutralize_macros: false,
            force_platform: None,
        };

        let source = r#"
int a;
#if 0
int b;
#endif
int c;
"#;

        let result = pipeline.process(source, &config);

        // Should have stripped the #if 0 block
        assert!(result.source.contains("int a;"));
        assert!(!result.source.contains("int b;"));
        assert!(result.source.contains("int c;"));
        assert!(result.stats.conditionals_stripped > 0);
    }

    #[test]
    fn test_pipeline_gcc_neutralization() {
        let pipeline = Pipeline::new();
        let config = PipelineConfig {
            inject_stubs: false,
            conditional_strategy: ConditionalStrategy::KeepAll,
            neutralize_gcc: true,
            strip_attributes: false,
            neutralize_macros: false,
            force_platform: None,
        };

        let source = "void __attribute__((packed)) foo(void) {}";
        let result = pipeline.process(source, &config);

        assert!(!result.source.contains("__attribute__"));
        assert!(result.stats.gcc_neutralized > 0);
    }

    #[test]
    fn test_pipeline_attribute_stripping() {
        let pipeline = Pipeline::new();
        let config = PipelineConfig {
            inject_stubs: false,
            conditional_strategy: ConditionalStrategy::KeepAll,
            neutralize_gcc: false,
            strip_attributes: true,
            neutralize_macros: false,
            force_platform: Some("linux".to_string()),
        };

        let source = "static __init int my_init(void) { return 0; }";
        let result = pipeline.process(source, &config);

        assert!(!result.source.contains("__init"));
        assert!(result.stats.attributes_stripped > 0);
    }

    #[test]
    fn test_pipeline_forced_platform() {
        let pipeline = Pipeline::new();
        let config = PipelineConfig {
            force_platform: Some("linux".to_string()),
            ..PipelineConfig::minimal()
        };

        let source = "int main() { return 0; }";
        let result = pipeline.process(source, &config);

        assert_eq!(result.platform.platform_id, "linux");
        assert!((result.platform.confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_pipeline_stats() {
        let pipeline = Pipeline::new();
        let config = PipelineConfig::for_kernel_code();

        let source = r#"
#include <linux/types.h>

static __init int my_init(void) {
    u32 x = 0;
    return x;
}
"#;

        let result = pipeline.process(source, &config);

        // Check stats
        assert!(result.stats.original_length > 0);
        assert!(result.stats.processed_length > 0);
    }
}
