#![allow(
    clippy::implicit_hasher,
    clippy::too_many_lines,
    clippy::missing_panics_doc,
    clippy::useless_let_if_seq,
    clippy::match_same_arms,
    clippy::or_fun_call,
    clippy::option_if_let_else
)]
use std::cell::RefCell;
use std::collections::HashMap;

pub mod diagnostics;
pub mod hir;
pub mod ownership;
pub mod semantic;

use crate::hir::{HirLowering, lower as lower_hir};
use crate::semantic::{Analysis, Profile, analyze};
use sarif_syntax::ast::{LoweredAst, lower as lower_ast};
use sarif_syntax::lexer::{LexOutput, lex};
use sarif_syntax::parser::{ParseOutput, parse};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceId(u32);

pub struct FrontendDatabase {
    sources: HashMap<SourceId, (String, String)>,
    next_id: u32,
    lex_cache: RefCell<HashMap<SourceId, LexOutput>>,
    parse_cache: RefCell<HashMap<SourceId, ParseOutput>>,
    ast_cache: RefCell<HashMap<SourceId, LoweredAst>>,
    hir_cache: RefCell<HashMap<SourceId, HirLowering>>,
    semantic_cache: RefCell<HashMap<(SourceId, Profile), Analysis>>,
}

impl FrontendDatabase {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
            next_id: 0,
            lex_cache: RefCell::new(HashMap::new()),
            parse_cache: RefCell::new(HashMap::new()),
            ast_cache: RefCell::new(HashMap::new()),
            hir_cache: RefCell::new(HashMap::new()),
            semantic_cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn add_source(&mut self, path: String, source: String) -> SourceId {
        let id = SourceId(self.next_id);
        self.next_id += 1;
        self.sources.insert(id, (path, source));
        id
    }

    #[must_use]
    pub fn lex(&self, id: SourceId) -> LexOutput {
        if let Some(cached) = self.lex_cache.borrow().get(&id) {
            return cached.clone();
        }
        let (_, source) = self.sources.get(&id).expect("valid source id");
        let result = lex(source);
        self.lex_cache.borrow_mut().insert(id, result.clone());
        result
    }

    #[must_use]
    pub fn parse(&self, id: SourceId) -> ParseOutput {
        if let Some(cached) = self.parse_cache.borrow().get(&id) {
            return cached.clone();
        }
        let lexed = self.lex(id);
        let result = parse(&lexed.tokens);
        self.parse_cache.borrow_mut().insert(id, result.clone());
        result
    }

    #[must_use]
    pub fn ast(&self, id: SourceId) -> LoweredAst {
        if let Some(cached) = self.ast_cache.borrow().get(&id) {
            return cached.clone();
        }
        let parsed = self.parse(id);
        let result = lower_ast(&parsed.root);
        self.ast_cache.borrow_mut().insert(id, result.clone());
        result
    }

    #[must_use]
    pub fn hir(&self, id: SourceId) -> HirLowering {
        if let Some(cached) = self.hir_cache.borrow().get(&id) {
            return cached.clone();
        }
        let ast = self.ast(id);
        let result = lower_hir(&ast.file);
        self.hir_cache.borrow_mut().insert(id, result.clone());
        result
    }

    #[must_use]
    pub fn semantic(&self, id: SourceId, profile: Profile) -> Analysis {
        let key = (id, profile);
        if let Some(cached) = self.semantic_cache.borrow().get(&key) {
            return cached.clone();
        }
        let hir = self.hir(id);
        let result = analyze(&hir.module, profile);
        self.semantic_cache.borrow_mut().insert(key, result.clone());
        result
    }
}

impl Default for FrontendDatabase {
    fn default() -> Self {
        Self::new()
    }
}
