use std::collections::BTreeSet;

use crate::hir::{EffectRef, Function, TypeRef};
use sarif_syntax::{Diagnostic, Span};

use super::{Type, TypeArrayLen, best_match, suggestion_help};

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
        Type::Array(_, TypeArrayLen::Param(name)) if !generic_params.contains(name) => (
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
                    TypeArrayLen::Literal(_) => true,
                    TypeArrayLen::Param(name) => generic_params.contains(name),
                }
        }
        Type::Named(name) => known_types.contains(name),
        Type::Param(name) => generic_params.contains(name),
        Type::I32
        | Type::F64
        | Type::Bool
        | Type::Text
        | Type::F64Vec
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
        | (Type::TextBuilder, Type::TextBuilder)
        | (Type::F64Vec, Type::F64Vec)
        | (Type::Unit, Type::Unit)
        | (Type::Named(_), Type::Named(_))
        | (Type::Param(_), Type::Param(_)) => expected == actual,
        (Type::Array(expected_element, expected_len), Type::Array(actual_element, actual_len)) => {
            types_compatible(expected_element, actual_element)
                && array_len_compatible(expected_len, actual_len)
        }
        _ => false,
    }
}

pub(super) fn array_len_compatible(expected: &TypeArrayLen, actual: &TypeArrayLen) -> bool {
    match (expected, actual) {
        (TypeArrayLen::Literal(expected), TypeArrayLen::Literal(actual)) => expected == actual,
        (TypeArrayLen::Param(_), _) | (_, TypeArrayLen::Param(_)) => true,
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
        "TextBuilder" => Some(Type::TextBuilder),
        "F64Vec" => Some(Type::F64Vec),
        "Unit" => Some(Type::Unit),
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
    let len = if let Ok(len) = len.parse::<usize>() {
        TypeArrayLen::Literal(len)
    } else {
        TypeArrayLen::Param(len.to_owned())
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
