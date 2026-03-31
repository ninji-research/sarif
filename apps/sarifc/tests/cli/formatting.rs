use super::support::{
    bootstrap_format_parity_inputs, bootstrap_syntax_dir, const_control_flow_example, fixture,
    multi_file_package_dir, multi_file_package_manifest, package_dir, package_manifest,
    run_path_profiled, temp_package, temp_source,
};

fn assert_text_eq_with_context(actual: &str, expected: &str) {
    if actual == expected {
        return;
    }

    let byte = actual
        .bytes()
        .zip(expected.bytes())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| actual.len().min(expected.len()));
    let line = actual[..byte].bytes().filter(|byte| *byte == b'\n').count() + 1;
    let actual_line = actual.lines().nth(line - 1).unwrap_or("");
    let expected_line = expected.lines().nth(line - 1).unwrap_or("");
    let actual_start = byte.saturating_sub(40);
    let expected_start = byte.saturating_sub(40);
    let actual_end = (byte + 40).min(actual.len());
    let expected_end = (byte + 40).min(expected.len());
    let actual_window = &actual[actual_start..actual_end];
    let expected_window = &expected[expected_start..expected_end];

    panic!(
        "text mismatch at byte {byte}, line {line}\nactual:   {actual_line:?}\nexpected: {expected_line:?}\nactual window:   {actual_window:?}\nexpected window: {expected_window:?}"
    );
}

#[test]
fn format_canonicalizes_mutable_locals() {
    let path = temp_source("fn main() -> I32 { let mut total = 20; total = total + 22; total }");
    let format = run_path_profiled("format", &path, "core");

    assert!(format.status.success());
    assert_eq!(
        String::from_utf8_lossy(&format.stdout).trim(),
        "fn main() -> I32 {\n    let mut total = 20;\n    total = total + 22;\n    total\n}"
    );
}

#[test]
fn format_keeps_long_binary_chains_multiline_in_bodies() {
    let path = temp_source(
        "fn main() -> Bool { true and false and true }\nfn score() -> I32 { 1 + 2 + 3 + 4 }",
    );
    let format = run_path_profiled("format", &path, "core");

    assert!(format.status.success());
    assert_eq!(
        String::from_utf8_lossy(&format.stdout).trim(),
        "fn main() -> Bool {\n    true and\n    false and\n    true\n}\n\nfn score() -> I32 {\n    1 +\n    2 +\n    3 +\n    4\n}"
    );
}

#[test]
fn format_is_idempotent_for_multi_item_programs() {
    let source = "enum Flag { on, off }\nconst answer: I32 = 42;\nfn main() -> I32 effects [parallel, io] { if true { answer } else { 0 } }";
    let first_path = temp_source(source);
    let first = run_path_profiled("format", &first_path, "core");

    assert!(first.status.success());
    let formatted = String::from_utf8_lossy(&first.stdout).into_owned();
    let second_path = temp_source(&formatted);
    let second = run_path_profiled("format", &second_path, "core");

    assert!(second.status.success());
    assert_eq!(formatted, String::from_utf8_lossy(&second.stdout));
}

#[test]
fn format_accepts_package_inputs() {
    for path in [
        package_dir(),
        package_manifest(),
        multi_file_package_dir(),
        multi_file_package_manifest(),
    ] {
        let output = run_path_profiled("format", &path, "core");
        assert!(
            output.status.success(),
            "{} should format cleanly",
            path.display()
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("fn main() -> I32"));
    }
}

#[test]
fn format_keeps_shipped_control_flow_example_stable() {
    let path = const_control_flow_example();
    let output = run_path_profiled("format", &path, "core");

    assert!(output.status.success());
    let formatted = String::from_utf8_lossy(&output.stdout);
    let expected = std::fs::read_to_string(fixture("const_control_flow.format.sarif"))
        .expect("fixture should be readable");
    assert_text_eq_with_context(&formatted, &expected);
}

#[test]
fn format_keeps_bootstrap_syntax_stable_and_idempotent() {
    let path = bootstrap_syntax_dir();
    let output = run_path_profiled("format", &path, "core");

    assert!(output.status.success());
    let formatted = String::from_utf8_lossy(&output.stdout).into_owned();
    let expected = std::fs::read_to_string(fixture("bootstrap_syntax.format.sarif"))
        .expect("fixture should be readable");
    assert_text_eq_with_context(&formatted, &expected);

    let rerun = run_path_profiled("format", &temp_source(&formatted), "core");
    assert!(rerun.status.success());
    assert_text_eq_with_context(&formatted, &String::from_utf8_lossy(&rerun.stdout));
}

#[test]
fn format_preserves_precedence_sensitive_parentheses() {
    let path = temp_source("fn main() -> I32 { 20 - (10 - 3) / (1 + 2) }");
    let output = run_path_profiled("format", &path, "core");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "fn main() -> I32 {\n    20 - (10 - 3) / (1 + 2)\n}"
    );
}

#[test]
fn format_accepts_float_literals_and_exponents() {
    let path = temp_source("fn main() -> F64 { -7.0 + 1.25e-2 }");
    let output = run_path_profiled("format", &path, "core");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "fn main() -> F64 {\n    -7.0 + 0.0125\n}"
    );
}

#[test]
fn format_multi_file_packages_respects_file_boundaries() {
    let package = temp_package(
        "[package]\nname = \"format-boundaries\"\nversion = \"0.1.0\"\nsources = [\"src/functions.sarif\", \"src/consts.sarif\"]\n",
        &[
            ("src/functions.sarif", "fn helper() -> I32 { 1 }\n"),
            ("src/consts.sarif", "const answer: I32 = 41;\n"),
        ],
    );
    let output = run_path_profiled("format", &package, "core");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "fn helper() -> I32 {\n    1\n}\n\nconst answer: I32 = 41;\n"
    );
}

#[test]
fn bootstrap_format_matches_maintained_formatter_for_shipped_inputs() {
    let handles = bootstrap_format_parity_inputs()
        .into_iter()
        .map(|path| {
            std::thread::spawn(move || {
                let maintained = run_path_profiled("format", &path, "core");
                let bootstrap = run_path_profiled("bootstrap-format", &path, "core");

                assert!(
                    maintained.status.success(),
                    "{} should format cleanly",
                    path.display()
                );
                assert!(
                    bootstrap.status.success(),
                    "{} should bootstrap-format cleanly",
                    path.display()
                );
                assert_text_eq_with_context(
                    &String::from_utf8_lossy(&bootstrap.stdout),
                    &String::from_utf8_lossy(&maintained.stdout),
                );
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle
            .join()
            .expect("bootstrap-format parity worker should run");
    }
}
