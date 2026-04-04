// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

// Go parser performance benchmarks
use codegraph::CodeGraph;
use codegraph_go::GoParser;
use codegraph_parser_api::CodeParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

fn bench_parse_simple_functions(c: &mut Criterion) {
    let source = r#"
package main

import "fmt"

func add(a, b int) int {
    return a + b
}

func multiply(x, y int) int {
    return x * y
}

func printSum(a, b int) {
    fmt.Println(add(a, b))
}
"#;

    c.bench_function("parse_simple_functions", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = GoParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.go"), &mut graph)
                .unwrap();
        });
    });
}

fn bench_parse_struct_and_methods(c: &mut Criterion) {
    let source = r#"
package main

type Point struct {
    X int
    Y int
}

func NewPoint(x, y int) *Point {
    return &Point{X: x, Y: y}
}

func (p *Point) Distance(other *Point) float64 {
    dx := float64(p.X - other.X)
    dy := float64(p.Y - other.Y)
    return math.Sqrt(dx*dx + dy*dy)
}

func (p Point) String() string {
    return fmt.Sprintf("(%d, %d)", p.X, p.Y)
}
"#;

    c.bench_function("parse_struct_and_methods", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = GoParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.go"), &mut graph)
                .unwrap();
        });
    });
}

fn bench_parse_interface_and_impl(c: &mut Criterion) {
    let source = r#"
package main

import (
    "fmt"
    "sync"
)

type Storage interface {
    Get(key string) (string, error)
    Set(key, value string) error
    Delete(key string) error
}

type InMemoryStorage struct {
    data map[string]string
    mu   sync.RWMutex
}

func NewInMemoryStorage() *InMemoryStorage {
    return &InMemoryStorage{
        data: make(map[string]string),
    }
}

func (s *InMemoryStorage) Get(key string) (string, error) {
    s.mu.RLock()
    defer s.mu.RUnlock()

    value, ok := s.data[key]
    if !ok {
        return "", fmt.Errorf("key not found: %s", key)
    }
    return value, nil
}

func (s *InMemoryStorage) Set(key, value string) error {
    s.mu.Lock()
    defer s.mu.Unlock()

    s.data[key] = value
    return nil
}

func (s *InMemoryStorage) Delete(key string) error {
    s.mu.Lock()
    defer s.mu.Unlock()

    delete(s.data, key)
    return nil
}
"#;

    c.bench_function("parse_interface_and_impl", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = GoParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.go"), &mut graph)
                .unwrap();
        });
    });
}

fn bench_parse_complex_package(c: &mut Criterion) {
    let source = r#"
package server

import (
    "context"
    "fmt"
    "net/http"
    "time"

    "github.com/gorilla/mux"
)

type Config struct {
    Port         int
    ReadTimeout  time.Duration
    WriteTimeout time.Duration
}

type Server struct {
    router *mux.Router
    config Config
    server *http.Server
}

func NewServer(config Config) *Server {
    router := mux.NewRouter()

    return &Server{
        router: router,
        config: config,
        server: &http.Server{
            Addr:         fmt.Sprintf(":%d", config.Port),
            Handler:      router,
            ReadTimeout:  config.ReadTimeout,
            WriteTimeout: config.WriteTimeout,
        },
    }
}

func (s *Server) Start(ctx context.Context) error {
    go func() {
        <-ctx.Done()
        s.Stop()
    }()

    return s.server.ListenAndServe()
}

func (s *Server) Stop() error {
    ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
    defer cancel()

    return s.server.Shutdown(ctx)
}

func (s *Server) RegisterHandler(path string, handler http.HandlerFunc) {
    s.router.HandleFunc(path, handler)
}
"#;

    c.bench_function("parse_complex_package", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = GoParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.go"), &mut graph)
                .unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_parse_simple_functions,
    bench_parse_struct_and_methods,
    bench_parse_interface_and_impl,
    bench_parse_complex_package
);
criterion_main!(benches);
