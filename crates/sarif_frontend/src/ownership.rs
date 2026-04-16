use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::hir::{BinaryOp, Body, Expr, Module, Stmt};
use crate::semantic::{
    EnumVariantInfo, FunctionSignature, Type, field_type, split_enum_variant_path,
};
use sarif_syntax::{Diagnostic, Span};

const MATCH_PAYLOAD_SEGMENT: &str = "$payload";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParamUsage {
    pub(crate) move_whole: bool,
    pub(crate) move_fields: Vec<Vec<String>>,
}

#[derive(Clone, Debug)]
enum UsageContext {
    Borrow,
    MoveWhole,
    MovePath(Vec<String>),
    MovePaths(Vec<Vec<String>>),
}

#[allow(clippy::too_many_arguments)]
pub fn check_affine_body_ownership(
    body: &Body,
    locals: &HashMap<String, Type>,
    functions: &BTreeMap<String, FunctionSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    function_name: &str,
) -> bool {
    let mut checker = AffineUseChecker {
        locals: locals.clone(),
        functions,
        enum_variants,
        struct_fields,
        struct_layouts,
        protected_roots: BTreeSet::new(),
        mutable_locals: BTreeSet::new(),
        aliases: HashMap::new(),
        function_name,
        diagnostics,
        first_moves: Vec::new(),
        mode: AffineCheckMode::Body,
        ok: true,
    };
    collect_body_moves(&mut checker, body);
    checker.ok
}

#[allow(clippy::too_many_arguments)]
pub fn check_contract_affine_ownership(
    expr: &Expr,
    locals: &HashMap<String, Type>,
    functions: &BTreeMap<String, FunctionSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    diagnostics: &mut Vec<Diagnostic>,
    function_name: &str,
    clause_name: &str,
    result_type: Option<&Type>,
) -> bool {
    let mut checker = AffineUseChecker {
        locals: locals.clone(),
        functions,
        enum_variants,
        struct_fields,
        struct_layouts,
        protected_roots: BTreeSet::new(),
        mutable_locals: BTreeSet::new(),
        aliases: HashMap::new(),
        function_name,
        diagnostics,
        first_moves: Vec::new(),
        mode: AffineCheckMode::Contract {
            clause_name,
            result_type,
        },
        ok: true,
    };
    checker.collect(expr, false);
    checker.ok
}

pub fn infer_param_modes(
    module: &Module,
    functions: &mut BTreeMap<String, FunctionSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
) {
    let mut changed = true;
    while changed {
        changed = false;
        for item in &module.items {
            let crate::hir::Item::Function(function) = item else {
                continue;
            };
            let Some(signature) = functions.get(&function.name).cloned() else {
                continue;
            };
            let mut locals = HashMap::<String, Type>::new();
            for (name, ty, _) in &signature.params {
                locals.insert(name.clone(), ty.clone());
            }
            let inferred = infer_function_param_modes(
                function.requires.as_ref(),
                function.ensures.as_ref(),
                function.body.as_ref(),
                &signature.params,
                &locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
            );
            if inferred != signature.param_usages {
                if let Some(entry) = functions.get_mut(&function.name) {
                    entry.param_usages = inferred;
                }
                changed = true;
            }
        }
    }
}

#[must_use]
pub fn is_affine_type(
    ty: &Type,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    enum_variants: &BTreeMap<String, Vec<crate::semantic::EnumVariantInfo>>,
) -> bool {
    let mut visiting = BTreeSet::new();
    is_affine_type_inner(ty, struct_fields, enum_variants, &mut visiting)
}

#[must_use]
pub fn struct_is_affine(
    name: &str,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    enum_variants: &BTreeMap<String, Vec<crate::semantic::EnumVariantInfo>>,
) -> bool {
    let mut visiting = BTreeSet::new();
    is_affine_type_inner(
        &Type::Named(name.to_owned()),
        struct_fields,
        enum_variants,
        &mut visiting,
    )
}

struct AffineUseChecker<'a> {
    locals: HashMap<String, Type>,
    functions: &'a BTreeMap<String, FunctionSignature>,
    enum_variants: &'a BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_fields: &'a BTreeMap<String, Vec<Type>>,
    struct_layouts: &'a BTreeMap<String, Vec<(String, Type)>>,
    protected_roots: BTreeSet<String>,
    mutable_locals: BTreeSet<String>,
    aliases: HashMap<String, LocalAlias>,
    function_name: &'a str,
    diagnostics: &'a mut Vec<Diagnostic>,
    first_moves: Vec<AffineMove>,
    mode: AffineCheckMode<'a>,
    ok: bool,
}

#[derive(Clone, Debug)]
struct AffineMove {
    root: String,
    fields: Vec<String>,
    span: Span,
}

#[derive(Clone, Debug)]
struct LocalAlias {
    root: String,
    fields: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
enum AffineCheckMode<'a> {
    Body,
    Contract {
        clause_name: &'a str,
        result_type: Option<&'a Type>,
    },
}

impl AffineCheckMode<'_> {
    const fn result_type(&self) -> Option<&Type> {
        match self {
            Self::Body => None,
            Self::Contract { result_type, .. } => *result_type,
        }
    }
}

impl AffineUseChecker<'_> {
    #[allow(clippy::too_many_lines)]
    fn collect(&mut self, expr: &Expr, borrow_only: bool) {
        match expr {
            Expr::Integer(_) | Expr::Float(_) | Expr::String(_) | Expr::Bool(_) => {}
            Expr::Name(expr) => {
                let Some(ty) = self.locals.get(&expr.name) else {
                    return;
                };
                if self.mutable_locals.contains(&expr.name) {
                    return;
                }
                if borrow_only || !is_affine_type(ty, self.struct_fields, self.enum_variants) {
                    return;
                }
                let (root, fields) = self
                    .aliased_access_path(&Expr::Name(expr.clone()))
                    .unwrap_or_else(|| (expr.name.clone(), Vec::new()));
                match self.mode {
                    AffineCheckMode::Body => {
                        self.record_body_move(expr.span, &root, &fields);
                    }
                    AffineCheckMode::Contract { clause_name, .. } => {
                        self.record_contract_move(
                            expr.span,
                            clause_name,
                            &render_affine_subject(&root, &fields),
                        );
                    }
                }
            }
            Expr::Field(expr) => {
                let field_type = expr_type_for_ownership(
                    &Expr::Field(expr.clone()),
                    &self.locals,
                    self.struct_layouts,
                    self.mode.result_type(),
                );
                let field_borrow_only = borrow_only
                    || field_type.as_ref().is_some_and(|ty| {
                        !is_affine_type(ty, self.struct_fields, self.enum_variants)
                    });
                if field_borrow_only {
                    self.collect(&expr.base, true);
                    return;
                }
                if let Some((root, fields)) = self.aliased_access_path(&Expr::Field(expr.clone())) {
                    match self.mode {
                        AffineCheckMode::Body => self.record_body_move(expr.span, &root, &fields),
                        AffineCheckMode::Contract { clause_name, .. } => self.record_contract_move(
                            expr.span,
                            clause_name,
                            &render_affine_subject(&root, &fields),
                        ),
                    }
                } else {
                    self.collect(&expr.base, false);
                }
            }
            Expr::Record(expr) => {
                for field in &expr.fields {
                    self.collect(&field.value, borrow_only);
                }
            }
            Expr::Array(expr) => {
                for element in &expr.elements {
                    self.collect(element, true);
                }
            }
            Expr::Index(expr) => {
                self.collect(&expr.base, true);
                self.collect(&expr.index, true);
            }
            Expr::ContractResult(expr) => {
                if borrow_only {
                    return;
                }
                let AffineCheckMode::Contract {
                    clause_name,
                    result_type: Some(result_type),
                } = self.mode
                else {
                    return;
                };
                if is_affine_type(result_type, self.struct_fields, self.enum_variants) {
                    self.record_contract_move(expr.span, clause_name, "result");
                }
            }
            Expr::If(expr) => {
                self.collect(&expr.condition, true);
                let base_moves = self.first_moves.clone();
                let mut then_diagnostics = Vec::new();
                let mut then_checker = AffineUseChecker {
                    locals: self.locals.clone(),
                    functions: self.functions,
                    enum_variants: self.enum_variants,
                    struct_fields: self.struct_fields,
                    struct_layouts: self.struct_layouts,
                    protected_roots: BTreeSet::new(),
                    mutable_locals: self.mutable_locals.clone(),
                    aliases: self.aliases.clone(),
                    function_name: self.function_name,
                    diagnostics: &mut then_diagnostics,
                    first_moves: base_moves.clone(),
                    mode: self.mode,
                    ok: true,
                };
                collect_body_moves(&mut then_checker, &expr.then_body);
                let then_moves = then_checker.first_moves;
                let then_falls_through = body_falls_through(&expr.then_body);
                self.ok &= then_checker.ok;
                self.diagnostics.extend(then_diagnostics);

                let mut else_diagnostics = Vec::new();
                let mut else_checker = AffineUseChecker {
                    locals: self.locals.clone(),
                    functions: self.functions,
                    enum_variants: self.enum_variants,
                    struct_fields: self.struct_fields,
                    struct_layouts: self.struct_layouts,
                    protected_roots: BTreeSet::new(),
                    mutable_locals: self.mutable_locals.clone(),
                    aliases: self.aliases.clone(),
                    function_name: self.function_name,
                    diagnostics: &mut else_diagnostics,
                    first_moves: base_moves.clone(),
                    mode: self.mode,
                    ok: true,
                };
                collect_body_moves(&mut else_checker, &expr.else_body);
                let else_moves = else_checker.first_moves;
                let else_falls_through = body_falls_through(&expr.else_body);
                self.ok &= else_checker.ok;
                self.diagnostics.extend(else_diagnostics);

                if then_falls_through {
                    for moved in then_moves.iter().skip(base_moves.len()) {
                        self.union_move(moved);
                    }
                }
                if else_falls_through {
                    for moved in else_moves.iter().skip(base_moves.len()) {
                        self.union_move(moved);
                    }
                }
            }
            Expr::Match(expr) => {
                self.collect(&expr.scrutinee, true);
                let base_moves = self.first_moves.clone();
                for arm in &expr.arms {
                    let mut arm_diagnostics = Vec::new();
                    let mut arm_checker = AffineUseChecker {
                        locals: self.locals.clone(),
                        functions: self.functions,
                        enum_variants: self.enum_variants,
                        struct_fields: self.struct_fields,
                        struct_layouts: self.struct_layouts,
                        protected_roots: BTreeSet::new(),
                        mutable_locals: self.mutable_locals.clone(),
                        aliases: self.aliases.clone(),
                        function_name: self.function_name,
                        diagnostics: &mut arm_diagnostics,
                        first_moves: base_moves.clone(),
                        mode: self.mode,
                        ok: true,
                    };
                    if let Some((binding, payload_ty)) = match_arm_payload_binding(
                        &expr.scrutinee,
                        &arm.pattern,
                        &arm_checker.locals,
                        self.enum_variants,
                        self.struct_layouts,
                    ) {
                        arm_checker.locals.insert(binding.clone(), payload_ty);
                        if let Some(alias) =
                            payload_alias_for_scrutinee(&expr.scrutinee, &arm_checker.aliases)
                        {
                            arm_checker.aliases.insert(binding, alias);
                        }
                    }
                    collect_body_moves(&mut arm_checker, &arm.body);
                    let arm_moves = arm_checker.first_moves;
                    let arm_falls_through = body_falls_through(&arm.body);
                    self.ok &= arm_checker.ok;
                    self.diagnostics.extend(arm_diagnostics);
                    if arm_falls_through {
                        for moved in arm_moves.iter().skip(base_moves.len()) {
                            self.union_move(moved);
                        }
                    }
                }
            }
            Expr::Repeat(expr) => {
                self.collect(&expr.count, true);
                let mut body_locals = self.locals.clone();
                if let Some(binding) = &expr.binding {
                    body_locals.insert(binding.clone(), Type::I32);
                }
                let mut body_checker = AffineUseChecker {
                    locals: body_locals,
                    functions: self.functions,
                    enum_variants: self.enum_variants,
                    struct_fields: self.struct_fields,
                    struct_layouts: self.struct_layouts,
                    protected_roots: self
                        .locals
                        .keys()
                        .filter(|name| !self.mutable_locals.contains(*name))
                        .cloned()
                        .collect(),
                    mutable_locals: self.mutable_locals.clone(),
                    aliases: self.aliases.clone(),
                    function_name: self.function_name,
                    diagnostics: self.diagnostics,
                    first_moves: self.first_moves.clone(),
                    mode: self.mode,
                    ok: true,
                };
                collect_body_moves(&mut body_checker, &expr.body);
                self.ok &= body_checker.ok;
            }
            Expr::While(expr) => {
                self.collect(&expr.condition, true);
                let mut body_checker = AffineUseChecker {
                    locals: self.locals.clone(),
                    functions: self.functions,
                    enum_variants: self.enum_variants,
                    struct_fields: self.struct_fields,
                    struct_layouts: self.struct_layouts,
                    protected_roots: self
                        .locals
                        .keys()
                        .filter(|name| !self.mutable_locals.contains(*name))
                        .cloned()
                        .collect(),
                    mutable_locals: self.mutable_locals.clone(),
                    aliases: self.aliases.clone(),
                    function_name: self.function_name,
                    diagnostics: self.diagnostics,
                    first_moves: self.first_moves.clone(),
                    mode: self.mode,
                    ok: true,
                };
                collect_body_moves(&mut body_checker, &expr.body);
                self.ok &= body_checker.ok;
            }
            Expr::Group(expr) => self.collect(&expr.inner, borrow_only),
            Expr::Unary(expr) => self.collect(&expr.inner, borrow_only),
            Expr::Comptime(body) => {
                let mut body_checker = AffineUseChecker {
                    locals: self.locals.clone(),
                    functions: self.functions,
                    enum_variants: self.enum_variants,
                    struct_fields: self.struct_fields,
                    struct_layouts: self.struct_layouts,
                    protected_roots: BTreeSet::new(),
                    mutable_locals: self.mutable_locals.clone(),
                    aliases: self.aliases.clone(),
                    function_name: self.function_name,
                    diagnostics: self.diagnostics,
                    first_moves: self.first_moves.clone(),
                    mode: self.mode,
                    ok: true,
                };
                collect_body_moves(&mut body_checker, body);
                self.ok &= body_checker.ok;
            }
            Expr::Handle(_) => {}
            Expr::Call(expr) => {
                if let Some(callee) = self.functions.get(&expr.callee) {
                    for (index, arg) in expr.args.iter().enumerate() {
                        if let Some(usage) = callee.param_usages.get(index) {
                            self.collect_with_usage(arg, usage);
                        } else {
                            self.collect(arg, false);
                        }
                    }
                } else if expr.callee == "len"
                    || expr.callee == "text_len"
                    || expr.callee == "bytes_len"
                    || expr.callee == "text_slice"
                    || expr.callee == "bytes_slice"
                    || expr.callee == "text_byte"
                    || expr.callee == "bytes_byte"
                    || expr.callee == "text_cmp"
                    || expr.callee == "text_eq_range"
                    || expr.callee == "text_find_byte_range"
                    || expr.callee == "bytes_find_byte_range"
                    || expr.callee == "text_line_end"
                    || expr.callee == "text_next_line"
                    || expr.callee == "text_field_end"
                    || expr.callee == "text_next_field"
                    || expr.callee == "text_builder_append_i32"
                    || expr.callee == "parse_i32_range"
                    || expr.callee == "text_builder_append_codepoint"
                    || expr.callee == "text_builder_append_ascii"
                    || expr.callee == "text_builder_append_slice"
                    || expr.callee == "text_index_get"
                    || expr.callee == "text_index_set"
                    || expr.callee == "list_len"
                    || expr.callee == "list_get"
                {
                    for arg in &expr.args {
                        self.collect(arg, true);
                    }
                } else {
                    for arg in &expr.args {
                        self.collect(arg, false);
                    }
                }
            }
            Expr::Binary(expr) => {
                let operand_borrow_only =
                    borrow_only || matches!(expr.op, BinaryOp::Eq | BinaryOp::Ne);
                self.collect(&expr.left, operand_borrow_only);
                self.collect(&expr.right, operand_borrow_only);
            }
            Expr::Perform(expr) => {
                for arg in &expr.args {
                    self.collect(arg, borrow_only);
                }
            }
        }
    }

    fn collect_with_usage(&mut self, expr: &Expr, usage: &ParamUsage) {
        if usage.is_borrow_only() {
            self.collect(expr, true);
            return;
        }
        if usage.move_whole {
            self.collect(expr, false);
            return;
        }
        if let Expr::Record(record) = expr {
            for field in &record.fields {
                let field_usage = usage.project_record_field(&field.name);
                self.collect_with_usage(&field.value, &field_usage);
            }
            return;
        }
        if let Some((root, base_fields)) = self.aliased_access_path(expr) {
            match self.mode {
                AffineCheckMode::Body => {
                    for path in &usage.move_fields {
                        let mut full = base_fields.clone();
                        full.extend(path.clone());
                        self.record_body_move(expr.span(), &root, &full);
                    }
                }
                AffineCheckMode::Contract { clause_name, .. } => {
                    for path in &usage.move_fields {
                        let mut full = base_fields.clone();
                        full.extend(path.clone());
                        self.record_contract_move(
                            expr.span(),
                            clause_name,
                            &render_affine_subject(&root, &full),
                        );
                    }
                }
            }
        } else {
            self.collect(expr, false);
        }
    }

    fn record_body_move(&mut self, span: Span, root: &str, fields: &[String]) {
        if self.protected_roots.contains(root) {
            let subject = render_affine_subject(root, fields);
            self.diagnostics.push(Diagnostic::new(
                "semantic.repeat-affine-capture",
                format!(
                    "repeat body in `{}` consumes outer affine value `{subject}`",
                    self.function_name
                ),
                span,
                Some(
                    "Stage-0 repeat bodies may run multiple times: move only affine values created inside the loop, or keep outer affine values borrow-only."
                        .to_owned(),
                ),
            ));
            self.ok = false;
            return;
        }
        if let Some(previous) = self
            .first_moves
            .iter()
            .find(|previous| previous.overlaps(root, fields))
        {
            let subject = render_affine_subject(root, fields);
            self.diagnostics.push(Diagnostic::new(
                "semantic.affine-reuse",
                format!(
                    "affine value `{subject}` is consumed more than once in `{}`",
                    self.function_name
                ),
                span,
                Some(format!(
                    "Use `{subject}` once in executable code or introduce an explicit borrow form later; the first move is at [{}..{}].",
                    previous.span.start, previous.span.end,
                )),
            ));
            self.ok = false;
            return;
        }
        self.first_moves.push(AffineMove {
            root: root.to_owned(),
            fields: fields.to_vec(),
            span,
        });
    }

    fn record_contract_move(&mut self, span: Span, clause_name: &str, subject: &str) {
        self.diagnostics.push(Diagnostic::new(
            "semantic.contract-affine-move",
            format!(
                "`{clause_name}` in `{}` consumes affine value `{subject}`",
                self.function_name
            ),
            span,
            Some(
                "Contracts are read-only in stage-0: compare affine values directly or call a borrow-safe predicate."
                    .to_owned(),
            ),
        ));
        self.ok = false;
    }

    fn union_move(&mut self, new_move: &AffineMove) {
        if let Some(existing) = self.first_moves.iter_mut().find(|existing| {
            existing.root == new_move.root
                && (existing.fields.is_empty()
                    || new_move.fields.is_empty()
                    || existing.fields.starts_with(&new_move.fields)
                    || new_move.fields.starts_with(&existing.fields))
        }) {
            if new_move.fields.is_empty() || existing.fields.starts_with(&new_move.fields) {
                existing.fields.clone_from(&new_move.fields);
            }
            return;
        }
        self.first_moves.push(new_move.clone());
    }

    fn aliased_access_path(&self, expr: &Expr) -> Option<(String, Vec<String>)> {
        access_path_for_ownership_with_aliases(expr, &self.aliases)
    }
}

impl AffineMove {
    fn overlaps(&self, root: &str, fields: &[String]) -> bool {
        self.root == root
            && (self.fields.is_empty()
                || fields.is_empty()
                || self.fields.as_slice().starts_with(fields)
                || fields.starts_with(self.fields.as_slice()))
    }
}

impl ParamUsage {
    #[must_use]
    pub const fn move_whole() -> Self {
        Self {
            move_whole: true,
            move_fields: Vec::new(),
        }
    }

    pub(crate) const fn borrow_only() -> Self {
        Self {
            move_whole: false,
            move_fields: Vec::new(),
        }
    }

    pub(crate) const fn is_borrow_only(&self) -> bool {
        !self.move_whole && self.move_fields.is_empty()
    }

    fn project_record_field(&self, field_name: &str) -> Self {
        if self.move_whole {
            return Self::move_whole();
        }
        let mut projected = Self::borrow_only();
        for path in &self.move_fields {
            let Some((head, tail)) = path.split_first() else {
                return Self::move_whole();
            };
            if head != field_name {
                continue;
            }
            if tail.is_empty() {
                return Self::move_whole();
            }
            projected.record_path(tail.to_vec());
        }
        projected
    }

    fn record_path(&mut self, path: Vec<String>) {
        if self.move_whole {
            return;
        }
        if path.is_empty() {
            *self = Self::move_whole();
            return;
        }
        if self
            .move_fields
            .iter()
            .any(|existing| path_starts_with(existing, &path) || path_starts_with(&path, existing))
        {
            self.move_fields
                .retain(|existing| !path_starts_with(existing, &path) || existing == &path);
            if !self.move_fields.iter().any(|existing| existing == &path) {
                self.move_fields.push(path);
            }
            return;
        }
        self.move_fields.push(path);
    }
}

impl UsageContext {
    fn project(self, field: &str) -> Self {
        match self {
            Self::Borrow => Self::Borrow,
            Self::MoveWhole => Self::MovePath(vec![field.to_owned()]),
            Self::MovePath(path) => {
                let mut next = vec![field.to_owned()];
                next.extend(path);
                Self::MovePath(next)
            }
            Self::MovePaths(paths) => Self::MovePaths(
                paths
                    .into_iter()
                    .map(|path| {
                        let mut next = vec![field.to_owned()];
                        next.extend(path);
                        next
                    })
                    .collect(),
            ),
        }
    }

    fn project_record_field(&self, field_name: &str) -> Self {
        match self {
            Self::Borrow => Self::Borrow,
            Self::MoveWhole => Self::MoveWhole,
            Self::MovePath(path) => project_path_field(path, field_name),
            Self::MovePaths(paths) => project_paths_field(paths, field_name),
        }
    }
}

fn path_starts_with(path: &[String], prefix: &[String]) -> bool {
    path.starts_with(prefix)
}

#[allow(clippy::too_many_arguments)]
fn infer_function_param_modes(
    requires: Option<&Expr>,
    ensures: Option<&Expr>,
    body: Option<&Body>,
    params: &[(String, Type, Span)],
    locals: &HashMap<String, Type>,
    functions: &BTreeMap<String, FunctionSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
) -> Vec<ParamUsage> {
    let mut usages = HashMap::<String, ParamUsage>::new();
    let aliases = HashMap::<String, LocalAlias>::new();
    if let Some(requires) = requires {
        collect_param_modes(
            requires,
            UsageContext::Borrow,
            locals,
            functions,
            enum_variants,
            struct_fields,
            struct_layouts,
            &aliases,
            &mut usages,
        );
    }
    if let Some(ensures) = ensures {
        collect_param_modes(
            ensures,
            UsageContext::Borrow,
            locals,
            functions,
            enum_variants,
            struct_fields,
            struct_layouts,
            &aliases,
            &mut usages,
        );
    }
    if let Some(body) = body {
        collect_body_param_modes(
            body,
            locals,
            functions,
            enum_variants,
            struct_fields,
            struct_layouts,
            &aliases,
            &mut usages,
        );
    }
    params
        .iter()
        .map(|(name, ty, _)| {
            if is_affine_type(ty, struct_fields, enum_variants) {
                usages
                    .get(name)
                    .cloned()
                    .unwrap_or_else(ParamUsage::borrow_only)
            } else {
                ParamUsage::move_whole()
            }
        })
        .collect::<Vec<_>>()
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn collect_param_modes(
    expr: &Expr,
    default_mode: UsageContext,
    locals: &HashMap<String, Type>,
    functions: &BTreeMap<String, FunctionSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    aliases: &HashMap<String, LocalAlias>,
    usages: &mut HashMap<String, ParamUsage>,
) {
    match expr {
        Expr::Integer(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::ContractResult(_) => {}
        Expr::Name(expr) => {
            let Some(ty) = locals.get(&expr.name) else {
                return;
            };
            if !is_affine_type(ty, struct_fields, enum_variants) {
                return;
            }
            if let Some(alias) = aliases.get(&expr.name) {
                record_usage_for_target(usages, &alias.root, &alias.fields, &default_mode);
            } else {
                record_usage_for_target(usages, &expr.name, &[], &default_mode);
            }
        }
        Expr::Group(expr) => {
            collect_param_modes(
                &expr.inner,
                default_mode,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
        }
        Expr::Unary(expr) => {
            collect_param_modes(
                &expr.inner,
                default_mode,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
        }
        Expr::Comptime(body) => {
            collect_body_param_modes(
                body,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
        }
        Expr::Handle(_) => {}
        Expr::If(expr) => {
            collect_param_modes(
                &expr.condition,
                UsageContext::Borrow,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
            collect_body_param_modes(
                &expr.then_body,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
            collect_body_param_modes(
                &expr.else_body,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
        }
        Expr::Match(expr) => {
            collect_param_modes(
                &expr.scrutinee,
                UsageContext::Borrow,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
            for arm in &expr.arms {
                let mut arm_locals = locals.clone();
                let mut arm_aliases = aliases.clone();
                if let Some((binding, payload_ty)) = match_arm_payload_binding(
                    &expr.scrutinee,
                    &arm.pattern,
                    locals,
                    enum_variants,
                    struct_layouts,
                ) {
                    arm_locals.insert(binding.clone(), payload_ty);
                    if let Some(alias) = payload_alias_for_scrutinee(&expr.scrutinee, aliases) {
                        arm_aliases.insert(binding, alias);
                    }
                }
                collect_body_param_modes(
                    &arm.body,
                    &arm_locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    &arm_aliases,
                    usages,
                );
            }
        }
        Expr::Repeat(expr) => {
            let mut body_locals = locals.clone();
            if let Some(binding) = &expr.binding {
                body_locals.insert(binding.clone(), Type::I32);
            }
            collect_param_modes(
                &expr.count,
                UsageContext::Borrow,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
            collect_body_param_modes_borrow_only(
                &expr.body,
                &body_locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
        }
        Expr::While(expr) => {
            collect_param_modes(
                &expr.condition,
                UsageContext::Borrow,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
            collect_body_param_modes(
                &expr.body,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
        }
        Expr::Binary(expr) => {
            let operand_mode = if matches!(expr.op, BinaryOp::Eq | BinaryOp::Ne) {
                UsageContext::Borrow
            } else {
                default_mode
            };
            collect_param_modes(
                &expr.left,
                operand_mode.clone(),
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
            collect_param_modes(
                &expr.right,
                operand_mode,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
        }
        Expr::Call(expr) => {
            if let Some(callee) = functions.get(&expr.callee) {
                for (index, arg) in expr.args.iter().enumerate() {
                    let usage = callee
                        .param_usages
                        .get(index)
                        .cloned()
                        .unwrap_or_else(ParamUsage::move_whole);
                    collect_argument_modes(
                        arg,
                        &usage,
                        locals,
                        functions,
                        enum_variants,
                        struct_fields,
                        struct_layouts,
                        aliases,
                        usages,
                    );
                }
            } else if expr.callee == "len"
                || expr.callee == "text_len"
                || expr.callee == "bytes_len"
                || expr.callee == "text_slice"
                || expr.callee == "bytes_slice"
                || expr.callee == "text_byte"
                || expr.callee == "bytes_byte"
                || expr.callee == "text_cmp"
                || expr.callee == "text_eq_range"
                || expr.callee == "text_find_byte_range"
                || expr.callee == "bytes_find_byte_range"
                || expr.callee == "text_line_end"
                || expr.callee == "text_next_line"
                || expr.callee == "text_field_end"
                || expr.callee == "text_next_field"
                || expr.callee == "text_builder_append_i32"
                || expr.callee == "parse_i32_range"
                || expr.callee == "text_builder_append_codepoint"
                || expr.callee == "text_builder_append_ascii"
                || expr.callee == "text_builder_append_slice"
                || expr.callee == "text_index_get"
                || expr.callee == "text_index_set"
                || expr.callee == "list_len"
                || expr.callee == "list_get"
            {
                for arg in &expr.args {
                    collect_param_modes(
                        arg,
                        UsageContext::Borrow,
                        locals,
                        functions,
                        enum_variants,
                        struct_fields,
                        struct_layouts,
                        aliases,
                        usages,
                    );
                }
            } else {
                for arg in &expr.args {
                    collect_param_modes(
                        arg,
                        UsageContext::MoveWhole,
                        locals,
                        functions,
                        enum_variants,
                        struct_fields,
                        struct_layouts,
                        aliases,
                        usages,
                    );
                }
            }
        }
        Expr::Field(expr) => {
            let field_type =
                expr_type_for_ownership(&Expr::Field(expr.clone()), locals, struct_layouts, None);
            let mode = if matches!(default_mode, UsageContext::Borrow)
                || field_type
                    .as_ref()
                    .is_some_and(|ty| !is_affine_type(ty, struct_fields, enum_variants))
            {
                UsageContext::Borrow
            } else {
                default_mode.project(&expr.field)
            };
            collect_param_modes(
                &expr.base,
                mode,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
        }
        Expr::Record(expr) => {
            for field in &expr.fields {
                collect_param_modes(
                    &field.value,
                    default_mode.project_record_field(&field.name),
                    locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    aliases,
                    usages,
                );
            }
        }
        Expr::Array(expr) => {
            for element in &expr.elements {
                collect_param_modes(
                    element,
                    UsageContext::Borrow,
                    locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    aliases,
                    usages,
                );
            }
        }
        Expr::Index(expr) => {
            collect_param_modes(
                &expr.base,
                UsageContext::Borrow,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
            collect_param_modes(
                &expr.index,
                UsageContext::Borrow,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
        }
        Expr::Perform(expr) => {
            for arg in &expr.args {
                collect_param_modes(
                    arg,
                    default_mode.clone(),
                    locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    aliases,
                    usages,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_argument_modes(
    expr: &Expr,
    usage: &ParamUsage,
    locals: &HashMap<String, Type>,
    functions: &BTreeMap<String, FunctionSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    aliases: &HashMap<String, LocalAlias>,
    usages: &mut HashMap<String, ParamUsage>,
) {
    if usage.is_borrow_only() {
        collect_param_modes(
            expr,
            UsageContext::Borrow,
            locals,
            functions,
            enum_variants,
            struct_fields,
            struct_layouts,
            aliases,
            usages,
        );
        return;
    }
    if usage.move_whole {
        collect_param_modes(
            expr,
            UsageContext::MoveWhole,
            locals,
            functions,
            enum_variants,
            struct_fields,
            struct_layouts,
            aliases,
            usages,
        );
        return;
    }
    if let Expr::Record(record) = expr {
        for field in &record.fields {
            let field_usage = usage.project_record_field(&field.name);
            collect_argument_modes(
                &field.value,
                &field_usage,
                locals,
                functions,
                enum_variants,
                struct_fields,
                struct_layouts,
                aliases,
                usages,
            );
        }
        return;
    }
    collect_param_modes(
        expr,
        usage_context_for_argument(expr, usage, aliases),
        locals,
        functions,
        enum_variants,
        struct_fields,
        struct_layouts,
        aliases,
        usages,
    );
}

fn expr_type_for_ownership(
    expr: &Expr,
    locals: &HashMap<String, Type>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    result_type: Option<&Type>,
) -> Option<Type> {
    match expr {
        Expr::Integer(_) => Some(Type::I32),
        Expr::Float(_) => Some(Type::F64),
        Expr::String(_) => Some(Type::Text),
        Expr::Bool(_) => Some(Type::Bool),
        Expr::Name(expr) => locals.get(&expr.name).cloned(),
        Expr::ContractResult(_) => result_type.cloned(),
        Expr::Group(expr) => {
            expr_type_for_ownership(&expr.inner, locals, struct_layouts, result_type)
        }
        Expr::Unary(expr) => {
            expr_type_for_ownership(&expr.inner, locals, struct_layouts, result_type)
        }
        Expr::Comptime(body) => body_type_for_ownership(body, locals, struct_layouts, result_type),
        Expr::Handle(_) => None,
        Expr::If(expr) => {
            let then_ty =
                body_type_for_ownership(&expr.then_body, locals, struct_layouts, result_type)?;
            let else_ty =
                body_type_for_ownership(&expr.else_body, locals, struct_layouts, result_type)?;
            (then_ty == else_ty).then_some(then_ty)
        }
        Expr::Match(expr) => {
            let mut arm_types = expr
                .arms
                .iter()
                .filter_map(|arm| {
                    body_type_for_ownership(&arm.body, locals, struct_layouts, result_type)
                })
                .collect::<Vec<_>>();
            let first = arm_types.pop()?;
            arm_types.into_iter().all(|ty| ty == first).then_some(first)
        }
        Expr::Repeat(_) | Expr::While(_) => Some(Type::Unit),
        Expr::Array(expr) => {
            let first = expr.elements.first().and_then(|element| {
                expr_type_for_ownership(element, locals, struct_layouts, result_type)
            })?;
            for element in &expr.elements[1..] {
                let element_ty =
                    expr_type_for_ownership(element, locals, struct_layouts, result_type)?;
                if element_ty != first {
                    return None;
                }
            }
            Some(Type::Array(
                Box::new(first),
                expr.repeat_len.clone().unwrap_or_else(|| {
                    crate::hir::ConstExpr::Literal(u32::try_from(expr.elements.len()).unwrap_or(0))
                }),
            ))
        }
        Expr::Index(expr) => {
            let base = expr_type_for_ownership(&expr.base, locals, struct_layouts, result_type)?;
            match base {
                Type::Array(element, _) | Type::List(element) => Some(*element),
                Type::Text | Type::Bytes => Some(Type::I32),
                _ => None,
            }
        }
        Expr::Field(expr) => {
            if let Expr::Name(base_name) = &*expr.base
                && !locals.contains_key(&base_name.name)
                && !struct_layouts.contains_key(&base_name.name)
            {
                return Some(Type::Named(base_name.name.clone()));
            }
            let base = expr_type_for_ownership(&expr.base, locals, struct_layouts, result_type)?;
            if expr.field == "len"
                && matches!(
                    base,
                    Type::Array(_, _) | Type::List(_) | Type::Text | Type::Bytes
                )
            {
                Some(Type::I32)
            } else {
                field_type(&base, &expr.field, struct_layouts)
            }
        }
        Expr::Record(expr) => Some(Type::Named(expr.name.clone())),
        Expr::Call(expr) => match expr.callee.as_str() {
            "text_builder_new"
            | "text_builder_append"
            | "text_builder_append_codepoint"
            | "text_builder_append_ascii"
            | "text_builder_append_slice"
            | "text_builder_append_i32"
            | "stdout_write_builder" => Some(Type::TextBuilder),
            "text_index_new" => Some(Type::TextIndex),
            "text_builder_finish" => Some(Type::Text),
            "stdin_bytes" => Some(Type::Bytes),
            "bytes_slice" => Some(Type::Bytes),
            "list_new" => expr
                .args
                .get(1)
                .and_then(|value| {
                    expr_type_for_ownership(value, locals, struct_layouts, result_type)
                })
                .map(|element| Type::List(Box::new(element))),
            "list_len" => Some(Type::I32),
            "list_get" => expr.args.first().and_then(|list| {
                let list_ty = expr_type_for_ownership(list, locals, struct_layouts, result_type)?;
                match list_ty {
                    Type::List(element) => Some(*element),
                    _ => None,
                }
            }),
            "list_set" => expr.args.first().and_then(|list| {
                expr_type_for_ownership(list, locals, struct_layouts, result_type)
            }),
            "list_push" => expr.args.first().and_then(|list| {
                expr_type_for_ownership(list, locals, struct_layouts, result_type)
            }),
            "list_sort_text" => expr.args.first().and_then(|list| {
                expr_type_for_ownership(list, locals, struct_layouts, result_type)
            }),
            "list_sort_by_text_field" => expr.args.first().and_then(|list| {
                expr_type_for_ownership(list, locals, struct_layouts, result_type)
            }),
            "text_index_get" => Some(Type::I32),
            "text_index_set" => Some(Type::TextIndex),
            "bytes_len" => Some(Type::I32),
            "bytes_byte" => Some(Type::I32),
            "bytes_find_byte_range" => Some(Type::I32),
            "text_eq_range" => Some(Type::Bool),
            "text_line_end" => Some(Type::I32),
            "text_next_line" => Some(Type::I32),
            "text_field_end" => Some(Type::I32),
            "text_next_field" => Some(Type::I32),
            "parse_i32_range" => Some(Type::I32),
            _ => None,
        },
        Expr::Binary(_) => None,
        Expr::Perform(_) => Some(Type::Unit), // Assume Unit for now
    }
}

fn access_path_for_ownership_with_aliases(
    expr: &Expr,
    aliases: &HashMap<String, LocalAlias>,
) -> Option<(String, Vec<String>)> {
    match expr {
        Expr::Name(expr) => aliases.get(&expr.name).map_or_else(
            || Some((expr.name.clone(), Vec::new())),
            |alias| Some((alias.root.clone(), alias.fields.clone())),
        ),
        Expr::ContractResult(_) => Some(("result".to_owned(), Vec::new())),
        Expr::Group(expr) => access_path_for_ownership_with_aliases(&expr.inner, aliases),
        Expr::Unary(expr) => access_path_for_ownership_with_aliases(&expr.inner, aliases),
        Expr::Field(expr) => {
            let (root, mut fields) = access_path_for_ownership_with_aliases(&expr.base, aliases)?;
            fields.push(expr.field.clone());
            Some((root, fields))
        }
        Expr::Integer(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::If(_)
        | Expr::Match(_)
        | Expr::Repeat(_)
        | Expr::While(_)
        | Expr::Array(_)
        | Expr::Index(_)
        | Expr::Record(_)
        | Expr::Call(_)
        | Expr::Binary(_)
        | Expr::Comptime(_)
        | Expr::Handle(_)
        | Expr::Perform(_) => None,
    }
}

fn render_affine_subject(root: &str, fields: &[String]) -> String {
    if fields.is_empty() {
        return root.to_owned();
    }
    format!("{root}.{}", fields.join("."))
}

fn usage_context_for_argument(
    expr: &Expr,
    usage: &ParamUsage,
    aliases: &HashMap<String, LocalAlias>,
) -> UsageContext {
    if usage.is_borrow_only() {
        return UsageContext::Borrow;
    }
    if usage.move_whole {
        return UsageContext::MoveWhole;
    }
    if access_path_for_ownership_with_aliases(expr, aliases).is_some() {
        return match usage.move_fields.as_slice() {
            [] => UsageContext::Borrow,
            [path] => UsageContext::MovePath(path.clone()),
            paths => UsageContext::MovePaths(paths.to_vec()),
        };
    }
    UsageContext::MoveWhole
}

fn is_affine_type_inner(
    ty: &Type,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    enum_variants: &BTreeMap<String, Vec<crate::semantic::EnumVariantInfo>>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match ty {
        Type::Text | Type::Bytes | Type::TextIndex | Type::TextBuilder => true,
        Type::List(_) => true,
        Type::Pair(left, right) => {
            is_affine_type_inner(left, struct_fields, enum_variants, visiting)
                || is_affine_type_inner(right, struct_fields, enum_variants, visiting)
        }
        Type::Named(name) => {
            if !visiting.insert(name.clone()) {
                return false;
            }
            if let Some(fields) = struct_fields.get(name) {
                let result = fields.iter().any(|field| {
                    is_affine_type_inner(field, struct_fields, enum_variants, visiting)
                });
                visiting.remove(name);
                return result;
            }
            if let Some(variants) = enum_variants.get(name) {
                let result = variants.iter().any(|v| {
                    v.payload.as_ref().is_some_and(|payload_ty| {
                        is_affine_type_inner(payload_ty, struct_fields, enum_variants, visiting)
                    })
                });
                visiting.remove(name);
                return result;
            }
            visiting.remove(name);
            true
        }
        Type::Array(element, _) => {
            is_affine_type_inner(element, struct_fields, enum_variants, visiting)
        }
        Type::I32 | Type::F64 | Type::Bool | Type::Unit | Type::Param(_) | Type::Error => false,
    }
}

fn project_path_field(path: &[String], field_name: &str) -> UsageContext {
    let Some((head, tail)) = path.split_first() else {
        return UsageContext::MoveWhole;
    };
    if head != field_name {
        return UsageContext::Borrow;
    }
    if tail.is_empty() {
        UsageContext::MoveWhole
    } else {
        UsageContext::MovePath(tail.to_vec())
    }
}

fn project_paths_field(paths: &[Vec<String>], field_name: &str) -> UsageContext {
    let mut projected = Vec::new();
    for path in paths {
        let Some((head, tail)) = path.split_first() else {
            return UsageContext::MoveWhole;
        };
        if head != field_name {
            continue;
        }
        if tail.is_empty() {
            return UsageContext::MoveWhole;
        }
        projected.push(tail.to_vec());
    }
    match projected.len() {
        0 => UsageContext::Borrow,
        1 => UsageContext::MovePath(projected.remove(0)),
        _ => UsageContext::MovePaths(projected),
    }
}

fn collect_body_moves(checker: &mut AffineUseChecker<'_>, body: &Body) {
    for statement in &body.statements {
        match statement {
            Stmt::Let(binding) => {
                checker.collect(&binding.value, false);
                if binding.mutable {
                    checker.mutable_locals.insert(binding.name.clone());
                }
                if !checker.locals.contains_key(&binding.name)
                    && let Some(ty) = expr_type_for_ownership(
                        &binding.value,
                        &checker.locals,
                        checker.struct_layouts,
                        checker.mode.result_type(),
                    )
                {
                    checker.locals.insert(binding.name.clone(), ty);
                }
            }
            Stmt::Assign(stmt) => {
                collect_assign_target_expr(checker, &stmt.target);
                checker.collect(&stmt.value, false);
            }
            Stmt::Expr(stmt) => {
                checker.collect(&stmt.expr, false);
                if !expr_falls_through(&stmt.expr) {
                    return;
                }
            }
        }
    }
    if let Some(tail) = &body.tail {
        checker.collect(tail, false);
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_body_param_modes(
    body: &Body,
    initial_locals: &HashMap<String, Type>,
    functions: &BTreeMap<String, FunctionSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    initial_aliases: &HashMap<String, LocalAlias>,
    usages: &mut HashMap<String, ParamUsage>,
) {
    let mut locals = initial_locals.clone();
    let aliases = initial_aliases.clone();
    for statement in &body.statements {
        match statement {
            Stmt::Let(binding) => {
                collect_param_modes(
                    &binding.value,
                    UsageContext::MoveWhole,
                    &locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    &aliases,
                    usages,
                );
                if !locals.contains_key(&binding.name)
                    && let Some(ty) =
                        expr_type_for_ownership(&binding.value, &locals, struct_layouts, None)
                {
                    locals.insert(binding.name.clone(), ty);
                }
            }
            Stmt::Assign(stmt) => {
                collect_assign_target_param_modes(
                    &stmt.target,
                    &locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    &aliases,
                    usages,
                    UsageContext::Borrow,
                );
                collect_param_modes(
                    &stmt.value,
                    UsageContext::MoveWhole,
                    &locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    &aliases,
                    usages,
                );
            }
            Stmt::Expr(stmt) => {
                collect_param_modes(
                    &stmt.expr,
                    UsageContext::MoveWhole,
                    &locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    &aliases,
                    usages,
                );
                if !expr_falls_through(&stmt.expr) {
                    return;
                }
            }
        }
    }
    if let Some(tail) = &body.tail {
        collect_param_modes(
            tail,
            UsageContext::MoveWhole,
            &locals,
            functions,
            enum_variants,
            struct_fields,
            struct_layouts,
            &aliases,
            usages,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_body_param_modes_borrow_only(
    body: &Body,
    initial_locals: &HashMap<String, Type>,
    functions: &BTreeMap<String, FunctionSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    initial_aliases: &HashMap<String, LocalAlias>,
    usages: &mut HashMap<String, ParamUsage>,
) {
    let mut locals = initial_locals.clone();
    let aliases = initial_aliases.clone();
    for statement in &body.statements {
        match statement {
            Stmt::Let(binding) => {
                collect_param_modes(
                    &binding.value,
                    UsageContext::Borrow,
                    &locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    &aliases,
                    usages,
                );
                if !locals.contains_key(&binding.name)
                    && let Some(ty) =
                        expr_type_for_ownership(&binding.value, &locals, struct_layouts, None)
                {
                    locals.insert(binding.name.clone(), ty);
                }
            }
            Stmt::Assign(stmt) => {
                collect_assign_target_param_modes(
                    &stmt.target,
                    &locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    &aliases,
                    usages,
                    UsageContext::Borrow,
                );
                collect_param_modes(
                    &stmt.value,
                    UsageContext::Borrow,
                    &locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    &aliases,
                    usages,
                );
            }
            Stmt::Expr(stmt) => {
                collect_param_modes(
                    &stmt.expr,
                    UsageContext::Borrow,
                    &locals,
                    functions,
                    enum_variants,
                    struct_fields,
                    struct_layouts,
                    &aliases,
                    usages,
                );
                if !expr_falls_through(&stmt.expr) {
                    return;
                }
            }
        }
    }
    if let Some(tail) = &body.tail {
        collect_param_modes(
            tail,
            UsageContext::Borrow,
            &locals,
            functions,
            enum_variants,
            struct_fields,
            struct_layouts,
            &aliases,
            usages,
        );
    }
}

fn record_usage_for_target(
    usages: &mut HashMap<String, ParamUsage>,
    root: &str,
    prefix: &[String],
    context: &UsageContext,
) {
    let entry = usages
        .entry(root.to_owned())
        .or_insert_with(ParamUsage::borrow_only);
    match context {
        UsageContext::Borrow => {}
        UsageContext::MoveWhole => entry.record_path(prefix.to_vec()),
        UsageContext::MovePath(path) => {
            let mut full = prefix.to_vec();
            full.extend(path.clone());
            entry.record_path(full);
        }
        UsageContext::MovePaths(paths) => {
            for path in paths {
                let mut full = prefix.to_vec();
                full.extend(path.clone());
                entry.record_path(full);
            }
        }
    }
}

fn payload_alias_for_scrutinee(
    scrutinee: &Expr,
    aliases: &HashMap<String, LocalAlias>,
) -> Option<LocalAlias> {
    let (root, mut fields) = access_path_for_ownership_with_aliases(scrutinee, aliases)?;
    fields.push(MATCH_PAYLOAD_SEGMENT.to_owned());
    Some(LocalAlias { root, fields })
}

fn match_arm_payload_binding(
    scrutinee: &Expr,
    pattern: &crate::hir::MatchPattern,
    locals: &HashMap<String, Type>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
) -> Option<(String, Type)> {
    let crate::hir::MatchPattern::Variant {
        path,
        binding: Some(binding),
        ..
    } = pattern
    else {
        return None;
    };
    let Type::Named(enum_name) = expr_type_for_ownership(scrutinee, locals, struct_layouts, None)?
    else {
        return None;
    };
    let (_, variant_name) = split_enum_variant_path(&path.path)?;
    let payload_ty = enum_variants
        .get(&enum_name)?
        .iter()
        .find(|variant| variant.name == variant_name)?
        .payload
        .clone()?;
    Some((binding.clone(), payload_ty))
}

fn body_falls_through(body: &Body) -> bool {
    for statement in &body.statements {
        match statement {
            Stmt::Let(_) | Stmt::Assign(_) => {}
            Stmt::Expr(stmt) => {
                if !expr_falls_through(&stmt.expr) {
                    return false;
                }
            }
        }
    }
    body.tail.as_ref().is_none_or(expr_falls_through)
}

fn expr_falls_through(expr: &Expr) -> bool {
    match expr {
        Expr::If(expr) => {
            body_falls_through(&expr.then_body) || body_falls_through(&expr.else_body)
        }
        Expr::Match(expr) => expr.arms.iter().any(|arm| body_falls_through(&arm.body)),
        Expr::Group(expr) => expr_falls_through(&expr.inner),
        Expr::Unary(expr) => expr_falls_through(&expr.inner),
        Expr::Comptime(body) => body_falls_through(body),
        Expr::Handle(_) | Expr::Perform(_) => true,
        Expr::Integer(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Name(_)
        | Expr::Field(_)
        | Expr::Array(_)
        | Expr::Index(_)
        | Expr::Record(_)
        | Expr::ContractResult(_)
        | Expr::Repeat(_)
        | Expr::While(_)
        | Expr::Call(_)
        | Expr::Binary(_) => true,
    }
}

fn body_type_for_ownership(
    body: &Body,
    initial_locals: &HashMap<String, Type>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    result_type: Option<&Type>,
) -> Option<Type> {
    let mut locals = initial_locals.clone();
    for statement in &body.statements {
        match statement {
            Stmt::Let(binding) => {
                let ty =
                    expr_type_for_ownership(&binding.value, &locals, struct_layouts, result_type)?;
                if !locals.contains_key(&binding.name) {
                    locals.insert(binding.name.clone(), ty);
                }
            }
            Stmt::Assign(stmt) => {
                let ty =
                    expr_type_for_ownership(&stmt.value, &locals, struct_layouts, result_type)?;
                if let Expr::Name(name) = &stmt.target
                    && let Some(slot) = locals.get_mut(&name.name)
                {
                    *slot = ty;
                }
            }
            Stmt::Expr(stmt) => {
                let _ = expr_type_for_ownership(&stmt.expr, &locals, struct_layouts, result_type)?;
            }
        }
    }
    body.tail.as_ref().map_or(Some(Type::Unit), |tail| {
        expr_type_for_ownership(tail, &locals, struct_layouts, result_type)
    })
}

fn collect_assign_target_expr(checker: &mut AffineUseChecker<'_>, target: &Expr) {
    if let Expr::Index(expr) = target {
        checker.collect(&expr.index, false);
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_assign_target_param_modes(
    target: &Expr,
    locals: &HashMap<String, Type>,
    functions: &BTreeMap<String, FunctionSignature>,
    enum_variants: &BTreeMap<String, Vec<EnumVariantInfo>>,
    struct_fields: &BTreeMap<String, Vec<Type>>,
    struct_layouts: &BTreeMap<String, Vec<(String, Type)>>,
    aliases: &HashMap<String, LocalAlias>,
    usages: &mut HashMap<String, ParamUsage>,
    context: UsageContext,
) {
    if let Expr::Index(expr) = target {
        collect_param_modes(
            &expr.index,
            context,
            locals,
            functions,
            enum_variants,
            struct_fields,
            struct_layouts,
            aliases,
            usages,
        );
    }
}
