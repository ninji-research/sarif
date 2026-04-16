use crate::{Diagnostic, Span, Token, TokenKind};
use logos::Logos;

#[derive(Logos, Clone, Copy, Debug, PartialEq, Eq)]
enum RawTokenKind {
    #[regex(r"[ \t\r\f]+")]
    Whitespace,
    #[regex(r"\n+")]
    Newline,
    #[regex(r"//[^\n]*", allow_greedy = true)]
    Comment,
    #[token("and")]
    KwAnd,
    #[token("const")]
    KwConst,
    #[token("comptime")]
    KwComptime,
    #[token("ensures")]
    KwEnsures,
    #[token("effect")]
    KwEffect,
    #[token("enum")]
    KwEnum,
    #[token("effects")]
    KwEffects,
    #[token("else")]
    KwElse,
    #[token("false")]
    KwFalse,
    #[token("fn")]
    KwFn,
    #[token("handle")]
    KwHandle,
    #[token("if")]
    KwIf,
    #[token("in")]
    KwIn,
    #[token("let")]
    KwLet,
    #[token("match")]
    KwMatch,
    #[token("mut")]
    KwMut,
    #[token("not")]
    KwNot,
    #[token("or")]
    KwOr,
    #[token("perform")]
    KwPerform,
    #[token("repeat")]
    KwRepeat,
    #[token("requires")]
    KwRequires,
    #[token("struct")]
    KwStruct,
    #[token("true")]
    KwTrue,
    #[token("while")]
    KwWhile,
    #[token("with")]
    KwWith,
    #[token("->")]
    Arrow,
    #[token("&")]
    Amp,
    #[token("^")]
    Caret,
    #[token(":")]
    Colon,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("=")]
    Eq,
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("*=")]
    StarEq,
    #[token("/=")]
    SlashEq,
    #[token("=>")]
    FatArrow,
    #[token("==")]
    EqEq,
    #[token(">=")]
    Ge,
    #[token(">")]
    Gt,
    #[token("{")]
    LBrace,
    #[token("[")]
    LBracket,
    #[token("<=")]
    Le,
    #[token("<")]
    Lt,
    #[token("(")]
    LParen,
    #[token("-")]
    Minus,
    #[token("!=")]
    NotEq,
    #[token("|")]
    Pipe,
    #[token("+")]
    Plus,
    #[token("}")]
    RBrace,
    #[token("]")]
    RBracket,
    #[token(")")]
    RParen,
    #[token(";")]
    Semicolon,
    #[token("<<")]
    Shl,
    #[token("/")]
    Slash,
    #[token(">>")]
    Shr,
    #[token("*")]
    Star,
    #[token("_")]
    Underscore,
    #[regex(r"[0-9]+\.[0-9]+(?:[eE][+-]?[0-9]+)?|[0-9]+[eE][+-]?[0-9]+")]
    Float,
    #[regex(r"[0-9]+")]
    Integer,
    #[regex(r#""([^"\\]|\\.)*""#)]
    String,
    #[regex(r"[A-Za-z][A-Za-z0-9_]*")]
    Ident,
}

#[derive(Clone, Debug, Default)]
pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

#[must_use]
pub fn lex(source: &str) -> LexOutput {
    let mut lexer = RawTokenKind::lexer(source);
    let mut output = LexOutput::default();

    while let Some(result) = lexer.next() {
        let range = lexer.span();
        let span = Span::new(range.start, range.end);
        let lexeme = &source[range];

        if let Ok(kind) = result {
            output
                .tokens
                .push(Token::new(map_token_kind(kind), lexeme, span));
        } else {
            output
                .tokens
                .push(Token::new(TokenKind::Invalid, lexeme, span));
            output.diagnostics.push(Diagnostic::new(
                "lex.invalid-token",
                format!("invalid token `{lexeme}`"),
                span,
                Some(
                    "Sarif source is ASCII-keyword based; remove or replace this token.".to_owned(),
                ),
            ));
        }
    }

    output.tokens.push(Token::new(
        TokenKind::Eof,
        String::new(),
        Span::empty_at(source.len()),
    ));

    output
}

const fn map_token_kind(kind: RawTokenKind) -> TokenKind {
    match kind {
        RawTokenKind::Whitespace => TokenKind::Whitespace,
        RawTokenKind::Newline => TokenKind::Newline,
        RawTokenKind::Comment => TokenKind::Comment,
        RawTokenKind::KwAnd => TokenKind::KwAnd,
        RawTokenKind::KwConst => TokenKind::KwConst,
        RawTokenKind::KwComptime => TokenKind::KwComptime,
        RawTokenKind::KwEnsures => TokenKind::KwEnsures,
        RawTokenKind::KwEffect => TokenKind::KwEffect,
        RawTokenKind::KwEnum => TokenKind::KwEnum,
        RawTokenKind::KwEffects => TokenKind::KwEffects,
        RawTokenKind::KwElse => TokenKind::KwElse,
        RawTokenKind::KwFalse => TokenKind::KwFalse,
        RawTokenKind::KwFn => TokenKind::KwFn,
        RawTokenKind::KwHandle => TokenKind::KwHandle,
        RawTokenKind::KwIf => TokenKind::KwIf,
        RawTokenKind::KwIn => TokenKind::KwIn,
        RawTokenKind::KwLet => TokenKind::KwLet,
        RawTokenKind::KwMatch => TokenKind::KwMatch,
        RawTokenKind::KwMut => TokenKind::KwMut,
        RawTokenKind::KwNot => TokenKind::KwNot,
        RawTokenKind::KwOr => TokenKind::KwOr,
        RawTokenKind::KwPerform => TokenKind::KwPerform,
        RawTokenKind::KwRepeat => TokenKind::KwRepeat,
        RawTokenKind::KwRequires => TokenKind::KwRequires,
        RawTokenKind::KwStruct => TokenKind::KwStruct,
        RawTokenKind::KwTrue => TokenKind::KwTrue,
        RawTokenKind::KwWhile => TokenKind::KwWhile,
        RawTokenKind::KwWith => TokenKind::KwWith,
        RawTokenKind::Arrow => TokenKind::Arrow,
        RawTokenKind::Amp => TokenKind::Amp,
        RawTokenKind::Caret => TokenKind::Caret,
        RawTokenKind::Colon => TokenKind::Colon,
        RawTokenKind::Comma => TokenKind::Comma,
        RawTokenKind::Dot => TokenKind::Dot,
        RawTokenKind::Eq => TokenKind::Eq,
        RawTokenKind::PlusEq => TokenKind::PlusEq,
        RawTokenKind::MinusEq => TokenKind::MinusEq,
        RawTokenKind::StarEq => TokenKind::StarEq,
        RawTokenKind::SlashEq => TokenKind::SlashEq,
        RawTokenKind::FatArrow => TokenKind::FatArrow,
        RawTokenKind::EqEq => TokenKind::EqEq,
        RawTokenKind::Ge => TokenKind::Ge,
        RawTokenKind::Gt => TokenKind::Gt,
        RawTokenKind::LBrace => TokenKind::LBrace,
        RawTokenKind::LBracket => TokenKind::LBracket,
        RawTokenKind::Le => TokenKind::Le,
        RawTokenKind::Lt => TokenKind::Lt,
        RawTokenKind::LParen => TokenKind::LParen,
        RawTokenKind::Minus => TokenKind::Minus,
        RawTokenKind::NotEq => TokenKind::NotEq,
        RawTokenKind::Pipe => TokenKind::Pipe,
        RawTokenKind::Plus => TokenKind::Plus,
        RawTokenKind::RBrace => TokenKind::RBrace,
        RawTokenKind::RBracket => TokenKind::RBracket,
        RawTokenKind::RParen => TokenKind::RParen,
        RawTokenKind::Semicolon => TokenKind::Semicolon,
        RawTokenKind::Shl => TokenKind::Shl,
        RawTokenKind::Slash => TokenKind::Slash,
        RawTokenKind::Shr => TokenKind::Shr,
        RawTokenKind::Star => TokenKind::Star,
        RawTokenKind::Underscore => TokenKind::Underscore,
        RawTokenKind::Float => TokenKind::Float,
        RawTokenKind::Integer => TokenKind::Integer,
        RawTokenKind::String => TokenKind::String,
        RawTokenKind::Ident => TokenKind::Ident,
    }
}

#[cfg(test)]
mod tests {
    use super::lex;
    use crate::TokenKind;

    #[test]
    fn lexes_keywords_and_trivia_losslessly() {
        let output = lex(
            "const answer: I32 = 42;\nenum Flag { off, on }\nfn main() requires result == 0 or not false {\n  let mut x = true;\n  x = false;\n  match Flag.on { Flag.off => { 0 }, Flag.on => { repeat i in 2 { if x { x } else { false } }; 0 }, };\n}\n",
        );

        let kinds: Vec<_> = output.tokens.iter().map(|token| token.kind).collect();
        assert!(kinds.contains(&TokenKind::KwFn));
        assert!(kinds.contains(&TokenKind::KwConst));
        assert!(kinds.contains(&TokenKind::KwEnum));
        assert!(kinds.contains(&TokenKind::KwLet));
        assert!(kinds.contains(&TokenKind::KwIf));
        assert!(kinds.contains(&TokenKind::KwIn));
        assert!(kinds.contains(&TokenKind::KwMatch));
        assert!(kinds.contains(&TokenKind::KwMut));
        assert!(kinds.contains(&TokenKind::KwNot));
        assert!(kinds.contains(&TokenKind::KwElse));
        assert!(kinds.contains(&TokenKind::KwTrue));
        assert!(kinds.contains(&TokenKind::KwRequires));
        assert!(kinds.contains(&TokenKind::KwRepeat));
        assert!(kinds.contains(&TokenKind::KwOr));
        assert!(kinds.contains(&TokenKind::Eq));
        assert!(kinds.contains(&TokenKind::FatArrow));
        assert!(kinds.contains(&TokenKind::EqEq));
        assert!(kinds.contains(&TokenKind::Semicolon));
        assert!(kinds.contains(&TokenKind::Newline));
        assert_eq!(output.diagnostics.len(), 0);
    }

    #[test]
    fn lexes_while_keyword() {
        let output = lex("fn main() { while true {} }");

        assert!(
            output
                .tokens
                .iter()
                .map(|token| token.kind)
                .any(|kind| kind == TokenKind::KwWhile)
        );
        assert!(output.diagnostics.is_empty());
    }

    #[test]
    fn lexes_compound_assignment_tokens() {
        let output = lex("fn main() = { let mut x = 0; x += 1; x -= 1; x *= 2; x /= 2; x };");
        let kinds: Vec<_> = output.tokens.iter().map(|token| token.kind).collect();

        assert!(kinds.contains(&TokenKind::PlusEq));
        assert!(kinds.contains(&TokenKind::MinusEq));
        assert!(kinds.contains(&TokenKind::StarEq));
        assert!(kinds.contains(&TokenKind::SlashEq));
        assert_eq!(output.diagnostics.len(), 0);
    }

    #[test]
    fn rejects_non_ascii_identifiers() {
        let output = lex("fn café() {}");

        assert_eq!(output.diagnostics.len(), 1);
        assert!(
            output
                .tokens
                .iter()
                .any(|token| token.kind == TokenKind::Invalid)
        );
    }
}
