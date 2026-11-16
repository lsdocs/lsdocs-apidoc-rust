//! rustdoc's Markdown made into lsdocs' Markdown: intra-doc links become
//! references (or links to other crates' rustdoc pages), Rust code blocks
//! lose their hidden lines and say they are Rust.

use std::collections::HashMap;

use lsdocs_apidoc::docstring::{closes, fence};

/// What an intra-doc link's destination stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// An item documented here (in any of the crates): a reference to it,
    /// by its path.
    Item(Vec<String>),
    /// A page elsewhere (another crate's rustdoc pages).
    Url(String),
    /// Known, but nowhere to link to (a private item): its text only.
    Text,
}

/// Rewrites a doc comment. `resolve` maps a link destination, as written
/// (a key of rustdoc's `links`), to what it stands for; `None` if rustdoc
/// did not resolve it (a URL, say), which leaves the link as written.
pub(crate) fn clean(text: &str, resolve: &dyn Fn(&str) -> Option<Target>) -> String {
    let lines: Vec<&str> = text.lines().collect();
    // Link reference definitions whose destination rustdoc resolved go;
    // their labels map to the destinations.
    let mut definitions: HashMap<String, String> = HashMap::new();
    let mut dropped = vec![false; lines.len()];
    let mut open: Option<(char, usize)> = None;
    for (i, line) in lines.iter().enumerate() {
        match open {
            Some(o) => {
                if closes(o, line) {
                    open = None;
                }
            }
            None => {
                if let Some(f) = fence(line) {
                    open = Some(f);
                } else if let Some((label, dest)) = definition(line)
                    && resolve(dest).is_some()
                {
                    definitions.insert(label.to_lowercase(), dest.to_owned());
                    dropped[i] = true;
                }
            }
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut prose: Vec<&str> = Vec::new();
    let mut code: Option<((char, usize), bool)> = None;
    let flush = |prose: &mut Vec<&str>, out: &mut Vec<String>| {
        if !prose.is_empty() {
            out.push(links(&prose.join("\n"), &definitions, resolve));
            prose.clear();
        }
    };
    for (i, line) in lines.iter().enumerate() {
        if dropped[i] {
            continue;
        }
        match code {
            Some((o, rust)) => {
                if closes(o, line) {
                    code = None;
                    out.push((*line).to_owned());
                } else if rust {
                    if let Some(shown) = shown_line(line) {
                        out.push(shown);
                    }
                } else {
                    out.push((*line).to_owned());
                }
            }
            None => match fence(line) {
                Some(f) => {
                    flush(&mut prose, &mut out);
                    let indent = &line[..line.len() - line.trim_start().len()];
                    let marker: String = std::iter::repeat_n(f.0, f.1).collect();
                    let info = line.trim_start()[f.1..].trim();
                    let rust = is_rust(info);
                    out.push(if rust {
                        format!("{indent}{marker}rust")
                    } else {
                        (*line).to_owned()
                    });
                    code = Some((f, rust));
                }
                None => prose.push(line),
            },
        }
    }
    flush(&mut prose, &mut out);
    let mut text = out.join("\n");
    // Dropped definitions can leave blank lines at the end.
    text.truncate(text.trim_end().len());
    text
}

/// Whether a code block's info string makes it Rust, as rustdoc reads
/// it: no language, `rust`, or only rustdoc's attributes.
fn is_rust(info: &str) -> bool {
    info.split([',', ' ', '\t'])
        .filter(|t| !t.is_empty())
        .all(|t| {
            matches!(
                t,
                "rust"
                    | "ignore"
                    | "no_run"
                    | "should_panic"
                    | "compile_fail"
                    | "test_harness"
                    | "standalone_crate"
            ) || t.starts_with("edition")
                || t.starts_with("ignore-")
                || t.starts_with('{')
                || t.starts_with('E') && t.len() == 5 && t[1..].chars().all(|c| c.is_ascii_digit())
        })
}

/// A Rust code block's line as shown: rustdoc hides `# ` lines and `#`
/// alone, and shows `##` as `#`.
fn shown_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    if trimmed == "#" || trimmed.starts_with("# ") || trimmed.starts_with("#\t") {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("##") {
        return Some(format!("{indent}#{rest}"));
    }
    Some(line.to_owned())
}

/// `[label]: destination`, a link reference definition.
fn definition(line: &str) -> Option<(&str, &str)> {
    if line.len() - line.trim_start().len() > 3 {
        return None;
    }
    let rest = line.trim_start().strip_prefix('[')?;
    let (label, rest) = rest.split_once("]:")?;
    let dest = rest.split_whitespace().next()?;
    let dest = dest
        .strip_prefix('<')
        .and_then(|d| d.strip_suffix('>'))
        .unwrap_or(dest);
    (!label.is_empty()).then_some((label, dest))
}

/// Rewrites the links in a run of Markdown prose.
fn links(
    text: &str,
    definitions: &HashMap<String, String>,
    resolve: &dyn Fn(&str) -> Option<Target>,
) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        match bytes[i] {
            b'\\' => {
                let next = text[i + 1..].chars().next().map_or(0, char::len_utf8);
                out.push_str(&text[i..i + 1 + next]);
                i += 1 + next;
            }
            b'`' => {
                let end = code_span(text, i);
                out.push_str(&text[i..end]);
                i = end;
            }
            b'[' if i == 0 || bytes[i - 1] != b'!' => match link(text, i, definitions, resolve) {
                Some((replacement, end)) => {
                    out.push_str(&replacement);
                    i = end;
                }
                None => {
                    out.push('[');
                    i += 1;
                }
            },
            _ => {
                let len = text[i..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&text[i..i + len]);
                i += len;
            }
        }
    }
    out
}

/// The end of the code span starting at `start` (a backtick run), or of
/// the run itself if nothing closes it.
fn code_span(text: &str, start: usize) -> usize {
    let run = text[start..].bytes().take_while(|&b| b == b'`').count();
    let mut i = start + run;
    while let Some(found) = text[i..].find('`') {
        let at = i + found;
        let len = text[at..].bytes().take_while(|&b| b == b'`').count();
        if len == run {
            return at + len;
        }
        i = at + len;
    }
    start + run
}

/// The end of a bracketed (or parenthesized) run starting at `start`,
/// after its closing character; code spans inside are skipped.
fn matching(text: &str, start: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'`' if open == b'[' => {
                i = code_span(text, i);
                continue;
            }
            b if b == open => depth += 1,
            b if b == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The link at `start` (a `[`) rewritten, and where it ends; `None` if it
/// is not an intra-doc link.
fn link(
    text: &str,
    start: usize,
    definitions: &HashMap<String, String>,
    resolve: &dyn Fn(&str) -> Option<Target>,
) -> Option<(String, usize)> {
    let close = matching(text, start, b'[', b']')?;
    let inner = &text[start + 1..close - 1];
    let rest = &text[close..];
    let (dest, end) = if rest.starts_with('(') {
        let end = matching(text, close, b'(', b')')?;
        let dest = text[close + 1..end - 1].trim();
        let dest = dest
            .strip_prefix('<')
            .and_then(|d| d.strip_suffix('>'))
            .unwrap_or(dest);
        (dest.to_owned(), end)
    } else if rest.starts_with('[') {
        let end = matching(text, close, b'[', b']')?;
        let label = &text[close + 1..end - 1];
        let label = if label.is_empty() { inner } else { label };
        (
            definitions
                .get(&label.to_lowercase())
                .cloned()
                .unwrap_or_else(|| label.to_owned()),
            end,
        )
    } else {
        (
            definitions
                .get(&inner.to_lowercase())
                .cloned()
                .unwrap_or_else(|| inner.to_owned()),
            close,
        )
    };
    let target = resolve(&dest)?;
    let shown = inner.trim();
    Some((
        match target {
            Target::Item(path) => {
                let plain = shown.trim_matches('`').replace('`', "");
                format!("{{rs:obj}}`{} <{}>`", plain, path.join("::"))
            }
            Target::Url(url) => format!("[{shown}]({url})"),
            Target::Text => shown.to_owned(),
        },
        end,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(dest: &str) -> Option<Target> {
        match dest.trim_matches('`') {
            "P" | "crate::P" => Some(Target::Item(vec!["k".into(), "P".into()])),
            "Vec" => Some(Target::Url("https://doc.test/Vec.html".into())),
            "Hidden" => Some(Target::Text),
            _ => None,
        }
    }

    #[test]
    fn links_become_references() {
        let text = "See [`P`], [the\npoint][P], [`P::x`](crate::P), [`Vec`], [`Hidden`],\n[q], `[P]`, ![P](P.png), [site](https://x.test) and [r].\n\n[q]: crate::P\n[r]: https://y.test";
        assert_eq!(
            clean(text, &resolve),
            "See {rs:obj}`P <k::P>`, {rs:obj}`the\npoint <k::P>`, {rs:obj}`P::x <k::P>`, [`Vec`](https://doc.test/Vec.html), `Hidden`,\n{rs:obj}`q <k::P>`, `[P]`, ![P](P.png), [site](https://x.test) and [r].\n\n[r]: https://y.test"
        );
    }

    #[test]
    fn rust_code_blocks() {
        let text = "Use it:\n\n```\n# use k::P;\nlet p = P::new();\n## not hidden\n#\n```\n\n```text\n# shown\n```\n\n~~~no_run,edition2021\n# hidden\n[P]\n~~~";
        assert_eq!(
            clean(text, &resolve),
            "Use it:\n\n```rust\nlet p = P::new();\n# not hidden\n```\n\n```text\n# shown\n```\n\n~~~rust\n[P]\n~~~"
        );
    }
}
