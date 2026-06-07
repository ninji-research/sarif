use crate::plan::{QueryPlan, SelectPlan, Statement};
use std::fmt::Write;

#[derive(Clone, Debug, Default)]
pub struct Optimizer;

impl Optimizer {
    #[must_use]
    pub fn optimize(&self, plan: &QueryPlan) -> OptimizedPlan {
        let optimized_statements: Vec<OptimizedStatement> = plan
            .statements
            .iter()
            .map(|stmt| self.optimize_statement(stmt))
            .collect();

        OptimizedPlan {
            statements: optimized_statements,
        }
    }

    fn optimize_statement(&self, stmt: &Statement) -> OptimizedStatement {
        match stmt {
            Statement::Select(select) => {
                let optimized_select = self.optimize_select(select);
                OptimizedStatement::Select(optimized_select)
            }
        }
    }

    fn optimize_select(&self, plan: &SelectPlan) -> OptimizedSelect {
        let select_order = self.determine_select_order(plan);
        OptimizedSelect {
            original: plan.clone(),
            execution_order: select_order,
        }
    }

    #[allow(clippy::unused_self)]
    fn determine_select_order(&self, plan: &SelectPlan) -> Vec<SelectOperation> {
        let mut order = Vec::new();

        if plan.subquery.is_some() {
            order.push(SelectOperation::Subquery);
        }

        order.push(SelectOperation::TableScan);

        if plan.where_clause.is_some() {
            order.push(SelectOperation::Filter);
        }

        if !plan.joins.is_empty() {
            order.push(SelectOperation::Joins);
        }

        if !plan.group_by.is_empty() {
            order.push(SelectOperation::GroupBy);
            order.push(SelectOperation::Aggregate);
        }

        if plan.order_by.is_some() {
            order.push(SelectOperation::OrderBy);
        }

        if plan.limit.is_some() {
            order.push(SelectOperation::Limit);
        }

        order
    }
}

#[derive(Clone, Debug, Default)]
pub struct OptimizedPlan {
    pub statements: Vec<OptimizedStatement>,
}

impl OptimizedPlan {
    #[must_use]
    pub fn pretty(&self) -> String {
        let mut output = String::from("OPTIMIZED PLAN\n");
        for stmt in &self.statements {
            match stmt {
                OptimizedStatement::Select(select) => {
                    writeln!(output, "{}", select.pretty()).expect("writing to string cannot fail");
                }
            }
        }
        output
    }
}

#[derive(Clone, Debug)]
pub enum OptimizedStatement {
    Select(OptimizedSelect),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OptimizedSelect {
    pub original: SelectPlan,
    pub execution_order: Vec<SelectOperation>,
}

impl OptimizedSelect {
    #[must_use]
    pub fn pretty(&self) -> String {
        let mut output = String::new();
        writeln!(output, "SELECT {}", self.original.table).expect("writing to string cannot fail");
        for op in &self.execution_order {
            writeln!(output, "  - {}", op.keyword()).expect("writing to string cannot fail");
        }
        output
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectOperation {
    #[default]
    TableScan,
    Filter,
    Joins,
    GroupBy,
    Aggregate,
    OrderBy,
    Limit,
    Subquery,
}

impl SelectOperation {
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::TableScan => "table-scan",
            Self::Filter => "filter",
            Self::Joins => "joins",
            Self::GroupBy => "group-by",
            Self::Aggregate => "aggregate",
            Self::OrderBy => "order-by",
            Self::Limit => "limit",
            Self::Subquery => "subquery",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{BinaryExpr, BinaryOp, Column, Expr, Join, JoinKind};

    #[test]
    fn test_basic_select_optimization() {
        let plan = QueryPlan {
            statements: vec![Statement::Select(SelectPlan {
                table: "users".to_string(),
                columns: vec![Column {
                    name: "id".to_string(),
                    expr: None,
                    alias: None,
                }],
                where_clause: None,
                joins: vec![],
                group_by: vec![],
                order_by: None,
                limit: None,
                subquery: None,
            })],
        };

        let optimizer = Optimizer;
        let optimized = optimizer.optimize(&plan);

        assert!(matches!(
            optimized.statements[0],
            OptimizedStatement::Select(_)
        ));
    }

    #[test]
    fn test_select_with_filter() {
        let plan = QueryPlan {
            statements: vec![Statement::Select(SelectPlan {
                table: "users".to_string(),
                columns: vec![],
                where_clause: Some(Expr::Binary(BinaryExpr {
                    left: Box::new(Expr::Identifier("age".to_string())),
                    op: BinaryOp::Gt,
                    right: Box::new(Expr::LiteralInt(18)),
                })),
                joins: vec![],
                group_by: vec![],
                order_by: None,
                limit: None,
                subquery: None,
            })],
        };

        let optimizer = Optimizer;
        let optimized = optimizer.optimize(&plan);

        let OptimizedStatement::Select(opt_select) = &optimized.statements[0];
        assert!(
            opt_select
                .execution_order
                .contains(&SelectOperation::Filter)
        );
    }

    #[test]
    fn test_select_with_join() {
        let plan = QueryPlan {
            statements: vec![Statement::Select(SelectPlan {
                table: "users".to_string(),
                columns: vec![],
                where_clause: None,
                joins: vec![Join {
                    kind: JoinKind::Inner,
                    table: "orders".to_string(),
                    condition: Expr::Binary(BinaryExpr {
                        left: Box::new(Expr::Identifier("users.id".to_string())),
                        op: BinaryOp::Eq,
                        right: Box::new(Expr::Identifier("orders.user_id".to_string())),
                    }),
                }],
                group_by: vec![],
                order_by: None,
                limit: None,
                subquery: None,
            })],
        };

        let optimizer = Optimizer;
        let optimized = optimizer.optimize(&plan);

        let OptimizedStatement::Select(opt_select) = &optimized.statements[0];
        assert!(opt_select.execution_order.contains(&SelectOperation::Joins));
    }
}
