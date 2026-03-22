use std::fmt::Write;

use sarif_syntax::ast;
use sarif_syntax::{Diagnostic, Span};

#[derive(Clone, Debug, Default)]
pub struct HirLowering {
    pub module: Module,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Default)]
pub struct Module {
    pub items: Vec<Item>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum Item {
    Const(Const),
    Function(Function),
    Enum(Enum),
    Struct(Struct),
    Effect(EffectItem),
}

#[derive(Clone, Debug)]
pub struct EffectItem {
    pub name: String,
    pub methods: Vec<EffectMethod>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EffectMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Const {
    pub name: String,
    pub ty: TypeRef,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub type_params: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub effects: Vec<EffectRef>,
    pub requires: Option<Expr>,
    pub ensures: Option<Expr>,
    pub body: Option<Body>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct GenericParam {
    pub name: String,
    pub kind: Option<String>,
    pub span: Span,
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
pub enum Stmt {
    Let(LetBinding),
    Assign(AssignStmt),
    Expr(ExprStmt),
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
        }
    }
}

#[derive(Clone, Debug)]
pub struct LetBinding {
    pub mutable: bool,
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct AssignStmt {
    pub target: Expr,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ExprStmt {
    pub expr: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Struct {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Enum {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EnumVariant {
    pub name: String,
    pub payload: Option<TypeRef>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub ty: TypeRef,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeRef {
    pub path: String,
    pub span: Span,
}

impl MatchPattern {
    #[must_use]
    pub fn pretty(&self) -> String {
        match self {
            Self::Variant {
                path,
                binding: None,
                ..
            } => path.path.clone(),
            Self::Variant {
                path,
                binding: Some(binding),
                ..
            } => format!("{}({binding})", path.path),
            Self::Integer { value, .. } => value.to_string(),
            Self::String { literal, .. } => literal.clone(),
            Self::Bool { value, .. } => value.to_string(),
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
            | Self::Wildcard { span } => *span,
        }
    }
}

#[derive(Clone, Debug)]
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
    While(Box<WhileExpr>),
    Record(RecordExpr),
    Unary(UnaryExpr),
    Binary(BinaryExpr),
    Group(GroupExpr),
    Comptime(Box<Body>),
    Handle(Box<HandleExpr>),
}

#[derive(Clone, Debug)]
pub struct HandleExpr {
    pub body: Body,
    pub arms: Vec<HandleArm>,
}

#[derive(Clone, Debug)]
pub struct HandleArm {
    pub name: String,
    pub params: Vec<String>,
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
            Self::While(expr) => expr.span,
            Self::Record(expr) => expr.span,
            Self::Unary(expr) => expr.span,
            Self::Binary(expr) => expr.span,
            Self::Group(expr) => expr.span,
            Self::Comptime(body) => body.span,
            Self::Handle(expr) => expr.body.span,
        }
    }

    #[must_use]
    pub fn pretty(&self) -> String {
        match self {
            Self::Integer(expr) => expr.value.to_string(),
            Self::Float(expr) => expr.value.to_string(),
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
            Self::Comptime(body) => format!("comptime {}", body.pretty()),
            Self::Handle(expr) => format!(
                "handle {} {{ {} }}",
                expr.body.pretty(),
                expr.arms
                    .iter()
                    .map(|arm| format!(
                        "{}({}) => {{ {} }}",
                        arm.name,
                        arm.params.join(", "),
                        arm.body.pretty()
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub struct IntegerExpr {
    pub value: i64,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FloatExpr {
    pub value: f64,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StringExpr {
    pub literal: String,
    pub value: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct BoolExpr {
    pub value: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct NameExpr {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ContractResultExpr {
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct CallExpr {
    pub callee: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ArrayExpr {
    pub elements: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FieldExpr {
    pub base: Box<Expr>,
    pub field: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct IndexExpr {
    pub base: Box<Expr>,
    pub index: Box<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct IfExpr {
    pub condition: Box<Expr>,
    pub then_body: Body,
    pub else_body: Body,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct MatchExpr {
    pub scrutinee: Box<Expr>,
    pub arms: Vec<MatchArm>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body: Body,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum MatchPattern {
    Variant {
        path: TypeRef,
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
    Wildcard {
        span: Span,
    },
}

#[derive(Clone, Debug)]
pub struct RepeatExpr {
    pub binding: Option<String>,
    pub count: Box<Expr>,
    pub body: Body,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct WhileExpr {
    pub condition: Box<Expr>,
    pub body: Body,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct RecordExpr {
    pub name: String,
    pub fields: Vec<FieldInit>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FieldInit {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub inner: Box<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct BinaryExpr {
    pub op: BinaryOp,
    pub left: Box<Expr>,
    pub right: Box<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct GroupExpr {
    pub inner: Box<Expr>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
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
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    Io,
    Alloc,
    Async,
    Parallel,
    Clock,
    Ffi,
    Nondet,
    User(String),
}

impl Effect {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "io" => Some(Self::Io),
            "alloc" => Some(Self::Alloc),
            "async" => Some(Self::Async),
            "parallel" => Some(Self::Parallel),
            "clock" => Some(Self::Clock),
            "ffi" => Some(Self::Ffi),
            "nondet" => Some(Self::Nondet),
            _ => Some(Self::User(name.to_owned())),
        }
    }

    #[must_use]
    pub fn keyword(&self) -> &str {
        match self {
            Self::Io => "io",
            Self::Alloc => "alloc",
            Self::Async => "async",
            Self::Parallel => "parallel",
            Self::Clock => "clock",
            Self::Ffi => "ffi",
            Self::Nondet => "nondet",
            Self::User(name) => name,
        }
    }

    const fn rank(&self) -> usize {
        match self {
            Self::Io => 0,
            Self::Alloc => 1,
            Self::Async => 2,
            Self::Parallel => 3,
            Self::Clock => 4,
            Self::Ffi => 5,
            Self::Nondet => 6,
            Self::User(_) => 7,
        }
    }
}

#[derive(Clone, Debug)]
pub enum EffectRef {
    Builtin { effect: Effect, span: Span },
    Unknown { name: String, span: Span },
}

impl EffectRef {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Builtin { effect, .. } => effect.keyword(),
            Self::Unknown { name, .. } => name,
        }
    }

    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Builtin { span, .. } | Self::Unknown { span, .. } => *span,
        }
    }
}

#[must_use]
pub fn lower(file: &ast::AstFile) -> HirLowering {
    let items = file
        .items
        .iter()
        .map(|item| match item {
            ast::Item::Const(item) => Item::Const(lower_const(item)),
            ast::Item::Function(item) => Item::Function(lower_function(item)),
            ast::Item::Enum(item) => Item::Enum(lower_enum(item)),
            ast::Item::Struct(item) => Item::Struct(lower_struct(item)),
            ast::Item::Effect(item) => Item::Effect(lower_effect(item)),
        })
        .collect();

    HirLowering {
        module: Module { items },
        diagnostics: Vec::new(),
    }
}

impl Module {
    #[must_use]
    pub fn pretty(&self) -> String {
        let mut output = String::new();
        writeln!(&mut output, "HIR").expect("writing to a string cannot fail");

        for item in &self.items {
            match item {
                Item::Const(const_item) => {
                    writeln!(
                        &mut output,
                        "  const {}: {} = {}",
                        const_item.name,
                        const_item.ty.path,
                        const_item.value.pretty()
                    )
                    .expect("writing to a string cannot fail");
                }
                Item::Function(function) => {
                    writeln!(
                        &mut output,
                        "  fn {}({}){} effects [{}]{}{} = {}",
                        function.name,
                        function
                            .params
                            .iter()
                            .map(|param| format!("{}: {}", param.name, param.ty.path))
                            .collect::<Vec<_>>()
                            .join(", "),
                        function
                            .return_type
                            .as_ref()
                            .map_or_else(String::new, |ty| format!(" -> {}", ty.path)),
                        function
                            .effects
                            .iter()
                            .map(EffectRef::name)
                            .collect::<Vec<_>>()
                            .join(", "),
                        function
                            .requires
                            .as_ref()
                            .map_or_else(String::new, |expr| format!(
                                " requires {}",
                                expr.pretty()
                            )),
                        function
                            .ensures
                            .as_ref()
                            .map_or_else(String::new, |expr| format!(" ensures {}", expr.pretty())),
                        function
                            .body
                            .as_ref()
                            .map_or_else(|| "{}".to_owned(), Body::pretty),
                    )
                    .expect("writing to a string cannot fail");
                }
                Item::Enum(enum_item) => {
                    writeln!(&mut output, "  enum {}", enum_item.name)
                        .expect("writing to a string cannot fail");
                }
                Item::Struct(struct_item) => {
                    writeln!(&mut output, "  struct {}", struct_item.name)
                        .expect("writing to a string cannot fail");
                }
                Item::Effect(effect) => {
                    writeln!(&mut output, "  effect {}", effect.name)
                        .expect("writing to a string cannot fail");
                }
            }
        }

        output
    }
}

fn lower_const(item: &ast::Const) -> Const {
    Const {
        name: item.name.clone(),
        ty: lower_type(&item.ty, item.span),
        value: lower_expr(&item.value),
        span: item.span,
    }
}

fn lower_function(item: &ast::Function) -> Function {
    let mut effects: Vec<_> = item
        .effects
        .iter()
        .map(|effect| {
            Effect::parse(&effect.name).map_or_else(
                || EffectRef::Unknown {
                    name: effect.name.clone(),
                    span: effect.span,
                },
                |builtin| EffectRef::Builtin {
                    effect: builtin,
                    span: effect.span,
                },
            )
        })
        .collect();
    effects.sort_by_key(|effect| match effect {
        EffectRef::Builtin { effect, .. } => (0usize, effect.rank(), effect.keyword().to_owned()),
        EffectRef::Unknown { name, .. } => (1usize, usize::MAX, name.clone()),
    });

    Function {
        name: item.name.clone(),
        type_params: item
            .type_params
            .iter()
            .map(|param| GenericParam {
                name: param.name.clone(),
                kind: param.kind.clone(),
                span: param.span,
            })
            .collect(),
        params: item
            .params
            .iter()
            .map(|param| Param {
                name: param.name.clone(),
                ty: lower_type(&param.ty, param.span),
                span: param.span,
            })
            .collect(),
        return_type: item
            .return_type
            .as_ref()
            .map(|return_type| lower_type(return_type, item.span)),
        effects,
        requires: item.requires.as_ref().map(lower_expr),
        ensures: item.ensures.as_ref().map(lower_expr),
        body: item.body.as_ref().map(lower_body),
        span: item.span,
    }
}

fn lower_effect(item: &ast::Effect) -> EffectItem {
    EffectItem {
        name: item.name.clone(),
        methods: item
            .methods
            .iter()
            .map(|method| EffectMethod {
                name: method.name.clone(),
                params: method
                    .params
                    .iter()
                    .map(|param| Param {
                        name: param.name.clone(),
                        ty: lower_type(&param.ty, param.span),
                        span: param.span,
                    })
                    .collect(),
                return_type: method
                    .return_type
                    .as_ref()
                    .map(|ty| lower_type(ty, method.span)),
                span: method.span,
            })
            .collect(),
        span: item.span,
    }
}

fn lower_struct(item: &ast::Struct) -> Struct {
    Struct {
        name: item.name.clone(),
        fields: item
            .fields
            .iter()
            .map(|field| Field {
                name: field.name.clone(),
                ty: lower_type(&field.ty, field.span),
                span: field.span,
            })
            .collect(),
        span: item.span,
    }
}

fn lower_enum(item: &ast::Enum) -> Enum {
    Enum {
        name: item.name.clone(),
        variants: item
            .variants
            .iter()
            .map(|variant| EnumVariant {
                name: variant.name.clone(),
                payload: variant
                    .payload
                    .as_ref()
                    .map(|payload| lower_type(payload, variant.span)),
                span: variant.span,
            })
            .collect(),
        span: item.span,
    }
}

fn lower_type(ty: &ast::TypePath, span: Span) -> TypeRef {
    TypeRef {
        path: ty.to_string(),
        span,
    }
}

fn lower_expr(expr: &ast::Expr) -> Expr {
    match expr {
        ast::Expr::Integer(expr) => Expr::Integer(IntegerExpr {
            value: expr.value,
            span: expr.span,
        }),
        ast::Expr::Float(expr) => Expr::Float(FloatExpr {
            value: expr.value,
            span: expr.span,
        }),
        ast::Expr::String(expr) => Expr::String(StringExpr {
            literal: expr.literal.clone(),
            value: expr.value.clone(),
            span: expr.span,
        }),
        ast::Expr::Bool(expr) => Expr::Bool(BoolExpr {
            value: expr.value,
            span: expr.span,
        }),
        ast::Expr::Name(expr) => Expr::Name(NameExpr {
            name: expr.name.clone(),
            span: expr.span,
        }),
        ast::Expr::ContractResult(expr) => {
            Expr::ContractResult(ContractResultExpr { span: expr.span })
        }
        ast::Expr::Call(expr) => Expr::Call(CallExpr {
            callee: expr.callee.clone(),
            args: expr.args.iter().map(lower_expr).collect(),
            span: expr.span,
        }),
        ast::Expr::Array(expr) => Expr::Array(ArrayExpr {
            elements: expr.elements.iter().map(lower_expr).collect(),
            span: expr.span,
        }),
        ast::Expr::Field(expr) => Expr::Field(FieldExpr {
            base: Box::new(lower_expr(&expr.base)),
            field: expr.field.clone(),
            span: expr.span,
        }),
        ast::Expr::Index(expr) => Expr::Index(IndexExpr {
            base: Box::new(lower_expr(&expr.base)),
            index: Box::new(lower_expr(&expr.index)),
            span: expr.span,
        }),
        ast::Expr::If(expr) => Expr::If(Box::new(lower_if_expr(expr))),
        ast::Expr::Match(expr) => Expr::Match(Box::new(lower_match_expr(expr))),
        ast::Expr::Repeat(expr) => Expr::Repeat(Box::new(RepeatExpr {
            binding: expr.binding.clone(),
            count: Box::new(lower_expr(&expr.count)),
            body: lower_body(&expr.body),
            span: expr.span,
        })),
        ast::Expr::While(expr) => Expr::While(Box::new(WhileExpr {
            condition: Box::new(lower_expr(&expr.condition)),
            body: lower_body(&expr.body),
            span: expr.span,
        })),
        ast::Expr::Record(expr) => Expr::Record(lower_record_expr(expr)),
        ast::Expr::Unary(expr) => Expr::Unary(lower_unary_expr(expr)),
        ast::Expr::Binary(expr) => Expr::Binary(BinaryExpr {
            op: lower_binary_op(expr.op),
            left: Box::new(lower_expr(&expr.left)),
            right: Box::new(lower_expr(&expr.right)),
            span: expr.span,
        }),
        ast::Expr::Group(expr) => Expr::Group(GroupExpr {
            inner: Box::new(lower_expr(&expr.inner)),
            span: expr.span,
        }),
        ast::Expr::Comptime(expr) => Expr::Comptime(Box::new(lower_body(&expr.body))),
        ast::Expr::Handle(expr) => Expr::Handle(Box::new(lower_handle_expr(expr))),
    }
}

fn lower_handle_expr(expr: &ast::HandleExpr) -> HandleExpr {
    HandleExpr {
        body: lower_body(&expr.body),
        arms: expr
            .arms
            .iter()
            .map(|arm| HandleArm {
                name: arm.name.clone(),
                params: arm.params.clone(),
                body: lower_body(&arm.body),
                span: arm.span,
            })
            .collect(),
    }
}

fn lower_if_expr(expr: &ast::IfExpr) -> IfExpr {
    IfExpr {
        condition: Box::new(lower_expr(&expr.condition)),
        then_body: lower_body(&expr.then_body),
        else_body: lower_body(&expr.else_body),
        span: expr.span,
    }
}

fn lower_match_expr(expr: &ast::MatchExpr) -> MatchExpr {
    MatchExpr {
        scrutinee: Box::new(lower_expr(&expr.scrutinee)),
        arms: expr
            .arms
            .iter()
            .map(|arm| MatchArm {
                pattern: lower_match_pattern(&arm.pattern),
                body: lower_body(&arm.body),
                span: arm.span,
            })
            .collect(),
        span: expr.span,
    }
}

fn lower_match_pattern(pattern: &ast::MatchPattern) -> MatchPattern {
    match pattern {
        ast::MatchPattern::Variant {
            path,
            binding,
            span,
        } => MatchPattern::Variant {
            path: lower_type(path, *span),
            binding: binding.clone(),
            span: *span,
        },
        ast::MatchPattern::Integer { value, span } => MatchPattern::Integer {
            value: *value,
            span: *span,
        },
        ast::MatchPattern::String {
            literal,
            value,
            span,
        } => MatchPattern::String {
            literal: literal.clone(),
            value: value.clone(),
            span: *span,
        },
        ast::MatchPattern::Bool { value, span } => MatchPattern::Bool {
            value: *value,
            span: *span,
        },
        ast::MatchPattern::Wildcard { span } => MatchPattern::Wildcard { span: *span },
    }
}

fn lower_record_expr(expr: &ast::RecordExpr) -> RecordExpr {
    RecordExpr {
        name: expr.name.clone(),
        fields: expr
            .fields
            .iter()
            .map(|field| FieldInit {
                name: field.name.clone(),
                value: lower_expr(&field.value),
                span: field.span,
            })
            .collect(),
        span: expr.span,
    }
}

fn lower_unary_expr(expr: &ast::UnaryExpr) -> UnaryExpr {
    UnaryExpr {
        op: lower_unary_op(expr.op),
        inner: Box::new(lower_expr(&expr.inner)),
        span: expr.span,
    }
}

const fn lower_binary_op(op: ast::BinaryOp) -> BinaryOp {
    match op {
        ast::BinaryOp::And => BinaryOp::And,
        ast::BinaryOp::Or => BinaryOp::Or,
        ast::BinaryOp::Eq => BinaryOp::Eq,
        ast::BinaryOp::Ne => BinaryOp::Ne,
        ast::BinaryOp::Lt => BinaryOp::Lt,
        ast::BinaryOp::Le => BinaryOp::Le,
        ast::BinaryOp::Gt => BinaryOp::Gt,
        ast::BinaryOp::Ge => BinaryOp::Ge,
        ast::BinaryOp::Add => BinaryOp::Add,
        ast::BinaryOp::Sub => BinaryOp::Sub,
        ast::BinaryOp::Mul => BinaryOp::Mul,
        ast::BinaryOp::Div => BinaryOp::Div,
    }
}

const fn lower_unary_op(op: ast::UnaryOp) -> UnaryOp {
    match op {
        ast::UnaryOp::Not => UnaryOp::Not,
    }
}

fn lower_body(body: &ast::Body) -> Body {
    Body {
        statements: body
            .statements
            .iter()
            .map(|stmt| match stmt {
                ast::Stmt::Let(binding) => Stmt::Let(LetBinding {
                    mutable: binding.mutable,
                    name: binding.name.clone(),
                    value: lower_expr(&binding.value),
                    span: binding.span,
                }),
                ast::Stmt::Assign(stmt) => Stmt::Assign(AssignStmt {
                    target: lower_expr(&stmt.target),
                    value: lower_expr(&stmt.value),
                    span: stmt.span,
                }),
                ast::Stmt::Expr(stmt) => Stmt::Expr(ExprStmt {
                    expr: lower_expr(&stmt.expr),
                    span: stmt.span,
                }),
            })
            .collect(),
        tail: body.tail.as_ref().map(lower_expr),
        span: body.span,
    }
}
