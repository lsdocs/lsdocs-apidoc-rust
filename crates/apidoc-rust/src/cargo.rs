//! Running rustdoc (through cargo) for a crate's JSON, and reading it.

use std::path::{Path, PathBuf};
use std::process::Command;

use rustdoc_types::{Crate, FORMAT_VERSION};
use serde::Deserialize;
use serde_json::Value;

/// One entry of the `crates` option.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Entry {
    /// A crate's directory (or its `Cargo.toml`), or a JSON file rustdoc
    /// wrote.
    Path(String),
    /// A crate, with what to build it with.
    Crate(CrateEntry),
}

/// A crate, with what to build it with.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CrateEntry {
    /// Its directory (or `Cargo.toml`), or a JSON file.
    pub path: String,
    /// The package, in a workspace (`-p`).
    #[serde(default)]
    pub package: Option<String>,
    /// Features to enable.
    #[serde(default)]
    pub features: Vec<String>,
    /// `--all-features`.
    #[serde(default)]
    pub all_features: bool,
    /// `--no-default-features`.
    #[serde(default)]
    pub no_default_features: bool,
}

impl Entry {
    pub(crate) fn crate_entry(&self) -> CrateEntry {
        match self {
            Entry::Path(path) => CrateEntry {
                path: path.clone(),
                package: None,
                features: Vec::new(),
                all_features: false,
                no_default_features: false,
            },
            Entry::Crate(c) => c.clone(),
        }
    }
}

/// How cargo is run.
pub(crate) struct Settings {
    /// The toolchain (`cargo +<toolchain>`); empty for cargo as found.
    pub toolchain: String,
    /// The target directory, if not cargo's own.
    pub target_dir: Option<PathBuf>,
    /// Document private items.
    pub private: bool,
}

/// A crate's rustdoc JSON, and where its files are.
pub(crate) struct Documented {
    pub krate: Crate,
    /// The directory rustdoc's file names are relative to (the
    /// workspace's), relative to the site directory (`/`-separated);
    /// unknown for a JSON file.
    pub base: Option<String>,
    /// Files the result depends on besides the sources (the manifest, or
    /// the JSON file), relative to the site directory.
    pub files: Vec<String>,
}

/// The crate at `entry` (relative to `root`, the site directory),
/// documented by rustdoc.
pub(crate) fn document(
    entry: &CrateEntry,
    root: &Path,
    settings: &Settings,
) -> Result<Documented, String> {
    let full = root.join(&entry.path);
    if entry.path.ends_with(".json") {
        let krate = read(&full)?;
        return Ok(Documented {
            krate,
            base: None,
            files: vec![entry.path.clone()],
        });
    }
    let (manifest, manifest_rel) = if entry.path.ends_with("Cargo.toml") {
        (full.clone(), entry.path.clone())
    } else {
        (full.join("Cargo.toml"), join(&entry.path, "Cargo.toml"))
    };
    let metadata = metadata(&manifest)?;
    let package = package(&metadata, &manifest, entry.package.as_deref())?;
    let lib = package
        .targets
        .iter()
        .find(|t| {
            t.kind
                .iter()
                .any(|k| matches!(k.as_str(), "lib" | "rlib" | "dylib" | "proc-macro"))
        })
        .ok_or_else(|| format!("package `{}` has no library to document", package.name))?;
    let mut cmd = Command::new("cargo");
    if !settings.toolchain.is_empty() {
        cmd.arg(format!("+{}", settings.toolchain));
    }
    cmd.arg("rustdoc")
        .arg("--manifest-path")
        .arg(&manifest)
        .args(["-p", &package.name, "--lib", "--quiet"]);
    if !entry.features.is_empty() {
        cmd.arg("--features").arg(entry.features.join(","));
    }
    if entry.all_features {
        cmd.arg("--all-features");
    }
    if entry.no_default_features {
        cmd.arg("--no-default-features");
    }
    let target_dir = match &settings.target_dir {
        Some(dir) => {
            cmd.arg("--target-dir").arg(dir);
            dir.clone()
        }
        None => PathBuf::from(&metadata.target_directory),
    };
    cmd.args(["--", "-Z", "unstable-options", "--output-format", "json"]);
    if settings.private {
        cmd.arg("--document-private-items");
    }
    // What a parent cargo or rustup set (when lsdocs runs under them)
    // must not choose the toolchain.
    cmd.env_remove("RUSTUP_TOOLCHAIN");
    let output = cmd.output().map_err(|e| format!("cannot run cargo: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(20).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        let mut message = format!(
            "`cargo rustdoc` failed for `{}`:\n{}",
            package.name,
            tail.join("\n")
        );
        if stderr.contains("is not installed") || stderr.contains("toolchain") {
            message.push_str(&format!(
                "\nrustdoc's JSON needs a nightly toolchain: `rustup toolchain install {}`",
                if settings.toolchain.is_empty() {
                    "nightly"
                } else {
                    &settings.toolchain
                }
            ));
        }
        return Err(message);
    }
    let json = target_dir
        .join("doc")
        .join(format!("{}.json", lib.name.replace('-', "_")));
    let krate = read(&json)?;
    // rustdoc names files relative to the workspace root; from the site,
    // that is the crate's directory, up as many levels as the crate is
    // below the workspace root.
    let crate_dir = parent(&manifest_rel);
    let depth = Path::new(&package.manifest_path)
        .parent()
        .and_then(|dir| dir.strip_prefix(&metadata.workspace_root).ok())
        .map_or(0, |rel| rel.components().count());
    let mut base = crate_dir;
    for _ in 0..depth {
        base = join(&base, "..");
    }
    Ok(Documented {
        krate,
        base: Some(normalize(&base)),
        files: vec![manifest_rel],
    })
}

/// Reads rustdoc's JSON, checking its format is the one this plugin
/// reads.
fn read(path: &Path) -> Result<Crate, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    #[derive(Deserialize)]
    struct Version {
        format_version: u32,
    }
    let version: Version = serde_json::from_str(&text)
        .map_err(|e| format!("{} is not rustdoc's JSON: {e}", path.display()))?;
    if version.format_version != FORMAT_VERSION {
        return Err(format!(
            "{} is in rustdoc's JSON format {}; this plugin reads format {FORMAT_VERSION}. \
             Set the `toolchain` option to a nightly that writes it (the default, {}, does)",
            path.display(),
            version.format_version,
            crate::NIGHTLY
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_root: String,
    target_directory: String,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    manifest_path: String,
    targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
}

fn metadata(manifest: &Path) -> Result<Metadata, String> {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(manifest)
        .env_remove("RUSTUP_TOOLCHAIN")
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "`cargo metadata` failed for {}: {}",
            manifest.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("cannot read cargo's metadata: {e}"))?;
    serde_json::from_value(value).map_err(|e| format!("cannot read cargo's metadata: {e}"))
}

/// The package to document: the one named, or the manifest's own.
fn package<'a>(
    metadata: &'a Metadata,
    manifest: &Path,
    name: Option<&str>,
) -> Result<&'a Package, String> {
    if let Some(name) = name {
        return metadata
            .packages
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| format!("no package `{name}` in {}", manifest.display()));
    }
    let wanted = std::fs::canonicalize(manifest).ok();
    metadata
        .packages
        .iter()
        .find(|p| std::fs::canonicalize(&p.manifest_path).ok() == wanted && wanted.is_some())
        .ok_or_else(|| {
            format!(
                "{} is a workspace, not a package: name one with `package`",
                manifest.display()
            )
        })
}

/// `a/b` joined to `c`, `/`-separated.
pub(crate) fn join(a: &str, b: &str) -> String {
    let a = a.trim_end_matches(['/', '\\']);
    if a.is_empty() || a == "." {
        b.to_owned()
    } else {
        format!("{a}/{b}")
    }
}

/// The directory of a `/`-separated path (`.` for a bare name).
fn parent(path: &str) -> String {
    let path = path.replace('\\', "/");
    match path.rsplit_once('/') {
        Some((dir, _)) => dir.to_owned(),
        None => ".".to_owned(),
    }
}

/// A `/`-separated path without `.` and with `..` taken where it can be.
pub(crate) fn normalize(path: &str) -> String {
    let path = path.replace('\\', "/");
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            ".." if absolute => {}
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    match (absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_owned(),
        (false, false) => joined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(normalize("../crates/sdk/../.."), "..");
        assert_eq!(normalize("./a/./b/../c"), "a/c");
        assert_eq!(normalize("a/.."), ".");
        assert_eq!(normalize("C:/x/../y"), "C:/y");
        assert_eq!(join(".", "src/lib.rs"), "src/lib.rs");
        assert_eq!(parent("a/b/Cargo.toml"), "a/b");
        assert_eq!(parent("Cargo.toml"), ".");
    }
}
