use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static UNIQUE_ID: AtomicU64 = AtomicU64::new(0);

pub const fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_sarifc")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
}

fn repo_path(relative: &str) -> PathBuf {
    repo_root()
        .join(relative)
        .canonicalize()
        .unwrap_or_else(|_| panic!("repo path should resolve: {relative}"))
}

pub fn relativize_repo_root(text: &str) -> String {
    let root = format!("{}/", repo_root().display());
    text.replace(&root, "")
}

pub fn strip_ansi(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            normalized.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' && matches!(chars.peek().copied(), Some('\\')) {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    normalized.replace("\r\n", "\n")
}

pub fn run_sarif(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("sarifc should run")
}

pub fn run_profiled(command: &str, path: &Path) -> Output {
    run_path_profiled(command, path, "core")
}

pub fn run_path(command: &str, path: &Path) -> Output {
    run_sarif(&[command, path.to_str().expect("utf-8 path")])
}

pub fn run_path_profiled(command: &str, path: &Path, profile: &str) -> Output {
    run_sarif(&[
        command,
        path.to_str().expect("utf-8 path"),
        "--profile",
        profile,
    ])
}

#[cfg(feature = "native-build")]
pub fn run_build_profiled(path: &Path, output: &Path, profile: &str) -> Output {
    run_sarif(&[
        "build",
        path.to_str().expect("utf-8 path"),
        "--profile",
        profile,
        "-o",
        output.to_str().expect("utf-8 path"),
    ])
}

pub fn example() -> PathBuf {
    repo_path("examples/hello.sarif")
}

pub fn const_control_flow_example() -> PathBuf {
    repo_path("examples/const_control_flow.sarif")
}

pub fn package_dir() -> PathBuf {
    repo_path("examples/hello-package")
}

pub fn package_manifest() -> PathBuf {
    repo_path("examples/hello-package/Sarif.toml")
}

pub fn multi_file_package_dir() -> PathBuf {
    repo_path("examples/multi-file-package")
}

pub fn multi_file_package_manifest() -> PathBuf {
    repo_path("examples/multi-file-package/Sarif.toml")
}

pub fn bootstrap_syntax_dir() -> PathBuf {
    repo_path("bootstrap/sarif_syntax")
}

pub fn bootstrap_tools_dir() -> PathBuf {
    repo_path("bootstrap/sarif_tools")
}

pub fn bootstrap_format_parity_inputs() -> Vec<PathBuf> {
    let manifest = fixture("bootstrap_format_parity_paths.txt");
    let root = repo_root();
    let contents = fs::read_to_string(&manifest).expect("parity manifest should be readable");

    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|relative| {
            root.join(relative)
                .canonicalize()
                .unwrap_or_else(|_| panic!("parity path should resolve: {relative}"))
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct BootstrapDocCase {
    pub path: PathBuf,
    pub expected: PathBuf,
}

#[derive(Clone, Debug)]
pub struct SemanticCheckCase {
    pub profile: String,
    pub path: PathBuf,
    pub expected: PathBuf,
}

#[derive(Clone, Debug)]
pub struct SemanticDocCase {
    pub profile: String,
    pub path: PathBuf,
    pub expected: PathBuf,
}

fn bootstrap_case_stem(relative: &str) -> &'static str {
    if relative == "examples/const_control_flow.sarif" {
        "const_control_flow"
    } else if relative == "examples/multi-file-package" {
        "multi_file_package"
    } else if relative == "bootstrap/sarif_syntax" {
        "bootstrap_syntax"
    } else if relative == "bootstrap/sarif_tools" {
        "bootstrap_tools"
    } else {
        panic!("unexpected bootstrap retained case: {relative}")
    }
}

fn bootstrap_exact_cases(
    manifest_name: &str,
    fixture_dir: &str,
    fixture_suffix: &str,
) -> Vec<(PathBuf, PathBuf)> {
    let manifest = fixture(manifest_name);
    let root = repo_root();
    let contents =
        fs::read_to_string(&manifest).expect("bootstrap retained manifest should be readable");

    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|relative| {
            let path = root
                .join(relative)
                .canonicalize()
                .unwrap_or_else(|_| panic!("bootstrap retained path should resolve: {relative}"));
            let expected = fixture(&format!(
                "{fixture_dir}/{}.{fixture_suffix}",
                bootstrap_case_stem(relative),
            ));
            (path, expected)
        })
        .collect()
}

pub fn bootstrap_doc_cases() -> Vec<BootstrapDocCase> {
    bootstrap_exact_cases("bootstrap_doc_parity_paths.txt", "bootstrap_doc", "doc.md")
        .into_iter()
        .map(|(path, expected)| BootstrapDocCase { path, expected })
        .collect()
}

pub fn semantic_check_cases() -> Vec<SemanticCheckCase> {
    let manifest = fixture("semantic_check_cases.tsv");
    let root = repo_root();
    let contents =
        fs::read_to_string(&manifest).expect("semantic check manifest should be readable");

    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut fields = line.split('\t');
            let profile = fields
                .next()
                .unwrap_or_else(|| panic!("missing profile in semantic check case: {line}"))
                .to_owned();
            let relative = fields
                .next()
                .unwrap_or_else(|| panic!("missing path in semantic check case: {line}"));
            let fixture_name = fields
                .next()
                .unwrap_or_else(|| panic!("missing fixture in semantic check case: {line}"));
            assert!(
                fields.next().is_none(),
                "unexpected extra semantic check fields: {line}"
            );

            SemanticCheckCase {
                profile,
                path: root.join(relative).canonicalize().unwrap_or_else(|_| {
                    panic!("semantic check case path should resolve: {relative}")
                }),
                expected: fixture(&format!("semantic_check/{fixture_name}")),
            }
        })
        .collect()
}

pub fn semantic_doc_cases() -> Vec<SemanticDocCase> {
    let manifest = fixture("semantic_doc_cases.tsv");
    let root = repo_root();
    let contents = fs::read_to_string(&manifest).expect("semantic doc manifest should be readable");

    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut fields = line.split('\t');
            let profile = fields
                .next()
                .unwrap_or_else(|| panic!("missing profile in semantic doc case: {line}"))
                .to_owned();
            let relative = fields
                .next()
                .unwrap_or_else(|| panic!("missing path in semantic doc case: {line}"));
            let fixture_name = fields
                .next()
                .unwrap_or_else(|| panic!("missing fixture in semantic doc case: {line}"));
            assert!(
                fields.next().is_none(),
                "unexpected extra semantic doc fields: {line}"
            );

            SemanticDocCase {
                profile,
                path: root.join(relative).canonicalize().unwrap_or_else(|_| {
                    panic!("semantic doc case path should resolve: {relative}")
                }),
                expected: fixture(fixture_name),
            }
        })
        .collect()
}

pub fn rt_invalid() -> PathBuf {
    repo_path("examples/invalid/rt_forbidden_effects.sarif")
}

pub fn rt_invalid_text() -> PathBuf {
    repo_path("examples/invalid/rt_forbidden_text.sarif")
}

pub fn rt_invalid_composites() -> PathBuf {
    repo_path("examples/invalid/rt_forbidden_composite_types.sarif")
}

pub fn total_invalid_repeat() -> PathBuf {
    repo_path("examples/invalid/total_forbidden_repeat.sarif")
}

pub fn total_invalid_while() -> PathBuf {
    repo_path("examples/invalid/total_forbidden_while.sarif")
}

pub fn contract_invalid() -> PathBuf {
    repo_path("examples/invalid/contract_failures.sarif")
}

pub fn affine_invalid() -> PathBuf {
    repo_path("examples/invalid/affine_reuse.sarif")
}

pub fn contract_affine_invalid() -> PathBuf {
    repo_path("examples/invalid/contract_affine_move.sarif")
}

pub fn temp_source(contents: &str) -> PathBuf {
    let unique = fresh_unique_id();
    let path = cli_temp_root().join(format!("sarif_cli_{unique}.sarif"));
    fs::write(&path, contents).expect("temporary source should be written");
    path
}

pub fn temp_package(manifest: &str, sources: &[(&str, &str)]) -> PathBuf {
    let unique = fresh_unique_id();
    let root = cli_temp_root().join(format!("sarif_cli_pkg_{unique}"));
    fs::create_dir_all(&root).expect("temporary package root should exist");
    for (relative, contents) in sources {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("temporary package parent should exist");
        }
        fs::write(path, contents).expect("temporary package source should be written");
    }
    fs::write(root.join("Sarif.toml"), manifest).expect("temporary manifest should be written");
    root
}

#[cfg(feature = "native-build")]
pub fn temp_artifact(stem: &str, extension: &str) -> PathBuf {
    let unique = fresh_unique_id();
    cli_temp_root().join(format!("sarif_cli_{stem}_{unique}.{extension}"))
}

pub fn temp_output(stem: &str, extension: &str) -> PathBuf {
    let unique = fresh_unique_id();
    cli_temp_root().join(format!("sarif_cli_{stem}_{unique}.{extension}"))
}

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn fresh_unique_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    let counter = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}", std::process::id(), timestamp, counter)
}

fn cli_temp_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.verify-target/cli-tmp");
    fs::create_dir_all(&root).expect("cli temp root should exist");
    root
}
