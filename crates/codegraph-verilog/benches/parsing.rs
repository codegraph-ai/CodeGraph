// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_verilog::VerilogParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

fn bench_parse_module(c: &mut Criterion) {
    let parser = VerilogParser::new();
    let source = r#"
module counter (
    input clk,
    input reset,
    output reg [7:0] count
);
    always @(posedge clk) begin
        if (reset)
            count <= 8'b0;
        else
            count <= count + 1;
    end
endmodule
"#;

    c.bench_function("parse_verilog_module", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let _ = parser.parse_source(black_box(source), Path::new("counter.v"), &mut graph);
        })
    });
}

criterion_group!(benches, bench_parse_module);
criterion_main!(benches);
