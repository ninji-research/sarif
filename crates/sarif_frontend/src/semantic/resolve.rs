use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::hir::{EffectRef, Module};
use crate::ownership::ParamUsage;
use sarif_syntax::Diagnostic;

use super::typecheck::{check_type_exists, type_from_ref};
use super::{
    ConstSignature, EnumVariantInfo, FunctionSignature, Type, best_match, suggestion_help,
};

#[derive(Clone, Debug)]
pub(super) struct ResolvedModule {
    pub functions: BTreeMap<String, FunctionSignature>,
    pub consts: BTreeMap<String, ConstSignature>,
    pub enum_variants: BTreeMap<String, Vec<EnumVariantInfo>>,
    pub struct_fields: BTreeMap<String, Vec<Type>>,
    pub struct_layouts: BTreeMap<String, Vec<(String, Type)>>,
    pub known_types: BTreeSet<String>,
}

#[must_use]
pub(super) fn resolve_module(module: &Module, diagnostics: &mut Vec<Diagnostic>) -> ResolvedModule {
    let mut functions = BTreeMap::<String, FunctionSignature>::new();
    let mut consts = BTreeMap::<String, ConstSignature>::new();
    let mut enum_variants = BTreeMap::<String, Vec<EnumVariantInfo>>::new();
    let mut struct_fields = BTreeMap::<String, Vec<Type>>::new();
    let mut struct_layouts = BTreeMap::<String, Vec<(String, Type)>>::new();
    let mut known_types = BTreeSet::from([
        "I32".to_owned(),
        "F64".to_owned(),
        "Bool".to_owned(),
        "Text".to_owned(),
        "Bytes".to_owned(),
        "TextIndex".to_owned(),
        "TextBuilder".to_owned(),
        "List".to_owned(),
        "Unit".to_owned(),
    ]);

    for item in &module.items {
        match item {
            crate::hir::Item::Const(_) => {}
            crate::hir::Item::Function(_) => {}
            crate::hir::Item::Enum(enum_item) => {
                known_types.insert(enum_item.name.clone());
            }
            crate::hir::Item::Struct(struct_item) => {
                known_types.insert(struct_item.name.clone());
            }
            crate::hir::Item::Effect(effect_item) => {
                known_types.insert(effect_item.name.clone());
            }
        }
    }

    for item in &module.items {
        match item {
            crate::hir::Item::Const(const_item) => {
                if consts.contains_key(&const_item.name)
                    || functions.contains_key(&const_item.name)
                    || enum_variants.contains_key(&const_item.name)
                    || struct_layouts.contains_key(&const_item.name)
                {
                    diagnostics.push(Diagnostic::new(
                        "semantic.duplicate-item",
                        format!(
                            "item `{}` is already declared in this module",
                            const_item.name
                        ),
                        const_item.span,
                        Some("Use a unique name for this constant.".to_owned()),
                    ));
                }
                check_type_exists(
                    diagnostics,
                    &known_types,
                    &BTreeSet::new(),
                    &const_item.ty.path,
                    const_item.ty.span,
                    "type",
                );
                consts.insert(
                    const_item.name.clone(),
                    ConstSignature {
                        ty: type_from_ref(&const_item.ty, &BTreeSet::new()),
                    },
                );
            }
            crate::hir::Item::Function(function) => {
                if functions.contains_key(&function.name)
                    || consts.contains_key(&function.name)
                    || enum_variants.contains_key(&function.name)
                    || struct_layouts.contains_key(&function.name)
                {
                    diagnostics.push(Diagnostic::new(
                        "semantic.duplicate-item",
                        format!(
                            "item `{}` is already declared in this module",
                            function.name
                        ),
                        function.span,
                        Some("Use a unique name for this function.".to_owned()),
                    ));
                }

                let mut generic_params = BTreeSet::<String>::new();
                for param in &function.type_params {
                    if !generic_params.insert(param.name.clone()) {
                        diagnostics.push(Diagnostic::new(
                            "semantic.duplicate-type-param",
                            format!(
                                "generic parameter `{}` is declared more than once",
                                param.name
                            ),
                            param.span,
                            Some("Use each generic parameter name only once.".to_owned()),
                        ));
                    } else if known_types.contains(&param.name) {
                        diagnostics.push(Diagnostic::new(
                            "semantic.duplicate-type-param",
                            format!(
                                "generic parameter `{}` conflicts with an existing type name",
                                param.name
                            ),
                            param.span,
                            Some(
                                "Rename the generic parameter so it does not shadow a type."
                                    .to_owned(),
                            ),
                        ));
                    }
                }

                let mut params = Vec::new();
                for param in &function.params {
                    check_type_exists(
                        diagnostics,
                        &known_types,
                        &generic_params,
                        &param.ty.path,
                        param.ty.span,
                        "parameter type",
                    );
                    params.push((
                        param.name.clone(),
                        type_from_ref(&param.ty, &generic_params),
                        param.span,
                    ));
                }

                if let Some(return_type) = &function.return_type {
                    check_type_exists(
                        diagnostics,
                        &known_types,
                        &generic_params,
                        &return_type.path,
                        return_type.span,
                        "return type",
                    );
                }

                let mut effects = Vec::new();
                let mut seen_effects = HashSet::new();
                for effect_ref in &function.effects {
                    match effect_ref {
                        EffectRef::Builtin { effect, span } => {
                            if !seen_effects.insert(effect.clone()) {
                                diagnostics.push(Diagnostic::new(
                                    "semantic.duplicate-effect",
                                    format!(
                                        "duplicate effect `{}` in declaration",
                                        effect.keyword()
                                    ),
                                    *span,
                                    Some("Remove the redundant effect name.".to_owned()),
                                ));
                            }
                            effects.push(effect.clone());
                        }
                        EffectRef::Unknown { name, span } => {
                            let suggestion = best_match(
                                name,
                                ["io", "alloc", "async", "parallel", "clock", "ffi", "nondet"]
                                    .into_iter(),
                            );
                            diagnostics.push(Diagnostic::new(
                                "semantic.unknown-effect",
                                format!("unknown effect `{name}`"),
                                *span,
                                Some(suggestion_help(suggestion, || {
                                    "Use a builtin effect such as `io` or `alloc`.".to_owned()
                                })),
                            ));
                        }
                    }
                }

                functions.insert(
                    function.name.clone(),
                    FunctionSignature {
                        const_params: collect_const_params_from_function(function, &generic_params),
                        param_usages: vec![ParamUsage::borrow_only(); params.len()],
                        params,
                        return_type: function
                            .return_type
                            .as_ref()
                            .map_or(Type::Unit, |return_type| {
                                type_from_ref(return_type, &generic_params)
                            }),
                        effects,
                        span: function.span,
                    },
                );
            }
            crate::hir::Item::Enum(enum_item) => {
                let mut variants = Vec::new();
                for variant in &enum_item.variants {
                    if let Some(payload) = &variant.payload {
                        check_type_exists(
                            diagnostics,
                            &known_types,
                            &BTreeSet::new(),
                            &payload.path,
                            payload.span,
                            "variant payload type",
                        );
                    }
                    variants.push(EnumVariantInfo {
                        name: variant.name.clone(),
                        payload: variant
                            .payload
                            .as_ref()
                            .map(|payload| type_from_ref(payload, &BTreeSet::new())),
                    });
                }
                enum_variants.insert(enum_item.name.clone(), variants);
            }
            crate::hir::Item::Struct(struct_item) => {
                let mut fields = Vec::new();
                let mut layout = Vec::new();
                for field in &struct_item.fields {
                    check_type_exists(
                        diagnostics,
                        &known_types,
                        &BTreeSet::new(),
                        &field.ty.path,
                        field.ty.span,
                        "field type",
                    );
                    let ty = type_from_ref(&field.ty, &BTreeSet::new());
                    fields.push(ty.clone());
                    layout.push((field.name.clone(), ty));
                }
                struct_fields.insert(struct_item.name.clone(), fields);
                struct_layouts.insert(struct_item.name.clone(), layout);
            }
            crate::hir::Item::Effect(_) => {}
        }
    }

    ResolvedModule {
        functions,
        consts,
        enum_variants,
        struct_fields,
        struct_layouts,
        known_types,
    }
}

fn collect_const_params_from_function(
    function: &crate::hir::Function,
    generic_params: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut params = BTreeSet::new();
    for param in &function.params {
        collect_const_params_from_type(&type_from_ref(&param.ty, generic_params), &mut params);
    }
    if let Some(return_type) = &function.return_type {
        collect_const_params_from_type(&type_from_ref(return_type, generic_params), &mut params);
    }
    params
}

fn collect_const_params_from_type(ty: &Type, params: &mut BTreeSet<String>) {
    match ty {
        Type::Array(element, size) => {
            collect_const_params_from_type(element, params);
            collect_const_expr_params(size, params);
        }
        Type::List(element) => collect_const_params_from_type(element, params),
        Type::Pair(left, right) => {
            collect_const_params_from_type(left, params);
            collect_const_params_from_type(right, params);
        }
        Type::I32
        | Type::F64
        | Type::Bool
        | Type::Text
        | Type::Bytes
        | Type::TextIndex
        | Type::TextBuilder
        | Type::Unit
        | Type::Named(_)
        | Type::Param(_)
        | Type::Error => {}
    }
}

fn collect_const_expr_params(expr: &crate::hir::ConstExpr, params: &mut BTreeSet<String>) {
    match expr {
        crate::hir::ConstExpr::Literal(_) => {}
        crate::hir::ConstExpr::Param(name) => {
            params.insert(name.clone());
        }
        crate::hir::ConstExpr::Add(left, right)
        | crate::hir::ConstExpr::Sub(left, right)
        | crate::hir::ConstExpr::Mul(left, right) => {
            collect_const_expr_params(left, params);
            collect_const_expr_params(right, params);
        }
    }
}
