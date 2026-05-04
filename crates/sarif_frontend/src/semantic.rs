use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::hir::{Body, ConstExpr, Effect, Expr, Module};
use crate::ownership::{
    ParamUsage, check_affine_body_ownership, check_contract_affine_ownership, infer_param_modes,
    is_affine_type, struct_is_affine,
};
use sarif_syntax::{Diagnostic, Span};

mod bodycheck;
mod exprcore;
mod profile;
mod resolve;
mod support;
mod typecheck;

use bodycheck::infer_body;
use exprcore::{
    CallSite, ExprContext, ExprInfo, infer_array_expr, infer_binary_expr, infer_call_expr,
    infer_comptime_expr, infer_contract_result_expr, infer_field_expr, infer_group_expr,
    infer_handle_expr, infer_if_expr, infer_index_expr, infer_match_expr, infer_perform_expr,
    infer_record_expr, infer_repeat_expr, infer_unary_expr, infer_while_expr,
};
pub use profile::Profile;
use profile::{body_contains_loop, type_is_rt_safe};
use resolve::{ResolvedModule, resolve_module};
use support::{
    best_match, check_recursion, enum_variant_info, expect_type, matching_numeric_type,
    mutable_local_allows_affine_values, suggestion_help, type_contains_affine_values,
};
pub(crate) use support::{field_type, split_enum_variant_path};
use typecheck::{render_signature, types_compatible};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    I32,
    F64,
    Bool,
    Text,
    Bytes,
    TextIndex,
    TextBuilder,
    Unit,
    Named(String),
    Param(String),
    Array(Box<Self>, ConstExpr),
    List(Box<Self>),
    Pair(Box<Self>, Box<Self>),
    Error,
}

impl Type {
    #[must_use]
    pub fn render(&self) -> String {
        match self {
            Self::I32 => "I32".to_owned(),
            Self::F64 => "F64".to_owned(),
            Self::Bool => "Bool".to_owned(),
            Self::Text => "Text".to_owned(),
            Self::Bytes => "Bytes".to_owned(),
            Self::TextIndex => "TextIndex".to_owned(),
            Self::TextBuilder => "TextBuilder".to_owned(),
            Self::Unit => "Unit".to_owned(),
            Self::Named(name) => name.clone(),
            Self::Param(name) => name.clone(),
            Self::Array(element, size) => {
                format!("[{}; {}]", element.render(), size.render())
            }
            Self::List(element) => {
                format!("List[{}]", element.render())
            }
            Self::Pair(left, right) => {
                format!("Pair[{}, {}]", left.render(), right.render())
            }
            Self::Error => "?".to_owned(),
        }
    }

    #[must_use]
    pub fn pretty(&self) -> String {
        self.render()
    }
}

#[derive(Clone, Debug)]
pub struct FunctionSignature {
    pub const_params: BTreeSet<String>,
    pub params: Vec<(String, Type, Span)>,
    pub param_usages: Vec<ParamUsage>,
    pub return_type: Type,
    pub effects: Vec<Effect>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ConstSignature {
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct EnumVariantInfo {
    pub name: String,
    pub payload: Option<Type>,
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub reports: Vec<ItemReport>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug)]
pub enum ItemReport {
    Const(ConstReport),
    Function(FunctionReport),
    Enum(EnumReport),
    Struct(StructReport),
}

#[derive(Clone, Debug)]
pub struct ConstReport {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FunctionReport {
    pub name: String,
    pub signature: String,
    pub ownership_status: String,
    pub rt_status: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EnumReport {
    pub name: String,
    pub variant_count: usize,
    pub ownership_status: String,
    pub rt_status: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StructReport {
    pub name: String,
    pub ownership_status: String,
    pub rt_status: String,
    pub span: Span,
}

impl ItemReport {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Const(report) => report.span,
            Self::Function(report) => report.span,
            Self::Enum(report) => report.span,
            Self::Struct(report) => report.span,
        }
    }
}

#[must_use]
pub fn analyze(module: &Module, profile: Profile) -> Analysis {
    let mut diagnostics = Vec::new();
    let ResolvedModule {
        mut functions,
        consts,
        enum_variants,
        struct_fields,
        struct_layouts,
        known_types: _known_types,
    } = resolve_module(module, &mut diagnostics);

    infer_param_modes(
        module,
        &mut functions,
        &enum_variants,
        &struct_fields,
        &struct_layouts,
    );

    let mut reports = Vec::new();
    let mut call_graph = BTreeMap::<String, Vec<CallSite>>::new();

    for item in &module.items {
        match item {
            crate::hir::Item::Const(const_item) => {
                let signature = consts.get(&const_item.name).expect("const signature");
                let info = infer_expr(
                    &const_item.value,
                    &HashMap::new(),
                    &HashSet::new(),
                    &functions,
                    &consts,
                    &enum_variants,
                    &struct_layouts,
                    &mut diagnostics,
                    &const_item.name,
                    &HashSet::new(),
                    &ExprContext::ContractRequires,
                );
                if info.ty != signature.ty && info.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.const-type",
                        format!(
                            "constant `{}` expects `{}`, found `{}`",
                            const_item.name,
                            signature.ty.pretty(),
                            info.ty.pretty(),
                        ),
                        const_item.span,
                        Some("Make the constant initializer match its declared type.".to_owned()),
                    ));
                }
                for call in &info.calls {
                    let is_pure = functions
                        .get(&call.callee)
                        .is_some_and(|sig| sig.effects.is_empty());
                    if !is_pure && !functions.contains_key(&call.callee) {
                        // builtins like len() are pure
                    } else if !is_pure {
                        diagnostics.push(Diagnostic::new(
                            "semantic.const-expr",
                            format!(
                                "constant `{}` calls effectful helper `{}`",
                                const_item.name, call.callee
                            ),
                            const_item.span,
                            Some("Constant initializers must be effect-free.".to_owned()),
                        ));
                    }
                }
                reports.push(ItemReport::Const(ConstReport {
                    name: const_item.name.clone(),
                    ty: signature.ty.clone(),
                    span: const_item.span,
                }));
            }
            crate::hir::Item::Function(function) => {
                let signature = functions.get(&function.name).expect("function signature");
                let mut locals = HashMap::new();
                for name in &signature.const_params {
                    locals.insert(name.clone(), Type::I32);
                }
                for (name, ty, _) in &signature.params {
                    locals.insert(name.clone(), ty.clone());
                }

                let caller_effects = signature.effects.iter().cloned().collect::<HashSet<_>>();

                if let Some(requires) = &function.requires {
                    let info = infer_expr(
                        requires,
                        &locals,
                        &HashSet::new(),
                        &functions,
                        &consts,
                        &enum_variants,
                        &struct_layouts,
                        &mut diagnostics,
                        &function.name,
                        &caller_effects,
                        &ExprContext::ContractRequires,
                    );
                    if info.ty != Type::Bool && info.ty != Type::Error {
                        diagnostics.push(Diagnostic::new(
                            "semantic.contract-type",
                            format!(
                                "`requires` clause in `{}` must be `Bool`, found `{}`",
                                function.name,
                                info.ty.render(),
                            ),
                            requires.span(),
                            Some("Use a boolean expression for preconditions.".to_owned()),
                        ));
                    }
                    check_contract_affine_ownership(
                        requires,
                        &locals,
                        &functions,
                        &enum_variants,
                        &struct_fields,
                        &struct_layouts,
                        &mut diagnostics,
                        &function.name,
                        "requires",
                        None,
                    );
                }

                if let Some(ensures) = &function.ensures {
                    let mut ensures_locals = locals.clone();
                    if ensures_locals.contains_key("result") {
                        diagnostics.push(Diagnostic::new(
                            "semantic.contract-result-conflict",
                            format!(
                                "function `{}` has a parameter named `result`, which conflicts with the implicit `ensures` result binding",
                                function.name
                            ),
                            function.span,
                            Some("Rename the parameter to allow using `result` in postconditions.".to_owned()),
                        ));
                    } else {
                        ensures_locals.insert("result".to_owned(), signature.return_type.clone());
                    }
                    let info = infer_expr(
                        ensures,
                        &ensures_locals,
                        &HashSet::new(),
                        &functions,
                        &consts,
                        &enum_variants,
                        &struct_layouts,
                        &mut diagnostics,
                        &function.name,
                        &caller_effects,
                        &ExprContext::ContractEnsures,
                    );
                    if info.ty != Type::Bool && info.ty != Type::Error {
                        diagnostics.push(Diagnostic::new(
                            "semantic.contract-type",
                            format!(
                                "`ensures` clause in `{}` must be `Bool`, found `{}`",
                                function.name,
                                info.ty.render(),
                            ),
                            ensures.span(),
                            Some("Use a boolean expression for postconditions.".to_owned()),
                        ));
                    }
                    check_contract_affine_ownership(
                        ensures,
                        &ensures_locals,
                        &functions,
                        &enum_variants,
                        &struct_fields,
                        &struct_layouts,
                        &mut diagnostics,
                        &function.name,
                        "ensures",
                        Some(&signature.return_type),
                    );
                }

                let mut body_calls = Vec::new();
                if let Some(body) = &function.body {
                    let info = infer_body(
                        body,
                        &locals,
                        &HashSet::new(),
                        &functions,
                        &consts,
                        &enum_variants,
                        &struct_layouts,
                        &mut diagnostics,
                        &function.name,
                        &caller_effects,
                    );
                    if !types_compatible(&info.ty, &signature.return_type)
                        && info.ty != Type::Error
                        && signature.return_type != Type::Error
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.return-type",
                            format!(
                                "function `{}` expects `{}`, found `{}`",
                                function.name,
                                signature.return_type.pretty(),
                                info.ty.pretty(),
                            ),
                            info.return_span,
                            Some(
                                "Make the body's tail expression match the return type.".to_owned(),
                            ),
                        ));
                    }
                    body_calls = info.calls;
                    check_affine_body_ownership(
                        body,
                        &locals,
                        &functions,
                        &enum_variants,
                        &struct_fields,
                        &struct_layouts,
                        &mut diagnostics,
                        &function.name,
                    );

                    // Stage-1 requires proper Escape Analysis to make this a hard error.
                    // For Core/Total profiles: emit as warning (semantic.alloc-escape)
                    // For RT profile: hard error (escape.analysis.required) - blocks build
                    if signature.effects.contains(&Effect::Alloc) {
                        let could_be_allocated = matches!(
                            signature.return_type,
                            Type::Named(_) | Type::List(_) | Type::Array(_, _) | Type::Text
                        );
                        if could_be_allocated
                            && signature.return_type != Type::Unit
                            && signature.return_type != Type::Error
                        {
                            let is_rt = profile == Profile::Rt;
                            let (code, help_msg) = if is_rt {
                                (
                                    "escape.analysis.required",
                                    "To fix: Either return a type that cannot reference arena memory, or restructure to not return allocated data. Stage-1 requires all alloc return values to be verified safe.".to_owned(),
                                )
                            } else {
                                (
                                    "semantic.alloc-escape",
                                    "Stage-1 will implement Escape Analysis as a HARD ERROR. Stage-0 provides NO memory safety guarantee for returned allocations.".to_owned(),
                                )
                            };
                            diagnostics.push(Diagnostic::new(
                                code,
                                format!(
                                    "function `{}` with `alloc` effect returns `{}`.{}",
                                    function.name,
                                    signature.return_type.render(),
                                    if is_rt { " This is UNSAFE and BLOCKS compilation in RT profile." } else { " This is UNSAFE in Stage-0: returned pointers become dangling after `alloc_pop()`." }
                                ),
                                function.span,
                                Some(help_msg),
                            ));
                        }
                    }
                }

                call_graph.insert(function.name.clone(), body_calls);

                let ownership_status = if signature
                    .param_usages
                    .iter()
                    .any(|usage| !usage.is_borrow_only())
                {
                    "consumes affine arguments".to_owned()
                } else if check_affine_body_ownership(
                    function.body.as_ref().unwrap_or(&crate::hir::Body {
                        statements: Vec::new(),
                        tail: None,
                        span: Span::default(),
                    }),
                    &locals,
                    &functions,
                    &enum_variants,
                    &struct_fields,
                    &struct_layouts,
                    &mut Vec::new(),
                    &function.name,
                ) {
                    "affine-safe in stage-0".to_owned()
                } else {
                    "affine violations".to_owned()
                };

                let rt_status = if signature.effects.contains(&Effect::Io)
                    || signature.effects.contains(&Effect::Alloc)
                    || signature.effects.contains(&Effect::Async)
                    || signature.effects.contains(&Effect::Parallel)
                {
                    "blocked in rt".to_owned()
                } else if type_is_rt_safe(&signature.return_type, &struct_fields)
                    && signature
                        .params
                        .iter()
                        .all(|(_, ty, _)| type_is_rt_safe(ty, &struct_fields))
                {
                    "profile-compatible".to_owned()
                } else {
                    "blocked in rt".to_owned()
                };

                reports.push(ItemReport::Function(FunctionReport {
                    name: function.name.clone(),
                    signature: render_signature(function),
                    ownership_status,
                    rt_status,
                    span: function.span,
                }));
            }
            crate::hir::Item::Enum(enum_item) => {
                let variants = enum_variants.get(&enum_item.name).expect("enum variants");
                let has_affine = variants.iter().any(|v| {
                    v.payload
                        .as_ref()
                        .is_some_and(|ty| is_affine_type(ty, &struct_fields, &enum_variants))
                });
                let ownership_status = if has_affine {
                    "contains affine payloads"
                } else if variants.iter().all(|v| v.payload.is_none()) {
                    "plain tag"
                } else {
                    "plain value"
                };

                let rt_status = if variants.iter().all(|v| {
                    v.payload
                        .as_ref()
                        .is_none_or(|ty| type_is_rt_safe(ty, &struct_fields))
                }) {
                    "profile-compatible"
                } else {
                    "blocked in rt"
                };

                reports.push(ItemReport::Enum(EnumReport {
                    name: enum_item.name.clone(),
                    variant_count: variants.len(),
                    ownership_status: ownership_status.to_owned(),
                    rt_status: rt_status.to_owned(),
                    span: enum_item.span,
                }));
            }
            crate::hir::Item::Struct(struct_item) => {
                let fields = struct_fields.get(&struct_item.name).expect("struct fields");
                let ownership_status =
                    if struct_is_affine(&struct_item.name, &struct_fields, &enum_variants) {
                        "contains affine fields"
                    } else {
                        "plain value"
                    };

                let rt_status = if fields.iter().all(|ty| type_is_rt_safe(ty, &struct_fields)) {
                    "profile-compatible"
                } else {
                    "blocked in rt"
                };

                reports.push(ItemReport::Struct(StructReport {
                    name: struct_item.name.clone(),
                    ownership_status: ownership_status.to_owned(),
                    rt_status: rt_status.to_owned(),
                    span: struct_item.span,
                }));
            }
            crate::hir::Item::Effect(_) => {}
        }
    }

    if profile == Profile::Total || profile == Profile::Rt {
        check_recursion(&functions, &call_graph, &mut diagnostics, profile);
    }

    for item in &module.items {
        match item {
            crate::hir::Item::Function(function) => {
                let signature = functions.get(&function.name).expect("function signature");
                if profile == Profile::Total {
                    if !signature.effects.is_empty() {
                        diagnostics.push(Diagnostic::new(
                            "semantic.total-effect",
                            format!(
                                "function `{}` in total profile declares effects [{}]",
                                function.name,
                                signature
                                    .effects
                                    .iter()
                                    .map(Effect::keyword)
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            ),
                            function.span,
                            Some(
                                "Remove all effect declarations for the total profile.".to_owned(),
                            ),
                        ));
                    }
                    if function.body.as_ref().is_some_and(body_contains_loop) {
                        diagnostics.push(Diagnostic::new(
                            "semantic.total-loop",
                            format!(
                                "function `{}` in total profile uses an iterative loop",
                                function.name
                            ),
                            function.span,
                            Some(
                                "Use recursion or a total iteration form instead of `repeat` or `while`."
                                    .to_owned(),
                            ),
                        ));
                    }
                }
                if profile == Profile::Rt {
                    for effect in &signature.effects {
                        if matches!(
                            effect,
                            Effect::Io | Effect::Alloc | Effect::Async | Effect::Parallel
                        ) {
                            diagnostics.push(Diagnostic::new(
                                "semantic.rt-effect",
                                format!(
                                    "function `{}` in rt profile declares blocked effect `{}`",
                                    function.name,
                                    effect.keyword(),
                                ),
                                function.span,
                                Some(
                                    "Remove I/O, allocation, and concurrency effects for rt."
                                        .to_owned(),
                                ),
                            ));
                        }
                    }
                    for (name, ty, span) in &signature.params {
                        if !type_is_rt_safe(ty, &struct_fields) {
                            diagnostics.push(Diagnostic::new(
                                "semantic.rt-type",
                                format!(
                                    "function `{}` parameter `{name}` has rt-unsafe type `{}`",
                                    function.name,
                                    ty.render(),
                                ),
                                *span,
                                Some(
                                    "Use only bounded, non-allocating types in rt signatures."
                                        .to_owned(),
                                ),
                            ));
                        }
                    }
                    if !type_is_rt_safe(&signature.return_type, &struct_fields) {
                        diagnostics.push(Diagnostic::new(
                            "semantic.rt-type",
                            format!(
                                "function `{}` has rt-unsafe return type `{}`",
                                function.name,
                                signature.return_type.render(),
                            ),
                            function.span,
                            Some(
                                "Use only bounded, non-allocating types in rt signatures."
                                    .to_owned(),
                            ),
                        ));
                    }
                }
            }
            crate::hir::Item::Enum(enum_item) => {
                if profile == Profile::Rt {
                    let variants = enum_variants.get(&enum_item.name).expect("enum variants");
                    for variant in variants {
                        if let Some(payload) = &variant.payload
                            && !type_is_rt_safe(payload, &struct_fields)
                        {
                            diagnostics.push(Diagnostic::new(
                                "semantic.rt-type",
                                format!(
                                    "enum `{}` variant `{}` has rt-unsafe payload type `{}`",
                                    enum_item.name,
                                    variant.name,
                                    payload.render(),
                                ),
                                enum_item.span,
                                Some(
                                    "Use only bounded types for payloads in the rt profile."
                                        .to_owned(),
                                ),
                            ));
                        }
                    }
                }
            }
            crate::hir::Item::Struct(struct_item) => {
                if profile == Profile::Rt {
                    let layout = struct_layouts
                        .get(&struct_item.name)
                        .expect("struct layout");
                    for (name, ty) in layout {
                        if !type_is_rt_safe(ty, &struct_fields) {
                            diagnostics.push(Diagnostic::new(
                                "semantic.rt-type",
                                format!(
                                    "struct `{}` field `{name}` has rt-unsafe type `{}`",
                                    struct_item.name,
                                    ty.render(),
                                ),
                                struct_item.span,
                                Some(
                                    "Use only bounded types for fields in the rt profile."
                                        .to_owned(),
                                ),
                            ));
                        }
                    }
                }
            }
            crate::hir::Item::Const(_) => {}
            crate::hir::Item::Effect(_) => {}
        }
    }

    Analysis {
        reports,
        diagnostics,
    }
}

#[derive(Clone, Debug)]
pub(super) struct BodyInfo {
    ty: Type,
    calls: Vec<CallSite>,
    return_span: Span,
}

#[derive(Clone, Debug)]
pub(super) struct BodyStatementsInfo {
    locals: HashMap<String, Type>,
    calls: Vec<CallSite>,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) fn infer_expr(
    expr: &Expr,
    locals: &HashMap<String, Type>,
    mutable_locals: &HashSet<String>,
    functions: &BTreeMap<String, FunctionSignature>,
    consts: &BTreeMap<String, ConstSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    fn_name: &str,
    caller_effects: &HashSet<Effect>,
    context: &ExprContext,
) -> ExprInfo {
    match expr {
        Expr::Integer(_) => ExprInfo {
            ty: Type::I32,
            calls: Vec::new(),
        },
        Expr::Float(_) => ExprInfo {
            ty: Type::F64,
            calls: Vec::new(),
        },
        Expr::String(_) => ExprInfo {
            ty: Type::Text,
            calls: Vec::new(),
        },
        Expr::Bool(_) => ExprInfo {
            ty: Type::Bool,
            calls: Vec::new(),
        },
        Expr::Name(expr) => {
            if let Some(ty) = locals.get(&expr.name) {
                ExprInfo {
                    ty: ty.clone(),
                    calls: Vec::new(),
                }
            } else if let Some(sig) = consts.get(&expr.name) {
                ExprInfo {
                    ty: sig.ty.clone(),
                    calls: Vec::new(),
                }
            } else {
                let suggestion = best_match(
                    &expr.name,
                    locals.keys().chain(consts.keys()).map(String::as_str),
                );
                diagnostics.push(Diagnostic::new(
                    "semantic.unknown-name",
                    format!("unknown name `{}` in `{fn_name}`", expr.name),
                    expr.span,
                    Some(suggestion_help(suggestion, || {
                        "Declare a local binding or a top-level constant.".to_owned()
                    })),
                ));
                ExprInfo {
                    ty: Type::Error,
                    calls: Vec::new(),
                }
            }
        }
        Expr::ContractResult(expr) => infer_contract_result_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::Array(expr) => infer_array_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::Index(expr) => infer_index_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::If(expr) => infer_if_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
        ),
        Expr::Match(expr) => infer_match_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::Repeat(expr) => infer_repeat_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::While(expr) => infer_while_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::Group(expr) => infer_group_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::Unary(expr) => infer_unary_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::Field(expr) => infer_field_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::Record(expr) => infer_record_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::Binary(expr) => infer_binary_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::Call(expr) => infer_call_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
            context,
        ),
        Expr::Comptime(body) => infer_comptime_expr(
            body,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
        ),
        Expr::Perform(expr) => infer_perform_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
        ),
        Expr::Handle(expr) => infer_handle_expr(
            expr,
            locals,
            mutable_locals,
            functions,
            consts,
            enum_variants,
            struct_layouts,
            diagnostics,
            fn_name,
            caller_effects,
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::hir;
    use crate::semantic::{Profile, analyze};

    fn analyze_source(source: &str) -> crate::semantic::Analysis {
        let lexed = sarif_syntax::lexer::lex(source);
        let parsed = sarif_syntax::parser::parse(&lexed.tokens);
        let ast = sarif_syntax::ast::lower(&parsed.root);
        let hir = hir::lower(&ast.file);
        analyze(&hir.module, Profile::Core)
    }

    #[test]
    fn f64_literal_types_and_mixed_arithmetic() {
        let source = "
fn mixed_math() -> F64 {
    let a = 1.2e-10;
    let b = 5.0;
    a + b
}
";
        let analysis = analyze_source(source);
        assert!(
            analysis.diagnostics.is_empty(),
            "{:#?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn alloc_effect_required_for_list_new() {
        let source = "
fn creates_list() -> List[F64] {
    list_new(10, 0.0)
}
";
        let analysis = analyze_source(source);
        assert!(!analysis.diagnostics.is_empty());
        let diag = analysis
            .diagnostics
            .iter()
            .find(|d| d.code == "semantic.alloc-effect")
            .unwrap();
        assert!(diag.message.contains("requires `alloc` effect"));
    }

    #[test]
    fn alloc_effect_satisfies_list_new() {
        let source = "
fn creates_list() -> List[F64] effects [alloc] {
    list_new(10, 0.0)
}
";
        let analysis = analyze_source(source);
        // Stage-0 (Core profile): [alloc] functions that return pointer types trigger a warning
        // because the compiler cannot verify the caller maintains proper scope.
        // The [alloc] effect itself is still satisfied when declared.
        let alloc_escape_warnings: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|d| d.code == "semantic.alloc-escape")
            .collect();
        let alloc_effect_errors: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|d| d.code == "semantic.alloc-effect")
            .collect();
        assert!(
            alloc_effect_errors.is_empty(),
            "should have no alloc-effect errors when [alloc] is declared, got: {:#?}",
            alloc_effect_errors
        );
        assert!(
            !alloc_escape_warnings.is_empty(),
            "should have at least one alloc-escape warning for returning List, got: {:#?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn alloc_effect_blocks_rt_profile() {
        let source = "
fn creates_list() -> List[F64] effects [alloc] {
    list_new(10, 0.0)
}
";
        let lexed = sarif_syntax::lexer::lex(source);
        let parsed = sarif_syntax::parser::parse(&lexed.tokens);
        let ast = sarif_syntax::ast::lower(&parsed.root);
        let hir = hir::lower(&ast.file);
        let analysis = analyze(&hir.module, Profile::Rt);
        // Stage-1 (RT profile): [alloc] functions that return pointer types trigger a hard error
        // via Escape Analysis. This prevents unsafe code from compiling.
        let escape_analysis_errors: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|d| d.code == "escape.analysis.required")
            .collect();
        let alloc_effect_errors: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|d| d.code == "semantic.alloc-effect")
            .collect();
        assert!(
            alloc_effect_errors.is_empty(),
            "should have no alloc-effect errors when [alloc] is declared, got: {:#?}",
            alloc_effect_errors
        );
        assert!(
            !escape_analysis_errors.is_empty(),
            "should have at least one escape.analysis.required error for RT profile, got: {:#?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn flags_profile_and_duplicate_effect_errors() {
        let source = "fn main() -> I32 effects [io, io] { 0 }";
        let lexed = sarif_syntax::lexer::lex(source);
        let parsed = sarif_syntax::parser::parse(&lexed.tokens);
        let ast = sarif_syntax::ast::lower(&parsed.root);
        let hir = hir::lower(&ast.file);
        let analysis = analyze(&hir.module, Profile::Total);

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|diag| diag.code == "semantic.duplicate-effect")
        );
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|diag| diag.code == "semantic.total-effect")
        );
    }

    #[test]
    fn accepts_exhaustive_scalar_match_patterns() {
        let analysis = analyze_source(
            "fn main() -> I32 { if match true { true => { match 41 { 40 => { false }, 41 => { match \"sarif\" { \"sarif\" => { true }, _ => { false }, } }, _ => { false }, } }, false => { false }, } { 42 } else { 0 } }",
        );

        assert!(
            !analysis
                .diagnostics
                .iter()
                .any(|diag| diag.code.starts_with("semantic.match"))
        );
    }

    #[test]
    fn rejects_non_exhaustive_integer_match_patterns() {
        let analysis = analyze_source("fn main() -> I32 { match 1 { 0 => { 0 }, } }");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|diag| diag.code == "semantic.match-exhaustive")
        );
    }

    #[test]
    fn accepts_explicit_array_types() {
        let analysis = analyze_source(
            "struct Grid { rows: [[I32; 2]; 2], }\nfn first(xs: [I32; 2]) -> I32 { xs[0] }\nfn main() -> I32 { let grid = Grid { rows: [[20, 22], [0, 0]] }; first(grid.rows[0]) }",
        );

        assert!(
            analysis.diagnostics.is_empty(),
            "{:#?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn accepts_const_generic_array_lengths() {
        let analysis = analyze_source("fn first[N](xs: [I32; N]) -> I32 { xs[0] }");

        assert!(
            analysis.diagnostics.is_empty(),
            "{:#?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn renders_const_generic_function_signatures() {
        let analysis = analyze_source("fn first[N](xs: [I32; N]) -> I32 { xs[0] }");
        let signature = analysis
            .reports
            .iter()
            .find_map(|report| match report {
                crate::semantic::ItemReport::Function(function) if function.name == "first" => {
                    Some(function.signature.as_str())
                }
                _ => None,
            })
            .expect("function report should exist");

        assert_eq!(signature, "fn first[N](xs: [I32; N]) -> I32");
    }

    #[test]
    fn rejects_unknown_array_length_generic_parameters() {
        let analysis = analyze_source("fn first(xs: [I32; N]) -> I32 { xs[0] }");

        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|diag| diag.code == "semantic.unknown-type"),
            "{:#?}",
            analysis.diagnostics
        );
    }
}
