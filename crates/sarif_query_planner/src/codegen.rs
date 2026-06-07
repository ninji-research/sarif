use std::fmt::Write;

use crate::plan::{BinaryExpr, BinaryOp, Expr, SelectPlan};

#[allow(clippy::missing_errors_doc)]
pub fn generate_sarif(plan: &SelectPlan) -> Result<String, CodegenError> {
    let mut output = String::new();

    generate_text_search(&mut output, plan)?;

    Ok(output)
}

#[allow(clippy::unnecessary_wraps)]
fn generate_text_search(output: &mut String, plan: &SelectPlan) -> Result<(), CodegenError> {
    writeln!(output, "fn query_execute(db: Text) -> Text {{")
        .expect("writing to string cannot fail");
    writeln!(output, " let lines = text_split(db, \"\\n\")")
        .expect("writing to string cannot fail");
    writeln!(output, " let result = text_builder_new()").expect("writing to string cannot fail");

    if let Some(ref where_expr) = plan.where_clause {
        writeln!(output, " let filtered = repeat {{").expect("writing to string cannot fail");
        generate_where_filter(output, where_expr, plan);
        writeln!(output, " }} in lines").expect("writing to string cannot fail");
    } else {
        writeln!(output, " let filtered = lines").expect("writing to string cannot fail");
    }

    if let Some(limit) = plan.limit {
        writeln!(output, " let limited = repeat {{ i32 }} in 0..{limit}")
            .expect("writing to string cannot fail");
    }

    if plan.order_by.is_some() {
        writeln!(output, " let sorted = text_sort(filtered)")
            .expect("writing to string cannot fail");
    } else {
        writeln!(output, " let sorted = filtered").expect("writing to string cannot fail");
    }

    writeln!(output, " text_builder_finish(result)").expect("writing to string cannot fail");
    writeln!(output, "}}").expect("writing to string cannot fail");

    writeln!(output, "fn main() -> Text {{").expect("writing to string cannot fail");
    writeln!(output, " let db = file_mmap(\"{}\")", plan.table)
        .expect("writing to string cannot fail");
    writeln!(output, " query_execute(db)").expect("writing to string cannot fail");
    writeln!(output, "}}").expect("writing to string cannot fail");

    Ok(())
}

fn generate_where_filter(output: &mut String, expr: &Expr, _plan: &SelectPlan) {
    match expr {
        Expr::Binary(bin) => {
            generate_binary_filter(output, bin);
        }
        Expr::In(in_expr) => {
            generate_in_filter(output, in_expr);
        }
        Expr::Exists(exists) => {
            generate_exists_filter(output, exists);
        }
        _ => {}
    }
}

fn generate_binary_filter(output: &mut String, expr: &BinaryExpr) {
    match expr.op {
        BinaryOp::Eq => {
            writeln!(output, " if text_contains(line, lhs) {{")
                .expect("writing to string cannot fail");
            writeln!(output, " text_builder_append(result, line)")
                .expect("writing to string cannot fail");
            writeln!(output, " }}").expect("writing to string cannot fail");
        }
        BinaryOp::Gt => {
            writeln!(output, " if text_len(line) > rhs {{").expect("writing to string cannot fail");
            writeln!(output, " text_builder_append(result, line)")
                .expect("writing to string cannot fail");
            writeln!(output, " }}").expect("writing to string cannot fail");
        }
        _ => {
            writeln!(output, " text_builder_append(result, line)")
                .expect("writing to string cannot fail");
        }
    }
}

fn generate_in_filter(output: &mut String, _expr: &crate::plan::InExpr) {
    writeln!(output, " text_builder_append(result, line)").expect("writing to string cannot fail");
}

fn generate_exists_filter(output: &mut String, _expr: &crate::plan::ExistsExpr) {
    writeln!(output, " text_builder_append(result, line)").expect("writing to string cannot fail");
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodegenError {
    pub message: String,
}

impl CodegenError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "codegen error: {}", self.message)
    }
}

impl std::error::Error for CodegenError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Column;

    #[test]
    fn test_generate_simple_select() {
        let plan = SelectPlan {
            table: "data.bcs".to_string(),
            columns: vec![Column {
                name: "value".to_string(),
                expr: None,
                alias: None,
            }],
            where_clause: None,
            joins: vec![],
            group_by: vec![],
            order_by: None,
            limit: None,
            subquery: None,
        };

        let code = generate_sarif(&plan);
        assert!(code.is_ok());
    }
}
