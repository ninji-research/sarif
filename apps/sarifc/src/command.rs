use sarif_frontend::semantic::Profile;

#[derive(Clone, Debug)]
pub struct Command {
    pub kind: CommandKind,
    pub path: String,
    pub profile: Profile,
    pub program_args: Vec<String>,
    pub print_main: bool,
    pub target: BuildTarget,
    pub output_path: Option<String>,
    pub dump_ir: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildTarget {
    Native,
    Wasm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandKind {
    Help,
    Version,
    Check,
    Doc,
    Format,
    BootstrapCheck,
    BootstrapDoc,
    BootstrapFormat,
    Run,
    Build,
}

#[must_use]
pub fn usage() -> String {
    let mut usage = "sarifc <command> <input> [options]\n\n".to_owned();
    usage += "commands:\n";
    usage += "  check             verify semantic correctness (default)\n";
    usage += "  doc               generate markdown documentation\n";
    usage += "  format            pretty-print source code\n";
    usage += "  bootstrap-check   maintained-semantic check bridge\n";
    usage += "  bootstrap-doc     maintained-semantic doc bridge\n";
    usage += "  bootstrap-format  retained formatter parity command\n";
    usage += "  run               execute the program's main function\n";
    usage += "                    append `-- <args>` to pass runtime args to `main` builtins\n";
    usage += "  build             compile to a native executable or wasm (`-o` required)\n";
    usage += "  help              show this help message\n";
    usage += "  version           show compiler version\n\n";
    usage += "profiles:\n";
    usage += "  --core            minimal safe language (default)\n";
    usage += "  --total           core + totality enforcement\n";
    usage += "  --rt              core + hard real-time enforcement\n\n";
    usage += "targets:\n";
    usage += "  --target native   compile to native executable (default)\n";
    usage += "  --target wasm     compile to binary webassembly (.wasm)\n\n";
    usage += "options:\n";
    usage += "  -o <path>         output path for build\n";
    usage +=
        "  --print-main      print native `main` results instead of using exit-code semantics\n";
    usage +=
        "  --dump-ir=<pass> dump IR after specific pass (resolve, typecheck, lower, codegen)\n";
    usage
}

pub fn parse_command(args: &[String]) -> Result<Command, String> {
    let mut kind = None;
    let mut path = None;
    let mut profile = Profile::Core;
    let mut program_args = Vec::new();
    let mut print_main = false;
    let mut target = BuildTarget::Native;
    let mut output_path = None;
    let mut dump_ir = None;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            program_args.extend(iter.cloned());
            break;
        }
        match arg.as_str() {
            "help" | "-h" | "--help" => kind = Some(CommandKind::Help),
            "version" | "-v" | "--version" => kind = Some(CommandKind::Version),
            "check" => kind = Some(CommandKind::Check),
            "doc" => kind = Some(CommandKind::Doc),
            "format" => kind = Some(CommandKind::Format),
            "bootstrap-check" => kind = Some(CommandKind::BootstrapCheck),
            "bootstrap-doc" => kind = Some(CommandKind::BootstrapDoc),
            "bootstrap-format" => kind = Some(CommandKind::BootstrapFormat),
            "run" => kind = Some(CommandKind::Run),
            "build" => kind = Some(CommandKind::Build),
            "--profile" => {
                if let Some(p) = iter.next() {
                    profile = match p.as_str() {
                        "core" => Profile::Core,
                        "total" => Profile::Total,
                        "rt" => Profile::Rt,
                        _ => return Err(format!("unknown profile `{p}`")),
                    };
                }
            }
            "--core" => profile = Profile::Core,
            "--total" => profile = Profile::Total,
            "--rt" => profile = Profile::Rt,
            "--target" => {
                if let Some(t) = iter.next() {
                    target = match t.as_str() {
                        "native" => BuildTarget::Native,
                        "wasm" => BuildTarget::Wasm,
                        _ => return Err(format!("unknown target `{t}`")),
                    };
                }
            }
            "-o" => {
                output_path = iter.next().cloned();
            }
            "--print-main" => {
                print_main = true;
            }
            other if other.starts_with("--dump-ir=") => {
                dump_ir = other.strip_prefix("--dump-ir=").map(String::from);
            }
            other if !other.starts_with('-') => {
                if path.replace(other.to_owned()).is_some() {
                    return Err(format!("unexpected positional argument `{other}`"));
                }
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }

    let kind = kind.unwrap_or(CommandKind::Check);
    if matches!(kind, CommandKind::Help | CommandKind::Version) {
        return Ok(Command {
            kind,
            path: String::new(),
            profile,
            program_args,
            print_main,
            target,
            output_path,
            dump_ir,
        });
    }

    let path = path.ok_or_else(|| "missing input file".to_owned())?;
    if !program_args.is_empty() && kind != CommandKind::Run {
        return Err("runtime arguments after `--` are only supported for `run`".to_owned());
    }
    if print_main && kind != CommandKind::Build {
        return Err("`--print-main` is only supported for `build`".to_owned());
    }

    Ok(Command {
        kind,
        path,
        profile,
        program_args,
        print_main,
        target,
        output_path,
        dump_ir,
    })
}

#[cfg(test)]
mod tests {
    use super::{BuildTarget, CommandKind, parse_command};
    use sarif_frontend::semantic::Profile;

    #[test]
    fn build_requires_the_output_flag_instead_of_a_second_positional_argument() {
        let error = parse_command(&[
            "build".to_owned(),
            "main.sarif".to_owned(),
            "out.bin".to_owned(),
        ])
        .expect_err("extra positional arguments should be rejected");
        assert!(error.contains("unexpected positional argument `out.bin`"));
    }

    #[test]
    fn build_parses_documented_options() {
        let command = parse_command(&[
            "build".to_owned(),
            "main.sarif".to_owned(),
            "--print-main".to_owned(),
            "--target".to_owned(),
            "wasm".to_owned(),
            "--profile".to_owned(),
            "total".to_owned(),
            "-o".to_owned(),
            "main.wasm".to_owned(),
        ])
        .expect("documented build command should parse");
        assert_eq!(command.kind, CommandKind::Build);
        assert_eq!(command.path, "main.sarif");
        assert_eq!(command.profile, Profile::Total);
        assert!(command.print_main);
        assert_eq!(command.target, BuildTarget::Wasm);
        assert_eq!(command.output_path.as_deref(), Some("main.wasm"));
    }

    #[test]
    fn run_parses_runtime_arguments_after_separator() {
        let command = parse_command(&[
            "run".to_owned(),
            "main.sarif".to_owned(),
            "--".to_owned(),
            "5000000".to_owned(),
        ])
        .expect("run args should parse");
        assert_eq!(command.kind, CommandKind::Run);
        assert_eq!(command.program_args, vec!["5000000"]);
    }

    #[test]
    fn non_run_commands_reject_runtime_arguments_after_separator() {
        let error = parse_command(&[
            "check".to_owned(),
            "main.sarif".to_owned(),
            "--".to_owned(),
            "5000000".to_owned(),
        ])
        .expect_err("only run should accept runtime args");
        assert_eq!(
            error,
            "runtime arguments after `--` are only supported for `run`"
        );
    }

    #[test]
    fn print_main_is_rejected_outside_build() {
        let error = parse_command(&[
            "run".to_owned(),
            "main.sarif".to_owned(),
            "--print-main".to_owned(),
        ])
        .expect_err("only build should accept --print-main");
        assert_eq!(error, "`--print-main` is only supported for `build`");
    }
}
