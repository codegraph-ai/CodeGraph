// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! C Preprocessor simulation layer
//!
//! This module provides a lightweight preprocessing layer that handles common
//! C macros without requiring actual header files. Unlike a real preprocessor,
//! this doesn't expand macros textually but instead helps tree-sitter parse
//! code that uses common macro patterns.
//!
//! Key strategies:
//! 1. Macro recognition: Identify and annotate common macro patterns
//! 2. Attribute simulation: Convert __attribute__ and similar to parseable form
//! 3. Kernel macro handling: Special support for Linux kernel macros

use std::collections::HashMap;

/// Known macro patterns and their semantic meaning
#[derive(Debug, Clone, PartialEq)]
pub enum MacroKind {
    /// Type-like macro (expands to a type): u8, u16, size_t, etc.
    TypeAlias,
    /// Attribute macro: __init, __exit, __user, __packed, etc.
    Attribute,
    /// Function-like macro that wraps a function definition
    FunctionWrapper,
    /// Module/export macro: MODULE_LICENSE, EXPORT_SYMBOL, etc.
    ModuleDeclaration,
    /// Locking primitive: DEFINE_MUTEX, spin_lock, etc.
    LockingPrimitive,
    /// Memory allocation: kmalloc, kfree, etc.
    MemoryOperation,
    /// Conditional compilation marker
    ConditionalMarker,
    /// Generic macro call
    Generic,
}

/// Information about a recognized macro
#[derive(Debug, Clone)]
pub struct MacroInfo {
    pub name: String,
    pub kind: MacroKind,
    pub expansion_hint: Option<String>,
}

/// C Preprocessor simulation for better parsing
pub struct CPreprocessor {
    /// Known type-like macros (expand to types)
    type_macros: HashMap<String, String>,
    /// Known attribute macros (can be stripped)
    attribute_macros: Vec<String>,
    /// Known function wrapper macros
    function_wrappers: HashMap<String, String>,
    /// Known module declaration macros
    module_macros: Vec<String>,
}

impl Default for CPreprocessor {
    fn default() -> Self {
        Self::new()
    }
}

impl CPreprocessor {
    pub fn new() -> Self {
        let mut preprocessor = Self {
            type_macros: HashMap::new(),
            attribute_macros: Vec::new(),
            function_wrappers: HashMap::new(),
            module_macros: Vec::new(),
        };
        preprocessor.init_kernel_macros();
        preprocessor.init_standard_macros();
        preprocessor
    }

    /// Initialize Linux kernel-specific macros
    fn init_kernel_macros(&mut self) {
        // Integer types
        for (macro_name, expansion) in [
            ("u8", "unsigned char"),
            ("u16", "unsigned short"),
            ("u32", "unsigned int"),
            ("u64", "unsigned long long"),
            ("s8", "signed char"),
            ("s16", "signed short"),
            ("s32", "signed int"),
            ("s64", "signed long long"),
            ("__u8", "unsigned char"),
            ("__u16", "unsigned short"),
            ("__u32", "unsigned int"),
            ("__u64", "unsigned long long"),
            ("__s8", "signed char"),
            ("__s16", "signed short"),
            ("__s32", "signed int"),
            ("__s64", "signed long long"),
            ("__le16", "unsigned short"),
            ("__le32", "unsigned int"),
            ("__le64", "unsigned long long"),
            ("__be16", "unsigned short"),
            ("__be32", "unsigned int"),
            ("__be64", "unsigned long long"),
            ("bool", "_Bool"),
            ("atomic_t", "int"),
            ("atomic64_t", "long long"),
            ("spinlock_t", "int"),
            ("rwlock_t", "int"),
            ("mutex", "int"),
            ("size_t", "unsigned long"),
            ("ssize_t", "long"),
            ("ptrdiff_t", "long"),
            ("uintptr_t", "unsigned long"),
            ("intptr_t", "long"),
            ("phys_addr_t", "unsigned long long"),
            ("dma_addr_t", "unsigned long long"),
            ("resource_size_t", "unsigned long long"),
            ("gfp_t", "unsigned int"),
            ("fmode_t", "unsigned int"),
            ("umode_t", "unsigned short"),
            ("dev_t", "unsigned int"),
            ("loff_t", "long long"),
            ("pid_t", "int"),
            ("uid_t", "unsigned int"),
            ("gid_t", "unsigned int"),
            ("ktime_t", "long long"),
        ] {
            self.type_macros
                .insert(macro_name.to_string(), expansion.to_string());
        }

        // Attribute macros (can be stripped for parsing)
        self.attribute_macros.extend(
            [
                // Section/init attributes
                "__init",
                "__exit",
                "__initdata",
                "__exitdata",
                "__initconst",
                "__devinit",
                "__devexit",
                // Compiler hints
                "__cold",
                "__hot",
                "__pure",
                "__const",
                "__noreturn",
                "__malloc",
                "__weak",
                "__alias",
                "__always_inline",
                "__noinline",
                "noinline",
                "inline",
                "__inline",
                "__inline__",
                "__section",
                "__visible",
                "__flatten",
                // Address space annotations
                "__user",
                "__kernel",
                "__iomem",
                "__percpu",
                "__rcu",
                "__force",
                "__bitwise",
                "__safe",
                // Unused/maybe annotations (common error source)
                "__maybe_unused",
                "__always_unused",
                "__unused",
                // Packing and alignment
                "__packed",
                "__aligned",
                "__cacheline_aligned",
                "__cacheline_aligned_in_smp",
                "__page_aligned_data",
                "__page_aligned_bss",
                // Deprecation
                "__deprecated",
                "__deprecated_for_modules",
                // Locking annotations
                "__must_check",
                "__must_hold",
                "__acquires",
                "__releases",
                "__acquire",
                "__release",
                "__cond_lock",
                // Memory placement
                "__read_mostly",
                "__ro_after_init",
                // Calling conventions
                "asmlinkage",
                "fastcall",
                "regparm",
                // Export symbols
                "EXPORT_SYMBOL",
                "EXPORT_SYMBOL_GPL",
                "EXPORT_SYMBOL_NS",
                "EXPORT_SYMBOL_NS_GPL",
                // Branch prediction
                "likely",
                "unlikely",
                // Memory access
                "ACCESS_ONCE",
                "READ_ONCE",
                "WRITE_ONCE",
                // Checksum types
                "__wsum",
                "__sum16",
                "__be16",
                "__be32",
                "__be64",
                "__le16",
                "__le32",
                "__le64",
            ]
            .iter()
            .map(|s| s.to_string()),
        );

        // Function wrapper macros
        for (wrapper, ret_type) in [
            ("SYSCALL_DEFINE0", "long"),
            ("SYSCALL_DEFINE1", "long"),
            ("SYSCALL_DEFINE2", "long"),
            ("SYSCALL_DEFINE3", "long"),
            ("SYSCALL_DEFINE4", "long"),
            ("SYSCALL_DEFINE5", "long"),
            ("SYSCALL_DEFINE6", "long"),
            ("COMPAT_SYSCALL_DEFINE0", "long"),
            ("COMPAT_SYSCALL_DEFINE1", "long"),
            ("COMPAT_SYSCALL_DEFINE2", "long"),
            ("COMPAT_SYSCALL_DEFINE3", "long"),
            ("COMPAT_SYSCALL_DEFINE4", "long"),
            ("COMPAT_SYSCALL_DEFINE5", "long"),
            ("COMPAT_SYSCALL_DEFINE6", "long"),
            ("__setup", "int"),
            ("early_param", "int"),
            ("core_param", "int"),
            ("module_param", "void"),
            ("module_param_named", "void"),
            ("DEFINE_PER_CPU", "void"),
            ("DECLARE_PER_CPU", "void"),
        ] {
            self.function_wrappers
                .insert(wrapper.to_string(), ret_type.to_string());
        }

        // Module declaration macros
        self.module_macros.extend(
            [
                "MODULE_LICENSE",
                "MODULE_AUTHOR",
                "MODULE_DESCRIPTION",
                "MODULE_VERSION",
                "MODULE_ALIAS",
                "MODULE_DEVICE_TABLE",
                "MODULE_FIRMWARE",
                "MODULE_INFO",
                "MODULE_PARM_DESC",
                "module_init",
                "module_exit",
                "late_initcall",
                "subsys_initcall",
                "fs_initcall",
                "device_initcall",
                "arch_initcall",
                "core_initcall",
                "postcore_initcall",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
    }

    /// Initialize standard C macros
    fn init_standard_macros(&mut self) {
        // Standard C constants
        for (macro_name, expansion) in [
            ("NULL", "((void*)0)"),
            ("EOF", "(-1)"),
            ("true", "1"),
            ("false", "0"),
            ("TRUE", "1"),
            ("FALSE", "0"),
        ] {
            self.type_macros
                .insert(macro_name.to_string(), expansion.to_string());
        }

        // C99 stdint.h fixed-width types — tree-sitter doesn't process
        // #include <stdint.h> so these must be injected as typedefs
        for (macro_name, expansion) in [
            ("uint8_t", "unsigned char"),
            ("uint16_t", "unsigned short"),
            ("uint32_t", "unsigned int"),
            ("uint64_t", "unsigned long long"),
            ("int8_t", "signed char"),
            ("int16_t", "signed short"),
            ("int32_t", "signed int"),
            ("int64_t", "signed long long"),
        ] {
            self.type_macros
                .insert(macro_name.to_string(), expansion.to_string());
        }

        // VMware ESX / VMKernel types — these appear as return types and parameter
        // types in ESX driver code and must be recognized as types by tree-sitter
        for (macro_name, expansion) in [
            // VMK return status and basic types
            ("VMK_ReturnStatus", "int"),
            ("vmk_Bool", "_Bool"),
            ("vmk_ByteCount", "unsigned long"),
            ("vmk_ByteCountSmall", "unsigned int"),
            // VMK integer types
            ("vmk_uint8", "unsigned char"),
            ("vmk_uint16", "unsigned short"),
            ("vmk_uint32", "unsigned int"),
            ("vmk_uint64", "unsigned long long"),
            ("vmk_int8", "signed char"),
            ("vmk_int16", "signed short"),
            ("vmk_int32", "signed int"),
            ("vmk_int64", "signed long long"),
            // VMK atomic types
            ("vmk_atomic8", "unsigned char"),
            ("vmk_atomic16", "unsigned short"),
            ("vmk_atomic32", "unsigned int"),
            ("vmk_atomic64", "unsigned long long"),
            // VMK handle/cookie types
            ("vmk_AddrCookie", "void*"),
            ("vmk_HeapID", "void*"),
            ("vmk_IntrCookie", "void*"),
            ("vmk_Lock", "void*"),
            ("vmk_Mutex", "void*"),
            ("vmk_Semaphore", "void*"),
            ("vmk_SpinlockIRQ", "void*"),
            ("vmk_LockDomainID", "void*"),
            ("vmk_ModuleID", "void*"),
            ("vmk_TimerCookie", "void*"),
            ("vmk_WorldID", "unsigned long"),
            // VMK device/driver types
            ("vmk_Device", "void*"),
            ("vmk_Driver", "void*"),
            ("vmk_DMAEngine", "void*"),
            ("vmk_DMADirection", "int"),
            ("vmk_IOA", "void*"),
            // VMK network types
            ("vmk_EthAddress", "unsigned char[6]"),
            ("vmk_PktHandle", "void*"),
            ("vmk_Uplink", "void*"),
            ("vmk_UplinkSharedData", "void*"),
            ("vmk_UplinkSharedQueueData", "void*"),
            ("vmk_VlanID", "unsigned short"),
            ("vmk_SwitchPortID", "void*"),
            // VMK list/vector types
            ("vmk_ListLinks", "void*"),
            ("vmk_BitVector", "void*"),
            // VMK misc types
            ("vmk_Name", "char[32]"),
            ("vmk_VA", "unsigned long"),
            ("vmk_MA", "unsigned long long"),
            ("vmk_LogComponent", "void*"),
            ("vmk_Helper", "void*"),
            ("vmk_HelperRequestFunc", "void*"),
        ] {
            self.type_macros
                .insert(macro_name.to_string(), expansion.to_string());
        }
    }

    /// Check if an identifier is a known type macro
    pub fn is_type_macro(&self, name: &str) -> bool {
        self.type_macros.contains_key(name)
    }

    /// Get the expansion hint for a type macro
    pub fn get_type_expansion(&self, name: &str) -> Option<&str> {
        self.type_macros.get(name).map(|s| s.as_str())
    }

    /// Check if an identifier is a known attribute macro
    pub fn is_attribute_macro(&self, name: &str) -> bool {
        self.attribute_macros.contains(&name.to_string())
    }

    /// Check if an identifier is a function wrapper macro
    pub fn is_function_wrapper(&self, name: &str) -> bool {
        self.function_wrappers.contains_key(name)
    }

    /// Check if an identifier is a module declaration macro
    pub fn is_module_macro(&self, name: &str) -> bool {
        self.module_macros.contains(&name.to_string())
    }

    /// Classify a macro by name
    pub fn classify_macro(&self, name: &str) -> MacroKind {
        if self.is_type_macro(name) {
            MacroKind::TypeAlias
        } else if self.is_attribute_macro(name) {
            MacroKind::Attribute
        } else if self.is_function_wrapper(name) {
            MacroKind::FunctionWrapper
        } else if self.is_module_macro(name) {
            MacroKind::ModuleDeclaration
        } else if name.starts_with("DEFINE_")
            || name.starts_with("DECLARE_")
            || name.contains("_LOCK")
            || name.contains("_MUTEX")
        {
            MacroKind::LockingPrimitive
        } else if name.contains("alloc")
            || name.contains("free")
            || name.starts_with("k")
                && (name.contains("alloc") || name.contains("free") || name.contains("zalloc"))
        {
            MacroKind::MemoryOperation
        } else if name.starts_with("CONFIG_")
            || name.starts_with("IS_ENABLED")
            || name.starts_with("IS_BUILTIN")
            || name.starts_with("IS_MODULE")
        {
            MacroKind::ConditionalMarker
        } else {
            MacroKind::Generic
        }
    }

    /// Add type definitions extracted from a header file.
    /// Call this before `preprocess()` to make header types available.
    pub fn add_type(&mut self, name: &str, expansion: &str) {
        self.type_macros
            .insert(name.to_string(), expansion.to_string());
    }

    /// Known C primitive types that are safe to use as typedef expansions.
    const SAFE_PRIMITIVES: &'static [&'static str] = &[
        "void",
        "char",
        "short",
        "int",
        "long",
        "float",
        "double",
        "unsigned",
        "signed",
        "unsigned char",
        "unsigned short",
        "unsigned int",
        "unsigned long",
        "unsigned long long",
        "signed char",
        "signed short",
        "signed int",
        "signed long",
        "signed long long",
        "long long",
        "long double",
        "_Bool",
    ];

    /// Scan a header file's source and extract safe type definitions.
    ///
    /// Only extracts typedefs that resolve to primitive C types or `struct`/`enum`
    /// forward declarations. This avoids injecting types that reference other
    /// unresolved types (which would cause more parse errors, not fewer).
    pub fn extract_header_types(header_source: &str) -> Vec<(String, String)> {
        let mut types = Vec::new();

        for line in header_source.lines() {
            let trimmed = line.trim();

            // typedef <primitive> <name>;
            // e.g., typedef unsigned int uint32_t;
            // Skip function pointers, complex types, struct bodies
            if let Some(rest) = trimmed.strip_prefix("typedef ") {
                if let Some(semi_pos) = rest.rfind(';') {
                    let decl = rest[..semi_pos].trim();
                    // Skip function pointers and complex types
                    if decl.contains("(*") || decl.contains('{') {
                        continue;
                    }
                    if let Some(name) = decl.split_whitespace().next_back() {
                        let name = name.trim_start_matches('*');
                        if name.is_empty()
                            || !name.starts_with(|c: char| c.is_alphabetic() || c == '_')
                        {
                            continue;
                        }
                        let expansion = decl[..decl.len() - name.len()].trim();
                        if expansion.is_empty() {
                            continue;
                        }
                        // Only accept typedefs that expand to known primitives
                        // or "struct X" / "enum X" patterns
                        let is_safe = Self::SAFE_PRIMITIVES
                            .iter()
                            .any(|p| expansion == *p || expansion.starts_with(&format!("{p} ")))
                            || expansion.starts_with("struct ")
                            || expansion.starts_with("enum ")
                            || expansion.starts_with("union ");
                        if is_safe {
                            types.push((name.to_string(), expansion.to_string()));
                        }
                    }
                }
            }
        }

        types
    }

    /// Preprocess source code to make it more parseable
    ///
    /// This performs lightweight transformations:
    /// - Strips problematic attributes
    /// - Normalizes some macro patterns
    pub fn preprocess(&self, source: &str) -> String {
        let mut result = String::with_capacity(source.len() + 2048);

        // Inject typedef preamble for any VMK/ESX types found in the source so
        // tree-sitter recognises them as type specifiers (function return types,
        // parameter types, local variable types).
        self.inject_type_preamble(source, &mut result);

        for line in source.lines() {
            let processed = self.process_line(line);
            result.push_str(&processed);
            result.push('\n');
        }

        result
    }

    /// Scan `source` for known type-macro identifiers and emit `typedef` lines
    /// at the top of `out` so tree-sitter can treat them as type specifiers.
    fn inject_type_preamble(&self, source: &str, out: &mut String) {
        for (name, expansion) in &self.type_macros {
            // Skip non-type expansions (constants like NULL → ((void*)0), true → 1)
            if !Self::is_type_expansion(expansion) {
                continue;
            }

            // Only inject if the identifier actually appears in the source.
            if !source.contains(name.as_str()) {
                continue;
            }

            // Pointer expansions: strip trailing `*` for proper typedef syntax
            if let Some(base) = expansion.strip_suffix('*') {
                out.push_str(&format!("typedef {base} *{name};\n"));
            } else {
                out.push_str(&format!("typedef {expansion} {name};\n"));
            }
        }
    }

    /// Check if an expansion string is a valid C type (not a constant or complex expression)
    fn is_type_expansion(expansion: &str) -> bool {
        // Reject expressions with parentheses: ((void*)0), (-1), etc.
        // Reject array types: unsigned char[6], char[32]
        // Reject pure numeric constants: 1, 0
        !expansion.contains('(')
            && !expansion.contains('[')
            && expansion
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
    }

    /// Process a single line
    fn process_line(&self, line: &str) -> String {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("//") {
            return line.to_string();
        }

        // Handle #include - keep as-is (tree-sitter handles these)
        if trimmed.starts_with("#include") {
            return line.to_string();
        }

        // Strip all preprocessor directives except #include (kept for import tracking).
        // #if/#ifdef/#else/#endif inside struct initializers break tree-sitter parsing
        // and prevent vtable/ops struct detection. Stripping to comments preserves line
        // numbers while letting tree-sitter see both branches as valid C code.
        if trimmed.starts_with('#') && !trimmed.starts_with("#include") {
            return "/* preprocessor directive stripped */".to_string();
        }

        // Strip known attribute macros that confuse the parser
        let mut result = line.to_string();
        for attr in &self.attribute_macros {
            // Handle both plain attributes and function-like attributes
            // e.g., __init, __section(".text")
            let patterns = [format!("{attr} "), format!("{attr}\t"), format!("{attr}(")];

            for pattern in &patterns {
                if result.contains(pattern.as_str()) {
                    // For function-like attributes, need to handle parentheses
                    if pattern.ends_with('(') {
                        // Find matching closing paren and remove the whole thing
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
                            }
                        }
                    } else {
                        result = result.replace(pattern, "");
                    }
                }
            }
        }

        // Handle offsetof(type, member) - common source of errors
        // Replace with 0 (a constant that tree-sitter can parse)
        while let Some(start) = result.find("offsetof(") {
            let rest = &result[start + 9..]; // after "offsetof("
            let mut depth = 1;
            let mut end_paren = 0;

            for (i, c) in rest.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end_paren = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if end_paren > 0 {
                result = format!(
                    "{}0{}",
                    &result[..start],
                    &result[start + 9 + end_paren + 1..]
                );
            } else {
                break;
            }
        }

        // Handle container_of(ptr, type, member) - 6.4% of errors
        // Replace with a simpler cast expression: ((type *)ptr)
        while let Some(start) = result.find("container_of(") {
            let rest = &result[start + 13..]; // after "container_of("
            let mut depth = 1;
            let mut end_paren = 0;
            let mut first_comma = None;

            for (i, c) in rest.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end_paren = i;
                            break;
                        }
                    }
                    ',' if depth == 1 && first_comma.is_none() => {
                        first_comma = Some(i);
                    }
                    _ => {}
                }
            }

            if end_paren > 0 {
                // Extract ptr (first argument)
                let ptr = if let Some(comma_pos) = first_comma {
                    rest[..comma_pos].trim()
                } else {
                    "ptr"
                };
                // Replace with (void*)ptr - simple cast that tree-sitter can parse
                let replacement = format!("((void*){ptr})");
                result = format!(
                    "{}{}{}",
                    &result[..start],
                    replacement,
                    &result[start + 13 + end_paren + 1..]
                );
            } else {
                break;
            }
        }

        // Handle __attribute__((...)) - complex case
        while let Some(start) = result.find("__attribute__") {
            if let Some(paren_start) = result[start..].find("((") {
                let abs_start = start + paren_start;
                let mut depth = 2; // Starting after "(("
                let mut end = abs_start + 2;
                for (i, c) in result[abs_start + 2..].char_indices() {
                    match c {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                end = abs_start + 2 + i + 1;
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
        }

        result
    }

    /// Get information about all recognized macros in source
    pub fn analyze_macros(&self, source: &str) -> Vec<MacroInfo> {
        let mut macros = Vec::new();

        // Simple token extraction for macro detection
        for word in source.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if word.is_empty() {
                continue;
            }

            let kind = self.classify_macro(word);
            if kind != MacroKind::Generic {
                macros.push(MacroInfo {
                    name: word.to_string(),
                    kind: kind.clone(),
                    expansion_hint: self.get_type_expansion(word).map(|s| s.to_string()),
                });
            }
        }

        // Deduplicate
        macros.sort_by(|a, b| a.name.cmp(&b.name));
        macros.dedup_by(|a, b| a.name == b.name);

        macros
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_macro_recognition() {
        let pp = CPreprocessor::new();
        assert!(pp.is_type_macro("u8"));
        assert!(pp.is_type_macro("u32"));
        assert!(pp.is_type_macro("size_t"));
        assert!(!pp.is_type_macro("unknown_type"));
    }

    #[test]
    fn test_attribute_macro_recognition() {
        let pp = CPreprocessor::new();
        assert!(pp.is_attribute_macro("__init"));
        assert!(pp.is_attribute_macro("__exit"));
        assert!(pp.is_attribute_macro("__user"));
        assert!(!pp.is_attribute_macro("regular_function"));
    }

    #[test]
    fn test_macro_classification() {
        let pp = CPreprocessor::new();
        assert_eq!(pp.classify_macro("u32"), MacroKind::TypeAlias);
        assert_eq!(pp.classify_macro("__init"), MacroKind::Attribute);
        assert_eq!(
            pp.classify_macro("MODULE_LICENSE"),
            MacroKind::ModuleDeclaration
        );
        assert_eq!(
            pp.classify_macro("DEFINE_MUTEX"),
            MacroKind::LockingPrimitive
        );
        assert_eq!(
            pp.classify_macro("CONFIG_DEBUG"),
            MacroKind::ConditionalMarker
        );
    }

    #[test]
    fn test_preprocess_strips_attributes() {
        let pp = CPreprocessor::new();
        let source = "static __init int my_init(void) { return 0; }";
        let processed = pp.preprocess(source);
        assert!(!processed.contains("__init"));
        assert!(processed.contains("static"));
        assert!(processed.contains("int my_init"));
    }

    #[test]
    fn test_preprocess_handles_attribute_syntax() {
        let pp = CPreprocessor::new();
        let source = "void __attribute__((packed)) my_struct;";
        let processed = pp.preprocess(source);
        assert!(!processed.contains("__attribute__"));
        assert!(processed.contains("void"));
    }

    #[test]
    fn test_analyze_macros() {
        let pp = CPreprocessor::new();
        let source = "u32 foo; __init static int bar(size_t n) { return 0; }";
        let macros = pp.analyze_macros(source);

        let names: Vec<_> = macros.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"u32"));
        assert!(names.contains(&"__init"));
        assert!(names.contains(&"size_t"));
    }

    #[test]
    fn test_preprocess_preserves_includes() {
        let pp = CPreprocessor::new();
        let source = "#include <linux/module.h>\n#include \"myheader.h\"";
        let processed = pp.preprocess(source);
        assert!(processed.contains("#include <linux/module.h>"));
        assert!(processed.contains("#include \"myheader.h\""));
    }
}
