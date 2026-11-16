//! rustdoc's types, generics and bounds written as Rust.

use rustdoc_types::{
    AssocItemConstraint, AssocItemConstraintKind, FunctionHeader, FunctionPointer,
    FunctionSignature, GenericArg, GenericArgs, GenericBound, GenericParamDef, GenericParamDefKind,
    Generics, Path, PreciseCapturingArg, Term, TraitBoundModifier, Type, WherePredicate,
};

/// A type, as Rust writes it.
pub(crate) fn ty(t: &Type) -> String {
    match t {
        Type::ResolvedPath(p) => path(p),
        Type::DynTrait(d) => {
            let mut parts: Vec<String> = d
                .traits
                .iter()
                .map(|p| format!("{}{}", hrtb(&p.generic_params), path(&p.trait_)))
                .collect();
            if let Some(lifetime) = &d.lifetime {
                parts.push(lifetime.clone());
            }
            format!("dyn {}", parts.join(" + "))
        }
        Type::Generic(name) | Type::Primitive(name) => name.clone(),
        Type::FunctionPointer(f) => function_pointer(f),
        Type::Tuple(types) if types.len() == 1 => format!("({},)", ty(&types[0])),
        Type::Tuple(types) => format!("({})", list(types.iter().map(ty))),
        Type::Slice(inner) => format!("[{}]", ty(inner)),
        Type::Array { type_, len } => format!("[{}; {len}]", ty(type_)),
        Type::Pat { type_, .. } => ty(type_),
        Type::ImplTrait(bounds) => format!("impl {}", self::bounds(bounds)),
        Type::Infer => "_".into(),
        Type::RawPointer { is_mutable, type_ } => {
            format!(
                "*{} {}",
                if *is_mutable { "mut" } else { "const" },
                ty(type_)
            )
        }
        Type::BorrowedRef {
            lifetime,
            is_mutable,
            type_,
        } => {
            let mut out = String::from("&");
            if let Some(lifetime) = lifetime {
                out.push_str(lifetime);
                out.push(' ');
            }
            if *is_mutable {
                out.push_str("mut ");
            }
            out.push_str(&ty(type_));
            out
        }
        Type::QualifiedPath {
            name,
            args,
            self_type,
            trait_,
        } => {
            let args = args.as_deref().map(generic_args).unwrap_or_default();
            match trait_ {
                // `Self::Item` and `T::Item`, as written.
                Some(t) if t.path.is_empty() => format!("{}::{name}{args}", ty(self_type)),
                Some(t) => format!("<{} as {}>::{name}{args}", ty(self_type), path(t)),
                None => format!("{}::{name}{args}", ty(self_type)),
            }
        }
    }
}

fn list(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

/// A path with its generic arguments: its last segment, as rustdoc
/// shows it (`Point`, not `crate::Point`).
pub(crate) fn path(p: &Path) -> String {
    let args = p.args.as_deref().map(generic_args).unwrap_or_default();
    let name = p.path.rsplit("::").next().unwrap_or(&p.path);
    format!("{name}{args}")
}

fn generic_args(args: &GenericArgs) -> String {
    match args {
        GenericArgs::AngleBracketed { args, constraints } => {
            let mut parts: Vec<String> = args.iter().map(generic_arg).collect();
            parts.extend(constraints.iter().map(constraint));
            if parts.is_empty() {
                String::new()
            } else {
                format!("<{}>", parts.join(", "))
            }
        }
        GenericArgs::Parenthesized { inputs, output } => {
            let mut out = format!("({})", list(inputs.iter().map(ty)));
            if let Some(output) = output {
                out.push_str(" -> ");
                out.push_str(&ty(output));
            }
            out
        }
        GenericArgs::ReturnTypeNotation => "(..)".into(),
    }
}

fn generic_arg(arg: &GenericArg) -> String {
    match arg {
        GenericArg::Lifetime(l) => l.clone(),
        GenericArg::Type(t) => ty(t),
        GenericArg::Const(c) => c.expr.clone(),
        GenericArg::Infer => "_".into(),
    }
}

fn constraint(c: &AssocItemConstraint) -> String {
    let args = c.args.as_deref().map(generic_args).unwrap_or_default();
    match &c.binding {
        AssocItemConstraintKind::Equality(term) => {
            format!("{}{args} = {}", c.name, self::term(term))
        }
        AssocItemConstraintKind::Constraint(b) => format!("{}{args}: {}", c.name, bounds(b)),
    }
}

fn term(t: &Term) -> String {
    match t {
        Term::Type(t) => ty(t),
        Term::Constant(c) => c.expr.clone(),
    }
}

/// Bounds joined with `+`.
pub(crate) fn bounds(bounds: &[GenericBound]) -> String {
    bounds.iter().map(bound).collect::<Vec<_>>().join(" + ")
}

fn bound(b: &GenericBound) -> String {
    match b {
        GenericBound::TraitBound {
            trait_,
            generic_params,
            modifier,
        } => {
            let modifier = match modifier {
                TraitBoundModifier::None => "",
                TraitBoundModifier::Maybe => "?",
                TraitBoundModifier::MaybeConst => "~const ",
            };
            format!("{}{modifier}{}", hrtb(generic_params), path(trait_))
        }
        GenericBound::Outlives(lifetime) => lifetime.clone(),
        GenericBound::Use(args) => format!(
            "use<{}>",
            list(args.iter().map(|a| match a {
                PreciseCapturingArg::Lifetime(l) => l.clone(),
                PreciseCapturingArg::Param(p) => p.clone(),
            }))
        ),
    }
}

/// `for<'a> `, for higher-ranked bounds.
fn hrtb(params: &[GenericParamDef]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!("for<{}> ", list(params.iter().map(param)))
    }
}

/// A generic parameter with its bounds and default.
fn param(p: &GenericParamDef) -> String {
    match &p.kind {
        GenericParamDefKind::Lifetime { outlives } if outlives.is_empty() => p.name.clone(),
        GenericParamDefKind::Lifetime { outlives } => {
            format!("{}: {}", p.name, outlives.join(" + "))
        }
        GenericParamDefKind::Type {
            bounds: b, default, ..
        } => {
            let mut out = p.name.clone();
            if !b.is_empty() {
                out.push_str(": ");
                out.push_str(&bounds(b));
            }
            if let Some(default) = default {
                out.push_str(" = ");
                out.push_str(&ty(default));
            }
            out
        }
        GenericParamDefKind::Const { type_, default } => {
            let mut out = format!("const {}: {}", p.name, ty(type_));
            if let Some(default) = default {
                out.push_str(" = ");
                out.push_str(default);
            }
            out
        }
    }
}

/// Generic parameters as written after a name, `<'a, T: Clone>`; those
/// rustdoc made up for `impl Trait` arguments are left out.
pub(crate) fn params(generics: &Generics) -> String {
    let shown: Vec<String> = generics
        .params
        .iter()
        .filter(|p| {
            !matches!(
                p.kind,
                GenericParamDefKind::Type {
                    is_synthetic: true,
                    ..
                }
            )
        })
        .map(param)
        .collect();
    if shown.is_empty() {
        String::new()
    } else {
        format!("<{}>", shown.join(", "))
    }
}

/// Generic parameters each as written, for the model.
pub(crate) fn param_list(generics: &Generics) -> Vec<String> {
    generics
        .params
        .iter()
        .filter(|p| {
            !matches!(
                p.kind,
                GenericParamDefKind::Type {
                    is_synthetic: true,
                    ..
                }
            )
        })
        .map(param)
        .collect()
}

/// A `where` clause on lines of its own (`\nwhere\n    T: Clone,`), or
/// nothing.
pub(crate) fn where_clause(generics: &Generics) -> String {
    let predicates: Vec<String> = generics
        .where_predicates
        .iter()
        .filter_map(|w| match w {
            WherePredicate::BoundPredicate {
                type_,
                bounds: b,
                generic_params,
            } => {
                // Bounds rustdoc moved from `impl Trait` arguments.
                if let Type::Generic(name) = type_
                    && name.starts_with("impl ")
                {
                    return None;
                }
                Some(format!(
                    "{}{}: {}",
                    hrtb(generic_params),
                    ty(type_),
                    bounds(b)
                ))
            }
            WherePredicate::LifetimePredicate { lifetime, outlives } => {
                Some(format!("{lifetime}: {}", outlives.join(" + ")))
            }
            WherePredicate::EqPredicate { lhs, rhs } => {
                Some(format!("{} == {}", ty(lhs), term(rhs)))
            }
        })
        .collect();
    if predicates.is_empty() {
        String::new()
    } else {
        format!("\nwhere\n    {},", predicates.join(",\n    "))
    }
}

/// `const async unsafe extern "C" `, as a function's header has it.
pub(crate) fn header(h: &FunctionHeader) -> String {
    let mut out = String::new();
    if h.is_const {
        out.push_str("const ");
    }
    if h.is_async {
        out.push_str("async ");
    }
    if h.is_unsafe {
        out.push_str("unsafe ");
    }
    if let Some(abi) = abi(&h.abi) {
        out.push_str(&format!("extern \"{abi}\" "));
    }
    out
}

fn abi(abi: &rustdoc_types::Abi) -> Option<String> {
    use rustdoc_types::Abi;
    let (name, unwind) = match abi {
        Abi::Rust => return None,
        Abi::C { unwind } => ("C", unwind),
        Abi::Cdecl { unwind } => ("cdecl", unwind),
        Abi::Stdcall { unwind } => ("stdcall", unwind),
        Abi::Fastcall { unwind } => ("fastcall", unwind),
        Abi::Aapcs { unwind } => ("aapcs", unwind),
        Abi::Win64 { unwind } => ("win64", unwind),
        Abi::SysV64 { unwind } => ("sysv64", unwind),
        Abi::System { unwind } => ("system", unwind),
        Abi::Other(name) => return Some(name.clone()),
    };
    Some(if *unwind {
        format!("{name}-unwind")
    } else {
        name.to_owned()
    })
}

/// A function's parameters, `self` as Rust writes it (`&self`,
/// `&mut self`, `self: Box<Self>`), each as `(name, written)`.
pub(crate) fn inputs(sig: &FunctionSignature) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = sig
        .inputs
        .iter()
        .map(|(name, t)| {
            let written = if name == "self" {
                match t {
                    Type::Generic(s) if s == "Self" => "self".to_owned(),
                    Type::BorrowedRef {
                        lifetime,
                        is_mutable,
                        type_,
                    } if matches!(type_.as_ref(), Type::Generic(s) if s == "Self") => {
                        let mut out = String::from("&");
                        if let Some(lifetime) = lifetime {
                            out.push_str(lifetime);
                            out.push(' ');
                        }
                        if *is_mutable {
                            out.push_str("mut ");
                        }
                        out.push_str("self");
                        out
                    }
                    other => format!("self: {}", ty(other)),
                }
            } else {
                format!("{name}: {}", ty(t))
            };
            (name.clone(), written)
        })
        .collect();
    if sig.is_c_variadic {
        out.push(("...".into(), "...".into()));
    }
    out
}

fn function_pointer(f: &FunctionPointer) -> String {
    let inputs: Vec<String> = f
        .sig
        .inputs
        .iter()
        .map(|(name, t)| {
            if name.is_empty() || name == "_" {
                ty(t)
            } else {
                format!("{name}: {}", ty(t))
            }
        })
        .collect();
    let mut out = format!(
        "{}{}fn({})",
        hrtb(&f.generic_params),
        header(&f.header),
        inputs.join(", ")
    );
    if let Some(output) = &f.sig.output {
        out.push_str(" -> ");
        out.push_str(&ty(output));
    }
    out
}
