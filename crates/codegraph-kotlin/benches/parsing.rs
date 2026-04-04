// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::CodeGraph;
use codegraph_kotlin::KotlinParser;
use codegraph_parser_api::CodeParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

const SIMPLE_CLASS: &str = r#"
class HelloWorld {
    fun main() {
        println("Hello, World!")
    }
}
"#;

const COMPLEX_CLASS: &str = r#"
package com.example.app

import java.util.List
import java.util.ArrayList
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * A complex class with multiple methods and inheritance
 */
class ComplexClass(
    private val name: String,
    private val value: Int
) : BaseClass(), Serializable, Comparable<ComplexClass> {

    private val items: MutableList<String> = ArrayList()

    fun getName(): String = name

    fun getValue(): Int = value

    fun addItem(item: String) {
        items.add(item)
    }

    fun getItems(): List<String> = items.toList()

    override fun compareTo(other: ComplexClass): Int {
        return value.compareTo(other.value)
    }

    override fun toString(): String {
        return "ComplexClass(name='$name', value=$value)"
    }

    suspend fun fetchData(): String = withContext(Dispatchers.IO) {
        "data"
    }

    private fun helper() {
        process()
        validate()
    }

    private fun process() {
        // Processing logic
    }

    private fun validate() {
        // Validation logic
    }

    companion object {
        fun create(name: String): ComplexClass = ComplexClass(name, 0)
    }
}
"#;

fn bench_simple_parsing(c: &mut Criterion) {
    let parser = KotlinParser::new();
    let path = Path::new("HelloWorld.kt");

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
    let parser = KotlinParser::new();
    let path = Path::new("ComplexClass.kt");

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
