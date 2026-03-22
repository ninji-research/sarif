use super::support::{
    bootstrap_doc_cases, bootstrap_syntax_dir, bootstrap_tools_dir, const_control_flow_example,
    multi_file_package_dir, multi_file_package_manifest, package_dir, package_manifest,
    relativize_repo_root, run_path_profiled, semantic_doc_cases, temp_package, temp_source,
};

#[cfg(feature = "codegen")]
#[test]
fn doc_reports_const_values_from_mutable_helper_control_flow() {
    let path = const_control_flow_example();
    let output = run_path_profiled("doc", &path, "core");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("## const answer"));
    assert!(stdout.contains("value: `42`"));
}

#[cfg(not(feature = "codegen"))]
#[test]
fn doc_renders_semantic_output_without_mir_const_values() {
    let path = temp_source("const answer: I32 = 42;\nfn main() -> I32 { answer }");
    let output = run_path_profiled("doc", &path, "core");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("## const answer"));
    assert!(!stdout.contains("value: `42`"));
}

#[test]
fn doc_rejects_invalid_programs_without_partial_output() {
    let path = temp_source("fn main() -> I32 { let value = 1; value; 0 }");
    let output = run_path_profiled("doc", &path, "core");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.trim().is_empty());
    assert!(stderr.contains("doc generation failed"));
    assert!(stderr.contains("statement expression"));
}

#[test]
fn doc_accepts_package_inputs() {
    for path in [
        package_dir(),
        package_manifest(),
        multi_file_package_dir(),
        multi_file_package_manifest(),
        bootstrap_syntax_dir(),
        bootstrap_tools_dir(),
    ] {
        let output = run_path_profiled("doc", &path, "core");
        assert!(
            output.status.success(),
            "{} should document cleanly",
            path.display()
        );
        assert!(!String::from_utf8_lossy(&output.stdout).trim().is_empty());
    }
}

#[test]
fn doc_emits_stable_markdown_for_retained_semantic_inputs() {
    for case in semantic_doc_cases() {
        let output = run_path_profiled("doc", &case.path, &case.profile);

        assert!(
            output.status.success(),
            "{} should emit semantic docs cleanly",
            case.path.display()
        );
        assert_eq!(
            relativize_repo_root(&String::from_utf8_lossy(&output.stdout)),
            std::fs::read_to_string(&case.expected).unwrap_or_else(|_| panic!(
                "fixture should be readable: {}",
                case.expected.display()
            ))
        );
    }
}

#[test]
fn doc_groups_multi_file_packages_by_source_file() {
    let package = temp_package(
        "[package]\nname = \"doc-boundaries\"\nversion = \"0.1.0\"\nsources = [\"src/types.sarif\", \"src/consts.sarif\", \"src/main.sarif\"]\n",
        &[
            (
                "src/types.sarif",
                "struct Pair { left: I32, right: I32, }\n",
            ),
            ("src/consts.sarif", "const answer: I32 = 42;\n"),
            ("src/main.sarif", "fn main() -> I32 { answer }\n"),
        ],
    );
    let output = run_path_profiled("doc", &package, "core");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("src/types.sarif"));
    assert!(stdout.contains("### struct Pair"));
    assert!(stdout.contains("src/consts.sarif"));
    assert!(stdout.contains("### const answer"));
    assert!(stdout.contains("src/main.sarif"));
    assert!(stdout.contains("### fn main"));
}

#[test]
fn bootstrap_doc_matches_retained_semantic_docs_for_single_files_and_packages() {
    for case in bootstrap_doc_cases() {
        let output = run_path_profiled("bootstrap-doc", &case.path, "core");
        assert!(
            output.status.success(),
            "{} should bootstrap-doc cleanly",
            case.path.display()
        );
        let stdout = relativize_repo_root(&String::from_utf8_lossy(&output.stdout));
        let expected = std::fs::read_to_string(&case.expected)
            .unwrap_or_else(|_| panic!("fixture should be readable: {}", case.expected.display()));
        assert_eq!(
            stdout.trim_end_matches('\n'),
            expected.trim_end_matches('\n')
        );
    }
}
