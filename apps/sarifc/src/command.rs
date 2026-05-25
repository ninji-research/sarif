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
    pub inspect: Option<String>,
    pub semantic: bool,
    pub debug: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildTarget {
    Native,
    Wasm,
    C,
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
    usage += "  bootstrap-check   run self-hosted semantic checks (stage-0 WASM)\n";
    usage += "  bootstrap-doc     run self-hosted doc generation (stage-0 WASM)\n";
    usage += "  bootstrap-format  run self-hosted formatter (stage-0 WASM)\n";
    usage += "  run               execute the program's main function\n";
    usage += "                    append `-- <args>` to pass runtime args to `main` builtins\n";
    usage += "  build             compile to native, wasm, or C (`-o` required)\n";
    usage += "  help              show this help message\n";
    usage += "  version           show compiler version\n\n";
    usage += "profiles:\n";
    usage += "  --core            minimal safe language (default)\n";
    usage += "  --total           core + totality enforcement\n";
    usage += "  --rt              core + hard real-time enforcement\n\n";
    usage += "targets:\n";
    usage += "  --target native   compile to native executable (default)\n";
    usage += "  --target wasm     compile to binary webassembly (.wasm)\n";
    usage += "  --target c        emit C source code (.c)\n\n";
    usage += "options:\n";
    usage += "  -o <path>         output path for build\n";
    usage +=
        "  --print-main      print native `main` results instead of using exit-code semantics\n";
    usage += "  --semantic        use the Rust semantic backend (default is stage-0 bootstrap)\n";
    usage +=
        "  --dump-ir=<pass>  dump IR after a compiler pass (hir, semantic, mir, cranelift, wasm, c)\n";
    usage += "                    wasm/c dumps require `--target wasm` or `--target c`\n";
    usage += "  --inspect=<tool>  inspect build output (wasmprinter; only for `build`)\n";
    usage += "  --debug           enable target runtime null-pointer trap checks\n";
    usage
}

const COMMAND_NAMES: &[&str] = &[
    "check",
    "doc",
    "format",
    "bootstrap-check",
    "bootstrap-doc",
    "bootstrap-format",
    "run",
    "build",
    "help",
    "version",
];

fn edit_distance(a: &str, b: &str) -> usize {
    let la = a.len();
    let lb = b.len();
    let mut prev: Vec<usize> = (0..=la).collect();
    let mut curr = vec![0; la + 1];
    for i in 1..=lb {
        curr[0] = i;
        for j in 1..=la {
            let cost = usize::from(a.as_bytes().get(j - 1) != b.as_bytes().get(i - 1));
            curr[j] = (curr[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[la]
}

fn closest_command(name: &str) -> Option<&'static str> {
    const THRESHOLD: usize = 3;
    let mut best: Option<(&str, usize)> = None;
    for &cmd in COMMAND_NAMES {
        let dist = edit_distance(name, cmd);
        if dist <= THRESHOLD {
            best = match best {
                None => Some((cmd, dist)),
                Some((_, d)) if dist < d => Some((cmd, dist)),
                _ => best,
            };
        }
    }
    best.map(|(cmd, _)| cmd)
}

#[allow(clippy::too_many_lines)]
fn parse_command_inner(args: &[String]) -> Result<Command, String> {
    let mut kind = None;
    let mut path = None;
    let mut profile = Profile::Core;
    let mut program_args = Vec::new();
    let mut print_main = false;
    let mut target = BuildTarget::Native;
    let mut output_path = None;
    let mut dump_ir = None;
    let mut inspect = None;
    let mut semantic = false;
    let mut debug = false;

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
                        "c" => BuildTarget::C,
                        _ => return Err(format!("unknown target `{t}`")),
                    };
                }
            }
            "-o" => output_path = iter.next().cloned(),
            "--print-main" => print_main = true,
            "--semantic" => semantic = true,
            "--debug" => debug = true,
            other if other.starts_with("--dump-ir=") => {
                dump_ir = other.strip_prefix("--dump-ir=").map(String::from);
            }
            other if other.starts_with("--inspect=") => {
                inspect = other.strip_prefix("--inspect=").map(String::from);
            }
            other if !other.starts_with('-') => {
                if kind.is_none() {
                    if let Some(suggestion) = closest_command(other) {
                        return Err(format!(
                            "unknown command `{other}` (did you mean `{suggestion}`?)"
                        ));
                    }
                    return Err(format!("unknown command `{other}`"));
                }
                if path.replace(other.to_owned()).is_some() {
                    return Err(format!("unexpected positional argument `{other}`"));
                }
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }

    let kind = kind.unwrap_or_else(|| {
        if path.is_none() && args.is_empty() {
            CommandKind::Help
        } else {
            CommandKind::Check
        }
    });
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
            inspect,
            semantic,
            debug,
        });
    }

    let path = path.ok_or_else(|| "missing input file".to_owned())?;
    if !program_args.is_empty() && kind != CommandKind::Run {
        return Err("runtime arguments after `--` are only supported for `run`".to_owned());
    }
    if print_main && kind != CommandKind::Build {
        return Err("`--print-main` is only supported for `build`".to_owned());
    }
    if inspect.is_some() && kind != CommandKind::Build {
        return Err("`--inspect` is only supported for `build`".to_owned());
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
        inspect,
        semantic,
        debug,
    })
}

pub fn parse_command(args: &[String]) -> Result<Command, String> {
    parse_command_inner(args)
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

    #[test]
    fn unknown_command_shows_suggestion() {
        let error =
            parse_command(&["chek".to_owned(), "foo.sarif".to_owned()]).expect_err("should reject");
        assert!(error.contains("unknown command `chek`"));
        assert!(error.contains("did you mean `check`?"));
    }

    #[test]
    fn empty_args_shows_help() {
        let command = parse_command(&[]).expect("empty args should not error");
        assert_eq!(command.kind, CommandKind::Help);
    }
}
