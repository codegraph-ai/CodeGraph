// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for COBOL source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use codegraph_parser_api::{CallRelation, FunctionEntity};

use crate::visitor::CobolVisitor;

/// Extract code entities and relationships from COBOL source code.
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = crate::ts_cobol::language();
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    // Note: NOT checking root_node.has_error() — COBOL dialects and complex
    // preprocessor directives can produce partial error nodes in the grammar
    // while still containing extractable entities.
    let root_node = tree.root_node();

    let mut ir = CodeIR::new(file_path.to_path_buf());

    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "cobol".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = CobolVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    // Map COBOL programs to classes and paragraphs to functions
    ir.classes = visitor.programs;
    ir.functions = visitor.paragraphs;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    // Source-level extraction for constructs the grammar doesn't parse:
    // GO TO and EXEC CICS XCTL/LINK
    extract_goto_and_cics(source, &ir.functions, &mut ir.calls);

    // EXEC SQL table references
    extract_exec_sql(source, &ir.functions, &mut ir.calls);

    Ok(ir)
}

/// Source-level extraction for GO TO and EXEC CICS constructs.
///
/// The tree-sitter COBOL grammar doesn't parse GO TO as a distinct node type
/// and doesn't understand CICS extensions at all. We extract these from source
/// text directly.
fn extract_goto_and_cics(
    source: &str,
    paragraphs: &[FunctionEntity],
    calls: &mut Vec<CallRelation>,
) {
    for (line_num, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        let line_1indexed = line_num + 1;

        // Find which paragraph this line belongs to
        let caller = paragraphs
            .iter()
            .rfind(|p| line_1indexed >= p.line_start && line_1indexed <= p.line_end)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "file".to_string());

        // GO TO paragraph-name
        if let Some(target) = trimmed
            .strip_prefix("GO TO ")
            .or_else(|| trimmed.strip_prefix("GO  TO "))
            .or_else(|| trimmed.strip_prefix("go to "))
        {
            let target = target.trim().trim_end_matches('.');
            if !target.is_empty() && !target.contains(' ') {
                calls.push(CallRelation::new(
                    caller.clone(),
                    target.to_string(),
                    line_1indexed,
                ));
            }
        }

        // EXEC CICS XCTL PROGRAM('name') or EXEC CICS LINK PROGRAM('name')
        if (trimmed.contains("EXEC CICS XCTL") || trimmed.contains("EXEC CICS LINK"))
            && trimmed.contains("PROGRAM")
        {
            // Program name might be on this line or next
            if let Some(prog) = extract_cics_program_name(trimmed) {
                calls.push(CallRelation::new(caller.clone(), prog, line_1indexed));
            }
        }
        // PROGRAM clause might be on the next line after EXEC CICS XCTL
        if trimmed.starts_with("PROGRAM") && trimmed.contains('(') {
            if let Some(prog) = extract_cics_program_name(trimmed) {
                calls.push(CallRelation::new(caller.clone(), prog, line_1indexed));
            }
        }
    }
}

/// Extract program name from CICS PROGRAM clause.
/// Handles: PROGRAM('MYPROG'), PROGRAM(WS-PROGNAME), PROGRAM (CDEMO-TO-PROGRAM)
fn extract_cics_program_name(text: &str) -> Option<String> {
    let idx = text.find("PROGRAM")?;
    let rest = &text[idx + 7..];
    let rest = rest.trim();
    let rest = rest.strip_prefix('(')?;
    let end = rest.find(')')?;
    let name = rest[..end].trim().trim_matches('\'').trim_matches('"');
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Source-level extraction for EXEC SQL embedded SQL statements.
///
/// COBOL programs embed SQL via `EXEC SQL ... END-EXEC`. This function
/// collects multi-line SQL blocks and extracts table names from common
/// DML patterns (SELECT/FROM, INSERT INTO, UPDATE, DELETE FROM).
/// Each table reference creates a CallRelation with the callee prefixed
/// by `SQL:` to distinguish from regular paragraph calls.
fn extract_exec_sql(source: &str, paragraphs: &[FunctionEntity], calls: &mut Vec<CallRelation>) {
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim().to_uppercase();

        // Look for EXEC SQL (but not EXEC CICS or other EXEC variants)
        if trimmed.contains("EXEC SQL") && !trimmed.contains("EXEC CICS") {
            let start_line = i + 1; // 1-indexed

            // Accumulate the full SQL block until END-EXEC
            let mut sql_text = String::new();
            let mut j = i;
            loop {
                let line_upper = lines[j].trim().to_uppercase();
                sql_text.push(' ');
                sql_text.push_str(lines[j].trim());

                if line_upper.contains("END-EXEC") {
                    break;
                }
                j += 1;
                if j >= lines.len() {
                    break;
                }
            }

            // Find which paragraph this EXEC SQL belongs to
            let caller = paragraphs
                .iter()
                .rfind(|p| start_line >= p.line_start && start_line <= p.line_end)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "file".to_string());

            // Extract table names from the accumulated SQL
            for table in extract_sql_table_names(&sql_text) {
                calls.push(CallRelation::new(
                    caller.clone(),
                    format!("SQL:{table}"),
                    start_line,
                ));
            }

            i = j + 1;
        } else {
            i += 1;
        }
    }
}

/// Extract table names from a SQL statement string.
///
/// Handles these patterns (case-insensitive):
/// - `SELECT ... FROM table_name`
/// - `INSERT INTO table_name`
/// - `UPDATE table_name`
/// - `DELETE FROM table_name`
///
/// Returns deduplicated table names in uppercase.
fn extract_sql_table_names(sql: &str) -> Vec<String> {
    let upper = sql.to_uppercase();
    let tokens: Vec<&str> = upper.split_whitespace().collect();
    let mut tables = Vec::new();

    for (idx, token) in tokens.iter().enumerate() {
        match *token {
            "FROM" => {
                // SELECT ... FROM table or DELETE FROM table
                // Skip if followed by SQL keywords or if it's part of non-DML context
                if let Some(next) = tokens.get(idx + 1) {
                    if is_valid_table_name(next) {
                        tables.push(next.to_string());
                    }
                }
            }
            "INTO" => {
                // INSERT INTO table
                // Check that the previous token is INSERT
                if idx > 0 && tokens[idx - 1] == "INSERT" {
                    if let Some(next) = tokens.get(idx + 1) {
                        if is_valid_table_name(next) {
                            tables.push(next.to_string());
                        }
                    }
                }
            }
            "UPDATE" => {
                if let Some(next) = tokens.get(idx + 1) {
                    if is_valid_table_name(next) {
                        tables.push(next.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    tables.retain(|t| seen.insert(t.clone()));
    tables
}

/// Check if a token looks like a valid SQL table name (not a keyword or punctuation).
fn is_valid_table_name(token: &str) -> bool {
    // Strip trailing commas, periods, parentheses
    let clean = token.trim_matches(|c: char| c == ',' || c == '.' || c == '(' || c == ')');
    if clean.is_empty() {
        return false;
    }

    // Reject SQL keywords that commonly follow FROM/INTO/UPDATE
    const SQL_KEYWORDS: &[&str] = &[
        "SELECT", "WHERE", "SET", "VALUES", "INTO", "JOIN", "LEFT", "RIGHT", "INNER", "OUTER",
        "CROSS", "ON", "AND", "OR", "NOT", "NULL", "AS", "ORDER", "GROUP", "BY", "HAVING", "UNION",
        "ALL", "DISTINCT", "EXISTS", "IN", "BETWEEN", "LIKE", "IS", "END-EXEC", "EXEC", "SQL",
        "WITH", "CURSOR", "FOR", "DECLARE", "OPEN", "CLOSE", "FETCH", "INCLUDE",
    ];
    !SQL_KEYWORDS.contains(&clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_COBOL: &str = concat!(
        "       identification division.\n",
        "       program-id. MINIMAL.\n",
        "       procedure division.\n",
        "       stop run.\n",
    );

    const COBOL_WITH_PARAGRAPH: &str = concat!(
        "       identification division.\n",
        "       program-id. MYPROG.\n",
        "       procedure division.\n",
        "       MAIN-PARA.\n",
        "           stop run.\n",
    );

    const COBOL_WITH_COPY: &str = concat!(
        "       identification division.\n",
        "       program-id. COPYPROG.\n",
        "       data division.\n",
        "       working-storage section.\n",
        "       copy MYBOOK.\n",
        "       procedure division.\n",
        "       stop run.\n",
    );

    #[test]
    fn test_extract_minimal_cobol() {
        let config = ParserConfig::default();
        let result = extract(MINIMAL_COBOL, Path::new("minimal.cob"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "MINIMAL");
    }

    #[test]
    fn test_extract_program_name() {
        let config = ParserConfig::default();
        let result = extract(MINIMAL_COBOL, Path::new("test.cob"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "test");
        assert_eq!(module.language, "cobol");
    }

    #[test]
    fn test_extract_paragraph() {
        let config = ParserConfig::default();
        let result = extract(COBOL_WITH_PARAGRAPH, Path::new("para.cob"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "MAIN-PARA");
        assert_eq!(ir.functions[0].parent_class, Some("MYPROG".to_string()));
    }

    #[test]
    fn test_extract_copy_statement() {
        let config = ParserConfig::default();
        let result = extract(COBOL_WITH_COPY, Path::new("copy.cob"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert!(!ir.imports.is_empty(), "Expected COPY import");
        assert_eq!(ir.imports[0].imported, "MYBOOK");
    }

    #[test]
    fn test_extract_module_line_count() {
        let config = ParserConfig::default();
        let result = extract(MINIMAL_COBOL, Path::new("count.cob"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        let module = ir.module.unwrap();
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_multiple_paragraphs() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. MULTI.\n",
            "       procedure division.\n",
            "       PARA-A.\n",
            "           continue.\n",
            "       PARA-B.\n",
            "           stop run.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("multi.cob"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.functions.len(), 2);
        let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(
            names.contains(&"PARA-A"),
            "Missing PARA-A, got: {:?}",
            names
        );
        assert!(
            names.contains(&"PARA-B"),
            "Missing PARA-B, got: {:?}",
            names
        );
    }

    #[test]
    fn test_extract_perform_calls() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. PERFTEST.\n",
            "       procedure division.\n",
            "       MAIN-PARA.\n",
            "           perform INIT-PARA\n",
            "           perform PROCESS-DATA\n",
            "           stop run.\n",
            "       INIT-PARA.\n",
            "           display 'init'.\n",
            "       PROCESS-DATA.\n",
            "           display 'process'.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("perf.cob"), &config);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();

        eprintln!(
            "Functions: {:?}",
            ir.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
        eprintln!(
            "Calls: {:?}",
            ir.calls
                .iter()
                .map(|c| format!("{} -> {}", c.caller, c.callee))
                .collect::<Vec<_>>()
        );

        assert!(
            !ir.calls.is_empty(),
            "Expected PERFORM calls to be extracted"
        );

        let callees: Vec<&str> = ir.calls.iter().map(|c| c.callee.as_str()).collect();
        assert!(
            callees.contains(&"INIT-PARA"),
            "Expected PERFORM INIT-PARA. Got: {:?}",
            callees
        );
        assert!(
            callees.contains(&"PROCESS-DATA"),
            "Expected PERFORM PROCESS-DATA. Got: {:?}",
            callees
        );
    }

    #[test]
    fn test_extract_goto_and_cics() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. GOTEST.\n",
            "       procedure division.\n",
            "       MAIN-PARA.\n",
            "           GO TO EXIT-PARA\n",
            "           stop run.\n",
            "       EXIT-PARA.\n",
            "           exit.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("goto.cob"), &config);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();

        let callees: Vec<&str> = ir.calls.iter().map(|c| c.callee.as_str()).collect();
        eprintln!("GO TO calls: {:?}", callees);
        assert!(
            callees.contains(&"EXIT-PARA"),
            "Expected GO TO EXIT-PARA. Got: {:?}",
            callees
        );
    }

    #[test]
    fn test_extract_perform_varying() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. VARTEST.\n",
            "       procedure division.\n",
            "       MAIN-PARA.\n",
            "           perform PROCESS-LOOP varying WS-I from 1 by 1\n",
            "              until WS-I > 10\n",
            "           stop run.\n",
            "       PROCESS-LOOP.\n",
            "           display 'hello'.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("vary.cob"), &config);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();

        let callees: Vec<&str> = ir.calls.iter().map(|c| c.callee.as_str()).collect();
        eprintln!("PERFORM VARYING calls: {:?}", callees);

        assert!(
            callees.contains(&"PROCESS-LOOP"),
            "Expected PERFORM target PROCESS-LOOP, not VARYING/WS-I. Got: {:?}",
            callees
        );
        assert!(
            !callees.contains(&"VARYING"),
            "Should NOT extract VARYING as callee. Got: {:?}",
            callees
        );
    }

    #[test]
    fn test_extract_exec_sql_select() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. SQLTEST.\n",
            "       procedure division.\n",
            "       MAIN-PARA.\n",
            "           EXEC SQL\n",
            "               SELECT EMPNO, ENAME\n",
            "               FROM EMPLOYEE\n",
            "               WHERE DEPTNO = :WS-DEPT\n",
            "           END-EXEC\n",
            "           stop run.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("sql.cob"), &config);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();

        let callees: Vec<&str> = ir.calls.iter().map(|c| c.callee.as_str()).collect();
        eprintln!("SQL calls: {:?}", callees);
        assert!(
            callees.contains(&"SQL:EMPLOYEE"),
            "Expected SQL:EMPLOYEE. Got: {:?}",
            callees
        );
    }

    #[test]
    fn test_extract_exec_sql_insert() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. SQLINS.\n",
            "       procedure division.\n",
            "       INSERT-PARA.\n",
            "           EXEC SQL\n",
            "               INSERT INTO ORDERS\n",
            "               VALUES (:WS-ID, :WS-AMT)\n",
            "           END-EXEC\n",
            "           stop run.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("sqlins.cob"), &config);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();

        let callees: Vec<&str> = ir.calls.iter().map(|c| c.callee.as_str()).collect();
        assert!(
            callees.contains(&"SQL:ORDERS"),
            "Expected SQL:ORDERS. Got: {:?}",
            callees
        );
    }

    #[test]
    fn test_extract_exec_sql_update() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. SQLUPD.\n",
            "       procedure division.\n",
            "       UPDATE-PARA.\n",
            "           EXEC SQL\n",
            "               UPDATE CUSTOMER\n",
            "               SET STATUS = 'A'\n",
            "               WHERE CUSTID = :WS-ID\n",
            "           END-EXEC\n",
            "           stop run.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("sqlupd.cob"), &config);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();

        let callees: Vec<&str> = ir.calls.iter().map(|c| c.callee.as_str()).collect();
        assert!(
            callees.contains(&"SQL:CUSTOMER"),
            "Expected SQL:CUSTOMER. Got: {:?}",
            callees
        );
    }

    #[test]
    fn test_extract_exec_sql_delete() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. SQLDEL.\n",
            "       procedure division.\n",
            "       DELETE-PARA.\n",
            "           EXEC SQL\n",
            "               DELETE FROM TEMP_DATA\n",
            "               WHERE CREATED < :WS-DATE\n",
            "           END-EXEC\n",
            "           stop run.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("sqldel.cob"), &config);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();

        let callees: Vec<&str> = ir.calls.iter().map(|c| c.callee.as_str()).collect();
        assert!(
            callees.contains(&"SQL:TEMP_DATA"),
            "Expected SQL:TEMP_DATA. Got: {:?}",
            callees
        );
    }

    #[test]
    fn test_extract_exec_sql_single_line() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. SQLONE.\n",
            "       procedure division.\n",
            "       MAIN-PARA.\n",
            "           EXEC SQL SELECT COUNT(*) FROM ACCOUNTS END-EXEC\n",
            "           stop run.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("sqlone.cob"), &config);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();

        let callees: Vec<&str> = ir.calls.iter().map(|c| c.callee.as_str()).collect();
        assert!(
            callees.contains(&"SQL:ACCOUNTS"),
            "Expected SQL:ACCOUNTS. Got: {:?}",
            callees
        );
    }

    #[test]
    fn test_extract_exec_sql_multiple_tables() {
        let source = concat!(
            "       identification division.\n",
            "       program-id. SQLMULTI.\n",
            "       procedure division.\n",
            "       PARA-A.\n",
            "           EXEC SQL\n",
            "               SELECT A.ID FROM ORDERS\n",
            "               WHERE A.ID = :WS-ID\n",
            "           END-EXEC\n",
            "       PARA-B.\n",
            "           EXEC SQL\n",
            "               UPDATE INVENTORY\n",
            "               SET QTY = :WS-QTY\n",
            "           END-EXEC\n",
            "           stop run.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("sqlmulti.cob"), &config);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();

        let callees: Vec<&str> = ir.calls.iter().map(|c| c.callee.as_str()).collect();
        eprintln!("Multi-SQL calls: {:?}", callees);
        assert!(
            callees.contains(&"SQL:ORDERS"),
            "Expected SQL:ORDERS. Got: {:?}",
            callees
        );
        assert!(
            callees.contains(&"SQL:INVENTORY"),
            "Expected SQL:INVENTORY. Got: {:?}",
            callees
        );
    }

    #[test]
    fn test_extract_exec_sql_ignores_non_dml() {
        // EXEC SQL INCLUDE and EXEC SQL DECLARE should not produce table calls
        let source = concat!(
            "       identification division.\n",
            "       program-id. SQLMISC.\n",
            "       procedure division.\n",
            "       MAIN-PARA.\n",
            "           EXEC SQL INCLUDE SQLCA END-EXEC\n",
            "           stop run.\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("sqlmisc.cob"), &config);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();

        let sql_calls: Vec<&str> = ir
            .calls
            .iter()
            .filter(|c| c.callee.starts_with("SQL:"))
            .map(|c| c.callee.as_str())
            .collect();
        assert!(
            sql_calls.is_empty(),
            "EXEC SQL INCLUDE should not produce SQL: calls. Got: {:?}",
            sql_calls
        );
    }

    #[test]
    fn test_extract_sql_table_names_helper() {
        assert_eq!(
            extract_sql_table_names("SELECT A, B FROM MYTABLE WHERE X = 1"),
            vec!["MYTABLE"]
        );
        assert_eq!(
            extract_sql_table_names("INSERT INTO ORDERS VALUES (1, 2)"),
            vec!["ORDERS"]
        );
        assert_eq!(
            extract_sql_table_names("UPDATE CUSTOMER SET X = 1"),
            vec!["CUSTOMER"]
        );
        assert_eq!(
            extract_sql_table_names("DELETE FROM TEMP_DATA WHERE Y = 2"),
            vec!["TEMP_DATA"]
        );
    }
}
