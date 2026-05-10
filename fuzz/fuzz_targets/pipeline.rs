#![no_main]

use libfuzzer_sys::fuzz_target;
use sarif_codegen::analyze_escapes;
use sarif_codegen::lower;
use sarif_frontend::hir;
use sarif_frontend::semantic::{self, Profile};
use sarif_syntax::ast;
use sarif_syntax::lexer;
use sarif_syntax::parser;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else { return };

    let lex_output = lexer::lex(source);
    let parse_output = parser::parse(&lex_output.tokens);
    let ast_output = ast::lower(&parse_output.root);
    let hir_output = hir::lower(&ast_output.file);
    let _semantic = semantic::analyze(&hir_output.module, Profile::Core);
    let _mir = lower(&hir_output.module);
    let _escape = analyze_escapes(&_mir.program);
});
