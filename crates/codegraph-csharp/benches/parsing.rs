// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::CodeGraph;
use codegraph_csharp::CSharpParser;
use codegraph_parser_api::CodeParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

const SIMPLE_CLASS: &str = r#"
public class HelloWorld
{
    public static void Main(string[] args)
    {
        Console.WriteLine("Hello, World!");
    }
}
"#;

const COMPLEX_CLASS: &str = r#"
using System;
using System.Collections.Generic;

namespace MyApp.Models
{
    /// <summary>
    /// A complex class with multiple methods and inheritance
    /// </summary>
    public class ComplexClass : BaseClass, ISerializable, IDisposable
    {
        private string _name;
        private int _value;
        private readonly List<string> _items;

        public string Name
        {
            get => _name;
            set => _name = value;
        }

        public int Value { get; set; }

        public ComplexClass(string name, int value)
        {
            _name = name;
            _value = value;
            _items = new List<string>();
        }

        public void AddItem(string item)
        {
            _items.Add(item);
        }

        public IReadOnlyList<string> GetItems()
        {
            return _items.AsReadOnly();
        }

        public int CompareTo(ComplexClass other)
        {
            return _value.CompareTo(other._value);
        }

        public override string ToString()
        {
            return $"ComplexClass(Name='{_name}', Value={_value})";
        }

        public async Task<string> FetchDataAsync()
        {
            await Task.Delay(100);
            return "data";
        }

        private void Helper()
        {
            Process();
            Validate();
        }

        private void Process()
        {
            // Processing logic
        }

        private void Validate()
        {
            // Validation logic
        }

        public static ComplexClass Create(string name)
        {
            return new ComplexClass(name, 0);
        }

        public void Dispose()
        {
            // Cleanup
        }
    }
}
"#;

fn bench_simple_parsing(c: &mut Criterion) {
    let parser = CSharpParser::new();
    let path = Path::new("HelloWorld.cs");

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
    let parser = CSharpParser::new();
    let path = Path::new("ComplexClass.cs");

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
