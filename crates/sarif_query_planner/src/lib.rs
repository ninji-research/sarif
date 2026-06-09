#[cfg(feature = "capi")]
mod capi;
mod codegen;
mod optimizer;
mod plan;
#[cfg(feature = "capi")]
mod wasm_api;

pub use codegen::{CodegenError, generate_sarif};
pub use optimizer::{
    OptimizedPlan, OptimizedSelect, OptimizedStatement, Optimizer, SelectOperation,
};
pub use plan::{
    Aggregate, BinaryExpr, BinaryOp, CallExpr, Column, ExistsExpr, Expr, InExpr, Join, JoinKind,
    OrderBy, QueryPlan, SelectPlan, SortOrder, Statement,
};

#[derive(Clone, Debug, Default)]
pub struct SarifQuery {
    pub path: String,
    pub metadata: QueryMetadata,
}

#[derive(Clone, Debug, Default)]
pub struct QueryMetadata {
    pub table_count: usize,
    pub column_count: usize,
}

#[derive(Clone, Debug, Default)]
pub struct SarifPlan {
    pub query: SarifQuery,
    pub optimized: Option<OptimizedPlan>,
}

#[derive(Clone, Debug, Default)]
pub struct SarifResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub current_row: usize,
}

impl SarifResult {
    pub const fn step(&mut self) -> bool {
        self.current_row += 1;
        self.current_row <= self.rows.len()
    }

    #[must_use]
    pub const fn column_count(&self) -> usize {
        self.columns.len()
    }

    #[must_use]
    pub const fn row_count(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn column_text(&self, col: usize) -> Option<&String> {
        self.rows
            .get(self.current_row - 1)
            .and_then(|row| row.get(col))
    }

    #[must_use]
    pub fn column_int(&self, col: usize) -> Option<i64> {
        self.column_text(col).and_then(|s| s.parse().ok())
    }

    #[must_use]
    pub fn column_double(&self, col: usize) -> Option<f64> {
        self.column_text(col).and_then(|s| s.parse().ok())
    }
}

#[allow(clippy::missing_errors_doc)]
pub fn open_database(path: &str) -> Result<SarifQuery, String> {
    if !std::path::Path::new(path).exists() {
        return Err(format!("database file not found: {path}"));
    }
    Ok(SarifQuery {
        path: path.to_string(),
        metadata: QueryMetadata::default(),
    })
}

#[allow(clippy::missing_errors_doc)]
pub fn prepare_query(db: &SarifQuery, sql: &str) -> Result<SarifPlan, String> {
    let plan = parse_sql(sql)?;
    let opt_result = Optimizer.optimize(&plan);
    Ok(SarifPlan {
        query: db.clone(),
        optimized: Some(opt_result),
    })
}

fn parse_sql(sql: &str) -> Result<QueryPlan, String> {
    let sql = sql.trim();

    if !sql.to_uppercase().starts_with("SELECT") {
        return Err("only SELECT statements are supported".to_string());
    }

    let parts = sql.to_uppercase();
    let table = extract_table(&parts).unwrap_or("data.bcs");
    let where_expr = extract_where(sql);
    let limit = extract_limit(&parts);

    let columns = vec![Column {
        name: "*".to_string(),
        expr: None,
        alias: None,
    }];

    Ok(QueryPlan {
        statements: vec![Statement::Select(SelectPlan {
            table: table.to_string(),
            columns,
            where_clause: where_expr,
            joins: vec![],
            group_by: vec![],
            order_by: None,
            limit,
            subquery: None,
        })],
    })
}

fn extract_table(sql_upper: &str) -> Option<&str> {
    let from_idx = sql_upper.find("FROM ")?;
    let rest = &sql_upper[from_idx + 5..];
    let end = rest.find([' ', '\n']).unwrap_or(rest.len());
    Some(&rest[..end])
}

fn extract_where(sql: &str) -> Option<Expr> {
    let upper = sql.to_uppercase();
    let where_idx = upper.find("WHERE ")?;
    let condition_str = &sql[where_idx + 6..];
    Some(parse_expr(condition_str))
}

fn extract_limit(sql_upper: &str) -> Option<usize> {
    let limit_idx = sql_upper.find("LIMIT ")?;
    let limit_str = &sql_upper[limit_idx + 6..];
    let end = limit_str.find([' ', '\n']).unwrap_or(limit_str.len());
    limit_str[..end].parse().ok()
}

fn parse_expr(s: &str) -> Expr {
    let trimmed = s.trim();
    if trimmed.contains(" AND ") {
        let parts: Vec<&str> = trimmed.split(" AND ").collect();
        if parts.len() == 2 {
            Expr::Binary(BinaryExpr {
                left: Box::new(parse_expr(parts[0])),
                op: BinaryOp::And,
                right: Box::new(parse_expr(parts[1])),
            })
        } else {
            Expr::Identifier(trimmed.to_string())
        }
    } else if trimmed.contains(" OR ") {
        let parts: Vec<&str> = trimmed.split(" OR ").collect();
        if parts.len() == 2 {
            Expr::Binary(BinaryExpr {
                left: Box::new(parse_expr(parts[0])),
                op: BinaryOp::Or,
                right: Box::new(parse_expr(parts[1])),
            })
        } else {
            Expr::Identifier(trimmed.to_string())
        }
    } else if trimmed.contains("==") {
        let parts: Vec<&str> = trimmed.split("==").collect();
        Expr::Binary(BinaryExpr {
            left: Box::new(parse_expr(parts[0])),
            op: BinaryOp::Eq,
            right: Box::new(parse_expr(parts[1])),
        })
    } else if trimmed.contains('>') {
        let parts: Vec<&str> = trimmed.split('>').collect();
        Expr::Binary(BinaryExpr {
            left: Box::new(parse_expr(parts[0])),
            op: BinaryOp::Gt,
            right: Box::new(parse_expr(parts[1])),
        })
    } else if trimmed.starts_with('"') || trimmed.ends_with('"') {
        let text = trimmed.trim_matches('"').to_string();
        Expr::LiteralText(text)
    } else if let Ok(v) = trimmed.parse::<i64>() {
        Expr::LiteralInt(v)
    } else {
        Expr::Identifier(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_select() {
        let plan = parse_sql("SELECT * FROM users").expect("parse should succeed");
        assert_eq!(plan.statements.len(), 1);
    }

    #[test]
    fn test_parse_select_with_where() {
        let plan =
            parse_sql("SELECT id, name FROM users WHERE age > 18").expect("parse should succeed");
        let Statement::Select(select) = &plan.statements[0];
        assert!(select.where_clause.is_some());
    }

    #[test]
    fn test_parse_select_with_limit() {
        let plan = parse_sql("SELECT * FROM users LIMIT 10").expect("parse should succeed");
        let Statement::Select(select) = &plan.statements[0];
        assert_eq!(select.limit, Some(10));
    }
}
