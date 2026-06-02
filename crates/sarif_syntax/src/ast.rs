#![allow(clippy::derive_partial_eq_without_eq)]

use std::fmt::Write;

use crate::{Diagnostic, Element, Node, NodeKind, Span, TokenKind};

#[derive(Clone, Debug, PartialEq)]
pub struct AstFile {
    pub items: Vec<Item>,
}

impl AstFile {
    #[must_use]
    pub fn pretty(&self) -> String {
        let mut output = String::new();

        for item in &self.items {
            match item {
                Item::Const(const_item) => {
                    writeln!(
                        output,
                        "const {}: {} = {};",
                        const_item.name,
                        const_item.ty,
                        const_item.value.pretty()
                    )
                    .expect("writing to string cannot fail");
                }
                Item::Function(function) => {
                    write!(output, "fn {}(", function.name).expect("writing to string cannot fail");
                    for (index, param) in function.params.iter().enumerate() {
                        if index > 0 {
                            output.push_str(", ");
                        }

                        write!(output, "{}: {}", param.name, param.ty)
                            .expect("writing to string cannot fail");
                    }
                    output.push(')');

                    if let Some(return_type) = &function.return_type {
                        write!(output, " -> {return_type}").expect("writing to string cannot fail");
                    }

                    if !function.effects.is_empty() {
                        write!(
                            output,
                            " effects [{}]",
                            function
                                .effects
                                .iter()
                                .map(|effect| effect.name.clone())
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                        .expect("writing to string cannot fail");
                    }

                    if let Some(requires) = &function.requires {
                        write!(output, " requires {}", requires.pretty())
                            .expect("writing to string cannot fail");
                    }

                    if let Some(ensures) = &function.ensures {
                        write!(output, " ensures {}", ensures.pretty())
                            .expect("writing to string cannot fail");
                    }

                    if let Some(body) = &function.body {
                        write!(output, " {{ {} }}", body.pretty())
                            .expect("writing to string cannot fail");
                    } else {
                        output.push_str(" {}");
                    }
                    output.push('\n');
                }
                Item::Enum(enum_item) => {
                    writeln!(output, "enum {}", enum_item.name)
                        .expect("writing to string cannot fail");
                    for variant in &enum_item.variants {
                        writeln!(
                            output,
                            "  {}{}",
                            variant.name,
                            variant
                                .payload
                                .as_ref()
                                .map_or_else(String::new, |payload| format!("({payload})")),
                        )
                        .expect("writing to string cannot fail");
                    }
                }
                Item::Struct(struct_item) => {
                    writeln!(output, "struct {}", struct_item.name)
                        .expect("writing to string cannot fail");
                    for field in &struct_item.fields {
                        writeln!(output, "  {}: {}", field.name, field.ty)
                            .expect("writing to string cannot fail");
                    }
                }
                Item::Effect(effect) => {
                    writeln!(output, "effect {}", effect.name)
                        .expect("writing to string cannot fail");
                    for method in &effect.methods {
                        writeln!(output, "  fn {}(...)", method.name)
                            .expect("writing to string cannot fail");
                    }
                }
        Item::ExternBlock(block) => {
            writeln!(output, "extern {{").expect("writing to string cannot fail");
            for function in &block.functions {
                write!(output, " fn {}(", function.name)
                    .expect("writing to string cannot fail");
                for (index, param) in function.params.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    write!(output, "{}: {}", param.name, param.ty)
                        .expect("writing to string cannot fail");
                }
                output.push(')');
                if let Some(return_type) = &function.return_type {
                    write!(output, " -> {return_type}")
                        .expect("writing to string cannot fail");
                }
                writeln!(output, ";").expect("writing to string cannot fail");
            }
            writeln!(output, "}}").expect("writing to string cannot fail");
        }
        Item::Import(import) => {
            write!(output, "from {} import ", import.module)
                .expect("writing to string cannot fail");
            for (index, name) in import.names.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                output.push_str(name);
            }
        output.push('\n');
        }
            }
        }

        output
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Const(Const),
    Function(Function),
    Enum(Enum),
    Struct(Struct),
    Effect(Effect),
    ExternBlock(ExternBlock),
    Import(ImportItem),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportItem {
    pub module: String,
    pub names: Vec<String>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub name: String,
    pub methods: Vec<EffectMethod>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExternBlock {
    pub functions: Vec<ExternFunction>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExternFunction {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypePath>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypePath>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Const {
    pub name: String,
    pub ty: TypePath,
    pub value: Expr,
    pub span: Span,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenericParam {
    pub name: String,
    pub kind: Option<String>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: String,
    pub type_params: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypePath>,
    pub effects: Vec<EffectName>,
    pub requires: Option<Expr>,
    pub ensures: Option<Expr>,
    pub body: Option<Body>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub statements: Vec<Stmt>,
    pub tail: Option<Expr>,
    pub span: Span,
}

impl Body {
    #[must_use]
    pub fn pretty(&self) -> String {
        let mut parts = self.statements.iter().map(Stmt::pretty).collect::<Vec<_>>();
        if let Some(tail) = &self.tail {
            parts.push(tail.pretty());
        }
        parts.join(" ")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Let(LetBinding),
    Assign(AssignStmt),
    Expr(ExprStmt),
    WithArena(WithArenaStmt),
}

impl Stmt {
    #[must_use]
    pub fn pretty(&self) -> String {
        match self {
            Self::Let(binding) => format!(
                "let {}{} = {};",
                if binding.mutable { "mut " } else { "" },
                binding.name,
                binding.value.pretty()
            ),
            Self::Assign(stmt) => format!("{} = {};", stmt.target.pretty(), stmt.value.pretty()),
            Self::Expr(stmt) => format!("{};", stmt.expr.pretty()),
            Self::WithArena(stmt) => format!("with_arena {};", stmt.body.pretty()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LetBinding {
    pub mutable: bool,
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AssignStmt {
    pub target: Expr,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExprStmt {
    pub expr: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WithArenaStmt {
    pub body: Body,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Struct {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Enum {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub payload: Option<TypePath>,
    pub discriminant: Option<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: TypePath,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: TypePath,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectName {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArrayLen {
    Literal(usize),
    Name(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypePath {
    Named { segments: Vec<String> },
    Array { element: Box<Self>, len: ArrayLen },
    Generic { name: String, args: Vec<Self> },
}

impl TypePath {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Named { segments } => {
                if segments.len() > 1 {
                    &segments[0]
                } else {
                    ""
                }
            }
            Self::Generic { name, .. } => name,
            Self::Array { .. } => "",
        }
    }

    #[must_use]
    pub fn operation(&self) -> &str {
        match self {
            Self::Named { segments } => segments.last().map_or("", String::as_str),
            Self::Generic { .. } | Self::Array { .. } => "",
        }
    }
}

impl std::fmt::Display for TypePath {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named { segments } => formatter.write_str(&segments.join(".")),
            Self::Array { element, len } => match len {
                ArrayLen::Literal(l) => write!(formatter, "[{element}; {l}]"),
                ArrayLen::Name(n) => write!(formatter, "[{element}; {n}]"),
            },
            Self::Generic { name, args } => {
                let args_str = args
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(formatter, "{name}[{args_str}]")
            }
        }
    }
}

impl MatchPattern {
    #[must_use]
    pub fn pretty(&self) -> String {
        match self {
            Self::Variant {
                path,
                binding: None,
                ..
            } => path.to_string(),
            Self::Variant {
                path,
                binding: Some(binding),
                ..
            } => format!("{path}({binding})"),
            Self::Integer { value, .. } => value.to_string(),
            Self::String { literal, .. } => literal.clone(),
            Self::Bool { value, .. } => value.to_string(),
            Self::IntegerRange { start, end, .. } => format!("{start}..{end}"),
            Self::Or { patterns, .. } => patterns
                .iter()
                .map(Self::pretty)
                .collect::<Vec<_>>()
                .join(" | "),
            Self::Wildcard { .. } => "_".to_owned(),
        }
    }

    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Variant { span, .. }
            | Self::Integer { span, .. }
            | Self::String { span, .. }
            | Self::Bool { span, .. }
            | Self::IntegerRange { span, .. }
            | Self::Or { span, .. }
            | Self::Wildcard { span } => *span,
        }
    }
}

impl FieldExpr {
    #[must_use]
    pub fn pretty(&self) -> String {
        format!("{}.{}", self.base.pretty(), self.field)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PerformExpr {
    pub callee: TypePath,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Integer(IntegerExpr),
    Float(FloatExpr),
    String(StringExpr),
    Bool(BoolExpr),
    Name(NameExpr),
    ContractResult(ContractResultExpr),
    Call(CallExpr),
    Array(ArrayExpr),
    Field(FieldExpr),
    Index(IndexExpr),
    If(Box<IfExpr>),
    Match(Box<MatchExpr>),
    Repeat(Box<RepeatExpr>),
    For(Box<ForExpr>),
    While(Box<WhileExpr>),
    Record(RecordExpr),
    Unary(UnaryExpr),
    Binary(BinaryExpr),
    Group(GroupExpr),
    Comptime(Box<ComptimeExpr>),
    Perform(PerformExpr),
    Handle(Box<HandleExpr>),
    Template(TemplateExpr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemplateExpr {
    pub segments: Vec<TemplateSegment>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TemplateSegment {
    Text(String),
    Expr(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct HandleExpr {
    pub body: Body,
    pub arms: Vec<HandleArm>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HandleArm {
    pub name: String,
    pub params: Vec<String>,
    pub body: Body,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComptimeExpr {
    pub body: Body,
    pub span: Span,
}

impl Expr {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Integer(expr) => expr.span,
            Self::Float(expr) => expr.span,
            Self::String(expr) => expr.span,
            Self::Bool(expr) => expr.span,
            Self::Name(expr) => expr.span,
            Self::ContractResult(expr) => expr.span,
            Self::Call(expr) => expr.span,
            Self::Array(expr) => expr.span,
            Self::Field(expr) => expr.span,
            Self::Index(expr) => expr.span,
            Self::If(expr) => expr.span,
            Self::Match(expr) => expr.span,
            Self::Repeat(expr) => expr.span,
            Self::For(expr) => expr.span,
            Self::While(expr) => expr.span,
            Self::Record(expr) => expr.span,
            Self::Unary(expr) => expr.span,
            Self::Binary(expr) => expr.span,
            Self::Group(expr) => expr.span,
            Self::Comptime(expr) => expr.span,
            Self::Perform(expr) => expr.span,
            Self::Handle(expr) => expr.span,
            Self::Template(expr) => expr.span,
        }
    }

    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn pretty(&self) -> String {
        match self {
            Self::Integer(expr) => expr.value.to_string(),
            Self::Float(expr) => {
                let mut s = expr.value.to_string();
                if !s.contains('.') {
                    s.push_str(".0");
                }
                s
            }
            Self::String(expr) => expr.literal.clone(),
            Self::Bool(expr) => expr.value.to_string(),
            Self::Name(expr) => expr.name.clone(),
            Self::ContractResult(_) => "result".to_owned(),
            Self::Call(expr) => format!(
                "{}({})",
                expr.callee,
                expr.args
                    .iter()
                    .map(Self::pretty)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Self::Array(expr) => format!(
                "[{}]",
                expr.elements
                    .iter()
                    .map(Self::pretty)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Self::Field(expr) => format!("{}.{}", expr.base.pretty(), expr.field),
            Self::Index(expr) => format!("{}[{}]", expr.base.pretty(), expr.index.pretty()),
            Self::If(expr) => format!(
                "if {} {{ {} }} else {{ {} }}",
                expr.condition.pretty(),
                expr.then_body.pretty(),
                expr.else_body.pretty(),
            ),
            Self::Match(expr) => format!(
                "match {} {{ {} }}",
                expr.scrutinee.pretty(),
                expr.arms
                    .iter()
                    .map(|arm| format!("{} => {{ {} }}", arm.pattern.pretty(), arm.body.pretty()))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Self::Repeat(expr) => format!(
                "{} {{ {} }}",
                expr.binding.as_ref().map_or_else(
                    || format!("repeat {}", expr.count.pretty()),
                    |binding| format!("repeat {binding} in {}", expr.count.pretty()),
                ),
                expr.body.pretty(),
            ),
            Self::For(expr) => format!(
                "for {} in {}..{} {{ {} }}",
                expr.binding,
                expr.start.pretty(),
                expr.end.pretty(),
                expr.body.pretty(),
            ),
            Self::While(expr) => {
                format!(
                    "while {} {{ {} }}",
                    expr.condition.pretty(),
                    expr.body.pretty()
                )
            }
            Self::Record(expr) => format!(
                "{} {{ {} }}",
                expr.name,
                expr.fields
                    .iter()
                    .map(|field| format!("{}: {}", field.name, field.value.pretty()))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Self::Unary(expr) => format!("{} {}", expr.op.symbol(), expr.inner.pretty()),
            Self::Binary(expr) => format!(
                "{} {} {}",
                expr.left.pretty(),
                expr.op.symbol(),
                expr.right.pretty(),
            ),
            Self::Group(expr) => format!("({})", expr.inner.pretty()),
            Self::Comptime(expr) => format!("comptime {{ {} }}", expr.body.pretty()),
            Self::Perform(expr) => {
                let args = expr
                    .args
                    .iter()
                    .map(Self::pretty)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("perform {}({})", expr.callee, args)
            }
            Self::Handle(expr) => format!("handle {} with {{ ... }}", expr.body.pretty()),
            Self::Template(expr) => {
                let mut out = String::from("\"");
                for seg in &expr.segments {
                    match seg {
                        TemplateSegment::Text(t) => out.push_str(t),
                        TemplateSegment::Expr(e) => {
                            out.push('{');
                            out.push_str(&e.pretty());
                            out.push('}');
                        }
                    }
                }
                out.push('"');
                out
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegerExpr {
    pub value: i64,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FloatExpr {
    pub value: f64,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringExpr {
    pub literal: String,
    pub value: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoolExpr {
    pub value: bool,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameExpr {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractResultExpr {
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CallExpr {
    pub callee: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArrayExpr {
    pub elements: Vec<Expr>,
    pub repeat_len: Option<ArrayLen>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldExpr {
    pub base: Box<Expr>,
    pub field: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IndexExpr {
    pub base: Box<Expr>,
    pub index: Box<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IfExpr {
    pub condition: Box<Expr>,
    pub then_body: Body,
    pub else_body: Body,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchExpr {
    pub scrutinee: Box<Expr>,
    pub arms: Vec<MatchArm>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body: Body,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MatchPattern {
    Variant {
        path: TypePath,
        binding: Option<String>,
        span: Span,
    },
    Integer {
        value: i64,
        span: Span,
    },
    String {
        literal: String,
        value: String,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    IntegerRange {
        start: i64,
        end: i64,
        span: Span,
    },
    Or {
        patterns: Vec<Self>,
        span: Span,
    },
    Wildcard {
        span: Span,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RepeatExpr {
    pub binding: Option<String>,
    pub count: Box<Expr>,
    pub body: Body,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ForExpr {
    pub binding: String,
    pub start: Box<Expr>,
    pub end: Box<Expr>,
    pub body: Body,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WhileExpr {
    pub condition: Box<Expr>,
    pub body: Body,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordExpr {
    pub name: String,
    pub fields: Vec<FieldInit>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldInit {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub inner: Box<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BinaryExpr {
    pub op: BinaryOp,
    pub left: Box<Expr>,
    pub right: Box<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupExpr {
    pub inner: Box<Expr>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    And,
    Or,
    BitOr,
    BitXor,
    BitAnd,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Shl,
    Shr,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
}

impl UnaryOp {
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Not => "not",
        }
    }
}

impl BinaryOp {
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::BitOr => "|",
            Self::BitXor => "^",
            Self::BitAnd => "&",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LoweredAst {
    pub file: AstFile,
    pub diagnostics: Vec<Diagnostic>,
}

#[must_use]
pub fn lower(root: &Node) -> LoweredAst {
    let mut lowerer = Lowerer {
        diagnostics: Vec::new(),
    };
    let file = lowerer.lower_file(root);
    LoweredAst {
        file,
        diagnostics: lowerer.diagnostics,
    }
}

struct Lowerer {
    diagnostics: Vec<Diagnostic>,
}

impl Lowerer {
    fn lower_file(&mut self, root: &Node) -> AstFile {
        let mut items = Vec::new();

        for child in &root.children {
            let Element::Node(node) = child else {
                continue;
            };

            match node.kind {
                NodeKind::ConstItem => {
                    if let Some(const_item) = self.lower_const(node) {
                        items.push(Item::Const(const_item));
                    }
                }
                NodeKind::FnItem => {
                    if let Some(function) = self.lower_function(node) {
                        items.push(Item::Function(function));
                    }
                }
                NodeKind::EnumItem => {
                    if let Some(enum_item) = self.lower_enum(node) {
                        items.push(Item::Enum(enum_item));
                    }
                }
                NodeKind::StructItem => {
                    if let Some(item) = self.lower_struct(node) {
                        items.push(Item::Struct(item));
                    }
                }
                NodeKind::EffectItem => {
                    if let Some(item) = self.lower_effect_item(node) {
                        items.push(Item::Effect(item));
                    }
                }
                NodeKind::ExternBlock => {
                    if let Some(item) = self.lower_extern_block(node) {
                        items.push(Item::ExternBlock(item));
                    }
                }
                NodeKind::ImportItem => {
                    if let Some(item) = self.lower_import(node) {
                        items.push(Item::Import(item));
                    }
                }
                _ => {}
            }
        }
        AstFile { items }
    }

    fn lower_effect_item(&mut self, node: &Node) -> Option<Effect> {
        let name = self.ident_after(node, TokenKind::KwEffect)?;
        let methods = node
            .children
            .iter()
            .filter_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::FnItem => {
                    let name = self.ident_after(child, TokenKind::KwFn)?;
                    let params = child
                        .children
                        .iter()
                        .find_map(|child| match child {
                            Element::Node(child) if child.kind == NodeKind::ParamList => Some(
                                child
                                    .children
                                    .iter()
                                    .filter_map(|element| match element {
                                        Element::Node(param) if param.kind == NodeKind::Param => {
                                            self.lower_param(param)
                                        }
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let return_type = child.children.iter().find_map(|child| match child {
                        Element::Node(child) if child.kind == NodeKind::ReturnType => {
                            child.children.iter().find_map(|element| match element {
                                Element::Node(ty)
                                    if matches!(
                                        ty.kind,
                                        NodeKind::TypePath | NodeKind::TypeArray
                                    ) =>
                                {
                                    self.lower_type_path(ty)
                                }
                                _ => None,
                            })
                        }
                        _ => None,
                    });
                    Some(EffectMethod {
                        name,
                        params,
                        return_type,
                        span: child.span,
                    })
                }
                _ => None,
            })
            .collect();
        Some(Effect {
            name,
            methods,
            span: node.span,
        })
    }

    fn lower_extern_block(&mut self, node: &Node) -> Option<ExternBlock> {
        let functions = node
            .children
            .iter()
            .filter_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::FnItem => {
                    let name = self.ident_after(child, TokenKind::KwFn)?;
                    let params = child
                        .children
                        .iter()
                        .find_map(|child| match child {
                            Element::Node(child) if child.kind == NodeKind::ParamList => Some(
                                child
                                    .children
                                    .iter()
                                    .filter_map(|element| match element {
                                        Element::Node(param) if param.kind == NodeKind::Param => {
                                            self.lower_param(param)
                                        }
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let return_type = child.children.iter().find_map(|child| match child {
                        Element::Node(child) if child.kind == NodeKind::ReturnType => {
                            child.children.iter().find_map(|element| match element {
                                Element::Node(ty)
                                    if matches!(
                                        ty.kind,
                                        NodeKind::TypePath | NodeKind::TypeArray
                                    ) =>
                                {
                                    self.lower_type_path(ty)
                                }
                                _ => None,
                            })
                        }
                        _ => None,
                    });
                    Some(ExternFunction {
                        name,
                        params,
                        return_type,
                        span: child.span,
                    })
                }
                _ => None,
            })
            .collect();
        Some(ExternBlock {
            functions,
            span: node.span,
        })
    }

    fn lower_import(&mut self, node: &Node) -> Option<ImportItem> {
        let module = self.ident_after(node, TokenKind::KwFrom)?;
        let mut names = Vec::new();
        let mut past_import = false;
        for child in &node.children {
            if let Element::Token(token) = child {
                if token.kind == TokenKind::KwImport {
                    past_import = true;
                } else if past_import && token.kind == TokenKind::Ident {
                    names.push(token.lexeme.clone());
                }
            }
        }
        Some(ImportItem {
            module,
            names,
            span: node.span,
        })
    }

    fn lower_const(&mut self, node: &Node) -> Option<Const> {
        let name = self.ident_after(node, TokenKind::KwConst)?;
        let ty = self.first_type_path(node)?;
        let value = node.children.iter().find_map(|child| match child {
            Element::Node(expr)
                if matches!(
                    expr.kind,
                    NodeKind::ExprInteger
                        | NodeKind::ExprFloat
                        | NodeKind::ExprString
                        | NodeKind::ExprBool
                        | NodeKind::ExprName
                        | NodeKind::ExprContractResult
                        | NodeKind::ExprCall
                        | NodeKind::ExprArray
                        | NodeKind::ExprField
                        | NodeKind::ExprIndex
                        | NodeKind::ExprIf
                        | NodeKind::ExprMatch
                        | NodeKind::ExprRepeat
                        | NodeKind::ExprWhile
                        | NodeKind::ExprRecord
                        | NodeKind::ExprUnary
                        | NodeKind::ExprBinary
                        | NodeKind::ExprGroup
                        | NodeKind::ExprComptime
                ) =>
            {
                self.lower_expr(expr)
            }
            Element::Token(_) | Element::Node(_) => None,
        })?;
        Some(Const {
            name,
            ty,
            value,
            span: node.span,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn lower_function(&mut self, node: &Node) -> Option<Function> {
        let name = self.ident_after(node, TokenKind::KwFn)?;
        let type_params = node
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::GenericParamList => Some(
                    child
                        .children
                        .iter()
                        .filter_map(|element| match element {
                            Element::Node(param) if param.kind == NodeKind::GenericParam => {
                                let name = Self::first_ident(param)?;
                                let kind = param
                                .children
                                .iter()
                                .skip_while(|e| {
                                    !matches!(e, Element::Token(t) if t.kind == TokenKind::Colon)
                                })
                                .skip(1)
                                .find_map(|e| match e {
                                    Element::Token(t) if t.kind == TokenKind::Ident => {
                                        Some(t.lexeme.clone())
                                    }
                                    _ => None,
                                });
                                Some(GenericParam {
                                    name,
                                    kind,
                                    span: param.span,
                                })
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .unwrap_or_default();
        let params = node
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::ParamList => Some(
                    child
                        .children
                        .iter()
                        .filter_map(|element| match element {
                            Element::Node(param) if param.kind == NodeKind::Param => {
                                self.lower_param(param)
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .unwrap_or_default();
        let return_type = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::ReturnType => {
                child.children.iter().find_map(|element| match element {
                    Element::Node(ty)
                        if matches!(ty.kind, NodeKind::TypePath | NodeKind::TypeArray) =>
                    {
                        self.lower_type_path(ty)
                    }
                    _ => None,
                })
            }
            _ => None,
        });
        let effects = node
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::EffectsClause => Some(
                    child
                        .children
                        .iter()
                        .find_map(|element| match element {
                            Element::Node(list) if list.kind == NodeKind::EffectList => Some(
                                list.children
                                    .iter()
                                    .filter_map(|entry| match entry {
                                        Element::Token(token) if token.kind == TokenKind::Ident => {
                                            Some(EffectName {
                                                name: token.lexeme.clone(),
                                                span: token.span,
                                            })
                                        }
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                            _ => None,
                        })
                        .unwrap_or_default(),
                ),
                _ => None,
            })
            .unwrap_or_default();
        let requires = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::RequiresClause => {
                self.lower_clause_expr(child)
            }
            _ => None,
        });
        let ensures = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::EnsuresClause => {
                self.lower_clause_expr(child)
            }
            _ => None,
        });
        let body = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::Body => Some(self.lower_body(child)),
            _ => None,
        });

        Some(Function {
            name,
            type_params,
            params,
            return_type,
            effects,
            requires,
            ensures,
            body,
            span: node.span,
        })
    }

    fn lower_struct(&mut self, node: &Node) -> Option<Struct> {
        let name = self.ident_after(node, TokenKind::KwStruct)?;
        let fields = node
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::FieldList => Some(
                    child
                        .children
                        .iter()
                        .filter_map(|element| match element {
                            Element::Node(field) if field.kind == NodeKind::Field => {
                                self.lower_field(field)
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .unwrap_or_default();

        Some(Struct {
            name,
            fields,
            span: node.span,
        })
    }

    fn lower_enum(&mut self, node: &Node) -> Option<Enum> {
        let name = self.ident_after(node, TokenKind::KwEnum)?;
        let variants = node
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::VariantList => Some(
                    child
                        .children
                        .iter()
                        .filter_map(|element| match element {
                            Element::Node(variant) if variant.kind == NodeKind::Variant => {
                                let mut name: Option<String> = None;
                                let mut payload: Option<TypePath> = None;
                                let mut discriminant: Option<Expr> = None;
                                let mut span: Span = Span::default();
                                let mut after_eq = false;

                                for child in &variant.children {
                                    match child {
                                        Element::Token(token)
                                            if token.kind == TokenKind::Ident && name.is_none() =>
                                        {
                                            name = Some(token.lexeme.clone());
                                            span = token.span;
                                        }
                                        Element::Token(token) if token.kind == TokenKind::Eq => {
                                            after_eq = true;
                                        }
                                        Element::Node(node)
                                            if matches!(
                                                node.kind,
                                                NodeKind::TypePath | NodeKind::TypeArray
                                            ) && payload.is_none()
                                                && !after_eq =>
                                        {
                                            payload = self.lower_type_path(node);
                                        }
                                        Element::Node(node)
                                            if node.kind.is_expr()
                                                && after_eq
                                                && discriminant.is_none() =>
                                        {
                                            discriminant = self.lower_expr(node);
                                        }
                                        _ => {}
                                    }
                                }

                                name.map(|name| EnumVariant {
                                    name,
                                    payload,
                                    discriminant,
                                    span,
                                })
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .unwrap_or_default();

        Some(Enum {
            name,
            variants,
            span: node.span,
        })
    }

    fn lower_param(&mut self, node: &Node) -> Option<Param> {
        Some(Param {
            name: Self::first_ident(node)?,
            ty: self.first_type_path(node)?,
            span: node.span,
        })
    }

    fn lower_field(&mut self, node: &Node) -> Option<Field> {
        Some(Field {
            name: Self::first_ident(node)?,
            ty: self.first_type_path(node)?,
            span: node.span,
        })
    }

    fn lower_type_path(&mut self, node: &Node) -> Option<TypePath> {
        match node.kind {
            NodeKind::TypePath => {
                let segments = node
                    .children
                    .iter()
                    .filter_map(|child| match child {
                        Element::Token(token) if token.kind == TokenKind::Ident => {
                            Some(token.lexeme.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();

                if segments.is_empty() {
                    self.diagnostics.push(Diagnostic::new(
                        "ast.missing-type-path",
                        "type path lowering failed",
                        node.span,
                        Some("Use a non-empty type path.".to_owned()),
                    ));
                    return None;
                }

                let mut args = Vec::new();
                for child in &node.children {
                    if let Element::Node(child_node) = child
                        && matches!(child_node.kind, NodeKind::TypePath | NodeKind::TypeArray)
                        && let Some(arg) = self.lower_type_path(child_node)
                    {
                        args.push(arg);
                    }
                }

                if args.is_empty() {
                    Some(TypePath::Named { segments })
                } else {
                    Some(TypePath::Generic {
                        name: segments.join("."),
                        args,
                    })
                }
            }
            NodeKind::TypeArray => {
                let element = node.children.iter().find_map(|child| match child {
                    Element::Node(child)
                        if matches!(child.kind, NodeKind::TypePath | NodeKind::TypeArray) =>
                    {
                        self.lower_type_path(child)
                    }
                    _ => None,
                })?;
                let len = node.children.iter().find_map(|child| match child {
                    Element::Token(token) if token.kind == TokenKind::Integer => {
                        token.lexeme.parse::<usize>().ok().map(ArrayLen::Literal)
                    }
                    Element::Token(token) if token.kind == TokenKind::Ident => {
                        Some(ArrayLen::Name(token.lexeme.clone()))
                    }
                    _ => None,
                })?;
                Some(TypePath::Array {
                    element: Box::new(element),
                    len,
                })
            }
            _ => None,
        }
    }

    fn lower_body(&mut self, node: &Node) -> Body {
        let mut statements = Vec::new();
        let mut tail = None;
        for child in &node.children {
            match child {
                Element::Node(child) if child.kind == NodeKind::LetStmt => {
                    if Self::let_is_destructure(child) {
                        if let Some(bindings) = self.lower_let_destructure(child) {
                            statements.extend(bindings.into_iter().map(Stmt::Let));
                        }
                    } else if let Some(binding) = self.lower_let_binding(child) {
                        statements.push(Stmt::Let(binding));
                    }
                }
                Element::Node(child) if child.kind == NodeKind::AssignStmt => {
                    if let Some(stmt) = self.lower_assign_stmt(child) {
                        statements.push(Stmt::Assign(stmt));
                    }
                }
                Element::Node(child) if child.kind == NodeKind::ExprStmt => {
                    if let Some(stmt) = self.lower_expr_stmt(child) {
                        statements.push(Stmt::Expr(stmt));
                    }
                }
                Element::Node(child) if child.kind == NodeKind::WithArenaStmt => {
                    if let Some(stmt) = self.lower_with_arena_stmt(child) {
                        statements.push(Stmt::WithArena(stmt));
                    }
                }
                Element::Node(child) if tail.is_none() => {
                    tail = self.lower_expr(child);
                }
                Element::Node(_) | Element::Token(_) => {}
            }
        }
        Body {
            statements,
            tail,
            span: node.span,
        }
    }

    fn let_is_destructure(node: &Node) -> bool {
        node.children
            .iter()
            .any(|child| matches!(child, Element::Token(token) if token.kind == TokenKind::LParen))
    }

    fn lower_let_destructure(&mut self, node: &Node) -> Option<Vec<LetBinding>> {
        let mut idents = Vec::new();
        let mut found_lparen = false;
        let mut value = None;
        for child in &node.children {
            match child {
                Element::Token(token) if token.kind == TokenKind::LParen => {
                    found_lparen = true;
                }
                Element::Token(token)
                    if token.kind == TokenKind::Ident && found_lparen && value.is_none() =>
                {
                    idents.push(token.lexeme.clone());
                }
                Element::Node(expr) if found_lparen && value.is_none() => {
                    value = self.lower_expr(expr);
                }
                _ => {}
            }
        }
        let value = value?;
        let mutable = node.children.iter().any(|child| {
            matches!(
                child,
                Element::Token(token) if token.kind == TokenKind::KwMut
            )
        });
        let span = node.span;
        let bindings: Vec<LetBinding> = idents
            .iter()
            .enumerate()
            .map(|(i, name)| LetBinding {
                mutable,
                name: name.clone(),
                value: Expr::Field(FieldExpr {
                    base: Box::new(value.clone()),
                    field: i.to_string(),
                    span,
                }),
                span,
            })
            .collect();
        if bindings.is_empty() {
            None
        } else {
            Some(bindings)
        }
    }

    fn lower_clause_expr(&mut self, node: &Node) -> Option<Expr> {
        node.children.iter().find_map(|child| match child {
            Element::Node(expr) => self.lower_expr(expr),
            Element::Token(_) => None,
        })
    }

fn lower_let_binding(&mut self, node: &Node) -> Option<LetBinding> {
    let name = Self::first_ident(node)
        .or_else(|| Self::first_token(node, TokenKind::Underscore).map(|_| "_".to_owned()))?;
        let mutable = node.children.iter().any(|child| {
            matches!(
                child,
                Element::Token(token) if token.kind == TokenKind::KwMut
            )
        });
        let value = node.children.iter().find_map(|child| match child {
            Element::Node(expr) => self.lower_expr(expr),
            Element::Token(_) => None,
        })?;
        Some(LetBinding {
            mutable,
            name,
            value,
            span: node.span,
        })
    }

    fn lower_assign_stmt(&mut self, node: &Node) -> Option<AssignStmt> {
        let mut target = None;
        let mut value = None;
        let mut op = TokenKind::Eq;
        for child in &node.children {
            match child {
                Element::Node(expr) => {
                    if target.is_none() {
                        target = self.lower_expr(expr);
                    } else if value.is_none() {
                        value = self.lower_expr(expr);
                    }
                }
                Element::Token(token)
                    if matches!(
                        token.kind,
                        TokenKind::Eq
                            | TokenKind::PlusEq
                            | TokenKind::MinusEq
                            | TokenKind::StarEq
                            | TokenKind::SlashEq
                    ) =>
                {
                    op = token.kind;
                }
                Element::Token(_) => {}
            }
        }
        let target = target?;
        let mut value = value?;
        if let Some(binary_op) = compound_assign_op(op) {
            value = Expr::Binary(BinaryExpr {
                op: binary_op,
                left: Box::new(target.clone()),
                right: Box::new(value),
                span: node.span,
            });
        }
        Some(AssignStmt {
            target,
            value,
            span: node.span,
        })
    }

    fn lower_expr_stmt(&mut self, node: &Node) -> Option<ExprStmt> {
        let expr = node.children.iter().find_map(|child| match child {
            Element::Node(expr) => self.lower_expr(expr),
            Element::Token(_) => None,
        })?;
        Some(ExprStmt {
            expr,
            span: node.span,
        })
    }

    fn lower_with_arena_stmt(&mut self, node: &Node) -> Option<WithArenaStmt> {
        let body = node.children.iter().find_map(|child| match child {
            Element::Node(body) => Some(self.lower_body(body)),
            Element::Token(_) => None,
        })?;
        Some(WithArenaStmt {
            body,
            span: node.span,
        })
    }

    fn lower_expr(&mut self, node: &Node) -> Option<Expr> {
        match node.kind {
            NodeKind::ExprInteger => Self::lower_integer_expr(node).map(Expr::Integer),
            NodeKind::ExprFloat => Self::lower_float_expr(node).map(Expr::Float),
            NodeKind::ExprString => {
                let string_expr = Self::lower_string_expr(node);
                match &string_expr {
                    Some(s) if s.value.contains('{') && Self::has_interpolation(&s.value) => {
                        match Self::lower_template_expr(node) {
                            Some(template)
                                if template
                                    .segments
                                    .iter()
                                    .any(|seg| matches!(seg, TemplateSegment::Expr(_))) =>
                            {
                                Some(Expr::Template(template))
                            }
                            _ => string_expr.map(Expr::String),
                        }
                    }
                    _ => string_expr.map(Expr::String),
                }
            }
            NodeKind::ExprBool => Self::lower_bool_expr(node).map(Expr::Bool),
            NodeKind::ExprName => Self::lower_name_expr(node).map(Expr::Name),
            NodeKind::ExprContractResult => Some(Expr::ContractResult(
                Self::lower_contract_result_expr(node)?,
            )),
            NodeKind::ExprCall => self.lower_call_expr(node).map(Expr::Call),
            NodeKind::ExprArray => Some(Expr::Array(self.lower_array_expr(node))),
            NodeKind::ExprField => self.lower_field_expr(node).map(Expr::Field),
            NodeKind::ExprIndex => self.lower_index_expr(node).map(Expr::Index),
            NodeKind::ExprIf => self
                .lower_if_expr(node)
                .map(|expr| Expr::If(Box::new(expr))),
            NodeKind::ExprMatch => self
                .lower_match_expr(node)
                .map(|expr| Expr::Match(Box::new(expr))),
            NodeKind::ExprRepeat => self
                .lower_repeat_expr(node)
                .map(|expr| Expr::Repeat(Box::new(expr))),
            NodeKind::ExprFor => Some(self.lower_for_expr(node)),
            NodeKind::ExprWhile => self
                .lower_while_expr(node)
                .map(|expr| Expr::While(Box::new(expr))),
            NodeKind::ExprRecord => self.lower_record_expr(node).map(Expr::Record),
            NodeKind::ExprUnary => self.lower_unary_expr(node).map(Expr::Unary),
            NodeKind::ExprBinary => self.lower_binary_expr(node).map(Expr::Binary),
            NodeKind::ExprGroup => self.lower_group_expr(node).map(Expr::Group),
            NodeKind::ExprComptime => self.lower_comptime_expr(node).map(Expr::Comptime),
            NodeKind::ExprPerform => self.lower_perform_expr(node).map(Expr::Perform),
            NodeKind::ExprHandle => self
                .lower_handle_expr(node)
                .map(|e| Expr::Handle(Box::new(e))),
            NodeKind::Error => None,
            _ => {
                self.diagnostics.push(Diagnostic::new(
                    "ast.invalid-expression",
                    "expression lowering failed",
                    node.span,
                    Some("Use a supported stage-0 expression form.".to_owned()),
                ));
                None
            }
        }
    }

    fn lower_handle_expr(&mut self, node: &Node) -> Option<HandleExpr> {
        let body = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::Body => Some(self.lower_body(child)),
            _ => None,
        })?;
        let arms = node
            .children
            .iter()
            .filter_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::HandleArm => {
                    self.lower_handle_arm(child)
                }
                _ => None,
            })
            .collect();
        Some(HandleExpr {
            body,
            arms,
            span: node.span,
        })
    }

    fn lower_handle_arm(&mut self, node: &Node) -> Option<HandleArm> {
        let name = Self::first_ident(node)?;
        let params = node
            .children
            .iter()
            .skip_while(|e| !matches!(e, Element::Token(t) if t.kind == TokenKind::LParen))
            .skip(1)
            .take_while(|e| !matches!(e, Element::Token(t) if t.kind == TokenKind::RParen))
            .filter_map(|e| match e {
                Element::Token(t) if t.kind == TokenKind::Ident => Some(t.lexeme.clone()),
                _ => None,
            })
            .collect();
        let body = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::Body => Some(self.lower_body(child)),
            _ => None,
        })?;
        Some(HandleArm {
            name,
            params,
            body,
            span: node.span,
        })
    }

    fn lower_comptime_expr(&mut self, node: &Node) -> Option<Box<ComptimeExpr>> {
        let body = node.children.iter().find_map(|child| match child {
            Element::Node(body) if body.kind == NodeKind::Body => Some(self.lower_body(body)),
            _ => None,
        })?;
        Some(Box::new(ComptimeExpr {
            body,
            span: node.span,
        }))
    }

    fn lower_perform_expr(&mut self, node: &Node) -> Option<PerformExpr> {
        let callee_node = node.children.iter().find_map(|child| {
            if let Element::Node(node) = child
                && node.kind == NodeKind::TypePath
            {
                Some(node)
            } else {
                None
            }
        })?;
        let callee = self.lower_type_path(callee_node)?;
        let args_node = node.children.iter().find_map(|child| {
            if let Element::Node(node) = child
                && node.kind == NodeKind::ArgList
            {
                Some(node)
            } else {
                None
            }
        })?;
        let args = args_node
            .children
            .iter()
            .filter_map(|child| match child {
                Element::Node(expr) => self.lower_expr(expr),
                Element::Token(_) => None,
            })
            .collect();
        Some(PerformExpr {
            callee,
            args,
            span: node.span,
        })
    }

    fn lower_integer_expr(node: &Node) -> Option<IntegerExpr> {
        let token = Self::first_token(node, TokenKind::Integer)?;
        let lexeme = Self::signed_numeric_lexeme(node, token);
        let value = lexeme.parse::<i64>().ok()?;
        Some(IntegerExpr {
            value,
            span: token.span,
        })
    }

    fn lower_float_expr(node: &Node) -> Option<FloatExpr> {
        let token = Self::first_token(node, TokenKind::Float)?;
        let lexeme = Self::signed_numeric_lexeme(node, token);
        let value = lexeme.parse::<f64>().ok()?;
        Some(FloatExpr {
            value,
            span: token.span,
        })
    }

    fn signed_numeric_lexeme(node: &Node, token: &crate::Token) -> String {
        if node
            .children
            .iter()
            .any(|child| matches!(child, Element::Token(prefix) if prefix.kind == TokenKind::Minus))
        {
            let mut lexeme = String::from("-");
            lexeme.push_str(&token.lexeme);
            lexeme
        } else {
            token.lexeme.clone()
        }
    }

    fn lower_string_expr(node: &Node) -> Option<StringExpr> {
        let token = Self::first_token(node, TokenKind::String)?;
        let value = token
            .lexeme
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(&token.lexeme)
            .replace("\\\"", "\"")
            .replace("\\n", "\n");
        Some(StringExpr {
            literal: token.lexeme.clone(),
            value,
            span: token.span,
        })
    }

    fn has_interpolation(value: &str) -> bool {
        let mut chars = value.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                let next_is_escape = chars.peek().copied() == Some('{');
                if !next_is_escape {
                    return true;
                }
                chars.next();
            }
        }
        false
    }

    fn lower_template_expr(node: &Node) -> Option<TemplateExpr> {
        let token = Self::first_token(node, TokenKind::String)?;
        let raw = token
            .lexeme
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(&token.lexeme);
        let mut segments = Vec::new();
        let mut buf = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(next) = chars.next() {
                    buf.push('\\');
                    buf.push(next);
                } else {
                    buf.push(c);
                }
            } else if c == '{' {
                if chars.peek().copied() == Some('{') {
                    chars.next();
                    buf.push('{');
                    continue;
                }
                if !buf.is_empty() {
                    let text = std::mem::take(&mut buf);
                    segments.push(TemplateSegment::Text(
                        text.replace("\\\"", "\"").replace("\\n", "\n"),
                    ));
                }
                let mut expr_text = String::new();
                let mut depth = 1usize;
                for ec in chars.by_ref() {
                    if ec == '{' {
                        depth += 1;
                    } else if ec == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    expr_text.push(ec);
                }
                let fake_source = format!("let _x = {expr_text}; _x");
                let tokens = crate::lexer::lex(&fake_source).tokens;
                let fake_root = crate::parser::parse(&tokens).root;
                let lowered = crate::ast::lower(&fake_root);
                for item in lowered.file.items {
                    if let Item::Function(f) = item
                        && let Some(body) = f.body
                        && let Some(tail) = body.tail
                    {
                        segments.push(TemplateSegment::Expr(Box::new(tail)));
                        break;
                    }
                }
            } else {
                buf.push(c);
            }
        }
        if !buf.is_empty() {
            segments.push(TemplateSegment::Text(
                buf.replace("\\\"", "\"").replace("\\n", "\n"),
            ));
        }
        Some(TemplateExpr {
            segments,
            span: token.span,
        })
    }

    fn lower_bool_expr(node: &Node) -> Option<BoolExpr> {
        let token = node.children.iter().find_map(|child| match child {
            Element::Token(token)
                if matches!(token.kind, TokenKind::KwTrue | TokenKind::KwFalse) =>
            {
                Some(token)
            }
            Element::Token(_) | Element::Node(_) => None,
        })?;
        Some(BoolExpr {
            value: token.kind == TokenKind::KwTrue,
            span: token.span,
        })
    }

    fn lower_name_expr(node: &Node) -> Option<NameExpr> {
        let token = Self::first_token(node, TokenKind::Ident)?;
        Some(NameExpr {
            name: token.lexeme.clone(),
            span: token.span,
        })
    }

    fn lower_contract_result_expr(node: &Node) -> Option<ContractResultExpr> {
        let token = Self::first_token(node, TokenKind::Ident)?;
        Some(ContractResultExpr { span: token.span })
    }

    fn lower_call_expr(&mut self, node: &Node) -> Option<CallExpr> {
        let callee_node = node.children.iter().find_map(|child| match child {
            Element::Node(child) => Some(child),
            Element::Token(_) => None,
        })?;

        let mut args = Vec::new();
        let callee = match self.lower_expr(callee_node)? {
            Expr::Field(field) => {
                let is_enum_variant = if let Expr::Name(ref name) = *field.base {
                    name.name
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase())
                } else {
                    false
                };
                if is_enum_variant {
                    field.pretty()
                } else {
                    args.push(*field.base.clone());
                    field.field
                }
            }
            Expr::Name(name) => name.name,
            Expr::Perform(perform) => perform.callee.to_string(),
            _ => return None,
        };

        if let Some(arg_node) = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::ArgList => Some(child),
            _ => None,
        }) {
            for element in &arg_node.children {
                if let Element::Node(expr) = element
                    && let Some(arg) = self.lower_expr(expr)
                {
                    args.push(arg);
                }
            }
        }

        Some(CallExpr {
            callee,
            args,
            span: node.span,
        })
    }

    fn lower_field_expr(&mut self, node: &Node) -> Option<FieldExpr> {
        let mut exprs = node.children.iter().filter_map(|child| match child {
            Element::Node(child) => self.lower_expr(child),
            Element::Token(_) => None,
        });
        let base = exprs.next()?;
        let field = node.children.iter().find_map(|child| match child {
            Element::Token(token) if token.kind == TokenKind::Ident => Some(token.lexeme.clone()),
            Element::Node(_) | Element::Token(_) => None,
        })?;

        Some(FieldExpr {
            base: Box::new(base),
            field,
            span: node.span,
        })
    }

    fn lower_array_expr(&mut self, node: &Node) -> ArrayExpr {
        let mut after_semicolon = false;
        let mut repeat_len = None;
        ArrayExpr {
            elements: node
                .children
                .iter()
                .filter_map(|child| match child {
                    Element::Node(child) if !after_semicolon => self.lower_expr(child),
                    Element::Node(_) => None,
                    Element::Token(token) => {
                        if token.kind == TokenKind::Semicolon {
                            after_semicolon = true;
                        } else if after_semicolon && repeat_len.is_none() {
                            repeat_len = match token.kind {
                                TokenKind::Integer => {
                                    token.lexeme.parse::<usize>().ok().map(ArrayLen::Literal)
                                }
                                TokenKind::Ident => Some(ArrayLen::Name(token.lexeme.clone())),
                                _ => None,
                            };
                        }
                        None
                    }
                })
                .collect(),
            repeat_len,
            span: node.span,
        }
    }

    fn lower_index_expr(&mut self, node: &Node) -> Option<IndexExpr> {
        let mut exprs = node.children.iter().filter_map(|child| match child {
            Element::Node(child) => self.lower_expr(child),
            Element::Token(_) => None,
        });
        Some(IndexExpr {
            base: Box::new(exprs.next()?),
            index: Box::new(exprs.next()?),
            span: node.span,
        })
    }

    fn lower_record_expr(&mut self, node: &Node) -> Option<RecordExpr> {
        let name = Self::first_ident(node)?;
        let fields = node
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::FieldInitList => Some(
                    child
                        .children
                        .iter()
                        .filter_map(|element| match element {
                            Element::Node(init) if init.kind == NodeKind::FieldInit => {
                                self.lower_field_init(init)
                            }
                            Element::Node(_) | Element::Token(_) => None,
                        })
                        .collect::<Vec<_>>(),
                ),
                Element::Node(_) | Element::Token(_) => None,
            })
            .unwrap_or_default();

        Some(RecordExpr {
            name,
            fields,
            span: node.span,
        })
    }

    fn lower_if_expr(&mut self, node: &Node) -> Option<IfExpr> {
        let condition = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind != NodeKind::Body => self.lower_expr(child),
            Element::Node(_) | Element::Token(_) => None,
        })?;
        let mut bodies = node.children.iter().filter_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::Body => Some(self.lower_body(child)),
            Element::Node(_) | Element::Token(_) => None,
        });
        let then_body = bodies.next()?;
        let else_body = bodies.next().unwrap_or_else(|| Body {
            statements: Vec::new(),
            tail: None,
            span: Span::empty_at(node.span.end),
        });

        Some(IfExpr {
            condition: Box::new(condition),
            then_body,
            else_body,
            span: node.span,
        })
    }

    fn lower_match_expr(&mut self, node: &Node) -> Option<MatchExpr> {
        let scrutinee = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind != NodeKind::MatchArm => self.lower_expr(child),
            _ => None,
        })?;
        let arms = node
            .children
            .iter()
            .filter_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::MatchArm => {
                    self.lower_match_arm(child)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        Some(MatchExpr {
            scrutinee: Box::new(scrutinee),
            arms,
            span: node.span,
        })
    }

    fn lower_match_arm(&mut self, node: &Node) -> Option<MatchArm> {
        let pattern = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::MatchPattern => {
                self.lower_match_pattern(child)
            }
            _ => None,
        })?;
        let body = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::Body => Some(self.lower_body(child)),
            _ => None,
        })?;
        Some(MatchArm {
            pattern,
            body,
            span: node.span,
        })
    }

    fn lower_match_pattern(&mut self, node: &Node) -> Option<MatchPattern> {
        let nested_patterns = node
            .children
            .iter()
            .filter_map(|child| match child {
                Element::Node(child) if child.kind == NodeKind::MatchPattern => {
                    self.lower_match_pattern(child)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if !nested_patterns.is_empty() {
            return Some(if nested_patterns.len() == 1 {
                nested_patterns.into_iter().next()?
            } else {
                MatchPattern::Or {
                    patterns: nested_patterns,
                    span: node.span,
                }
            });
        }

        if let Some(path) = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::TypePath => self.lower_type_path(child),
            _ => None,
        }) {
            let binding = node.children.iter().find_map(|entry| match entry {
                Element::Token(token) if token.kind == TokenKind::Ident => {
                    Some(token.lexeme.clone())
                }
                _ => None,
            });
            return Some(MatchPattern::Variant {
                path,
                binding,
                span: node.span,
            });
        }

        let tokens = node
            .children
            .iter()
            .filter_map(|child| match child {
                Element::Token(token) => Some(token),
                Element::Node(_) => None,
            })
            .collect::<Vec<_>>();
        if tokens.len() == 3
            && tokens[0].kind == TokenKind::Integer
            && tokens[1].kind == TokenKind::DotDot
            && tokens[2].kind == TokenKind::Integer
        {
            return Some(MatchPattern::IntegerRange {
                start: tokens[0].lexeme.parse::<i64>().ok()?,
                end: tokens[2].lexeme.parse::<i64>().ok()?,
                span: node.span,
            });
        }

        let token = *tokens.first()?;
        match token.kind {
            TokenKind::Integer => Some(MatchPattern::Integer {
                value: token.lexeme.parse::<i64>().ok()?,
                span: token.span,
            }),
            TokenKind::String => Some(MatchPattern::String {
                literal: token.lexeme.clone(),
                value: token
                    .lexeme
                    .strip_prefix('"')
                    .and_then(|value| value.strip_suffix('"'))
                    .unwrap_or(&token.lexeme)
                    .replace("\\\"", "\"")
                    .replace("\\n", "\n"),
                span: token.span,
            }),
            TokenKind::KwTrue | TokenKind::KwFalse => Some(MatchPattern::Bool {
                value: token.kind == TokenKind::KwTrue,
                span: token.span,
            }),
            TokenKind::Underscore => Some(MatchPattern::Wildcard { span: token.span }),
            _ => None,
        }
    }

    fn lower_repeat_expr(&mut self, node: &Node) -> Option<RepeatExpr> {
        let binding = node.children.iter().find_map(|child| {
            match child {
            Element::Token(token)
                if token.kind == TokenKind::Ident
                    && node.children.iter().any(|entry| {
                        matches!(entry, Element::Token(token) if token.kind == TokenKind::KwIn)
                    }) =>
            {
                Some(token.lexeme.clone())
            }
            _ => None,
        }
        });
        let count = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind != NodeKind::Body => self.lower_expr(child),
            Element::Node(_) | Element::Token(_) => None,
        })?;
        let body = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::Body => Some(self.lower_body(child)),
            Element::Node(_) | Element::Token(_) => None,
        })?;
        Some(RepeatExpr {
            binding,
            count: Box::new(count),
            body,
            span: node.span,
        })
    }

    fn lower_for_expr(&mut self, node: &Node) -> Expr {
        let binding = node
            .children
            .iter()
            .find_map(|child| match child {
                Element::Token(token) if token.kind == TokenKind::Ident => {
                    Some(token.lexeme.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| "_i".to_owned());
        let mut expr_nodes = node.children.iter().filter_map(|child| match child {
            Element::Node(child) if child.kind != NodeKind::Body => self.lower_expr(child),
            Element::Node(_) | Element::Token(_) => None,
        });
        let start = expr_nodes.next();
        let end = expr_nodes.next();
        let body = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::Body => Some(self.lower_body(child)),
            Element::Node(_) | Element::Token(_) => None,
        });
        match (start, end, body) {
            (Some(start), Some(end), Some(body)) => {
                let span = node.span;
                let condition = Expr::Binary(BinaryExpr {
                    left: Box::new(start.clone()),
                    op: BinaryOp::Lt,
                    right: Box::new(end.clone()),
                    span,
                });
                let ascending = self.desugar_for_direction(
                    binding.clone(),
                    body.clone(),
                    end.clone(),
                    start.clone(),
                    start.clone(),
                    BinaryOp::Sub,
                    BinaryOp::Add,
                    span,
                );
                let descending = self.desugar_for_direction(
                    binding,
                    body,
                    start.clone(),
                    end.clone(),
                    start.clone(),
                    BinaryOp::Sub,
                    BinaryOp::Sub,
                    span,
                );
                Expr::If(Box::new(IfExpr {
                    condition: Box::new(condition),
                    then_body: Body {
                        statements: Vec::new(),
                        tail: Some(ascending),
                        span,
                    },
                    else_body: Body {
                        statements: Vec::new(),
                        tail: Some(descending),
                        span,
                    },
                    span,
                }))
            }
            _ => Expr::Repeat(Box::new(RepeatExpr {
                binding: None,
                count: Box::new(Expr::Integer(IntegerExpr {
                    value: 0,
                    span: node.span,
                })),
                body: Body {
                    statements: Vec::new(),
                    tail: None,
                    span: node.span,
                },
                span: node.span,
            })),
        }
    }

    fn desugar_for_direction(
        &self,
        binding: String,
        body: Body,
        count_left: Expr,
        count_right: Expr,
        value_base: Expr,
        count_op: BinaryOp,
        value_op: BinaryOp,
        span: Span,
    ) -> Expr {
        let count = Expr::Binary(BinaryExpr {
            left: Box::new(count_left),
            op: count_op,
            right: Box::new(count_right),
            span,
        });
        let inner_binding = format!("__{binding}_idx");
        let index_expr = Expr::Name(NameExpr {
            name: inner_binding.clone(),
            span,
        });
        let loop_value = Expr::Binary(BinaryExpr {
            left: Box::new(value_base),
            op: value_op,
            right: Box::new(index_expr),
            span,
        });
        let let_binding = Stmt::Let(LetBinding {
            mutable: false,
            name: binding,
            value: loop_value,
            span,
        });
        let desugared_body = Body {
            statements: std::iter::once(let_binding)
                .chain(body.statements)
                .collect(),
            tail: body.tail,
            span: body.span,
        };
        Expr::Repeat(Box::new(RepeatExpr {
            binding: Some(inner_binding),
            count: Box::new(count),
            body: desugared_body,
            span,
        }))
    }

    fn lower_while_expr(&mut self, node: &Node) -> Option<WhileExpr> {
        let condition = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind != NodeKind::Body => self.lower_expr(child),
            Element::Node(_) | Element::Token(_) => None,
        })?;
        let body = node.children.iter().find_map(|child| match child {
            Element::Node(child) if child.kind == NodeKind::Body => Some(self.lower_body(child)),
            Element::Node(_) | Element::Token(_) => None,
        })?;
        Some(WhileExpr {
            condition: Box::new(condition),
            body,
            span: node.span,
        })
    }

    fn lower_field_init(&mut self, node: &Node) -> Option<FieldInit> {
        let name = Self::first_ident(node)?;
        let value = node
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(expr) => self.lower_expr(expr),
                Element::Token(_) => None,
            })
            .unwrap_or_else(|| {
                Expr::Name(NameExpr {
                    name: name.clone(),
                    span: node.span,
                })
            });

        Some(FieldInit {
            name,
            value,
            span: node.span,
        })
    }

    fn lower_binary_expr(&mut self, node: &Node) -> Option<BinaryExpr> {
        let mut expr_children = node.children.iter().filter_map(|child| match child {
            Element::Node(expr) => self.lower_expr(expr),
            Element::Token(_) => None,
        });
        let left = expr_children.next()?;
        let right = expr_children.next()?;
        let op = node.children.iter().find_map(|child| match child {
            Element::Token(token) => match token.kind {
                TokenKind::KwAnd => Some(BinaryOp::And),
                TokenKind::KwOr => Some(BinaryOp::Or),
                TokenKind::Pipe => Some(BinaryOp::BitOr),
                TokenKind::Caret => Some(BinaryOp::BitXor),
                TokenKind::Amp => Some(BinaryOp::BitAnd),
                TokenKind::EqEq => Some(BinaryOp::Eq),
                TokenKind::NotEq => Some(BinaryOp::Ne),
                TokenKind::Lt => Some(BinaryOp::Lt),
                TokenKind::Le => Some(BinaryOp::Le),
                TokenKind::Gt => Some(BinaryOp::Gt),
                TokenKind::Ge => Some(BinaryOp::Ge),
                TokenKind::Shl => Some(BinaryOp::Shl),
                TokenKind::Shr => Some(BinaryOp::Shr),
                TokenKind::Plus => Some(BinaryOp::Add),
                TokenKind::Minus => Some(BinaryOp::Sub),
                TokenKind::Star => Some(BinaryOp::Mul),
                TokenKind::Slash => Some(BinaryOp::Div),
                TokenKind::Percent => Some(BinaryOp::Rem),
                _ => None,
            },
            Element::Node(_) => None,
        })?;

        Some(BinaryExpr {
            op,
            left: Box::new(left),
            right: Box::new(right),
            span: node.span,
        })
    }

    fn lower_unary_expr(&mut self, node: &Node) -> Option<UnaryExpr> {
        let inner = node.children.iter().find_map(|child| match child {
            Element::Node(expr) => self.lower_expr(expr),
            Element::Token(_) => None,
        })?;
        let op = node.children.iter().find_map(|child| match child {
            Element::Token(token) => match token.kind {
                TokenKind::KwNot => Some(UnaryOp::Not),
                _ => None,
            },
            Element::Node(_) => None,
        })?;

        Some(UnaryExpr {
            op,
            inner: Box::new(inner),
            span: node.span,
        })
    }

    fn lower_group_expr(&mut self, node: &Node) -> Option<GroupExpr> {
        let inner = node.children.iter().find_map(|child| match child {
            Element::Node(expr) => self.lower_expr(expr),
            Element::Token(_) => None,
        })?;

        Some(GroupExpr {
            inner: Box::new(inner),
            span: node.span,
        })
    }

    fn ident_after(&mut self, node: &Node, keyword: TokenKind) -> Option<String> {
        let mut saw_keyword = false;

        for child in &node.children {
            let Element::Token(token) = child else {
                continue;
            };

            if token.kind == keyword {
                saw_keyword = true;
            } else if saw_keyword && token.kind == TokenKind::Ident {
                return Some(token.lexeme.clone());
            }
        }

        self.diagnostics.push(Diagnostic::new(
            "ast.missing-name",
            "missing identifier while lowering item",
            node.span,
            Some("Provide an ASCII identifier immediately after the item keyword.".to_owned()),
        ));
        None
    }

    fn first_ident(node: &Node) -> Option<String> {
        Self::first_token(node, TokenKind::Ident).map(|token| token.lexeme.clone())
    }

    fn first_type_path(&mut self, node: &Node) -> Option<TypePath> {
        node.children.iter().find_map(|child| match child {
            Element::Node(child)
                if matches!(child.kind, NodeKind::TypePath | NodeKind::TypeArray) =>
            {
                self.lower_type_path(child)
            }
            _ => None,
        })
    }

    fn first_token(node: &Node, kind: TokenKind) -> Option<&crate::Token> {
        node.children.iter().find_map(|child| match child {
            Element::Token(token) if token.kind == kind => Some(token),
            Element::Node(_) | Element::Token(_) => None,
        })
    }
}

const fn compound_assign_op(kind: TokenKind) -> Option<BinaryOp> {
    match kind {
        TokenKind::PlusEq => Some(BinaryOp::Add),
        TokenKind::MinusEq => Some(BinaryOp::Sub),
        TokenKind::StarEq => Some(BinaryOp::Mul),
        TokenKind::SlashEq => Some(BinaryOp::Div),
        TokenKind::PercentEq => Some(BinaryOp::Rem),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::ast::{ArrayLen, BinaryOp, Expr, Item, MatchPattern, Stmt, UnaryOp, lower};
    use crate::lexer::lex;
    use crate::parser::parse;

    #[test]
    fn lowers_function_bodies_into_ast_expressions() {
        let lexed = lex("fn main() -> I32 requires true ensures result == 3 { add(1, 2) + 3 }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let body = function.body.as_ref().expect("body expression");
        let Expr::Binary(body) = body.tail.as_ref().expect("tail expression") else {
            panic!("expected binary expression");
        };
        assert_eq!(body.op, BinaryOp::Add);
        assert!(function.requires.is_some());
        assert!(function.ensures.is_some());
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_float_literal_expressions() {
        let lexed = lex("fn main() -> F64 { 3.5 }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let Expr::Float(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected float expression");
        };
        assert!((expr.value - 3.5).abs() < f64::EPSILON);
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_not_expressions() {
        let lexed = lex("fn main() -> Bool { not false and true }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let Expr::Binary(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected binary expression");
        };
        let Expr::Unary(left) = expr.left.as_ref() else {
            panic!("expected unary expression");
        };
        assert_eq!(left.op, UnaryOp::Not);
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_not_call_expressions() {
        let lexed = lex("fn flag() -> Bool { false }\nfn main() -> Bool { not flag() }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.last().expect("function item") else {
            panic!("expected function");
        };

        let Expr::Unary(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected unary expression");
        };
        assert_eq!(expr.op, UnaryOp::Not);
        assert!(matches!(expr.inner.as_ref(), Expr::Call(_)));
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_if_expressions_with_branch_bodies() {
        let lexed = lex("fn main() -> I32 { if true { let left = 1; left } else { 2 } }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let Expr::If(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected if expression");
        };
        assert_eq!(expr.then_body.statements.len(), 1);
        assert!(expr.else_body.tail.is_some());
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_if_expressions_with_empty_else_bodies() {
        let lexed = lex(
            "fn main(flag: Bool, text: Text) -> Text { if flag { let sink = text; } else { }; text }",
        );
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let body = function.body.as_ref().expect("body");
        assert_eq!(body.statements.len(), 1);
        let Stmt::Expr(stmt) = &body.statements[0] else {
            panic!("expected expression statement");
        };
        let Expr::If(expr) = &stmt.expr else {
            panic!("expected if expression");
        };
        assert_eq!(expr.then_body.statements.len(), 1);
        assert!(expr.then_body.tail.is_none());
        assert!(expr.else_body.statements.is_empty());
        assert!(expr.else_body.tail.is_none());
        assert!(matches!(body.tail, Some(Expr::Name(_))));
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_if_expressions_without_else_bodies_as_unit() {
        let lexed =
            lex("fn main(flag: Bool, text: Text) -> Text { if flag { let sink = text; }; text }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let body = function.body.as_ref().expect("body");
        assert_eq!(body.statements.len(), 1);
        let Stmt::Expr(stmt) = &body.statements[0] else {
            panic!("expected expression statement");
        };
        let Expr::If(expr) = &stmt.expr else {
            panic!("expected if expression");
        };
        assert_eq!(expr.then_body.statements.len(), 1);
        assert!(expr.else_body.statements.is_empty());
        assert!(expr.else_body.tail.is_none());
        assert!(matches!(body.tail, Some(Expr::Name(_))));
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_repeat_expressions_with_bodies() {
        let lexed = lex("fn main() { repeat 3 { let value = 1; value } }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let Expr::Repeat(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected repeat expression");
        };
        assert!(matches!(*expr.count, Expr::Integer(_)));
        assert_eq!(expr.body.statements.len(), 1);
        assert!(expr.body.tail.is_some());
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_indexed_repeat_expressions_with_bindings() {
        let lexed = lex("fn main() { repeat i in 3 { i } }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let Expr::Repeat(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected repeat expression");
        };

        assert_eq!(expr.binding.as_deref(), Some("i"));
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_record_field_punning_to_name_exprs() {
        let lexed = lex(
            "struct Pair { left: I32, right: I32 }\nfn main() -> Pair { let left = 7; let right = 9; Pair { left, right } }",
        );
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.last().expect("function item") else {
            panic!("expected function");
        };

        let Expr::Record(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected record expression");
        };

        assert!(matches!(expr.fields[0].value, Expr::Name(_)));
        assert!(matches!(expr.fields[1].value, Expr::Name(_)));
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_while_expressions_with_bodies() {
        let lexed = lex("fn main() { while true { let value = 1; value; } }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let Expr::While(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected while expression");
        };
        assert!(matches!(*expr.condition, Expr::Bool(_)));
        assert_eq!(expr.body.statements.len(), 2);
        assert!(expr.body.tail.is_none());
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_enum_and_match_expressions() {
        let lexed = lex(
            "enum Color { red, green }\nfn main() -> I32 { match Color.red { Color.red => { 1 }, Color.green => { 2 }, } }",
        );
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);

        assert!(matches!(ast.file.items[0], Item::Enum(_)));
        let Item::Function(function) = ast.file.items.last().expect("function item") else {
            panic!("expected function");
        };
        let Expr::Match(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected match expression");
        };
        assert_eq!(expr.arms.len(), 2);
        assert_eq!(expr.arms[0].pattern.pretty(), "Color.red");
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_scalar_and_wildcard_match_patterns() {
        let lexed =
            lex("fn main() -> I32 { match 41 { 40 => { 0 }, true => { 1 }, _ => { 2 }, } }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let Expr::Match(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected match expression");
        };
        assert_eq!(expr.arms[0].pattern.pretty(), "40");
        assert_eq!(expr.arms[1].pattern.pretty(), "true");
        assert_eq!(expr.arms[2].pattern.pretty(), "_");
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_or_and_range_match_patterns() {
        let lexed = lex(
            "fn main(byte: I32) -> I32 { match byte { 65 | 97 => { 1 }, 100..200 => { 2 }, _ => { 3 }, } }",
        );
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let Expr::Match(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected match expression");
        };
        assert_eq!(expr.arms[0].pattern.pretty(), "65 | 97");
        assert_eq!(expr.arms[1].pattern.pretty(), "100..200");
        assert!(matches!(expr.arms[0].pattern, MatchPattern::Or { .. }));
        assert!(matches!(
            expr.arms[1].pattern,
            MatchPattern::IntegerRange {
                start: 100,
                end: 200,
                ..
            }
        ));
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_else_if_expression_chains() {
        let lexed = lex("fn main() -> I32 { if false { 0 } else if true { 42 } else { 7 } }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);
        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };

        let Expr::If(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected if expression");
        };
        let Expr::If(nested) = expr.else_body.tail.as_ref().expect("nested else-if tail") else {
            panic!("expected nested if expression");
        };
        assert!(matches!(nested.then_body.tail, Some(Expr::Integer(_))));
        assert!(matches!(nested.else_body.tail, Some(Expr::Integer(_))));
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_array_types_in_declarations() {
        let lexed =
            lex("struct Grid { rows: [[I32; 2]; 2], }\nfn sum(xs: [I32; 2]) -> [I32; 2] { xs }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);

        let Item::Struct(struct_item) = ast.file.items.first().expect("struct item") else {
            panic!("expected struct");
        };
        assert_eq!(struct_item.fields[0].ty.to_string(), "[[I32; 2]; 2]");

        let Item::Function(function) = ast.file.items.last().expect("function item") else {
            panic!("expected function");
        };
        assert_eq!(function.params[0].ty.to_string(), "[I32; 2]");
        assert_eq!(
            function
                .return_type
                .as_ref()
                .expect("return type")
                .to_string(),
            "[I32; 2]"
        );
        assert!(ast.diagnostics.is_empty());
    }

    #[test]
    fn lowers_repeat_array_literals() {
        let lexed = lex("fn main[N]() -> [I32; N] { [42; N] }");
        let parsed = parse(&lexed.tokens);
        let ast = lower(&parsed.root);

        let Item::Function(function) = ast.file.items.first().expect("function item") else {
            panic!("expected function");
        };
        let Expr::Array(expr) = function
            .body
            .as_ref()
            .and_then(|body| body.tail.as_ref())
            .expect("tail expression")
        else {
            panic!("expected array expression");
        };
        assert_eq!(expr.elements.len(), 1);
        assert!(matches!(expr.repeat_len, Some(ArrayLen::Name(ref name)) if name == "N"));
        assert!(ast.diagnostics.is_empty());
    }
}
