use super::support::{
    affine_invalid, bootstrap_syntax_dir, bootstrap_tools_dir, const_control_flow_example,
    contract_affine_invalid, contract_invalid, example, multi_file_package_dir,
    multi_file_package_manifest, package_dir, package_manifest, relativize_repo_root, rt_invalid,
    rt_invalid_composites, rt_invalid_text, run_path, run_path_profiled, run_sarif,
    semantic_check_cases, strip_ansi, temp_package, temp_source, total_invalid_repeat,
    total_invalid_while,
};

#[test]
fn check_accepts_the_shipped_example() {
    let output = run_path("check", &example());

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok [core]");
}

#[test]
fn check_accepts_const_control_flow_example() {
    let output = run_path("check", &const_control_flow_example());

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok [core]");
}

#[test]
fn check_accepts_package_inputs() {
    for path in [
        package_dir(),
        package_manifest(),
        multi_file_package_dir(),
        multi_file_package_manifest(),
    ] {
        let output = run_path("check", &path);
        assert!(
            output.status.success(),
            "{} should resolve as a package",
            path.display()
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok [core]");
    }
}

#[test]
fn check_accepts_bootstrap_packages() {
    for path in [bootstrap_syntax_dir(), bootstrap_tools_dir()] {
        let output = run_path("check", &path);
        assert!(
            output.status.success(),
            "{} should resolve as a bootstrap package",
            path.display()
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok [core]");
    }
}

#[test]
fn help_and_version_are_stable() {
    let help = run_sarif(&["help"]);
    let version = run_sarif(&["version"]);

    assert!(help.status.success());
    let help_stdout = String::from_utf8_lossy(&help.stdout);
    assert!(help_stdout.contains("sarifc <command>"));
    assert!(help_stdout.contains("bootstrap-check"));
    assert!(help_stdout.contains("bootstrap-format"));
    assert!(help_stdout.contains("bootstrap-doc"));
    assert!(help_stdout.contains("--print-main"));

    #[cfg(feature = "native-build")]
    assert!(help_stdout.contains("build"));
    #[cfg(not(feature = "native-build"))]
    assert!(!help_stdout.contains("build"));

    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).contains("sarifc 0.1.0"));
}

#[test]
fn cli_reports_option_errors_directly() {
    let bad_profile = run_sarif(&[
        "check",
        example().to_str().expect("utf-8 path"),
        "--profile",
        "fast",
    ]);
    assert!(!bad_profile.status.success());
    assert!(String::from_utf8_lossy(&bad_profile.stderr).contains("unknown profile `fast`"));

    let bad_target = run_sarif(&[
        "build",
        example().to_str().expect("utf-8 path"),
        "--target",
        "bogus",
        "-o",
        "out",
    ]);
    assert!(!bad_target.status.success());
    assert!(String::from_utf8_lossy(&bad_target.stderr).contains("unknown target `bogus`"));

    let missing_output = run_sarif(&["build", example().to_str().expect("utf-8 path")]);
    assert!(!missing_output.status.success());
    assert!(String::from_utf8_lossy(&missing_output.stderr).contains("missing output path"));

    let bad_dump = run_sarif(&[
        "check",
        example().to_str().expect("utf-8 path"),
        "--dump-ir=bogus",
    ]);
    assert!(!bad_dump.status.success());
    assert!(String::from_utf8_lossy(&bad_dump.stderr).contains("unknown IR dump pass `bogus`"));
}

#[test]
fn dump_ir_emits_the_requested_representation() {
    let example = example();

    let resolve = run_sarif(&[
        "check",
        example.to_str().expect("utf-8 path"),
        "--dump-ir=resolve",
    ]);
    assert!(resolve.status.success());
    let resolve_stdout = String::from_utf8_lossy(&resolve.stdout);
    assert!(resolve_stdout.contains("HIR"));
    assert!(resolve_stdout.contains("fn main()"));
    assert!(resolve_stdout.contains("ok [core]"));

    let typecheck = run_sarif(&[
        "check",
        example.to_str().expect("utf-8 path"),
        "--dump-ir=typecheck",
    ]);
    assert!(typecheck.status.success());
    let typecheck_stdout = String::from_utf8_lossy(&typecheck.stdout);
    assert!(typecheck_stdout.contains("# Sarif Semantic Docs"));
    assert!(typecheck_stdout.contains("## fn main"));
    assert!(typecheck_stdout.contains("ok [core]"));

    #[cfg(feature = "codegen")]
    {
        let lower = run_sarif(&[
            "build",
            example.to_str().expect("utf-8 path"),
            "--dump-ir=lower",
            "-o",
            "/tmp/sarif-cli-dump-ir-lower",
        ]);
        assert!(lower.status.success());
        let lower_stdout = String::from_utf8_lossy(&lower.stdout);
        assert!(lower_stdout.contains("MIR"));
        assert!(lower_stdout.contains("fn main() -> I32"));
    }

    #[cfg(all(feature = "codegen", feature = "wasm"))]
    {
        let codegen = run_sarif(&[
            "build",
            example.to_str().expect("utf-8 path"),
            "--target",
            "wasm",
            "--dump-ir=codegen",
            "-o",
            "/tmp/sarif-cli-dump-ir-codegen.wasm",
        ]);
        assert!(codegen.status.success());
        let codegen_stdout = String::from_utf8_lossy(&codegen.stdout);
        assert!(codegen_stdout.contains("(module"));
        assert!(codegen_stdout.contains("(memory (export \"memory\")"));
    }
}

#[test]
fn profile_checks_reject_invalid_examples() {
    for (profile, path, code) in [
        ("rt", rt_invalid(), "semantic.rt-effect"),
        ("rt", rt_invalid_text(), "semantic.rt-type"),
        ("rt", rt_invalid_composites(), "semantic.rt-type"),
        ("total", total_invalid_repeat(), "semantic.total-loop"),
        ("total", total_invalid_while(), "semantic.total-loop"),
    ] {
        let output = run_path_profiled("check", &path, profile);
        assert!(
            !output.status.success(),
            "{profile} invalid example should fail"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "expected {code} in diagnostics for {profile} invalid example"
        );
    }
}

#[test]
fn contract_and_affine_failures_remain_reported() {
    for (path, code) in [
        (contract_invalid(), "semantic.contract-result-context"),
        (affine_invalid(), "semantic.affine-reuse"),
        (contract_affine_invalid(), "semantic.contract-affine-move"),
    ] {
        let output = run_path_profiled("check", &path, "core");
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(code));
    }
}

#[test]
fn mutation_diagnostics_are_specific() {
    let immutable = temp_source("fn main() -> I32 { let total = 20; total = total + 22; total }");
    let immutable_output = run_path_profiled("check", &immutable, "core");
    assert!(!immutable_output.status.success());
    assert!(
        String::from_utf8_lossy(&immutable_output.stderr).contains("semantic.assign-immutable")
    );

    let affine = temp_source("fn main() -> Text { let mut text = \"hello\"; text }");
    let affine_output = run_path_profiled("check", &affine, "core");
    assert!(affine_output.status.success());

    let nested = temp_source(
        "fn main() -> I32 { let mut total = 20; if true { total = total + 22; }; total }",
    );
    let nested_output = run_path_profiled("check", &nested, "core");
    assert!(nested_output.status.success());

    let not_bool = temp_source("fn main() -> Bool { not false and true }");
    let not_bool_output = run_path_profiled("check", &not_bool, "core");
    assert!(not_bool_output.status.success());

    let not_int = temp_source("fn main() -> Bool { not 1 }");
    let not_int_output = run_path_profiled("check", &not_int, "core");
    assert!(!not_int_output.status.success());
    let not_int_stderr = strip_ansi(&String::from_utf8_lossy(&not_int_output.stderr));
    assert!(not_int_stderr.contains("semantic.binary-type"));
    assert!(not_int_stderr.contains("Use a boolean operand with `not`."));

    let repeat_text = temp_source(
        "fn main() -> Text { let mut text = \"\"; repeat 3 { text = text_concat(text, \"a\"); }; text }",
    );
    let repeat_output = run_path_profiled("check", &repeat_text, "core");
    assert!(repeat_output.status.success());

    let array_mutation = temp_source(
        "fn main() -> I32 { let mut xs = [0, 0]; xs[0] = 20; xs[1] = 22; xs[0] + xs[1] }",
    );
    let array_mutation_output = run_path_profiled("check", &array_mutation, "core");
    assert!(array_mutation_output.status.success());

    let const_generic_arrays = temp_source(
        "fn first[N](xs: [I32; N]) -> I32 { xs[0] }\nfn main() -> I32 { let xs = [42]; first(xs) }",
    );
    let const_generic_arrays_output = run_path_profiled("check", &const_generic_arrays, "core");
    assert!(const_generic_arrays_output.status.success());

    let list_f64_mutation = temp_source(
        "fn main() -> Text effects [alloc] { let mut xs = list_new(2, 0.0); xs = list_set(xs, 0, 1.5); xs = list_set(xs, 1, 2.25); text_from_f64_fixed(list_get(xs, 0) + list_get(xs, 1), 2) }",
    );
    let list_f64_mutation_output = run_path_profiled("check", &list_f64_mutation, "core");
    assert!(list_f64_mutation_output.status.success());

    let list_f64_const =
        temp_source("const XS: List[F64] = list_new(2, 0.0);\nfn main() -> I32 { 0 }");
    let list_f64_const_output = run_path_profiled("check", &list_f64_const, "core");
    assert!(!list_f64_const_output.status.success());
    let stderr = String::from_utf8_lossy(&list_f64_const_output.stderr);
    assert!(
        stderr.contains("semantic.list-runtime-context"),
        "unexpected stderr: {stderr}"
    );

    let immutable_array = temp_source("fn main() -> I32 { let xs = [0, 0]; xs[0] = 1; xs[0] }");
    let immutable_array_output = run_path_profiled("check", &immutable_array, "core");
    assert!(!immutable_array_output.status.success());
    assert!(
        String::from_utf8_lossy(&immutable_array_output.stderr)
            .contains("semantic.assign-immutable")
    );

    let non_array = temp_source("fn main() -> I32 { let mut total = 0; total[0] = 1; total }");
    let non_array_output = run_path_profiled("check", &non_array, "core");
    assert!(!non_array_output.status.success());
    assert!(
        String::from_utf8_lossy(&non_array_output.stderr).contains("semantic.array-index-base")
    );
}

#[test]
fn bootstrap_check_accepts_shipped_and_bootstrap_inputs() {
    for path in [
        const_control_flow_example(),
        multi_file_package_dir(),
        bootstrap_syntax_dir(),
        bootstrap_tools_dir(),
    ] {
        let output = run_path_profiled("bootstrap-check", &path, "core");
        assert!(
            output.status.success(),
            "{} should bootstrap-check cleanly",
            path.display()
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "ok [core]");
    }
}

#[test]
fn bootstrap_check_matches_semantic_diagnostics_for_retained_core_invalid_inputs() {
    for case in semantic_check_cases()
        .into_iter()
        .filter(|case| case.profile == "core")
    {
        let output = run_path_profiled("bootstrap-check", &case.path, "core");
        assert!(
            !output.status.success(),
            "{} should fail bootstrap-check for core profile",
            case.path.display()
        );
        let stderr = relativize_repo_root(&strip_ansi(&String::from_utf8_lossy(&output.stderr)));
        assert_eq!(
            stderr,
            std::fs::read_to_string(&case.expected).unwrap_or_else(|_| panic!(
                "fixture should be readable: {}",
                case.expected.display()
            ))
        );
    }
}

#[test]
fn check_emits_stable_diagnostics_for_retained_invalid_inputs() {
    for case in semantic_check_cases() {
        let output = run_path_profiled("check", &case.path, &case.profile);
        assert!(
            !output.status.success(),
            "{} should fail semantic check for profile {}",
            case.path.display(),
            case.profile
        );
        let stderr = relativize_repo_root(&strip_ansi(&String::from_utf8_lossy(&output.stderr)));
        assert_eq!(
            stderr,
            std::fs::read_to_string(&case.expected).unwrap_or_else(|_| panic!(
                "fixture should be readable: {}",
                case.expected.display()
            ))
        );
    }
}

#[test]
fn package_diagnostics_report_originating_source_files() {
    let package = temp_package(
        "[package]\nname = \"broken-package\"\nversion = \"0.1.0\"\nsources = [\"src/types.sarif\", \"src/main.sarif\"]\n",
        &[
            ("src/types.sarif", "struct Pair { left: I32, }\n"),
            (
                "src/main.sarif",
                "fn main() -> I32 { Pair { left: 20, right: 22 }.right }\n",
            ),
        ],
    );
    let output = run_path_profiled("check", &package, "core");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("semantic.record-field"));
    assert!(stderr.contains("src/main.sarif"));
}
