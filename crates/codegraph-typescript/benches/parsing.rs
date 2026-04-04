// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

// TypeScript parser performance benchmarks
use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_typescript::TypeScriptParser;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

fn bench_parse_simple_functions(c: &mut Criterion) {
    let source = r#"
function add(a: number, b: number): number {
    return a + b;
}

const multiply = (x: number, y: number): number => {
    return x * y;
};

async function fetchData(url: string): Promise<string> {
    const response = await fetch(url);
    return response.text();
}
"#;

    c.bench_function("parse_simple_functions", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = TypeScriptParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.ts"), &mut graph)
                .unwrap();
        });
    });
}

fn bench_parse_class_with_methods(c: &mut Criterion) {
    let source = r#"
export class User {
    private id: string;
    public name: string;
    protected email: string;

    constructor(id: string, name: string, email: string) {
        this.id = id;
        this.name = name;
        this.email = email;
    }

    public getId(): string {
        return this.id;
    }

    public async save(): Promise<void> {
        await database.save(this);
    }

    private validate(): boolean {
        return this.email.includes('@');
    }
}
"#;

    c.bench_function("parse_class_with_methods", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = TypeScriptParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.ts"), &mut graph)
                .unwrap();
        });
    });
}

fn bench_parse_react_component(c: &mut Criterion) {
    let source = r#"
import React, { useState, useEffect } from 'react';
import { Button } from './components/Button';
import type { UserProfile } from './types';

interface Props {
    initialCount: number;
    onUpdate?: (count: number) => void;
}

export const Counter: React.FC<Props> = ({ initialCount, onUpdate }) => {
    const [count, setCount] = useState(initialCount);

    useEffect(() => {
        if (onUpdate) {
            onUpdate(count);
        }
    }, [count, onUpdate]);

    const increment = () => {
        setCount(prev => prev + 1);
    };

    return (
        <div className="counter">
            <h1>Count: {count}</h1>
            <Button onClick={increment}>
                Increment
            </Button>
        </div>
    );
};
"#;

    c.bench_function("parse_react_component", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = TypeScriptParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.tsx"), &mut graph)
                .unwrap();
        });
    });
}

fn bench_parse_complex_module(c: &mut Criterion) {
    let source = r#"
import { Connection, Query } from './database';
import * as utils from './utils';

export interface Repository<T> {
    findById(id: string): Promise<T | null>;
    save(entity: T): Promise<void>;
    delete(id: string): Promise<boolean>;
}

export class BaseRepository<T> implements Repository<T> {
    constructor(private connection: Connection) {}

    async findById(id: string): Promise<T | null> {
        const query = new Query('SELECT * FROM table WHERE id = ?', [id]);
        return this.connection.execute(query);
    }

    async save(entity: T): Promise<void> {
        await this.connection.insert(entity);
    }

    async delete(id: string): Promise<boolean> {
        const result = await this.connection.execute(
            new Query('DELETE FROM table WHERE id = ?', [id])
        );
        return result.affectedRows > 0;
    }
}

export function createRepository<T>(connection: Connection): Repository<T> {
    return new BaseRepository<T>(connection);
}
"#;

    c.bench_function("parse_complex_module", |b| {
        b.iter(|| {
            let mut graph = CodeGraph::in_memory().unwrap();
            let parser = TypeScriptParser::new();
            parser
                .parse_source(black_box(source), Path::new("benchmark.ts"), &mut graph)
                .unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_parse_simple_functions,
    bench_parse_class_with_methods,
    bench_parse_react_component,
    bench_parse_complex_module
);
criterion_main!(benches);
