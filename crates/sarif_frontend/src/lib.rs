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
use std::collections::{BTreeMap, HashMap};

pub mod diagnostics;
pub mod hir;
pub mod ownership;
pub mod semantic;

use crate::hir::{HirLowering, ImportDecl, Item, lower as lower_hir};
use crate::semantic::{Analysis, Profile, ResolvedModule, analyze, resolve_module};
use sarif_syntax::ast::{LoweredAst, lower as lower_ast};
use sarif_syntax::lexer::{LexOutput, lex};
use sarif_syntax::parser::{ParseOutput, parse};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceId(u32);

pub struct FrontendDatabase {
    sources: HashMap<SourceId, (String, String)>,
    import_sources: HashMap<String, SourceId>,
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
            import_sources: HashMap::new(),
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

    pub fn add_import_source(
        &mut self,
        module_name: String,
        path: String,
        source: String,
    ) -> SourceId {
        let id = self.add_source(path, source);
        self.import_sources.insert(module_name, id);
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
        if let Some(cached) = self.semantic_cache.borrow().get(&key).cloned() {
            return cached;
        }
        let hir = self.hir(id);
        let imported_modules = self.resolve_imports(&hir.module, &mut Vec::new(), &mut Vec::new());
        let result = analyze(&hir.module, profile, &imported_modules);
        self.semantic_cache.borrow_mut().insert(key, result.clone());
        result
    }

    pub fn import_sources(&self) -> &HashMap<String, SourceId> {
        &self.import_sources
    }

    pub fn resolve_imports(
        &self,
        module: &crate::hir::Module,
        resolving: &mut Vec<String>,
        diagnostics: &mut Vec<sarif_syntax::Diagnostic>,
    ) -> BTreeMap<String, ResolvedModule> {
        let mut result = BTreeMap::new();
        for item in &module.items {
            if let Item::Import(ImportDecl {
                module: mod_name,
                span,
                ..
            }) = item
            {
                if result.contains_key(mod_name) {
                    continue;
                }
                if resolving.contains(mod_name) {
                    diagnostics.push(sarif_syntax::Diagnostic::new(
                        "semantic.import-cycle",
                        format!("circular import: `{mod_name}` is already being resolved"),
                        *span,
                        Some("Remove the circular import chain.".to_owned()),
                    ));
                    continue;
                }
                let Some(&source_id) = self.import_sources.get(mod_name) else {
                    continue;
                };
                resolving.push(mod_name.clone());
                let imported_hir = self.hir(source_id);
                let nested = self.resolve_imports(&imported_hir.module, resolving, diagnostics);
                let resolved = resolve_module(&imported_hir.module, diagnostics, &nested);
                resolving.pop();
                result.insert(mod_name.clone(), resolved);
            }
        }
        result
    }
}

impl Default for FrontendDatabase {
    fn default() -> Self {
        Self::new()
    }
}
