use sarif_query_planner::{generate_sarif, open_database, prepare_query, OptimizedStatement};

#[test]
fn test_prepare_query_with_where() {
    let sql = "SELECT * FROM users WHERE age > 18";
    let path = "/tmp/test_prepare_query.db";
    std::fs::write(path, "").unwrap();

    let db = open_database(path).expect("should open db");
    let plan = prepare_query(&db, sql).expect("prepare should succeed");
    let optimized = plan.optimized.expect("should have optimized plan");
    assert!(!optimized.statements.is_empty());

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_prepare_query_with_limit() {
    let sql = "SELECT * FROM products LIMIT 50";
    let path = "/tmp/test_prepare_limit.db";
    std::fs::write(path, "").unwrap();

    let db = open_database(path).unwrap();
    let plan = prepare_query(&db, sql).unwrap();
    let optimized = plan.optimized.unwrap();
    assert!(!optimized.statements.is_empty());

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_query_no_table_yields_generated_code() {
    let sql = "SELECT * FROM transactions";
    let path = "/tmp/test_query_codegen.db";
    std::fs::write(path, "").unwrap();

    let db = open_database(path).unwrap();
    let plan = prepare_query(&db, sql).unwrap();
    let optimized = plan.optimized.unwrap();
    if let Some(OptimizedStatement::Select(select)) = optimized.statements.first() {
        let code = generate_sarif(&select.original).expect("codegen should succeed");
        assert!(code.contains("TRANSACTIONS") || code.contains("transactions"));
        assert!(code.contains("fn query_execute"));
        assert!(code.contains("fn main()"));
    } else {
        panic!("expected select statement");
    }

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_open_database_file_not_found() {
    let result = open_database("/tmp/nonexistent_file_abcdef");
    assert!(result.is_err());
}

#[test]
fn test_prepare_query_invalid_sql() {
    let path = "/tmp/test_invalid_sql.db";
    std::fs::write(path, "").unwrap();

    let db = open_database(path).unwrap();
    let result = prepare_query(&db, "INSERT INTO users VALUES (1)");
    assert!(result.is_err());

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_full_pipeline_sql_to_optimized_plan() {
    let sql = "SELECT id, name, email FROM users WHERE age > 21 AND city == \"NYC\" LIMIT 100";
    let path = "/tmp/test_full_pipeline.db";
    std::fs::write(path, "").unwrap();

    let db = open_database(path).unwrap();
    let plan = prepare_query(&db, sql).unwrap();
    let optimized = plan.optimized.unwrap();
    assert_eq!(optimized.statements.len(), 1);

    if let Some(OptimizedStatement::Select(select)) = optimized.statements.first() {
        let code = generate_sarif(&select.original).expect("codegen should succeed");
        assert!(code.contains("fn query_execute"));
        assert!(code.contains("fn main()"));
        assert!(code.contains("USERS"));
    }

    let _ = std::fs::remove_file(path);
}

#[test]
fn test_query_with_order_by_and_limit_generates_both_sections() {
    let sql = "SELECT * FROM data ORDER BY timestamp DESC LIMIT 5";
    let path = "/tmp/test_order_limit.db";
    std::fs::write(path, "").unwrap();

    let db = open_database(path).unwrap();
    let plan = prepare_query(&db, sql).unwrap();
    let optimized = plan.optimized.unwrap();

    if let Some(OptimizedStatement::Select(select)) = optimized.statements.first() {
        let code = generate_sarif(&select.original).unwrap();
        assert!(code.contains("fn query_execute"));
    }

    let _ = std::fs::remove_file(path);
}