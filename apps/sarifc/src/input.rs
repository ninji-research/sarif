use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

use toml::Value;

#[derive(Clone, Debug)]
pub struct PackageIdentity {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug)]
pub struct ResolvedInput {
    pub source_paths: Vec<String>,
    pub display_path: String,
    pub package: PackageIdentity,
}

pub fn resolve_input(path: &str) -> Result<ResolvedInput, String> {
    let input = Path::new(path);
    if input.is_dir() {
        return resolve_manifest(&input.join("Sarif.toml"));
    }
    if input
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "Sarif.toml")
    {
        return resolve_manifest(input);
    }

    Ok(ResolvedInput {
        source_paths: vec![path.to_owned()],
        display_path: path.to_owned(),
        package: PackageIdentity::standalone(input),
    })
}

fn resolve_manifest(path: &Path) -> Result<ResolvedInput, String> {
    let manifest = fs::read_to_string(path)
        .map_err(|error| format!("failed to read `{}`: {error}", path.display()))?;
    let parsed = manifest.parse::<toml::Table>().map_err(|error| {
        format!(
            "failed to parse manifest `{}` as TOML: {error}",
            path.display()
        )
    })?;
    let package = parsed
        .get("package")
        .and_then(Value::as_table)
        .ok_or_else(|| format!("manifest `{}` is missing a [package] table", path.display()))?;
    let name = required_string(package, "name", path)?;
    let version = required_string(package, "version", path)?;

    let root = path
        .parent()
        .ok_or_else(|| format!("manifest `{}` has no parent directory", path.display()))?;
    let source_paths = manifest_sources(package, root, path)?;

    Ok(ResolvedInput {
        source_paths,
        display_path: path.display().to_string(),
        package: PackageIdentity { name, version },
    })
}

impl PackageIdentity {
    fn standalone(path: &Path) -> Self {
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("main")
            .to_owned();
        Self {
            name,
            version: "0.0.0".to_owned(),
        }
    }

    pub fn symbol_stem(&self) -> String {
        format!("{}_{}", self.name, self.version)
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect()
    }
}

fn required_string(package: &toml::Table, key: &str, path: &Path) -> Result<String, String> {
    package
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("manifest `{}` is missing package.{key}", path.display()))
}

fn manifest_sources(
    package: &toml::Table,
    root: &Path,
    path: &Path,
) -> Result<Vec<String>, String> {
    let entries = package
        .get("sources")
        .map(|value| {
            value.as_array().ok_or_else(|| {
                format!(
                    "manifest `{}` has non-array package.sources",
                    path.display()
                )
            })
        })
        .transpose()?;

    let relative_paths = if let Some(entries) = entries {
        if entries.is_empty() {
            return Err(format!(
                "manifest `{}` must list at least one package.sources entry",
                path.display()
            ));
        }
        let mut paths = Vec::with_capacity(entries.len());
        for entry in entries {
            let relative = entry.as_str().ok_or_else(|| {
                format!(
                    "manifest `{}` contains a non-string package.sources entry",
                    path.display()
                )
            })?;
            paths.push(relative.to_owned());
        }
        paths
    } else {
        vec!["src/main.sarif".to_owned()]
    };

    let mut seen = HashSet::new();
    let mut resolved = Vec::with_capacity(relative_paths.len());
    for relative in relative_paths {
        if !seen.insert(relative.clone()) {
            return Err(format!(
                "manifest `{}` lists duplicate package source `{relative}`",
                path.display()
            ));
        }
        let source_path = root.join(&relative);
        if !source_path.is_file() {
            return Err(format!(
                "package `{}` is missing `{}`",
                path.display(),
                source_path.display()
            ));
        }
        resolved.push(normalize_lexical_path(&source_path).display().to_string());
    }

    Ok(resolved)
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::resolve_input;

    #[test]
    fn resolves_single_file_inputs_with_a_standalone_identity() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello.sarif");
        let resolved = resolve_input(source.to_str().expect("utf-8 path"))
            .expect("standalone source should load");
        assert_eq!(resolved.source_paths.len(), 1);
        assert!(resolved.source_paths[0].ends_with("examples/hello.sarif"));
        assert_eq!(resolved.package.name, "hello");
        assert_eq!(resolved.package.version, "0.0.0");
    }

    #[test]
    fn resolves_package_directories_and_manifests() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello-package");
        let directory = resolve_input(root.to_str().expect("utf-8 path"))
            .expect("package directory should resolve");
        let manifest = resolve_input(
            root.join("Sarif.toml")
                .to_str()
                .expect("utf-8 manifest path"),
        )
        .expect("package manifest should resolve");

        assert_eq!(directory.source_paths, manifest.source_paths);
        assert!(directory.source_paths[0].ends_with("examples/hello-package/src/main.sarif"));
        assert_eq!(directory.package.name, "hello-package");
        assert!(!directory.package.version.is_empty());
        assert_eq!(directory.package.name, manifest.package.name);
        assert_eq!(directory.package.version, manifest.package.version);
    }

    #[test]
    fn derives_stable_symbol_stems_for_package_identities() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello.sarif");
        let standalone = resolve_input(source.to_str().expect("utf-8 path"))
            .expect("standalone source should load");
        let package = resolve_input(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/hello-package")
                .to_str()
                .expect("utf-8 path"),
        )
        .expect("package directory should resolve");

        assert_eq!(standalone.package.symbol_stem(), "hello_0_0_0");
        assert_eq!(package.package.symbol_stem(), "hello_package_0_2_0");
    }

    #[test]
    fn resolves_manifest_ordered_multi_file_packages() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/multi-file-package");
        let resolved = resolve_input(root.to_str().expect("utf-8 path"))
            .expect("multi-file package should resolve");

        assert_eq!(resolved.source_paths.len(), 3);
        assert!(resolved.source_paths[0].ends_with("examples/multi-file-package/src/types.sarif"));
        assert!(resolved.source_paths[1].ends_with("examples/multi-file-package/src/consts.sarif"));
        assert!(
            resolved.source_paths[2].ends_with("examples/multi-file-package/src/functions.sarif")
        );
    }

    #[test]
    fn resolves_packages_that_share_stage0_sources_across_bootstrap_packages() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bootstrap/sarif_tools");
        let resolved = resolve_input(root.to_str().expect("utf-8 path"))
            .expect("bootstrap tools package should resolve");

        assert_eq!(resolved.source_paths.len(), 2);
        assert!(resolved.source_paths[0].contains("bootstrap/sarif_syntax/src/main.sarif"));
        assert!(resolved.source_paths[1].ends_with("bootstrap/sarif_tools/src/main.sarif"));
    }
}
