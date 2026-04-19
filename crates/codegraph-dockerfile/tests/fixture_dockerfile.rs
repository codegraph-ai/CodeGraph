// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! End-to-end test against the security-fixture Dockerfile to confirm the
//! parser captures every directive the IaC scanner relies on.

use codegraph::CodeGraph;
use codegraph_dockerfile::DockerfileParser;
use codegraph_parser_api::CodeParser;
use std::path::Path;

const SAMPLE: &str = r#"# Intentionally vulnerable Dockerfile for security_scan_iac testing
FROM python:latest

# CWE-250: Running as root (no USER directive)
WORKDIR /app

# CWE-829: ADD with URL instead of COPY
ADD https://example.com/malicious.sh /app/setup.sh

# Hardcoded secrets in build args
ARG DB_PASSWORD=admin123
ENV API_KEY=sk-1234567890abcdef
ENV AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY

COPY . /app
RUN pip install -r requirements.txt

# Exposing remote access ports
EXPOSE 22
EXPOSE 3389
EXPOSE 8080

# Privileged execution
USER root

CMD ["python", "app.py", "--debug"]
"#;

#[test]
fn parses_full_security_fixture() {
    let parser = DockerfileParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();
    let info = parser
        .parse_source(SAMPLE, Path::new("Dockerfile"), &mut graph)
        .expect("parse should succeed");

    // Expect at least one node per directive line present in the fixture.
    // 1 FROM, 1 WORKDIR, 1 ADD, 1 ARG, 2 ENV, 1 COPY, 1 RUN, 3 EXPOSE, 1 USER, 1 CMD = 13
    assert!(
        info.functions.len() >= 13,
        "expected >=13 directive nodes, got {}",
        info.functions.len()
    );
}
