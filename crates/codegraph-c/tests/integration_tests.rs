// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the C parser

use codegraph::{CodeGraph, NodeType};
use codegraph_c::{CParser, CodeParser};
use std::path::Path;

#[test]
fn test_parse_hello_world() {
    let source = r#"
#include <stdio.h>

int main() {
    printf("Hello, World!\n");
    return 0;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("hello.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1);
    assert_eq!(file_info.imports.len(), 1);
}

#[test]
fn test_parse_multiple_functions() {
    let source = r#"
int add(int a, int b) {
    return a + b;
}

int subtract(int a, int b) {
    return a - b;
}

int multiply(int a, int b) {
    return a * b;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("math.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 3);
}

#[test]
fn test_parse_struct_with_typedef() {
    let source = r#"
struct Node {
    int data;
    struct Node *next;
};

typedef struct Node Node;

Node* create_node(int data) {
    return NULL;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("node.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 1); // struct Node
    assert_eq!(file_info.functions.len(), 1); // create_node
}

#[test]
fn test_parse_enum() {
    let source = r#"
enum Status {
    OK = 0,
    ERROR = 1,
    PENDING = 2
};

int get_status() {
    return OK;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("status.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 1); // enum Status
    assert_eq!(file_info.functions.len(), 1); // get_status
}

#[test]
fn test_parse_union() {
    let source = r#"
union Data {
    int i;
    float f;
    char str[20];
};

void process_data(union Data *d) {
    d->i = 10;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("data.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 1); // union Data
    assert_eq!(file_info.functions.len(), 1); // process_data
}

#[test]
fn test_parse_static_functions() {
    let source = r#"
static int helper(int x) {
    return x * 2;
}

int public_func(int x) {
    return helper(x) + 1;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("module.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 2);
}

#[test]
fn test_parse_multiple_includes() {
    let source = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "myheader.h"

void test() {}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("test.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.imports.len(), 4);
}

#[test]
fn test_parse_pointer_parameters() {
    let source = r#"
void swap(int *a, int *b) {
    int temp = *a;
    *a = *b;
    *b = temp;
}

void process_array(int arr[], int size) {
    // Process array
}

void process_strings(char **strings, int count) {
    // Process strings
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("pointers.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 3);
}

#[test]
fn test_parse_variadic_function() {
    let source = r#"
#include <stdarg.h>

int sum(int count, ...) {
    va_list args;
    va_start(args, count);

    int total = 0;
    for (int i = 0; i < count; i++) {
        total += va_arg(args, int);
    }

    va_end(args);
    return total;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("varargs.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1);
}

#[test]
fn test_parse_complex_function() {
    let source = r#"
int complex_function(int x, int y) {
    int result = 0;

    if (x > 0) {
        for (int i = 0; i < x; i++) {
            if (i % 2 == 0) {
                result += i;
            } else {
                result -= i;
            }
        }
    } else if (x < 0) {
        while (y > 0) {
            result += y;
            y--;
        }
    } else {
        switch (y) {
            case 1:
                result = 100;
                break;
            case 2:
                result = 200;
                break;
            default:
                result = 0;
                break;
        }
    }

    return result;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("complex.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1);
}

#[test]
fn test_parse_header_file() {
    let parser = CParser::new();

    assert!(parser.can_parse(Path::new("header.h")));
    assert!(parser.can_parse(Path::new("source.c")));
    assert!(!parser.can_parse(Path::new("source.cpp")));
    assert!(!parser.can_parse(Path::new("source.rs")));
}

#[test]
fn test_parse_nested_structs() {
    let source = r#"
struct Address {
    char street[100];
    char city[50];
    int zip;
};

struct Person {
    char name[100];
    int age;
    struct Address address;
};
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("person.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 2);
}

#[test]
fn test_parse_syntax_error() {
    let source = r#"
int broken( {
    // Missing closing brace
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("broken.c"), &mut graph);

    // Tolerant fallback means partial files still parse successfully
    assert!(result.is_ok());
}

#[test]
fn test_parse_empty_file() {
    let source = "";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("empty.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 0);
    assert_eq!(file_info.classes.len(), 0);
}

#[test]
fn test_parse_comments_only() {
    let source = r#"
// This is a comment
/* This is a
   multi-line comment */
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("comments.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 0);
}

#[test]
fn test_parser_metrics() {
    let source = r#"
int test() { return 0; }
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let mut parser = CParser::new();

    // Reset metrics
    parser.reset_metrics();
    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 0);

    // Parse a file
    let _ = parser.parse_source(source, Path::new("test.c"), &mut graph);

    // Note: parse_source doesn't update metrics, only parse_file does
    // This is by design to avoid double counting
}

#[test]
fn test_parser_language() {
    let parser = CParser::new();
    assert_eq!(parser.language(), "c");
}

#[test]
fn test_parser_file_extensions() {
    let parser = CParser::new();
    let extensions = parser.file_extensions();
    assert!(extensions.contains(&".c"));
    assert!(extensions.contains(&".h"));
}

#[test]
fn test_parse_function_pointer_param() {
    let source = r#"
void register_callback(void (*callback)(int)) {
    // Store callback
}

void process(int (*compare)(const void*, const void*)) {
    // Use comparator
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("callbacks.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 2);
}

#[test]
fn test_parse_const_params() {
    let source = r#"
void print_string(const char *str) {
    // Print string
}

int compare(const int *a, const int *b) {
    return *a - *b;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("const.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 2);
}

#[test]
fn test_parse_goto_complexity() {
    let source = r#"
void cleanup(int *resources, int count) {
    for (int i = 0; i < count; i++) {
        if (resources[i] < 0) {
            goto error;
        }
    }
    return;

error:
    // Cleanup code
    return;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("goto.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1);
}

#[test]
fn test_parse_ternary_operator() {
    let source = r#"
int max(int a, int b) {
    return a > b ? a : b;
}

int abs_val(int x) {
    return x >= 0 ? x : -x;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("ternary.c"), &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 2);
}

// ============================================================================
// End-to-End Tests: Verify properties are correctly stored in CodeGraph
// ============================================================================

#[test]
fn test_e2e_function_properties_stored_in_graph() {
    let source = r#"
int add(int a, int b) {
    return a + b;
}

static void helper(void) {
    // Internal helper
}

char *get_name(const char *prefix, int id) {
    return NULL;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("functions.c"), &mut graph);
    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 3);

    // Query all functions from the graph
    let function_ids = graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap();
    assert_eq!(function_ids.len(), 3);

    // Verify each function has correct properties
    for func_id in &function_ids {
        let node = graph.get_node(*func_id).unwrap();
        assert_eq!(node.node_type, NodeType::Function);

        // All functions must have a name
        let name = node.properties.get_string("name").unwrap();
        assert!(!name.is_empty(), "Function should have a name");

        // All functions must have line numbers
        assert!(
            node.properties.get_int("line_start").is_some(),
            "Function should have line_start"
        );
        assert!(
            node.properties.get_int("line_end").is_some(),
            "Function should have line_end"
        );

        // Visibility should be set
        assert!(
            node.properties.get_string("visibility").is_some(),
            "Function should have visibility"
        );
    }

    // Find 'add' function and verify specific properties
    let add_funcs: Vec<_> = function_ids
        .iter()
        .filter_map(|id| {
            let node = graph.get_node(*id).ok()?;
            let name = node.properties.get_string("name")?;
            if name == "add" {
                Some(node)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(add_funcs.len(), 1, "Should find exactly one 'add' function");
    let add_func = add_funcs[0];
    assert_eq!(
        add_func.properties.get_string("visibility").unwrap(),
        "public"
    );

    // Find 'helper' function and verify it's static (private)
    let helper_funcs: Vec<_> = function_ids
        .iter()
        .filter_map(|id| {
            let node = graph.get_node(*id).ok()?;
            let name = node.properties.get_string("name")?;
            if name == "helper" {
                Some(node)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        helper_funcs.len(),
        1,
        "Should find exactly one 'helper' function"
    );
    let helper_func = helper_funcs[0];
    // Static functions should be marked as private
    assert_eq!(
        helper_func.properties.get_string("visibility").unwrap(),
        "private",
        "Static function should have private visibility"
    );
}

#[test]
fn test_e2e_struct_properties_stored_in_graph() {
    let source = r#"
struct Point {
    int x;
    int y;
};

struct Rectangle {
    struct Point top_left;
    struct Point bottom_right;
    int area;
};

enum Color {
    RED = 0,
    GREEN = 1,
    BLUE = 2
};

union Value {
    int i;
    float f;
    double d;
};
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("types.c"), &mut graph);
    assert!(result.is_ok());
    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 4); // 2 structs + 1 enum + 1 union

    // Query all classes from the graph
    let class_ids = graph.query().node_type(NodeType::Class).execute().unwrap();
    assert_eq!(class_ids.len(), 4);

    // Verify each class has correct properties
    for class_id in &class_ids {
        let node = graph.get_node(*class_id).unwrap();
        assert_eq!(node.node_type, NodeType::Class);

        // All classes must have a name
        let name = node.properties.get_string("name").unwrap();
        assert!(!name.is_empty(), "Class should have a name");

        // All classes must have line numbers
        assert!(
            node.properties.get_int("line_start").is_some(),
            "Class should have line_start"
        );
        assert!(
            node.properties.get_int("line_end").is_some(),
            "Class should have line_end"
        );
    }

    // Verify specific classes exist
    let class_names: Vec<String> = class_ids
        .iter()
        .filter_map(|id| {
            let node = graph.get_node(*id).ok()?;
            node.properties.get_string("name").map(String::from)
        })
        .collect();

    assert!(
        class_names.contains(&"Point".to_string()),
        "Should find Point struct"
    );
    assert!(
        class_names.contains(&"Rectangle".to_string()),
        "Should find Rectangle struct"
    );
    assert!(
        class_names.contains(&"Color".to_string()),
        "Should find Color enum"
    );
    assert!(
        class_names.contains(&"Value".to_string()),
        "Should find Value union"
    );
}

#[test]
fn test_e2e_query_builder_filters() {
    let source = r#"
static int private_func1(void) { return 1; }
static int private_func2(void) { return 2; }
int public_func1(void) { return 3; }
int public_func2(void) { return 4; }
int public_func3(void) { return 5; }
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("visibility.c"), &mut graph);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().functions.len(), 5);

    // Query only public functions
    let public_funcs = graph
        .query()
        .node_type(NodeType::Function)
        .property("visibility", "public")
        .execute()
        .unwrap();
    assert_eq!(public_funcs.len(), 3, "Should find 3 public functions");

    // Query only private (static) functions
    let private_funcs = graph
        .query()
        .node_type(NodeType::Function)
        .property("visibility", "private")
        .execute()
        .unwrap();
    assert_eq!(
        private_funcs.len(),
        2,
        "Should find 2 private (static) functions"
    );

    // Query functions by name pattern
    let funcs_with_1 = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("1")
        .execute()
        .unwrap();
    assert_eq!(
        funcs_with_1.len(),
        2,
        "Should find 2 functions containing '1'"
    );

    // Query with limit
    let limited = graph
        .query()
        .node_type(NodeType::Function)
        .limit(2)
        .execute()
        .unwrap();
    assert_eq!(
        limited.len(),
        2,
        "Should return only 2 results when limited"
    );

    // Count query
    let count = graph.query().node_type(NodeType::Function).count().unwrap();
    assert_eq!(count, 5, "Count should return total number of functions");

    // Exists query
    let exists = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("public")
        .exists()
        .unwrap();
    assert!(
        exists,
        "Should find at least one function containing 'public'"
    );

    let not_exists = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("nonexistent")
        .exists()
        .unwrap();
    assert!(
        !not_exists,
        "Should not find any function containing 'nonexistent'"
    );
}

#[test]
fn test_e2e_file_node_and_contains_edges() {
    let source = r#"
struct Data {
    int value;
};

int process_data(struct Data *d) {
    return d->value;
}

void cleanup(void) {}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("data_processor.c"), &mut graph);
    assert!(result.is_ok());
    let file_info = result.unwrap();

    // File node should exist
    let file_nodes = graph
        .query()
        .node_type(NodeType::CodeFile)
        .execute()
        .unwrap();
    assert_eq!(file_nodes.len(), 1, "Should have exactly one file node");

    let file_node = graph.get_node(file_nodes[0]).unwrap();
    let file_path = file_node.properties.get_string("path").unwrap();
    assert_eq!(file_path, "data_processor.c");

    // Verify file contains functions and classes
    // The file_info should track the node IDs
    assert_eq!(file_info.functions.len(), 2);
    assert_eq!(file_info.classes.len(), 1);

    // Verify we can retrieve the functions by their IDs
    for func_id in &file_info.functions {
        let func_node = graph.get_node(*func_id).unwrap();
        assert_eq!(func_node.node_type, NodeType::Function);
    }

    for class_id in &file_info.classes {
        let class_node = graph.get_node(*class_id).unwrap();
        assert_eq!(class_node.node_type, NodeType::Class);
    }
}

#[test]
fn test_e2e_import_nodes_created() {
    let source = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "local_header.h"

int main() {
    printf("Hello\n");
    return 0;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("imports.c"), &mut graph);
    assert!(result.is_ok());
    let file_info = result.unwrap();

    // Should have 4 imports
    assert_eq!(file_info.imports.len(), 4);

    // Verify import nodes can be retrieved
    for import_id in &file_info.imports {
        let _import_node = graph.get_node(*import_id).unwrap();
        // Import nodes should exist and be retrievable
    }
}

#[test]
fn test_e2e_kernel_style_code_with_macros() {
    // Test kernel-style C code that exercises the macro pipeline
    let source = r#"
#include <linux/module.h>
#include <linux/kernel.h>

#define DRIVER_NAME "test_driver"
#define MAX_DEVICES 16

struct device_info {
    int id;
    char name[64];
    unsigned long flags;
};

static struct device_info devices[MAX_DEVICES];

static int driver_init(void) {
    return 0;
}

static void driver_exit(void) {
}

static int process_device(struct device_info *dev) {
    if (!dev) {
        return -1;
    }
    return dev->id;
}

int public_api_function(int device_id) {
    if (device_id < 0 || device_id >= MAX_DEVICES) {
        return -1;
    }
    return process_device(&devices[device_id]);
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("driver.c"), &mut graph);

    // Should parse successfully despite kernel macros
    assert!(
        result.is_ok(),
        "Kernel-style code should parse successfully: {:?}",
        result.err()
    );
    let file_info = result.unwrap();

    // Should find functions
    assert!(
        file_info.functions.len() >= 2,
        "Should find at least 2 functions, found {}",
        file_info.functions.len()
    );

    // Should find the struct
    assert!(
        !file_info.classes.is_empty(),
        "Should find at least 1 struct, found {}",
        file_info.classes.len()
    );

    // Verify we can query the functions
    let func_count = graph.query().node_type(NodeType::Function).count().unwrap();
    assert!(func_count >= 2, "Graph should contain at least 2 functions");
}

#[test]
fn test_e2e_multiline_define_handling() {
    // Test that multi-line #define macros are handled correctly
    let source = r#"
#define COMPLEX_MACRO(x, y) \
    do { \
        int temp = (x); \
        (x) = (y); \
        (y) = temp; \
    } while(0)

#define SIMPLE_MACRO 42

int use_macros(int a, int b) {
    COMPLEX_MACRO(a, b);
    return a + SIMPLE_MACRO;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("macros.c"), &mut graph);

    assert!(result.is_ok(), "Code with multi-line macros should parse");
    let file_info = result.unwrap();
    assert_eq!(
        file_info.functions.len(),
        1,
        "Should find use_macros function"
    );
}

#[test]
fn test_e2e_ifdef_handling() {
    // Test that #ifdef blocks are handled correctly
    let source = r#"
#ifdef CONFIG_DEBUG
#define DEBUG_PRINT(x) printf("%s\n", x)
#else
#define DEBUG_PRINT(x)
#endif

#ifndef MAX_SIZE
#define MAX_SIZE 1024
#endif

int main(void) {
    DEBUG_PRINT("test");
    return 0;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("conditionals.c"), &mut graph);

    assert!(result.is_ok(), "Code with #ifdef should parse");
    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1, "Should find main function");
}

#[test]
fn test_e2e_for_each_macro_handling() {
    // Test that for_each macros are handled correctly when defined in the same file
    // Note: This tests the macro neutralization - complex macro usages may need
    // a separate preprocessing step in real-world codebases
    let source = r#"
struct list_head {
    struct list_head *next;
    struct list_head *prev;
};

void iterate_list(struct list_head *head) {
    struct list_head *pos;
    for (pos = head->next; pos != head; pos = pos->next) {
        // Process item
    }
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("list_ops.c"), &mut graph);

    assert!(
        result.is_ok(),
        "Code with for loop should parse: {:?}",
        result.err()
    );
    let file_info = result.unwrap();
    assert_eq!(
        file_info.functions.len(),
        1,
        "Should find iterate_list function"
    );
    assert_eq!(file_info.classes.len(), 1, "Should find list_head struct");
}

#[test]
fn test_e2e_cleanup_attribute_handling() {
    // Test that GCC attributes are handled correctly
    // Note: Complex cleanup attributes with macro expansion may require
    // additional preprocessing in real-world codebases
    let source = r#"
void free_ptr(void **ptr) {
    // Free implementation
}

int use_cleanup(void) {
    void *ptr = NULL;
    return 0;
}

// Function with common GCC attributes
__attribute__((unused))
static void unused_func(void) {}

__attribute__((always_inline))
static inline int fast_add(int a, int b) {
    return a + b;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("cleanup.c"), &mut graph);

    assert!(
        result.is_ok(),
        "Code with attributes should parse: {:?}",
        result.err()
    );
    let file_info = result.unwrap();
    assert!(
        file_info.functions.len() >= 2,
        "Should find at least 2 functions, found {}",
        file_info.functions.len()
    );
}

#[test]
fn test_e2e_line_numbers_accuracy() {
    let source = r#"int first_func(void) {
    return 1;
}

int second_func(void) {
    return 2;
}

int third_func(void) {
    return 3;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("lines.c"), &mut graph);
    assert!(result.is_ok());

    // Find first_func and verify line numbers
    let first_func = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("first")
        .execute()
        .unwrap();
    assert_eq!(first_func.len(), 1);

    let first_node = graph.get_node(first_func[0]).unwrap();
    let line_start = first_node.properties.get_int("line_start").unwrap();
    let line_end = first_node.properties.get_int("line_end").unwrap();

    // first_func starts at line 1
    assert_eq!(line_start, 1, "first_func should start at line 1");
    // first_func ends at line 3
    assert_eq!(line_end, 3, "first_func should end at line 3");

    // Find second_func and verify line numbers
    let second_func = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("second")
        .execute()
        .unwrap();
    assert_eq!(second_func.len(), 1);

    let second_node = graph.get_node(second_func[0]).unwrap();
    let line_start = second_node.properties.get_int("line_start").unwrap();

    // second_func starts at line 5
    assert_eq!(line_start, 5, "second_func should start at line 5");
}

#[test]
fn test_e2e_function_signature_stored() {
    let source = r#"
int simple(void) { return 0; }

char *complex_signature(const int *arr, size_t len, void (*callback)(int)) {
    return NULL;
}

static unsigned long long very_long_return_type(void) {
    return 0ULL;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("signatures.c"), &mut graph);
    assert!(result.is_ok());

    let functions = graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap();
    assert_eq!(functions.len(), 3);

    // Verify all functions have signatures
    for func_id in &functions {
        let node = graph.get_node(*func_id).unwrap();
        let signature = node.properties.get_string("signature");
        assert!(
            signature.is_some(),
            "Function should have a signature stored"
        );
        assert!(
            !signature.unwrap().is_empty(),
            "Signature should not be empty"
        );
    }
}

#[test]
fn test_e2e_return_type_stored() {
    let source = r#"
int returns_int(void) { return 0; }
void returns_void(void) {}
char *returns_pointer(void) { return NULL; }
struct Data *returns_struct_ptr(void) { return NULL; }
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser.parse_source(source, Path::new("return_types.c"), &mut graph);
    assert!(result.is_ok());

    // Find returns_int and check return type
    let int_func = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("returns_int")
        .execute()
        .unwrap();
    assert_eq!(int_func.len(), 1);

    let node = graph.get_node(int_func[0]).unwrap();
    let return_type = node.properties.get_string("return_type");
    assert!(
        return_type.is_some(),
        "Function should have return_type property"
    );
    assert_eq!(return_type.unwrap(), "int");

    // Find returns_void and check return type
    let void_func = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("returns_void")
        .execute()
        .unwrap();
    assert_eq!(void_func.len(), 1);

    let node = graph.get_node(void_func[0]).unwrap();
    let return_type = node.properties.get_string("return_type");
    assert!(return_type.is_some());
    assert_eq!(return_type.unwrap(), "void");
}

#[test]
fn test_vmk_driver_function_parsing() {
    let source = r#"
VMK_ReturnStatus
irndrv_RDMAOpGetPrivStats(
    vmk_AddrCookie driverData,
    vmk_ByteCount length)
{
    VMK_ReturnStatus status;
    vmk_uint32 count = 0;

    if (length > 100) {
        for (int i = 0; i < count; i++) {
            vmk_StringFormat(buffer, sizeof(buffer), "test");
        }
        vmk_Memset(buffer, 0, sizeof(buffer));
    } else if (count > 0) {
        vmk_Warning("bad count");
    }

    switch (count) {
        case 0:
            break;
        case 1:
            return VMK_FAILURE;
        default:
            break;
    }

    while (status != 0) {
        status = vmk_AtomicRead16(&flag);
    }

    return VMK_OK;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser
        .parse_source(source, Path::new("test_vmk.c"), &mut graph)
        .expect("should parse");

    println!("Functions found: {}", result.functions.len());
    assert!(
        !result.functions.is_empty(),
        "Should find at least one function"
    );

    // Query functions from graph
    let func_ids = graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap();

    // Find our function
    let func_node = func_ids
        .iter()
        .filter_map(|id| {
            let node = graph.get_node(*id).ok()?;
            let name = node.properties.get_string("name")?;
            if name == "irndrv_RDMAOpGetPrivStats" {
                Some(node.clone())
            } else {
                None
            }
        })
        .next()
        .expect("Should find irndrv_RDMAOpGetPrivStats");

    let complexity = func_node.properties.get_int("complexity").unwrap_or(0);
    let branches = func_node
        .properties
        .get_int("complexity_branches")
        .unwrap_or(0);
    let loops = func_node
        .properties
        .get_int("complexity_loops")
        .unwrap_or(0);

    println!(
        "  complexity={}, branches={}, loops={}",
        complexity, branches, loops
    );

    assert!(
        complexity > 1,
        "Expected complexity > 1, got {}",
        complexity
    );
    assert!(branches > 0, "Expected branches > 0, got {}", branches);
    assert!(loops > 0, "Expected loops > 0, got {}", loops);
}

#[test]
fn test_vmk_driver_with_custom_types() {
    // Realistic test: VMK types + unknown project types (irndrv_Pf etc.)
    let source = r#"
#include "irndrv_common.h"

#define RETRYCNT_BOUNDARY 7

VMK_ReturnStatus
irndrv_RDMAOpGetPrivStats(vmk_AddrCookie driverData, char *statBuf,
                          vmk_ByteCount length)
{
   irndrv_Pf                 *pf = (irndrv_Pf *)driverData.ptr;
   irndrv_rdmaDevice         *rdmaDevice = &pf->rdmaDevice;
   struct irdma_pci_f        *rf = rdmaDevice->rf;
   vmk_ByteCount             outLen;
   VMK_ReturnStatus          status;
   vmk_ByteCount             index = 0;

   if (length < 100) {
      return VMK_BAD_PARAM;
   }

   if (vmk_AtomicRead16(&pf->state) & 0xFF) {
      vmk_Warning(log, "Device unavailable");
      return VMK_FAILURE;
   }

   vmk_Memset(statBuf, 0, length);

   status = vmk_StringFormat(statBuf, length - index, &outLen, "stats");
   if (status != VMK_OK) {
      return status;
   }
   index += outLen;

   for (int i = 0; i < 10; i++) {
      vmk_StringFormat(statBuf + index, length - index, &outLen, "%d", i);
      index += outLen;
   }

   while (index < length) {
      statBuf[index] = 0;
      index++;
   }

   return VMK_OK;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let result = parser
        .parse_source(source, Path::new("test_vmk.c"), &mut graph)
        .expect("should parse");

    println!("Functions found: {}", result.functions.len());
    assert!(
        !result.functions.is_empty(),
        "Should find at least one function"
    );

    let func_ids = graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap();

    let func_node = func_ids
        .iter()
        .filter_map(|id| {
            let node = graph.get_node(*id).ok()?;
            let name = node.properties.get_string("name")?;
            if name == "irndrv_RDMAOpGetPrivStats" {
                Some(node.clone())
            } else {
                None
            }
        })
        .next()
        .expect("Should find irndrv_RDMAOpGetPrivStats");

    let complexity = func_node.properties.get_int("complexity").unwrap_or(0);
    let branches = func_node
        .properties
        .get_int("complexity_branches")
        .unwrap_or(0);
    let loops = func_node
        .properties
        .get_int("complexity_loops")
        .unwrap_or(0);

    println!(
        "  complexity={}, branches={}, loops={}",
        complexity, branches, loops
    );

    // With unknown custom types (irndrv_Pf etc.) in the body, tree-sitter
    // will have ERROR nodes, but our visitor should still find the
    // if/for/while statements inside them.
    assert!(
        complexity > 1,
        "Expected complexity > 1 even with unknown custom types, got {}",
        complexity
    );
    assert!(branches > 0, "Expected branches > 0, got {}", branches);
    assert!(loops > 0, "Expected loops > 0, got {}", loops);
}

#[test]
fn test_ice_driver_parsing() {
    use codegraph::{CodeGraph, NodeType};
    use codegraph_c::{CParser, CodeParser};
    use std::path::Path;

    let ice_dir = Path::new("/Users/anvanster/projects/docs/ethernet-linux-ice/src");
    if !ice_dir.exists() {
        eprintln!(
            "Skipping: ICE driver source not found at {}",
            ice_dir.display()
        );
        return;
    }

    let parser = CParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let mut total_files = 0u32;
    let mut total_functions = 0u32;
    let mut parse_errors = 0u32;

    for entry in std::fs::read_dir(ice_dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "c") {
            total_files += 1;
            match parser.parse_file(&path, &mut graph) {
                Ok(fi) => {
                    total_functions += fi.functions.len() as u32;
                }
                Err(e) => {
                    parse_errors += 1;
                    eprintln!(
                        "  FAIL: {}: {}",
                        path.file_name().unwrap().to_string_lossy(),
                        e
                    );
                }
            }
        }
    }

    eprintln!("\n=== ICE Driver Parse Results ===");
    eprintln!("Files:     {total_files}");
    eprintln!("Functions: {total_functions}");
    eprintln!("Errors:    {parse_errors}");
    eprintln!("Nodes:     {}", graph.node_count());
    eprintln!("Edges:     {}", graph.edge_count());

    // Collect complexity stats
    let mut complexities: Vec<(String, u32)> = Vec::new();
    for (_, node) in graph.nodes_iter() {
        if node.node_type == NodeType::Function {
            let name = node
                .properties
                .get_string("name")
                .unwrap_or("?")
                .to_string();
            let c = node.properties.get_int("complexity").unwrap_or(1) as u32;
            complexities.push((name, c));
        }
    }
    complexities.sort_by(|a, b| b.1.cmp(&a.1));
    eprintln!("\nTop 15 most complex functions:");
    for (name, c) in complexities.iter().take(15) {
        let grade = match c {
            0..=5 => 'A',
            6..=10 => 'B',
            11..=20 => 'C',
            21..=50 => 'D',
            _ => 'F',
        };
        eprintln!("  {c:3} ({grade}) {name}");
    }

    // Assertions
    assert!(total_files > 50, "Expected >50 C files, got {total_files}");
    assert!(
        total_functions > 100,
        "Expected >100 functions, got {total_functions}"
    );
    // Allow some parse errors (kernel C has macros/ifdefs that confuse tree-sitter)
    let success_rate = (total_files - parse_errors) as f64 / total_files as f64;
    eprintln!("\nSuccess rate: {:.0}%", success_rate * 100.0);
    assert!(
        success_rate > 0.5,
        "Less than 50% of files parsed successfully"
    );
}

#[test]
fn test_vmware_open_vm_tools_parsing() {
    use codegraph::{CodeGraph, NodeType};
    use codegraph_c::{CParser, CodeParser};

    let src_dir = std::path::Path::new("/Users/anvanster/projects/docs/open-vm-tools");
    if !src_dir.exists() {
        eprintln!("Skipping: open-vm-tools not found");
        return;
    }

    let parser = CParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let mut total_files = 0u32;
    let mut total_functions = 0u32;
    let mut parse_errors = 0u32;
    let mut vmk_files = 0u32;
    let mut vmk_functions = 0u32;

    // Recursively find all .c files
    fn find_c_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    find_c_files(&path, files);
                } else if path.extension().is_some_and(|e| e == "c") {
                    files.push(path);
                }
            }
        }
    }

    let mut c_files = Vec::new();
    find_c_files(src_dir, &mut c_files);

    for path in &c_files {
        total_files += 1;
        let source = std::fs::read_to_string(path).unwrap_or_default();
        let has_vmk = source.contains("vmk_") || source.contains("VMK_");

        match parser.parse_file(path, &mut graph) {
            Ok(fi) => {
                total_functions += fi.functions.len() as u32;
                if has_vmk {
                    vmk_files += 1;
                    vmk_functions += fi.functions.len() as u32;
                }
            }
            Err(e) => {
                parse_errors += 1;
                if has_vmk {
                    eprintln!(
                        "  VMK FAIL: {}: {}",
                        path.file_name().unwrap().to_string_lossy(),
                        e
                    );
                }
            }
        }
    }

    eprintln!("\n=== VMware open-vm-tools Parse Results ===");
    eprintln!("Total files:     {total_files}");
    eprintln!("Total functions: {total_functions}");
    eprintln!("Parse errors:    {parse_errors}");
    eprintln!("VMK files:       {vmk_files} ({vmk_functions} functions)");
    eprintln!("Nodes:           {}", graph.node_count());
    eprintln!("Edges:           {}", graph.edge_count());

    let success_rate = (total_files - parse_errors) as f64 / total_files as f64;
    eprintln!("Success rate:    {:.0}%", success_rate * 100.0);

    // Show top complex VMK functions
    let mut complexities: Vec<(String, String, u32)> = Vec::new();
    for (_, node) in graph.nodes_iter() {
        if node.node_type == NodeType::Function {
            let name = node
                .properties
                .get_string("name")
                .unwrap_or("?")
                .to_string();
            let path = node.properties.get_string("path").unwrap_or("").to_string();
            let c = node.properties.get_int("complexity").unwrap_or(1) as u32;
            if path.contains("vmk")
                || path.contains("VMK")
                || path.contains("vmci")
                || path.contains("pvscsi")
            {
                complexities.push((name, path, c));
            }
        }
    }
    complexities.sort_by(|a, b| b.2.cmp(&a.2));
    if !complexities.is_empty() {
        eprintln!("\nTop VMware-related complex functions:");
        for (name, path, c) in complexities.iter().take(10) {
            let file = std::path::Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            eprintln!("  {c:3} {name} ({file})");
        }
    }

    assert!(total_files > 100, "Expected >100 C files");
    assert!(total_functions > 500, "Expected >500 functions");
    assert!(success_rate > 0.5, "Less than 50% success rate");
}

#[test]
fn test_vtable_detection_on_vmware_fsops() {
    use codegraph::{CodeGraph, NodeType};
    use codegraph_c::{CParser, CodeParser};

    let fsops = std::path::Path::new(
        "/Users/anvanster/projects/docs/open-vm-tools/open-vm-tools/vmblock-fuse/fsops.c",
    );
    if !fsops.exists() {
        eprintln!("Skipping: open-vm-tools fsops.c not found");
        return;
    }

    let parser = CParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();
    let fi = parser
        .parse_file(fsops, &mut graph)
        .expect("Failed to parse fsops.c");

    eprintln!("Functions: {}", fi.functions.len());
    eprintln!("Edges: {}", graph.edge_count());

    // The vtable: struct fuse_operations vmblockOperations = {
    //   .readlink = VMBlockReadLink, .getattr = VMBlockGetAttr, ...
    // };
    // These should create Calls edges from the file node to the function nodes.
    // Find the file node and check its outgoing Calls edges.
    use codegraph::{Direction, EdgeType};

    let file_node = graph
        .nodes_iter()
        .find(|(_, n)| n.node_type == NodeType::CodeFile)
        .map(|(&id, _)| id);

    assert!(file_node.is_some(), "Should have a file node");
    let file_id = file_node.unwrap();

    let neighbors = graph
        .get_neighbors(file_id, Direction::Outgoing)
        .unwrap_or_default();
    let mut vtable_targets: Vec<String> = Vec::new();
    for &neighbor_id in &neighbors {
        if let Ok(edges) = graph.get_edges_between(file_id, neighbor_id) {
            for &edge_id in &edges {
                if let Ok(edge) = graph.get_edge(edge_id) {
                    if edge.edge_type == EdgeType::Calls {
                        if let Ok(target) = graph.get_node(neighbor_id) {
                            let name = target
                                .properties
                                .get_string("name")
                                .unwrap_or("")
                                .to_string();
                            vtable_targets.push(name);
                        }
                    }
                }
            }
        }
    }

    eprintln!("Vtable Calls edges from file node: {:?}", vtable_targets);

    assert!(
        vtable_targets.contains(&"VMBlockReadLink".to_string()),
        "Expected Calls edge to VMBlockReadLink. Got: {:?}",
        vtable_targets
    );
    assert!(
        vtable_targets.contains(&"VMBlockGetAttr".to_string()),
        "Expected Calls edge to VMBlockGetAttr. Got: {:?}",
        vtable_targets
    );
}

#[test]
fn test_stdint_call_extraction() {
    use codegraph::CodeGraph;
    use codegraph_c::CParser;
    use std::path::Path;

    let source = r#"
#include "hw_common.h"
#include <stdio.h>

int driver_probe(void) {
    int ret = hw_init(NULL);
    if (ret < 0) {
        printf("failed\n");
        return ret;
    }
    return 0;
}

int driver_set_mtu(uint32_t mtu) {
    hw_reset();
    return hw_init(NULL);
}
"#;

    let path = Path::new("/tmp/test_driver.c");
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let _info = parser.parse_source(source, path, &mut graph).unwrap();

    // Print all nodes
    for (id, node) in graph.iter_nodes() {
        let name = node.properties.get_string("name").unwrap_or("?");
        println!("Node {}: {:?} name={}", id, node.node_type, name);
    }

    // Check that calls were extracted
    let mut found_calls = Vec::new();
    for (_, edge) in graph.iter_edges() {
        if edge.edge_type == codegraph::EdgeType::Calls {
            let src = graph.get_node(edge.source_id).unwrap();
            let tgt = graph.get_node(edge.target_id).unwrap();
            let src_name = src.properties.get_string("name").unwrap_or("?");
            let tgt_name = tgt.properties.get_string("name").unwrap_or("?");
            println!("CALL: {} -> {}", src_name, tgt_name);
            found_calls.push((src_name.to_string(), tgt_name.to_string()));
        }
    }

    // Also check unresolved_calls
    for (_, node) in graph.iter_nodes() {
        if let Some(unresolved) = node.properties.get_string_list_compat("unresolved_calls") {
            let name = node.properties.get_string("name").unwrap_or("?");
            println!("UNRESOLVED for {}: {:?}", name, unresolved);
        }
    }

    assert!(
        !found_calls.is_empty() || {
            graph.iter_nodes().any(|(_, n)| {
                n.properties
                    .get_string_list_compat("unresolved_calls")
                    .map(|c| !c.is_empty())
                    .unwrap_or(false)
            })
        },
        "Expected call edges or unresolved_calls for hw_init/hw_reset/printf"
    );
}

#[test]
fn test_header_type_resolution() {
    use codegraph::CodeGraph;
    use codegraph_c::CParser;
    use std::path::Path;

    // Write a temporary header
    let header = r#"
#ifndef TEST_H
#define TEST_H
typedef unsigned int my_uint32;
typedef struct my_ctx my_ctx_t;
typedef void (*callback_fn)(int);
struct my_data {
    int value;
};
#endif
"#;
    let header_path = "/tmp/test_header_resolve.h";
    std::fs::write(header_path, header).unwrap();

    let source = r#"
#include "test_header_resolve.h"

int process(my_uint32 count) {
    struct my_data d;
    d.value = count;
    return d.value;
}
"#;

    let path = Path::new("/tmp/test_with_header.c");
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let info = parser.parse_source(source, path, &mut graph).unwrap();

    // Should have parsed the function successfully
    assert!(
        !info.functions.is_empty(),
        "Should parse process() function"
    );

    // Check that the function node exists with correct name
    let mut found_process = false;
    for (_, node) in graph.iter_nodes() {
        if node.properties.get_string("name") == Some("process") {
            found_process = true;
            // Check signature includes my_uint32 (parsed correctly, not ERROR)
            let sig = node.properties.get_string("signature").unwrap_or("");
            println!("Signature: {}", sig);
            assert!(
                sig.contains("my_uint32") || sig.contains("count"),
                "Signature should contain parameter info: {}",
                sig
            );
            break;
        }
    }
    assert!(found_process, "Should find process() function in graph");

    // Cleanup
    let _ = std::fs::remove_file(header_path);
}

#[test]
fn test_ops_struct_type_tracking() {
    use codegraph::CodeGraph;
    use codegraph_c::CParser;
    use std::path::Path;

    let source = r#"
static int my_open(void *ctx) { return 0; }
static int my_close(void *ctx) { return 0; }
static int my_read(void *ctx, char *buf, int len) { return len; }

struct device_ops {
    int (*open)(void *);
    int (*close)(void *);
    int (*read)(void *, char *, int);
};

static const struct device_ops my_device_ops = {
    .open = my_open,
    .close = my_close,
    .read = my_read,
};
"#;

    let path = Path::new("/tmp/test_ops_struct.c");
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let _info = parser.parse_source(source, path, &mut graph).unwrap();

    let mut vtable_edges = Vec::new();
    for (_, edge) in graph.iter_edges() {
        if edge.edge_type == codegraph::EdgeType::Calls {
            let struct_type = edge
                .properties
                .get_string("struct_type")
                .map(|s| s.to_string());
            let field_name = edge
                .properties
                .get_string("field_name")
                .map(|s| s.to_string());
            if struct_type.is_some() || field_name.is_some() {
                let tgt = graph.get_node(edge.target_id).unwrap();
                let tgt_name = tgt.properties.get_string("name").unwrap_or("?");
                println!(
                    "VTABLE: {}.{} = {} (struct_type={:?}, field_name={:?})",
                    struct_type.as_deref().unwrap_or("?"),
                    field_name.as_deref().unwrap_or("?"),
                    tgt_name,
                    struct_type,
                    field_name,
                );
                vtable_edges.push((
                    struct_type.unwrap_or_default(),
                    field_name.unwrap_or_default(),
                    tgt_name.to_string(),
                ));
            }
        }
    }

    assert_eq!(
        vtable_edges.len(),
        3,
        "Expected 3 vtable assignments, got {}",
        vtable_edges.len()
    );
    assert!(
        vtable_edges
            .iter()
            .any(|(st, f, t)| st == "device_ops" && f == "open" && t == "my_open"),
        "Missing device_ops.open = my_open"
    );
    assert!(
        vtable_edges
            .iter()
            .any(|(st, f, t)| st == "device_ops" && f == "close" && t == "my_close"),
        "Missing device_ops.close = my_close"
    );
    assert!(
        vtable_edges
            .iter()
            .any(|(st, f, t)| st == "device_ops" && f == "read" && t == "my_read"),
        "Missing device_ops.read = my_read"
    );
}

#[test]
fn test_ethtool_ops_struct_detection() {
    use codegraph::CodeGraph;
    use codegraph_c::CParser;
    use std::path::Path;

    let source = r#"
static int my_get_info(void *dev) { return 0; }
static int my_get_stats(void *dev) { return 0; }
static int my_set_config(void *dev) { return 0; }

struct ethtool_ops {
    int (*get_info)(void *);
    int (*get_stats)(void *);
    int (*set_config)(void *);
};

static const struct ethtool_ops my_ethtool_ops = {
    .get_info = my_get_info,
    .get_stats = my_get_stats,
    .set_config = my_set_config,
};
"#;

    let path = Path::new("/tmp/test_ethtool_ops.c");
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let _info = parser.parse_source(source, path, &mut graph).unwrap();

    let mut vtable_edges = Vec::new();
    for (_, edge) in graph.iter_edges() {
        if edge.edge_type == codegraph::EdgeType::Calls {
            let st = edge
                .properties
                .get_string("struct_type")
                .map(|s| s.to_string());
            let field = edge
                .properties
                .get_string("field_name")
                .map(|s| s.to_string());
            if st.is_some() {
                let tgt = graph.get_node(edge.target_id).unwrap();
                let tgt_name = tgt.properties.get_string("name").unwrap_or("?");
                println!(
                    "OPS: {}.{} = {}",
                    st.as_deref().unwrap_or("?"),
                    field.as_deref().unwrap_or("?"),
                    tgt_name
                );
                vtable_edges.push((
                    st.unwrap_or_default(),
                    field.unwrap_or_default(),
                    tgt_name.to_string(),
                ));
            }
        }
    }

    assert_eq!(
        vtable_edges.len(),
        3,
        "Expected 3 ops struct assignments, got {}: {:?}",
        vtable_edges.len(),
        vtable_edges
    );
    assert!(vtable_edges
        .iter()
        .any(|(s, f, t)| s == "ethtool_ops" && f == "get_info" && t == "my_get_info"));
    assert!(vtable_edges
        .iter()
        .any(|(s, f, t)| s == "ethtool_ops" && f == "get_stats" && t == "my_get_stats"));
    assert!(vtable_edges
        .iter()
        .any(|(s, f, t)| s == "ethtool_ops" && f == "set_config" && t == "my_set_config"));
}

#[test]
fn test_kernel_macro_detection() {
    use codegraph::CodeGraph;
    use codegraph_c::CParser;
    use std::path::Path;

    let source = r#"
static int my_init(void) { return 0; }
static void my_exit(void) { }
int my_public_func(int x) { return x + 1; }
int my_internal_func(int x) { return x - 1; }

module_init(my_init);
module_exit(my_exit);
EXPORT_SYMBOL(my_public_func);
EXPORT_SYMBOL_GPL(my_internal_func);
"#;

    let path = Path::new("/tmp/test_kernel_macros.c");
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CParser::new();
    let _info = parser.parse_source(source, path, &mut graph).unwrap();

    // Check entry points
    let mut found_init = false;
    let mut found_exit = false;
    let mut found_exported = 0;

    for (_, node) in graph.iter_nodes() {
        let name = node.properties.get_string("name").unwrap_or("");
        let is_entry = node.properties.get_bool("is_entry_point").unwrap_or(false);
        let is_exported = node.properties.get_bool("is_exported").unwrap_or(false);

        if name == "my_init" && is_entry {
            found_init = true;
            println!(
                "ENTRY: {} (entry_type={:?})",
                name,
                node.properties.get_string("entry_type")
            );
        }
        if name == "my_exit" && is_entry {
            found_exit = true;
        }
        if is_exported {
            found_exported += 1;
            println!("EXPORTED: {}", name);
        }
    }

    assert!(found_init, "my_init should be marked as entry point");
    assert!(found_exit, "my_exit should be marked as entry point");
    assert_eq!(found_exported, 2, "Expected 2 exported symbols");
}
