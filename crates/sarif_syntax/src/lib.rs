use std::fmt::Write;

pub mod ast;
pub mod lexer;
pub mod parser;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn empty_at(offset: usize) -> Self {
        Self::new(offset, offset)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
}

impl Diagnostic {
    #[must_use]
    pub fn new(
        code: &'static str,
        message: impl Into<String>,
        span: Span,
        help: Option<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            span,
            help,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Whitespace,
    Newline,
    Comment,
    Ident,
    Integer,
    Float,
    String,
    KwAnd,
    KwConst,
    KwComptime,
    KwEnsures,
    KwEffect,
    KwEnum,
    KwEffects,
    KwElse,
    KwFalse,
    KwFn,
    KwHandle,
    KwIf,
    KwIn,
    KwLet,
    KwMatch,
    KwMut,
    KwNot,
    KwOr,
    KwRepeat,
    KwRequires,
    KwStruct,
    KwTrue,
    KwWhile,
    KwWith,
    Arrow,
    Colon,
    Comma,
    Dot,
    Eq,
    FatArrow,
    EqEq,
    Ge,
    Gt,
    LBrace,
    LBracket,
    Le,
    Lt,
    LParen,
    Minus,
    NotEq,
    Plus,
    RBrace,
    RBracket,
    RParen,
    Semicolon,
    Slash,
    Star,
    Underscore,
    Invalid,
    Eof,
}

impl TokenKind {
    #[must_use]
    pub const fn is_trivia(self) -> bool {
        matches!(self, Self::Whitespace | Self::Newline | Self::Comment)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub span: Span,
}

impl Token {
    #[must_use]
    pub fn new(kind: TokenKind, lexeme: impl Into<String>, span: Span) -> Self {
        Self {
            kind,
            lexeme: lexeme.into(),
            span,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    SourceFile,
    ConstItem,
    FnItem,
    EnumItem,
    StructItem,
    ParamList,
    Param,
    ReturnType,
    EffectsClause,
    RequiresClause,
    EnsuresClause,
    EffectList,
    FieldList,
    VariantList,
    GenericParamList,
    GenericParam,
    Field,
    Variant,
    EffectItem,
    TypePath,
    TypeArray,
    Body,
    LetStmt,
    AssignStmt,
    ExprStmt,
    ArgList,
    FieldInitList,
    FieldInit,
    ExprBinary,
    ExprBool,
    ExprCall,
    ExprArray,
    ExprContractResult,
    ExprField,
    ExprIndex,
    ExprIf,
    ExprMatch,
    ExprRepeat,
    ExprWhile,
    ExprGroup,
    ExprInteger,
    ExprFloat,
    ExprName,
    ExprRecord,
    ExprUnary,
    ExprString,
    ExprComptime,
    ExprHandle,
    HandleArm,
    MatchPattern,
    MatchArm,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Element {
    Node(Node),
    Token(Token),
}

impl Element {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Node(node) => node.span,
            Self::Token(token) => token.span,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
    pub children: Vec<Element>,
}

impl Node {
    #[must_use]
    pub fn new(kind: NodeKind, children: Vec<Element>) -> Self {
        let span = span_from_elements(&children);
        Self {
            kind,
            span,
            children,
        }
    }

    #[must_use]
    pub const fn empty(kind: NodeKind, offset: usize) -> Self {
        Self {
            kind,
            span: Span::empty_at(offset),
            children: Vec::new(),
        }
    }

    /// Render this node and its children as an indented debug tree.
    ///
    /// # Panics
    ///
    /// Panics only if writing into an in-memory `String` fails, which should
    /// be unreachable.
    #[must_use]
    pub fn pretty(&self) -> String {
        let mut output = String::new();
        self.pretty_into(&mut output, 0)
            .expect("writing to a string cannot fail");
        output
    }

    fn pretty_into(&self, output: &mut String, indent: usize) -> std::fmt::Result {
        for _ in 0..indent {
            output.push_str("  ");
        }

        writeln!(
            output,
            "{:?} [{}..{}]",
            self.kind, self.span.start, self.span.end,
        )?;

        for child in &self.children {
            match child {
                Element::Node(node) => node.pretty_into(output, indent + 1)?,
                Element::Token(token) => {
                    for _ in 0..=indent {
                        output.push_str("  ");
                    }

                    writeln!(
                        output,
                        "{:?} [{}..{}] {:?}",
                        token.kind, token.span.start, token.span.end, token.lexeme,
                    )?;
                }
            }
        }

        Ok(())
    }
}

#[must_use]
pub fn span_from_elements(elements: &[Element]) -> Span {
    let Some(first) = elements.first() else {
        return Span::default();
    };

    let Some(last) = elements.last() else {
        return Span::default();
    };

    Span::new(first.span().start, last.span().end)
}
