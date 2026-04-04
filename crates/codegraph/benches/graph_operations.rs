// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::{CodeGraph, EdgeType, NodeType};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use tempfile::TempDir;

fn bench_node_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("node_lookup");

    for size in [1000, 10_000, 100_000].iter() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("bench.graph");
        let mut graph = CodeGraph::open(&db_path).unwrap();

        // Populate graph
        let node_ids: Vec<_> = (0..*size)
            .map(|i| {
                graph
                    .add_node(
                        NodeType::Function,
                        vec![("name".to_string(), format!("func_{i}").into())]
                            .into_iter()
                            .collect(),
                    )
                    .unwrap()
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("lookup", size), size, |b, _| {
            let node_id = node_ids[node_ids.len() / 2];
            b.iter(|| {
                black_box(graph.get_node(node_id).unwrap());
            });
        });
    }

    group.finish();
}

fn bench_neighbor_queries(c: &mut Criterion) {
    let mut group = c.benchmark_group("neighbor_queries");

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("bench.graph");
    let mut graph = CodeGraph::open(&db_path).unwrap();

    // Create a node with varying numbers of neighbors
    let center_node = graph
        .add_node(NodeType::Function, Default::default())
        .unwrap();

    for num_neighbors in [10, 100, 1000].iter() {
        for _i in 0..*num_neighbors {
            let neighbor = graph
                .add_node(NodeType::Function, Default::default())
                .unwrap();
            graph
                .add_edge(center_node, neighbor, EdgeType::Calls, Default::default())
                .unwrap();
        }

        group.bench_with_input(
            BenchmarkId::new("get_neighbors", num_neighbors),
            num_neighbors,
            |b, _| {
                b.iter(|| {
                    black_box(
                        graph
                            .get_neighbors(center_node, codegraph::Direction::Outgoing)
                            .unwrap(),
                    );
                });
            },
        );
    }

    group.finish();
}

fn bench_batch_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_insert");

    for size in [100, 1000, 10_000].iter() {
        group.bench_with_input(BenchmarkId::new("nodes", size), size, |b, &size| {
            b.iter_with_setup(
                || {
                    let temp_dir = TempDir::new().unwrap();
                    let db_path = temp_dir.path().join("bench.graph");
                    let graph = CodeGraph::open(&db_path).unwrap();
                    (graph, temp_dir)
                },
                |(mut graph, _temp_dir)| {
                    let nodes: Vec<_> = (0..size)
                        .map(|i| {
                            (
                                NodeType::Function,
                                vec![("name".to_string(), format!("func_{i}").into())]
                                    .into_iter()
                                    .collect(),
                            )
                        })
                        .collect();
                    black_box(graph.add_nodes_batch(nodes).unwrap());
                },
            );
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_node_lookup,
    bench_neighbor_queries,
    bench_batch_insert
);
criterion_main!(benches);
