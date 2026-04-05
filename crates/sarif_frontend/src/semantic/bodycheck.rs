use std::collections::{BTreeMap, HashMap, HashSet};

use crate::hir::{AssignStmt, Body, Expr, ExprStmt, LetBinding, Stmt};
use sarif_syntax::Diagnostic;

use super::exprcore::{CallSite, ExprContext, infer_expr};
use super::{
    BodyInfo, BodyStatementsInfo, ConstSignature, EnumVariantInfo, FunctionSignature, Type,
};
use super::{mutable_local_allows_affine_values, type_contains_affine_values};
use crate::hir::Effect;

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_body(
    body: &Body,
    initial_locals: &HashMap<String, Type>,
    initial_mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
) -> BodyInfo {
    let BodyStatementsInfo { locals, mut calls } = infer_body_statements(
        body,
        initial_locals,
        initial_mutable_locals,
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
    );

    if let Some(tail) = &body.tail {
        let info = infer_expr(
            tail,
            &locals,
            initial_mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            &ExprContext::BodyTail,
        );
        calls.extend(info.calls.clone());
        BodyInfo {
            ty: info.ty,
            calls,
            return_span: tail.span(),
        }
    } else {
        BodyInfo {
            ty: Type::Unit,
            calls,
            return_span: body.span,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn infer_body_statements(
    body: &Body,
    initial_locals: &HashMap<String, Type>,
    initial_mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
) -> BodyStatementsInfo {
    let mut state = BodyStmtState {
        locals: initial_locals.clone(),
        mutable_locals: initial_mutable_locals.clone(),
        calls: Vec::new(),
    };
    let mut context = BodyCheckContext {
        functions,
        consts,
        enum_variants,
        struct_layouts,
        diagnostics,
        fn_name,
        caller_effects,
    };

    for statement in &body.statements {
        match statement {
            Stmt::Let(binding) => handle_let_statement(binding, &mut state, &mut context),
            Stmt::Assign(statement) => handle_assign_statement(statement, &mut state, &mut context),
            Stmt::Expr(stmt) => handle_expr_statement(stmt, &mut state, &mut context),
        }
    }

    BodyStatementsInfo {
        locals: state.locals,
        calls: state.calls,
    }
}

struct BodyStmtState {
    locals: HashMap<String, Type>,
    mutable_locals: HashSet<String>,
    calls: Vec<CallSite>,
}

struct BodyCheckContext<'a> {
    functions: &'a BTreeMap<String, FunctionSignature>,
    consts: &'a BTreeMap<String, ConstSignature>,
    enum_variants: &'a BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &'a BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &'a mut Vec<Diagnostic>,
    fn_name: &'a str,
    caller_effects: &'a HashSet<Effect>,
}

fn handle_let_statement(
    binding: &LetBinding,
    state: &mut BodyStmtState,
    context: &mut BodyCheckContext<'_>,
) {
    let info = infer_expr(
        &binding.value,
        &state.locals,
        &state.mutable_locals,
        context.functions,
        context.consts,
        context.enum_variants,
        context.struct_layouts,
        context.diagnostics,
        context.fn_name,
        context.caller_effects,
        &ExprContext::Body,
    );
    state.calls.extend(info.calls);
    if state.locals.contains_key(&binding.name) {
        context.diagnostics.push(Diagnostic::new(
            "semantic.duplicate-local",
            format!(
                "local binding `{}` is already declared in `{}`",
                binding.name, context.fn_name
            ),
            binding.span,
            Some(
                "Use a fresh local name instead of shadowing parameters or earlier locals."
                    .to_owned(),
            ),
        ));
        return;
    }

    if binding.mutable
        && type_contains_affine_values(&info.ty, context.struct_layouts, context.enum_variants)
        && !mutable_local_allows_affine_values(&info.ty)
    {
        context.diagnostics.push(Diagnostic::new(
            "semantic.mutable-affine",
            format!(
                "mutable local `{}` in `{}` cannot hold affine values of type `{}`",
                binding.name,
                context.fn_name,
                info.ty.pretty(),
            ),
            binding.span,
            Some(
                "Keep mutable locals plain in stage-0, or use an immutable `let` binding unless the type is one of the maintained slot-backed affine builtins."
                    .to_owned(),
            ),
        ));
    } else if binding.mutable {
        state.mutable_locals.insert(binding.name.clone());
    }
    state.locals.insert(binding.name.clone(), info.ty);
}

fn handle_assign_statement(
    statement: &AssignStmt,
    state: &mut BodyStmtState,
    context: &mut BodyCheckContext<'_>,
) {
    let info = infer_expr(
        &statement.value,
        &state.locals,
        &state.mutable_locals,
        context.functions,
        context.consts,
        context.enum_variants,
        context.struct_layouts,
        context.diagnostics,
        context.fn_name,
        context.caller_effects,
        &ExprContext::Body,
    );
    state.calls.extend(info.calls);

    let (name, expected_ty) = match &statement.target {
        Expr::Name(target) => {
            let Some(current_ty) = state.locals.get(&target.name).cloned() else {
                context.diagnostics.push(Diagnostic::new(
                    "semantic.assign-unknown",
                    format!(
                        "cannot assign to `{}` in `{}` because it is not a local binding",
                        target.name, context.fn_name
                    ),
                    statement.span,
                    Some(
                        "Declare a local with `let mut name = ...;` before assigning to it."
                            .to_owned(),
                    ),
                ));
                return;
            };
            (target.name.clone(), current_ty)
        }
        Expr::Index(target) => {
            let Expr::Name(base) = target.base.as_ref() else {
                context.diagnostics.push(Diagnostic::new(
                    "semantic.assign-complex",
                    "unsupported complex assignment target in stage-0",
                    statement.span,
                    Some(
                        "Only simple local name or indexed array/list assignments are supported."
                            .to_owned(),
                    ),
                ));
                return;
            };
            let index = infer_expr(
                &target.index,
                &state.locals,
                &state.mutable_locals,
                context.functions,
                context.consts,
                context.enum_variants,
                context.struct_layouts,
                context.diagnostics,
                context.fn_name,
                context.caller_effects,
                &ExprContext::Body,
            );
            state.calls.extend(index.calls);
            if index.ty != Type::I32 && index.ty != Type::Error {
                context.diagnostics.push(Diagnostic::new(
                    "semantic.array-index-type",
                    format!(
                        "array index in `{}` assignment must be `I32`, found `{}`",
                        context.fn_name,
                        index.ty.render(),
                    ),
                    target.index.span(),
                    Some("Use an integer index for stage-0 array assignment.".to_owned()),
                ));
            }
            let Some(current_ty) = state.locals.get(&base.name).cloned() else {
                context.diagnostics.push(Diagnostic::new(
                    "semantic.assign-unknown",
                    format!(
                        "cannot assign to `{}` in `{}` because it is not a local binding",
                        base.name, context.fn_name
                    ),
                    statement.span,
                    Some(
                        "Declare a local with `let mut name = ...;` before assigning to it."
                            .to_owned(),
                    ),
                ));
                return;
            };
            let expected_ty = match current_ty {
                Type::Array(element, _) | Type::List(element) => *element,
                other => {
                    context.diagnostics.push(Diagnostic::new(
                        "semantic.array-index-base",
                        format!(
                            "cannot index value of type `{}` in `{}`",
                            other.pretty(),
                            context.fn_name,
                        ),
                        target.base.span(),
                        Some(
                            "Index assignment requires a mutable local array or list value."
                                .to_owned(),
                        ),
                    ));
                    return;
                }
            };
            (base.name.clone(), expected_ty)
        }
        _ => {
            context.diagnostics.push(Diagnostic::new(
                "semantic.assign-complex",
                "unsupported complex assignment target in stage-0",
                statement.span,
                Some(
                    "Only simple local name or indexed array/list assignments are supported."
                        .to_owned(),
                ),
            ));
            return;
        }
    };

    if !state.mutable_locals.contains(&name) {
        context.diagnostics.push(Diagnostic::new(
            "semantic.assign-immutable",
            format!(
                "cannot assign to `{}` in `{}` because it is not mutable",
                name, context.fn_name
            ),
            statement.span,
            Some("Declare the local with `let mut name = ...;` to allow `name = ...;`.".to_owned()),
        ));
    } else if info.ty != Type::Error && expected_ty != Type::Error {
        if info.ty != expected_ty {
            context.diagnostics.push(Diagnostic::new(
                "semantic.assign-type",
                format!(
                    "cannot assign `{}` to mutable local `{}` of type `{}` in `{}`",
                    info.ty.pretty(),
                    name,
                    expected_ty.pretty(),
                    context.fn_name
                ),
                statement.span,
                Some("Assign a value with the same type as the mutable local.".to_owned()),
            ));
        }
    } else {
        state.locals.insert(name, info.ty);
    }
}

fn handle_expr_statement(
    stmt: &ExprStmt,
    state: &mut BodyStmtState,
    context: &mut BodyCheckContext<'_>,
) {
    let info = infer_expr(
        &stmt.expr,
        &state.locals,
        &state.mutable_locals,
        context.functions,
        context.consts,
        context.enum_variants,
        context.struct_layouts,
        context.diagnostics,
        context.fn_name,
        context.caller_effects,
        &ExprContext::Statement,
    );
    state.calls.extend(info.calls);
    if info.ty != Type::Unit && info.ty != Type::Error {
        context.diagnostics.push(Diagnostic::new(
            "semantic.statement-type",
            format!(
                "statement expression in `{}` must be `Unit`, found `{}`",
                context.fn_name,
                info.ty.render(),
            ),
            stmt.span,
            Some(
                "Use a unit-valued expression as a statement, or move the value into a `let` binding or tail expression."
                    .to_owned(),
            ),
        ));
    }
}
