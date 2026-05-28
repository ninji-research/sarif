#![no_main]

use libfuzzer_sys::fuzz_target;
use sarif_codegen::analyze_escapes;
use sarif_codegen::lower;
use sarif_frontend::hir;
use sarif_frontend::semantic::{self, Profile};
use sarif_syntax::ast;
use sarif_syntax::lexer;
use sarif_syntax::parser;

// Fuzz target for parser and allocator robustness under memory constraints.
// Tests that deeply nested inputs, large token streams, and pathological
// constructs do not panic or leak.
fuzz_target!(|data: &[u8]| {
    // Reject inputs larger than 16KB to avoid OOM in CI
    if data.len() > 16384 {
        return;
    }

    let Ok(source) = std::str::from_utf8(data) else { return };

    let lex_output = lexer::lex(source);
    let parse_output = parser::parse(&lex_output.tokens);
    let ast_output = ast::lower(&parse_output.root);
    // Only run full pipeline on well-formed inputs (no parse errors)
    if parse_output.root.len() > 0 && ast_output.file.items.len() > 0 {
        let hir_output = hir::lower(&ast_output.file);
        let _semantic = semantic::analyze(&hir_output.module, Profile::Core);
        let _mir = lower(&hir_output.module);
        let _escape = analyze_escapes(&_mir.program);
    }
});
