use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

mod native;

use native::runtime_metadata_source;
use sarif_codegen::{Program, collect_native_enums, collect_native_records};

static UNIQUE_TEMP_ID: AtomicU64 = AtomicU64::new(0);
const RUNTIME_SOURCE: &str = include_str!("../../../runtime/sarif_runtime.c");

pub fn link_executable(
    program: &Program,
    object: &[u8],
    output: &str,
    print_main: bool,
) -> Result<(), String> {
    let temp_dir = TempDir::new("sarif_build")?;

    let object_path = temp_dir.path().join("module.o");
    let runtime_path = temp_dir.path().join("sarif_runtime.c");
    let metadata_path = temp_dir.path().join("sarif_runtime_meta.c");
    fs::write(&object_path, object)
        .map_err(|error| format!("failed to write `{}`: {error}", object_path.display()))?;
    fs::write(&runtime_path, RUNTIME_SOURCE)
        .map_err(|error| format!("failed to write `{}`: {error}", runtime_path.display()))?;
    fs::write(&metadata_path, runtime_metadata_source(program)?)
        .map_err(|error| format!("failed to write `{}`: {error}", metadata_path.display()))?;

    if let Some(parent) = Path::new(output).parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create `{}`: {error}", parent.display()))?;
    }

    let (linker, linker_flavor) = preferred_linker();
    let mut command = ProcessCommand::new(&linker);
    if let Some(flavor) = linker_flavor {
        command.arg(flavor);
    }
    command.env("TMPDIR", temp_dir.path());
    for define in runtime_defines(program, print_main)? {
        command.arg(define);
    }
    command
        .arg("-std=c11")
        .arg(&runtime_path)
        .arg(&metadata_path)
        .arg(&object_path)
        .arg("-o")
        .arg(output);

    let output_result = command
        .output()
        .map_err(|error| format!("failed to invoke `{linker}`: {error}"))?;
    if output_result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "native link failed with `{linker}` (args: {:?}):\nSTDOUT:\n{}\nSTDERR:\n{}",
            command,
            String::from_utf8_lossy(&output_result.stdout),
            String::from_utf8_lossy(&output_result.stderr)
        ))
    }
}

fn runtime_defines(program: &Program, print_main: bool) -> Result<Vec<String>, String> {
    let main = program
        .functions
        .iter()
        .find(|function| function.name == "main")
        .ok_or_else(|| "missing `main` entrypoint".to_owned())?;
    let records = collect_native_records(program)?;
    let enums = collect_native_enums(program);
    let kind = match main.return_type.as_deref().unwrap_or("Unit") {
        "I32" => 1,
        "Bool" => 2,
        "Text" => 3,
        "F64" => 6,
        "Unit" => 0,
        other if records.contains_key(other) => 4,
        other if enums.contains_key(other) => 5,
        other => Err(format!(
            "native build does not support `main` returning `{other}` in stage-0"
        ))?,
    };
    let print_flag = i32::from(print_main);
    Ok(vec![
        format!("-DSARIF_MAIN_KIND={kind}"),
        format!("-DSARIF_MAIN_PRINT={print_flag}"),
    ])
}

fn preferred_linker() -> (String, Option<String>) {
    let driver = if linker_available("clang") || !linker_available("cc") {
        CompilerDriver::Clang
    } else {
        CompilerDriver::Cc
    };
    let (driver, flavor) = linker_plan(
        driver,
        linker_available("mold") || linker_available("ld.mold"),
        linker_available("ld.lld"),
    );
    (driver.to_owned(), flavor.map(str::to_owned))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompilerDriver {
    Clang,
    Cc,
}

impl CompilerDriver {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Clang => "clang",
            Self::Cc => "cc",
        }
    }
}

fn linker_plan(
    driver: CompilerDriver,
    has_mold: bool,
    has_lld: bool,
) -> (&'static str, Option<&'static str>) {
    let flavor = if has_mold {
        Some("-fuse-ld=mold")
    } else if driver == CompilerDriver::Clang && has_lld {
        Some("-fuse-ld=lld")
    } else {
        None
    };
    (driver.as_str(), flavor)
}

fn linker_available(name: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|path| env::split_paths(&path).any(|entry| executable_exists(&entry, name)))
}

fn executable_exists(directory: &Path, name: &str) -> bool {
    let candidate = directory.join(name);
    candidate.is_file()
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let root = env::temp_dir().join("sarif");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    let counter = UNIQUE_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    root.join(format!(
        "{prefix}_{}_{}_{}",
        std::process::id(),
        timestamp,
        counter
    ))
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Result<Self, String> {
        let path = unique_temp_dir(prefix);
        fs::create_dir_all(&path)
            .map_err(|error| format!("failed to create `{}`: {error}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
