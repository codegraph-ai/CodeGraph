// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_tcl::TclParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

const SAMPLE_TCL: &str = r#"
package require Tcl 8.6

namespace eval synth {
    proc read_design {verilog_file lib_file} {
        read_verilog $verilog_file
        read_liberty $lib_file
        link_design
    }

    proc run_synthesis {} {
        compile
        report_timing
        report_area
    }
}

create_clock -name clk -period 10 [get_ports clk_in]
set_input_delay -clock clk 0.5 [all_inputs]
set_output_delay -clock clk 0.5 [all_outputs]
set_false_path -from [get_clocks async_clk] -to [get_clocks clk]

synth::read_design design.v lib.db
synth::run_synthesis
write_def output.def
"#;

fn bench_parse_tcl(c: &mut Criterion) {
    let parser = TclParser::new();

    c.bench_function("parse_tcl_source", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(SAMPLE_TCL), Path::new("bench.tcl"), &mut graph)
                .unwrap();
        });
    });
}

criterion_group!(benches, bench_parse_tcl);
criterion_main!(benches);
