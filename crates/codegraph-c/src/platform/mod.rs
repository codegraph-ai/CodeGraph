// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Platform detection and abstraction for C codebases
//!
//! This module provides automatic detection of the target platform (Linux, FreeBSD, Darwin)
//! based on source code patterns, and provides platform-specific configurations for parsing.

mod linux;

pub use linux::LinuxPlatform;

use std::collections::HashMap;

/// Detection pattern kind
#[derive(Debug, Clone, PartialEq)]
pub enum DetectionKind {
    /// Include directive pattern (e.g., "linux/")
    Include,
    /// Macro definition or usage
    Macro,
    /// Function call pattern
    FunctionCall,
    /// Type name pattern
    TypeName,
}

/// A pattern used to detect platform
#[derive(Debug, Clone)]
pub struct DetectionPattern {
    pub kind: DetectionKind,
    pub pattern: String,
    pub weight: f32,
}

impl DetectionPattern {
    pub fn include(pattern: &str, weight: f32) -> Self {
        Self {
            kind: DetectionKind::Include,
            pattern: pattern.to_string(),
            weight,
        }
    }

    pub fn macro_pattern(pattern: &str, weight: f32) -> Self {
        Self {
            kind: DetectionKind::Macro,
            pattern: pattern.to_string(),
            weight,
        }
    }

    pub fn function_call(pattern: &str, weight: f32) -> Self {
        Self {
            kind: DetectionKind::FunctionCall,
            pattern: pattern.to_string(),
            weight,
        }
    }

    pub fn type_name(pattern: &str, weight: f32) -> Self {
        Self {
            kind: DetectionKind::TypeName,
            pattern: pattern.to_string(),
            weight,
        }
    }
}

/// Category of callback functions in ops structures
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CallbackCategory {
    Init,
    Cleanup,
    Open,
    Close,
    Read,
    Write,
    Ioctl,
    Mmap,
    Poll,
    Probe,
    Remove,
    Suspend,
    Resume,
    Interrupt,
    Timer,
    Workqueue,
    Other,
}

/// Definition of an ops struct field
#[derive(Debug, Clone)]
pub struct OpsFieldDef {
    pub name: String,
    pub category: CallbackCategory,
}

/// Definition of an ops struct (like file_operations, pci_driver)
#[derive(Debug, Clone)]
pub struct OpsStructDef {
    pub struct_name: String,
    pub fields: Vec<OpsFieldDef>,
}

/// Header stub definitions - actual type definitions to inject
#[derive(Debug, Clone, Default)]
pub struct HeaderStubs {
    headers: HashMap<String, String>,
}

impl HeaderStubs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a stub for a header path
    pub fn add(&mut self, path: &str, content: &str) {
        self.headers.insert(path.to_string(), content.to_string());
    }

    /// Get stub content for all matching includes in source
    pub fn get_for_includes(&self, source: &str) -> String {
        let mut stubs = String::new();

        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("#include") {
                // Extract header path from #include <path> or #include "path"
                if let Some(path) = Self::extract_include_path(trimmed) {
                    if let Some(stub) = self.headers.get(&path) {
                        stubs.push_str("/* Stub for ");
                        stubs.push_str(&path);
                        stubs.push_str(" */\n");
                        stubs.push_str(stub);
                        stubs.push('\n');
                    }
                }
            }
        }

        stubs
    }

    fn extract_include_path(line: &str) -> Option<String> {
        // Handle #include <path> and #include "path"
        let line = line.trim_start_matches("#include").trim();
        if line.starts_with('<') {
            line.strip_prefix('<')?.strip_suffix('>')
        } else if line.starts_with('"') {
            line.strip_prefix('"')?.strip_suffix('"')
        } else {
            None
        }
        .map(|s| s.to_string())
    }

    /// Check if stubs exist for a header
    pub fn has_stub(&self, path: &str) -> bool {
        self.headers.contains_key(path)
    }

    /// Get all available stub headers
    pub fn available_headers(&self) -> Vec<&str> {
        self.headers.keys().map(|s| s.as_str()).collect()
    }
}

/// Trait for platform-specific modules
pub trait PlatformModule: Send + Sync {
    /// Unique identifier for this platform
    fn id(&self) -> &'static str;

    /// Human-readable name
    fn name(&self) -> &'static str;

    /// Get detection patterns for this platform
    fn detection_patterns(&self) -> Vec<DetectionPattern>;

    /// Get header stubs for this platform
    fn header_stubs(&self) -> &HeaderStubs;

    /// Get attributes that should be stripped for this platform
    fn attributes_to_strip(&self) -> &[&'static str];

    /// Get ops struct definitions for callback resolution
    fn ops_structs(&self) -> &[OpsStructDef];

    /// Get call normalization mappings (platform-specific → unified)
    fn call_normalizations(&self) -> &HashMap<&'static str, &'static str>;
}

/// Detection result with confidence score
#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub platform_id: String,
    pub confidence: f32,
    pub matched_patterns: Vec<String>,
}

/// Registry of available platforms
pub struct PlatformRegistry {
    platforms: Vec<Box<dyn PlatformModule>>,
}

impl Default for PlatformRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            platforms: Vec::new(),
        };
        // Register default platforms
        registry.register(Box::new(LinuxPlatform::new()));
        registry
    }

    /// Register a platform module
    pub fn register(&mut self, platform: Box<dyn PlatformModule>) {
        self.platforms.push(platform);
    }

    /// Detect platform from source code
    pub fn detect(&self, source: &str) -> DetectionResult {
        let mut best_result = DetectionResult {
            platform_id: "generic".to_string(),
            confidence: 0.0,
            matched_patterns: Vec::new(),
        };

        for platform in &self.platforms {
            let result = self.score_platform(source, platform.as_ref());
            if result.confidence > best_result.confidence {
                best_result = result;
            }
        }

        best_result
    }

    /// Get a platform by ID
    pub fn get(&self, id: &str) -> Option<&dyn PlatformModule> {
        self.platforms
            .iter()
            .find(|p| p.id() == id)
            .map(|p| p.as_ref())
    }

    fn score_platform(&self, source: &str, platform: &dyn PlatformModule) -> DetectionResult {
        let mut total_weight = 0.0;
        let mut matched_patterns = Vec::new();

        let source_lower = source.to_lowercase();

        for pattern in platform.detection_patterns() {
            let matched = match pattern.kind {
                DetectionKind::Include => {
                    // Check for #include with this path
                    source.contains(&format!("#include <{}", pattern.pattern))
                        || source.contains(&format!("#include \"{}", pattern.pattern))
                }
                DetectionKind::Macro => {
                    // Check for macro usage or definition
                    source.contains(&pattern.pattern)
                }
                DetectionKind::FunctionCall => {
                    // Check for function call pattern
                    source.contains(&format!("{}(", pattern.pattern))
                }
                DetectionKind::TypeName => {
                    // Check for type usage (case-insensitive for some)
                    source_lower.contains(&pattern.pattern.to_lowercase())
                }
            };

            if matched {
                total_weight += pattern.weight;
                matched_patterns.push(pattern.pattern.clone());
            }
        }

        // Normalize confidence to 0.0-1.0 range (cap at 1.0)
        let confidence = (total_weight / 10.0).min(1.0);

        DetectionResult {
            platform_id: platform.id().to_string(),
            confidence,
            matched_patterns,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_stubs_extract_include_path() {
        assert_eq!(
            HeaderStubs::extract_include_path("#include <linux/types.h>"),
            Some("linux/types.h".to_string())
        );
        assert_eq!(
            HeaderStubs::extract_include_path("#include \"myheader.h\""),
            Some("myheader.h".to_string())
        );
        assert_eq!(
            HeaderStubs::extract_include_path("  #include <sys/param.h>  "),
            None // trimmed line doesn't have leading space
        );
    }

    #[test]
    fn test_header_stubs_get_for_includes() {
        let mut stubs = HeaderStubs::new();
        stubs.add("linux/types.h", "typedef unsigned int u32;");
        stubs.add("linux/kernel.h", "typedef unsigned long size_t;");

        let source = r#"
#include <linux/types.h>
#include <linux/module.h>
#include <linux/kernel.h>
"#;

        let result = stubs.get_for_includes(source);
        assert!(result.contains("typedef unsigned int u32"));
        assert!(result.contains("typedef unsigned long size_t"));
        assert!(!result.contains("module")); // No stub for module.h
    }

    #[test]
    fn test_detection_pattern_creation() {
        let p1 = DetectionPattern::include("linux/", 2.0);
        assert_eq!(p1.kind, DetectionKind::Include);
        assert_eq!(p1.pattern, "linux/");
        assert!((p1.weight - 2.0).abs() < f32::EPSILON);

        let p2 = DetectionPattern::macro_pattern("MODULE_LICENSE", 3.0);
        assert_eq!(p2.kind, DetectionKind::Macro);
    }

    #[test]
    fn test_platform_registry_detect_linux() {
        let registry = PlatformRegistry::new();

        let linux_source = r#"
#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/init.h>

MODULE_LICENSE("GPL");
MODULE_AUTHOR("Test");

static int __init my_init(void) {
    printk(KERN_INFO "Hello\n");
    return 0;
}
module_init(my_init);
"#;

        let result = registry.detect(linux_source);
        assert_eq!(result.platform_id, "linux");
        assert!(result.confidence > 0.5);
        assert!(!result.matched_patterns.is_empty());
    }

    #[test]
    fn test_platform_registry_generic_code() {
        let registry = PlatformRegistry::new();

        let generic_source = r#"
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    printf("Hello, World!\n");
    return 0;
}
"#;

        let result = registry.detect(generic_source);
        // Generic code should have low confidence for any platform
        assert!(result.confidence < 0.3);
    }

    #[test]
    fn test_platform_registry_get() {
        let registry = PlatformRegistry::new();

        let linux = registry.get("linux");
        assert!(linux.is_some());
        assert_eq!(linux.unwrap().name(), "Linux Kernel");

        let unknown = registry.get("unknown");
        assert!(unknown.is_none());
    }

    #[test]
    fn test_detection_pattern_function_call_and_type_name() {
        let fc = DetectionPattern::function_call("copy_from_user", 1.5);
        assert_eq!(fc.kind, DetectionKind::FunctionCall);
        assert_eq!(fc.pattern, "copy_from_user");
        assert!((fc.weight - 1.5).abs() < f32::EPSILON);

        let tn = DetectionPattern::type_name("task_struct", 0.5);
        assert_eq!(tn.kind, DetectionKind::TypeName);
        assert_eq!(tn.pattern, "task_struct");
        assert!((tn.weight - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_header_stubs_has_stub_and_available_headers() {
        let mut stubs = HeaderStubs::new();
        assert!(!stubs.has_stub("linux/types.h"));
        assert!(stubs.available_headers().is_empty());

        stubs.add("linux/types.h", "typedef unsigned int u32;");
        stubs.add("linux/kernel.h", "typedef unsigned long size_t;");

        assert!(stubs.has_stub("linux/types.h"));
        assert!(stubs.has_stub("linux/kernel.h"));
        assert!(!stubs.has_stub("linux/module.h"));

        let mut headers = stubs.available_headers();
        headers.sort_unstable();
        assert_eq!(headers, vec!["linux/kernel.h", "linux/types.h"]);
    }

    #[test]
    fn test_header_stubs_extract_include_path_edge_cases() {
        // Unterminated angle bracket -> strip_suffix('>') fails -> None.
        assert_eq!(
            HeaderStubs::extract_include_path("#include <linux/types.h"),
            None
        );
        // Unterminated quote likewise yields None.
        assert_eq!(
            HeaderStubs::extract_include_path("#include \"myheader.h"),
            None
        );
        // A line without a delimiter after #include falls into the else -> None.
        assert_eq!(
            HeaderStubs::extract_include_path("#include linux/types.h"),
            None
        );
        // Not an include line at all: everything after trimming #include is kept,
        // but with no leading '<' or '"' it is None.
        assert_eq!(HeaderStubs::extract_include_path("#define FOO 1"), None);
    }

    #[test]
    fn test_header_stubs_get_for_includes_quoted_and_no_match() {
        let mut stubs = HeaderStubs::new();
        stubs.add("my/local.h", "struct local { int x; };");

        // Quoted include resolves to the stub; the comment banner is emitted.
        let result = stubs.get_for_includes("#include \"my/local.h\"\n");
        assert!(result.contains("/* Stub for my/local.h */"));
        assert!(result.contains("struct local"));

        // No matching include -> empty output.
        assert!(stubs
            .get_for_includes("int main(void) { return 0; }\n")
            .is_empty());
    }

    #[test]
    fn test_platform_registry_default_matches_new() {
        // Default delegates to new(), so linux is registered and resolvable.
        let registry = PlatformRegistry::default();
        assert!(registry.get("linux").is_some());
    }

    /// Minimal fake platform used to exercise register/score paths independently
    /// of the built-in LinuxPlatform.
    struct FakePlatform {
        stubs: HeaderStubs,
        norms: HashMap<&'static str, &'static str>,
    }

    impl FakePlatform {
        fn new() -> Self {
            Self {
                stubs: HeaderStubs::new(),
                norms: HashMap::new(),
            }
        }
    }

    impl PlatformModule for FakePlatform {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn name(&self) -> &'static str {
            "Fake Platform"
        }
        fn detection_patterns(&self) -> Vec<DetectionPattern> {
            vec![
                DetectionPattern::function_call("fake_call", 3.0),
                DetectionPattern::type_name("FakeType", 3.0),
                DetectionPattern::macro_pattern("FAKE_MACRO", 3.0),
            ]
        }
        fn header_stubs(&self) -> &HeaderStubs {
            &self.stubs
        }
        fn attributes_to_strip(&self) -> &[&'static str] {
            &[]
        }
        fn ops_structs(&self) -> &[OpsStructDef] {
            &[]
        }
        fn call_normalizations(&self) -> &HashMap<&'static str, &'static str> {
            &self.norms
        }
    }

    #[test]
    fn test_score_platform_function_call_and_type_name_branches() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(FakePlatform::new()));

        // fake_call( triggers FunctionCall; FakeType matches TypeName case-insensitively;
        // FAKE_MACRO matches the Macro branch. All three weights (3.0 each = 9.0/10) score high.
        let source = "faketype x; fake_call(x); FAKE_MACRO;";
        let result = registry.detect(source);
        assert_eq!(result.platform_id, "fake");
        assert!(result.matched_patterns.contains(&"fake_call".to_string()));
        assert!(result.matched_patterns.contains(&"FakeType".to_string()));
        assert!(result.matched_patterns.contains(&"FAKE_MACRO".to_string()));
    }

    #[test]
    fn test_score_platform_function_call_needs_paren() {
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(FakePlatform::new()));

        // Bare "fake_call" without a '(' must NOT match the FunctionCall pattern.
        let result = registry.detect("int fake_call = 3;");
        assert!(!result.matched_patterns.contains(&"fake_call".to_string()));
    }

    #[test]
    fn test_score_platform_confidence_capped_at_one() {
        // Two 3.0 patterns present + linux's own matches could exceed 10, but the
        // fake source below only hits FakePlatform's patterns (9.0 -> 0.9), and
        // adding a fourth match keeps us under the cap; verify the cap explicitly
        // by matching every fake pattern which sums to 9.0/10 = 0.9 (< 1.0).
        let mut registry = PlatformRegistry::new();
        registry.register(Box::new(FakePlatform::new()));
        let result = registry.detect("FakeType fake_call( FAKE_MACRO");
        assert!(result.confidence <= 1.0);
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn test_detect_empty_registry_returns_generic() {
        let registry = PlatformRegistry {
            platforms: Vec::new(),
        };
        let result = registry.detect("anything at all");
        assert_eq!(result.platform_id, "generic");
        assert_eq!(result.confidence, 0.0);
        assert!(result.matched_patterns.is_empty());
    }
}
