// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Diagnostic test that dumps the tree-sitter AST for a representative Dockerfile.
//! Run with: `cargo test -p codegraph-dockerfile --test dump_ast -- --nocapture --ignored`

#[test]
#[ignore]
fn dump_ast() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&codegraph_dockerfile::ts_dockerfile_language())
        .unwrap();
    let source = r#"FROM python:3.11
USER root
WORKDIR /app
ARG SECRET=hardcoded
ENV API_KEY=abc123
EXPOSE 22
EXPOSE 8080
ADD https://example.com/file.sh /app/
COPY . /app
RUN pip install -r requirements.txt
CMD ["python", "app.py"]
"#;
    let tree = parser.parse(source, None).unwrap();
    println!("{}", tree.root_node().to_sexp());
}
