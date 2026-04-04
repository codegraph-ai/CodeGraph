// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::CodeGraph;
use codegraph_fortran::{CodeParser, FortranParser};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

fn bench_parse_program(c: &mut Criterion) {
    let parser = FortranParser::new();
    let source = r#"program benchmark
  use iso_fortran_env, only: int32, real64
  implicit none
  integer(int32) :: i, n
  real(real64) :: sum

  n = 1000
  sum = 0.0
  do i = 1, n
    sum = sum + real(i, real64)
  end do
  print *, 'Sum:', sum
end program benchmark
"#;

    c.bench_function("parse_fortran_program", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(
                    black_box(source),
                    black_box(Path::new("benchmark.f90")),
                    &mut graph,
                )
                .unwrap()
        })
    });
}

criterion_group!(benches, bench_parse_program);
criterion_main!(benches);
