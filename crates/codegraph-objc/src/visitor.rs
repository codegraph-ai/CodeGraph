// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Objective-C entities
//!
//! Grammar shape (from debug_dump_ast):
//! - `preproc_include` / `preproc_import` — #import / #include
//!   - `system_lib_string` or `string_literal` child for the path
//! - `class_interface` — @interface Foo : Bar ... @end
//!   - children: `@interface`, `identifier` (name), `:`, `identifier` (superclass),
//!     `method_declaration`* (directly, no wrapping list), `@end`
//! - `class_implementation` — @implementation Foo ... @end
//!   - children: `@implementation`, `identifier` (name),
//!     `implementation_definition`* -> `method_definition` -> `compound_statement`
//! - `protocol_declaration` — @protocol Foo ... @end
//!   - children: `@protocol`, `identifier` (name), `method_declaration`*, `@end`
//! - `method_declaration` — `-/+` `method_type` `identifier` `;`
//! - `method_definition`  — `-/+` `method_type` `identifier` `compound_statement`

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, Parameter, TraitEntity,
};
use tree_sitter::Node;

pub(crate) struct ObjcVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub traits: Vec<TraitEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_class: Option<String>,
    current_function: Option<String>,
}

impl<'a> ObjcVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            traits: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_class: None,
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "class_interface" => {
                self.visit_class_interface(node);
                return;
            }
            "class_implementation" => {
                self.visit_class_implementation(node);
                return;
            }
            "protocol_declaration" => {
                self.visit_protocol(node);
                return;
            }
            "preproc_include" | "preproc_import" => {
                self.visit_import(node);
                // Don't recurse into preprocessor directives
                return;
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_class_interface(&mut self, node: Node) {
        let name = self.first_identifier(node);
        if name.is_empty() {
            return;
        }

        let superclass = self.find_superclass(node);
        let base_classes = superclass.into_iter().collect();

        let class = ClassEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes,
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment: None,
            attributes: Vec::new(),
            type_parameters: Vec::new(),
            body_prefix: None,
        };

        self.classes.push(class);

        let previous_class = self.current_class.take();
        self.current_class = Some(name);

        // Visit method_declaration children directly under class_interface
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "method_declaration" {
                self.visit_method_decl(child);
            }
        }

        self.current_class = previous_class;
    }

    fn visit_class_implementation(&mut self, node: Node) {
        let name = self.first_identifier(node);
        if name.is_empty() {
            return;
        }

        let previous_class = self.current_class.take();
        self.current_class = Some(name);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "implementation_definition" {
                // implementation_definition -> method_definition
                let mut c2 = child.walk();
                for grandchild in child.children(&mut c2) {
                    if grandchild.kind() == "method_definition" {
                        self.visit_method_def(grandchild);
                    }
                }
            }
        }

        self.current_class = previous_class;
    }

    fn visit_protocol(&mut self, node: Node) {
        let name = self.first_identifier(node);
        if name.is_empty() {
            return;
        }

        let trait_entity = TraitEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            required_methods: Vec::new(),
            parent_traits: Vec::new(),
            doc_comment: None,
            attributes: Vec::new(),
        };

        self.traits.push(trait_entity);

        let previous_class = self.current_class.take();
        self.current_class = Some(name);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "method_declaration" {
                self.visit_method_decl(child);
            }
        }

        self.current_class = previous_class;
    }

    /// Handle a `method_declaration` (interface/protocol — no body)
    fn visit_method_decl(&mut self, node: Node) {
        let is_class_method = self.is_class_method_node(node);
        let name = self.extract_method_name_from_node(node);
        if name.is_empty() {
            return;
        }

        let signature = self
            .node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .trim_end_matches(';')
            .to_string();
        let parameters = self.extract_method_parameters_from_node(node);
        let parent_class = self.current_class.clone();

        let func = FunctionEntity {
            name,
            signature,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: is_class_method,
            is_abstract: true,
            parameters,
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class,
            complexity: None,
            body_prefix: None,
        };

        self.functions.push(func);
    }

    /// Handle a `method_definition` (implementation — has body)
    fn visit_method_def(&mut self, node: Node) {
        let is_class_method = self.is_class_method_node(node);
        let name = self.extract_method_name_from_node(node);
        if name.is_empty() {
            return;
        }

        let signature = self
            .node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        let parameters = self.extract_method_parameters_from_node(node);
        let parent_class = self.current_class.clone();

        // Find the compound_statement body
        let mut body_cursor = node.walk();
        let body_node = {
            let x = node
                .children(&mut body_cursor)
                .find(|c| c.kind() == "compound_statement");
            x
        };

        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|body| self.calculate_complexity(body));

        let func_name = name.clone();

        let func = FunctionEntity {
            name,
            signature,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: is_class_method,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class,
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        // Track calls inside the body
        let previous_function = self.current_function.take();
        self.current_function = Some(func_name);

        if let Some(body) = body_node {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    fn visit_import(&mut self, node: Node) {
        // Look for system_lib_string or string_literal child
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "system_lib_string" => {
                    // e.g. <Foundation/Foundation.h>
                    let raw = self.node_text(child);
                    let clean = raw.trim_matches('<').trim_matches('>');
                    if !clean.is_empty() {
                        self.imports.push(ImportRelation {
                            importer: "main".to_string(),
                            imported: clean.to_string(),
                            symbols: Vec::new(),
                            is_wildcard: false,
                            alias: None,
                        });
                    }
                    return;
                }
                "string_literal" => {
                    // e.g. "MyHelper.h"
                    let raw = self.node_text(child);
                    let clean = raw.trim_matches('"');
                    if !clean.is_empty() {
                        self.imports.push(ImportRelation {
                            importer: "main".to_string(),
                            imported: clean.to_string(),
                            symbols: Vec::new(),
                            is_wildcard: false,
                            alias: None,
                        });
                    }
                    return;
                }
                _ => {}
            }
        }
        // Fallback: parse the text directly
        let text = self.node_text(node);
        let after = text
            .trim_start_matches("#import")
            .trim_start_matches("#include")
            .trim();
        let module = if after.starts_with('<') {
            after.trim_start_matches('<').trim_end_matches('>').trim()
        } else if after.starts_with('"') {
            after.trim_matches('"').trim()
        } else {
            return;
        };
        if !module.is_empty() {
            self.imports.push(ImportRelation {
                importer: "main".to_string(),
                imported: module.to_string(),
                symbols: Vec::new(),
                is_wildcard: false,
                alias: None,
            });
        }
    }

    /// Return the first `identifier` or `type_identifier` child of a node.
    fn first_identifier(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" | "type_identifier" => {
                    let text = self.node_text(child);
                    if !text.is_empty() {
                        return text;
                    }
                }
                _ => {}
            }
        }
        String::new()
    }

    /// Find the superclass: second identifier after the `:` child.
    fn find_superclass(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        let mut after_colon = false;
        let mut first_id_seen = false;
        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" | "type_identifier" => {
                    if !first_id_seen {
                        first_id_seen = true; // This is the class name
                        continue;
                    }
                    if after_colon {
                        let text = self.node_text(child);
                        if !text.is_empty() {
                            return Some(text);
                        }
                    }
                }
                ":" => {
                    after_colon = true;
                }
                _ => {}
            }
        }
        None
    }

    /// Determine if method node represents a class method (`+` prefix).
    fn is_class_method_node(&self, node: Node) -> bool {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "+" {
                return true;
            }
            // Stop at the first relevant token
            if matches!(child.kind(), "-" | "+" | "method_type" | "identifier") {
                break;
            }
        }
        false
    }

    /// Extract method name from `method_declaration` or `method_definition`.
    /// The grammar has: `-/+` `method_type` `identifier` for simple methods,
    /// and may have `keyword_selector` for multi-part selectors.
    fn extract_method_name_from_node(&self, node: Node) -> String {
        // Collect all `identifier` children that come after the method_type
        let mut past_method_type = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "method_type" => {
                    past_method_type = true;
                }
                "identifier" if past_method_type => {
                    let text = self.node_text(child);
                    if !text.is_empty() {
                        return text;
                    }
                }
                "compound_statement" | ";" => {
                    break;
                }
                _ => {}
            }
        }
        String::new()
    }

    fn extract_method_parameters_from_node(&self, node: Node) -> Vec<Parameter> {
        // For now, parameters in simple ObjC methods are after the selector name.
        // Keyword selectors look like: - (void)foo:(NSString *)arg1 bar:(int)arg2
        // The grammar represents these as `keyword_selector` nodes.
        // In the simple case tested (single identifier), there are no parameters.
        // This can be extended for multi-keyword methods.
        let _ = node;
        Vec::new()
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // ObjC function calls appear as call_expression nodes (C-style: NSLog(...))
            if child.kind() == "call_expression" {
                self.visit_call_expression(child);
            }
            self.visit_body_for_calls(child);
        }
    }

    fn visit_call_expression(&mut self, node: Node) {
        if let Some(ref caller) = self.current_function.clone() {
            // The `function` field child (or first identifier) is the callee name
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    let callee = self.node_text(child);
                    if !callee.is_empty() {
                        self.calls.push(CallRelation {
                            caller: caller.clone(),
                            callee,
                            call_site_line: node.start_position().row + 1,
                            is_direct: true,
                            struct_type: None,
                            field_name: None,
                        });
                    }
                    break;
                }
            }
        }
    }

    fn calculate_complexity(&self, body: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(body, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "if_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "else_clause" => {
                builder.add_branch();
            }
            "for_statement" | "while_statement" | "do_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "switch_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "binary_expression" => {
                let text = self.node_text(node);
                if text.contains("&&") || text.contains("||") {
                    builder.add_logical_operator();
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        match node.kind() {
            "if_statement" | "for_statement" | "while_statement" | "do_statement"
            | "switch_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph_parser_api::BODY_PREFIX_MAX_CHARS;
    use tree_sitter::Parser;

    fn parse_and_visit(source: &[u8]) -> ObjcVisitor<'_> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_objc::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = ObjcVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_class_extraction() {
        let source = br#"
@interface MyClass : NSObject
- (void)greet;
@end

@implementation MyClass
- (void)greet {
    NSLog(@"Hello");
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "MyClass");
    }

    #[test]
    fn test_visitor_method_extraction() {
        let source = br#"
@implementation MyClass
- (void)greet {
    NSLog(@"Hello");
}
+ (instancetype)sharedInstance {
    return nil;
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 2);
        let names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"greet"));
        assert!(names.contains(&"sharedInstance"));
    }

    #[test]
    fn test_visitor_protocol_extraction() {
        let source = br#"
@protocol MyProtocol
- (void)doSomething;
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "MyProtocol");
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = br#"
#import <Foundation/Foundation.h>
#import "MyHelper.h"
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 2);
    }

    #[test]
    fn test_visitor_superclass() {
        let source = br#"
@interface MyClass : NSObject
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].base_classes, vec!["NSObject"]);
    }

    #[test]
    fn test_no_superclass_empty_base_classes() {
        // A category-less interface with no `: Super` still parses; base_classes empty.
        let source = br#"
@interface Standalone
- (void)ping;
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        assert_eq!(visitor.classes[0].name, "Standalone");
        assert!(visitor.classes[0].base_classes.is_empty());
    }

    #[test]
    fn test_class_metadata() {
        let source = br#"
@interface MyClass : NSObject
@end
"#;
        let visitor = parse_and_visit(source);
        let class = &visitor.classes[0];
        assert_eq!(class.visibility, "public");
        assert!(!class.is_abstract);
        assert!(!class.is_interface);
        // @interface starts on line 2 (leading newline), single line span.
        assert_eq!(class.line_start, 2);
        assert_eq!(class.line_end, 3);
    }

    #[test]
    fn test_instance_method_not_static() {
        let source = br#"
@implementation MyClass
- (void)greet {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        let greet = visitor
            .functions
            .iter()
            .find(|f| f.name == "greet")
            .unwrap();
        assert!(!greet.is_static);
    }

    #[test]
    fn test_class_method_is_static() {
        let source = br#"
@implementation MyClass
+ (instancetype)sharedInstance {
    return nil;
}
@end
"#;
        let visitor = parse_and_visit(source);
        let shared = visitor
            .functions
            .iter()
            .find(|f| f.name == "sharedInstance")
            .unwrap();
        assert!(shared.is_static);
    }

    #[test]
    fn test_interface_method_is_abstract() {
        // A `method_declaration` (no body) under @interface is abstract with no complexity/body.
        let source = br#"
@interface MyClass : NSObject
- (void)greet;
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        let greet = &visitor.functions[0];
        assert_eq!(greet.name, "greet");
        assert!(greet.is_abstract);
        assert!(greet.complexity.is_none());
        assert!(greet.body_prefix.is_none());
        // Signature is the declaration line with the trailing `;` trimmed.
        assert_eq!(greet.signature, "- (void)greet");
    }

    #[test]
    fn test_implementation_method_not_abstract() {
        let source = br#"
@implementation MyClass
- (void)greet {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        let greet = &visitor.functions[0];
        assert!(!greet.is_abstract);
        assert!(greet.complexity.is_some());
        assert!(greet.body_prefix.is_some());
    }

    #[test]
    fn test_method_parent_class() {
        let source = br#"
@implementation MyClass
- (void)greet {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.functions[0].parent_class,
            Some("MyClass".to_string())
        );
    }

    #[test]
    fn test_protocol_method_parent_class() {
        let source = br#"
@protocol MyProtocol
- (void)doSomething;
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        let m = &visitor.functions[0];
        assert_eq!(m.name, "doSomething");
        assert!(m.is_abstract);
        assert_eq!(m.parent_class, Some("MyProtocol".to_string()));
    }

    #[test]
    fn test_method_complexity_branching() {
        let source = br#"
@implementation MyClass
- (void)decide:(int)x {
    if (x > 0) {
        return;
    }
}
@end
"#;
        let visitor = parse_and_visit(source);
        let decide = visitor
            .functions
            .iter()
            .find(|f| f.name == "decide")
            .unwrap();
        let complexity = decide.complexity.as_ref().unwrap();
        assert!(complexity.cyclomatic_complexity >= 2);
    }

    #[test]
    fn test_call_extraction() {
        let source = br#"
@implementation MyClass
- (void)greet {
    NSLog(@"Hello");
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert!(!visitor.calls.is_empty());
        let call = &visitor.calls[0];
        assert_eq!(call.caller, "greet");
        assert_eq!(call.callee, "NSLog");
        assert!(call.is_direct);
    }

    #[test]
    fn test_system_import_cleaned() {
        let source = br#"
#import <Foundation/Foundation.h>
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.imported, "Foundation/Foundation.h");
        assert_eq!(imp.importer, "main");
        assert!(!imp.is_wildcard);
        assert!(imp.alias.is_none());
    }

    #[test]
    fn test_quoted_import_cleaned() {
        let source = br#"
#import "MyHelper.h"
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "MyHelper.h");
    }

    #[test]
    fn test_multiple_classes() {
        let source = br#"
@interface Alpha : NSObject
@end

@interface Beta : NSObject
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 2);
        let names: Vec<&str> = visitor.classes.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Alpha"));
        assert!(names.contains(&"Beta"));
    }

    #[test]
    fn test_empty_source() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.classes.is_empty());
        assert!(visitor.functions.is_empty());
        assert!(visitor.traits.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_method_line_numbers_one_indexed() {
        // The method_definition starts on physical line 3 (leading newline = line 1).
        let source = br#"
@implementation MyClass
- (void)greet {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        let greet = &visitor.functions[0];
        assert_eq!(greet.line_start, 3);
        assert_eq!(greet.line_end, 5);
    }

    #[test]
    fn test_method_def_signature_first_line_only() {
        // Signature keeps only the first physical line of the definition (with the `{`).
        let source = br#"
@implementation MyClass
- (void)greet {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].signature, "- (void)greet {");
    }

    #[test]
    fn test_method_def_body_prefix_content() {
        let source = br#"
@implementation MyClass
- (void)greet {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        let body = visitor.functions[0].body_prefix.as_ref().unwrap();
        assert!(body.contains("return"));
    }

    #[test]
    fn test_method_default_flags() {
        // is_async/is_test are always false for ObjC methods.
        let source = br#"
@implementation MyClass
- (void)greet {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        let greet = &visitor.functions[0];
        assert!(!greet.is_async);
        assert!(!greet.is_test);
        assert!(greet.return_type.is_none());
        assert!(greet.doc_comment.is_none());
    }

    #[test]
    fn test_method_complexity_loop() {
        let source = br#"
@implementation MyClass
- (void)loop:(int)n {
    for (int i = 0; i < n; i++) {
        NSLog(@"%d", i);
    }
}
@end
"#;
        let visitor = parse_and_visit(source);
        let m = visitor.functions.iter().find(|f| f.name == "loop").unwrap();
        assert!(m.complexity.as_ref().unwrap().cyclomatic_complexity >= 2);
    }

    #[test]
    fn test_method_complexity_switch() {
        let source = br#"
@implementation MyClass
- (void)choose:(int)x {
    switch (x) {
        case 1:
            break;
        default:
            break;
    }
}
@end
"#;
        let visitor = parse_and_visit(source);
        let m = visitor
            .functions
            .iter()
            .find(|f| f.name == "choose")
            .unwrap();
        assert!(m.complexity.as_ref().unwrap().cyclomatic_complexity >= 2);
    }

    #[test]
    fn test_method_complexity_logical_operator() {
        let source = br#"
@implementation MyClass
- (void)both:(BOOL)a with:(BOOL)b {
    if (a && b) {
        return;
    }
}
@end
"#;
        let visitor = parse_and_visit(source);
        let m = visitor.functions.iter().find(|f| f.name == "both").unwrap();
        // if branch (+1) plus the && logical operator (+1) over the base of 1.
        assert!(m.complexity.as_ref().unwrap().cyclomatic_complexity >= 3);
    }

    #[test]
    fn test_multiple_imports_order() {
        let source = br#"
#import <Foundation/Foundation.h>
#import "MyHelper.h"
#import <UIKit/UIKit.h>
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 3);
        assert_eq!(visitor.imports[0].imported, "Foundation/Foundation.h");
        assert_eq!(visitor.imports[1].imported, "MyHelper.h");
        assert_eq!(visitor.imports[2].imported, "UIKit/UIKit.h");
    }

    #[test]
    fn test_protocol_trait_metadata() {
        let source = br#"
@protocol MyProtocol
- (void)doSomething;
@end
"#;
        let visitor = parse_and_visit(source);
        let t = &visitor.traits[0];
        assert_eq!(t.visibility, "public");
        assert_eq!(t.line_start, 2);
        assert!(t.parent_traits.is_empty());
        assert!(t.doc_comment.is_none());
    }

    #[test]
    fn test_call_site_line_recorded() {
        let source = br#"
@implementation MyClass
- (void)greet {
    NSLog(@"Hello");
}
@end
"#;
        let visitor = parse_and_visit(source);
        // NSLog(...) sits on physical line 4.
        assert_eq!(visitor.calls[0].call_site_line, 4);
    }

    #[test]
    fn test_interface_multiple_methods_all_abstract() {
        let source = br#"
@interface MyClass : NSObject
- (void)one;
- (void)two;
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 2);
        assert!(visitor.functions.iter().all(|f| f.is_abstract));
        let names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"one"));
        assert!(names.contains(&"two"));
    }

    #[test]
    fn test_method_body_prefix_truncated() {
        // A body longer than BODY_PREFIX_MAX_CHARS is truncated to exactly that many bytes.
        let filler = "    x = x + 1;\n".repeat(200);
        let source = format!(
            "@implementation MyClass\n- (void)big {{\n{}}}\n@end\n",
            filler
        );
        let visitor = parse_and_visit(source.as_bytes());
        let big = visitor.functions.iter().find(|f| f.name == "big").unwrap();
        let body = big.body_prefix.as_ref().unwrap();
        assert_eq!(body.len(), BODY_PREFIX_MAX_CHARS);
    }

    #[test]
    fn test_method_complexity_baseline_one() {
        // A straight-line method with no branches keeps cyclomatic complexity 1.
        let source = br#"
@implementation MyClass
- (void)plain {
    int y = 1;
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        let m = visitor
            .functions
            .iter()
            .find(|f| f.name == "plain")
            .unwrap();
        assert_eq!(m.complexity.as_ref().unwrap().cyclomatic_complexity, 1);
    }

    #[test]
    fn test_method_complexity_else_branch() {
        // An if/else raises complexity above a lone if (else_clause adds a branch).
        let source = br#"
@implementation MyClass
- (void)pick:(int)x {
    if (x > 0) {
        return;
    } else {
        return;
    }
}
@end
"#;
        let visitor = parse_and_visit(source);
        let m = visitor.functions.iter().find(|f| f.name == "pick").unwrap();
        assert!(m.complexity.as_ref().unwrap().cyclomatic_complexity >= 3);
    }

    #[test]
    fn test_method_complexity_while() {
        let source = br#"
@implementation MyClass
- (void)spin:(int)n {
    while (n > 0) {
        n--;
    }
}
@end
"#;
        let visitor = parse_and_visit(source);
        let m = visitor.functions.iter().find(|f| f.name == "spin").unwrap();
        assert!(m.complexity.as_ref().unwrap().cyclomatic_complexity >= 2);
    }

    #[test]
    fn test_method_complexity_do_while() {
        let source = br#"
@implementation MyClass
- (void)repeat:(int)n {
    do {
        n--;
    } while (n > 0);
}
@end
"#;
        let visitor = parse_and_visit(source);
        let m = visitor
            .functions
            .iter()
            .find(|f| f.name == "repeat")
            .unwrap();
        assert!(m.complexity.as_ref().unwrap().cyclomatic_complexity >= 2);
    }

    #[test]
    fn test_call_default_metadata() {
        let source = br#"
@implementation MyClass
- (void)greet {
    NSLog(@"Hello");
}
@end
"#;
        let visitor = parse_and_visit(source);
        let call = &visitor.calls[0];
        assert_eq!(call.caller, "greet");
        assert_eq!(call.callee, "NSLog");
        assert!(call.is_direct);
        assert!(call.struct_type.is_none());
        assert!(call.field_name.is_none());
    }

    #[test]
    fn test_multiple_calls_in_body() {
        let source = br#"
@implementation MyClass
- (void)greet {
    NSLog(@"one");
    NSLog(@"two");
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.calls.len(), 2);
        assert!(visitor.calls.iter().all(|c| c.caller == "greet"));
    }

    #[test]
    fn test_nested_call_attributed_to_method() {
        // A call inside an if block is still attributed to the enclosing method.
        let source = br#"
@implementation MyClass
- (void)guarded:(int)x {
    if (x > 0) {
        NSLog(@"positive");
    }
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "guarded");
        assert_eq!(visitor.calls[0].callee, "NSLog");
    }

    #[test]
    fn test_class_method_in_interface_abstract_and_static() {
        // A `+` class-method declaration under @interface is abstract and static.
        let source = br#"
@interface MyClass : NSObject
+ (instancetype)make;
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        let make = &visitor.functions[0];
        assert_eq!(make.name, "make");
        assert!(make.is_abstract);
        assert!(make.is_static);
    }

    #[test]
    fn test_two_method_defs_source_order_lines() {
        let source = br#"
@implementation MyClass
- (void)first {
    return;
}
- (void)second {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        let first = visitor
            .functions
            .iter()
            .find(|f| f.name == "first")
            .unwrap();
        let second = visitor
            .functions
            .iter()
            .find(|f| f.name == "second")
            .unwrap();
        assert!(second.line_start > first.line_end);
    }

    #[test]
    fn test_protocol_multiple_methods_parented() {
        let source = br#"
@protocol MyProtocol
- (void)alpha;
- (void)beta;
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 2);
        assert!(visitor
            .functions
            .iter()
            .all(|f| f.parent_class == Some("MyProtocol".to_string()) && f.is_abstract));
    }

    #[test]
    fn test_implementation_without_interface_extracts_methods() {
        // A bare @implementation (no matching @interface) still extracts its methods.
        let source = br#"
@implementation Orphan
- (void)work {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        let work = &visitor.functions[0];
        assert_eq!(work.name, "work");
        assert_eq!(work.parent_class, Some("Orphan".to_string()));
        assert!(!work.is_abstract);
    }

    #[test]
    fn test_category_interface_extracted_as_class() {
        // `@interface Foo (Private)` parses as a class_interface; the class name is Foo
        // and the `(Private)` category identifier is ignored (not a superclass).
        let source = br#"
@interface Foo (Private)
- (void)ping;
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        let class = &visitor.classes[0];
        assert_eq!(class.name, "Foo");
        // The trailing `Private` identifier appears with no `:`, so it is not a superclass.
        assert!(class.base_classes.is_empty());
        // The category's method is still extracted and parented to Foo.
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "ping");
        assert_eq!(visitor.functions[0].parent_class, Some("Foo".to_string()));
    }

    #[test]
    fn test_protocol_inheritance_parent_traits_empty_gap() {
        // `@protocol Sub <Base>` records Sub but never reads the protocol_reference_list,
        // so parent_traits stays empty - a latent gap pinned here.
        let source = br#"
@protocol Sub <Base>
- (void)go;
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.traits.len(), 1);
        assert_eq!(visitor.traits[0].name, "Sub");
        assert!(visitor.traits[0].parent_traits.is_empty());
    }

    #[test]
    fn test_class_protocol_conformance_implemented_traits_empty_gap() {
        // `@interface Foo : NSObject <Bar>` keeps NSObject as the superclass but the
        // `<Bar>` conformance (a parameterized_arguments node) is never read, so
        // implemented_traits stays empty - a latent gap pinned here.
        let source = br#"
@interface Foo : NSObject <Bar>
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.classes.len(), 1);
        let class = &visitor.classes[0];
        assert_eq!(class.base_classes, vec!["NSObject"]);
        assert!(class.implemented_traits.is_empty());
    }

    #[test]
    fn test_keyword_selector_name_first_part_only_gap() {
        // A multi-part keyword selector `setX:y:` yields only the first segment `setX`
        // because extract_method_name_from_node returns the first identifier after the
        // method_type - the full selector name is truncated (a latent gap).
        let source = br#"
@implementation C
- (void)setX:(int)x y:(int)y {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "setX");
    }

    #[test]
    fn test_keyword_selector_parameters_empty_gap() {
        // extract_method_parameters_from_node always returns an empty vec, so even a
        // keyword selector with two typed parameters records no parameters (a latent gap).
        let source = br#"
@implementation C
- (void)setX:(int)x y:(int)y {
    return;
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions[0].parameters.is_empty());
    }

    #[test]
    fn test_message_expression_not_tracked_as_call_gap() {
        // An ObjC message send `[self ping]` parses as a message_expression, but
        // visit_body_for_calls only tracks C-style call_expression nodes, so no call
        // is recorded - a latent gap pinned here.
        let source = br#"
@implementation C
- (void)go {
    [self ping];
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_ternary_no_complexity_gap() {
        // A ternary `x > 0 ? 1 : 2` parses as a conditional_expression, which the
        // complexity visitor does not count, so complexity stays at the baseline 1.
        let source = br#"
@implementation C
- (int)pick:(int)x {
    return x > 0 ? 1 : 2;
}
@end
"#;
        let visitor = parse_and_visit(source);
        let m = visitor.functions.iter().find(|f| f.name == "pick").unwrap();
        assert_eq!(m.complexity.as_ref().unwrap().cyclomatic_complexity, 1);
    }

    #[test]
    fn test_for_in_enumeration_complexity_and_call() {
        // Fast enumeration `for (id x in a)` parses as a for_statement, so it raises
        // complexity as a loop, and a C-style call in its body is still attributed.
        let source = br#"
@implementation C
- (void)each:(NSArray *)a {
    for (id x in a) {
        NSLog(@"%@", x);
    }
}
@end
"#;
        let visitor = parse_and_visit(source);
        let m = visitor.functions.iter().find(|f| f.name == "each").unwrap();
        assert!(m.complexity.as_ref().unwrap().cyclomatic_complexity >= 2);
        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "each");
        assert_eq!(visitor.calls[0].callee, "NSLog");
    }

    #[test]
    fn test_multiple_protocols_extracted() {
        let source = br#"
@protocol Alpha
- (void)a;
@end

@protocol Beta
- (void)b;
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.traits.len(), 2);
        let names: Vec<&str> = visitor.traits.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"Alpha"));
        assert!(names.contains(&"Beta"));
    }

    #[test]
    fn test_class_method_definition_static_with_body() {
        // A `+` method with a body under @implementation is static, non-abstract, and
        // carries complexity/body_prefix (unlike an interface class-method declaration).
        let source = br#"
@implementation C
+ (instancetype)make {
    return nil;
}
@end
"#;
        let visitor = parse_and_visit(source);
        let make = visitor.functions.iter().find(|f| f.name == "make").unwrap();
        assert!(make.is_static);
        assert!(!make.is_abstract);
        assert!(make.complexity.is_some());
        assert!(make.body_prefix.is_some());
    }

    #[test]
    fn test_call_in_switch_case_attributed() {
        // A C-style call inside a switch case body is attributed to the enclosing method.
        let source = br#"
@implementation C
- (void)route:(int)x {
    switch (x) {
        case 1:
            NSLog(@"one");
            break;
        default:
            break;
    }
}
@end
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.calls.len(), 1);
        assert_eq!(visitor.calls[0].caller, "route");
        assert_eq!(visitor.calls[0].callee, "NSLog");
    }
}
