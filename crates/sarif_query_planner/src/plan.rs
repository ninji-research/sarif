use std::fmt::Write;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueryPlan {
    pub statements: Vec<Statement>,
}

impl QueryPlan {
    #[must_use]
    pub fn pretty(&self) -> String {
        let mut output = String::new();
        for stmt in &self.statements {
            writeln!(output, "{}", stmt.pretty()).expect("writing to string cannot fail");
        }
        output
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Select(SelectPlan),
}

impl Statement {
    #[must_use]
    pub fn pretty(&self) -> String {
        match self {
            Self::Select(plan) => plan.pretty(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectPlan {
    pub table: String,
    pub columns: Vec<Column>,
    pub where_clause: Option<Expr>,
    pub joins: Vec<Join>,
    pub group_by: Vec<String>,
    pub order_by: Option<OrderBy>,
    pub limit: Option<usize>,
    pub subquery: Option<Box<Self>>,
}

impl SelectPlan {
    #[must_use]
    pub fn pretty(&self) -> String {
        let mut output = String::new();
        write!(&mut output, "SELECT ").expect("writing to string cannot fail");

        let cols: Vec<String> = self.columns.iter().map(Column::pretty).collect();
        output.push_str(&cols.join(", "));

        output.push_str(" FROM ");
        output.push_str(&self.table);

        if let Some(ref where_expr) = self.where_clause {
            write!(&mut output, " WHERE {}", where_expr.pretty())
                .expect("writing to string cannot fail");
        }

        for join in &self.joins {
            write!(&mut output, " {}", join.pretty()).expect("writing to string cannot fail");
        }

        if !self.group_by.is_empty() {
            write!(&mut output, " GROUP BY {}", self.group_by.join(", "))
                .expect("writing to string cannot fail");
        }

        if let Some(ref order) = self.order_by {
            write!(&mut output, " ORDER BY {}", order.pretty())
                .expect("writing to string cannot fail");
        }

        if let Some(limit) = self.limit {
            write!(&mut output, " LIMIT {limit}").expect("writing to string cannot fail");
        }

        output
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub name: String,
    pub expr: Option<Expr>,
    pub alias: Option<String>,
}

impl Column {
    #[must_use]
    #[allow(clippy::option_if_let_else)]
    pub fn pretty(&self) -> String {
        match &self.expr {
            Some(expr) => {
                let mut s = format!("{} AS {}", expr.pretty(), self.name);
                if let Some(alias) = &self.alias {
                    s.push_str(" AS ");
                    s.push_str(alias);
                }
                s
            }
            None => self.name.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Join {
    pub kind: JoinKind,
    pub table: String,
    pub condition: Expr,
}

impl Join {
    #[must_use]
    pub fn pretty(&self) -> String {
        let kind_str = match self.kind {
            JoinKind::Inner => "INNER JOIN",
            JoinKind::Left => "LEFT JOIN",
            JoinKind::Right => "RIGHT JOIN",
        };
        format!("{} {} ON {}", kind_str, self.table, self.condition.pretty())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JoinKind {
    #[default]
    Inner,
    Left,
    Right,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrderBy {
    pub expressions: Vec<(Expr, SortOrder)>,
}

impl OrderBy {
    #[must_use]
    pub fn pretty(&self) -> String {
        self.expressions
            .iter()
            .map(|(expr, order)| format!("{} {}", expr.pretty(), order.keyword()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

impl SortOrder {
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Identifier(String),
    LiteralInt(i64),
    LiteralF64(f64),
    LiteralText(String),
    Binary(BinaryExpr),
    Call(CallExpr),
    In(InExpr),
    Exists(ExistsExpr),
}

impl Expr {
    #[must_use]
    pub fn pretty(&self) -> String {
        match self {
            Self::Identifier(name) => name.clone(),
            Self::LiteralInt(v) => v.to_string(),
            Self::LiteralF64(v) => {
                let mut s = v.to_string();
                if !s.contains('.') {
                    s.push_str(".0");
                }
                s
            }
            Self::LiteralText(s) => format!("\"{s}\""),
            Self::Binary(expr) => expr.pretty(),
            Self::Call(expr) => expr.pretty(),
            Self::In(expr) => expr.pretty(),
            Self::Exists(expr) => expr.pretty(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BinaryExpr {
    pub left: Box<Expr>,
    pub op: BinaryOp,
    pub right: Box<Expr>,
}

impl BinaryExpr {
    #[must_use]
    pub fn pretty(&self) -> String {
        format!(
            "({} {} {})",
            self.left.pretty(),
            self.op.symbol(),
            self.right.pretty()
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Add,
    Sub,
    Mul,
    Div,
}

impl BinaryOp {
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CallExpr {
    pub callee: String,
    pub args: Vec<Expr>,
    pub aggregate: Option<Aggregate>,
}

impl CallExpr {
    #[must_use]
    pub fn pretty(&self) -> String {
        let mut s = self.callee.clone();
        let args: Vec<String> = self.args.iter().map(Expr::pretty).collect();
        s.push('(');
        s.push_str(&args.join(", "));
        s.push(')');
        s
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Aggregate {
    #[default]
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl Aggregate {
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Count => "COUNT",
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InExpr {
    pub expr: Box<Expr>,
    pub values: Vec<Expr>,
}

impl InExpr {
    #[must_use]
    pub fn pretty(&self) -> String {
        let values: Vec<String> = self.values.iter().map(Expr::pretty).collect();
        format!("{} IN ({})", self.expr.pretty(), values.join(", "))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExistsExpr {
    pub subquery: Box<SelectPlan>,
}

impl ExistsExpr {
    #[must_use]
    pub fn pretty(&self) -> String {
        format!("EXISTS ({})", self.subquery.pretty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_plan_pretty() {
        let plan = SelectPlan {
            table: "users".to_string(),
            columns: vec![
                Column {
                    name: "id".to_string(),
                    expr: None,
                    alias: None,
                },
                Column {
                    name: "name".to_string(),
                    expr: Some(Expr::Identifier("name".to_string())),
                    alias: None,
                },
            ],
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
        };

        let pretty = plan.pretty();
        assert!(pretty.contains("SELECT"));
        assert!(pretty.contains("FROM users"));
        assert!(pretty.contains("WHERE"));
    }

    #[test]
    fn test_join_pretty() {
        let join = Join {
            kind: JoinKind::Inner,
            table: "orders".to_string(),
            condition: Expr::Binary(BinaryExpr {
                left: Box::new(Expr::Identifier("users.id".to_string())),
                op: BinaryOp::Eq,
                right: Box::new(Expr::Identifier("orders.user_id".to_string())),
            }),
        };
        assert!(join.pretty().contains("INNER JOIN"));
        assert!(join.pretty().contains("orders"));
    }
}
