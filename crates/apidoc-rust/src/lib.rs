//! First-party plugin documenting Rust crates, from rustdoc's JSON output.
//!
//! For each crate it runs `cargo +<toolchain> rustdoc --lib -- -Z
//! unstable-options --output-format json` (a nightly toolchain: the JSON
//! output is unstable) and reads what rustdoc wrote, with
//! [`rustdoc_types`] at its format version ([`rustdoc_types::FORMAT_VERSION`]).
//! Types, paths and re-exports are rustdoc's, so they are exact; building
//! the crate's documentation runs its build scripts and procedural macros,
//! as `cargo doc` does. Documents the crates with the API framework
//! (`lsdocs-apidoc-plugin`): a page per module under `api/rs`, the `rs`
//! domain's directives and roles, references between them.
//!
//! ```toml
//! [[plugins]]
//! name = "rust"
//! builtin = "apidoc-rust"
//! ```
//!
//! with its settings in `.config/lsdocs/rust/settings.toml`:
//!
//! ```toml
//! crates = ["../crates/my-crate", { path = "..", package = "other", features = ["x"] }]
//! ```
//!
//! - **Crates** (relative to the site directory): a crate's directory or
//!   `Cargo.toml`, or a JSON file rustdoc wrote; `{ path, package,
//!   features, all_features, no_default_features }` picks a package of a
//!   workspace and its features.
//! - **What is documented:** what rustdoc documents: the public items of
//!   public modules, and items re-exported from private ones (where they
//!   are re-exported, under that name). Re-exports of items documented
//!   elsewhere link to them. Structs, enums and unions list their fields
//!   or variants and their inherent methods, and the traits they
//!   implement; traits their items and implementors.
//! - **Doc comments** are Markdown, with rustdoc's sections (`# Examples`,
//!   `# Panics`, `# Errors`, `# Safety`) and intra-doc links: a link to an
//!   item documented by the plugin (in any of its crates) becomes a
//!   reference to it; one to another crate's item, a link to its rustdoc
//!   pages (the standard library's, or docs.rs). Code blocks are Rust
//!   unless they say otherwise, without their hidden (`# `) lines.
//! - **The `rs` domain:** `rs:function`, `rs:method`, `rs:struct`,
//!   `rs:enum`, `rs:union`, `rs:trait`, `rs:type`, `rs:const`,
//!   `rs:static`, `rs:macro`, `rs:field`, `rs:variant`, `rs:module`,
//!   `rs:currentmodule`, `rs:auto`; roles `rs:fn`, `rs:method`,
//!   `rs:struct`, `rs:enum`, `rs:union`, `rs:trait`, `rs:type`,
//!   `rs:const`, `rs:static`, `rs:macro`, `rs:field`, `rs:variant`,
//!   `rs:mod`, `rs:obj`.
//!
//! Settings: `crates` (required for generated pages), `toolchain` (default
//! [`NIGHTLY`], one that writes the format this plugin reads; `""` runs
//! cargo as found), `target_dir` (cargo's own by default), and those of
//! the framework (`output`, `private`, `title`, `doc_format`, `doc_style`,
//! `generate`, `primary`).

mod cargo;
mod docs;
mod extract;
#[cfg(test)]
mod tests;
mod types;

use std::path::Path;

use lsdocs_apidoc::{DocStyle, Item, ItemKind};
use lsdocs_apidoc_plugin::{Extracted, Language};
use lsdocs_plugin_sdk::Plugin;
use lsdocs_plugin_sdk::protocol::{Diagnostic, Severity};
use serde_json::Value;

/// The nightly toolchain run by default: one whose rustdoc writes the
/// JSON format this plugin reads.
pub const NIGHTLY: &str = "nightly-2026-11-14";

/// The plugin, as registered with the host.
pub fn plugin() -> Plugin {
    lsdocs_apidoc_plugin::plugin(Rust, env!("CARGO_PKG_VERSION"))
}

/// Rust.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rust;

impl Language for Rust {
    fn domain(&self) -> &str {
        "rs"
    }

    fn name(&self) -> &str {
        "Rust"
    }

    fn code_language(&self) -> &str {
        "rust"
    }

    fn separator(&self) -> &str {
        "::"
    }

    fn settings(&self) -> &[&str] {
        &["crates", "toolchain", "target_dir"]
    }

    fn doc_format(&self) -> &str {
        "md"
    }

    fn doc_style(&self) -> DocStyle {
        DocStyle::Headings
    }

    fn extract(&self, options: &Value, root: &Path) -> Result<Extracted, String> {
        let entries: Vec<cargo::Entry> = match options.get("crates") {
            Some(crates) => serde_json::from_value(crates.clone()).map_err(|e| {
                format!(
                    "`crates` must be a list of paths or {{ path, package, features }} tables: {e}"
                )
            })?,
            None => return Err("the `crates` option is missing".into()),
        };
        let settings = cargo::Settings {
            toolchain: match options.get("toolchain") {
                Some(Value::String(t)) => t.clone(),
                Some(_) => return Err("`toolchain` must be a string".into()),
                None => NIGHTLY.to_owned(),
            },
            target_dir: match options.get("target_dir") {
                Some(Value::String(dir)) => Some(root.join(dir)),
                Some(_) => return Err("`target_dir` must be a string".into()),
                None => None,
            },
            private: options.get("private").and_then(Value::as_bool) == Some(true),
        };
        let mut files: Vec<String> = Vec::new();
        let mut diagnostics = Vec::new();
        let mut crates = Vec::new();
        for entry in &entries {
            let entry = entry.crate_entry();
            let documented = match cargo::document(&entry, root, &settings) {
                Ok(d) => d,
                Err(message) => {
                    diagnostics.push(problem(message));
                    continue;
                }
            };
            files.extend(documented.files);
            let Some(mut extracted) = extract::extract(&documented.krate) else {
                diagnostics.push(problem(format!(
                    "{}: rustdoc's JSON has no root module",
                    entry.path
                )));
                continue;
            };
            if let Some(base) = &documented.base {
                // File names from the site directory.
                for file in &extracted.files {
                    files.push(cargo::normalize(&cargo::join(base, file)));
                }
                relocate(&mut extracted.root, base);
            }
            crates.push((documented.krate, extracted));
        }
        files.sort();
        files.dedup();
        Ok(Extracted {
            items: extract::finish(crates),
            files,
            diagnostics,
        })
    }

    fn objects(&self) -> Vec<(String, ItemKind)> {
        [
            ("function", ItemKind::Function),
            ("method", ItemKind::Method),
            ("struct", ItemKind::Struct),
            ("enum", ItemKind::Enum),
            ("union", ItemKind::Union),
            ("trait", ItemKind::Trait),
            ("type", ItemKind::TypeAlias),
            ("const", ItemKind::Constant),
            ("static", ItemKind::Variable),
            ("macro", ItemKind::Macro),
            ("field", ItemKind::Field),
            ("variant", ItemKind::Variant),
        ]
        .into_iter()
        .map(|(n, k)| (n.to_owned(), k))
        .collect()
    }

    fn roles(&self) -> Vec<(String, Vec<ItemKind>)> {
        use ItemKind::*;
        [
            ("fn", vec![Function, Method]),
            ("method", vec![Method, Function]),
            ("struct", vec![Struct]),
            ("enum", vec![Enum]),
            ("union", vec![Union]),
            ("trait", vec![Trait]),
            ("type", vec![TypeAlias, Struct, Enum, Union, Trait]),
            ("const", vec![Constant]),
            ("static", vec![Variable]),
            ("macro", vec![Macro]),
            ("field", vec![Field]),
            ("variant", vec![Variant]),
            ("mod", vec![Module]),
            ("obj", vec![]),
        ]
        .into_iter()
        .map(|(n, k)| (n.to_owned(), k))
        .collect()
    }
}

fn problem(message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        message,
        code: Some("apidoc-rust".into()),
        ..Diagnostic::default()
    }
}

/// Makes items' file names relative to the site directory.
fn relocate(item: &mut Item, base: &str) {
    if let Some(location) = &mut item.location {
        location.file = cargo::normalize(&cargo::join(base, &location.file));
    }
    for member in &mut item.members {
        relocate(member, base);
    }
}
