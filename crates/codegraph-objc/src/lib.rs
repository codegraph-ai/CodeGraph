// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Objective-C parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::ObjcParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = ObjcParser::new();
        assert_eq!(parser.language(), "objc");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = ObjcParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
#import <Foundation/Foundation.h>

@interface MyClass : NSObject
@property (nonatomic, strong) NSString *name;
- (void)greet;
@end

@implementation MyClass
- (void)greet {
    NSLog(@"Hello, %@", self.name);
}
@end
"#;

        let result = parser.parse_source(source, Path::new("MyClass.m"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert!(!file_info.classes.is_empty() || !file_info.functions.is_empty());
    }
}
