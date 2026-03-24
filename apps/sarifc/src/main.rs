use std::{env, fs, process::ExitCode};

#[cfg(feature = "native-build")]
mod artifact;
mod command;
mod input;
mod reports;

#[cfg(feature = "native-build")]
use artifact::link_executable;
use command::{CommandKind, parse_command, usage};
#[cfg(feature = "native-build")]
use input::PackageIdentity;
use input::resolve_input;
#[cfg(feature = "codegen")]
use reports::render_bootstrap_format;
#[cfg(feature = "format")]
use reports::render_format;
use reports::render_package_diagnostics;
#[cfg(feature = "native-build")]
use sarif_codegen::emit_object;
#[cfg(feature = "codegen")]
use sarif_codegen::lower as lower_mir;
#[cfg(all(test, feature = "codegen"))]
use sarif_codegen::{RuntimeValue, run_function};
#[cfg(feature = "wasm")]
use sarif_codegen::{emit_wasm, emit_wat};
use sarif_frontend::semantic::Profile;
use sarif_frontend::{FrontendDatabase, SourceId};
use sarif_syntax::Diagnostic;
#[cfg(feature = "docs")]
use sarif_syntax::Span;
#[cfg(not(feature = "docs"))]
use sarif_tools::report::semantic_snapshot;
use sarif_tools::report::{SemanticSnapshot, render_semantic_check};
#[cfg(feature = "docs")]
use sarif_tools::report::{
    render_semantic_doc, semantic_package_snapshot_from_analysis, semantic_snapshot_from_analysis,
};

#[cfg(feature = "codegen")]
const BOOTSTRAP_TOOL_STACK_SIZE: usize = 32 * 1024 * 1024;

struct LoadedSource {
    path: String,
    source: String,
    segments: Vec<PackageSegment>,
    database: FrontendDatabase,
    source_id: SourceId,
    #[cfg(feature = "codegen")]
    mir_cache: std::cell::OnceCell<sarif_codegen::MirLowering>,
    #[cfg(feature = "native-build")]
    package: PackageIdentity,
}

struct PackageSegment {
    path: String,
    source: String,
    start: usize,
    end: usize,
}

struct PackageSource {
    combined: String,
    segments: Vec<PackageSegment>,
}

impl LoadedSource {
    fn load(path: &str) -> Result<Self, String> {
        let resolved = resolve_input(path)?;
        let package_source = read_virtual_package_source(&resolved)?;
        let mut database = FrontendDatabase::new();
        let source_id = database.add_source(
            resolved.display_path.clone(),
            package_source.combined.clone(),
        );
        Ok(Self {
            path: resolved.display_path,
            source: package_source.combined,
            segments: package_source.segments,
            database,
            source_id,
            #[cfg(feature = "codegen")]
            mir_cache: std::cell::OnceCell::new(),
            #[cfg(feature = "native-build")]
            package: resolved.package,
        })
    }

    fn lex_diagnostics(&self) -> Vec<Diagnostic> {
        self.database.lex(self.source_id).diagnostics
    }

    fn parse_diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.lex_diagnostics();
        diagnostics.extend(self.database.parse(self.source_id).diagnostics);
        diagnostics
    }

    fn ast_diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.parse_diagnostics();
        diagnostics.extend(self.database.ast(self.source_id).diagnostics);
        diagnostics
    }

    fn hir_diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.ast_diagnostics();
        diagnostics.extend(self.database.hir(self.source_id).diagnostics);
        diagnostics
    }

    fn semantic_diagnostics(&self, profile: Profile) -> Vec<Diagnostic> {
        let mut diagnostics = self.hir_diagnostics();
        diagnostics.extend(self.database.semantic(self.source_id, profile).diagnostics);
        diagnostics
    }

    #[cfg(feature = "codegen")]
    fn mir(&self) -> &sarif_codegen::MirLowering {
        self.mir_cache
            .get_or_init(|| lower_mir(&self.database.hir(self.source_id).module))
    }

    #[cfg(feature = "codegen")]
    fn mir_diagnostics(&self, profile: Profile) -> Vec<Diagnostic> {
        let mut diagnostics = self.semantic_diagnostics(profile);
        diagnostics.extend(self.mir().diagnostics.iter().cloned());
        diagnostics
    }

    fn ensure_no_diagnostics(
        &self,
        diagnostics: &[Diagnostic],
        failure: &str,
    ) -> Result<(), String> {
        if diagnostics.is_empty() {
            Ok(())
        } else {
            eprint!(
                "{}",
                render_package_diagnostics(&self.path, &self.source, &self.segments, diagnostics)
            );
            Err(failure.to_owned())
        }
    }

    fn require_semantic(&self, profile: Profile, failure: &str) -> Result<(), String> {
        let diagnostics = self.semantic_diagnostics(profile);
        self.ensure_no_diagnostics(&diagnostics, failure)
    }

    fn semantic_snapshot(
        &self,
        profile: Profile,
        failure: &str,
    ) -> Result<SemanticSnapshot, String> {
        self.require_semantic(profile, failure)?;
        #[cfg(feature = "docs")]
        {
            let semantic = self.database.semantic(self.source_id, profile);
            #[cfg(feature = "codegen")]
            let const_values = {
                let mir = self.mir();
                mir.const_values
                    .iter()
                    .map(|(name, value)| (name.clone(), value.render()))
                    .collect()
            };
            #[cfg(not(feature = "codegen"))]
            let const_values = std::collections::BTreeMap::new();

            let sections = if self.segments.len() <= 1 {
                Vec::new()
            } else {
                self.segments
                    .iter()
                    .map(|segment| (segment.path.clone(), Span::new(segment.start, segment.end)))
                    .collect()
            };
            if sections.is_empty() {
                Ok(semantic_snapshot_from_analysis(
                    profile,
                    &semantic,
                    &const_values,
                ))
            } else {
                Ok(semantic_package_snapshot_from_analysis(
                    profile,
                    &semantic,
                    &const_values,
                    &sections,
                ))
            }
        }
        #[cfg(not(feature = "docs"))]
        {
            Ok(semantic_snapshot(profile))
        }
    }

    #[cfg(feature = "codegen")]
    fn require_mir(&self, profile: Profile, failure: &str) -> Result<(), String> {
        let diagnostics = self.mir_diagnostics(profile);
        self.ensure_no_diagnostics(&diagnostics, failure)
    }

    #[cfg(feature = "codegen")]
    fn lower_program(
        &self,
        profile: Profile,
        failure: &str,
    ) -> Result<sarif_codegen::Program, String> {
        self.require_mir(profile, failure)?;
        Ok(self.mir().program.clone())
    }

    #[cfg(all(test, feature = "codegen"))]
    fn run_text_function(
        &self,
        profile: Profile,
        function: &str,
        input: &str,
    ) -> Result<String, String> {
        let program = self.lower_program(profile, "tool execution failed")?;
        let value = run_function(&program, function, &[RuntimeValue::Text(input.to_owned())])
            .map_err(|error| format!("runtime error: {}", error.message))?;
        match value {
            RuntimeValue::Text(text) => Ok(text),
            other => Err(format!(
                "tool function `{function}` must return Text, found {}",
                other.render()
            )),
        }
    }

    #[cfg(feature = "native-build")]
    fn native_object_name(&self) -> String {
        self.package.symbol_stem()
    }
}

fn read_virtual_package_source(resolved: &input::ResolvedInput) -> Result<PackageSource, String> {
    let mut combined = String::new();
    let mut segments = Vec::with_capacity(resolved.source_paths.len());
    for source_path in &resolved.source_paths {
        let source = fs::read_to_string(source_path)
            .map_err(|error| format!("failed to read `{source_path}`: {error}"))?;
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        let start = combined.len();
        combined.push_str(&source);
        let end = combined.len();
        segments.push(PackageSegment {
            path: source_path.clone(),
            source,
            start,
            end,
        });
    }
    Ok(PackageSource { combined, segments })
}

fn load_and_render<F>(path: &str, render: F) -> Result<String, String>
where
    F: FnOnce(&LoadedSource) -> Result<String, String>,
{
    let loaded = LoadedSource::load(path)?;
    render(&loaded)
}

fn print_loaded_render<F>(path: &str, render: F) -> Result<(), String>
where
    F: FnOnce(&LoadedSource) -> Result<String, String>,
{
    print!("{}", load_and_render(path, render)?);
    Ok(())
}

#[cfg(feature = "codegen")]
fn bootstrap_tools_path() -> Result<String, String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bootstrap/sarif_tools")
        .canonicalize()
        .map_err(|error| format!("failed to resolve bootstrap tools package: {error}"))?;
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "bootstrap tools path must be valid utf-8".to_owned())
}

#[cfg(feature = "codegen")]
fn load_bootstrap_tools() -> Result<LoadedSource, String> {
    let path = bootstrap_tools_path()?;
    LoadedSource::load(&path)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let command = parse_command(&args)?;

    match command.kind {
        CommandKind::Help => {
            println!("{}", usage());
            Ok(())
        }
        CommandKind::Version => {
            println!("sarifc {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CommandKind::Check => print_loaded_render(&command.path, |loaded| {
            Ok(render_semantic_check(
                &loaded.semantic_snapshot(command.profile, "check failed")?,
            ))
        }),
        #[cfg(feature = "format")]
        CommandKind::Format => {
            let loaded = LoadedSource::load(&command.path)?;
            match render_format(&loaded) {
                Ok(formatted) => {
                    print!("{formatted}");
                    Ok(())
                }
                Err(rendered) => {
                    eprint!("{rendered}");
                    Err("formatting failed".to_owned())
                }
            }
        }
        #[cfg(not(feature = "format"))]
        CommandKind::Format => Err("format requires the `format` feature".to_owned()),
        #[cfg(feature = "codegen")]
        CommandKind::BootstrapFormat => {
            let path = command.path;
            print!(
                "{}",
                run_bootstrap_tool(move || load_and_render(&path, render_bootstrap_format))?
            );
            Ok(())
        }
        #[cfg(not(feature = "codegen"))]
        CommandKind::BootstrapFormat => {
            Err("bootstrap-format requires the `codegen` feature".to_owned())
        }
        #[cfg(feature = "docs")]
        CommandKind::Doc => print_loaded_render(&command.path, |loaded| {
            Ok(render_semantic_doc(&loaded.semantic_snapshot(
                command.profile,
                "doc generation failed",
            )?))
        }),
        #[cfg(not(feature = "docs"))]
        CommandKind::Doc => Err("doc requires the `docs` feature".to_owned()),
        #[cfg(feature = "codegen")]
        CommandKind::BootstrapCheck => {
            let path = command.path;
            print!(
                "{}",
                run_bootstrap_tool(move || {
                    load_and_render(&path, |loaded| {
                        Ok(render_semantic_check(
                            &loaded.semantic_snapshot(Profile::Core, "check failed")?,
                        ))
                    })
                })?
            );
            Ok(())
        }
        #[cfg(not(feature = "codegen"))]
        CommandKind::BootstrapCheck => print_loaded_render(&command.path, |loaded| {
            Ok(render_semantic_check(
                &loaded.semantic_snapshot(Profile::Core, "check failed")?,
            ))
        }),
        #[cfg(all(feature = "codegen", feature = "docs"))]
        CommandKind::BootstrapDoc => {
            let path = command.path;
            print!(
                "{}",
                run_bootstrap_tool(move || {
                    load_and_render(&path, |loaded| {
                        Ok(render_semantic_doc(&loaded.semantic_snapshot(
                            Profile::Core,
                            "bootstrap doc failed",
                        )?))
                    })
                })?
            );
            Ok(())
        }
        #[cfg(not(all(feature = "codegen", feature = "docs")))]
        CommandKind::BootstrapDoc => {
            Err("bootstrap-doc requires the `codegen` and `docs` features".to_owned())
        }
        #[cfg(feature = "codegen")]
        CommandKind::Run => {
            let loaded = LoadedSource::load(&command.path)?;
            let program = loaded.lower_program(command.profile, "run failed")?;
            let mut program_args = Vec::with_capacity(command.program_args.len() + 1);
            program_args.push(command.path.clone());
            program_args.extend(command.program_args.iter().cloned());
            let stdin_text =
                read_stdin_text().map_err(|error| format!("failed to read stdin: {error}"))?;
            let (result, stdout_text) =
                sarif_codegen::run_main_with_io_capture(&program, &program_args, stdin_text)
                    .map_err(|error| format!("runtime error: {}", error.message))?;
            if !stdout_text.is_empty() {
                print!("{stdout_text}");
            }
            if !matches!(result, sarif_codegen::RuntimeValue::Unit) || stdout_text.is_empty() {
                println!("{}", result.render());
            }
            Ok(())
        }
        #[cfg(not(feature = "codegen"))]
        CommandKind::Run => Err("run requires the `codegen` feature".to_owned()),
        #[cfg(feature = "codegen")]
        CommandKind::Build => {
            let loaded = LoadedSource::load(&command.path)?;
            let program = loaded.lower_program(command.profile, "build failed")?;
            let output = command
                .output_path
                .as_deref()
                .ok_or("missing output path")?;

            match command.target {
                #[cfg(feature = "native-build")]
                command::BuildTarget::Native => {
                    let object = emit_object(&program, &loaded.native_object_name())
                        .map_err(|error| format!("object error: {}", error.message))?;
                    link_executable(&program, &object, output, command.print_main)?;
                }
                #[cfg(feature = "wasm")]
                command::BuildTarget::Wasm => {
                    if std::path::Path::new(output)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("wat"))
                    {
                        let wat = emit_wat(&program)
                            .map_err(|error| format!("wasm error: {}", error.message))?;
                        fs::write(output, wat)
                            .map_err(|error| format!("failed to write `{output}`: {error}"))?;
                    } else {
                        let wasm = emit_wasm(&program)
                            .map_err(|error| format!("wasm error: {}", error.message))?;
                        fs::write(output, wasm)
                            .map_err(|error| format!("failed to write `{output}`: {error}"))?;
                    }
                }
                #[cfg(not(feature = "native-build"))]
                command::BuildTarget::Native => {
                    return Err("native-build feature is disabled".to_owned());
                }
                #[cfg(not(feature = "wasm"))]
                command::BuildTarget::Wasm => {
                    return Err("wasm feature is disabled".to_owned());
                }
            }
            Ok(())
        }
        #[cfg(not(feature = "codegen"))]
        CommandKind::Build => Err("build requires the `codegen` feature".to_owned()),
    }
}

#[cfg(feature = "codegen")]
fn read_stdin_text() -> Result<String, std::io::Error> {
    use std::io::Read as _;

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

#[cfg(feature = "codegen")]
fn run_bootstrap_tool<F>(tool: F) -> Result<String, String>
where
    F: FnOnce() -> Result<String, String> + Send + 'static,
{
    std::thread::Builder::new()
        .stack_size(BOOTSTRAP_TOOL_STACK_SIZE)
        .spawn(tool)
        .map_err(|error| format!("failed to spawn bootstrap tool worker: {error}"))?
        .join()
        .map_err(|_| "bootstrap tool worker panicked".to_owned())?
}

#[cfg(all(test, feature = "codegen"))]
mod tests {
    use crate::BOOTSTRAP_TOOL_STACK_SIZE;
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use sarif_frontend::semantic::Profile;

    use super::LoadedSource;

    static UNIQUE_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn runs_text_tool_functions_from_multi_file_packages() {
        let package = write_temp_package(
            "tool_pkg",
            "[package]\nname = \"tool-pkg\"\nversion = \"0.1.0\"\nsources = [\"src/helpers.sarif\", \"src/main.sarif\"]\n",
            &[
                (
                    "src/helpers.sarif",
                    "fn is_empty(source: Text) -> Bool { text_len(source) == 0 }\n",
                ),
                (
                    "src/main.sarif",
                    "fn format_text(source: Text) -> Text { if is_empty(source) { \"empty\" } else { text_concat(source, \"!\") } }\nfn main() -> I32 { 0 }\n",
                ),
            ],
        );
        let loaded = LoadedSource::load(package.to_str().expect("utf-8 package path"))
            .expect("package should load");

        let formatted = loaded
            .run_text_function(Profile::Core, "format_text", "sarif")
            .expect("text tool function should run");
        let empty = loaded
            .run_text_function(Profile::Core, "format_text", "")
            .expect("text tool function should run");

        assert_eq!(formatted, "sarif!");
        assert_eq!(empty, "empty");
    }

    #[test]
    fn runs_bootstrap_text_tool_functions() {
        std::thread::Builder::new()
            .stack_size(BOOTSTRAP_TOOL_STACK_SIZE)
            .spawn(|| {
                let package = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../bootstrap/sarif_tools")
                    .canonicalize()
                    .expect("bootstrap tools package should exist");
                let loaded = LoadedSource::load(package.to_str().expect("utf-8 package path"))
                    .expect("package should load");
                assert_bootstrap_text_outputs(&loaded);
            })
            .expect("bootstrap tools thread should spawn")
            .join()
            .expect("bootstrap tools thread should complete");
    }

    #[cfg(feature = "format")]
    #[test]
    fn bootstrap_format_matches_rust_formatter_for_simple_functions() {
        use sarif_syntax::ast::lower as lower_ast;
        use sarif_syntax::lexer::lex;
        use sarif_syntax::parser::parse;
        use sarif_tools::format::format_file;

        std::thread::Builder::new()
            .stack_size(BOOTSTRAP_TOOL_STACK_SIZE)
            .spawn(|| {
                let package = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../bootstrap/sarif_tools")
                    .canonicalize()
                    .expect("bootstrap tools package should exist");
                let loaded = LoadedSource::load(package.to_str().expect("utf-8 package path"))
                    .expect("package should load");

                for source in ["fn main() -> I32 { 0 }", "fn main() {}"] {
                    let bootstrap = loaded
                        .run_text_function(Profile::Core, "format_text", source)
                        .expect("bootstrap format_text should run");
                    let lexed = lex(source);
                    let parsed = parse(&lexed.tokens);
                    let ast = lower_ast(&parsed.root);
                    assert!(
                        ast.diagnostics.is_empty(),
                        "format parity source should parse cleanly: {source}"
                    );
                    let rust = format_file(&ast.file);
                    assert_eq!(bootstrap, rust, "bootstrap formatter drifted for {source}");
                }
            })
            .expect("bootstrap format parity thread should spawn")
            .join()
            .expect("bootstrap format parity thread should complete");
    }

    #[test]
    fn rejects_non_text_tool_results() {
        let path = write_temp_source(
            "fn classify(source: Text) -> Bool { text_len(source) == 0 }\nfn main() -> I32 { 0 }\n",
        );
        let loaded =
            LoadedSource::load(path.to_str().expect("utf-8 path")).expect("source should load");
        let error = loaded
            .run_text_function(Profile::Core, "classify", "")
            .expect_err("non-Text tool result should be rejected");

        assert!(error.contains("must return Text"));
    }

    fn write_temp_source(contents: &str) -> PathBuf {
        let path = temp_root().join(format!("sarif_main_{}.sarif", fresh_unique_id()));
        fs::write(&path, contents).expect("temporary source should be written");
        path
    }

    fn write_temp_package(stem: &str, manifest: &str, sources: &[(&str, &str)]) -> PathBuf {
        let root = temp_root().join(format!("{stem}_{}", fresh_unique_id()));
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

    fn fresh_unique_id() -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let counter = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
        format!("{}_{}_{}", std::process::id(), timestamp, counter)
    }

    fn temp_root() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.verify-target/unit-tmp");
        fs::create_dir_all(&root).expect("unit temp root should exist");
        root
    }

    const fn bootstrap_truncated_source() -> &'static str {
        "const a: I32 = 0;\nconst b: I32 = 0;\nconst c: I32 = 0;\nconst d: I32 = 0;\nconst e: I32 = 0;\nconst f: I32 = 0;\n"
    }

    fn assert_bootstrap_text_outputs(loaded: &LoadedSource) {
        let formatted = loaded
            .run_text_function(Profile::Core, "format_text", "fn main() -> I32 { 0 }")
            .expect("bootstrap format_text should run");
        let formatted_multi = loaded
            .run_text_function(Profile::Core, "format_text", "struct Pair {}\nfn main() {}")
            .expect("bootstrap format_text should run");
        let formatted_existing_newline = loaded
            .run_text_function(Profile::Core, "format_text", "fn main() -> I32 { 0 }\n")
            .expect("bootstrap format_text should run");
        let truncated_format = loaded
            .run_text_function(Profile::Core, "format_text", bootstrap_truncated_source())
            .expect("bootstrap format_text should run");

        assert_eq!(formatted, "fn main() -> I32 {\n    0\n}\n");
        assert_eq!(formatted_existing_newline, "fn main() -> I32 {\n    0\n}\n");
        assert_eq!(formatted_multi, "struct Pair {}\n\nfn main() {\n}\n");
        assert_eq!(
            truncated_format,
            "const a: I32 = 0;\n\nconst b: I32 = 0;\n\nconst c: I32 = 0;\n\nconst d: I32 = 0;\n\nconst e: I32 = 0;\n\nconst f: I32 = 0;\n"
        );
    }
}

#[cfg(test)]
mod bootstrap_check_tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::LoadedSource;
    use sarif_frontend::semantic::Profile;
    use sarif_tools::report::render_semantic_check;

    static UNIQUE_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn bootstrap_check_keeps_working_without_codegen() {
        let path = write_temp_source("fn main() -> I32 { 0 }\n");
        let loaded =
            LoadedSource::load(path.to_str().expect("utf-8 path")).expect("source should load");

        let rendered = render_semantic_check(
            &loaded
                .semantic_snapshot(Profile::Core, "check failed")
                .expect("bootstrap-check should succeed"),
        );

        assert_eq!(rendered, "ok [core]\n");
    }

    fn write_temp_source(contents: &str) -> PathBuf {
        let path = temp_root().join(format!("sarif_bootstrap_check_{}.sarif", fresh_unique_id()));
        fs::write(&path, contents).expect("temporary source should be written");
        path
    }

    fn fresh_unique_id() -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let counter = UNIQUE_ID.fetch_add(1, Ordering::Relaxed);
        format!("{}_{}_{}", std::process::id(), timestamp, counter)
    }

    fn temp_root() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.verify-target/unit-tmp");
        fs::create_dir_all(&root).expect("unit temp root should exist");
        root
    }
}
