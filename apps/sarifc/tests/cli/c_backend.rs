use super::support::{
    bootstrap_syntax_dir, const_control_flow_example, multi_file_package_dir,
    multi_file_package_manifest, package_dir, package_manifest, run_sarif, temp_output,
    temp_source,
};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Build a SARIF source string to a C-compiled binary and return the output path.
fn c_build(source: &str, stem: &str) -> std::path::PathBuf {
    let path = temp_source(source);
    let binary_path = temp_output(stem, "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "c",
        "--profile",
        "core",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        build.status.success(),
        "C build should succeed for `{stem}`: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    binary_path
}

/// Build a SARIF source string to a C-compiled binary with --print-main and return the output path.
fn c_build_print_main(source: &str, stem: &str) -> std::path::PathBuf {
    let path = temp_source(source);
    let binary_path = temp_output(stem, "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "c",
        "--profile",
        "core",
        "--print-main",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        build.status.success(),
        "C build with --print-main should succeed for `{stem}`: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    binary_path
}

/// Build a SARIF file path to a C-compiled binary and return the output path.
fn c_build_path(path: &std::path::Path, stem: &str) -> std::path::PathBuf {
    let binary_path = temp_output(stem, "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "c",
        "--profile",
        "core",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        build.status.success(),
        "C build should succeed for `{}`: {}",
        path.display(),
        String::from_utf8_lossy(&build.stderr)
    );
    binary_path
}

/// Run a compiled C binary and return its exit code.
fn run_c_exit(binary_path: &std::path::Path) -> i32 {
    let output = Command::new(binary_path)
        .output()
        .expect("C-compiled binary should run");
    output.status.code().unwrap_or(-1)
}

/// Run a compiled C binary and return its stdout as a trimmed string.
fn run_c_stdout(binary_path: &std::path::Path) -> String {
    let output = Command::new(binary_path)
        .output()
        .expect("C-compiled binary should run");
    assert!(
        output.status.success(),
        "C-compiled binary should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned()
}

// ============================================================
// Basic I32 main tests (exit-code semantics)
// ============================================================

#[test]
fn c_build_emits_a_runnable_binary() {
    let binary = c_build("fn main() -> I32 { 42 }", "c_build_basic");
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_mutable_locals() {
    let binary = c_build(
        "fn main() -> I32 { let mut total = 20; total = total + 22; total }",
        "c_build_mutable",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_expression_bodied_functions() {
    let binary = c_build(
        "fn add(left: I32, right: I32) -> I32 = left + right;\nfn main() -> I32 = add(20, 22);",
        "c_build_expr_body",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_compound_assignments() {
    let binary = c_build(
        "fn main() -> I32 { let mut total = 20; total += 22; total }",
        "c_build_compound",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_if_else() {
    let binary = c_build(
        "fn main() -> I32 { if false { 0 } else if true { 42 } else { 7 } }",
        "c_build_if_else",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_not_operator() {
    let binary = c_build(
        "fn flag() -> Bool { false }\nfn main() -> Bool { not flag() }",
        "c_build_not",
    );
    // Bool main returns exit 0 for true, 1 for false (without --print-main)
    assert_eq!(run_c_exit(&binary), 0);
}

#[test]
fn c_build_supports_while_loops() {
    let binary = c_build(
        "fn main() -> I32 { let mut n = 0; let mut total = 0; while n < 2 { total = total + 21; n = n + 1; }; total }",
        "c_build_while",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_scalar_match() {
    let binary = c_build(
        "fn pick(code: I32) -> I32 { match code { 40 => { 1 }, 41 => { 2 }, _ => { 3 }, } }\nfn main() -> I32 { pick(41) }",
        "c_build_match",
    );
    assert_eq!(run_c_exit(&binary), 2);
}

#[test]
fn c_build_supports_enum_match() {
    let binary = c_build(
        "enum Color { red, green, blue }\nfn main() -> I32 { match Color.green { Color.red => { 1 }, Color.green => { 2 }, Color.blue => { 3 }, } }",
        "c_build_enum_match",
    );
    assert_eq!(run_c_exit(&binary), 2);
}

// ============================================================
// Text main tests (stdout semantics)
// ============================================================

#[test]
fn c_build_text_main_prints_to_stdout() {
    let binary = c_build(
        "fn main() -> Text { text_concat(\"\", text_concat(\"sa\", text_concat(\"rif\", \"\"))) }",
        "c_build_text_concat",
    );
    assert_eq!(run_c_stdout(&binary), "sarif");
}

#[test]
fn c_build_text_slice_prints_to_stdout() {
    let binary = c_build(
        "fn main() -> Text { text_slice(\"sarif\", 1, 4) }",
        "c_build_text_slice",
    );
    assert_eq!(run_c_stdout(&binary), "ari");
}

#[test]
fn c_build_text_eq_range_as_bool_main() {
    // text_eq_range works correctly (avoids the text == pointer comparison bug)
    let binary = c_build(
        "fn main() -> Bool { text_eq_range(\"sarif\", 0, 5, \"sarif\") and text_eq_range(\"sarif\", 1, 4, \"ari\") }",
        "c_build_text_eq_range",
    );
    // Bool true -> exit code 0
    assert_eq!(run_c_exit(&binary), 0);
}

// ============================================================
// Record and struct tests
// ============================================================

#[test]
fn c_build_supports_record_field_punning() {
    let binary = c_build(
        "struct Pair { left: I32, right: I32 }\nfn main() -> I32 { let left = 7; let right = 9; let pair = Pair { left, right }; pair.left + pair.right }",
        "c_build_record_punning",
    );
    assert_eq!(run_c_exit(&binary), 16);
}

#[test]
fn c_build_supports_mutable_record_fields() {
    let binary = c_build(
        "struct Pair { left: I32, right: I32 }\nfn main() -> I32 { let mut pair = Pair { left: 7, right: 9 }; pair.left = 20; pair.left + pair.right }",
        "c_build_mutable_record",
    );
    assert_eq!(run_c_exit(&binary), 29);
}

// ============================================================
// Array tests
// ============================================================

#[test]
fn c_build_supports_nested_arrays() {
    let binary = c_build(
        "fn main() -> I32 { let xs = [[20, 22], [0, 0]]; xs[0][0] + xs[0][1] }",
        "c_build_nested_arrays",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_mutable_array_elements() {
    let binary = c_build(
        "fn main() -> I32 { let mut xs = [0, 0]; xs[0] = 20; xs[1] = 22; xs[0] + xs[1] }",
        "c_build_mutable_arrays",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_const_generic_array_lengths() {
    let binary = c_build(
        "fn sum[N](xs: [I32; N]) -> I32 { let mut total = 0; repeat i in N { total += xs[i]; }; total }\nfn main() -> I32 { sum([10, 10, 10, 12]) }",
        "c_build_const_generic_arrays",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_explicit_array_types() {
    let binary = c_build(
        "struct Grid { rows: [[I32; 2]; 2], }\nfn first(xs: [I32; 2]) -> I32 { xs[0] + xs[1] }\nfn main() -> I32 { let grid = Grid { rows: [[20, 22], [0, 0]] }; first(grid.rows[0]) }",
        "c_build_explicit_arrays",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_repeat_array_literals() {
    let binary = c_build(
        "fn first_repeat[N](xs: [I32; N]) -> I32 { let ys = [xs[0]; N]; ys[0] }\nfn main() -> I32 { first_repeat([42]) }",
        "c_build_repeat_array_lit",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_top_level_array_consts() {
    let binary = c_build(
        "const XS: [I32; 2] = [20, 22];\nfn main() -> I32 { XS[0] + XS[1] }",
        "c_build_array_consts",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

// ============================================================
// F64 tests
// ============================================================

#[test]
fn c_build_supports_float_sqrt_pipeline() {
    let binary = c_build(
        "fn main() -> Text { text_from_f64_fixed(sqrt(9.0) + 0.125, 3) }",
        "c_build_sqrt_pipeline",
    );
    assert_eq!(run_c_stdout(&binary), "3.125");
}

#[test]
fn c_build_supports_f64_from_i32() {
    let binary = c_build(
        "fn main() -> Text { text_from_f64_fixed(f64_from_i32(7) / 2.0, 1) }",
        "c_build_f64_from_i32",
    );
    assert_eq!(run_c_stdout(&binary), "3.5");
}

#[test]
fn c_build_supports_top_level_float_consts() {
    let binary = c_build(
        "const X: F64 = 3.5;\nfn main() -> Text { text_from_f64_fixed(X, 1) }",
        "c_build_float_consts",
    );
    assert_eq!(run_c_stdout(&binary), "3.5");
}

#[test]
fn c_build_supports_text_from_f64_fixed() {
    let binary = c_build(
        "fn main() -> Text { text_from_f64_fixed(3.5, 2) }",
        "c_build_text_from_f64",
    );
    assert_eq!(run_c_stdout(&binary), "3.50");
}

// ============================================================
// Text builder and text index tests (alloc effect)
// ============================================================

#[test]
fn c_build_supports_text_builder() {
    let binary = c_build(
        "fn main() -> Text effects [alloc] { let mut builder = text_builder_new(); builder = text_builder_append(builder, \"sa\"); builder = text_builder_append(builder, text_slice(\"sarif\", 2, 5)); text_builder_finish(builder) }",
        "c_build_text_builder",
    );
    assert_eq!(run_c_stdout(&binary), "sarif");
}

#[test]
fn c_build_supports_text_index_set() {
    let binary = c_build(
        "fn main() -> I32 effects [alloc] { let mut index = text_index_new(); index = text_index_set(index, \"alpha\", 7); let a = text_index_get(index, \"alpha\"); index = text_index_set(index, \"alpha\", 9); let b = text_index_get(index, \"alpha\"); a * 10 + b }",
        "c_build_text_index_set",
    );
    assert_eq!(run_c_exit(&binary), 79);
}

#[test]
fn c_build_supports_text_index_get_or_insert() {
    let binary = c_build(
        "fn main() -> I32 effects [alloc] { let index = text_index_new(); let a = text_index_get_or_insert(index, \"alpha\", 7); let b = text_index_get_or_insert(index, \"alpha\", 9); a * 10 + b }",
        "c_build_text_index_get_or_insert",
    );
    assert_eq!(run_c_exit(&binary), 77);
}

// ============================================================
// Const and parse tests
// ============================================================

#[test]
fn c_build_supports_top_level_comptime_consts() {
    let binary = c_build(
        "const X: I32 = comptime { 20 + 22 };\nfn main() -> I32 { X }",
        "c_build_comptime_consts",
    );
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_supports_parse_i32_range() {
    let binary = c_build(
        "fn main() -> I32 { parse_i32_range(\"xx-42yy\", 2, 5) + parse_i32_range(\"0017\", 0, 4) }",
        "c_build_parse_i32",
    );
    // Without --print-main, I32 main returns exit codes (0-255), so -25 wraps to 231.
    // This matches the native backend behavior.
    assert_eq!(run_c_exit(&binary), 231);
}

// ============================================================
// Runtime arguments
// ============================================================

#[test]
fn c_build_passes_process_arguments() {
    let binary = c_build(
        "fn main() -> I32 effects [SystemIO] { perform SystemIO.arg_count() }",
        "c_build_arg_count",
    );
    // Running with no extra args: arg_count() should return 1 (program name)
    let output = Command::new(&binary)
        .output()
        .expect("C-compiled binary should run");
    assert_eq!(output.status.code(), Some(1));
}

// ============================================================
// Stdin tests
// ============================================================

#[test]
fn c_build_reads_stdin_text() {
    let binary = c_build(
        "fn main() -> Text effects [SystemIO] { perform SystemIO.stdin_text() }",
        "c_build_stdin_text",
    );
    let mut child = Command::new(&binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("C-compiled binary should spawn");
    child
        .stdin
        .take()
        .expect("stdin pipe should exist")
        .write_all(b">id\nACGT\n")
        .expect("stdin should be writable");
    let output = child
        .wait_with_output()
        .expect("C-compiled binary should run");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        ">id\nACGT"
    );
}

// ============================================================
// --dump-ir=c tests
// ============================================================

#[test]
fn c_dump_ir_emits_c_source() {
    let path = temp_source("fn main() -> I32 { 42 }");
    let output = run_sarif(&[
        "check",
        path.to_str().expect("utf-8 path"),
        "--dump-ir=c",
        "--target",
        "c",
    ]);

    assert!(output.status.success(), "dump-ir=c should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("#include <stdint.h>"),
        "C output should include stdint.h"
    );
    assert!(
        stdout.contains("sarif_user_main"),
        "C output should define sarif_user_main"
    );
}

// ============================================================
// Package and example inputs
// ============================================================

#[test]
fn c_build_accepts_package_inputs() {
    for (path, expected) in [
        (package_dir(), 42),
        (package_manifest(), 42),
        (multi_file_package_dir(), 42),
        (multi_file_package_manifest(), 42),
        (const_control_flow_example(), 42),
    ] {
        let binary = c_build_path(&path, "c_build_package");
        assert_eq!(
            run_c_exit(&binary),
            expected,
            "C build of {} should return {expected}",
            path.display()
        );
    }
}

#[test]
fn c_build_links_extern_ffi_function() {
    let sarif_source = "extern {
        fn ffi_double_it(x: I32) -> I32;
    }
    fn main() -> I32 { ffi_double_it(21) }";

    let source_path = temp_source(sarif_source);
    let dump = run_sarif(&[
        "check",
        source_path.to_str().expect("utf-8 path"),
        "--target",
        "c",
        "--dump-ir=c",
    ]);
    assert!(
        dump.status.success(),
        "C dump should succeed: {}",
        String::from_utf8_lossy(&dump.stderr)
    );
    let c_code = String::from_utf8(dump.stdout).expect("C output should be valid UTF-8");
    // sarifc check appends "ok [<profile>]" as a status line — strip it
    let c_code = c_code.strip_prefix('\u{feff}').unwrap_or(&c_code); // strip BOM
    let c_code = c_code.trim_end();
    let c_code = match c_code.rsplit_once('\n') {
        Some((body, status)) if status.starts_with("ok [") => body,
        _ => c_code,
    };
    let c_code = c_code.trim_end();
    assert!(
        c_code.contains("ffi_double_it"),
        "C output should reference ffi_double_it"
    );

    // Find the repo root for the runtime source
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let runtime_source = manifest_dir
        .join("../../runtime/sarif_runtime.c")
        .canonicalize()
        .expect("runtime source should exist");

    // Write the C implementation of the extern function
    let impl_path = temp_output("ffi_impl", "c");
    let mut impl_file = std::fs::File::create(&impl_path).expect("create impl file");
    write!(
        impl_file,
        "#include <stdint.h>\nuint64_t ffi_double_it(uint64_t x) {{ return x * 2; }}\n"
    )
    .expect("write impl file");

    // Write the generated C code
    let gen_path = temp_output("generated", "c");
    std::fs::write(&gen_path, &c_code).expect("write generated C code");

    // Compile everything together
    let binary_path = temp_output("ffi_test", "bin");
    let compile = Command::new("cc")
        .args([
            "-O3",
            "-g0",
            "-fno-stack-protector",
            "-DSARIF_MAIN_KIND=1",
            "-lm",
            "-o",
            binary_path.to_str().expect("utf-8 path"),
            gen_path.to_str().expect("utf-8 path"),
            impl_path.to_str().expect("utf-8 path"),
            runtime_source.to_str().expect("utf-8 path"),
        ])
        .output()
        .expect("cc should run");
    assert!(
        compile.status.success(),
        "C compilation should succeed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    // Run the binary
    let output = Command::new(&binary_path)
        .output()
        .expect("binary should run");
    assert_eq!(
        output.status.code().unwrap_or(-1),
        42,
        "ffi_double_it(21) should return 42"
    );
}

#[test]
fn c_build_links_extern_ffi_void_function() {
    let sarif_source = "extern {
        fn ffi_write_text(text: Text);
    }
    fn main() -> I32 { ffi_write_text(\"hello ffi\"); 0 }";

    let source_path = temp_source(sarif_source);
    let dump = run_sarif(&[
        "check",
        source_path.to_str().expect("utf-8 path"),
        "--target",
        "c",
        "--dump-ir=c",
    ]);
    assert!(
        dump.status.success(),
        "C dump should succeed: {}",
        String::from_utf8_lossy(&dump.stderr)
    );
    let c_code = String::from_utf8(dump.stdout).expect("C output should be valid UTF-8");
    let c_code = c_code.trim_end();
    let c_code = match c_code.rsplit_once('\n') {
        Some((body, status)) if status.starts_with("ok [") => body,
        _ => c_code,
    };
    let c_code = c_code.trim_end();
    assert!(
        c_code.contains("ffi_write_text"),
        "C output should reference ffi_write_text"
    );

    // Write the C implementation — accepts uint64_t (pointer to interned text bytes)
    // and writes to stdout via fwrite. The Sarif Text type stores a pointer to
    // interned bytes, so we cast uint64_t -> const unsigned char* and compute
    // the length from the 8-byte length prefix (as the runtime does internally).
    // But for simplicity, we just call the runtime's sarif_stdout_write through
    // the Text representation we receive (which is layout-compatible).
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let runtime_source = manifest_dir
        .join("../../runtime/sarif_runtime.c")
        .canonicalize()
        .expect("runtime source should exist");

    let impl_path = temp_output("ffi_void_impl", "c");
    let mut impl_file = std::fs::File::create(&impl_path).expect("create impl file");
    write!(
        impl_file,
        "#include <stdint.h>\n\
         #include <stdio.h>\n\
         void ffi_write_text(uint64_t text) {{\n\
             // Text is a pointer to interned bytes with an 8-byte length prefix\n\
             unsigned char* bytes = (unsigned char*)text;\n\
             uint64_t len;\n\
             __builtin_memcpy(&len, bytes, 8);\n\
             fwrite(bytes + 8, 1, len, stdout);\n\
         }}\n"
    )
    .expect("write impl file");

    let gen_path = temp_output("ffi_void_gen", "c");
    std::fs::write(&gen_path, &c_code).expect("write generated C code");

    let binary_path = temp_output("ffi_void_test", "bin");
    let compile = Command::new("cc")
        .args([
            "-O3",
            "-g0",
            "-fno-stack-protector",
            "-DSARIF_MAIN_KIND=1",
            "-lm",
            "-o",
            binary_path.to_str().expect("utf-8 path"),
            gen_path.to_str().expect("utf-8 path"),
            impl_path.to_str().expect("utf-8 path"),
            runtime_source.to_str().expect("utf-8 path"),
        ])
        .output()
        .expect("cc should run");
    assert!(
        compile.status.success(),
        "C compilation should succeed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let output = Command::new(&binary_path)
        .output()
        .expect("binary should run");
    assert!(output.status.success(), "binary should exit successfully");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "hello ffi");
}

#[test]
fn c_build_accepts_bootstrap_syntax() {
    let binary = c_build_path(&bootstrap_syntax_dir(), "c_build_bootstrap_syntax");
    assert_eq!(run_c_exit(&binary), 35);
}

// NOTE: bootstrap_tools is not tested here because it uses with_arena (alloc scopes),
// which the C backend does not yet support (missing extern declarations for
// sarif_alloc_push and sarif_alloc_pop).

// ============================================================
// --print-main tests (note: currently C backend ignores --print-main)
// ============================================================

#[test]
fn c_build_print_main_i32_exit_code() {
    // The C backend currently does NOT support --print-main (it ignores the flag).
    // Without --print-main for the native backend, I32 main uses exit-code semantics.
    // With --print-main for the C backend, the behavior is the same as without it:
    // the I32 return value becomes the process exit code.
    let binary = c_build_print_main("fn main() -> I32 { 42 }", "c_build_print_main_i32");
    // Even with --print-main, C backend uses exit-code semantics for I32
    assert_eq!(run_c_exit(&binary), 42);
}

#[test]
fn c_build_print_main_text_stdout() {
    // Text main always prints to stdout regardless of --print-main,
    // since SARIF_MAIN_KIND=3 unconditionally writes the text.
    let binary = c_build_print_main(
        "fn main() -> Text { text_concat(\"sa\", \"rif\") }",
        "c_build_print_main_text",
    );
    assert_eq!(run_c_stdout(&binary), "sarif");
}
