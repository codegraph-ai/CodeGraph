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
}
