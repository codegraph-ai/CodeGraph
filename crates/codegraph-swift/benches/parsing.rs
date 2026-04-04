// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Benchmarks for Swift parser

use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_swift::SwiftParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

const SAMPLE_CODE: &str = r#"
import Foundation

/// A protocol for drawable objects
protocol Drawable {
    func draw()
}

/// A generic container class
class Container<T> {
    private var items: [T] = []

    func add(_ item: T) {
        items.append(item)
    }

    func get(_ index: Int) -> T? {
        guard index < items.count else { return nil }
        return items[index]
    }

    var count: Int {
        return items.count
    }
}

/// Base shape class
class Shape: Drawable {
    var name: String

    init(name: String) {
        self.name = name
    }

    func draw() {
        print("Drawing \(name)")
    }
}

/// Circle shape
class Circle: Shape {
    var radius: Double

    init(radius: Double) {
        self.radius = radius
        super.init(name: "Circle")
    }

    override func draw() {
        print("Drawing circle with radius \(radius)")
    }
}

/// Helper function
func greet(_ name: String) -> String {
    return "Hello, \(name)!"
}

/// Main entry point
func main() {
    let container = Container<Int>()
    container.add(1)
    container.add(2)

    let circle = Circle(radius: 5.0)
    circle.draw()

    print(greet("World"))
}
"#;

fn benchmark_parse_source(c: &mut Criterion) {
    let parser = SwiftParser::new();

    c.bench_function("swift_parse_source", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(SAMPLE_CODE), Path::new("bench.swift"), &mut graph)
                .unwrap()
        })
    });
}

fn benchmark_extract_entities(c: &mut Criterion) {
    let parser = SwiftParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();
    let file_info = parser
        .parse_source(SAMPLE_CODE, Path::new("bench.swift"), &mut graph)
        .unwrap();

    c.bench_function("swift_entity_count", |b| {
        b.iter(|| black_box(file_info.classes.len() + file_info.functions.len()))
    });
}

criterion_group!(benches, benchmark_parse_source, benchmark_extract_entities);
criterion_main!(benches);
