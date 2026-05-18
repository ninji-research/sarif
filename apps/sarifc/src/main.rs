use std::{env, fs, io::Read, process::ExitCode};

#[cfg(feature = "native-build")]
mod artifact;
mod command;
mod input;
mod reports;

#[cfg(feature = "native-build")]
use artifact::link_executable;
use command::{BuildTarget, CommandKind, parse_command, usage};
use input::resolve_input;
#[cfg(feature = "codegen")]
use reports::{render_bootstrap_check, render_bootstrap_format};
use reports::{
    render_package_diagnostics, render_semantic_check, render_semantic_doc, render_semantic_format,
};
#[cfg(feature = "codegen")]
use sarif_codegen::{RuntimeError, RuntimeValue, analyze_escapes, lower as lower_mir};
#[cfg(feature = "codegen")]
use sarif_codegen::{emit_clif, emit_object};
#[cfg(feature = "wasm")]
use sarif_codegen::{emit_wasm, emit_wat};
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
        let escape_diags = analyze_escapes(&self.mir().program);
        for mut diag in escape_diags {
            if profile != Profile::Rt {
                diag.code = "semantic.alloc-escape";
            }
            diagnostics.push(diag);
        }
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

    fn blocking_diagnostics(diagnostics: &[Diagnostic], profile: Profile) -> Vec<Diagnostic> {
        diagnostics
            .iter()
            .filter(|d| {
                if profile == Profile::Rt {
                    d.code != "semantic.alloc-escape"
                } else {
                    d.code != "semantic.alloc-escape" && d.code != "escape.analysis.required"
                }
            })
            .cloned()
            .collect()
    }
}

fn exit_code_from_result(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    match parse_command(&args[1..]) {
        Ok(command) => run_command(command),
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run_command(command: command::Command) -> ExitCode {
    match command.kind {
        CommandKind::Help => {
            println!("{}", usage());
            ExitCode::SUCCESS
        }
        CommandKind::Version => {
            println!("sarifc {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        CommandKind::Check => exit_code_from_result(run_check(&command)),
        CommandKind::Format => exit_code_from_result(run_format(&command)),
        CommandKind::BootstrapFormat => exit_code_from_result(run_bootstrap_format(&command)),
        CommandKind::Doc => exit_code_from_result(run_doc(&command)),
        CommandKind::BootstrapCheck => exit_code_from_result(run_bootstrap_check(&command)),
        CommandKind::BootstrapDoc => exit_code_from_result(run_bootstrap_doc(&command)),
        CommandKind::Run => run_program(command),
        CommandKind::Build => exit_code_from_result(build_program(&command)),
    }
}

fn run_check(command: &command::Command) -> Result<(), String> {
    print_loaded_render(command, |loaded| {
        emit_requested_dump(loaded, command)?;
        render_semantic_check(loaded, command.profile)
    })
}

fn run_format(command: &command::Command) -> Result<(), String> {
    print_loaded_render(command, |loaded| {
        emit_requested_dump(loaded, command)?;
        render_semantic_format(loaded)
    })
}

fn run_doc(command: &command::Command) -> Result<(), String> {
    print_loaded_render(command, |loaded| {
        emit_requested_dump(loaded, command)?;
        render_semantic_doc(loaded, command.profile)
    })
}

fn run_bootstrap_format(command: &command::Command) -> Result<(), String> {
    let loaded = LoadedSource::load(&command.path)?;
    emit_requested_dump(&loaded, command)?;
    #[cfg(feature = "codegen")]
    {
        let path = command.path.clone();
        let result = std::thread::Builder::new()
            .name("bootstrap-format".to_owned())
            .stack_size(48 * 1024 * 1024)
            .spawn(move || {
                let mem_loaded = LoadedSource::load(&path)?;
                render_bootstrap_format(&mem_loaded)
            })
            .map_err(|e| format!("failed to spawn bootstrap thread: {e}"))?
            .join()
            .map_err(|_| "bootstrap format thread panicked".to_owned())?;
        print!("{}", result?);
        Ok(())
    }
    #[cfg(not(feature = "codegen"))]
    {
        let _ = loaded;
        Err("bootstrap format requires the `codegen` feature".to_owned())
    }
}

fn run_bootstrap_check(command: &command::Command) -> Result<(), String> {
    let loaded = LoadedSource::load(&command.path)?;
    emit_requested_dump(&loaded, command)?;
    #[cfg(feature = "codegen")]
    {
        let path = command.path.clone();
        let result = std::thread::Builder::new()
            .name("bootstrap-check".to_owned())
            .stack_size(48 * 1024 * 1024)
            .spawn(move || {
                let mem_loaded = LoadedSource::load(&path)?;
                render_bootstrap_check(&mem_loaded)
            })
            .map_err(|e| format!("failed to spawn bootstrap thread: {e}"))?
            .join()
            .map_err(|_| "bootstrap check thread panicked".to_owned())?;
        print!("{}", result?);
        Ok(())
    }
    #[cfg(not(feature = "codegen"))]
    {
        let _ = loaded;
        Err("bootstrap check requires the `codegen` feature".to_owned())
    }
}

fn run_bootstrap_doc(command: &command::Command) -> Result<(), String> {
    // bootstrap-doc uses the Rust doc generator (Sarif bootstrap doesn't
    // have full doc parity yet; format and basic check are flipped).
    print_loaded_render(command, |loaded| {
        emit_requested_dump(loaded, command)?;
        render_semantic_doc(loaded, command.profile)
    })
}

#[cfg(feature = "codegen")]
fn run_program(command: command::Command) -> ExitCode {
    let loaded = match LoadedSource::load(&command.path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let diagnostics = loaded.mir_diagnostics(command.profile);
    if let Err(msg) = loaded.ensure_no_diagnostics(
        &LoadedSource::blocking_diagnostics(&diagnostics, command.profile),
        "execution failed",
    ) {
        eprintln!("{msg}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = emit_requested_dump(&loaded, &command) {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    }

    let mut program_args = vec![command.path];
    program_args.extend(command.program_args);
    let mut stdin_text = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut stdin_text) {
        eprintln!("failed to read stdin: {e}");
        return ExitCode::FAILURE;
    }

    let program = loaded.mir().program.clone();
    let (result, stdout_text) = match std::thread::Builder::new()
        .name("sarif-run".to_owned())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || sarif_codegen::run_main_with_io_capture(&program, &program_args, stdin_text))
    {
        Ok(handle) => match handle.join() {
            Ok(Ok((result, stdout_text))) => (result, stdout_text),
            Ok(Err(e)) => {
                let message = match e {
                    RuntimeError::Message(m) => m,
                    RuntimeError::EffectUnwind {
                        effect, operation, ..
                    } => format!("unhandled effect {effect}.{operation}"),
                };
                eprintln!("runtime error: {message}");
                return ExitCode::FAILURE;
            }
            Err(_) => {
                eprintln!("runtime error: run thread panicked");
                return ExitCode::FAILURE;
            }
        },
        Err(e) => {
            eprintln!("failed to start run thread: {e}");
            return ExitCode::FAILURE;
        }
    };
    print!("{stdout_text}");
    if !matches!(result, RuntimeValue::Unit) {
        println!("{}", result.render());
    }
    runtime_value_to_exit_code(&result)
}

fn runtime_value_to_exit_code(value: &RuntimeValue) -> ExitCode {
    match value {
        RuntimeValue::Int(i) => {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let exit = *i as u8;
            ExitCode::from(exit)
        }
        RuntimeValue::Bool(b) => {
            if *b {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        RuntimeValue::F64(_)
        | RuntimeValue::Text(_)
        | RuntimeValue::Bytes(_)
        | RuntimeValue::TextIndex(_)
        | RuntimeValue::TextBuilder(_)
        | RuntimeValue::List(_)
        | RuntimeValue::Enum(_)
        | RuntimeValue::Record(_)
        | RuntimeValue::Unit => ExitCode::SUCCESS,
    }
}

#[cfg(not(feature = "codegen"))]
fn run_program(_command: command::Command) -> ExitCode {
    eprintln!("run requires the `codegen` feature");
    ExitCode::FAILURE
}

#[cfg(feature = "codegen")]
fn build_program(command: &command::Command) -> Result<(), String> {
    let loaded = LoadedSource::load(&command.path)?;
    let all_diagnostics = loaded.mir_diagnostics(command.profile);
    loaded.ensure_no_diagnostics(
        &LoadedSource::blocking_diagnostics(&all_diagnostics, command.profile),
        "build failed",
    )?;
    emit_requested_dump(&loaded, command)?;

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
                    .map_err(|error| format!("failed to emit object file: {error}"))?;

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

fn print_loaded_render<F>(command: &command::Command, renderer: F) -> Result<(), String>
where
    F: FnOnce(&LoadedSource) -> Result<String, String>,
{
    let loaded = LoadedSource::load(&command.path)?;
    let output = renderer(&loaded)?;
    print!("{output}");
    Ok(())
}

fn emit_requested_dump(loaded: &LoadedSource, command: &command::Command) -> Result<(), String> {
    let Some(pass) = command.dump_ir.as_deref() else {
        return Ok(());
    };

    let rendered = match pass {
        "resolve" => loaded.database.hir(loaded.source_id).module.pretty(),
        "typecheck" => render_semantic_doc(loaded, command.profile)?,
        #[cfg(feature = "codegen")]
        "lower" => render_lower_dump(loaded),
        #[cfg(not(feature = "codegen"))]
        "lower" => return Err("lower IR dumps require the `codegen` feature".to_owned()),
        #[cfg(feature = "native-build")]
        "clif" => emit_clif(&loaded.mir().program).map_err(|e| e.message)?,
        #[cfg(not(feature = "native-build"))]
        "clif" => return Err("clif IR dumps require the `native-build` feature".to_owned()),
        "codegen" => render_codegen_dump(loaded, command)?,
        other => {
            return Err(format!(
                "unknown IR dump pass `{other}`; expected resolve, typecheck, lower, clif, or codegen"
            ));
        }
    };
    println!("{rendered}");
    Ok(())
}

#[cfg(feature = "codegen")]
fn render_lower_dump(loaded: &LoadedSource) -> String {
    loaded.mir().program.pretty()
}

#[cfg(feature = "wasm")]
fn render_codegen_dump(
    loaded: &LoadedSource,
    command: &command::Command,
) -> Result<String, String> {
    if command.target != BuildTarget::Wasm {
        return Err(
            "codegen IR dumps are currently supported only with `--target wasm`".to_owned(),
        );
    }
    emit_wat(&loaded.mir().program).map_err(|error| error.message)
}

#[cfg(all(feature = "codegen", not(feature = "wasm")))]
fn render_codegen_dump(
    _loaded: &LoadedSource,
    _command: &command::Command,
) -> Result<String, String> {
    Err("codegen IR dumps are currently supported only with `--target wasm`".to_owned())
}

#[cfg(not(feature = "codegen"))]
fn render_codegen_dump(
    _loaded: &LoadedSource,
    _command: &command::Command,
) -> Result<String, String> {
    Err("codegen IR dumps require the `codegen` feature".to_owned())
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
