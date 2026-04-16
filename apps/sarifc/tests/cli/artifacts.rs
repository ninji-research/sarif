use super::support::{
    bootstrap_doc_cases, bootstrap_syntax_dir, bootstrap_tools_dir, const_control_flow_example,
    multi_file_package_dir, multi_file_package_manifest, package_dir, package_manifest,
    relativize_repo_root, run_path_profiled, run_sarif_with_env, semantic_doc_cases, temp_artifact,
    temp_output, temp_package, temp_source,
};
use std::fs;

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
    let path = temp_source("fn main() -> I32 { let value = true; value + 1 }");
    let output = run_path_profiled("doc", &path, "core");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.trim().is_empty());
    assert!(stderr.contains("doc generation failed"));
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

#[cfg(feature = "native-build")]
#[test]
fn native_build_reuses_cached_runtime_objects_for_identical_build_modes() {
    let cache_root = temp_output("runtime_cache", "tmp");
    fs::create_dir_all(&cache_root).expect("cache root should exist");
    let path = temp_source("fn main() -> I32 { 42 }");
    let first_binary = temp_artifact("runtime_cache_first", "bin");
    let second_binary = temp_artifact("runtime_cache_second", "bin");
    let tmpdir = cache_root.to_str().expect("utf-8 cache root");

    let first = run_sarif_with_env(
        &[
            "build",
            path.to_str().expect("utf-8 path"),
            "--print-main",
            "-o",
            first_binary.to_str().expect("utf-8 path"),
        ],
        &[("TMPDIR", tmpdir), ("SARIF_NATIVE_CPU", "baseline")],
    );
    assert!(
        first.status.success(),
        "first native build should succeed: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let second = run_sarif_with_env(
        &[
            "build",
            path.to_str().expect("utf-8 path"),
            "--print-main",
            "-o",
            second_binary.to_str().expect("utf-8 path"),
        ],
        &[("TMPDIR", tmpdir), ("SARIF_NATIVE_CPU", "baseline")],
    );
    assert!(
        second.status.success(),
        "second native build should succeed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let runtime_cache = cache_root.join("sarif/runtime-cache");
    let object_count = fs::read_dir(&runtime_cache)
        .expect("runtime cache should exist")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "o"))
        .count();
    assert_eq!(
        object_count, 1,
        "identical native builds should reuse one cached runtime object"
    );
}

#[cfg(feature = "native-build")]
#[test]
fn native_build_runtime_cache_changes_across_cpu_modes() {
    let cache_root = temp_output("runtime_cache_modes", "tmp");
    fs::create_dir_all(&cache_root).expect("cache root should exist");
    let path = temp_source("fn main() -> I32 { 42 }");
    let baseline_binary = temp_artifact("runtime_cache_baseline", "bin");
    let native_binary = temp_artifact("runtime_cache_native", "bin");
    let tmpdir = cache_root.to_str().expect("utf-8 cache root");

    let baseline = run_sarif_with_env(
        &[
            "build",
            path.to_str().expect("utf-8 path"),
            "--print-main",
            "-o",
            baseline_binary.to_str().expect("utf-8 path"),
        ],
        &[("TMPDIR", tmpdir), ("SARIF_NATIVE_CPU", "baseline")],
    );
    assert!(
        baseline.status.success(),
        "baseline native build should succeed: {}",
        String::from_utf8_lossy(&baseline.stderr)
    );

    let native = run_sarif_with_env(
        &[
            "build",
            path.to_str().expect("utf-8 path"),
            "--print-main",
            "-o",
            native_binary.to_str().expect("utf-8 path"),
        ],
        &[("TMPDIR", tmpdir), ("SARIF_NATIVE_CPU", "native")],
    );
    assert!(
        native.status.success(),
        "native-tuned build should succeed: {}",
        String::from_utf8_lossy(&native.stderr)
    );

    let runtime_cache = cache_root.join("sarif/runtime-cache");
    let object_count = fs::read_dir(&runtime_cache)
        .expect("runtime cache should exist")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "o"))
        .count();
    assert_eq!(
        object_count, 2,
        "distinct CPU modes should produce distinct cached runtime objects"
    );
}
