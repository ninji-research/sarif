use std::{env, fs, process::ExitCode};

#[cfg(feature = "native-build")]
mod artifact;
mod command;
mod input;
mod reports;

#[cfg(feature = "native-build")]
use artifact::link_executable;
use command::{BuildTarget, CommandKind, parse_command, usage};
use input::resolve_input;
use reports::{
    render_package_diagnostics, render_semantic_check, render_semantic_doc, render_semantic_format,
};
#[cfg(all(test, feature = "codegen"))]
use sarif_codegen::Program;
#[cfg(feature = "codegen")]
use sarif_codegen::emit_object;
#[cfg(feature = "wasm")]
use sarif_codegen::emit_wasm;
#[cfg(feature = "codegen")]
use sarif_codegen::{RuntimeError, RuntimeValue, lower as lower_mir};
use sarif_frontend::semantic::Profile;
use sarif_frontend::{FrontendDatabase, SourceId};
use sarif_syntax::Diagnostic;

#[cfg(all(test, feature = "codegen"))]
const BOOTSTRAP_TOOL_STACK_SIZE: usize = 32 * 1024 * 1024;

struct PackageSegment {
    path: String,
    source: String,
    combined_span: sarif_syntax::Span,
}

struct LoadedSource {
    path: String,
    source: String,
    segments: Vec<PackageSegment>,
    database: FrontendDatabase,
    source_id: SourceId,
    #[cfg(feature = "codegen")]
    mir_cache: std::cell::OnceCell<sarif_codegen::MirLowering>,
    #[cfg(feature = "native-build")]
    package: input::PackageIdentity,
}

impl LoadedSource {
    fn load(path: &str) -> Result<Self, String> {
        let resolved = resolve_input(path)?;
        let mut segments = Vec::new();
        let mut combined_source = String::new();
        for source_path in &resolved.source_paths {
            let source = fs::read_to_string(source_path)
                .map_err(|error| format!("failed to read `{source_path}`: {error}"))?;
            let start = combined_source.len();
            combined_source.push_str(&source);
            if !source.ends_with('\n') {
                combined_source.push('\n');
            }
            let end = combined_source.len();
            segments.push(PackageSegment {
                path: source_path.clone(),
                source,
                combined_span: sarif_syntax::Span::new(start, end),
            });
        }

        let mut database = FrontendDatabase::default();
        let source_id = database.add_source(resolved.display_path.clone(), combined_source.clone());
        Ok(Self {
            path: resolved.display_path,
            source: combined_source,
            segments,
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

    #[cfg(all(test, feature = "codegen"))]
    fn lower_program(&self, profile: Profile, failure: &str) -> Result<&Program, String> {
        self.ensure_no_diagnostics(&self.mir_diagnostics(profile), failure)?;
        Ok(&self.mir().program)
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    match run(&args[1..]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    run_command(parse_command(args)?)
}

fn run_command(command: command::Command) -> Result<(), String> {
    match command.kind {
        CommandKind::Help => {
            println!("{}", usage());
            Ok(())
        }
        CommandKind::Version => {
            println!("sarifc {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CommandKind::Check => run_check(&command),
        CommandKind::Format => run_format(&command),
        CommandKind::BootstrapFormat => run_bootstrap_format(&command),
        CommandKind::Doc => run_doc(&command),
        CommandKind::BootstrapCheck => run_bootstrap_check(&command),
        CommandKind::BootstrapDoc => run_bootstrap_doc(&command),
        CommandKind::Run => run_program(command),
        CommandKind::Build => build_program(&command),
    }
}

fn run_check(command: &command::Command) -> Result<(), String> {
    print_loaded_render(&command.path, |loaded| {
        render_semantic_check(loaded, command.profile)
    })
}

fn run_format(command: &command::Command) -> Result<(), String> {
    print_loaded_render(&command.path, render_semantic_format)
}

fn run_doc(command: &command::Command) -> Result<(), String> {
    print_loaded_render(&command.path, |loaded| {
        render_semantic_doc(loaded, command.profile)
    })
}

fn run_bootstrap_format(command: &command::Command) -> Result<(), String> {
    print_loaded_render(&command.path, render_semantic_format)
}

fn run_bootstrap_check(command: &command::Command) -> Result<(), String> {
    print_loaded_render(&command.path, |loaded| {
        render_semantic_check(loaded, command.profile)
    })
}

fn run_bootstrap_doc(command: &command::Command) -> Result<(), String> {
    print_loaded_render(&command.path, |loaded| {
        render_semantic_doc(loaded, command.profile)
    })
}

#[cfg(feature = "codegen")]
fn run_program(command: command::Command) -> Result<(), String> {
    let loaded = LoadedSource::load(&command.path)?;
    let diagnostics = loaded.mir_diagnostics(command.profile);
    loaded.ensure_no_diagnostics(&diagnostics, "execution failed")?;

    let mut program_args = vec![command.path];
    program_args.extend(command.program_args);

    let result = sarif_codegen::run_main_with_args(&loaded.mir().program, &program_args).map_err(
        |error| {
            let message = match error {
                RuntimeError::Message(m) => m,
                RuntimeError::EffectUnwind {
                    effect, operation, ..
                } => format!("unhandled effect {effect}.{operation}"),
            };
            format!("runtime error: {message}")
        },
    )?;
    if !matches!(result, RuntimeValue::Unit) {
        println!("{}", result.render());
    }
    Ok(())
}

#[cfg(not(feature = "codegen"))]
fn run_program(_command: command::Command) -> Result<(), String> {
    Err("run requires the `codegen` feature".to_owned())
}

#[cfg(feature = "codegen")]
fn build_program(command: &command::Command) -> Result<(), String> {
    let loaded = LoadedSource::load(&command.path)?;
    let diagnostics = loaded.mir_diagnostics(command.profile);
    loaded.ensure_no_diagnostics(&diagnostics, "build failed")?;

    let output_path = command
        .output_path
        .as_deref()
        .ok_or("missing output path")?;

    match command.target {
        BuildTarget::Native => {
            #[cfg(feature = "native-build")]
            {
                let stem = loaded.package.symbol_stem();
                let object_bytes = emit_object(&loaded.mir().program, &stem)
                    .map_err(|error| format!("failed to emit object file: {error:?}"))?;

                link_executable(
                    &loaded.mir().program,
                    &object_bytes,
                    output_path,
                    command.print_main,
                )
                .map_err(|error| format!("failed to link executable: {error}"))?;
                Ok(())
            }
            #[cfg(not(feature = "native-build"))]
            {
                Err("native build requires the `native-build` feature".to_owned())
            }
        }
        BuildTarget::Wasm => {
            #[cfg(feature = "wasm")]
            {
                let wasm_bytes = emit_wasm(&loaded.mir().program)
                    .map_err(|error| format!("failed to emit wasm: {}", error.message))?;
                fs::write(output_path, wasm_bytes).map_err(|error| {
                    format!("failed to write wasm file `{output_path}`: {error}")
                })?;
                Ok(())
            }
            #[cfg(not(feature = "wasm"))]
            {
                Err("wasm build requires the `wasm` feature".to_owned())
            }
        }
    }
}

#[cfg(not(feature = "codegen"))]
fn build_program(_command: &command::Command) -> Result<(), String> {
    Err("build requires the `codegen` feature".to_owned())
}

fn print_loaded_render<F>(path: &str, renderer: F) -> Result<(), String>
where
    F: FnOnce(&LoadedSource) -> Result<String, String>,
{
    let loaded = LoadedSource::load(path)?;
    let output = renderer(&loaded)?;
    print!("{output}");
    Ok(())
}

#[cfg(all(test, feature = "codegen"))]
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
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{LoadedSource, run_bootstrap_tool};

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

        let result = run_bootstrap_tool(move || {
            let loaded = LoadedSource::load(&package.to_string_lossy())?;
            crate::reports::render_bootstrap_format(&loaded)
        })
        .expect("tool should run");

        assert_eq!(
            result,
            "fn is_empty(source: Text) -> Bool {\n    text_len(source) == 0\n}\n\nfn format_text(source: Text) -> Text {\n    if is_empty(source) { \"empty\" } else { text_concat(source, \"!\") }\n}\n\nfn main() -> I32 {\n    0\n}\n"
        );
    }

    fn write_temp_package(name: &str, manifest: &str, sources: &[(&str, &str)]) -> PathBuf {
        let root = temp_root().join(format!("{}_{}", name, unique_id()));
        fs::create_dir_all(root.join("src")).expect("failed to create temp package root");
        fs::write(root.join("Sarif.toml"), manifest).expect("failed to write manifest");
        for (path, content) in sources {
            let full_path = root.join(path);
            fs::create_dir_all(full_path.parent().unwrap()).expect("failed to create parent dir");
            fs::write(full_path, content).expect("failed to write source");
        }
        root
    }

    fn unique_id() -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = UNIQUE_ID.fetch_add(1, Ordering::SeqCst);
        format!("{}_{}_{}", std::process::id(), timestamp, counter)
    }

    fn temp_root() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.verify-target/unit-tmp");
        fs::create_dir_all(&root).expect("unit temp root should exist");
        root
    }
}
