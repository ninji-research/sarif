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
use reports::{render_bootstrap_check, render_bootstrap_doc, render_bootstrap_format};
use reports::{render_package_diagnostics, render_semantic_doc};
#[cfg(feature = "c-backend")]
use sarif_codegen::c::emit_c;
#[cfg(feature = "native-build")]
use sarif_codegen::emit_clif;
#[cfg(feature = "native-build")]
use sarif_codegen::emit_object;
#[cfg(feature = "codegen")]
use sarif_codegen::{RuntimeError, RuntimeValue, analyze_escapes, lower as lower_mir};
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
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
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
    let loaded = LoadedSource::load(&command.path)?;
    emit_requested_dump(&loaded, command)?;
    let all_diagnostics = loaded.mir_diagnostics(command.profile);
    if command.format.as_deref() == Some("sarif") {
        let sarif_json = reports::render_sarif_json(&loaded, &all_diagnostics, command.profile);
        print!("{sarif_json}");
        let blocking = LoadedSource::blocking_diagnostics(&all_diagnostics, command.profile);
        if !blocking.is_empty() {
            return Err(String::new());
        }
    } else {
        loaded.ensure_no_diagnostics(
            &LoadedSource::blocking_diagnostics(&all_diagnostics, command.profile),
            "check failed",
        )?;
        println!("ok [{}]", command.profile.keyword());
    }
    Ok(())
}

fn run_format(command: &command::Command) -> Result<(), String> {
    run_bootstrap_format(command)
}

fn run_doc(command: &command::Command) -> Result<(), String> {
    run_bootstrap_doc(command)
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
    print_loaded_render(command, |loaded| {
        emit_requested_dump(loaded, command)?;
        render_bootstrap_doc(loaded)
    })
}

#[cfg(feature = "codegen")]
fn run_program(command: command::Command) -> ExitCode {
    #[cfg(feature = "native-build")]
    sarif_codegen::native_set_debug(command.debug);
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

    #[cfg(feature = "native-build")]
    let run_fn = sarif_codegen::run_main_native_with_io_capture;
    #[cfg(not(feature = "native-build"))]
    let run_fn = sarif_codegen::run_main_with_io_capture;

    let (result, stdout_text) = match std::thread::Builder::new()
        .name("sarif-run".to_owned())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || run_fn(&program, &program_args, stdin_text))
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
        | RuntimeValue::BytesView { .. }
        | RuntimeValue::TextIndex(_)
        | RuntimeValue::TextBuilder(_)
        | RuntimeValue::List(_)
        | RuntimeValue::Enum(_)
        | RuntimeValue::Record(_)
        | RuntimeValue::File(_)
        | RuntimeValue::Unit => ExitCode::SUCCESS,
    }
}

#[cfg(not(feature = "codegen"))]
fn run_program(_command: command::Command) -> ExitCode {
    eprintln!("run requires the `codegen` feature");
    ExitCode::FAILURE
}

#[cfg(feature = "codegen")]
#[allow(clippy::too_many_lines)]
fn build_program(command: &command::Command) -> Result<(), String> {
    #[cfg(feature = "native-build")]
    sarif_codegen::native_set_debug(command.debug);
    let loaded = LoadedSource::load(&command.path)?;
    let all_diagnostics = loaded.mir_diagnostics(command.profile);
    if command.format.as_deref() == Some("sarif") {
        let sarif_json = reports::render_sarif_json(&loaded, &all_diagnostics, command.profile);
        print!("{sarif_json}");
        let blocking = LoadedSource::blocking_diagnostics(&all_diagnostics, command.profile);
        if !blocking.is_empty() {
            return Err(String::new());
        }
    } else {
        loaded.ensure_no_diagnostics(
            &LoadedSource::blocking_diagnostics(&all_diagnostics, command.profile),
            "build failed",
        )?;
    }
    emit_requested_dump(&loaded, command)?;

    let output_path = command
        .output_path
        .as_deref()
        .ok_or("missing output path")?;

    build_for_target(command, &loaded, output_path)?;
    emit_requested_inspect(&loaded, command)?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn build_for_target(
    command: &command::Command,
    loaded: &LoadedSource,
    output_path: &str,
) -> Result<(), String> {
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
        BuildTarget::C => {
            #[cfg(feature = "c-backend")]
            {
                use std::path::Path;
                use std::process::Command;

                let c_source =
                    emit_c(&loaded.mir().program).map_err(|e| format!("c codegen failed: {e}"))?;

                // Determine main_kind from main function's return type
                let main_func = loaded
                    .mir()
                    .program
                    .functions
                    .iter()
                    .find(|f| f.name == "main")
                    .ok_or_else(|| "missing `main` entrypoint".to_owned())?;
                let main_result_type = main_func.return_type.as_deref().unwrap_or("Unit");
                let main_kind = match main_result_type {
                    "I32" => 1,
                    "Bool" => 2,
                    "Text" => 3,
                    "F64" => 6,
                    "Unit" => 0,
                    other => {
                        return Err(format!(
                            "c backend does not support `main` returning `{other}`"
                        ));
                    }
                };

                // Create output directory if needed
                let output_path_obj = Path::new(output_path);
                if let Some(parent) = output_path_obj.parent()
                    && !parent.as_os_str().is_empty()
                {
                    fs::create_dir_all(parent).map_err(|error| {
                        format!("failed to create `{}`: {error}", parent.display())
                    })?;
                }

                // Write C source to a .c file
                let c_source_path = format!("{output_path}.c");
                fs::write(&c_source_path, &c_source).map_err(|error| {
                    format!("failed to write C file `{c_source_path}`: {error}")
                })?;

                // Compile C source to executable with clang
                let mut cmd = Command::new("clang");
                cmd.arg("-O3")
                    .arg("-std=c11")
                    .arg("-Wall")
                    .arg("-Wextra")
                    .arg(format!("-DSARIF_MAIN_KIND={main_kind}"))
                    .arg("-o")
                    .arg(output_path)
                    .arg(&c_source_path)
                    .arg(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../runtime/sarif_runtime.c"
                    ))
                    .arg("-lm");

                if let Some(parent) = output_path_obj.parent()
                    && !parent.as_os_str().is_empty()
                {
                    cmd.env("TMPDIR", parent);
                }

                let status = cmd
                    .status()
                    .map_err(|e| format!("failed to run clang: {e}"))?;
                if !status.success() {
                    return Err(format!(
                        "clang compilation failed with exit code {}",
                        status.code().unwrap_or(-1)
                    ));
                }

                // Clean up intermediate .c file
                let _ = fs::remove_file(&c_source_path);

                Ok(())
            }
            #[cfg(not(feature = "c-backend"))]
            {
                Err("c backend requires the `c-backend` feature".to_owned())
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
        "hir" | "resolve" => loaded.database.hir(loaded.source_id).module.pretty(),
        "semantic" | "typecheck" | "sem" => render_semantic_doc(loaded, command.profile)?,
        #[cfg(feature = "codegen")]
        "mir" | "lower" => render_lower_dump(loaded),
        #[cfg(not(feature = "codegen"))]
        "mir" | "lower" => return Err("MIR dumps require the `codegen` feature".to_owned()),
        #[cfg(feature = "native-build")]
        "cranelift" | "clif" => emit_clif(&loaded.mir().program).map_err(|e| e.message)?,
        #[cfg(not(feature = "native-build"))]
        "cranelift" | "clif" => {
            return Err("cranelift IR dumps require the `native-build` feature".to_owned());
        }
        "wasm" | "c" | "codegen" => render_codegen_dump(loaded, command)?,
        other => {
            return Err(format!(
                "unknown IR dump pass `{other}`; expected hir, semantic, mir, cranelift, wasm, or c"
            ));
        }
    };
    println!("{rendered}");
    Ok(())
}

fn emit_requested_inspect(loaded: &LoadedSource, command: &command::Command) -> Result<(), String> {
    let Some(tool) = command.inspect.as_deref() else {
        return Ok(());
    };
    match tool {
        #[cfg(feature = "wasm")]
        "wasmprinter" => {
            let wat = emit_wat(&loaded.mir().program).map_err(|error| error.message)?;
            println!("{wat}");
            Ok(())
        }
        #[cfg(not(feature = "wasm"))]
        "wasmprinter" => Err("wasmprinter requires the `wasm` feature".to_owned()),
        other => Err(format!(
            "unknown inspect tool `{other}`; expected wasmprinter"
        )),
    }
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
    if command.target != BuildTarget::Wasm && command.target != BuildTarget::C {
        return Err(
            "codegen IR dumps are currently supported only with `--target wasm` or `--target c`"
                .to_owned(),
        );
    }
    if command.target == BuildTarget::C {
        return emit_c(&loaded.mir().program).map_err(|e| format!("c codegen failed: {e}"));
    }
    emit_wat(&loaded.mir().program).map_err(|error| error.message)
}

#[cfg(all(feature = "codegen", not(feature = "wasm")))]
#[allow(clippy::used_underscore_binding)]
fn render_codegen_dump(
    _loaded: &LoadedSource,
    command: &command::Command,
) -> Result<String, String> {
    if command.target == BuildTarget::C {
        #[cfg(feature = "c-backend")]
        return emit_c(&_loaded.mir().program).map_err(|e| format!("c codegen failed: {e}"));
        #[cfg(not(feature = "c-backend"))]
        return Err("c backend requires the `c-backend` feature".to_owned());
    }
    Err(
        "codegen IR dumps are currently supported only with `--target wasm` or `--target c`"
            .to_owned(),
    )
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
