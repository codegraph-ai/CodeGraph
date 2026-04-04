// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Benchmarks for the C parser

use codegraph::CodeGraph;
use codegraph_c::{CParser, CodeParser};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

fn generate_simple_c_code(num_functions: usize) -> String {
    let mut code = String::from("#include <stdio.h>\n\n");

    for i in 0..num_functions {
        code.push_str(&format!(
            r#"
int function_{i}(int x) {{
    return x + {i};
}}
"#
        ));
    }

    code
}

fn generate_complex_c_code(num_functions: usize) -> String {
    let mut code = String::from("#include <stdio.h>\n#include <stdlib.h>\n\n");

    for i in 0..num_functions {
        code.push_str(&format!(
            r#"
int complex_function_{i}(int x, int y) {{
    int result = 0;

    if (x > 0) {{
        for (int j = 0; j < x; j++) {{
            if (j % 2 == 0 && y > 0) {{
                result += j;
            }} else if (j % 3 == 0 || y < 0) {{
                result -= j;
            }}
        }}
    }} else {{
        while (y > 0) {{
            switch (y % 3) {{
                case 0: result += 10; break;
                case 1: result -= 5; break;
                default: result *= 2; break;
            }}
            y--;
        }}
    }}

    return result + {i};
}}
"#
        ));
    }

    code
}

fn generate_struct_heavy_code(num_structs: usize) -> String {
    let mut code = String::new();

    for i in 0..num_structs {
        code.push_str(&format!(
            r#"
struct Struct_{i} {{
    int field_a;
    int field_b;
    char *name;
    struct Struct_{i} *next;
}};
"#
        ));
    }

    // Add some functions using the structs
    for i in 0..num_structs.min(10) {
        code.push_str(&format!(
            r#"
struct Struct_{i}* create_struct_{i}(int a, int b) {{
    return NULL;
}}
"#
        ));
    }

    code
}

fn benchmark_small_file(c: &mut Criterion) {
    let source = generate_simple_c_code(10);
    let parser = CParser::new();

    c.bench_function("parse_small_file_10_funcs", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(&source), Path::new("test.c"), &mut graph)
                .unwrap()
        })
    });
}

fn benchmark_medium_file(c: &mut Criterion) {
    let source = generate_simple_c_code(50);
    let parser = CParser::new();

    c.bench_function("parse_medium_file_50_funcs", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(&source), Path::new("test.c"), &mut graph)
                .unwrap()
        })
    });
}

fn benchmark_large_file(c: &mut Criterion) {
    let source = generate_simple_c_code(200);
    let parser = CParser::new();

    c.bench_function("parse_large_file_200_funcs", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(&source), Path::new("test.c"), &mut graph)
                .unwrap()
        })
    });
}

fn benchmark_complex_code(c: &mut Criterion) {
    let source = generate_complex_c_code(20);
    let parser = CParser::new();

    c.bench_function("parse_complex_20_funcs", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(&source), Path::new("test.c"), &mut graph)
                .unwrap()
        })
    });
}

fn benchmark_struct_heavy(c: &mut Criterion) {
    let source = generate_struct_heavy_code(50);
    let parser = CParser::new();

    c.bench_function("parse_struct_heavy_50_structs", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(&source), Path::new("test.c"), &mut graph)
                .unwrap()
        })
    });
}

criterion_group!(
    benches,
    benchmark_small_file,
    benchmark_medium_file,
    benchmark_large_file,
    benchmark_complex_code,
    benchmark_struct_heavy
);
criterion_main!(benches);
