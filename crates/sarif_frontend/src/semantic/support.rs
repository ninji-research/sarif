use std::collections::{BTreeMap, BTreeSet};

use crate::hir::Expr;
use sarif_syntax::{Diagnostic, Span};

use super::exprcore::CallSite;
use super::{EnumVariantInfo, FunctionSignature, Profile, Type};

pub(super) fn check_recursion(
    functions: &BTreeMap<String, FunctionSignature>,
    call_graph: &BTreeMap<String, Vec<CallSite>>,
    diagnostics: &mut Vec<Diagnostic>,
    profile: Profile,
) {
    let mut visiting = BTreeSet::<String>::new();
    let mut visited = BTreeSet::<String>::new();

    for name in functions.keys() {
        let mut stack = Vec::<String>::new();
        visit_function(
            name,
            functions,
            call_graph,
            diagnostics,
            profile,
            &mut visiting,
            &mut visited,
            &mut stack,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn visit_function(
    name: &str,
    functions: &BTreeMap<String, FunctionSignature>,
    call_graph: &BTreeMap<String, Vec<CallSite>>,
    diagnostics: &mut Vec<Diagnostic>,
    profile: Profile,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
) {
    if visited.contains(name) {
        return;
    }
    if !visiting.insert(name.to_owned()) {
        if let Some(signature) = functions.get(name) {
            diagnostics.push(Diagnostic::new(
                "semantic.recursion-forbidden",
                format!(
                    "function `{name}` participates in recursion, which is forbidden in `{}`",
                    profile.keyword(),
                ),
                signature.span,
                Some("Refactor to an acyclic call graph for total and rt code.".to_owned()),
            ));
        }
        return;
    }

    stack.push(name.to_owned());
    if let Some(calls) = call_graph.get(name) {
        for call in calls {
            if stack.iter().any(|item| item == &call.callee) {
                if let Some(signature) = functions.get(name) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.recursion-forbidden",
                        format!(
                            "function `{}` participates in recursion, which is forbidden in `{}`",
                            call.callee,
                            profile.keyword(),
                        ),
                        signature.span,
                        Some("Refactor to an acyclic call graph for total and rt code.".to_owned()),
                    ));
                }
            } else if functions.contains_key(&call.callee) {
                visit_function(
                    &call.callee,
                    functions,
                    call_graph,
                    diagnostics,
                    profile,
                    visiting,
                    visited,
                    stack,
                );
            }
        }
    }
    stack.pop();
    visiting.remove(name);
    visited.insert(name.to_owned());
}

pub(super) fn type_contains_affine_values(
    ty: &Type,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
) -> bool {
    let mut visiting = BTreeSet::new();
    type_contains_affine_values_inner(ty, struct_layouts, enum_variants, &mut visiting)
}

pub(super) const fn mutable_local_allows_affine_values(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Text | Type::Bytes | Type::TextIndex | Type::TextBuilder | Type::List(_)
    )
}

fn type_contains_affine_values_inner(
    ty: &Type,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match ty {
        Type::Text | Type::Bytes => true,
        Type::Array(element, _) => {
            type_contains_affine_values_inner(element, struct_layouts, enum_variants, visiting)
        }
        Type::Pair(left, right) => {
            type_contains_affine_values_inner(left, struct_layouts, enum_variants, visiting)
                || type_contains_affine_values_inner(right, struct_layouts, enum_variants, visiting)
        }
        Type::Named(name) => {
            if !visiting.insert(name.clone()) {
                return false;
            }
            if let Some(fields) = struct_layouts.get(name) {
                let contains = fields.iter().any(|(_, field)| {
                    type_contains_affine_values_inner(
                        field,
                        struct_layouts,
                        enum_variants,
                        visiting,
                    )
                });
                visiting.remove(name);
                return contains;
            }
            if let Some(variants) = enum_variants.get(name) {
                let contains = variants.iter().any(|v| {
                    v.payload.as_ref().is_some_and(|payload_ty| {
                        type_contains_affine_values_inner(
                            payload_ty,
                            struct_layouts,
                            enum_variants,
                            visiting,
                        )
                    })
                });
                visiting.remove(name);
                return contains;
            }
            visiting.remove(name);
            true
        }
        Type::TextIndex | Type::TextBuilder | Type::List(_) => true,
        Type::Param(_) => false,
        Type::I32 | Type::F64 | Type::Bool | Type::Unit | Type::Error => false,
    }
}

pub fn field_type(
    base: &Type,
    field: &str,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
) -> Option<Type> {
    let Type::Named(name) = base else {
        return None;
    };
    struct_layouts.get(name).and_then(|fields| {
        fields
            .iter()
            .find_map(|(candidate, ty)| (candidate == field).then(|| ty.clone()))
    })
}

pub fn split_enum_variant_path(path: &str) -> Option<(&str, &str)> {
    let (enum_name, variant) = path.rsplit_once('.')?;
    (!enum_name.is_empty() && !variant.is_empty()).then_some((enum_name, variant))
}

pub(super) fn enum_variant_info<'a>(
    path: &'a str,
    enum_variants: &'a BTreeMap<String, Vec<EnumVariantInfo>>,
) -> Option<(&'a str, &'a EnumVariantInfo)> {
    let (enum_name, variant_name) = split_enum_variant_path(path)?;
    let variant = enum_variants
        .get(enum_name)?
        .iter()
        .find(|variant| variant.name == variant_name)?;
    Some((enum_name, variant))
}

pub(super) fn enum_literal_type_name(
    base: &Expr,
    field: &str,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    diagnostics: &mut Vec<Diagnostic>,
    span: Span,
) -> Option<String> {
    let Expr::Name(base_name) = base else {
        return None;
    };
    let variants = enum_variants.get(&base_name.name)?;
    if let Some(variant) = variants.iter().find(|variant| variant.name == field) {
        if variant.payload.is_some() {
            diagnostics.push(Diagnostic::new(
                "semantic.enum-variant",
                format!(
                    "enum variant `{}.{field}` carries a payload and must be constructed with an argument",
                    base_name.name
                ),
                span,
                Some("Call the constructor as `Enum.variant(value)`.".to_owned()),
            ));
            None
        } else {
            Some(base_name.name.clone())
        }
    } else {
        let suggestion = best_match(field, variants.iter().map(|variant| variant.name.as_str()));
        diagnostics.push(Diagnostic::new(
            "semantic.enum-variant",
            format!("enum `{}` has no variant `{field}`", base_name.name),
            span,
            Some(suggestion_help(suggestion, || {
                "Use one of the declared enum variants.".to_owned()
            })),
        ));
        None
    }
}

pub(super) fn field_names_for_type(
    base: &Type,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
) -> Option<Vec<String>> {
    let Type::Named(name) = base else {
        return None;
    };
    struct_layouts.get(name).map(|fields| {
        fields
            .iter()
            .map(|(candidate, _)| candidate.clone())
            .collect()
    })
}

pub(super) fn expect_type(
    diagnostics: &mut Vec<Diagnostic>,
    expr: &Expr,
    actual: &Type,
    expected: &Type,
    op: &str,
    side: &str,
    help: &str,
) {
    if *actual != *expected && *actual != Type::Error {
        diagnostics.push(Diagnostic::new(
            "semantic.binary-type",
            format!(
                "{side} operand of `{op}` must be `{}`, found `{}`",
                expected.render(),
                actual.render(),
            ),
            expr.span(),
            Some(help.to_owned()),
        ));
    }
}

pub(super) const fn matching_numeric_type(left: &Type, right: &Type) -> Option<Type> {
    match (left, right) {
        (Type::I32, Type::I32) => Some(Type::I32),
        (Type::F64, Type::F64) => Some(Type::F64),
        _ => None,
    }
}

pub(super) fn best_match<'a>(
    name: &str,
    candidates: impl Iterator<Item = &'a str>,
) -> Option<String> {
    candidates
        .map(|candidate| (strsim::levenshtein(name, candidate), candidate))
        .filter(|(distance, _)| *distance <= 3)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate.to_owned())
}

pub(super) fn suggestion_help(
    suggestion: Option<String>,
    default: impl FnOnce() -> String,
) -> String {
    suggestion.map_or_else(default, |suggestion| {
        format!("Did you mean `{suggestion}`?")
    })
}
