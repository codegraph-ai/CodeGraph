// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

// Rust parser performance benchmarks
use codegraph::CodeGraph;
use codegraph_parser_api::{CodeParser, ParserConfig};
use codegraph_rust::RustParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

fn bench_parse_simple_function(c: &mut Criterion) {
    let source = r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn multiply(x: i32, y: i32) -> i32 {
    x * y
}
"#;

    c.bench_function("parse_simple_function", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = RustParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.rs"), &mut graph)
                .unwrap();
        });
    });
}

fn bench_parse_struct_and_impl(c: &mut Criterion) {
    let source = r#"
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn distance(&self, other: &Point) -> f64 {
        let dx = (self.x - other.x) as f64;
        let dy = (self.y - other.y) as f64;
        (dx * dx + dy * dy).sqrt()
    }
}
"#;

    c.bench_function("parse_struct_and_impl", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = RustParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.rs"), &mut graph)
                .unwrap();
        });
    });
}

fn bench_parse_complex_module(c: &mut Criterion) {
    let source = r#"
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub trait Storage {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&mut self, key: String, value: String);
}

pub struct InMemoryStorage {
    data: HashMap<String, String>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
}

impl Storage for InMemoryStorage {
    fn get(&self, key: &str) -> Option<String> {
        self.data.get(key).cloned()
    }

    fn set(&mut self, key: String, value: String) {
        self.data.insert(key, value);
    }
}

pub enum StorageError {
    NotFound,
    InvalidKey,
    Internal(String),
}

pub async fn fetch_data(storage: Arc<Mutex<dyn Storage>>, key: &str) -> Result<String, StorageError> {
    let storage = storage.lock().unwrap();
    storage.get(key).ok_or(StorageError::NotFound)
}
"#;

    c.bench_function("parse_complex_module", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = RustParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.rs"), &mut graph)
                .unwrap();
        });
    });
}

fn bench_parse_real_project(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_project");

    // Parse the actual codegraph-monorepo crates directory (~94 Rust files)
    let crates_path = Path::new("/Users/anvanster/projects/codegraph-monorepo/crates");

    // Sequential parsing
    group.bench_function("sequential", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let config = ParserConfig {
                parallel: false,
                ..Default::default()
            };
            let parser = RustParser::with_config(config);
            parser
                .parse_directory(black_box(crates_path), &mut graph)
                .unwrap();
        });
    });

    // Parallel parsing with 2 workers
    group.bench_function("parallel_2", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let config = ParserConfig {
                parallel: true,
                parallel_workers: Some(2),
                ..Default::default()
            };
            let parser = RustParser::with_config(config);
            parser
                .parse_directory(black_box(crates_path), &mut graph)
                .unwrap();
        });
    });

    // Parallel parsing with 4 workers
    group.bench_function("parallel_4", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let config = ParserConfig {
                parallel: true,
                parallel_workers: Some(4),
                ..Default::default()
            };
            let parser = RustParser::with_config(config);
            parser
                .parse_directory(black_box(crates_path), &mut graph)
                .unwrap();
        });
    });

    // Parallel parsing with 8 workers
    group.bench_function("parallel_8", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let config = ParserConfig {
                parallel: true,
                parallel_workers: Some(8),
                ..Default::default()
            };
            let parser = RustParser::with_config(config);
            parser
                .parse_directory(black_box(crates_path), &mut graph)
                .unwrap();
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_parse_simple_function,
    bench_parse_struct_and_impl,
    bench_parse_complex_module,
    bench_parse_real_project
);
criterion_main!(benches);
