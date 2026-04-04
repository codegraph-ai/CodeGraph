// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Benchmarks for PHP parsing performance

use codegraph::CodeGraph;
use codegraph_php::{CodeParser, PhpParser};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;

fn bench_parse_simple(c: &mut Criterion) {
    let source = r#"<?php
function hello(string $name): string {
    return "Hello, " . $name . "!";
}

function add(int $a, int $b): int {
    return $a + $b;
}
"#;

    c.bench_function("parse_simple_functions", |b| {
        b.iter(|| {
            let parser = PhpParser::new();
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(source), Path::new("test.php"), &mut graph)
                .unwrap()
        })
    });
}

fn bench_parse_class(c: &mut Criterion) {
    let source = r#"<?php
class Person {
    private string $name;
    private int $age;

    public function __construct(string $name, int $age) {
        $this->name = $name;
        $this->age = $age;
    }

    public function getName(): string {
        return $this->name;
    }

    public function getAge(): int {
        return $this->age;
    }

    public static function create(string $name, int $age): Person {
        return new Person($name, $age);
    }
}
"#;

    c.bench_function("parse_class_with_methods", |b| {
        b.iter(|| {
            let parser = PhpParser::new();
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(source), Path::new("test.php"), &mut graph)
                .unwrap()
        })
    });
}

fn bench_parse_complex(c: &mut Criterion) {
    let source = r#"<?php
namespace App\Services;

use App\Models\User;
use App\Contracts\AuthenticationService;
use Psr\Log\LoggerInterface;

interface Authenticatable {
    public function getIdentifier(): string;
    public function getPassword(): string;
}

trait HasApiTokens {
    private ?string $token = null;

    public function getToken(): ?string {
        return $this->token;
    }

    public function setToken(string $token): void {
        $this->token = $token;
    }
}

abstract class BaseAuthService implements AuthenticationService {
    protected LoggerInterface $logger;

    public function __construct(LoggerInterface $logger) {
        $this->logger = $logger;
    }

    abstract protected function validateCredentials(string $email, string $password): bool;

    public function authenticate(string $email, string $password): ?User {
        if ($this->validateCredentials($email, $password)) {
            return $this->findUser($email);
        }
        return null;
    }

    protected function findUser(string $email): ?User {
        return User::findByEmail($email);
    }
}

class JwtAuthService extends BaseAuthService {
    use HasApiTokens;

    protected function validateCredentials(string $email, string $password): bool {
        $user = $this->findUser($email);
        return $user && password_verify($password, $user->getPassword());
    }

    public function generateToken(User $user): string {
        $token = bin2hex(random_bytes(32));
        $this->setToken($token);
        return $token;
    }
}
"#;

    c.bench_function("parse_complex_file", |b| {
        b.iter(|| {
            let parser = PhpParser::new();
            let mut graph = CodeGraph::in_memory().unwrap();
            parser
                .parse_source(black_box(source), Path::new("test.php"), &mut graph)
                .unwrap()
        })
    });
}

criterion_group!(
    benches,
    bench_parse_simple,
    bench_parse_class,
    bench_parse_complex
);
criterion_main!(benches);
