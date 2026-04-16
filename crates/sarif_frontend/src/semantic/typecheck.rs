use std::collections::BTreeSet;

use crate::hir::{EffectRef, Function, TypeRef};
use sarif_syntax::{Diagnostic, Span};

use crate::hir::ConstExpr;

use super::{Type, best_match, suggestion_help};

pub(super) fn check_type_exists(
    diagnostics: &mut Vec<Diagnostic>,
    known_types: &BTreeSet<String>,
    generic_params: &BTreeSet<String>,
    path: &str,
    span: Span,
    context: &str,
) {
    let ty = parse_type_name(path, generic_params).unwrap_or_else(|| Type::Named(path.to_owned()));
    if type_exists(&ty, known_types, generic_params) {
        return;
    }

    let suggestion = best_match(path, known_types.iter().map(String::as_str));
    let (message, help) = match &ty {
        Type::Param(name) if !generic_params.contains(name) => (
            format!("unknown generic parameter `{name}`"),
            Some("Declare the generic parameter on the enclosing function.".to_owned()),
        ),
        Type::Array(_, ConstExpr::Param(name)) if !generic_params.contains(name) => (
            format!("unknown array length parameter `{name}`"),
            Some("Declare the length parameter on the enclosing function.".to_owned()),
        ),
        _ => (
            format!("unknown {context} `{path}`"),
            Some(suggestion_help(suggestion, || {
                "Use a builtin type, a declared struct name, or a generic parameter.".to_owned()
            })),
        ),
    };
    diagnostics.push(Diagnostic::new(
        "semantic.unknown-type",
        message,
        span,
        help,
    ));
}

pub(super) fn type_exists(
    ty: &Type,
    known_types: &BTreeSet<String>,
    generic_params: &BTreeSet<String>,
) -> bool {
    match ty {
        Type::Array(element, len) => {
            type_exists(element, known_types, generic_params)
                && match len {
                    ConstExpr::Literal(_) => true,
                    ConstExpr::Param(name) => generic_params.contains(name),
                    ConstExpr::Add(left, right)
                    | ConstExpr::Sub(left, right)
                    | ConstExpr::Mul(left, right) => {
                        let mut ok = true;
                        if let ConstExpr::Param(name) = &**left {
                            ok &= generic_params.contains(name);
                        }
                        if let ConstExpr::Param(name) = &**right {
                            ok &= generic_params.contains(name);
                        }
                        ok
                    }
                }
        }
        Type::Named(name) => known_types.contains(name),
        Type::Pair(left, right) => {
            type_exists(left, known_types, generic_params)
                && type_exists(right, known_types, generic_params)
        }
        Type::Param(name) => generic_params.contains(name),
        Type::I32
        | Type::F64
        | Type::Bool
        | Type::Text
        | Type::Bytes
        | Type::TextIndex
        | Type::List(_)
        | Type::TextBuilder
        | Type::Unit
        | Type::Error => true,
    }
}

pub(super) fn types_compatible(expected: &Type, actual: &Type) -> bool {
    match (expected, actual) {
        (Type::Error, _) | (_, Type::Error) => true,
        (Type::I32, Type::I32)
        | (Type::F64, Type::F64)
        | (Type::Bool, Type::Bool)
        | (Type::Text, Type::Text)
        | (Type::Bytes, Type::Bytes)
        | (Type::TextIndex, Type::TextIndex)
        | (Type::TextBuilder, Type::TextBuilder)
        | (Type::Unit, Type::Unit)
        | (Type::Named(_), Type::Named(_))
        | (Type::Param(_), Type::Param(_)) => expected == actual,
        (Type::List(expected_inner), Type::List(actual_inner)) => {
            types_compatible(expected_inner, actual_inner)
        }
        (Type::Pair(expected_left, expected_right), Type::Pair(actual_left, actual_right)) => {
            types_compatible(expected_left, actual_left)
                && types_compatible(expected_right, actual_right)
        }
        (Type::Array(expected_element, expected_len), Type::Array(actual_element, actual_len)) => {
            types_compatible(expected_element, actual_element)
                && array_len_compatible(expected_len, actual_len)
        }
        _ => false,
    }
}

pub(super) fn array_len_compatible(expected: &ConstExpr, actual: &ConstExpr) -> bool {
    match (expected, actual) {
        (ConstExpr::Literal(expected), ConstExpr::Literal(actual)) => expected == actual,
        (ConstExpr::Param(_), _) | (_, ConstExpr::Param(_)) => true,
        (ConstExpr::Add(_, _) | ConstExpr::Sub(_, _) | ConstExpr::Mul(_, _), _)
        | (_, ConstExpr::Add(_, _) | ConstExpr::Sub(_, _) | ConstExpr::Mul(_, _)) => true,
    }
}

pub(super) fn type_from_ref(ty: &TypeRef, generic_params: &BTreeSet<String>) -> Type {
    parse_type_name(&ty.path, generic_params).unwrap_or_else(|| Type::Named(ty.path.clone()))
}

pub(super) fn parse_type_name(name: &str, generic_params: &BTreeSet<String>) -> Option<Type> {
    if generic_params.contains(name) {
        return Some(Type::Param(name.to_owned()));
    }
    match name {
        "I32" => Some(Type::I32),
        "F64" => Some(Type::F64),
        "Bool" => Some(Type::Bool),
        "Text" => Some(Type::Text),
        "Bytes" => Some(Type::Bytes),
        "TextIndex" => Some(Type::TextIndex),
        "TextBuilder" => Some(Type::TextBuilder),
        "Unit" => Some(Type::Unit),
        other if other.starts_with("List[") && other.ends_with(']') => {
            let inner = &other[5..other.len() - 1];
            let element = parse_type_name(inner, generic_params)?;
            Some(Type::List(Box::new(element)))
        }
        _ => parse_array_type_name(name, generic_params)
            .or_else(|| Some(Type::Named(name.to_owned()))),
    }
}

pub(super) fn parse_array_type_name(name: &str, generic_params: &BTreeSet<String>) -> Option<Type> {
    let inner = name.strip_prefix('[')?.strip_suffix(']')?;
    let mut depth = 0usize;
    let mut split = None::<usize>;
    for (index, ch) in inner.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                split = Some(index);
                break;
            }
            _ => {}
        }
    }
    let split = split?;
    let element = inner[..split].trim();
    let len = inner[split + 1..].trim();
    let len = if let Ok(len) = len.parse::<u32>() {
        ConstExpr::Literal(len)
    } else {
        ConstExpr::Param(len.to_owned())
    };
    let element = parse_type_name(element, generic_params)?;
    Some(Type::Array(Box::new(element), len))
}

pub(super) fn render_signature(function: &Function) -> String {
    let type_params = if function.type_params.is_empty() {
        String::new()
    } else {
        format!(
            "[{}]",
            function
                .type_params
                .iter()
                .map(|param| match &param.kind {
                    Some(kind) => format!("{}: {kind}", param.name),
                    None => param.name.clone(),
                })
                .collect::<Vec<_>>()
                .join(", "),
        )
    };
    let params = function
        .params
        .iter()
        .map(|param| format!("{}: {}", param.name, param.ty.path))
        .collect::<Vec<_>>()
        .join(", ");
    let return_type = function
        .return_type
        .as_ref()
        .map_or_else(String::new, |return_type| {
            format!(" -> {}", return_type.path)
        });
    let effects = if function.effects.is_empty() {
        String::new()
    } else {
        format!(
            " effects [{}]",
            function
                .effects
                .iter()
                .map(EffectRef::name)
                .collect::<Vec<_>>()
                .join(", "),
        )
    };
    let requires = function
        .requires
        .as_ref()
        .map_or_else(String::new, |expr| format!(" requires {}", expr.pretty()));
    let ensures = function
        .ensures
        .as_ref()
        .map_or_else(String::new, |expr| format!(" ensures {}", expr.pretty()));

    format!(
        "fn {}{type_params}({params}){return_type}{effects}{requires}{ensures}",
        function.name
    )
}
