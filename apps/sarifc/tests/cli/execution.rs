use super::support::{
    bootstrap_syntax_dir, bootstrap_tools_dir, const_control_flow_example, multi_file_package_dir,
    multi_file_package_manifest, package_dir, package_manifest, run_build_profiled,
    run_path_profiled, run_profiled, run_sarif, temp_output, temp_source,
};
use std::io::Write;
use std::process::{Command, Stdio};
#[cfg(feature = "wasm")]
use wasmtime::{Engine, Instance, Module, Store, TypedFunc};

fn assert_run_parity(source: &str, expected: &str) {
    let path = temp_source(source);
    let output = run_profiled("run", &path);
    assert!(
        output.status.success(),
        "run should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
}

fn assert_run_path(path: &std::path::Path, expected: &str) {
    let output = run_path_profiled("run", path, "core");
    assert!(
        output.status.success(),
        "{} should run successfully",
        path.display()
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
}

#[cfg(feature = "wasm")]
fn run_wasm_main(path: &std::path::Path) -> Result<i64, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("failed to read wasm artifact: {error}"))?;
    let engine = Engine::default();
    let module = Module::new(&engine, bytes)
        .map_err(|error| format!("failed to compile wasm artifact: {error}"))?;
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[])
        .map_err(|error| format!("failed to instantiate wasm artifact: {error}"))?;
    let main: TypedFunc<(), i64> = instance
        .get_typed_func(&mut store, "main")
        .map_err(|error| format!("failed to load wasm `main`: {error}"))?;
    main.call(&mut store, ())
        .map_err(|error| format!("wasm call failed: {error}"))
}

#[test]
fn run_supports_mutable_locals() {
    assert_run_parity(
        "fn main() -> I32 { let mut total = 20; total = total + 22; total }",
        "42",
    );
}

#[test]
fn run_accepts_expression_bodied_functions() {
    assert_run_parity(
        "fn add(left: I32, right: I32) -> I32 = left + right;\nfn main() -> I32 = add(20, 22);",
        "42",
    );
}

#[test]
fn run_accepts_compound_assignments() {
    assert_run_parity(
        "fn main() -> I32 { let mut total = 20; total += 22; total }",
        "42",
    );
}

#[test]
fn run_executes_nested_mutation_consistently() {
    assert_run_parity(
        "fn main() -> I32 { let mut total = 20; if true { total = total + 22; }; total }",
        "42",
    );
}

#[test]
fn run_executes_else_if_chains_consistently() {
    assert_run_parity(
        "fn main() -> I32 { if false { 0 } else if true { 42 } else { 7 } }",
        "42",
    );
}

#[test]
fn run_executes_not_before_calls_consistently() {
    assert_run_parity(
        "fn flag() -> Bool { false }\nfn main() -> Bool { not flag() }",
        "true",
    );
}

#[test]
fn run_executes_scalar_match_consistently() {
    assert_run_parity(
        "fn pick(code: I32) -> I32 { match code { 40 => { 1 }, 41 => { 2 }, _ => { 3 }, } }\nfn is_sarif(word: Text) -> Bool { match word { \"sarif\" => { true }, _ => { false }, } }\nfn main() -> I32 { if match true { true => { pick(41) == 2 and is_sarif(\"sarif\") }, false => { false }, } { 42 } else { 0 } }",
        "42",
    );
}

#[test]
fn run_executes_indexed_repeat_consistently() {
    assert_run_parity(
        "fn main() -> I32 { let xs = [20, 22]; let mut total = 0; repeat i in len(xs) { total = total + xs[i]; }; total }",
        "42",
    );
}

#[test]
fn run_executes_while_loops_consistently() {
    assert_run_parity(
        "fn main() -> I32 { let mut n = 0; let mut total = 0; while n < 2 { total = total + 21; n = n + 1; }; total }",
        "42",
    );
}

#[test]
fn run_executes_text_concat_consistently() {
    assert_run_parity(
        "fn main() -> Text { text_concat(\"\", text_concat(\"sa\", text_concat(\"rif\", \"\"))) }",
        "sarif",
    );
}

#[test]
fn run_executes_text_slice_consistently() {
    assert_run_parity(
        "fn main() -> Bool { text_slice(\"sarif\", 0, 5) == \"sarif\" and text_slice(\"sarif\", 1, 4) == \"ari\" and text_slice(\"sarif\", 3, 99) == \"if\" and text_slice(\"sarif\", 4, 2) == \"\" }",
        "true",
    );
}

#[test]
fn run_executes_text_eq_range_consistently() {
    assert_run_parity(
        "fn main() -> Bool { text_eq_range(\"sarif\", 1, 4, \"ari\") and text_eq_range(\"sarif\", 3, 99, \"if\") and text_eq_range(\"sarif\", 4, 2, \"\") }",
        "true",
    );
}

#[test]
fn run_executes_bytes_builtins_consistently() {
    let path = temp_source(
        "fn main() -> I32 { let xs = stdin_bytes(); if bytes_len(xs) == 6 and bytes_byte(xs, 0) == 115 and bytes_find_byte_range(xs, 0, bytes_len(xs), 105) == 3 and bytes_len(bytes_slice(xs, 1, 4)) == 3 { 0 } else { 1 } }",
    );
    let mut run = Command::new(std::env::var("CARGO_BIN_EXE_sarifc").expect("sarifc binary"));
    run.arg("run")
        .arg(path.to_str().expect("utf-8 path"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = run.spawn().expect("sarifc run should spawn");
    child
        .stdin
        .take()
        .expect("stdin pipe should exist")
        .write_all(b"sarif\n")
        .expect("stdin should be writable");
    let output = child
        .wait_with_output()
        .expect("sarifc run should complete");

    assert!(output.status.success(), "bytes run should succeed");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
}

#[test]
fn run_executes_not_consistently() {
    assert_run_parity("fn main() -> Bool { not false and true }", "true");
}

#[test]
fn run_executes_text_builder_consistently() {
    assert_run_parity(
        "fn main() -> Text effects [alloc] { let mut builder = text_builder_new(); builder = text_builder_append(builder, \"sa\"); builder = text_builder_append(builder, text_slice(\"sarif\", 2, 5)); text_builder_finish(builder) }",
        "sarif",
    );
}

#[test]
fn run_executes_record_field_punning_consistently() {
    assert_run_parity(
        "struct Pair { left: I32, right: I32 }\nfn main() -> I32 { let left = 7; let right = 9; let pair = Pair { left, right }; pair.left + pair.right }",
        "16",
    );
}

#[test]
fn run_executes_record_field_assignment_consistently() {
    assert_run_parity(
        "struct Pair { left: I32, right: I32 }\nfn main() -> I32 { let mut pair = Pair { left: 7, right: 9 }; pair.left = 20; pair.left + pair.right }",
        "29",
    );
}

#[test]
fn run_executes_nested_record_field_assignment_consistently() {
    assert_run_parity(
        "struct Pair { left: I32, right: I32 }\nfn main() -> I32 { let mut pairs = [Pair { left: 7, right: 9 }, Pair { left: 1, right: 2 }]; pairs[0].left = 20; pairs[0].left + pairs[1].right }",
        "22",
    );
}

#[test]
fn run_executes_list_f64_consistently() {
    assert_run_parity(
        "fn main() -> Text effects [alloc] { let mut xs = list_new(3, 0.0); xs = list_set(xs, 0, 1.5); xs = list_set(xs, 1, 2.25); xs = list_set(xs, 2, list_get(xs, 0) + list_get(xs, 1)); text_from_f64_fixed(list_get(xs, 2), 2) }",
        "3.75",
    );
}

#[test]
fn run_executes_f64_from_i32_consistently() {
    assert_run_parity(
        "fn main() -> Text { text_from_f64_fixed(f64_from_i32(7) / 2.0, 1) }",
        "3.5",
    );
}

#[test]
fn run_executes_top_level_float_consts_consistently() {
    assert_run_parity(
        "const X: F64 = 3.5;\nfn main() -> Text { text_from_f64_fixed(X, 1) }",
        "3.5",
    );
}

#[test]
fn run_executes_top_level_array_consts_consistently() {
    assert_run_parity(
        "const XS: [I32; 2] = [20, 22];\nfn main() -> I32 { XS[0] + XS[1] }",
        "42",
    );
}

#[test]
fn run_executes_top_level_comptime_consts_consistently() {
    assert_run_parity(
        "const X: I32 = comptime { 20 + 22 };\nfn main() -> I32 { X }",
        "42",
    );
}

#[test]
fn run_executes_text_from_f64_fixed_consistently() {
    assert_run_parity("fn main() -> Text { text_from_f64_fixed(3.5, 2) }", "3.50");
}

#[test]
fn run_executes_parse_i32_range_consistently() {
    assert_run_parity(
        "fn main() -> I32 { parse_i32_range(\"xx-42yy\", 2, 5) + parse_i32_range(\"0017\", 0, 4) }",
        "-25",
    );
}

#[test]
fn run_executes_alloc_scopes_consistently() {
    assert_run_parity(
        "enum Tree { leaf, branch(Branch) }\nstruct Branch { left: Tree, right: Tree }\nfn count(tree: Tree) -> I32 { match tree { Tree.leaf => { 1 }, Tree.branch(node) => { 1 + count(node.left) + count(node.right) }, } }\nfn build(depth: I32) -> Tree effects [alloc] { if depth > 0 { Tree.branch(Branch { left: build(depth - 1), right: build(depth - 1) }) } else { Tree.leaf } }\nfn main() -> I32 effects [alloc] { alloc_push(); let first = count(build(4)); alloc_pop(); let second = count(build(3)); first + second }",
        "46",
    );
}

#[test]
fn run_executes_float_sqrt_pipeline_consistently() {
    assert_run_parity(
        "fn main() -> Text { text_from_f64_fixed(sqrt(9.0) + 0.125, 3) }",
        "3.125",
    );
}

#[test]
fn run_passes_runtime_arguments_to_argument_builtins() {
    let path =
        temp_source("fn main() -> Text { if arg_count() > 1 { arg_text(1) } else { \"\" } }");
    let output = run_sarif(&["run", path.to_str().expect("utf-8 path"), "--", "sarif"]);

    assert!(
        output.status.success(),
        "run with runtime args should succeed"
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "sarif");
}

#[test]
fn run_executes_stdout_write_consistently() {
    let path = temp_source("fn main() { stdout_write(\"sarif\") }");
    let output = run_sarif(&["run", path.to_str().expect("utf-8 path")]);

    assert!(output.status.success(), "stdout_write run should succeed");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "sarif");
}

#[test]
fn run_executes_nested_arrays_consistently() {
    assert_run_parity(
        "fn main() -> I32 { let xs = [[20, 22], [0, 0]]; xs[0][0] + xs[0][1] }",
        "42",
    );
}

#[test]
fn run_executes_mutable_nested_arrays_consistently() {
    assert_run_parity(
        "fn main() -> I32 { let mut xs = [[0, 0], [0, 0]]; xs = [[20, 22], [0, 0]]; xs[0][0] + xs[0][1] }",
        "42",
    );
}

#[test]
fn run_executes_mutable_array_element_assignment_consistently() {
    assert_run_parity(
        "fn main() -> I32 { let mut xs = [0, 0]; xs[0] = 20; xs[1] = 22; xs[0] + xs[1] }",
        "42",
    );
}

#[test]
fn run_executes_const_generic_array_lengths_as_i32_values() {
    assert_run_parity(
        "fn sum[N](xs: [I32; N]) -> I32 { let mut total = 0; repeat i in N { total += xs[i]; }; total }\nfn main() -> I32 { sum([10, 10, 10, 12]) }",
        "42",
    );
}

#[test]
fn run_executes_explicit_array_types_consistently() {
    assert_run_parity(
        "struct Grid { rows: [[I32; 2]; 2], }\nfn first(xs: [I32; 2]) -> I32 { xs[0] + xs[1] }\nfn main() -> I32 { let grid = Grid { rows: [[20, 22], [0, 0]] }; first(grid.rows[0]) }",
        "42",
    );
}

#[test]
fn run_executes_repeat_array_literals_consistently() {
    assert_run_parity(
        "fn first_repeat[N](xs: [I32; N]) -> I32 { let ys = [xs[0]; N]; ys[0] }\nfn main() -> I32 { first_repeat([42]) }",
        "42",
    );
}

#[test]
fn run_reports_array_bounds_failures() {
    let path = temp_source("fn main() -> I32 { let xs = [20, 22]; xs[2] }");
    let output = run_profiled("run", &path);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("bounds assertion failed"));
}

#[test]
fn run_accepts_package_inputs() {
    for path in [
        package_dir(),
        package_manifest(),
        multi_file_package_dir(),
        multi_file_package_manifest(),
    ] {
        assert_run_path(&path, "42");
    }
}

#[test]
fn run_executes_const_control_flow_example() {
    assert_run_path(&const_control_flow_example(), "42");
}

#[test]
fn run_accepts_bootstrap_packages() {
    for (path, expected) in [(bootstrap_syntax_dir(), "32"), (bootstrap_tools_dir(), "8")] {
        assert_run_path(&path, expected);
    }
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_emits_a_runnable_binary() {
    let path = temp_source("fn main() -> I32 { 42 }");
    let binary_path = super::support::temp_artifact("stable_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    if !build.status.success() {
        eprintln!("STDOUT: {}", String::from_utf8_lossy(&build.stdout));
        eprintln!("STDERR: {}", String::from_utf8_lossy(&build.stderr));
    }
    assert!(build.status.success());
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert_eq!(native.status.code(), Some(42));
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_print_main_emits_i32_stdout() {
    let path = temp_source("fn main() -> I32 { 42 }");
    let binary_path = super::support::temp_artifact("stable_build_print_main", "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--profile",
        "core",
        "--print-main",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success());
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "42\n");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_print_main_emits_bool_stdout() {
    let path = temp_source("fn main() -> Bool { true }");
    let binary_path = super::support::temp_artifact("stable_build_print_main_bool", "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--profile",
        "core",
        "--print-main",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success());
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "true\n");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_print_main_handles_fixed_array_values() {
    let path = temp_source(
        "fn score(xs: [F64; 5], ys: [I32; 4]) -> F64 { xs[0] + xs[4] + f64_from_i32(ys[0] + ys[3]) }\nfn main() -> Text { let xs = [1.5, 0.0, 0.0, 0.0, 2.5]; let ys = [10, 0, 0, 28]; text_from_f64_fixed(score(xs, ys), 1) }",
    );
    let binary_path = super::support::temp_artifact("stable_build_print_main_arrays", "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--profile",
        "core",
        "--print-main",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success());
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "42.0");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_print_main_handles_signature_only_fixed_arrays() {
    let path = temp_source(
        "fn score(xs: [F64; 5], ys: [I32; 4]) -> F64 { xs[0] + xs[4] + f64_from_i32(ys[0] + ys[3]) }\nfn main() -> I32 { 0 }",
    );
    let binary_path =
        super::support::temp_artifact("stable_build_print_main_signature_only_arrays", "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--profile",
        "core",
        "--print-main",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success());
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(native.status.code(), Some(0));
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_print_main_handles_mutable_fixed_array_loops() {
    let path = temp_source(
        "fn sum(xs: [I32; 4]) -> I32 { let mut total = 0; repeat i in 4 { total += xs[i]; }; total }\nfn main() -> I32 { let mut xs = [1, 2, 3, 4]; repeat i in 4 { xs[i] += 8; }; sum(xs) }",
    );
    let binary_path =
        super::support::temp_artifact("stable_build_print_main_mutable_arrays", "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--profile",
        "core",
        "--print-main",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success());
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "42\n");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_print_main_handles_inferred_const_generic_arrays() {
    let path = temp_source(
        "fn sum[N](xs: [I32; N]) -> I32 { let mut total = 0; repeat i in N { total += xs[i]; }; total }\nfn main() -> I32 { let xs = [10, 10, 10, 12]; sum(xs) }",
    );
    let binary_path =
        super::support::temp_artifact("stable_build_print_main_generic_arrays", "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--profile",
        "core",
        "--print-main",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success());
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "42\n");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_print_main_handles_repeat_array_literals() {
    let path = temp_source(
        "fn first_repeat[N](xs: [I32; N]) -> I32 { let ys = [xs[0]; N]; ys[0] }\nfn main() -> I32 { first_repeat([42]) }",
    );
    let binary_path = super::support::temp_artifact("stable_build_print_main_repeat_arrays", "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--profile",
        "core",
        "--print-main",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success());
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "42\n");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_accepts_shipped_and_bootstrap_inputs() {
    for (path, expected) in [
        (package_dir(), 42),
        (package_manifest(), 42),
        (multi_file_package_dir(), 42),
        (multi_file_package_manifest(), 42),
        (const_control_flow_example(), 42),
        (bootstrap_syntax_dir(), 32),
        (bootstrap_tools_dir(), 8),
    ] {
        let binary_path = super::support::temp_artifact("package_build", "bin");
        let build = run_build_profiled(&path, &binary_path, "core");

        assert!(
            build.status.success(),
            "{} should build on the native target",
            path.display()
        );
        let native = Command::new(&binary_path)
            .output()
            .expect("built binary should run");
        assert_eq!(native.status.code(), Some(expected));
    }
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_prints_payload_enum_main_results() {
    let path = temp_source(
        "enum OptionText { none, some(Text) }\nfn main() -> OptionText { OptionText.some(\"hello\") }",
    );
    let binary_path = super::support::temp_artifact("payload_enum_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "payload enum main should build on the native target"
    );
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(
        native.status.success(),
        "built binary should print successfully"
    );
    assert_eq!(
        String::from_utf8_lossy(&native.stdout),
        "OptionText.some(hello)\n"
    );
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_passes_process_arguments_to_argument_builtins() {
    let path =
        temp_source("fn main() -> Text { if arg_count() > 1 { arg_text(1) } else { \"\" } }");
    let binary_path = super::support::temp_artifact("arg_text_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "arg_text text main should build on the native target"
    );
    let native = Command::new(&binary_path)
        .arg("sarif")
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "sarif");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_reads_stdin_text() {
    let path = temp_source("fn main() -> Text { stdin_text() }");
    let binary_path = super::support::temp_artifact("stdin_text_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "stdin_text program should build on the native target"
    );
    let mut native = Command::new(&binary_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("built binary should spawn");
    native
        .stdin
        .take()
        .expect("stdin pipe should exist")
        .write_all(b">id\nACGT\n")
        .expect("stdin should be writable");
    let output = native.wait_with_output().expect("built binary should run");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), ">id\nACGT\n");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_executes_bytes_programs() {
    let path = temp_source(
        "fn main() -> I32 { let xs = stdin_bytes(); if bytes_len(xs) == 6 and bytes_byte(xs, 0) == 115 and bytes_find_byte_range(xs, 0, bytes_len(xs), 105) == 3 and bytes_len(bytes_slice(xs, 1, 4)) == 3 { 0 } else { 1 } }",
    );
    let binary_path = super::support::temp_artifact("stdin_bytes_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "bytes program should build on the native target"
    );
    let mut native = Command::new(&binary_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("built binary should spawn");
    native
        .stdin
        .take()
        .expect("stdin pipe should exist")
        .write_all(b"sarif\n")
        .expect("stdin should be writable");
    let output = native.wait_with_output().expect("built binary should run");
    assert_eq!(output.status.code(), Some(0));
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_streams_stdout_write() {
    let path = temp_source("fn main() { stdout_write(\"sarif\") }");
    let binary_path = super::support::temp_artifact("stdout_write_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "stdout_write program should build on the native target"
    );
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "sarif");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_executes_text_builder_programs() {
    let path = temp_source(
        "fn main() -> Text effects [alloc] { let mut builder = text_builder_new(); builder = text_builder_append(builder, \"sa\"); builder = text_builder_append(builder, text_slice(\"sarif\", 2, 5)); text_builder_finish(builder) }",
    );
    let binary_path = super::support::temp_artifact("text_builder_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "text builder program should build on the native target"
    );
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "sarif");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_executes_list_f64_programs() {
    let path = temp_source(
        "fn main() -> Text effects [alloc] { let mut xs = list_new(3, 0.0); xs = list_set(xs, 0, 1.5); xs = list_set(xs, 1, 2.25); xs = list_set(xs, 2, list_get(xs, 0) + list_get(xs, 1)); text_from_f64_fixed(list_get(xs, 2), 2) }",
    );
    let binary_path = super::support::temp_artifact("list_f64_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "list f64 program should build on the native target"
    );
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "3.75");
}

#[test]
fn stable_build_executes_f64_from_i32_programs() {
    let path = temp_source("fn main() -> Text { text_from_f64_fixed(f64_from_i32(7) / 2.0, 1) }");
    let binary_path = super::support::temp_artifact("f64_from_i32_build", "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--print-main",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);
    assert!(
        build.status.success(),
        "f64_from_i32 program should build on the native target"
    );
    let native = Command::new(&binary_path)
        .output()
        .expect("run built native binary");
    assert!(native.status.success(), "built binary should succeed");
    assert_eq!(String::from_utf8_lossy(&native.stdout), "3.5");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_executes_payload_enum_equality_programs() {
    let path = temp_source(
        "enum OptionText { none, some(Text) }\nfn main() -> Bool { OptionText.some(\"hello\") == OptionText.some(\"hello\") }",
    );
    let binary_path = super::support::temp_artifact("payload_enum_eq_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "payload enum equality program should build on the native target"
    );
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert_eq!(native.status.code(), Some(0));
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_executes_text_slice_programs() {
    let path = temp_source(
        "fn main() -> Bool { text_slice(\"sarif\", 1, 4) == \"ari\" and text_slice(\"sarif\", 3, 99) == \"if\" and text_slice(\"sarif\", 4, 2) == \"\" }",
    );
    let binary_path = super::support::temp_artifact("text_slice_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "text slice program should build on the native target"
    );
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert_eq!(native.status.code(), Some(0));
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_executes_text_from_f64_fixed_programs() {
    let path = temp_source("fn main() -> Text { text_from_f64_fixed(3.5, 2) }");
    let binary_path = super::support::temp_artifact("text_from_f64_fixed_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "text_from_f64_fixed program should build on the native target"
    );
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "3.50");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_executes_float_sqrt_pipeline_programs() {
    let path = temp_source("fn main() -> Text { text_from_f64_fixed(sqrt(9.0) + 0.125, 3) }");
    let binary_path = super::support::temp_artifact("float_sqrt_pipeline_build", "bin");
    let build = run_build_profiled(&path, &binary_path, "core");

    assert!(
        build.status.success(),
        "float sqrt pipeline program should build on the native target"
    );
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "3.125");
}

#[cfg(feature = "native-build")]
#[test]
fn stable_build_print_main_emits_f64_stdout() {
    let path = temp_source("fn main() -> F64 { sqrt(2.25) }");
    let binary_path = super::support::temp_artifact("stable_build_print_main_f64", "bin");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--profile",
        "core",
        "--print-main",
        "-o",
        binary_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success());
    let native = Command::new(&binary_path)
        .output()
        .expect("built binary should run");
    assert!(native.status.success());
    assert_eq!(String::from_utf8_lossy(&native.stdout), "1.5\n");
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_emits_a_runnable_module() {
    let path = temp_source("fn main() -> I32 { 42 }");
    let wasm_path = temp_output("stable_build", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success(), "wasm build should succeed");
    let bytes = std::fs::read(&wasm_path).expect("wasm artifact should exist");
    assert!(bytes.starts_with(b"\0asm"));
    assert_eq!(
        run_wasm_main(&wasm_path).expect("built wasm should run"),
        42
    );
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_accepts_text_kernel_modules() {
    let path = temp_source(
        "fn main() -> I32 {\n  if text_cmp(\"abc\", \"abd\") < 0 and\n     text_eq_range(\"abc\", 0, 2, \"ab\") and\n     text_find_byte_range(\"a,b\", 0, 3, 44) == 1 and\n     text_line_end(\"a\\nb\", 0) == 1 and\n     text_next_line(\"a\\nb\", 0) == 2 and\n     text_field_end(\"aa,bb\", 0, 5, 44) == 2 and\n     text_next_field(\"aa,bb\", 0, 5, 44) == 3 and\n     parse_i32_range(\"17\", 0, 2) == 17 {\n    42\n  } else {\n    0\n  }\n}",
    );
    let wasm_path = temp_output("text_kernel_build", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success(), "wasm text kernels should build");
    assert_eq!(
        run_wasm_main(&wasm_path).expect("built wasm should run"),
        42
    );
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_rejects_runtime_argument_builtins() {
    let path = temp_source("fn main() -> Text { arg_text(1) }");
    let wasm_path = temp_output("arg_text_build", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        !build.status.success(),
        "arg_text should be rejected on the wasm backend for now"
    );
    assert!(
        String::from_utf8_lossy(&build.stderr)
            .contains("wasm backend does not yet support runtime input builtins"),
        "wasm rejection should explain the current stage-0 backend limitation"
    );
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_rejects_stdin_text_modules() {
    let path = temp_source("fn main() -> Text { stdin_text() }");
    let wasm_path = temp_output("stdin_text_build", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        !build.status.success(),
        "stdin_text should be rejected on the wasm backend for now"
    );
    assert!(
        String::from_utf8_lossy(&build.stderr)
            .contains("wasm backend does not yet support runtime input builtins"),
        "wasm rejection should explain the current stage-0 backend limitation"
    );
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_rejects_stdin_bytes_modules() {
    let path = temp_source(
        "fn main() -> Bool { let xs = stdin_bytes(); bytes_len(xs) == 0 and bytes_len(bytes_slice(xs, 0, 0)) == 0 }",
    );
    let wasm_path = temp_output("stdin_bytes_build", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        !build.status.success(),
        "stdin_bytes should be rejected on the wasm backend for now"
    );
    assert!(
        String::from_utf8_lossy(&build.stderr)
            .contains("wasm backend does not yet support runtime input builtins"),
        "wasm rejection should explain the current runtime input backend limitation"
    );
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_rejects_stdout_write_modules() {
    let path = temp_source("fn main() { stdout_write(\"sarif\") }");
    let wasm_path = temp_output("stdout_write_build", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        !build.status.success(),
        "stdout_write should be rejected on the wasm backend for now"
    );
    assert!(
        String::from_utf8_lossy(&build.stderr)
            .contains("wasm backend does not yet support runtime io builtins"),
        "wasm rejection should explain the current stage-0 backend limitation"
    );
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_rejects_text_builder_modules() {
    let path = temp_source(
        "fn main() -> Text effects [alloc] { let mut builder = text_builder_new(); builder = text_builder_append(builder, \"sarif\"); text_builder_finish(builder) }",
    );
    let wasm_path = temp_output("text_builder_build", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        !build.status.success(),
        "text builder builtins should be rejected on the wasm backend for now"
    );
    assert!(
        String::from_utf8_lossy(&build.stderr)
            .contains("wasm backend does not yet support text builder builtins"),
        "wasm rejection should explain the current stage-0 backend limitation"
    );
}

#[test]
#[cfg(feature = "wasm")]
fn wasm_build_accepts_list_f64_modules() {
    let path = temp_source("fn carry(xs: List[F64]) -> List[F64] { xs }\nfn main() { }");
    let wasm_path = temp_output("list_f64_build", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 output path"),
    ]);

    assert!(
        build.status.success(),
        "list f64 builtins should be accepted on the wasm backend:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_rejects_text_from_f64_fixed_modules() {
    let path = temp_source("fn main() -> Text { text_from_f64_fixed(3.5, 2) }");
    let wasm_path = temp_output("text_from_f64_fixed_build", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        !build.status.success(),
        "text_from_f64_fixed should be rejected on the wasm backend for now"
    );
    assert!(
        String::from_utf8_lossy(&build.stderr)
            .contains("wasm backend does not yet support `text_from_f64_fixed` in stage-0"),
        "wasm rejection should explain the current stage-0 float limitation"
    );
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_accepts_sqrt_modules() {
    let path = temp_source("fn main() -> F64 { sqrt(2.25) }");
    let wasm_path = temp_output("sqrt_build", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        build.status.success(),
        "sqrt should be accepted on the wasm backend:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_accepts_package_inputs() {
    for (path, expected) in [
        (package_dir(), 42),
        (package_manifest(), 42),
        (multi_file_package_dir(), 42),
        (multi_file_package_manifest(), 42),
        (bootstrap_syntax_dir(), 32),
    ] {
        let wasm_path = temp_output("package_build", "wasm");
        let build = run_sarif(&[
            "build",
            path.to_str().expect("utf-8 path"),
            "--target",
            "wasm",
            "-o",
            wasm_path.to_str().expect("utf-8 path"),
        ]);

        assert!(
            build.status.success(),
            "{} should build to wasm",
            path.display()
        );
        let bytes = std::fs::read(&wasm_path).expect("wasm artifact should exist");
        assert!(bytes.starts_with(b"\0asm"));
        assert_eq!(
            run_wasm_main(&wasm_path).expect("built wasm should run"),
            expected
        );
    }
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_accepts_payload_enum_equality_programs() {
    let path = temp_source(
        "enum OptionText { none, some(Text) }\nfn main() -> Bool { OptionText.some(\"hello\") == OptionText.some(\"hello\") }",
    );
    let wasm_path = temp_output("payload_enum_eq", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        build.status.success(),
        "payload enum equality program should build on the wasm target"
    );
    let bytes = std::fs::read(&wasm_path).expect("wasm artifact should exist");
    assert!(bytes.starts_with(b"\0asm"));
    assert_eq!(run_wasm_main(&wasm_path).expect("built wasm should run"), 1);
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_accepts_text_slice_programs() {
    let path = temp_source(
        "fn main() -> Bool { text_slice(\"sarif\", 1, 4) == \"ari\" and text_slice(\"sarif\", 3, 99) == \"if\" and text_slice(\"sarif\", 4, 2) == \"\" }",
    );
    let wasm_path = temp_output("text_slice", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(
        build.status.success(),
        "text slice program should build on the wasm target"
    );
    let bytes = std::fs::read(&wasm_path).expect("wasm artifact should exist");
    assert!(bytes.starts_with(b"\0asm"));
    assert_eq!(run_wasm_main(&wasm_path).expect("built wasm should run"), 1);
}

#[cfg(feature = "wasm")]
#[test]
fn wasm_build_preserves_runtime_traps() {
    let path = temp_source("fn main() -> I32 { let xs = [20, 22]; xs[2] }");
    let wasm_path = temp_output("bounds_failure", "wasm");
    let build = run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--target",
        "wasm",
        "-o",
        wasm_path.to_str().expect("utf-8 path"),
    ]);

    assert!(build.status.success(), "wasm build should succeed");
    let error = run_wasm_main(&wasm_path).expect_err("built wasm should trap");
    assert!(error.contains("wasm call failed"), "{error}");
    assert!(error.contains("!main"), "{error}");
}
