//! Comment-preserving bounded editing of settings YAML documents.
//!
//! Shared by the desktop template merge ([`crate::desktop_settings`]) and the
//! runtime settings writers ([`crate::runtime_config`]). Java persists settings
//! through snakeyaml configured with `parseComments`/`dumpComments`
//! (`YamlHelper`, driven by `GeneralUtils.saveKeyToSettings`), so comments,
//! ordering and blank lines in `settings.yml` survive every write. A serde
//! round-trip destroys all of that, so these helpers instead rewrite only the
//! value portion of the targeted `key:` lines — every other byte of the
//! document is preserved verbatim.
//!
//! Scope limitation (shared with the template merge): only values that live
//! inline on their key's line — scalars and inline flow sequences (`[]`,
//! `[a, b]`) — are addressable. Keys opening nested block mappings are treated
//! as structure, not values.

use std::collections::{HashMap, HashSet};

/// The YAML quoting style of a leaf's existing value text, so a replacement
/// value is re-emitted in the document's own style rather than the emitter's
/// default rendering.
#[derive(Clone, Copy)]
enum ScalarStyle {
    DoubleQuoted,
    SingleQuoted,
    Plain,
}

/// One physical line of a document: its content, its exact terminator
/// (`""`, `"\n"`, or `"\r\n"`), and its indent width — `None` for blank and
/// comment lines, which carry no mapping structure.
struct Line<'a> {
    content: &'a str,
    terminator: &'a str,
    indent: Option<usize>,
}

fn parse_lines(document: &str) -> Vec<Line<'_>> {
    document
        .split_inclusive('\n')
        .map(|chunk| {
            let (content, terminator) = split_line_terminator(chunk);
            let trimmed = content.trim_start();
            let indent = (!trimmed.is_empty() && !trimmed.starts_with('#'))
                .then(|| content.len() - trimmed.len());
            Line {
                content,
                terminator,
                indent,
            }
        })
        .collect()
}

/// Splits a `split_inclusive('\n')` chunk into its content and its line
/// terminator (`""`, `"\n"`, or `"\r\n"`), so exact newlines — including a
/// possibly missing final one — round-trip untouched.
fn split_line_terminator(chunk: &str) -> (&str, &str) {
    let Some(body) = chunk.strip_suffix('\n') else {
        return (chunk, "");
    };
    let split_at = body.strip_suffix('\r').map_or(body.len(), str::len);
    (&chunk[..split_at], &chunk[split_at..])
}

/// The result of [`rewrite_inline_values`]: the rewritten document plus the set
/// of requested paths that were found as addressable leaves (whether or not
/// their value actually changed), and the requested paths that exist but open a
/// nested block mapping — those are structure, not writable values.
pub struct InlineRewrite {
    /// The rewritten document, byte-identical to the input outside the
    /// rewritten value spans.
    pub content: String,
    pub(crate) matched: HashSet<String>,
    pub(crate) blocked: HashSet<String>,
}

/// Walks `document` line by line, tracking the current key path via
/// indentation, and rewrites only the value portion of each leaf line whose
/// lowercased dotted path appears in `replacements` (keys MUST be
/// ASCII-lowercased dotted paths — lookups are ASCII-case-insensitive to match
/// Java's relaxed binding). Every other byte — comments, blank lines, parent
/// keys, indentation, inline comments — is preserved exactly.
///
/// A key line with no inline value that also has no child lines (an
/// empty/null leaf such as `UUID:`) is fillable: the rendered value is
/// inserted after the colon, keeping any trailing inline comment.
#[must_use]
pub fn rewrite_inline_values<S: std::hash::BuildHasher>(
    document: &str,
    replacements: &HashMap<String, serde_yaml::Value, S>,
) -> InlineRewrite {
    let lines = parse_lines(document);
    let mut output = String::with_capacity(document.len());
    let mut matched = HashSet::new();
    let mut blocked = HashSet::new();
    // Mapping keys currently open, as (indent width, lowercased key).
    let mut open_mappings: Vec<(usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        match rewrite_line(
            line,
            index,
            &lines,
            &mut open_mappings,
            replacements,
            &mut blocked,
        ) {
            Some((rewritten, path)) => {
                matched.insert(path);
                output.push_str(&rewritten);
            }
            None => output.push_str(line.content),
        }
        output.push_str(line.terminator);
    }
    InlineRewrite {
        content: output,
        matched,
        blocked,
    }
}

/// Returns the rewritten line and its matched path if this line is an
/// addressable leaf for one of the replacements, or `None` to keep the line
/// verbatim. Updates `open_mappings` as it descends and ascends the document's
/// indentation.
fn rewrite_line<S: std::hash::BuildHasher>(
    line: &Line<'_>,
    index: usize,
    lines: &[Line<'_>],
    open_mappings: &mut Vec<(usize, String)>,
    replacements: &HashMap<String, serde_yaml::Value, S>,
    blocked: &mut HashSet<String>,
) -> Option<(String, String)> {
    // Blank lines and comments carry no structure: emit verbatim and leave the
    // open-mapping stack untouched.
    let indent = line.indent?;
    let content = line.content;
    let trimmed = &content[indent..];
    // Every mutable line in a settings document is a `key:` mapping entry, and
    // no settings key contains a colon, so the first colon separates key from
    // value. Lines without a colon (e.g. block-sequence items) stay verbatim.
    let colon = trimmed.find(':')?;
    let key = &trimmed[..colon];
    let after_colon = &trimmed[colon + 1..];

    // Dedent: drop mappings at or deeper than this line before resolving its path.
    while open_mappings
        .last()
        .is_some_and(|(open_indent, _)| *open_indent >= indent)
    {
        open_mappings.pop();
    }

    let path = dotted_path(open_mappings, key);
    let Some((value_start, value_end)) = value_span(after_colon) else {
        // Nothing but (optionally) a comment after the colon: either an
        // empty/null leaf or the opener of a nested mapping, disambiguated by
        // whether any structurally deeper line follows.
        let opens_mapping = next_structural_indent(lines, index + 1)
            .is_some_and(|next_indent| next_indent > indent);
        if opens_mapping || !replacements.contains_key(&path) {
            if opens_mapping && replacements.contains_key(&path) {
                blocked.insert(path.clone());
            }
            open_mappings.push((indent, key.to_ascii_lowercase()));
            return None;
        }
        let rendered = render_value(&replacements[&path], ScalarStyle::Plain)?;
        // Insert ` value` directly after the colon; any trailing whitespace and
        // inline comment stay in place.
        let colon_offset = indent + colon + 1;
        let mut rewritten = String::with_capacity(content.len() + rendered.len() + 1);
        rewritten.push_str(&content[..colon_offset]);
        rewritten.push(' ');
        rewritten.push_str(&rendered);
        rewritten.push_str(&content[colon_offset..]);
        return Some((rewritten, path));
    };

    let value_text = &after_colon[value_start..value_end];
    let replacement = replacements.get(&path)?;
    let rendered = render_value(replacement, scalar_style(value_text))?;
    if rendered == value_text {
        // Already the requested value: the path is matched, the bytes stand.
        return Some((content.to_owned(), path));
    }

    // Rewrite ONLY the value portion, keeping indentation, key, the gap before
    // any inline comment, and the comment itself byte-for-byte. Offsets are
    // computed in `content`, and every slice boundary sits on the ASCII value
    // region.
    let value_offset = indent + colon + 1 + value_start;
    let value_offset_end = indent + colon + 1 + value_end;
    let mut rewritten = String::with_capacity(content.len() + rendered.len());
    rewritten.push_str(&content[..value_offset]);
    rewritten.push_str(&rendered);
    rewritten.push_str(&content[value_offset_end..]);
    Some((rewritten, path))
}

fn next_structural_indent(lines: &[Line<'_>], from: usize) -> Option<usize> {
    lines.get(from..)?.iter().find_map(|line| line.indent)
}

/// Comment-preserving upsert of `section.key` scalar values into a settings
/// document: existing value lines are rewritten in place (via
/// [`rewrite_inline_values`]), keys the document lacks are inserted into the
/// existing top-level section, and a missing section is appended at the end.
/// Section and key lookups are ASCII-case-insensitive, so an existing relaxed
/// spelling (`automaticallyGenerated:`) is reused rather than duplicated. A
/// top-level `section:` line carrying a non-mapping inline value is repaired
/// into a section opener (its value is dropped, matching the serde writer this
/// replaces), keeping any inline comment.
///
/// # Errors
///
/// Returns a description when a value cannot be rendered as a single-line
/// inline YAML scalar/flow sequence (e.g. a nested mapping or a multi-line
/// string).
pub(crate) fn upsert_section_values(
    document: &str,
    section: &str,
    entries: &[(&str, serde_yaml::Value)],
) -> Result<String, String> {
    let mut replacements = HashMap::with_capacity(entries.len());
    for (key, value) in entries {
        if render_value(value, ScalarStyle::Plain).is_none() {
            return Err(format!(
                "the value for {section}.{key} cannot be written as an inline YAML scalar"
            ));
        }
        replacements.insert(
            format!("{section}.{key}").to_ascii_lowercase(),
            value.clone(),
        );
    }
    let rewrite = rewrite_inline_values(document, &replacements);
    if let Some((key, _)) = entries.iter().find(|(key, _)| {
        rewrite
            .blocked
            .contains(&format!("{section}.{key}").to_ascii_lowercase())
    }) {
        // Inserting the key anyway would create a duplicate and corrupt the
        // document, so refuse cleanly instead.
        return Err(format!(
            "{section}.{key} exists as a nested mapping and cannot be replaced with a scalar"
        ));
    }
    let missing: Vec<&(&str, serde_yaml::Value)> = entries
        .iter()
        .filter(|(key, _)| {
            !rewrite
                .matched
                .contains(&format!("{section}.{key}").to_ascii_lowercase())
        })
        .collect();
    if missing.is_empty() {
        return Ok(rewrite.content);
    }
    Ok(insert_into_section(&rewrite.content, section, &missing))
}

/// Inserts `key: value` lines for `entries` at the end of the top-level
/// `section` block (creating the section at the end of the document when it
/// does not exist), preserving every existing byte apart from a repaired
/// non-mapping section value.
fn insert_into_section(
    document: &str,
    section: &str,
    entries: &[&(&str, serde_yaml::Value)],
) -> String {
    let lines = parse_lines(document);
    let header = lines.iter().position(|line| {
        line.indent == Some(0)
            && line.content.split(':').next().is_some_and(|key| {
                line.content[..key.len()].eq_ignore_ascii_case(section)
                    && line.content.len() > key.len()
            })
    });
    let Some(header) = header else {
        let mut output = document.to_owned();
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(section);
        output.push_str(":\n");
        for (key, value) in entries {
            push_entry_line(&mut output, "  ", key, value);
        }
        return output;
    };

    // The section's children are the structurally deeper lines that follow the
    // header, up to the next top-level structural line. Insertion goes right
    // after the LAST child, so blank lines and comment banners belonging to the
    // following section stay attached to it. Child indentation follows the
    // existing children (default two spaces).
    let mut last_child = header;
    let mut child_indent: Option<&str> = None;
    for (index, line) in lines.iter().enumerate().skip(header + 1) {
        match line.indent {
            Some(0) => break,
            Some(indent) => {
                last_child = index;
                child_indent.get_or_insert(&line.content[..indent]);
            }
            None => {}
        }
    }
    let child_indent = child_indent.unwrap_or("  ");

    let mut output = String::with_capacity(document.len() + entries.len() * 48);
    for (index, line) in lines.iter().enumerate() {
        if index == header {
            output.push_str(&repaired_section_opener(line.content));
        } else {
            output.push_str(line.content);
        }
        if index == last_child {
            if line.terminator.is_empty() {
                output.push('\n');
            } else {
                output.push_str(line.terminator);
            }
            for (key, value) in entries {
                push_entry_line(&mut output, child_indent, key, value);
            }
        } else {
            output.push_str(line.terminator);
        }
    }
    output
}

/// Drops a non-mapping inline value from a `section: value` header line so the
/// inserted children form a mapping, keeping any inline comment. A header that
/// already opens a mapping is returned unchanged.
fn repaired_section_opener(content: &str) -> String {
    let Some(colon) = content.find(':') else {
        return content.to_owned();
    };
    let after_colon = &content[colon + 1..];
    let Some((_, value_end)) = value_span(after_colon) else {
        return content.to_owned();
    };
    format!("{}{}", &content[..=colon], &after_colon[value_end..])
}

fn push_entry_line(output: &mut String, indent: &str, key: &str, value: &serde_yaml::Value) {
    output.push_str(indent);
    output.push_str(key);
    output.push_str(": ");
    // The caller (`upsert_section_values`) pre-validated renderability.
    output.push_str(&render_value(value, ScalarStyle::Plain).unwrap_or_default());
    output.push('\n');
}

/// Locates the value token within the text after a `key:` separator, returning
/// its `(start, end)` byte offsets, or `None` when only whitespace/an inline
/// comment follows (i.e. the key opens a nested mapping or holds an empty
/// value). Whitespace inside the value is retained; a trailing ` # comment`
/// and surrounding gap are excluded.
fn value_span(after_colon: &str) -> Option<(usize, usize)> {
    let bytes = after_colon.as_bytes();
    let start = bytes
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t'))?;
    if bytes[start] == b'#' {
        return None;
    }
    let mut end = start;
    let mut quote: Option<u8> = None;
    for index in start..bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(active) => {
                end = index + 1;
                if byte == active {
                    quote = None;
                }
            }
            None => match byte {
                // A YAML inline comment starts at a `#` preceded by whitespace.
                b'#' if matches!(bytes[index - 1], b' ' | b'\t') => break,
                b'"' | b'\'' => {
                    quote = Some(byte);
                    end = index + 1;
                }
                b' ' | b'\t' => {}
                _ => end = index + 1,
            },
        }
    }
    Some((start, end))
}

fn dotted_path(open_mappings: &[(usize, String)], key: &str) -> String {
    let mut path = String::new();
    for (_, open_key) in open_mappings {
        path.push_str(open_key);
        path.push('.');
    }
    path.push_str(&key.to_ascii_lowercase());
    path
}

fn scalar_style(value_text: &str) -> ScalarStyle {
    let bytes = value_text.as_bytes();
    match (bytes.first(), bytes.last()) {
        (Some(b'"'), Some(b'"')) if bytes.len() >= 2 => ScalarStyle::DoubleQuoted,
        (Some(b'\''), Some(b'\'')) if bytes.len() >= 2 => ScalarStyle::SingleQuoted,
        _ => ScalarStyle::Plain,
    }
}

/// Renders a value into the requested quoting style. Returns `None` for nested
/// collections (mappings / tagged / nested sequences), which are out of the
/// inline scope.
fn render_value(value: &serde_yaml::Value, style: ScalarStyle) -> Option<String> {
    match value {
        serde_yaml::Value::Bool(flag) => Some(flag.to_string()),
        serde_yaml::Value::Number(number) => Some(number.to_string()),
        serde_yaml::Value::Null => Some("null".to_owned()),
        serde_yaml::Value::String(text) => render_scalar_string(text, style),
        serde_yaml::Value::Sequence(items) => render_flow_sequence(items),
        serde_yaml::Value::Mapping(_) | serde_yaml::Value::Tagged(_) => None,
    }
}

/// Renders a String into a leaf's quoting style.
///
/// For a DOUBLE- or SINGLE-quoted value the string is re-emitted in that same
/// style. For a PLAIN-styled value it is emitted as an inline YAML scalar that
/// reparses to exactly `text` — bare when plain-safe, otherwise quoted with
/// correct escaping (see [`render_plain_inline_scalar`]). Returns `None` only
/// when the plain rendering would span multiple lines, which is outside the
/// single-line inline scope.
fn render_scalar_string(text: &str, style: ScalarStyle) -> Option<String> {
    match style {
        ScalarStyle::DoubleQuoted => Some(format!("\"{}\"", escape_double_quoted(text))),
        ScalarStyle::SingleQuoted => Some(format!("'{}'", text.replace('\'', "''"))),
        ScalarStyle::Plain => render_plain_inline_scalar(text),
    }
}

/// Emits `text` as a single-line inline YAML scalar using `serde_yaml`'s own
/// emitter, so the "is this plain-safe / how must it be quoted" decision is
/// parser-backed rather than a hand-maintained list of unsafe characters. A
/// plain-safe value comes out bare (`postgres`), matching a plain-styled
/// document with no quoting churn; a value that a raw emit would corrupt — one
/// carrying a `#`/`:`/`*`/leading-indicator, leading/trailing space, an empty
/// string, or one that would otherwise reparse as a bool/number/null — comes
/// out correctly single- or double-quoted so it round-trips back to exactly
/// `text`.
///
/// Returns `None` if `serde_yaml` selects a block/multiline style (only
/// reachable for a value containing a newline; a settings scalar is
/// single-line) or fails to serialize, so the caller never injects a line
/// break that would corrupt the surrounding line.
fn render_plain_inline_scalar(text: &str) -> Option<String> {
    let serialized = serde_yaml::to_string(&serde_yaml::Value::String(text.to_owned())).ok()?;
    let inline = serialized.strip_suffix('\n').unwrap_or(&serialized);
    if inline.contains('\n') {
        return None;
    }
    Some(inline.to_owned())
}

fn escape_double_quoted(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn render_flow_sequence(items: &[serde_yaml::Value]) -> Option<String> {
    let mut rendered = Vec::with_capacity(items.len());
    for item in items {
        let part = match item {
            serde_yaml::Value::Bool(flag) => flag.to_string(),
            serde_yaml::Value::Number(number) => number.to_string(),
            serde_yaml::Value::Null => "null".to_owned(),
            serde_yaml::Value::String(text) => format!("\"{}\"", escape_double_quoted(text)),
            // Nested collections in a flow list are beyond the inline scope.
            _ => return None,
        };
        rendered.push(part);
    }
    Some(format!("[{}]", rendered.join(", ")))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{rewrite_inline_values, upsert_section_values};

    fn string_value(text: &str) -> serde_yaml::Value {
        serde_yaml::Value::String(text.to_owned())
    }

    #[test]
    fn rewrites_only_the_value_and_keeps_every_comment_byte() {
        let document = "# banner\nsection: # opener comment\n  key: old # inline comment\n\nother:\n  untouched: yes\n";
        let replacements = HashMap::from([("section.key".to_owned(), string_value("new"))]);
        let rewrite = rewrite_inline_values(document, &replacements);
        assert_eq!(
            rewrite.content,
            "# banner\nsection: # opener comment\n  key: new # inline comment\n\nother:\n  untouched: yes\n"
        );
        assert!(rewrite.matched.contains("section.key"));
    }

    #[test]
    fn fills_an_empty_childless_leaf_and_keeps_its_inline_comment() {
        let document = "section:\n  empty: # fill me\n  opener:\n    nested: 1\n";
        let replacements = HashMap::from([
            ("section.empty".to_owned(), string_value("filled")),
            ("section.opener".to_owned(), string_value("must-not-apply")),
        ]);
        let rewrite = rewrite_inline_values(document, &replacements);
        assert_eq!(
            rewrite.content,
            "section:\n  empty: filled # fill me\n  opener:\n    nested: 1\n"
        );
        assert!(rewrite.matched.contains("section.empty"));
        // A key opening a nested mapping is structure, never a writable leaf.
        assert!(!rewrite.matched.contains("section.opener"));
    }

    #[test]
    fn upsert_inserts_missing_keys_before_the_next_section_banner()
    -> Result<(), Box<dyn std::error::Error>> {
        let document = "alpha:\n  present: 1\n\n# beta banner\nbeta:\n  other: 2\n";
        let updated = upsert_section_values(
            document,
            "alpha",
            &[
                ("present", serde_yaml::Value::Number(9.into())),
                ("added", string_value("value")),
            ],
        )?;
        assert_eq!(
            updated,
            "alpha:\n  present: 9\n  added: value\n\n# beta banner\nbeta:\n  other: 2\n"
        );
        Ok(())
    }

    #[test]
    fn upsert_creates_a_missing_section_and_reuses_relaxed_spellings()
    -> Result<(), Box<dyn std::error::Error>> {
        let created = upsert_section_values("", "Generated", &[("key", string_value("v"))])?;
        assert_eq!(created, "Generated:\n  key: v\n");

        let relaxed = upsert_section_values(
            "generated:\n  existing: kept\n",
            "Generated",
            &[("key", string_value("v"))],
        )?;
        assert_eq!(relaxed, "generated:\n  existing: kept\n  key: v\n");
        Ok(())
    }

    #[test]
    fn upsert_repairs_a_scalar_section_into_a_mapping() -> Result<(), Box<dyn std::error::Error>> {
        let updated = upsert_section_values(
            "before: 1\nsection: 42\nafter: 2\n",
            "section",
            &[("key", string_value("v"))],
        )?;
        assert_eq!(updated, "before: 1\nsection:\n  key: v\nafter: 2\n");
        let parsed: serde_yaml::Value = serde_yaml::from_str(&updated)?;
        assert_eq!(parsed["section"]["key"], string_value("v"));
        Ok(())
    }

    #[test]
    fn upsert_refuses_to_shadow_a_nested_mapping_key() {
        // Inserting `section.key` here would duplicate the existing opener and
        // corrupt the document; the upsert must fail cleanly instead.
        let document = "section:\n  key:\n    nested: 1\n";
        let result = upsert_section_values(document, "section", &[("key", string_value("v"))]);
        assert!(result.is_err());
    }

    #[test]
    fn upsert_rejects_values_with_no_inline_rendering() {
        let nested = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        assert!(upsert_section_values("", "section", &[("key", nested)]).is_err());
        assert!(
            upsert_section_values("", "section", &[("key", string_value("two\nlines"))]).is_err()
        );
    }
}
