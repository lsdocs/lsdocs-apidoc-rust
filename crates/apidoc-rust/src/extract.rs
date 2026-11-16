//! rustdoc's JSON (a [`Crate`]) to API items.
//!
//! What a crate's public modules hold is documented where it is; an item
//! re-exported from a private module (`pub use inner::Thing`, or a glob)
//! is documented where it is re-exported, under the name it has there, as
//! rustdoc's pages do. Re-exports of items documented elsewhere (in the
//! crate, or another crate) are items of their own that link to them.

use std::collections::{BTreeMap, HashMap, HashSet};

use lsdocs_apidoc::{Item, ItemKind, Location, Param, ParamKind, Signature, Visibility};
use rustdoc_types::{
    Crate, Id, Impl, ItemEnum, ItemKind as RdKind, StructKind, Type, VariantKind,
    Visibility as RdVisibility,
};

use crate::docs::{self, Target};
use crate::types;

/// Values longer than this are cut (`...`).
const MAX_VALUE: usize = 80;

/// Signatures longer than this put each parameter on its own line.
const MAX_LINE: usize = 100;

/// One crate's items, before its doc comments are rewritten.
pub(crate) struct Extracted {
    /// The crate's root module, everything inside.
    pub root: Item,
    /// The files its items are in, as rustdoc names them.
    pub files: Vec<String>,
    /// Where each item was documented, by rustdoc id.
    documented: HashMap<Id, Vec<String>>,
    /// The rustdoc id of each documented item, by path.
    ids: HashMap<Vec<String>, Id>,
    /// Text added to items' doc comments (trait implementations,
    /// implementors, re-exports, deprecation), by path.
    added: HashMap<Vec<String>, String>,
}

/// The crate's items.
pub(crate) fn extract(krate: &Crate) -> Option<Extracted> {
    let mut walker = Walker {
        krate,
        reachable: HashSet::new(),
        documented: HashMap::new(),
        ids: HashMap::new(),
        added: HashMap::new(),
        files: BTreeMap::new(),
        inlining: Vec::new(),
    };
    walker.reach(&krate.root);
    let root = walker.item(&krate.root, &[], None)?;
    Some(Extracted {
        root,
        files: walker.files.into_keys().collect(),
        documented: walker.documented,
        ids: walker.ids,
        added: walker.added,
    })
}

/// Rewrites the doc comments of every crate's items, now that it is known
/// where each item is documented: intra-doc links become references to
/// items documented in any of the crates, or links to rustdoc pages.
pub(crate) fn finish(crates: Vec<(Crate, Extracted)>) -> Vec<Item> {
    let names: HashSet<&str> = crates
        .iter()
        .filter_map(|(_, e)| e.root.path.first().map(String::as_str))
        .collect();
    let mut out = Vec::new();
    for (krate, extracted) in &crates {
        let mut root = extracted.root.clone();
        rewrite(&mut root, krate, extracted, &names);
        out.push(root);
    }
    out
}

fn rewrite(item: &mut Item, krate: &Crate, extracted: &Extracted, names: &HashSet<&str>) {
    let links = extracted
        .ids
        .get(&item.path)
        .and_then(|id| krate.index.get(id))
        .map(|i| &i.links);
    let resolve = |dest: &str| -> Option<Target> {
        let id = links?.get(dest)?;
        Some(target(krate, extracted, names, id))
    };
    let mut text = item
        .docs
        .as_deref()
        .map(|d| docs::clean(d, &resolve))
        .unwrap_or_default();
    if let Some(added) = extracted.added.get(&item.path) {
        let added = docs::clean(added, &|dest| {
            Some(match dest.parse::<u32>() {
                Ok(n) => target(krate, extracted, names, &Id(n)),
                Err(_) => Target::Text,
            })
        });
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&added);
    }
    item.docs = (!text.trim().is_empty()).then_some(text);
    for member in &mut item.members {
        rewrite(member, krate, extracted, names);
    }
}

/// What rustdoc's item `id` is to a link.
fn target(krate: &Crate, extracted: &Extracted, names: &HashSet<&str>, id: &Id) -> Target {
    if let Some(path) = extracted.documented.get(id) {
        return Target::Item(path.clone());
    }
    let Some(summary) = krate.paths.get(id) else {
        return Target::Text;
    };
    if summary.crate_id == 0 {
        return Target::Text;
    }
    let Some(external) = krate.external_crates.get(&summary.crate_id) else {
        return Target::Text;
    };
    if names.contains(external.name.as_str()) {
        return Target::Item(summary.path.clone());
    }
    match url(external, &summary.path, &summary.kind) {
        Some(url) => Target::Url(url),
        None => Target::Text,
    }
}

/// The address of another crate's item on its rustdoc pages: its own
/// `html_root_url` (the standard library's stable docs for its crates), or
/// docs.rs.
fn url(external: &rustdoc_types::ExternalCrate, path: &[String], kind: &RdKind) -> Option<String> {
    let mut root = match &external.html_root_url {
        Some(url) => url.replace("doc.rust-lang.org/nightly/", "doc.rust-lang.org/stable/"),
        None => format!("https://docs.rs/{}/latest/", external.name),
    };
    if !root.ends_with('/') {
        root.push('/');
    }
    let (name, dirs) = path.split_last()?;
    let dirs = dirs.join("/");
    let prefix = match kind {
        RdKind::Module => return Some(format!("{root}{}/index.html", path.join("/"))),
        RdKind::Variant | RdKind::StructField | RdKind::AssocConst | RdKind::AssocType => {
            let (parent, outer) = dirs.rsplit_once('/')?;
            let fragment = match kind {
                RdKind::Variant => "variant",
                RdKind::StructField => "structfield",
                RdKind::AssocConst => "associatedconstant",
                _ => "associatedtype",
            };
            // The parent's kind is not known; enums have variants.
            let parent_kind = if *kind == RdKind::Variant {
                "enum"
            } else {
                "struct"
            };
            return Some(format!(
                "{root}{parent}/{parent_kind}.{outer}.html#{fragment}.{name}"
            ));
        }
        RdKind::Struct => "struct",
        RdKind::Enum => "enum",
        RdKind::Union => "union",
        RdKind::Trait => "trait",
        RdKind::TraitAlias => "traitalias",
        RdKind::Function => "fn",
        RdKind::TypeAlias => "type",
        RdKind::Constant => "constant",
        RdKind::Static => "static",
        RdKind::Macro => "macro",
        RdKind::ProcAttribute => "attr",
        RdKind::ProcDerive => "derive",
        RdKind::Primitive => "primitive",
        RdKind::Keyword => "keyword",
        RdKind::ExternType => "foreigntype",
        _ => return None,
    };
    Some(format!("{root}{dirs}/{prefix}.{name}.html"))
}

struct Walker<'a> {
    krate: &'a Crate,
    /// Items public modules hold directly (not through a re-export).
    reachable: HashSet<Id>,
    documented: HashMap<Id, Vec<String>>,
    ids: HashMap<Vec<String>, Id>,
    added: HashMap<Vec<String>, String>,
    files: BTreeMap<String, ()>,
    /// Modules being inlined (glob re-exports), against cycles.
    inlining: Vec<Id>,
}

impl Walker<'_> {
    /// Notes what public modules hold directly, from `module` down.
    fn reach(&mut self, module: &Id) {
        let Some(ItemEnum::Module(m)) = self.krate.index.get(module).map(|i| &i.inner) else {
            return;
        };
        for id in &m.items {
            let Some(item) = self.krate.index.get(id) else {
                continue;
            };
            match &item.inner {
                ItemEnum::Use(_) => {}
                ItemEnum::Module(inner) if !inner.is_stripped => {
                    self.reachable.insert(*id);
                    self.reach(id);
                }
                ItemEnum::Module(_) => {}
                _ => {
                    self.reachable.insert(*id);
                }
            }
        }
    }

    /// The items a module holds, re-exports resolved.
    fn module_items(&mut self, ids: &[Id], path: &[String]) -> Vec<Item> {
        let mut out = Vec::new();
        for id in ids {
            let Some(item) = self.krate.index.get(id) else {
                continue;
            };
            match &item.inner {
                ItemEnum::Use(u) => out.extend(self.reexport(id, u, path)),
                ItemEnum::Module(m) if m.is_stripped => {}
                ItemEnum::Impl(_) | ItemEnum::ExternCrate { .. } | ItemEnum::Primitive(_) => {}
                _ => out.extend(self.item(id, path, None)),
            }
        }
        out
    }

    /// A `pub use`: the item itself if it is not documented anywhere else
    /// (or, for a glob, the items of the module), else an item that links
    /// to it.
    fn reexport(&mut self, id: &Id, u: &rustdoc_types::Use, path: &[String]) -> Vec<Item> {
        let target = u.id.and_then(|t| self.krate.index.get(&t).map(|_| t));
        if u.is_glob {
            let Some(target) = target else {
                return Vec::new();
            };
            if self.reachable.contains(&target) || self.inlining.contains(&target) {
                return Vec::new();
            }
            let Some(ItemEnum::Module(m)) = self.krate.index.get(&target).map(|i| &i.inner) else {
                return Vec::new();
            };
            self.inlining.push(target);
            let items = self.module_items(&m.items.clone(), path);
            self.inlining.pop();
            return items;
        }
        if let Some(target) = target
            && !self.reachable.contains(&target)
            && self.krate.index.get(&target).map(|i| i.crate_id) == Some(0)
        {
            return self
                .item(&target, path, Some(&u.name))
                .into_iter()
                .collect();
        }
        // A re-export of something documented elsewhere.
        let mut full = path.to_vec();
        full.push(u.name.clone());
        let mut item = Item::new(ItemKind::Other("re-export".into()), full.clone());
        let last = u.source.rsplit("::").next().unwrap_or(&u.source);
        let text = if last == u.name {
            format!("pub use {};", u.source)
        } else {
            format!("pub use {} as {};", u.source, u.name)
        };
        item.signature = Some(Signature {
            text: Some(text),
            ..Signature::default()
        });
        if let Some(own) = self.krate.index.get(id) {
            item.docs.clone_from(&own.docs);
            item.location = self.location(own);
        }
        if let Some(target) = u.id {
            item.reexport_of = self
                .documented
                .get(&target)
                .cloned()
                .or_else(|| self.krate.paths.get(&target).map(|s| s.path.clone()));
            self.added.insert(
                full.clone(),
                format!("Re-export of [`{}`]({}).", u.source, target.0),
            );
        }
        self.ids.insert(full, *id);
        vec![item]
    }

    fn location(&mut self, item: &rustdoc_types::Item) -> Option<Location> {
        if item.crate_id != 0 {
            return None;
        }
        let span = item.span.as_ref()?;
        let file = span.filename.to_string_lossy().replace('\\', "/");
        self.files.insert(file.clone(), ());
        Some(Location {
            file,
            line: u32::try_from(span.begin.0).ok(),
        })
    }

    /// The item `id`, at `path` (its parent's), named `name` or its own.
    fn item(&mut self, id: &Id, path: &[String], name: Option<&str>) -> Option<Item> {
        let krate = self.krate;
        let source = krate.index.get(id)?;
        let name = name.or(source.name.as_deref())?.to_owned();
        let mut full = path.to_vec();
        full.push(name.clone());
        let vis = visibility(&source.visibility);
        let pub_ = match &source.visibility {
            RdVisibility::Public => "pub ".to_owned(),
            RdVisibility::Default => String::new(),
            RdVisibility::Crate => "pub(crate) ".to_owned(),
            RdVisibility::Restricted { path, .. } => format!("pub(in {path}) "),
        };
        let mut item = match &source.inner {
            ItemEnum::Module(m) => {
                let mut item = Item::new(ItemKind::Module, full.clone());
                item.signature = Some(text(if m.is_crate {
                    format!("crate {name}")
                } else {
                    format!("{pub_}mod {name}")
                }));
                self.documented.insert(*id, full.clone());
                item.members = self.module_items(&m.items, &full);
                item
            }
            ItemEnum::Struct(s) => {
                let mut item = Item::new(ItemKind::Struct, full.clone());
                let generics = types::params(&s.generics);
                let where_ = types::where_clause(&s.generics);
                let text = match &s.kind {
                    StructKind::Unit => format!("{pub_}struct {name}{generics}{where_};"),
                    StructKind::Tuple(fields) => format!(
                        "{pub_}struct {name}{generics}({}){where_};",
                        self.tuple_fields(fields)
                    ),
                    StructKind::Plain {
                        has_stripped_fields,
                        fields,
                    } => {
                        let body = if *has_stripped_fields && fields.is_empty() {
                            " { /* private fields */ }"
                        } else {
                            ""
                        };
                        format!("{pub_}struct {name}{generics}{where_}{body}")
                    }
                };
                item.signature = Some(generic(text, &s.generics));
                self.documented.insert(*id, full.clone());
                match &s.kind {
                    StructKind::Plain { fields, .. } => {
                        item.members = fields
                            .iter()
                            .filter_map(|f| self.item(f, &full, None))
                            .collect();
                    }
                    StructKind::Tuple(fields) => {
                        item.members = self.documented_tuple_fields(fields, &full);
                    }
                    StructKind::Unit => {}
                }
                self.impls(&mut item, &s.impls);
                item
            }
            ItemEnum::Union(u) => {
                let mut item = Item::new(ItemKind::Union, full.clone());
                item.signature = Some(generic(
                    format!(
                        "{pub_}union {name}{}{}",
                        types::params(&u.generics),
                        types::where_clause(&u.generics)
                    ),
                    &u.generics,
                ));
                self.documented.insert(*id, full.clone());
                item.members = u
                    .fields
                    .iter()
                    .filter_map(|f| self.item(f, &full, None))
                    .collect();
                self.impls(&mut item, &u.impls);
                item
            }
            ItemEnum::StructField(t) => {
                let mut item = Item::new(ItemKind::Field, full.clone());
                item.ty = Some(types::ty(t));
                item.signature = Some(text(format!("{pub_}{name}: {}", types::ty(t))));
                item
            }
            ItemEnum::Enum(e) => {
                let mut item = Item::new(ItemKind::Enum, full.clone());
                let body = if e.has_stripped_variants && e.variants.is_empty() {
                    " { /* private variants */ }"
                } else {
                    ""
                };
                item.signature = Some(generic(
                    format!(
                        "{pub_}enum {name}{}{}{body}",
                        types::params(&e.generics),
                        types::where_clause(&e.generics)
                    ),
                    &e.generics,
                ));
                self.documented.insert(*id, full.clone());
                item.members = e
                    .variants
                    .iter()
                    .filter_map(|v| self.item(v, &full, None))
                    .collect();
                self.impls(&mut item, &e.impls);
                item
            }
            ItemEnum::Variant(v) => {
                let mut item = Item::new(ItemKind::Variant, full.clone());
                let mut text = name.clone();
                match &v.kind {
                    VariantKind::Plain => {}
                    VariantKind::Tuple(fields) => {
                        text.push_str(&format!("({})", self.tuple_fields(fields)));
                        item.members = self.documented_tuple_fields(fields, &full);
                    }
                    VariantKind::Struct {
                        fields,
                        has_stripped_fields,
                    } => {
                        let mut shown: Vec<String> = fields
                            .iter()
                            .filter_map(|f| krate.index.get(f))
                            .filter_map(|f| match &f.inner {
                                ItemEnum::StructField(t) => Some(format!(
                                    "{}: {}",
                                    f.name.as_deref().unwrap_or("_"),
                                    types::ty(t)
                                )),
                                _ => None,
                            })
                            .collect();
                        if *has_stripped_fields {
                            shown.push("..".into());
                        }
                        text.push_str(&format!(" {{ {} }}", shown.join(", ")));
                        item.members = fields
                            .iter()
                            .filter(|f| krate.index.get(f).is_some_and(|f| f.docs.is_some()))
                            .filter_map(|f| self.item(f, &full, None))
                            .collect();
                    }
                }
                if let Some(d) = &v.discriminant {
                    text.push_str(&format!(" = {}", d.expr));
                    item.value = Some(d.value.clone());
                }
                item.signature = Some(self::text(text));
                item
            }
            ItemEnum::Function(f) => {
                let inputs = types::inputs(&f.sig);
                let receiver = f.sig.inputs.first().is_some_and(|(n, _)| n == "self");
                let kind = if receiver {
                    ItemKind::Method
                } else {
                    ItemKind::Function
                };
                let mut item = Item::new(kind, full.clone());
                let head = format!(
                    "{pub_}{}fn {name}{}",
                    types::header(&f.header),
                    types::params(&f.generics)
                );
                let output = f
                    .sig
                    .output
                    .as_ref()
                    .map(|t| format!(" -> {}", types::ty(t)))
                    .unwrap_or_default();
                let written: Vec<&str> = inputs.iter().map(|(_, w)| w.as_str()).collect();
                let one_line = format!("{head}({}){output}", written.join(", "));
                let mut text = if one_line.len() > MAX_LINE {
                    format!("{head}(\n    {},\n){output}", written.join(",\n    "))
                } else {
                    one_line
                };
                text.push_str(&types::where_clause(&f.generics));
                let mut modifiers = Vec::new();
                for (on, word) in [
                    (f.header.is_const, "const"),
                    (f.header.is_async, "async"),
                    (f.header.is_unsafe, "unsafe"),
                ] {
                    if on {
                        modifiers.push(word.to_owned());
                    }
                }
                item.signature = Some(Signature {
                    generics: types::param_list(&f.generics),
                    params: f
                        .sig
                        .inputs
                        .iter()
                        .map(|(n, t)| Param {
                            name: n.clone(),
                            ty: Some(types::ty(t)),
                            default: None,
                            kind: if n == "self" {
                                ParamKind::Receiver
                            } else {
                                ParamKind::Normal
                            },
                        })
                        .collect(),
                    returns: f.sig.output.as_ref().map(types::ty),
                    modifiers,
                    text: Some(text),
                });
                item
            }
            ItemEnum::Trait(t) => {
                let mut item = Item::new(ItemKind::Trait, full.clone());
                let supertraits = if t.bounds.is_empty() {
                    String::new()
                } else {
                    format!(": {}", types::bounds(&t.bounds))
                };
                item.bases = t
                    .bounds
                    .iter()
                    .map(|b| types::bounds(std::slice::from_ref(b)))
                    .collect();
                item.signature = Some(generic(
                    format!(
                        "{pub_}{}{}trait {name}{}{supertraits}{}",
                        if t.is_unsafe { "unsafe " } else { "" },
                        if t.is_auto { "auto " } else { "" },
                        types::params(&t.generics),
                        types::where_clause(&t.generics)
                    ),
                    &t.generics,
                ));
                self.documented.insert(*id, full.clone());
                item.members = t
                    .items
                    .iter()
                    .filter_map(|i| self.item(i, &full, None))
                    .collect();
                // Trait methods: `;` when required, `{ ... }` when provided.
                for (member, mid) in item.members.iter_mut().zip(&t.items) {
                    if let Some(ItemEnum::Function(f)) = krate.index.get(mid).map(|i| &i.inner)
                        && let Some(sig) = &mut member.signature
                        && let Some(text) = &mut sig.text
                    {
                        text.push_str(if f.has_body { " { ... }" } else { ";" });
                    }
                }
                self.implementors(&full, &t.implementations);
                item
            }
            ItemEnum::TraitAlias(t) => {
                let mut item = Item::new(ItemKind::Other("trait alias".into()), full.clone());
                item.signature = Some(text(format!(
                    "{pub_}trait {name}{} = {}{};",
                    types::params(&t.generics),
                    types::bounds(&t.params),
                    types::where_clause(&t.generics)
                )));
                item
            }
            ItemEnum::TypeAlias(t) => {
                let mut item = Item::new(ItemKind::TypeAlias, full.clone());
                item.ty = Some(types::ty(&t.type_));
                item.signature = Some(generic(
                    format!(
                        "{pub_}type {name}{}{} = {};",
                        types::params(&t.generics),
                        types::where_clause(&t.generics),
                        types::ty(&t.type_)
                    ),
                    &t.generics,
                ));
                item
            }
            ItemEnum::Constant { type_, const_ } => {
                let mut item = Item::new(ItemKind::Constant, full.clone());
                let value = if const_.expr == "_" {
                    const_.value.clone().unwrap_or_else(|| "_".into())
                } else {
                    const_.expr.clone()
                };
                let value = cut(&value);
                item.ty = Some(types::ty(type_));
                item.value = Some(value.clone());
                item.signature = Some(text(format!(
                    "{pub_}const {name}: {} = {value};",
                    types::ty(type_)
                )));
                item
            }
            ItemEnum::Static(s) => {
                let mut item = Item::new(ItemKind::Variable, full.clone());
                item.ty = Some(types::ty(&s.type_));
                let value = cut(&s.expr);
                let value = if value.is_empty() {
                    String::new()
                } else {
                    format!(" = {value}")
                };
                item.signature = Some(text(format!(
                    "{pub_}static {}{name}: {}{value};",
                    if s.is_mutable { "mut " } else { "" },
                    types::ty(&s.type_)
                )));
                item
            }
            ItemEnum::ExternType => {
                let mut item = Item::new(ItemKind::Other("extern type".into()), full.clone());
                item.signature = Some(text(format!("{pub_}type {name};")));
                item
            }
            ItemEnum::Macro(source) => {
                let mut item = Item::new(ItemKind::Macro, full.clone());
                item.signature = Some(text(source.clone()));
                item
            }
            ItemEnum::ProcMacro(p) => {
                use rustdoc_types::MacroKind;
                let (kind, text) = match p.kind {
                    MacroKind::Bang => {
                        (ItemKind::Macro, format!("{name}!() {{ /* proc-macro */ }}"))
                    }
                    MacroKind::Attr => (
                        ItemKind::Other("attribute macro".into()),
                        format!("#[{name}]"),
                    ),
                    MacroKind::Derive => {
                        let mut text = format!("#[derive({name})]");
                        if !p.helpers.is_empty() {
                            text.push_str(&format!(
                                "\n// helper attributes: {}",
                                p.helpers
                                    .iter()
                                    .map(|h| format!("#[{h}]"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                        (ItemKind::Other("derive macro".into()), text)
                    }
                };
                let mut item = Item::new(kind, full.clone());
                item.signature = Some(self::text(text));
                item
            }
            ItemEnum::AssocConst { type_, value, .. } => {
                let mut item = Item::new(ItemKind::Constant, full.clone());
                item.ty = Some(types::ty(type_));
                let value = value
                    .as_deref()
                    .filter(|v| *v != "_")
                    .map(|v| format!(" = {}", cut(v)))
                    .unwrap_or_default();
                item.signature = Some(text(format!(
                    "{pub_}const {name}: {}{value};",
                    types::ty(type_)
                )));
                item
            }
            ItemEnum::AssocType {
                generics,
                bounds,
                type_,
                ..
            } => {
                let mut item = Item::new(ItemKind::TypeAlias, full.clone());
                let mut text = format!("type {name}{}", types::params(generics));
                if !bounds.is_empty() {
                    text.push_str(&format!(": {}", types::bounds(bounds)));
                }
                text.push_str(&types::where_clause(generics));
                if let Some(t) = type_ {
                    item.ty = Some(types::ty(t));
                    text.push_str(&format!(" = {}", types::ty(t)));
                }
                text.push(';');
                item.signature = Some(self::text(text));
                item
            }
            ItemEnum::Use(_)
            | ItemEnum::Impl(_)
            | ItemEnum::ExternCrate { .. }
            | ItemEnum::Primitive(_) => return None,
        };
        item.visibility = vis;
        item.docs.clone_from(&source.docs);
        item.location = self.location(source);
        if let Some(deprecation) = &source.deprecation
            && !matches!(source.inner, ItemEnum::StructField(_))
        {
            let text = match (&deprecation.since, &deprecation.note) {
                (Some(since), Some(note)) => format!("Since {since}: {note}"),
                (Some(since), None) => format!("Since {since}."),
                (None, Some(note)) => note.clone(),
                (None, None) => String::new(),
            };
            self.add(&full, format!("# Deprecated\n\n{text}"));
        }
        self.documented.entry(*id).or_insert_with(|| full.clone());
        self.ids.insert(full, *id);
        Some(item)
    }

    /// Adds a section to an item's doc comment.
    fn add(&mut self, path: &[String], text: String) {
        let added = self.added.entry(path.to_vec()).or_default();
        if !added.is_empty() {
            added.push_str("\n\n");
        }
        added.push_str(&text);
    }

    /// A tuple's fields, as written in it: `pub T, _`.
    fn tuple_fields(&self, fields: &[Option<Id>]) -> String {
        fields
            .iter()
            .map(|f| match f.and_then(|f| self.krate.index.get(&f)) {
                Some(field) => match &field.inner {
                    ItemEnum::StructField(t) => {
                        let pub_ = if field.visibility == RdVisibility::Public {
                            "pub "
                        } else {
                            ""
                        };
                        format!("{pub_}{}", types::ty(t))
                    }
                    _ => "_".into(),
                },
                None => "_".into(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// A tuple's documented fields, as members (`0`, `1`, ...).
    fn documented_tuple_fields(&mut self, fields: &[Option<Id>], path: &[String]) -> Vec<Item> {
        fields
            .iter()
            .flatten()
            .filter(|f| self.krate.index.get(f).is_some_and(|f| f.docs.is_some()))
            .filter_map(|f| self.item(f, path, None))
            .collect()
    }

    /// A type's impls: inherent ones' items become its members; the traits
    /// it implements are listed in its documentation.
    fn impls(&mut self, item: &mut Item, impls: &[Id]) {
        let mut traits: Vec<String> = Vec::new();
        for id in impls {
            let Some(ItemEnum::Impl(imp)) = self.krate.index.get(id).map(|i| &i.inner) else {
                continue;
            };
            if imp.is_synthetic || imp.blanket_impl.is_some() {
                continue;
            }
            match &imp.trait_ {
                None => {
                    let path = item.path.clone();
                    let members: Vec<Item> = imp
                        .items
                        .iter()
                        .filter_map(|i| self.item(i, &path, None))
                        .collect();
                    item.members.extend(members);
                }
                Some(trait_) => {
                    item.bases.push(types::path(trait_));
                    traits.push(impl_line(imp, trait_));
                }
            }
        }
        if !traits.is_empty() {
            self.add(
                &item.path,
                format!("# Trait implementations\n\n{}", traits.join("\n")),
            );
        }
    }

    /// Lists the types implementing a trait in its documentation.
    fn implementors(&mut self, path: &[String], impls: &[Id]) {
        let lines: Vec<String> = impls
            .iter()
            .filter_map(|id| match self.krate.index.get(id).map(|i| &i.inner) {
                Some(ItemEnum::Impl(imp)) if !imp.is_synthetic => {
                    let shown = types::ty(&imp.for_);
                    Some(match &imp.for_ {
                        Type::ResolvedPath(p) => {
                            format!(
                                "- {}[`{shown}`]({}){}",
                                negative(imp),
                                p.id.0,
                                generics_of(imp)
                            )
                        }
                        _ => format!("- {}`{shown}`{}", negative(imp), generics_of(imp)),
                    })
                }
                _ => None,
            })
            .collect();
        if !lines.is_empty() {
            self.add(path, format!("# Implementors\n\n{}", lines.join("\n")));
        }
    }
}

fn negative(imp: &Impl) -> &'static str {
    if imp.is_negative { "!" } else { "" }
}

fn generics_of(imp: &Impl) -> String {
    let params = types::params(&imp.generics);
    if params.is_empty() {
        String::new()
    } else {
        format!(" (`impl{params}`)")
    }
}

/// A trait implementation, as a list item linking to the trait: rustdoc
/// ids stand in for destinations (`[`Clone`](17)`), resolved later.
fn impl_line(imp: &Impl, trait_: &rustdoc_types::Path) -> String {
    format!(
        "- {}[`{}`]({}){}",
        negative(imp),
        types::path(trait_),
        trait_.id.0,
        generics_of(imp)
    )
}

fn visibility(v: &RdVisibility) -> Visibility {
    match v {
        RdVisibility::Public | RdVisibility::Default => Visibility::Public,
        RdVisibility::Crate | RdVisibility::Restricted { .. } => Visibility::Internal,
    }
}

fn text(text: String) -> Signature {
    Signature {
        text: Some(text),
        ..Signature::default()
    }
}

fn generic(text: String, generics: &rustdoc_types::Generics) -> Signature {
    Signature {
        generics: types::param_list(generics),
        text: Some(text),
        ..Signature::default()
    }
}

/// `value`, cut to [`MAX_VALUE`] characters.
fn cut(value: &str) -> String {
    let value = value.trim();
    if value.chars().count() > MAX_VALUE {
        let cut: String = value.chars().take(MAX_VALUE).collect();
        format!("{cut}...")
    } else {
        value.to_owned()
    }
}
