// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Benchmarks for C++ parser

use codegraph::CodeGraph;
use codegraph_cpp::CppParser;
use codegraph_parser_api::CodeParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

const SAMPLE_CODE: &str = r#"
#include <iostream>
#include <vector>
#include <memory>

namespace myproject {

template<typename T>
class Container {
public:
    Container() = default;
    ~Container() = default;

    void add(T item) {
        items.push_back(std::move(item));
    }

    T& get(size_t index) {
        return items[index];
    }

    size_t size() const {
        return items.size();
    }

private:
    std::vector<T> items;
};

class Base {
public:
    virtual ~Base() = default;
    virtual void process() = 0;
};

class Derived : public Base {
public:
    void process() override {
        std::cout << "Processing" << std::endl;
    }
};

void helper(int x) {
    std::cout << x << std::endl;
}

int main() {
    Container<int> container;
    container.add(1);
    container.add(2);

    auto derived = std::make_unique<Derived>();
    derived->process();

    helper(container.size());

    return 0;
}

} // namespace myproject
"#;

fn benchmark_parse_source(c: &mut Criterion) {
    let parser = CppParser::new();

    c.bench_function("cpp_parse_source", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(SAMPLE_CODE), Path::new("bench.cpp"), &mut graph)
                .unwrap()
        })
    });
}

fn benchmark_extract_entities(c: &mut Criterion) {
    let parser = CppParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();
    let file_info = parser
        .parse_source(SAMPLE_CODE, Path::new("bench.cpp"), &mut graph)
        .unwrap();

    c.bench_function("cpp_entity_count", |b| {
        b.iter(|| black_box(file_info.classes.len() + file_info.functions.len()))
    });
}

criterion_group!(benches, benchmark_parse_source, benchmark_extract_entities);
criterion_main!(benches);
