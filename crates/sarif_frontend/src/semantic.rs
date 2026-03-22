use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::hir::{Body, Effect, EffectRef, Expr, Module, Stmt, TypeRef};
use crate::ownership::{
    ParamUsage, check_affine_body_ownership, check_contract_affine_ownership, infer_param_modes,
    is_affine_type, struct_is_affine,
};
use sarif_syntax::{Diagnostic, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Profile {
    Core,
    Total,
    Rt,
}

impl Profile {
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Total => "total",
            Self::Rt => "rt",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeArrayLen {
    Literal(usize),
    Param(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    I32,
    F64,
    Bool,
    Text,
    TextBuilder,
    F64Vec,
    Unit,
    Named(String),
    Param(String),
    Array(Box<Self>, TypeArrayLen),
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
            Self::TextBuilder => "TextBuilder".to_owned(),
            Self::F64Vec => "F64Vec".to_owned(),
            Self::Unit => "Unit".to_owned(),
            Self::Named(name) => name.clone(),
            Self::Param(name) => name.clone(),
            Self::Array(element, size) => {
                let size_str = match size {
                    TypeArrayLen::Literal(l) => l.to_string(),
                    TypeArrayLen::Param(p) => p.clone(),
                };
                format!("[{}; {size_str}]", element.render())
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

#[derive(Clone, Debug)]
pub struct FunctionSignature {
    pub params: Vec<(String, Type, Span)>,
    pub param_usages: Vec<ParamUsage>,
    pub return_type: Type,
    pub effects: Vec<Effect>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ConstSignature {
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EnumVariantInfo {
    pub name: String,
    pub payload: Option<Type>,
}

#[must_use]
pub fn analyze(module: &Module, profile: Profile) -> Analysis {
    let mut diagnostics = Vec::new();
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
        "TextBuilder".to_owned(),
        "F64Vec".to_owned(),
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
                    &mut diagnostics,
                    &known_types,
                    &const_item.ty.path,
                    const_item.ty.span,
                    "type",
                );
                consts.insert(
                    const_item.name.clone(),
                    ConstSignature {
                        ty: type_from_ref(&const_item.ty),
                        span: const_item.span,
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

                let mut params = Vec::new();
                for param in &function.params {
                    check_type_exists(
                        &mut diagnostics,
                        &known_types,
                        &param.ty.path,
                        param.ty.span,
                        "parameter type",
                    );
                    params.push((param.name.clone(), type_from_ref(&param.ty), param.span));
                }

                if let Some(return_type) = &function.return_type {
                    check_type_exists(
                        &mut diagnostics,
                        &known_types,
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
                        param_usages: vec![ParamUsage::borrow_only(); params.len()],
                        params,
                        return_type: function
                            .return_type
                            .as_ref()
                            .map_or(Type::Unit, type_from_ref),
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
                            &mut diagnostics,
                            &known_types,
                            &payload.path,
                            payload.span,
                            "variant payload type",
                        );
                    }
                    variants.push(EnumVariantInfo {
                        name: variant.name.clone(),
                        payload: variant.payload.as_ref().map(type_from_ref),
                    });
                }
                enum_variants.insert(enum_item.name.clone(), variants);
            }
            crate::hir::Item::Struct(struct_item) => {
                let mut fields = Vec::new();
                let mut layout = Vec::new();
                for field in &struct_item.fields {
                    check_type_exists(
                        &mut diagnostics,
                        &known_types,
                        &field.ty.path,
                        field.ty.span,
                        "field type",
                    );
                    let ty = type_from_ref(&field.ty);
                    fields.push(ty.clone());
                    layout.push((field.name.clone(), ty));
                }
                struct_fields.insert(struct_item.name.clone(), fields);
                struct_layouts.insert(struct_item.name.clone(), layout);
            }
            crate::hir::Item::Effect(_) => {}
        }
    }

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
                    if info.ty != signature.return_type
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

fn type_is_rt_safe(ty: &Type, struct_fields: &BTreeMap<String, Vec<Type>>) -> bool {
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

fn body_contains_loop(body: &Body) -> bool {
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

#[derive(Clone, Debug)]
struct ExprInfo {
    ty: Type,
    calls: Vec<CallSite>,
}

#[derive(Clone, Debug)]
struct BodyInfo {
    ty: Type,
    calls: Vec<CallSite>,
    return_span: Span,
}

#[derive(Clone, Debug)]
struct BodyStatementsInfo {
    locals: HashMap<String, Type>,
    calls: Vec<CallSite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSite {
    pub callee: String,
}

enum ExprContext {
    Body,
    BodyTail,
    Statement,
    ContractRequires,
    ContractEnsures,
}

const fn nested_expr_context(context: &ExprContext) -> ExprContext {
    match context {
        ExprContext::Body | ExprContext::BodyTail | ExprContext::Statement => ExprContext::Body,
        ExprContext::ContractRequires => ExprContext::ContractRequires,
        ExprContext::ContractEnsures => ExprContext::ContractEnsures,
    }
}

const fn allows_runtime_builtin_context(context: &ExprContext) -> bool {
    matches!(
        context,
        ExprContext::Body | ExprContext::BodyTail | ExprContext::Statement
    )
}

fn require_runtime_builtin_context(
    code: &'static str,
    builtin: &str,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
    context: &ExprContext,
) {
    if allows_runtime_builtin_context(context) {
        return;
    }
    diagnostics.push(Diagnostic::new(
        code,
        format!("builtin `{builtin}` is only available in executable function bodies"),
        span,
        Some("Use this builtin inside a function body or body-tail expression.".to_owned()),
    ));
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn infer_expr(
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
        Expr::ContractResult(expr) => {
            if !matches!(context, ExprContext::ContractEnsures) {
                diagnostics.push(Diagnostic::new(
                    "semantic.contract-result-context",
                    "`result` is only available inside `ensures` clauses",
                    expr.span,
                    Some("Use local bindings or parameters instead.".to_owned()),
                ));
            }
            if let Some(ty) = locals.get("result") {
                ExprInfo {
                    ty: ty.clone(),
                    calls: Vec::new(),
                }
            } else {
                ExprInfo {
                    ty: Type::Error,
                    calls: Vec::new(),
                }
            }
        }
        Expr::Array(expr) => {
            let nested_context = nested_expr_context(context);
            let mut calls = Vec::new();
            let mut element_type = None::<Type>;
            let mut ok = true;

            if expr.elements.is_empty() {
                diagnostics.push(Diagnostic::new(
                    "semantic.array-empty",
                    format!("empty array literal in `{fn_name}` has no inferable type"),
                    expr.span,
                    Some("Use a non-empty array literal in stage-0.".to_owned()),
                ));
                ok = false;
            }

            for element in &expr.elements {
                let info = infer_expr(
                    element,
                    locals,
                    mutable_locals,
                    functions,
                    consts,
                    enum_variants,
                    struct_layouts,
                    diagnostics,
                    fn_name,
                    caller_effects,
                    &nested_context,
                );
                calls.extend(info.calls);
                if let Some(expected) = &element_type {
                    if *expected != info.ty && info.ty != Type::Error {
                        diagnostics.push(Diagnostic::new(
                            "semantic.array-element-type",
                            format!(
                                "array elements in `{fn_name}` must have the same type, found `{}` and `{}`",
                                expected.render(),
                                info.ty.render(),
                            ),
                            element.span(),
                            Some("Make all array elements the same type.".to_owned()),
                        ));
                        ok = false;
                    }
                } else {
                    element_type = Some(info.ty.clone());
                }
                if info.ty == Type::Error {
                    ok = false;
                }
            }

            ExprInfo {
                ty: if ok {
                    Type::Array(
                        Box::new(element_type.unwrap_or(Type::Error)),
                        TypeArrayLen::Literal(expr.elements.len()),
                    )
                } else {
                    Type::Error
                },
                calls,
            }
        }
        Expr::Index(expr) => {
            let nested_context = nested_expr_context(context);
            let base = infer_expr(
                &expr.base,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &nested_context,
            );
            let index = infer_expr(
                &expr.index,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &nested_context,
            );
            if index.ty != Type::I32 && index.ty != Type::Error {
                diagnostics.push(Diagnostic::new(
                    "semantic.array-index-type",
                    format!(
                        "array index in `{fn_name}` must be `I32`, found `{}`",
                        index.ty.render(),
                    ),
                    expr.index.span(),
                    Some("Use an integer index for stage-0 array access.".to_owned()),
                ));
            }
            let ty = match &base.ty {
                Type::Array(element, _) => (**element).clone(),
                Type::Error => Type::Error,
                other => {
                    diagnostics.push(Diagnostic::new(
                        "semantic.array-index-base",
                        format!(
                            "cannot index value of type `{}` in `{fn_name}`",
                            other.pretty(),
                        ),
                        expr.base.span(),
                        Some(
                            "Index into an array literal or an array-valued local binding."
                                .to_owned(),
                        ),
                    ));
                    Type::Error
                }
            };
            let mut calls = base.calls;
            calls.extend(index.calls);
            ExprInfo {
                ty: if base.ty == Type::Error || index.ty == Type::Error {
                    Type::Error
                } else {
                    ty
                },
                calls,
            }
        }
        Expr::If(expr) => {
            let condition = infer_expr(
                &expr.condition,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &ExprContext::Body,
            );
            if condition.ty != Type::Bool && condition.ty != Type::Error {
                diagnostics.push(Diagnostic::new(
                    "semantic.if-condition",
                    format!(
                        "`if` condition in `{fn_name}` must be `Bool`, found `{}`",
                        condition.ty.render(),
                    ),
                    expr.condition.span(),
                    Some("Use a boolean condition before the branch bodies.".to_owned()),
                ));
            }
            let then_info = infer_body(
                &expr.then_body,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
            );
            let else_info = infer_body(
                &expr.else_body,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
            );
            let mut calls = condition.calls;
            calls.extend(then_info.calls);
            calls.extend(else_info.calls);
            if then_info.ty != Type::Error
                && else_info.ty != Type::Error
                && then_info.ty != else_info.ty
            {
                diagnostics.push(Diagnostic::new(
                    "semantic.if-branch-type",
                    format!(
                        "`if` branches in `{fn_name}` must return the same type, found `{}` and `{}`",
                        then_info.ty.render(),
                        else_info.ty.render(),
                    ),
                    expr.span,
                    Some("Make both branch bodies produce the same type.".to_owned()),
                ));
            }
            let has_type_error = condition.ty == Type::Error
                || then_info.ty == Type::Error
                || else_info.ty == Type::Error
                || then_info.ty != else_info.ty;
            ExprInfo {
                ty: if has_type_error {
                    Type::Error
                } else {
                    then_info.ty
                },
                calls,
            }
        }
        Expr::Match(expr) => {
            let nested_context = nested_expr_context(context);
            let scrutinee = infer_expr(
                &expr.scrutinee,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &nested_context,
            );
            let enum_name = match &scrutinee.ty {
                Type::Named(name) if enum_variants.contains_key(name) => Some(name.clone()),
                _ => None,
            };
            let declared_variants = enum_name
                .as_ref()
                .and_then(|name| enum_variants.get(name))
                .cloned()
                .unwrap_or_default();
            let supported_scrutinee = matches!(
                scrutinee.ty,
                Type::I32 | Type::Bool | Type::Text | Type::Error
            ) || enum_name.is_some();
            if !supported_scrutinee && scrutinee.ty != Type::Error {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-scrutinee",
                    format!(
                        "`match` in `{fn_name}` requires `I32`, `Bool`, `Text`, or an enum scrutinee, found `{}`",
                        scrutinee.ty.render(),
                    ),
                    expr.scrutinee.span(),
                    Some("Match over an integer, boolean, text, or declared enum value.".to_owned()),
                ));
            }

            let mut seen_enum_variants = BTreeSet::<String>::new();
            let mut seen_int_patterns = BTreeSet::<i64>::new();
            let mut seen_text_patterns = BTreeSet::<String>::new();
            let mut seen_true = false;
            let mut seen_false = false;
            let mut has_wildcard = false;
            let mut branch_type = None::<Type>;
            let mut calls = scrutinee.calls;
            let mut ok = supported_scrutinee || scrutinee.ty == Type::Error;
            for (arm_index, arm) in expr.arms.iter().enumerate() {
                let is_last_arm = arm_index + 1 == expr.arms.len();
                let mut variant_payload = None::<Type>;

                match (&scrutinee.ty, &arm.pattern) {
                    (_, crate::hir::MatchPattern::Wildcard { span }) => {
                        if has_wildcard {
                            diagnostics.push(Diagnostic::new(
                                "semantic.match-pattern",
                                format!("wildcard arm `_` appears more than once in `{fn_name}`"),
                                *span,
                                Some("Keep at most one wildcard arm.".to_owned()),
                            ));
                            ok = false;
                        }
                        if !is_last_arm {
                            diagnostics.push(Diagnostic::new(
                                "semantic.match-pattern",
                                "wildcard arm `_` must be the final match arm",
                                *span,
                                Some(
                                    "Move `_` to the final arm so later arms stay reachable."
                                        .to_owned(),
                                ),
                            ));
                            ok = false;
                        }
                        has_wildcard = true;
                    }
                    (
                        Type::Named(expected_enum),
                        crate::hir::MatchPattern::Variant {
                            path,
                            binding,
                            span,
                        },
                    ) if enum_name.is_some() => {
                        let Some((pattern_enum, variant_name)) =
                            split_enum_variant_path(&path.path)
                        else {
                            diagnostics.push(Diagnostic::new(
                                "semantic.match-pattern",
                                format!(
                                    "match arm in `{fn_name}` must use `Enum.variant`, found `{}`",
                                    path.path,
                                ),
                                *span,
                                Some("Rewrite the arm pattern as `Enum.variant`.".to_owned()),
                            ));
                            ok = false;
                            continue;
                        };
                        if pattern_enum != expected_enum {
                            diagnostics.push(Diagnostic::new(
                                "semantic.match-pattern",
                                format!(
                                    "match arm `{}` does not belong to enum `{expected_enum}`",
                                    arm.pattern.pretty(),
                                ),
                                *span,
                                Some("Use variants from the scrutinee's enum only.".to_owned()),
                            ));
                            ok = false;
                        } else if !declared_variants
                            .iter()
                            .any(|variant| variant.name == variant_name)
                        {
                            let suggestion = best_match(
                                variant_name,
                                declared_variants
                                    .iter()
                                    .map(|variant| variant.name.as_str()),
                            );
                            diagnostics.push(Diagnostic::new(
                                "semantic.match-pattern",
                                format!("enum `{expected_enum}` has no variant `{variant_name}`"),
                                *span,
                                Some(suggestion_help(suggestion, || {
                                    "Use one of the declared enum variants.".to_owned()
                                })),
                            ));
                            ok = false;
                        } else if !seen_enum_variants.insert(variant_name.to_owned()) {
                            diagnostics.push(Diagnostic::new(
                                "semantic.match-pattern",
                                format!(
                                    "match arm `{}` appears more than once in `{fn_name}`",
                                    arm.pattern.pretty(),
                                ),
                                *span,
                                Some("Keep one arm per enum variant.".to_owned()),
                            ));
                            ok = false;
                        }

                        variant_payload = declared_variants
                            .iter()
                            .find(|variant| variant.name == variant_name)
                            .and_then(|variant| variant.payload.clone());

                        if variant_payload.is_none() && binding.is_some() {
                            diagnostics.push(Diagnostic::new(
                                "semantic.match-pattern",
                                format!(
                                    "payload binding `{}` is only valid for payload-carrying variants",
                                    binding.as_deref().unwrap_or("_"),
                                ),
                                *span,
                                Some(
                                    "Remove the binding, or match a variant that carries a payload."
                                        .to_owned(),
                                ),
                            ));
                            ok = false;
                        }
                    }
                    (Type::Bool, crate::hir::MatchPattern::Bool { value, span }) => {
                        let seen = if *value {
                            &mut seen_true
                        } else {
                            &mut seen_false
                        };
                        if *seen {
                            diagnostics.push(Diagnostic::new(
                                "semantic.match-pattern",
                                format!(
                                    "match arm `{}` appears more than once in `{fn_name}`",
                                    arm.pattern.pretty(),
                                ),
                                *span,
                                Some("Keep one arm per boolean value.".to_owned()),
                            ));
                            ok = false;
                        }
                        *seen = true;
                    }
                    (Type::I32, crate::hir::MatchPattern::Integer { value, span }) => {
                        if !seen_int_patterns.insert(*value) {
                            diagnostics.push(Diagnostic::new(
                                "semantic.match-pattern",
                                format!(
                                    "match arm `{}` appears more than once in `{fn_name}`",
                                    arm.pattern.pretty(),
                                ),
                                *span,
                                Some("Keep one arm per integer literal.".to_owned()),
                            ));
                            ok = false;
                        }
                    }
                    (Type::Text, crate::hir::MatchPattern::String { value, span, .. }) => {
                        if !seen_text_patterns.insert(value.clone()) {
                            diagnostics.push(Diagnostic::new(
                                "semantic.match-pattern",
                                format!(
                                    "match arm `{}` appears more than once in `{fn_name}`",
                                    arm.pattern.pretty(),
                                ),
                                *span,
                                Some("Keep one arm per text literal.".to_owned()),
                            ));
                            ok = false;
                        }
                    }
                    (Type::Error, _) => {}
                    (
                        _,
                        crate::hir::MatchPattern::Variant { span, .. }
                        | crate::hir::MatchPattern::Integer { span, .. }
                        | crate::hir::MatchPattern::String { span, .. }
                        | crate::hir::MatchPattern::Bool { span, .. },
                    ) => {
                        diagnostics.push(Diagnostic::new(
                            "semantic.match-pattern",
                            format!(
                                "match arm `{}` is not compatible with scrutinee type `{}`",
                                arm.pattern.pretty(),
                                scrutinee.ty.render(),
                            ),
                            *span,
                            Some("Use enum variants, matching literal kinds, or `_`.".to_owned()),
                        ));
                        ok = false;
                    }
                }

                let mut arm_locals = locals.clone();
                if let (
                    crate::hir::MatchPattern::Variant {
                        binding: Some(binding),
                        ..
                    },
                    Some(payload_ty),
                ) = (&arm.pattern, variant_payload)
                {
                    arm_locals.insert(binding.clone(), payload_ty);
                }

                let body = infer_body(
                    &arm.body,
                    &arm_locals,
                    mutable_locals,
                    functions,
                    consts,
                    enum_variants,
                    struct_layouts,
                    diagnostics,
                    fn_name,
                    caller_effects,
                );
                calls.extend(body.calls);
                if let Some(expected) = &branch_type {
                    if *expected != body.ty && body.ty != Type::Error {
                        diagnostics.push(Diagnostic::new(
                            "semantic.match-branch-type",
                            format!(
                                "match arms in `{fn_name}` must return the same type, found `{}` and `{}`",
                                expected.render(),
                                body.ty.render(),
                            ),
                            arm.body.span,
                            Some("Make every match arm produce the same type.".to_owned()),
                        ));
                        ok = false;
                    }
                } else {
                    branch_type = Some(body.ty.clone());
                }
                if body.ty == Type::Error {
                    ok = false;
                }
            }
            if let Some(expected_enum) = &enum_name {
                let missing = declared_variants
                    .iter()
                    .filter(|variant| !seen_enum_variants.contains(variant.name.as_str()))
                    .map(|variant| variant.name.clone())
                    .collect::<Vec<_>>();
                if !missing.is_empty() && !has_wildcard {
                    diagnostics.push(Diagnostic::new(
                        "semantic.match-exhaustive",
                        format!(
                            "match in `{fn_name}` is not exhaustive for enum `{expected_enum}`; missing {}",
                            missing.join(", "),
                        ),
                        expr.span,
                        Some("Add one arm for every declared enum variant.".to_owned()),
                    ));
                    ok = false;
                }
            } else if scrutinee.ty == Type::Bool {
                if !(has_wildcard || (seen_true && seen_false)) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.match-exhaustive",
                        format!(
                            "match in `{fn_name}` is not exhaustive for `Bool`; add the missing boolean arm or `_`"
                        ),
                        expr.span,
                        Some("Cover both `true` and `false`, or end with `_`.".to_owned()),
                    ));
                    ok = false;
                }
            } else if matches!(scrutinee.ty, Type::I32 | Type::Text) && !has_wildcard {
                diagnostics.push(Diagnostic::new(
                    "semantic.match-exhaustive",
                    format!(
                        "match in `{fn_name}` over `{}` requires a final `_` fallback arm",
                        scrutinee.ty.render(),
                    ),
                    expr.span,
                    Some("End integer and text matches with `_ => { ... }`.".to_owned()),
                ));
                ok = false;
            }
            ExprInfo {
                ty: if ok {
                    branch_type.unwrap_or(Type::Unit)
                } else {
                    Type::Error
                },
                calls,
            }
        }
        Expr::Repeat(expr) => {
            if !matches!(context, ExprContext::BodyTail | ExprContext::Statement) {
                diagnostics.push(Diagnostic::new(
                    "semantic.repeat-position",
                    format!(
                        "`repeat` in `{fn_name}` is only allowed as a direct body tail expression or statement"
                    ),
                    expr.span,
                    Some(
                        "Place `repeat` directly at the end of a body, or use it as an explicit statement."
                            .to_owned(),
                    ),
                ));
            }
            let count = infer_expr(
                &expr.count,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &ExprContext::Body,
            );
            if count.ty != Type::I32 && count.ty != Type::Error {
                diagnostics.push(Diagnostic::new(
                    "semantic.repeat-count",
                    format!(
                        "`repeat` count in `{fn_name}` must be `I32`, found `{}`",
                        count.ty.render(),
                    ),
                    expr.count.span(),
                    Some("Use an integer iteration count for stage-0 repeat loops.".to_owned()),
                ));
            }
            let mut body_locals = locals.clone();
            if let Some(binding) = &expr.binding {
                if body_locals.contains_key(binding) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.duplicate-local",
                        format!(
                            "local binding `{binding}` is already declared in `{fn_name}`"
                        ),
                        expr.span,
                        Some(
                            "Use a fresh loop index name instead of shadowing parameters or earlier locals."
                                .to_owned(),
                        ),
                    ));
                } else {
                    body_locals.insert(binding.clone(), Type::I32);
                }
            }
            let body = infer_body(
                &expr.body,
                &body_locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
            );
            let mut calls = count.calls;
            calls.extend(body.calls);
            ExprInfo {
                ty: Type::Unit,
                calls,
            }
        }
        Expr::While(expr) => {
            if !matches!(context, ExprContext::BodyTail | ExprContext::Statement) {
                diagnostics.push(Diagnostic::new(
                    "semantic.while-position",
                    format!(
                        "`while` in `{fn_name}` is only allowed as a direct body tail expression or statement"
                    ),
                    expr.span,
                    Some(
                        "Place `while` directly at the end of a body, or use it as an explicit statement."
                            .to_owned(),
                    ),
                ));
            }
            let condition = infer_expr(
                &expr.condition,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &ExprContext::Body,
            );
            if condition.ty != Type::Bool && condition.ty != Type::Error {
                diagnostics.push(Diagnostic::new(
                    "semantic.while-condition",
                    format!(
                        "`while` condition in `{fn_name}` must be `Bool`, found `{}`",
                        condition.ty.render(),
                    ),
                    expr.condition.span(),
                    Some("Use a boolean condition for stage-0 `while` loops.".to_owned()),
                ));
            }
            let body = infer_body(
                &expr.body,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
            );
            let mut calls = condition.calls;
            calls.extend(body.calls);
            ExprInfo {
                ty: Type::Unit,
                calls,
            }
        }
        Expr::Group(expr) => {
            let nested_context = nested_expr_context(context);
            infer_expr(
                &expr.inner,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &nested_context,
            )
        }
        Expr::Unary(expr) => {
            let nested_context = nested_expr_context(context);
            let inner = infer_expr(
                &expr.inner,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &nested_context,
            );
            let ty = match expr.op {
                crate::hir::UnaryOp::Not => {
                    expect_type(
                        diagnostics,
                        &expr.inner,
                        &inner.ty,
                        &Type::Bool,
                        expr.op.symbol(),
                        "operand",
                        "Use a boolean operand with `not`.",
                    );
                    Type::Bool
                }
            };
            ExprInfo {
                ty: if inner.ty == Type::Error {
                    Type::Error
                } else {
                    ty
                },
                calls: inner.calls,
            }
        }
        Expr::Field(expr) => {
            if let Some(enum_name) = enum_literal_type_name(
                &expr.base,
                &expr.field,
                enum_variants,
                diagnostics,
                expr.span,
            ) {
                return ExprInfo {
                    ty: Type::Named(enum_name),
                    calls: Vec::new(),
                };
            }
            let nested_context = nested_expr_context(context);
            let base = infer_expr(
                &expr.base,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &nested_context,
            );
            let ty = if let Some(ty) = field_type(&base.ty, &expr.field, struct_layouts) {
                ty
            } else {
                let suggestion = field_names_for_type(&base.ty, struct_layouts)
                    .and_then(|fields| best_match(&expr.field, fields.iter().map(String::as_str)));
                diagnostics.push(Diagnostic::new(
                    "semantic.field-access",
                    format!(
                        "cannot read field `{}` from value of type `{}`",
                        expr.field,
                        base.ty.render(),
                    ),
                    expr.span,
                    Some(suggestion_help(suggestion, || {
                        "Use a declared struct value and one of its field names.".to_owned()
                    })),
                ));
                Type::Error
            };
            ExprInfo {
                ty: if base.ty == Type::Error || ty == Type::Error {
                    Type::Error
                } else {
                    ty
                },
                calls: base.calls,
            }
        }
        Expr::Record(expr) => {
            let nested_context = nested_expr_context(context);
            let mut calls = Vec::new();
            let field_infos = expr
                .fields
                .iter()
                .map(|field| {
                    let info = infer_expr(
                        &field.value,
                        locals,
                        mutable_locals,
                        functions,
                        consts,
                        enum_variants,
                        struct_layouts,
                        diagnostics,
                        fn_name,
                        caller_effects,
                        &nested_context,
                    );
                    calls.extend(info.calls.iter().cloned());
                    (field, info)
                })
                .collect::<Vec<_>>();

            let Some(layout) = struct_layouts.get(&expr.name) else {
                let suggestion = best_match(&expr.name, struct_layouts.keys().map(String::as_str));
                diagnostics.push(Diagnostic::new(
                    "semantic.record-type",
                    format!("record literal uses unknown struct `{}`", expr.name),
                    expr.span,
                    Some(suggestion_help(suggestion, || {
                        "Declare the struct before constructing it.".to_owned()
                    })),
                ));
                return ExprInfo {
                    ty: Type::Error,
                    calls,
                };
            };

            let mut ok = true;
            let mut seen = HashSet::<String>::new();
            for (field, info) in &field_infos {
                let Some(expected) = layout
                    .iter()
                    .find_map(|(name, ty)| (name == &field.name).then_some(ty))
                else {
                    let suggestion =
                        best_match(&field.name, layout.iter().map(|(name, _)| name.as_str()));
                    diagnostics.push(Diagnostic::new(
                        "semantic.record-field",
                        format!("struct `{}` has no field `{}`", expr.name, field.name),
                        field.span,
                        Some(suggestion_help(suggestion, || {
                            "Use one of the declared struct fields.".to_owned()
                        })),
                    ));
                    ok = false;
                    continue;
                };
                if !seen.insert(field.name.clone()) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.record-field",
                        format!(
                            "record literal for `{}` initializes field `{}` more than once",
                            expr.name, field.name
                        ),
                        field.span,
                        Some("Initialize each field exactly once.".to_owned()),
                    ));
                    ok = false;
                }
                if info.ty != *expected && info.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.record-field-type",
                        format!(
                            "field `{}` of `{}` expects `{}`, found `{}`",
                            field.name,
                            expr.name,
                            expected.render(),
                            info.ty.render(),
                        ),
                        field.span,
                        Some(
                            "Make the field initializer match the declared field type.".to_owned(),
                        ),
                    ));
                    ok = false;
                }
            }

            for (field_name, _) in layout {
                if !seen.contains(field_name) {
                    diagnostics.push(Diagnostic::new(
                        "semantic.record-field",
                        format!(
                            "record literal for `{}` is missing field `{field_name}`",
                            expr.name
                        ),
                        expr.span,
                        Some("Initialize every declared field exactly once.".to_owned()),
                    ));
                    ok = false;
                }
            }

            if field_infos.iter().any(|(_, info)| info.ty == Type::Error) {
                ok = false;
            }

            ExprInfo {
                ty: if ok {
                    Type::Named(expr.name.clone())
                } else {
                    Type::Error
                },
                calls,
            }
        }
        Expr::Binary(expr) => {
            let nested_context = nested_expr_context(context);
            let left = infer_expr(
                &expr.left,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &nested_context,
            );
            let right = infer_expr(
                &expr.right,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
                &nested_context,
            );
            let mut calls = left.calls;
            calls.extend(right.calls);
            let ty = match expr.op {
                crate::hir::BinaryOp::And | crate::hir::BinaryOp::Or => {
                    expect_type(
                        diagnostics,
                        &expr.left,
                        &left.ty,
                        &Type::Bool,
                        expr.op.symbol(),
                        "left",
                        "Use boolean operands with `and` and `or`.",
                    );
                    expect_type(
                        diagnostics,
                        &expr.right,
                        &right.ty,
                        &Type::Bool,
                        expr.op.symbol(),
                        "right",
                        "Use boolean operands with `and` and `or`.",
                    );
                    Type::Bool
                }
                crate::hir::BinaryOp::Add
                | crate::hir::BinaryOp::Sub
                | crate::hir::BinaryOp::Mul
                | crate::hir::BinaryOp::Div => {
                    if let Some(ty) = matching_numeric_type(&left.ty, &right.ty) {
                        ty
                    } else {
                        if left.ty != Type::Error && right.ty != Type::Error {
                            diagnostics.push(Diagnostic::new(
                                "semantic.binary-type",
                                format!(
                                    "operands of `{}` must both be `I32` or both be `F64`, found `{}` and `{}`",
                                    expr.op.symbol(),
                                    left.ty.render(),
                                    right.ty.render(),
                                ),
                                expr.span,
                                Some(
                                    "Use matching numeric operand types for arithmetic."
                                        .to_owned(),
                                ),
                            ));
                        }
                        Type::Error
                    }
                }
                crate::hir::BinaryOp::Lt
                | crate::hir::BinaryOp::Le
                | crate::hir::BinaryOp::Gt
                | crate::hir::BinaryOp::Ge => {
                    if matching_numeric_type(&left.ty, &right.ty).is_none()
                        && left.ty != Type::Error
                        && right.ty != Type::Error
                    {
                        diagnostics.push(Diagnostic::new(
                            "semantic.binary-type",
                            format!(
                                "operands of `{}` must both be `I32` or both be `F64`, found `{}` and `{}`",
                                expr.op.symbol(),
                                left.ty.render(),
                                right.ty.render(),
                            ),
                            expr.span,
                            Some(
                                "Use matching numeric operand types for comparisons.".to_owned(),
                            ),
                        ));
                    }
                    Type::Bool
                }
                crate::hir::BinaryOp::Eq | crate::hir::BinaryOp::Ne => {
                    if left.ty != Type::Error && right.ty != Type::Error && left.ty != right.ty {
                        diagnostics.push(Diagnostic::new(
                            "semantic.binary-type",
                            format!(
                                "operands of `{}` must have the same type, found `{}` and `{}`",
                                expr.op.symbol(),
                                left.ty.render(),
                                right.ty.render(),
                            ),
                            expr.span,
                            Some("Compare values of the same type.".to_owned()),
                        ));
                    }
                    Type::Bool
                }
            };
            ExprInfo {
                ty: if left.ty == Type::Error || right.ty == Type::Error {
                    Type::Error
                } else {
                    ty
                },
                calls,
            }
        }
        Expr::Call(expr) => {
            let nested_context = nested_expr_context(context);
            let mut calls = Vec::new();
            let args = expr
                .args
                .iter()
                .map(|arg| {
                    let info = infer_expr(
                        arg,
                        locals,
                        mutable_locals,
                        functions,
                        consts,
                        enum_variants,
                        struct_layouts,
                        diagnostics,
                        fn_name,
                        caller_effects,
                        &nested_context,
                    );
                    calls.extend(info.calls.iter().cloned());
                    info
                })
                .collect::<Vec<_>>();

            if expr.callee == ARRAY_LEN_BUILTIN && !functions.contains_key(ARRAY_LEN_BUILTIN) {
                if args.len() != 1 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.len-arity",
                        format!(
                            "builtin `{ARRAY_LEN_BUILTIN}` expects 1 argument but got {}",
                            args.len(),
                        ),
                        expr.span,
                        Some("Call `len(xs)` with exactly one array argument.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                let arg = &args[0];
                return match &arg.ty {
                    Type::Array(_, _) => ExprInfo {
                        ty: Type::I32,
                        calls,
                    },
                    Type::Error => ExprInfo {
                        ty: Type::Error,
                        calls,
                    },
                    _ => {
                        diagnostics.push(Diagnostic::new(
                            "semantic.len-type",
                            format!(
                                "builtin `{ARRAY_LEN_BUILTIN}` expects an array argument, found `{}`",
                                arg.ty.render(),
                            ),
                            expr.span,
                            Some("Pass an internal stage-0 array such as `len(xs)`.".to_owned()),
                        ));
                        ExprInfo {
                            ty: Type::Error,
                            calls,
                        }
                    }
                };
            }

            if expr.callee == "text_len" && !functions.contains_key("text_len") {
                if args.len() != 1 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_len-arity",
                        format!(
                            "builtin `text_len` expects 1 argument but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `text_len(text)` with exactly one Text argument.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                let arg = &args[0];
                return match &arg.ty {
                    Type::Text => ExprInfo {
                        ty: Type::I32,
                        calls,
                    },
                    Type::Error => ExprInfo {
                        ty: Type::Error,
                        calls,
                    },
                    _ => {
                        diagnostics.push(Diagnostic::new(
                            "semantic.text_len-type",
                            format!(
                                "builtin `text_len` expects a Text argument, found `{}`",
                                arg.ty.render()
                            ),
                            expr.span,
                            Some("Pass a Text argument.".to_owned()),
                        ));
                        ExprInfo {
                            ty: Type::Error,
                            calls,
                        }
                    }
                };
            }

            if expr.callee == "text_byte" && !functions.contains_key("text_byte") {
                if args.len() != 2 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_byte-arity",
                        format!(
                            "builtin `text_byte` expects 2 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `text_byte(text, index)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                let first_arg = &args[0];
                let second_arg = &args[1];
                if first_arg.ty != Type::Text && first_arg.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_byte-type",
                        format!(
                            "builtin `text_byte` first argument must be Text, found `{}`",
                            first_arg.ty.render()
                        ),
                        expr.span,
                        Some("Pass a Text argument.".to_owned()),
                    ));
                }
                if second_arg.ty != Type::I32 && second_arg.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_byte-type",
                        format!(
                            "builtin `text_byte` second argument must be I32, found `{}`",
                            second_arg.ty.render()
                        ),
                        expr.span,
                        Some("Pass an I32 index.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::I32,
                    calls,
                };
            }

            if expr.callee == "text_concat" && !functions.contains_key("text_concat") {
                if args.len() != 2 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_concat-arity",
                        format!(
                            "builtin `text_concat` expects 2 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `text_concat(left, right)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                let left = &args[0];
                let right = &args[1];
                if left.ty != Type::Text && left.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_concat-type",
                        format!(
                            "builtin `text_concat` first argument must be Text, found `{}`",
                            left.ty.render()
                        ),
                        expr.span,
                        Some("Pass a Text argument.".to_owned()),
                    ));
                }
                if right.ty != Type::Text && right.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_concat-type",
                        format!(
                            "builtin `text_concat` second argument must be Text, found `{}`",
                            right.ty.render()
                        ),
                        expr.span,
                        Some("Pass a Text argument.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::Text,
                    calls,
                };
            }

            if expr.callee == "text_slice" && !functions.contains_key("text_slice") {
                if args.len() != 3 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_slice-arity",
                        format!(
                            "builtin `text_slice` expects 3 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `text_slice(text, start, end)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                let text = &args[0];
                let start = &args[1];
                let end = &args[2];
                if text.ty != Type::Text && text.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_slice-type",
                        format!(
                            "builtin `text_slice` first argument must be Text, found `{}`",
                            text.ty.render()
                        ),
                        expr.span,
                        Some("Pass a Text argument.".to_owned()),
                    ));
                }
                if start.ty != Type::I32 && start.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_slice-type",
                        format!(
                            "builtin `text_slice` second argument must be I32, found `{}`",
                            start.ty.render()
                        ),
                        expr.span,
                        Some("Pass an I32 start offset.".to_owned()),
                    ));
                }
                if end.ty != Type::I32 && end.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_slice-type",
                        format!(
                            "builtin `text_slice` third argument must be I32, found `{}`",
                            end.ty.render()
                        ),
                        expr.span,
                        Some("Pass an I32 end offset.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::Text,
                    calls,
                };
            }

            if expr.callee == "text_builder_new" && !functions.contains_key("text_builder_new") {
                require_runtime_builtin_context(
                    "semantic.text_builder_new-runtime-context",
                    "text_builder_new",
                    expr.span,
                    diagnostics,
                    context,
                );
                if !args.is_empty() {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_builder_new-arity",
                        format!(
                            "builtin `text_builder_new` expects 0 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `text_builder_new()` with no arguments.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                return ExprInfo {
                    ty: Type::TextBuilder,
                    calls,
                };
            }

            if expr.callee == "text_builder_append"
                && !functions.contains_key("text_builder_append")
            {
                require_runtime_builtin_context(
                    "semantic.text_builder_append-runtime-context",
                    "text_builder_append",
                    expr.span,
                    diagnostics,
                    context,
                );
                if args.len() != 2 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_builder_append-arity",
                        format!(
                            "builtin `text_builder_append` expects 2 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `text_builder_append(builder, text)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::TextBuilder && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_builder_append-type",
                        format!(
                            "builtin `text_builder_append` first argument must be TextBuilder, found `{}`",
                            args[0].ty.render(),
                        ),
                        expr.span,
                        Some("Pass a TextBuilder accumulator.".to_owned()),
                    ));
                }
                if args[1].ty != Type::Text && args[1].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_builder_append-type",
                        format!(
                            "builtin `text_builder_append` second argument must be Text, found `{}`",
                            args[1].ty.render(),
                        ),
                        expr.span,
                        Some("Pass a Text value to append.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::TextBuilder,
                    calls,
                };
            }

            if expr.callee == "text_builder_finish"
                && !functions.contains_key("text_builder_finish")
            {
                require_runtime_builtin_context(
                    "semantic.text_builder_finish-runtime-context",
                    "text_builder_finish",
                    expr.span,
                    diagnostics,
                    context,
                );
                if args.len() != 1 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_builder_finish-arity",
                        format!(
                            "builtin `text_builder_finish` expects 1 argument but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `text_builder_finish(builder)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::TextBuilder && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_builder_finish-type",
                        format!(
                            "builtin `text_builder_finish` expects TextBuilder, found `{}`",
                            args[0].ty.render(),
                        ),
                        expr.span,
                        Some("Pass a TextBuilder value.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::Text,
                    calls,
                };
            }

            if expr.callee == "f64_vec_new" && !functions.contains_key("f64_vec_new") {
                require_runtime_builtin_context(
                    "semantic.f64_vec-runtime-context",
                    "f64_vec_new",
                    expr.span,
                    diagnostics,
                    context,
                );
                if args.len() != 2 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_new-arity",
                        format!(
                            "builtin `f64_vec_new` expects 2 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `f64_vec_new(len, fill)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::I32 && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_new-type",
                        format!(
                            "builtin `f64_vec_new` first argument must be I32, found `{}`",
                            args[0].ty.render(),
                        ),
                        expr.span,
                        Some("Pass an integer length.".to_owned()),
                    ));
                }
                if args[1].ty != Type::F64 && args[1].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_new-type",
                        format!(
                            "builtin `f64_vec_new` second argument must be F64, found `{}`",
                            args[1].ty.render(),
                        ),
                        expr.span,
                        Some("Pass a float fill value.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::F64Vec,
                    calls,
                };
            }

            if expr.callee == "f64_vec_len" && !functions.contains_key("f64_vec_len") {
                require_runtime_builtin_context(
                    "semantic.f64_vec-runtime-context",
                    "f64_vec_len",
                    expr.span,
                    diagnostics,
                    context,
                );
                if args.len() != 1 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_len-arity",
                        format!(
                            "builtin `f64_vec_len` expects 1 argument but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `f64_vec_len(vec)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::F64Vec && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_len-type",
                        format!(
                            "builtin `f64_vec_len` expects F64Vec, found `{}`",
                            args[0].ty.render(),
                        ),
                        expr.span,
                        Some("Pass an F64Vec value.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::I32,
                    calls,
                };
            }

            if expr.callee == "f64_vec_get" && !functions.contains_key("f64_vec_get") {
                require_runtime_builtin_context(
                    "semantic.f64_vec-runtime-context",
                    "f64_vec_get",
                    expr.span,
                    diagnostics,
                    context,
                );
                if args.len() != 2 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_get-arity",
                        format!(
                            "builtin `f64_vec_get` expects 2 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `f64_vec_get(vec, index)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::F64Vec && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_get-type",
                        format!(
                            "builtin `f64_vec_get` first argument must be F64Vec, found `{}`",
                            args[0].ty.render(),
                        ),
                        expr.span,
                        Some("Pass an F64Vec value.".to_owned()),
                    ));
                }
                if args[1].ty != Type::I32 && args[1].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_get-type",
                        format!(
                            "builtin `f64_vec_get` second argument must be I32, found `{}`",
                            args[1].ty.render(),
                        ),
                        expr.span,
                        Some("Pass an integer index.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::F64,
                    calls,
                };
            }

            if expr.callee == "f64_vec_set" && !functions.contains_key("f64_vec_set") {
                require_runtime_builtin_context(
                    "semantic.f64_vec-runtime-context",
                    "f64_vec_set",
                    expr.span,
                    diagnostics,
                    context,
                );
                if args.len() != 3 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_set-arity",
                        format!(
                            "builtin `f64_vec_set` expects 3 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `f64_vec_set(vec, index, value)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::F64Vec && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_set-type",
                        format!(
                            "builtin `f64_vec_set` first argument must be F64Vec, found `{}`",
                            args[0].ty.render(),
                        ),
                        expr.span,
                        Some("Pass an F64Vec value.".to_owned()),
                    ));
                }
                if args[1].ty != Type::I32 && args[1].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_set-type",
                        format!(
                            "builtin `f64_vec_set` second argument must be I32, found `{}`",
                            args[1].ty.render(),
                        ),
                        expr.span,
                        Some("Pass an integer index.".to_owned()),
                    ));
                }
                if args[2].ty != Type::F64 && args[2].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_vec_set-type",
                        format!(
                            "builtin `f64_vec_set` third argument must be F64, found `{}`",
                            args[2].ty.render(),
                        ),
                        expr.span,
                        Some("Pass a float element value.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::F64Vec,
                    calls,
                };
            }

            if expr.callee == "f64_from_i32" && !functions.contains_key("f64_from_i32") {
                if args.len() != 1 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_from_i32-arity",
                        format!(
                            "builtin `f64_from_i32` expects 1 argument but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `f64_from_i32(value)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::I32 && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.f64_from_i32-type",
                        format!(
                            "builtin `f64_from_i32` expects I32, found `{}`",
                            args[0].ty.render(),
                        ),
                        expr.span,
                        Some("Pass an integer value.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::F64,
                    calls,
                };
            }

            if expr.callee == "text_from_f64_fixed"
                && !functions.contains_key("text_from_f64_fixed")
            {
                if args.len() != 2 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_from_f64_fixed-arity",
                        format!(
                            "builtin `text_from_f64_fixed` expects 2 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `text_from_f64_fixed(value, digits)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::F64 && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_from_f64_fixed-type",
                        format!(
                            "builtin `text_from_f64_fixed` first argument must be F64, found `{}`",
                            args[0].ty.render(),
                        ),
                        expr.span,
                        Some("Pass a float value.".to_owned()),
                    ));
                }
                if args[1].ty != Type::I32 && args[1].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.text_from_f64_fixed-type",
                        format!(
                            "builtin `text_from_f64_fixed` second argument must be I32, found `{}`",
                            args[1].ty.render(),
                        ),
                        expr.span,
                        Some("Pass an integer digit count.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::Text,
                    calls,
                };
            }

            if expr.callee == "sqrt" && !functions.contains_key("sqrt") {
                if args.len() != 1 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.sqrt-arity",
                        format!("builtin `sqrt` expects 1 argument but got {}", args.len()),
                        expr.span,
                        Some("Call `sqrt(value)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::F64 && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.sqrt-type",
                        format!(
                            "builtin `sqrt` expects F64, found `{}`",
                            args[0].ty.render()
                        ),
                        expr.span,
                        Some("Pass a float value.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::F64,
                    calls,
                };
            }

            if expr.callee == "parse_i32" && !functions.contains_key("parse_i32") {
                if args.len() != 1 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.parse_i32-arity",
                        format!(
                            "builtin `parse_i32` expects 1 argument but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `parse_i32(text)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::Text && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.parse_i32-type",
                        format!(
                            "builtin `parse_i32` expects Text, found `{}`",
                            args[0].ty.render()
                        ),
                        expr.span,
                        Some("Pass a Text value.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::I32,
                    calls,
                };
            }

            if expr.callee == "arg_count" && !functions.contains_key("arg_count") {
                if !args.is_empty() {
                    diagnostics.push(Diagnostic::new(
                        "semantic.arg_count-arity",
                        format!(
                            "builtin `arg_count` expects 0 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `arg_count()` with no arguments.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                return ExprInfo {
                    ty: Type::I32,
                    calls,
                };
            }

            if expr.callee == "arg_text" && !functions.contains_key("arg_text") {
                if args.len() != 1 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.arg_text-arity",
                        format!(
                            "builtin `arg_text` expects 1 argument but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `arg_text(index)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::I32 && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.arg_text-type",
                        format!(
                            "builtin `arg_text` expects I32, found `{}`",
                            args[0].ty.render()
                        ),
                        expr.span,
                        Some("Pass an integer index.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::Text,
                    calls,
                };
            }

            if expr.callee == "stdin_text" && !functions.contains_key("stdin_text") {
                if !args.is_empty() {
                    diagnostics.push(Diagnostic::new(
                        "semantic.stdin_text-arity",
                        format!(
                            "builtin `stdin_text` expects 0 arguments but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `stdin_text()` with no arguments.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                return ExprInfo {
                    ty: Type::Text,
                    calls,
                };
            }

            if expr.callee == "stdout_write" && !functions.contains_key("stdout_write") {
                if args.len() != 1 {
                    diagnostics.push(Diagnostic::new(
                        "semantic.stdout_write-arity",
                        format!(
                            "builtin `stdout_write` expects 1 argument but got {}",
                            args.len()
                        ),
                        expr.span,
                        Some("Call `stdout_write(text)`.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if args[0].ty != Type::Text && args[0].ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.stdout_write-type",
                        format!(
                            "builtin `stdout_write` expects Text, found `{}`",
                            args[0].ty.render(),
                        ),
                        expr.span,
                        Some("Pass a Text value.".to_owned()),
                    ));
                }
                return ExprInfo {
                    ty: Type::Unit,
                    calls,
                };
            }

            if let Some((enum_name, variant)) = enum_variant_info(&expr.callee, enum_variants) {
                let expected_arity = usize::from(variant.payload.is_some());
                if args.len() != expected_arity {
                    diagnostics.push(Diagnostic::new(
                        "semantic.enum-constructor-arity",
                        format!(
                            "enum constructor `{}` expects {} argument{} but got {}",
                            expr.callee,
                            expected_arity,
                            if expected_arity == 1 { "" } else { "s" },
                            args.len(),
                        ),
                        expr.span,
                        Some("Pass exactly one payload argument for payload variants, or no arguments for plain variants.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                if let (Some(expected), Some(actual)) = (&variant.payload, args.first())
                    && actual.ty != *expected
                    && actual.ty != Type::Error
                {
                    diagnostics.push(Diagnostic::new(
                        "semantic.enum-constructor-type",
                        format!(
                            "enum constructor `{}` expects `{}`, found `{}`",
                            expr.callee,
                            expected.render(),
                            actual.ty.render(),
                        ),
                        expr.span,
                        Some("Pass a payload value of the declared variant type.".to_owned()),
                    ));
                    return ExprInfo {
                        ty: Type::Error,
                        calls,
                    };
                }
                return ExprInfo {
                    ty: Type::Named(enum_name.to_owned()),
                    calls,
                };
            }

            let Some(callee) = functions.get(&expr.callee) else {
                let suggestion = best_match(&expr.callee, functions.keys().map(String::as_str));
                diagnostics.push(Diagnostic::new(
                    "semantic.unknown-call",
                    format!("call to unknown function `{}`", expr.callee),
                    expr.span,
                    Some(suggestion_help(suggestion, || {
                        "Declare the callee before using it.".to_owned()
                    })),
                ));
                return ExprInfo {
                    ty: Type::Error,
                    calls,
                };
            };

            if callee.params.len() != args.len() {
                diagnostics.push(Diagnostic::new(
                    "semantic.call-arity",
                    format!(
                        "call to `{}` supplies {} arguments but {} were declared",
                        expr.callee,
                        args.len(),
                        callee.params.len(),
                    ),
                    expr.span,
                    Some("Match the argument list to the function signature.".to_owned()),
                ));
            }

            for ((_, expected, param_span), actual) in callee.params.iter().zip(args.iter()) {
                if actual.ty != *expected && actual.ty != Type::Error {
                    diagnostics.push(Diagnostic::new(
                        "semantic.call-type",
                        format!(
                            "argument type mismatch: expected `{}`, found `{}`",
                            expected.render(),
                            actual.ty.render(),
                        ),
                        *param_span,
                        Some("Pass an argument of the declared parameter type.".to_owned()),
                    ));
                }
            }

            if !matches!(context, ExprContext::Body | ExprContext::BodyTail)
                && !callee.effects.is_empty()
            {
                diagnostics.push(Diagnostic::new(
                    "semantic.contract-effect",
                    format!(
                        "contract expression calls `{}` which declares effects [{}]",
                        expr.callee,
                        callee
                            .effects
                            .iter()
                            .map(Effect::keyword)
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                    expr.span,
                    Some("Contracts must remain effect-free in stage-0.".to_owned()),
                ));
            }

            for effect in &callee.effects {
                if matches!(context, ExprContext::Body | ExprContext::BodyTail)
                    && !caller_effects.contains(effect)
                {
                    diagnostics.push(Diagnostic::new(
                        "semantic.missing-effect",
                        format!(
                            "function `{fn_name}` calls `{}` but does not declare the `{}` effect",
                            expr.callee,
                            effect.keyword(),
                        ),
                        expr.span,
                        Some(
                            "Add the callee's effect to the caller or remove the call.".to_owned(),
                        ),
                    ));
                }
            }

            calls.push(CallSite {
                callee: expr.callee.clone(),
            });
            ExprInfo {
                ty: callee.return_type.clone(),
                calls,
            }
        }
        Expr::Comptime(body) => {
            let info = infer_body(
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
            );
            ExprInfo {
                ty: info.ty,
                calls: info.calls,
            }
        }
        Expr::Handle(expr) => {
            let mut calls = Vec::new();
            let info = infer_body(
                &expr.body,
                locals,
                mutable_locals,
                functions,
                consts,
                enum_variants,
                struct_layouts,
                diagnostics,
                fn_name,
                caller_effects,
            );
            calls.extend(info.calls);
            ExprInfo { ty: info.ty, calls }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn infer_body(
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
    binding: &crate::hir::LetBinding,
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
    statement: &crate::hir::AssignStmt,
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
                        "Only simple local name or indexed array assignments are supported."
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
            let Type::Array(element, _) = current_ty else {
                context.diagnostics.push(Diagnostic::new(
                    "semantic.array-index-base",
                    format!(
                        "cannot index value of type `{}` in `{}`",
                        current_ty.pretty(),
                        context.fn_name,
                    ),
                    target.base.span(),
                    Some(
                        "Index into an array literal or an array-valued local binding.".to_owned(),
                    ),
                ));
                return;
            };
            (base.name.clone(), *element)
        }
        _ => {
            context.diagnostics.push(Diagnostic::new(
                "semantic.assign-complex",
                "unsupported complex assignment target in stage-0",
                statement.span,
                Some(
                    "Only simple local name or indexed array assignments are supported.".to_owned(),
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
        // Simple type check for now, can be improved
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
    stmt: &crate::hir::ExprStmt,
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

pub(crate) fn field_type(
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

fn type_contains_affine_values(
    ty: &Type,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
) -> bool {
    let mut visiting = BTreeSet::new();
    type_contains_affine_values_inner(ty, struct_layouts, enum_variants, &mut visiting)
}

const fn mutable_local_allows_affine_values(ty: &Type) -> bool {
    matches!(ty, Type::Text | Type::TextBuilder | Type::F64Vec)
}

fn type_contains_affine_values_inner(
    ty: &Type,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match ty {
        Type::Text => true,
        Type::Array(element, _) => {
            type_contains_affine_values_inner(element, struct_layouts, enum_variants, visiting)
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
        Type::TextBuilder | Type::F64Vec => true,
        Type::Param(_) => false,
        Type::I32 | Type::F64 | Type::Bool | Type::Unit | Type::Error => false,
    }
}

pub(crate) fn split_enum_variant_path(path: &str) -> Option<(&str, &str)> {
    let (enum_name, variant) = path.rsplit_once('.')?;
    (!enum_name.is_empty() && !variant.is_empty()).then_some((enum_name, variant))
}

fn enum_variant_info<'a>(
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

const ARRAY_LEN_BUILTIN: &str = "len";

fn enum_literal_type_name(
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

fn field_names_for_type(
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

fn expect_type(
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

fn check_recursion(
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

fn check_type_exists(
    diagnostics: &mut Vec<Diagnostic>,
    known_types: &BTreeSet<String>,
    path: &str,
    span: Span,
    context: &str,
) {
    if !type_exists(
        &parse_type_name(path).unwrap_or_else(|| Type::Named(path.to_owned())),
        known_types,
    ) {
        let suggestion = best_match(path, known_types.iter().map(String::as_str));
        diagnostics.push(Diagnostic::new(
            "semantic.unknown-type",
            format!("unknown {context} `{path}`"),
            span,
            Some(suggestion_help(suggestion, || {
                "Use a builtin type or a declared struct name.".to_owned()
            })),
        ));
    }
}

fn type_exists(ty: &Type, known_types: &BTreeSet<String>) -> bool {
    match ty {
        Type::Array(element, _) => type_exists(element, known_types),
        Type::Named(name) => known_types.contains(name),
        Type::Param(name) => known_types.contains(name),
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

fn type_from_ref(ty: &TypeRef) -> Type {
    parse_type_name(&ty.path).unwrap_or_else(|| Type::Named(ty.path.clone()))
}

fn parse_type_name(name: &str) -> Option<Type> {
    match name {
        "I32" => Some(Type::I32),
        "F64" => Some(Type::F64),
        "Bool" => Some(Type::Bool),
        "Text" => Some(Type::Text),
        "TextBuilder" => Some(Type::TextBuilder),
        "F64Vec" => Some(Type::F64Vec),
        "Unit" => Some(Type::Unit),
        _ => parse_array_type_name(name).or_else(|| Some(Type::Named(name.to_owned()))),
    }
}

const fn matching_numeric_type(left: &Type, right: &Type) -> Option<Type> {
    match (left, right) {
        (Type::I32, Type::I32) => Some(Type::I32),
        (Type::F64, Type::F64) => Some(Type::F64),
        _ => None,
    }
}

fn parse_array_type_name(name: &str) -> Option<Type> {
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
    let len = inner[split + 1..].trim().parse::<usize>().ok()?;
    let element = parse_type_name(element)?;
    Some(Type::Array(Box::new(element), TypeArrayLen::Literal(len)))
}

fn render_signature(function: &crate::hir::Function) -> String {
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
        "fn {}({params}){return_type}{effects}{requires}{ensures}",
        function.name
    )
}

fn best_match<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    candidates
        .map(|candidate| (strsim::levenshtein(name, candidate), candidate))
        .filter(|(distance, _)| *distance <= 3)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate.to_owned())
}

fn suggestion_help(suggestion: Option<String>, default: impl FnOnce() -> String) -> String {
    suggestion.map_or_else(default, |suggestion| {
        format!("Did you mean `{suggestion}`?")
    })
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
}
