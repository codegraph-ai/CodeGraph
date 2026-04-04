// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::CodeGraph;
use codegraph_cobol::CobolParser;
use codegraph_parser_api::CodeParser;
use criterion::{criterion_group, criterion_main, Criterion};
use std::path::Path;

const BENCH_SOURCE: &str = concat!(
    "       identification division.\n",
    "       program-id. BENCH.\n",
    "       procedure division.\n",
    "       PARA-A.\n",
    "           continue.\n",
    "       PARA-B.\n",
    "           stop run.\n",
);

fn bench_parse(c: &mut Criterion) {
    c.bench_function("parse_cobol_source", |b| {
        b.iter(|| {
            let parser = CobolParser::new();
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(BENCH_SOURCE, Path::new("bench.cob"), &mut graph)
                .unwrap();
        });
    });
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
