use std::{
    env, fs,
    hash::{DefaultHasher, Hash, Hasher},
    io,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

mod native;

use native::{native_build_plan, runtime_defines, runtime_metadata_source};
use sarif_codegen::Program;

static UNIQUE_TEMP_ID: AtomicU64 = AtomicU64::new(0);
const RUNTIME_SOURCE: &str = include_str!("../../../runtime/sarif_runtime.c");

pub fn link_executable(
    program: &Program,
    object: &[u8],
    output: &str,
    print_main: bool,
) -> Result<(), String> {
    ensure_supported_native_host()?;
    let temp_dir = TempDir::new("sarif_build")?;
    let build_plan = native_build_plan(program)?;
    let runtime_defines = runtime_defines(&build_plan, print_main);
    let linker = preferred_linker()?;
    let runtime_object = cached_runtime_object(&linker, &runtime_defines)?;

    let object_path = temp_dir.path().join("module.o");
    fs::write(&object_path, object)
        .map_err(|error| format!("failed to write `{}`: {error}", object_path.display()))?;
    let metadata_path = if let Some(metadata) = runtime_metadata_source(&build_plan) {
        let path = temp_dir.path().join("sarif_runtime_meta.c");
        fs::write(&path, metadata)
            .map_err(|error| format!("failed to write `{}`: {error}", path.display()))?;
        Some(path)
    } else {
        None
    };

    if let Some(parent) = Path::new(output).parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create `{}`: {error}", parent.display()))?;
    }

    let mut command = ProcessCommand::new(&linker.command);
    if let Some(ref flavor) = linker.flavor {
        command.arg(flavor);
    }
    command.env("TMPDIR", temp_dir.path());
    command
        .args(release_c_flags())
        .args(release_link_flags(linker.family))
        .arg("-std=c11");
    if let Some(path) = &metadata_path {
        command.arg(path);
    }
    command
        .arg(&runtime_object)
        .arg(&object_path)
        .arg("-o")
        .arg(output);

    let output_result = command
        .output()
        .map_err(|error| format!("failed to invoke `{}`: {error}", linker.command))?;
    if output_result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "native link failed with `{}` (args: {:?}):\nSTDOUT:\n{}\nSTDERR:\n{}",
            linker.command,
            command,
            String::from_utf8_lossy(&output_result.stdout),
            String::from_utf8_lossy(&output_result.stderr)
        ))
    }
}

fn cached_runtime_object(linker: &LinkerInvocation, defines: &[String]) -> Result<PathBuf, String> {
    let cache_root = env::temp_dir().join("sarif").join("runtime-cache");
    fs::create_dir_all(&cache_root)
        .map_err(|error| format!("failed to create `{}`: {error}", cache_root.display()))?;

    let flags = runtime_c_flags();
    let key = runtime_cache_key(&linker.command, &linker.identity, defines, &flags);
    let source_path = cache_root.join(format!("sarif_runtime_{key:016x}.c"));
    let object_path = cache_root.join(format!("sarif_runtime_{key:016x}.o"));
    if object_path.is_file() {
        return Ok(object_path);
    }
    if !source_path.is_file() {
        fs::write(&source_path, RUNTIME_SOURCE)
            .map_err(|error| format!("failed to write `{}`: {error}", source_path.display()))?;
    }

    let temp_object = cache_root.join(format!("sarif_runtime_{key:016x}.tmp.o"));
    let output = ProcessCommand::new(&linker.command)
        .args(&flags)
        .arg("-std=c11")
        .arg("-c")
        .args(defines)
        .arg(&source_path)
        .arg("-o")
        .arg(&temp_object)
        .output()
        .map_err(|error| format!("failed to invoke `{}`: {error}", linker.command))?;
    if !output.status.success() {
        return Err(format!(
            "native runtime compile failed with `{}`:\nSTDOUT:\n{}\nSTDERR:\n{}",
            linker.command,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    fs::rename(&temp_object, &object_path)
        .or_else(|_| {
            if object_path.is_file() {
                let _ = fs::remove_file(&temp_object);
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "failed to publish cached runtime object",
                ))
            }
        })
        .map_err(|error| format!("failed to publish `{}`: {error}", object_path.display()))?;
    Ok(object_path)
}

fn runtime_cache_key(
    linker: &str,
    linker_identity: &str,
    defines: &[String],
    flags: &[&str],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    linker.hash(&mut hasher);
    linker_identity.hash(&mut hasher);
    RUNTIME_SOURCE.hash(&mut hasher);
    for define in defines {
        define.hash(&mut hasher);
    }
    for flag in flags {
        flag.hash(&mut hasher);
    }
    hasher.finish()
}

fn preferred_linker() -> Result<LinkerInvocation, String> {
    if let Some(explicit) = explicit_linker_plan() {
        return explicit;
    }
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
    LinkerInvocation::new(driver.to_owned(), flavor.map(str::to_owned))
}

fn release_c_flags() -> Vec<&'static str> {
    let cpu_mode = env::var("SARIF_NATIVE_CPU").unwrap_or_else(|_| "native".to_owned());
    let lto_mode = env::var("SARIF_NATIVE_LTO").unwrap_or_else(|_| "off".to_owned());
    release_c_flags_for(&cpu_mode, &lto_mode)
}

fn release_c_flags_for(cpu_mode: &str, lto_mode: &str) -> Vec<&'static str> {
    let mut flags = vec![
        "-O3",
        "-fomit-frame-pointer",
        "-fno-math-errno",
        "-fno-trapping-math",
        "-fno-stack-protector",
        "-fno-ident",
        "-fno-unwind-tables",
        "-fno-asynchronous-unwind-tables",
        "-ffunction-sections",
        "-fdata-sections",
        "-fvisibility=hidden",
        "-pipe",
    ];
    if cpu_mode == "native" {
        flags.push("-march=native");
        flags.push("-mtune=native");
    }
    match lto_mode {
        "thin" => flags.push("-flto=thin"),
        "full" => flags.push("-flto"),
        _ => {}
    }
    flags
}

fn runtime_c_flags() -> Vec<&'static str> {
    let cpu_mode = env::var("SARIF_NATIVE_CPU").unwrap_or_else(|_| "native".to_owned());
    let lto_mode = env::var("SARIF_NATIVE_LTO").unwrap_or_else(|_| "off".to_owned());
    runtime_c_flags_for(&cpu_mode, &lto_mode)
}

fn runtime_c_flags_for(cpu_mode: &str, lto_mode: &str) -> Vec<&'static str> {
    let mut flags = vec![
        "-Os",
        "-fomit-frame-pointer",
        "-fno-math-errno",
        "-fno-trapping-math",
        "-fno-stack-protector",
        "-fno-ident",
        "-fno-unwind-tables",
        "-fno-asynchronous-unwind-tables",
        "-ffunction-sections",
        "-fdata-sections",
        "-fvisibility=hidden",
        "-pipe",
    ];
    if cpu_mode == "native" {
        flags.push("-march=native");
        flags.push("-mtune=native");
    }
    match lto_mode {
        "thin" => flags.push("-flto=thin"),
        "full" => flags.push("-flto"),
        _ => {}
    }
    flags
}

fn release_link_flags(family: LinkerFamily) -> Vec<&'static str> {
    let mut flags = match family {
        LinkerFamily::Elf => vec!["-Wl,--gc-sections", "-Wl,--build-id=none"],
        LinkerFamily::MachO => vec!["-Wl,-dead_strip"],
    };
    if cfg!(target_os = "linux") && family == LinkerFamily::Elf {
        flags.push("-Wl,-z,noseparate-code");
    }
    if matches!(env::consts::OS, "linux" | "android") && family == LinkerFamily::Elf {
        flags.push("-Wl,--as-needed");
    }
    if family == LinkerFamily::Elf {
        flags.push("-Wl,--icf=all");
    }
    flags
}

fn explicit_linker_plan() -> Option<Result<LinkerInvocation, String>> {
    let selection = env::var("SARIF_NATIVE_LINKER").ok()?;
    Some(explicit_linker_plan_for(&selection))
}

fn explicit_linker_plan_for(selection: &str) -> Result<LinkerInvocation, String> {
    match selection {
        "system" => LinkerInvocation::new("cc".to_owned(), None),
        "clang" => LinkerInvocation::new("clang".to_owned(), None),
        "lld" => LinkerInvocation::new("clang".to_owned(), Some("-fuse-ld=lld".to_owned())),
        "mold" => LinkerInvocation::new("clang".to_owned(), Some("-fuse-ld=mold".to_owned())),
        _ => Err(format!("unknown SARIF_NATIVE_LINKER `{selection}`")),
    }
}

fn ensure_supported_native_host() -> Result<(), String> {
    match env::consts::OS {
        "linux" | "macos" => Ok(()),
        "windows" => Err(
            "native build is not maintained on Windows yet; use `--target wasm` or a POSIX host"
                .to_owned(),
        ),
        other => Err(format!(
            "native build is not maintained on `{other}` yet; use `--target wasm` or a maintained POSIX host"
        )),
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkerFamily {
    Elf,
    MachO,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LinkerInvocation {
    command: String,
    flavor: Option<String>,
    family: LinkerFamily,
    identity: String,
}

impl LinkerInvocation {
    fn new(command: String, flavor: Option<String>) -> Result<Self, String> {
        let family = host_linker_family();
        let resolved = resolve_executable(&command).ok_or_else(|| {
            format!(
                "native build requires `{command}` on PATH; set `SARIF_NATIVE_LINKER` or install a supported C toolchain"
            )
        })?;
        let identity = tool_identity(&resolved)
            .map_err(|error| format!("failed to fingerprint `{}`: {error}", resolved.display()))?;
        Ok(Self {
            command,
            flavor,
            family,
            identity,
        })
    }
}

fn host_linker_family() -> LinkerFamily {
    match env::consts::OS {
        "macos" => LinkerFamily::MachO,
        _ => LinkerFamily::Elf,
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
    resolve_executable(name).is_some()
}

fn resolve_executable(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|entry| entry.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn tool_identity(path: &Path) -> io::Result<String> {
    let metadata = fs::metadata(path)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    Ok(format!(
        "{}:{}:{}",
        path.display(),
        metadata.len(),
        modified
    ))
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

#[cfg(test)]
mod tests {
    use super::{
        LinkerFamily, explicit_linker_plan_for, release_c_flags_for, release_link_flags,
        runtime_c_flags_for, runtime_cache_key,
    };

    #[test]
    fn release_flags_default_to_native_non_lto() {
        let flags = release_c_flags_for("native", "off");
        assert!(flags.contains(&"-O3"));
        assert!(flags.contains(&"-march=native"));
        assert!(flags.contains(&"-mtune=native"));
        assert!(flags.contains(&"-fno-unwind-tables"));
        assert!(flags.contains(&"-fno-asynchronous-unwind-tables"));
        assert!(flags.contains(&"-ffunction-sections"));
        assert!(flags.contains(&"-fdata-sections"));
        assert!(flags.contains(&"-fvisibility=hidden"));
        assert!(!flags.iter().any(|flag| flag.starts_with("-flto")));
    }

    #[test]
    fn release_flags_allow_portable_cpu_and_full_lto() {
        let flags = release_c_flags_for("baseline", "full");
        assert!(!flags.contains(&"-march=native"));
        assert!(!flags.contains(&"-mtune=native"));
        assert!(flags.contains(&"-flto"));
    }

    #[test]
    fn runtime_flags_bias_toward_size_without_losing_native_cpu_selection() {
        let flags = runtime_c_flags_for("native", "off");
        assert!(flags.contains(&"-Os"));
        assert!(!flags.contains(&"-O3"));
        assert!(flags.contains(&"-march=native"));
        assert!(flags.contains(&"-mtune=native"));
        assert!(flags.contains(&"-ffunction-sections"));
        assert!(flags.contains(&"-fdata-sections"));
    }

    #[test]
    fn release_link_flags_enable_section_gc_everywhere() {
        let flags = release_link_flags(LinkerFamily::Elf);
        assert!(flags.contains(&"-Wl,--gc-sections"));
        assert!(flags.contains(&"-Wl,--build-id=none"));
        if cfg!(target_os = "linux") {
            assert!(flags.contains(&"-Wl,-z,noseparate-code"));
        }
        assert!(flags.contains(&"-Wl,--icf=all"));
    }

    #[test]
    fn release_link_flags_enable_icf_for_lld_like_linkers() {
        let flags = release_link_flags(LinkerFamily::Elf);
        assert!(flags.contains(&"-Wl,--icf=all"));
    }

    #[test]
    fn release_link_flags_switch_to_dead_strip_on_macho() {
        let flags = release_link_flags(LinkerFamily::MachO);
        assert!(flags.contains(&"-Wl,-dead_strip"));
        assert!(!flags.contains(&"-Wl,--gc-sections"));
        assert!(!flags.contains(&"-Wl,--icf=all"));
    }

    #[test]
    fn runtime_cache_key_changes_when_linker_identity_changes() {
        let defines = vec!["-DSARIF_MAIN_KIND=1".to_owned()];
        let flags = vec!["-O3", "-std=c11"];
        let first = runtime_cache_key("clang", "/usr/bin/clang:100:1", &defines, &flags);
        let second = runtime_cache_key("clang", "/opt/clang/bin/clang:100:1", &defines, &flags);
        assert_ne!(first, second);
    }

    #[test]
    fn explicit_linker_plan_rejects_unknown_values() {
        assert!(explicit_linker_plan_for("unknown").is_err());
    }
}
