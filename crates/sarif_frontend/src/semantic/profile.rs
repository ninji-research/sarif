use std::collections::{BTreeMap, BTreeSet};

use crate::hir::{Expr, Stmt};

use super::{Body, Type};

pub(super) fn type_is_rt_safe(ty: &Type, struct_fields: &BTreeMap<String, Vec<Type>>) -> bool {
    let mut visiting = BTreeSet::new();
    type_is_rt_safe_inner(ty, struct_fields, &mut visiting)
}

fn type_is_rt_safe_inner(
    ty: &Type,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match ty {
        Type::Text => false,
        Type::Named(name) => {
            let Some(fields) = struct_fields.get(name) else {
                return true;
            };
            if !visiting.insert(name.clone()) {
                return true;
            }
            let result = fields
                .iter()
                .all(|field| type_is_rt_safe_inner(field, struct_fields, visiting));
            visiting.remove(name);
            result
        }
        Type::Array(element, _) => type_is_rt_safe_inner(element, struct_fields, visiting),
        Type::Param(_) => false,
        Type::I32 | Type::F64 | Type::Bool | Type::Unit | Type::Error => true,
        Type::TextBuilder | Type::F64Vec => false,
    }
}

pub(super) fn body_contains_loop(body: &Body) -> bool {
    body.statements.iter().any(|stmt| match stmt {
        Stmt::Let(binding) => expr_contains_loop(&binding.value),
        Stmt::Assign(stmt) => expr_contains_loop(&stmt.value),
        Stmt::Expr(stmt) => expr_contains_loop(&stmt.expr),
    }) || body.tail.as_ref().is_some_and(expr_contains_loop)
}

fn expr_contains_loop(expr: &Expr) -> bool {
    match expr {
        Expr::Repeat(_) | Expr::While(_) => true,
        Expr::If(expr) => {
            expr_contains_loop(&expr.condition)
                || body_contains_loop(&expr.then_body)
                || body_contains_loop(&expr.else_body)
        }
        Expr::Match(expr) => {
            expr_contains_loop(&expr.scrutinee)
                || expr.arms.iter().any(|arm| body_contains_loop(&arm.body))
        }
        Expr::Call(expr) => expr.args.iter().any(expr_contains_loop),
        Expr::Array(expr) => expr.elements.iter().any(expr_contains_loop),
        Expr::Record(expr) => expr.fields.iter().any(|f| expr_contains_loop(&f.value)),
        Expr::Unary(expr) => expr_contains_loop(&expr.inner),
        Expr::Binary(expr) => expr_contains_loop(&expr.left) || expr_contains_loop(&expr.right),
        Expr::Index(expr) => expr_contains_loop(&expr.base) || expr_contains_loop(&expr.index),
        Expr::Field(expr) => expr_contains_loop(&expr.base),
        Expr::Group(expr) => expr_contains_loop(&expr.inner),
        Expr::Integer(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Name(_)
        | Expr::ContractResult(_) => false,
        Expr::Comptime(body) => body_contains_loop(body),
        Expr::Handle(expr) => body_contains_loop(&expr.body),
    }
}
