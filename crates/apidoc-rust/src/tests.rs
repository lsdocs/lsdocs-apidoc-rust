#![allow(clippy::unwrap_used, clippy::panic)]

//! These run rustdoc on `tests/fixture`, with the plugin's nightly
//! toolchain ([`crate::NIGHTLY`]), which must be installed.

use std::path::PathBuf;

use lsdocs_apidoc::{Item, ItemKind};
use lsdocs_apidoc_plugin::{Extracted, Language};
use lsdocs_plugin_sdk::doctree::{Node, plain_text};
use lsdocs_plugin_sdk::protocol::{InitializeParams, SiteInfo};
use lsdocs_plugin_sdk::testing::{context_with_parsers, run_transform_in};
use serde_json::{Value, json};

use super::{Rust, plugin};

/// The site directory of the tests: the plugin's directory, so the
/// fixture crate is `tests/fixture`.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A target directory for the fixture (kept between runs).
fn target_dir() -> String {
    std::env::temp_dir()
        .join("lsdocs-apidoc-rust-fixture")
        .to_string_lossy()
        .into_owned()
}

fn with_target(mut options: Value) -> Value {
    options["target_dir"] = json!(target_dir());
    options
}

fn extract(options: Value) -> Extracted {
    Rust.extract(&with_target(options), &root()).unwrap()
}

fn find<'a>(items: &'a [Item], path: &str) -> &'a Item {
    items
        .iter()
        .flat_map(Item::walk)
        .find(|i| i.qualified("::") == path)
        .unwrap_or_else(|| panic!("no {path}"))
}

fn sig(items: &[Item], path: &str) -> String {
    Rust.signature(find(items, path))
}

fn docs<'a>(items: &'a [Item], path: &str) -> &'a str {
    find(items, path).docs.as_deref().unwrap_or_default()
}

#[test]
fn extracts_a_crate() {
    let found = extract(json!({"crates": ["tests/fixture"]}));
    assert!(found.diagnostics.is_empty(), "{:?}", found.diagnostics);
    assert_eq!(
        found.files,
        [
            "tests/fixture/Cargo.toml",
            "tests/fixture/src/lib.rs",
            "tests/fixture/src/shapes.rs"
        ]
    );
    let items = &found.items;
    let root = &items[0];
    assert_eq!(root.qualified("::"), "geo");
    let names: Vec<&str> = root.members.iter().map(Item::name).collect();
    assert_eq!(
        names,
        [
            "shapes",
            "Revealed",
            "Circle",
            "HashMap",
            "Point",
            "Area",
            "Direction",
            "Bits",
            "Pair",
            "ANSWER",
            "COUNT",
            "total",
            "raw",
            "later",
            "old",
            "point",
            "Secret",
            "globbed"
        ]
    );
    // Intra-doc links: to items here, and to the standard library.
    assert_eq!(
        docs(items, "geo"),
        "Geometry, for testing.\n\nSee {rs:obj}`Point <geo::Point>`, {rs:obj}`the shapes <geo::shapes>` and [`Vec`](https://doc.rust-lang.org/stable/alloc/vec/struct.Vec.html)."
    );
    // Items of private modules are documented where re-exported, under
    // that name; re-exports of items documented elsewhere link to them.
    assert_eq!(sig(items, "geo::Revealed"), "pub struct Revealed(pub u8);");
    assert_eq!(sig(items, "geo::globbed"), "pub fn globbed()");
    let circle = find(items, "geo::Circle");
    assert_eq!(circle.kind, ItemKind::Other("re-export".into()));
    assert_eq!(
        circle.reexport_of.as_deref(),
        Some(&["geo".to_owned(), "shapes".to_owned(), "Circle".to_owned()][..])
    );
    assert_eq!(
        docs(items, "geo::Circle"),
        "Re-export of {rs:obj}`shapes::Circle <geo::shapes::Circle>`."
    );
    assert_eq!(
        sig(items, "geo::HashMap"),
        "pub use std::collections::HashMap;"
    );
    // Structs: fields, inherent items, the traits implemented.
    let point = find(items, "geo::Point");
    let members: Vec<String> = point.members.iter().map(|m| Rust.signature(m)).collect();
    assert_eq!(
        members,
        [
            "pub x: f64",
            "pub y: f64",
            "pub const ORIGIN: Point;",
            "pub const fn new(x: f64, y: f64) -> Self",
            "pub fn norm(&self) -> f64",
            "pub fn shift(&mut self, by: impl Into<Point>)",
        ]
    );
    assert_eq!(find(items, "geo::Point::new").kind, ItemKind::Function);
    assert_eq!(find(items, "geo::Point::norm").kind, ItemKind::Method);
    assert_eq!(
        point.bases,
        [
            "Debug",
            "Clone",
            "Copy",
            "StructuralPartialEq",
            "PartialEq",
            "Display"
        ]
    );
    // Hidden doctest lines go; code blocks say they are Rust.
    assert!(
        docs(items, "geo::Point").starts_with(
            "A point.\n\n# Examples\n\n```rust\nlet p = Point::new(1.0, 2.0);\nassert_eq!(p.x, 1.0);\n```\n\n# Trait implementations\n\n- [`Debug`](https://doc.rust-lang.org/stable/core/fmt/trait.Debug.html)\n"
        ),
        "{}",
        docs(items, "geo::Point")
    );
    assert_eq!(
        docs(items, "geo::Point::norm"),
        "How far from {rs:obj}`Point::ORIGIN <geo::Point::ORIGIN>`."
    );
    // Traits: their items, required or provided, and implementors.
    assert_eq!(sig(items, "geo::Area"), "pub trait Area: Debug");
    let area: Vec<String> = find(items, "geo::Area")
        .members
        .iter()
        .map(|m| Rust.signature(m))
        .collect();
    assert_eq!(
        area,
        [
            "type Unit;",
            "const SIDES: usize = 0;",
            "fn area(&self) -> f64;",
            "fn double(&self) -> f64 { ... }"
        ]
    );
    assert!(docs(items, "geo::Area").ends_with(
        "# Implementors\n\n- {rs:obj}`Circle <geo::shapes::Circle>`\n- {rs:obj}`Square <geo::shapes::Square>`"
    ));
    assert_eq!(
        sig(items, "geo::shapes::Square"),
        "pub struct Square(pub f64);"
    );
    // Enums and unions.
    let variants: Vec<String> = find(items, "geo::Direction")
        .members
        .iter()
        .map(|m| Rust.signature(m))
        .collect();
    assert_eq!(variants, ["North = 1", "East(u32)", "South { far: u32 }"]);
    assert_eq!(sig(items, "geo::Direction::South::far"), "far: u32");
    assert_eq!(sig(items, "geo::Bits"), "pub union Bits");
    // The rest.
    assert_eq!(sig(items, "geo::Pair"), "pub type Pair<T> = (T, T);");
    assert_eq!(sig(items, "geo::ANSWER"), "pub const ANSWER: u32 = 42;");
    assert_eq!(sig(items, "geo::COUNT"), "pub static mut COUNT: usize = 0;");
    assert_eq!(
        sig(items, "geo::total"),
        "pub fn total<'a, I, A>(items: I, scale: Option<&'a [f64]>) -> Result<f64, String>\nwhere\n    I: IntoIterator<Item = &'a A>,\n    A: Area + 'a,"
    );
    assert_eq!(
        sig(items, "geo::raw"),
        "pub unsafe extern \"C\" fn raw(ptr: *const u8, len: usize) -> i32"
    );
    assert_eq!(sig(items, "geo::later"), "pub async fn later()");
    assert_eq!(
        sig(items, "geo::point"),
        "macro_rules! point {\n    ($x:expr, $y:expr) => { ... };\n}"
    );
    assert_eq!(
        docs(items, "geo::old"),
        "Old.\n\n# Deprecated\n\nSince 0.2.0: use `total`"
    );
    let location = find(items, "geo::shapes::Circle").location.clone().unwrap();
    assert_eq!(location.file, "tests/fixture/src/shapes.rs");
    assert_eq!(location.line, Some(7));
}

#[test]
fn reads_json_files_and_checks_their_format() {
    // The JSON the other tests had rustdoc write (written here if this
    // test runs first).
    let json = PathBuf::from(target_dir()).join("doc/geo.json");
    if !json.exists() {
        extract(json!({"crates": ["tests/fixture"]}));
    }
    let found = extract(json!({"crates": [json.to_string_lossy()]}));
    assert!(found.diagnostics.is_empty(), "{:?}", found.diagnostics);
    assert_eq!(found.files, [json.to_string_lossy().into_owned()]);
    assert_eq!(found.items[0].qualified("::"), "geo");

    let old = std::env::temp_dir().join(format!("lsdocs-rustdoc-old-{}.json", std::process::id()));
    std::fs::write(&old, r#"{"format_version": 1}"#).unwrap();
    let found = extract(json!({"crates": [old.to_string_lossy()]}));
    let _ = std::fs::remove_file(&old);
    assert_eq!(found.diagnostics.len(), 1);
    assert!(
        found.diagnostics[0]
            .message
            .contains("is in rustdoc's JSON format 1; this plugin reads format"),
        "{:?}",
        found.diagnostics
    );

    // A workspace without a package named, and missing options.
    let found = extract(json!({"crates": [{"path": "tests/fixture", "package": "nope"}]}));
    assert!(
        found.diagnostics[0]
            .message
            .starts_with("no package `nope`")
    );
    assert!(Rust.extract(&json!({}), &root()).is_err());
}

#[tokio::test]
async fn documents_a_crate_with_references() {
    let page = "```{rs:currentmodule} geo\n```\n\nSee {rs:struct}`Point`, {rs:fn}`~geo::Point::norm` and {rs:trait}`shapes::Circle`.\n\n```{rs:auto} total\n```\n";
    let plugin = plugin();
    let init = lsdocs_plugin_sdk::Setup::with_settings(
        InitializeParams {
            site: SiteInfo {
                root: Some(root().to_string_lossy().into_owned()),
                ..SiteInfo::default()
            },
            ..InitializeParams::default()
        },
        with_target(json!({"crates": ["tests/fixture"]})),
    );
    let declared = plugin.capabilities_for(&init).syntax;
    let (guide, diagnostics) =
        lsdocs_parser_markdown::parse_with_syntax("guide.md", page, &declared);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let ctx = context_with_parsers(
        &plugin,
        &init,
        vec![
            lsdocs_parser_markdown::plugin(),
            lsdocs_parser_rst::plugin(),
        ],
    );
    let out = run_transform_in(&plugin, &ctx, vec![guide]).await.unwrap();
    for (doc, diagnostics) in &out {
        assert!(diagnostics.is_empty(), "{}: {diagnostics:?}", doc.source);
    }
    let sources: Vec<&str> = out.iter().map(|(d, _)| d.source.as_str()).collect();
    assert_eq!(
        sources,
        [
            "guide.md",
            "api/rs/index.md",
            "api/rs/geo.md",
            "api/rs/geo.shapes.md"
        ]
    );
    let links = |nodes: &[Node]| -> Vec<String> {
        nodes
            .iter()
            .flat_map(Node::descendants)
            .filter_map(|n| match n {
                Node::Link(l) => Some(format!("{} -> {}", plain_text(&l.children), l.url)),
                _ => None,
            })
            .collect()
    };
    let (guide, _) = &out[0];
    // `rs:trait` does not take a struct: shown, not linked.
    assert_eq!(
        links(&guide.children),
        [
            "Point -> api/rs/geo.md#rs.geo.Point",
            "norm() -> api/rs/geo.md#rs.geo.Point.norm"
        ]
    );
    assert!(guide.descendants().any(|n| matches!(
        n,
        Node::CodeBlock(c) if c.value.starts_with("pub fn total<'a, I, A>")
    )));
    let (geo, _) = &out[2];
    let all = links(&geo.children);
    for expected in [
        "Point -> #rs.geo.Point",
        "the shapes -> geo.shapes.md#rs.geo.shapes",
        "Vec -> https://doc.rust-lang.org/stable/alloc/vec/struct.Vec.html",
        "Point::ORIGIN -> #rs.geo.Point.ORIGIN",
        "Circle -> geo.shapes.md#rs.geo.shapes.Circle",
    ] {
        assert!(all.contains(&expected.to_owned()), "no {expected}: {all:?}");
    }
    // rustdoc's sections are parts of the item, not sections of the page.
    let sections: Vec<String> = geo
        .descendants()
        .filter_map(|n| match n {
            Node::Section(s) => s.id.clone(),
            _ => None,
        })
        .collect();
    assert!(
        !sections.iter().any(|s| !s.starts_with("rs.")),
        "{sections:?}"
    );
    let text = plain_text(&geo.children);
    for expected in [
        "Example",
        "Panics",
        "Never.",
        "Safety",
        "Trait implementations",
    ] {
        assert!(text.contains(expected), "no {expected:?} in {text}");
    }
    assert!(geo.descendants().any(|n| matches!(
        n,
        Node::CodeBlock(c) if c.language.as_deref() == Some("rust")
            && c.value.trim() == "let p = Point::new(1.0, 2.0);\nassert_eq!(p.x, 1.0);"
    )));
    assert!(
        geo.descendants()
            .any(|n| matches!(n, Node::Admonition(a) if a.kind == "deprecated"))
    );
    // Parameters from `# Arguments`.
    let new = find_section(geo, "rs.geo.Point.new");
    assert!(plain_text(std::slice::from_ref(new)).contains("Parameters"));
}

fn find_section<'a>(doc: &'a lsdocs_plugin_sdk::doctree::Document, id: &str) -> &'a Node {
    doc.descendants()
        .find(|n| matches!(n, Node::Section(s) if s.id.as_deref() == Some(id)))
        .unwrap_or_else(|| panic!("no section {id}"))
}

#[test]
fn the_nightly_is_named_the_same_everywhere() {
    // README.md is the one copy of the name (nur installs the nightly from
    // NIGHTLY itself).
    let text = std::fs::read_to_string(root().join("../../README.md")).unwrap();
    assert!(
        text.contains(crate::NIGHTLY),
        "README.md should name {}",
        crate::NIGHTLY
    );
}
