// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::CodeGraph;
use codegraph_java::JavaParser;
use codegraph_parser_api::CodeParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

const SIMPLE_CLASS: &str = r#"
public class HelloWorld {
    public static void main(String[] args) {
        System.out.println("Hello, World!");
    }
}
"#;

const COMPLEX_CLASS: &str = r#"
package com.example.app;

import java.util.List;
import java.util.ArrayList;
import java.util.Map;
import java.util.HashMap;

/**
 * A complex class with multiple methods and inheritance
 */
public class ComplexClass extends BaseClass implements Serializable, Comparable<ComplexClass> {
    private final String name;
    private final int value;
    private List<String> items;

    public ComplexClass(String name, int value) {
        this.name = name;
        this.value = value;
        this.items = new ArrayList<>();
    }

    public String getName() {
        return name;
    }

    public int getValue() {
        return value;
    }

    public void addItem(String item) {
        items.add(item);
    }

    public List<String> getItems() {
        return new ArrayList<>(items);
    }

    @Override
    public int compareTo(ComplexClass other) {
        return Integer.compare(this.value, other.value);
    }

    @Override
    public String toString() {
        return String.format("ComplexClass{name='%s', value=%d}", name, value);
    }

    private void helper() {
        process();
        validate();
    }

    private void process() {
        // Processing logic
    }

    private void validate() {
        // Validation logic
    }
}
"#;

fn bench_simple_parsing(c: &mut Criterion) {
    let parser = JavaParser::new();
    let path = Path::new("HelloWorld.java");

    c.bench_function("parse_simple_class", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(SIMPLE_CLASS), path, &mut graph)
                .unwrap()
        })
    });
}

fn bench_complex_parsing(c: &mut Criterion) {
    let parser = JavaParser::new();
    let path = Path::new("ComplexClass.java");

    c.bench_function("parse_complex_class", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(COMPLEX_CLASS), path, &mut graph)
                .unwrap()
        })
    });
}

criterion_group!(benches, bench_simple_parsing, bench_complex_parsing);
criterion_main!(benches);
