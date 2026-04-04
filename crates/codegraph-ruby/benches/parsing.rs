// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_ruby::RubyParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

const SIMPLE_CLASS: &str = r#"
class HelloWorld
  def main
    puts "Hello, World!"
  end
end
"#;

const COMPLEX_CLASS: &str = r#"
require 'json'
require_relative './helper'

# A complex class with multiple methods and inheritance
class ComplexClass < BaseClass
  include Serializable
  extend ClassMethods

  attr_accessor :name, :value

  def initialize(name, value)
    @name = name
    @value = value
    @items = []
  end

  def name
    @name
  end

  def value
    @value
  end

  def add_item(item)
    @items << item
  end

  def items
    @items.dup
  end

  def <=>(other)
    value <=> other.value
  end

  def to_s
    "ComplexClass(name='#{@name}', value=#{@value})"
  end

  def fetch_data
    # Simulating async operation
    "data"
  end

  private

  def helper
    process
    validate
  end

  def process
    # Processing logic
  end

  def validate
    # Validation logic
  end

  class << self
    def create(name)
      new(name, 0)
    end
  end
end
"#;

fn bench_simple_parsing(c: &mut Criterion) {
    let parser = RubyParser::new();
    let path = Path::new("hello_world.rb");

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
    let parser = RubyParser::new();
    let path = Path::new("complex_class.rb");

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
