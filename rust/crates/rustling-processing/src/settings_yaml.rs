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
//! as structure, not values, and a key holding a block scalar (`key: |` /
//! `key: >`) is refused rather than rewritten: replacing only the indicator
//! would fold the scalar's continuation lines into the new value, silently
//! persisting the wrong data.

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
/// nested block mapping or hold a block scalar (`|` / `>`) — those are block
/// structure, not inline-writable values.
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
    // While inside a block scalar (`key: |`), the indent of its key line:
    // every deeper line is literal scalar content, never mapping structure.
    let mut block_scalar_indent: Option<usize> = None;
    for (index, line) in lines.iter().enumerate() {
        match rewrite_line(
            line,
            index,
            &lines,
            &mut open_mappings,
            &mut block_scalar_indent,
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
    block_scalar_indent: &mut Option<usize>,
    replacements: &HashMap<String, serde_yaml::Value, S>,
    blocked: &mut HashSet<String>,
) -> Option<(String, String)> {
    // Blank lines and comments carry no structure: emit verbatim and leave the
    // open-mapping stack untouched. (Inside a block scalar they are literal
    // content, which is equally verbatim.)
    let indent = line.indent?;
    // Block-scalar content: every line deeper than the scalar's key line is
    // literal text — even when it looks like a `key: value` entry — so it must
    // never be rewritten or tracked as mapping structure.
    if let Some(scalar_indent) = *block_scalar_indent {
        if indent > scalar_indent {
            return None;
        }
        *block_scalar_indent = None;
    }
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
    if is_block_scalar_header(value_text) {
        // A block scalar (`key: |` / `key: >`): rewriting only the indicator
        // span would leave the continuation lines to fold into the new value
        // as a plain multiline scalar — valid YAML carrying the WRONG data.
        // Refuse the path and skip the scalar's content lines entirely.
        *block_scalar_indent = Some(indent);
        if replacements.contains_key(&path) {
            blocked.insert(path);
        }
        return None;
    }
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
/// top-level `section:` line carrying an inline scalar value is repaired
/// into a section opener (its value is dropped, matching the serde writer this
/// replaces), keeping any inline comment.
///
/// # Errors
///
/// Returns a description when a value cannot be rendered as a single-line
/// inline YAML scalar/flow sequence (e.g. a nested mapping or a multi-line
/// string), when the target section holds an inline flow collection
/// (`section: {…}` / `section: […]`) — real data that inserting block children
/// would silently destroy — or a block scalar (`section: |`), when the section
/// holds a block sequence (its children are `- item` lines a mapping entry
/// cannot join), when a targeted key holds a block scalar (rewriting only the
/// indicator would fold its continuation lines into the new value), or when
/// the document root itself is a flow collection or block sequence (see
/// [`root_is_flow_collection`] / [`root_is_block_sequence`]).
pub(crate) fn upsert_section_values(
    document: &str,
    section: &str,
    entries: &[(&str, serde_yaml::Value)],
) -> Result<String, String> {
    let document = empty_flow_mapping_as_blank(document);
    if root_is_flow_collection(document) {
        return Err(
            "the document root is an inline flow collection and cannot hold block sections"
                .to_owned(),
        );
    }
    if root_is_block_sequence(document) {
        return Err(
            "the document root is a block sequence and cannot hold mapping sections".to_owned(),
        );
    }
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
            "{section}.{key} holds a nested block mapping or block scalar and cannot be \
             rewritten as an inline value"
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
    insert_into_section(&rewrite.content, section, &missing)
}

/// Whether `value` can be written by this editor as a single-line inline YAML
/// scalar or flow sequence (see [`render_value`]); nested mappings, tagged
/// values, nested sequences, and multi-line strings cannot.
pub(crate) fn renders_inline(value: &serde_yaml::Value) -> bool {
    render_value(value, ScalarStyle::Plain).is_some()
}

/// Treats a document that is exactly an EMPTY flow mapping (`{}`) as a blank
/// document: the serde-based writer this machinery replaced serialized "no
/// settings" as `{}`, so such files exist at rest. They hold no data, so
/// starting a fresh block mapping loses nothing — unlike a populated flow
/// root, which is refused (see [`root_is_flow_collection`]).
fn empty_flow_mapping_as_blank(document: &str) -> &str {
    if document.trim() == "{}" {
        ""
    } else {
        document
    }
}

/// Whether the document's root node is an inline flow collection (`{…}` /
/// `[…]`): the first non-blank, non-comment line's content starts with a flow
/// indicator. Such a document parses as a mapping/sequence but carries no
/// block structure this editor can extend — a section-header scan never
/// matches, and appending a block section after the flow node would produce a
/// second YAML document that fails reparse — so the upserts refuse it and the
/// callers leave the file untouched. No settings key can start with `{` or
/// `[` (a plain YAML scalar cannot begin with a flow indicator), so this
/// never misfires on a block mapping. Only the first structural line is
/// inspected: a document-start marker (`---`) preceding the flow node defeats
/// this check, and those shapes are caught instead by both callers' pre-write
/// reparse proofs (admin leaf read-back / identity reparse-as-mapping guard).
fn root_is_flow_collection(document: &str) -> bool {
    parse_lines(document)
        .iter()
        .find_map(|line| {
            line.indent
                .map(|indent| matches!(line.content.as_bytes().get(indent), Some(b'{' | b'[')))
        })
        .unwrap_or(false)
}

/// Whether the document's root node is a block sequence (the first structural
/// line is a `- item`): such a document carries no mapping the upserts could
/// extend — appending `key: value` lines after sequence items produces
/// unparseable YAML — so it is refused and the callers leave the file
/// untouched. (Both callers also pre-check that the parsed root is a mapping;
/// this keeps the editor safe on its own.) As with the flow-root check, only
/// the first structural line is inspected — a `---` document-start marker
/// defeats it, and the callers' pre-write reparse proofs catch those shapes.
fn root_is_block_sequence(document: &str) -> bool {
    parse_lines(document)
        .iter()
        .find_map(|line| {
            line.indent
                .map(|indent| is_sequence_item(&line.content[indent..]))
        })
        .unwrap_or(false)
}

/// Whether a trimmed line body is a block-sequence entry (`-` alone or `- item`).
fn is_sequence_item(trimmed: &str) -> bool {
    trimmed == "-" || trimmed.starts_with("- ") || trimmed.starts_with("-\t")
}

/// Whether an inline value token is a block-scalar header (`|` / `>` with
/// optional chomping/indentation indicators). A plain YAML scalar can never
/// begin with either indicator character, so on a parseable document this
/// never misfires on real data (quoted values begin with their quote).
fn is_block_scalar_header(value_text: &str) -> bool {
    matches!(value_text.as_bytes().first(), Some(b'|' | b'>'))
}

/// Classifies an inline value that makes its key un-extendable with block
/// children: a flow collection is real data that inserted block lines would
/// destroy, and a block scalar's header cannot be repaired into a mapping
/// opener without its continuation lines corrupting the document. Returns a
/// human-readable description, or `None` for a repairable stray scalar.
fn unextendable_inline_value_kind(value_text: &str) -> Option<&'static str> {
    match value_text.as_bytes().first() {
        Some(b'{' | b'[') => Some("an inline flow collection"),
        Some(b'|' | b'>') => Some("a block scalar"),
        _ => None,
    }
}

/// The newline to use for INSERTED lines: `\r\n` when the document already
/// uses CRLF anywhere, `\n` otherwise, so an upsert into a CRLF file does not
/// introduce mixed line endings. Rewritten lines always keep their own exact
/// terminator.
fn document_newline(document: &str) -> &'static str {
    if document.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Inserts `key: value` lines for `entries` at the end of the top-level
/// `section` block (creating the section at the end of the document when it
/// does not exist), preserving every existing byte apart from a repaired
/// scalar section value. A section holding an inline flow collection or a
/// block scalar, or whose children form a block sequence, is refused (see
/// [`upsert_section_values`]).
fn insert_into_section(
    document: &str,
    section: &str,
    entries: &[&(&str, serde_yaml::Value)],
) -> Result<String, String> {
    let newline = document_newline(document);
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
            output.push_str(newline);
        }
        output.push_str(section);
        output.push(':');
        output.push_str(newline);
        for (key, value) in entries {
            push_entry_line(&mut output, "  ", key, value, newline);
        }
        return Ok(output);
    };
    // A flow collection (`{…}` / `[…]`) on the header line is valid section
    // content, not a repairable stray scalar: dropping it to make room for
    // block children would silently destroy the user's data. A block scalar
    // header (`|` / `>`) cannot be repaired either — its continuation lines
    // would corrupt the mapping. Refuse both; the caller then leaves the
    // document untouched.
    if let Some(kind) =
        inline_section_value(lines[header].content).and_then(unextendable_inline_value_kind)
    {
        return Err(format!(
            "{section} holds {kind} and cannot accept inserted keys without destroying it"
        ));
    }

    // The section's children are the structurally deeper lines that follow the
    // header, up to the next top-level structural line. Insertion goes right
    // after the LAST child, so blank lines and comment banners belonging to the
    // following section stay attached to it. Child indentation follows the
    // existing children (default two spaces).
    let mut last_child = header;
    let mut child_indent: Option<&str> = None;
    let mut first_child_is_sequence_item = false;
    for (index, line) in lines.iter().enumerate().skip(header + 1) {
        match line.indent {
            Some(0) => break,
            Some(indent) => {
                if child_indent.is_none() {
                    first_child_is_sequence_item = is_sequence_item(&line.content[indent..]);
                }
                last_child = index;
                child_indent.get_or_insert(&line.content[..indent]);
            }
            None => {}
        }
    }
    // A block SEQUENCE under the header is real data mapping entries cannot
    // join: appending `key: value` lines after `- item` children writes
    // unparseable YAML, so refuse instead.
    if first_child_is_sequence_item {
        return Err(format!(
            "{section} holds a block sequence and cannot accept inserted mapping keys"
        ));
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
                output.push_str(newline);
            } else {
                output.push_str(line.terminator);
            }
            for (key, value) in entries {
                push_entry_line(&mut output, child_indent, key, value, newline);
            }
        } else {
            output.push_str(line.terminator);
        }
    }
    Ok(output)
}

/// Comment-preserving upsert of arbitrary-depth `a.b.c` dotted-path scalar
/// values, extending [`upsert_section_values`] (one section, flat leaves at
/// one level) to any nesting depth with the same semantics applied at every
/// level: existing leaves are rewritten in place (ASCII-case-insensitively,
/// via [`rewrite_inline_values`]), missing keys are inserted at the end of
/// their nearest existing ancestor mapping (creating intermediate openers,
/// two extra spaces per level under the existing child indentation), and a
/// missing top-level chain is appended at the end of the document. An
/// intermediate key holding a stray inline scalar is repaired into an opener
/// (its value dropped, matching the serde writer this replaces).
///
/// Updates are applied one path at a time in order, so later paths see the
/// insertions of earlier ones (two paths sharing a new parent end up as
/// siblings under one opener).
///
/// # Errors
///
/// Returns a description when a value cannot be rendered as a single-line
/// inline YAML scalar/flow sequence, when a path segment is not a plain-safe
/// YAML key, when the document root is a flow collection or block sequence,
/// when any key on the path holds an inline flow collection (`{…}` / `[…]` —
/// real data that block children would destroy) or a block scalar (`|` / `>`),
/// when a mapping on the path holds block-sequence children (`- item` lines a
/// mapping entry cannot join), or when the leaf exists as a nested block
/// mapping or block scalar (replacing block structure with an inline scalar is
/// refused, exactly like [`upsert_section_values`]).
pub(crate) fn upsert_dotted_values(
    document: &str,
    updates: &[(String, serde_yaml::Value)],
) -> Result<String, String> {
    let document = empty_flow_mapping_as_blank(document);
    if root_is_flow_collection(document) {
        return Err(
            "the document root is an inline flow collection and cannot hold block sections"
                .to_owned(),
        );
    }
    if root_is_block_sequence(document) {
        return Err(
            "the document root is a block sequence and cannot hold mapping sections".to_owned(),
        );
    }
    for (path, value) in updates {
        if render_value(value, ScalarStyle::Plain).is_none() {
            return Err(format!(
                "the value for {path} cannot be written as an inline YAML scalar"
            ));
        }
        if path.split('.').any(|segment| !is_plain_safe_key(segment)) {
            return Err(format!("{path} is not a plain-safe dotted settings path"));
        }
    }
    let mut current = document.to_owned();
    for (path, value) in updates {
        current = upsert_dotted_value(&current, path, value)?;
    }
    Ok(current)
}

/// Whether `segment` can be emitted verbatim as a plain YAML mapping key:
/// non-empty ASCII alphanumerics plus `_`/`-`. Everything the settings
/// surfaces produce satisfies this; anything else is refused rather than
/// risking a key that reparses as something other than itself.
fn is_plain_safe_key(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// One path of [`upsert_dotted_values`]: rewrite the existing leaf in place,
/// or insert it (with any missing intermediate openers) when absent.
fn upsert_dotted_value(
    document: &str,
    path: &str,
    value: &serde_yaml::Value,
) -> Result<String, String> {
    let lowercased = path.to_ascii_lowercase();
    let replacements = HashMap::from([(lowercased.clone(), value.clone())]);
    let rewrite = rewrite_inline_values(document, &replacements);
    if rewrite.blocked.contains(&lowercased) {
        return Err(format!(
            "{path} holds a nested block mapping or block scalar and cannot be rewritten as an \
             inline value"
        ));
    }
    if rewrite.matched.contains(&lowercased) {
        return Ok(rewrite.content);
    }
    // Nothing matched, so the rewrite pass changed nothing: insert into the
    // original document.
    insert_dotted_value(document, path, value)
}

/// Inserts a missing `a.b.c` leaf, creating any missing intermediate mapping
/// openers, preserving every existing byte apart from a repaired stray scalar
/// on the deepest matched opener. Matching at each level is
/// ASCII-case-insensitive against the existing spelling, exactly like
/// [`insert_into_section`] at the top level.
fn insert_dotted_value(
    document: &str,
    path: &str,
    value: &serde_yaml::Value,
) -> Result<String, String> {
    let newline = document_newline(document);
    let segments: Vec<&str> = path.split('.').collect();
    let lines = parse_lines(document);
    // The mapping currently being searched: its entry-line range. Starts as
    // the whole document (the root mapping).
    let mut start = 0_usize;
    let mut end = lines.len();
    // Index of the deepest matched opener line, and how many segments it
    // resolves.
    let mut opener: Option<usize> = None;
    let mut matched = 0_usize;

    for segment in &segments[..segments.len().saturating_sub(1)] {
        // Direct keys of this mapping sit at the indent of its first
        // structural line; deeper lines belong to nested openers.
        let Some(level_indent) = lines[start..end].iter().find_map(|line| line.indent) else {
            break;
        };
        let found = (start..end).find(|&index| {
            let line = &lines[index];
            line.indent == Some(level_indent)
                && line_key(line.content, level_indent)
                    .is_some_and(|key| key.eq_ignore_ascii_case(segment))
        });
        let Some(found) = found else {
            break;
        };
        if let Some(kind) =
            inline_section_value(lines[found].content).and_then(unextendable_inline_value_kind)
        {
            // Flow-collection data on the path would be silently destroyed by
            // inserted block children; a block scalar's continuation lines
            // would corrupt the mapping. Refuse both (the caller then leaves
            // the document untouched).
            return Err(format!(
                "{segment} on the path {path} holds {kind} and cannot accept inserted keys \
                 without destroying it"
            ));
        }
        // Children of the matched opener: the structurally deeper lines up to
        // the next line at or above its indent.
        end = (found + 1..end)
            .find(|&index| {
                lines[index]
                    .indent
                    .is_some_and(|indent| indent <= level_indent)
            })
            .unwrap_or(end);
        start = found + 1;
        // A block SEQUENCE under the matched opener is real data mapping
        // entries cannot join: appending `key: value` lines after `- item`
        // children writes unparseable YAML, so refuse instead.
        let first_child_sequence = lines[start..end].iter().find_map(|line| {
            line.indent
                .map(|indent| is_sequence_item(&line.content[indent..]))
        });
        if first_child_sequence == Some(true) {
            return Err(format!(
                "{segment} on the path {path} holds a block sequence and cannot accept inserted \
                 mapping keys"
            ));
        }
        opener = Some(found);
        matched += 1;
    }

    let Some(opener) = opener else {
        // No segment exists yet: append the whole chain at the end of the
        // document, two spaces per level.
        let mut output = document.to_owned();
        if !output.is_empty() && !output.ends_with('\n') {
            output.push_str(newline);
        }
        push_dotted_chain(&mut output, "", &segments, value, newline);
        return Ok(output);
    };

    // Insert right after the opener's LAST structural child (any depth), so
    // blank lines and comment banners belonging to what follows stay attached
    // to it; with no children, insert directly after the opener line. The
    // first new level follows the existing child indentation (default: the
    // opener's own indent plus two spaces).
    let insert_after = (start..end)
        .rev()
        .find(|&index| lines[index].indent.is_some())
        .unwrap_or(opener);
    let child_indent = lines[start..end]
        .iter()
        .find_map(|line| line.indent.map(|indent| &line.content[..indent]))
        .map_or_else(
            || {
                let opener_line = lines[opener].content;
                let opener_indent = lines[opener].indent.unwrap_or(0);
                format!("{}  ", &opener_line[..opener_indent])
            },
            ToOwned::to_owned,
        );
    // A stray scalar on the deepest matched opener is repaired into a mapping
    // opener; a flow collection was already refused above.
    let repair = inline_section_value(lines[opener].content).is_some();

    let mut output = String::with_capacity(document.len() + (segments.len() - matched) * 48);
    for (index, line) in lines.iter().enumerate() {
        if index == opener && repair {
            output.push_str(&repaired_section_opener(line.content));
        } else {
            output.push_str(line.content);
        }
        if index == insert_after {
            if line.terminator.is_empty() {
                output.push_str(newline);
            } else {
                output.push_str(line.terminator);
            }
            push_dotted_chain(
                &mut output,
                &child_indent,
                &segments[matched..],
                value,
                newline,
            );
        } else {
            output.push_str(line.terminator);
        }
    }
    Ok(output)
}

/// Appends the remaining `segments` of a dotted path as nested opener lines
/// (`key:`) plus the final `leaf: value` line, indenting two extra spaces per
/// level under `base_indent`, terminating each inserted line with the
/// document's own `newline`.
fn push_dotted_chain(
    output: &mut String,
    base_indent: &str,
    segments: &[&str],
    value: &serde_yaml::Value,
    newline: &str,
) {
    for (depth, segment) in segments.iter().enumerate() {
        let indent = format!("{base_indent}{}", "  ".repeat(depth));
        if depth == segments.len() - 1 {
            push_entry_line(output, &indent, segment, value, newline);
        } else {
            output.push_str(&indent);
            output.push_str(segment);
            output.push(':');
            output.push_str(newline);
        }
    }
}

/// The key text of a `key: …` line at a known indent, or `None` for lines
/// without a colon (block-sequence items and other non-entry lines).
fn line_key(content: &str, indent: usize) -> Option<&str> {
    let trimmed = &content[indent..];
    let colon = trimmed.find(':')?;
    Some(&trimmed[..colon])
}

/// The inline value text on a `section: value` header line, or `None` when the
/// header opens a mapping (nothing but whitespace/a comment after the colon).
fn inline_section_value(content: &str) -> Option<&str> {
    let colon = content.find(':')?;
    let after_colon = &content[colon + 1..];
    let (value_start, value_end) = value_span(after_colon)?;
    Some(&after_colon[value_start..value_end])
}

/// Drops a stray scalar inline value from a `section: value` header line so
/// the inserted children form a mapping, keeping any inline comment. A header
/// that already opens a mapping is returned unchanged. Flow collections never
/// reach this repair — [`insert_into_section`] refuses them first.
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

fn push_entry_line(
    output: &mut String,
    indent: &str,
    key: &str,
    value: &serde_yaml::Value,
    newline: &str,
) {
    output.push_str(indent);
    output.push_str(key);
    output.push_str(": ");
    // The caller (`upsert_section_values`) pre-validated renderability.
    output.push_str(&render_value(value, ScalarStyle::Plain).unwrap_or_default());
    output.push_str(newline);
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

    use super::{rewrite_inline_values, upsert_dotted_values, upsert_section_values};

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

    /// Regression: a section holding an inline FLOW mapping (`a: {b: 1}`)
    /// used to be treated as a repairable stray scalar — the whole `{b: 1}`
    /// value was dropped before inserting the new key, silently losing `b`.
    /// Such a section (and its flow-sequence sibling) must be refused instead.
    #[test]
    fn upsert_refuses_to_destroy_a_flow_collection_section() {
        for document in ["a: {b: 1}\n", "a: [1, 2] # keep\n"] {
            let result = upsert_section_values(document, "a", &[("key", string_value("v"))]);
            assert!(result.is_err(), "must refuse {document:?}");
        }
    }

    /// Regression: a document whose ROOT is a flow collection (`{a: 1}` /
    /// `[1, 2]`) parses as a mapping/sequence, but the section-header scan
    /// never matches it, so the missing-header branch used to APPEND a block
    /// section — producing a two-document YAML that fails reparse. The upsert
    /// must refuse instead so callers leave the file untouched.
    #[test]
    fn upsert_refuses_a_flow_collection_document_root() {
        for document in [
            "{a: 1}\n",
            "{a: 1}",
            "[1, 2]\n",
            "# leading comment\n\n{a: 1}\n",
        ] {
            let result = upsert_section_values(document, "section", &[("key", string_value("v"))]);
            assert!(result.is_err(), "must refuse {document:?}");
            let nested =
                upsert_dotted_values(document, &[("section.key".to_owned(), string_value("v"))]);
            assert!(nested.is_err(), "nested upsert must refuse {document:?}");
        }
        // A blank/comment-only document is NOT a flow root: it is created as
        // a fresh block mapping like before.
        for document in ["", "# just a comment\n"] {
            assert!(
                upsert_section_values(document, "section", &[("key", string_value("v"))]).is_ok(),
                "must still accept {document:?}"
            );
        }
    }

    /// The serde-based writers this machinery replaced persisted "no
    /// settings" as `{}`; such a file holds no data, so both upserts treat it
    /// as a blank document instead of refusing it as a flow root.
    #[test]
    fn upsert_treats_an_empty_flow_mapping_document_as_blank()
    -> Result<(), Box<dyn std::error::Error>> {
        let updated = upsert_section_values("{}\n", "section", &[("key", string_value("v"))])?;
        assert_eq!(updated, "section:\n  key: v\n");
        let nested =
            upsert_dotted_values("{}\n", &[("section.key".to_owned(), string_value("v"))])?;
        assert_eq!(nested, "section:\n  key: v\n");
        Ok(())
    }

    /// Regression: a leaf holding a block scalar (`key: |` + continuation
    /// lines) used to have only its `|` indicator replaced — the continuation
    /// lines then folded into the new value as a plain multiline scalar,
    /// producing VALID YAML that silently carried the wrong value. The leaf
    /// must be blocked instead, and the scalar's content lines must never be
    /// mistaken for mapping structure.
    #[test]
    fn rewrite_blocks_block_scalar_leaves_and_skips_their_content() {
        let document = "wrapper:\n  block: |\n    inner: sneaky\n  inner: real-old\n";
        let sneaky = rewrite_inline_values(
            document,
            &HashMap::from([("wrapper.inner".to_owned(), string_value("new"))]),
        );
        // Only the REAL wrapper.inner leaf is rewritten; the identical-looking
        // line inside the block scalar is literal content and stays verbatim.
        assert_eq!(
            sneaky.content,
            "wrapper:\n  block: |\n    inner: sneaky\n  inner: new\n"
        );
        assert!(sneaky.matched.contains("wrapper.inner"));

        let blocked = rewrite_inline_values(
            document,
            &HashMap::from([("wrapper.block".to_owned(), string_value("new"))]),
        );
        assert_eq!(blocked.content, document, "the bytes must stand untouched");
        assert!(blocked.blocked.contains("wrapper.block"));
        assert!(!blocked.matched.contains("wrapper.block"));
    }

    /// Regression (tester probe): updating a block-scalar leaf through the
    /// upserts used to report success while persisting a folded wrong value
    /// (`New abc def`). Every block-scalar shape — literal/folded, chomping
    /// indicators, leaf or intermediate key, section header — must refuse.
    #[test]
    fn upsert_refuses_block_scalar_leaves_and_paths() {
        for document in [
            "ui:\n  appName: |\n    abc\n    def\n",
            "ui:\n  appName: >-\n    abc\n    def\n",
            "ui:\n  appName: |2\n    abc\n",
        ] {
            let result =
                upsert_dotted_values(document, &[("ui.appName".to_owned(), string_value("New"))]);
            assert!(result.is_err(), "must refuse the leaf in {document:?}");
        }
        // A block scalar on an INTERMEDIATE path key: repairing it into an
        // opener would leave its continuation lines corrupting the mapping.
        let intermediate = upsert_dotted_values(
            "a:\n  b: |\n    text\n",
            &[("a.b.c".to_owned(), string_value("v"))],
        );
        assert!(intermediate.is_err());
        // A top-level section header holding a block scalar refuses the same
        // way through the flat section upsert.
        let section = upsert_section_values("a: |\n  text\n", "a", &[("key", string_value("v"))]);
        assert!(section.is_err());
        let leaf_in_section = upsert_section_values(
            "AutomaticallyGenerated:\n  UUID: |\n    junk\n",
            "AutomaticallyGenerated",
            &[("UUID", string_value("123e4567-e89b-12d3-a456-426614174000"))],
        );
        assert!(leaf_in_section.is_err());
    }

    /// Regression: a section (or any mapping on the path) holding a block
    /// SEQUENCE used to have `key: value` lines appended after its `- item`
    /// children, writing unparseable YAML. Sequence-valued mappings and
    /// sequence roots must refuse instead.
    #[test]
    fn upsert_refuses_block_sequence_sections_and_roots() {
        let section = "automaticallyGenerated:\n  - 1\n  - 2\n";
        assert!(
            upsert_section_values(
                section,
                "AutomaticallyGenerated",
                &[("UUID", string_value("v"))]
            )
            .is_err()
        );
        assert!(
            upsert_dotted_values(
                section,
                &[("automaticallyGenerated.UUID".to_owned(), string_value("v"))]
            )
            .is_err()
        );
        // A deeper sequence-valued mapping on a dotted path.
        assert!(
            upsert_dotted_values(
                "a:\n  b:\n    - 1\n",
                &[("a.b.c".to_owned(), string_value("v"))]
            )
            .is_err()
        );
        // A block-sequence ROOT has no mapping to extend at all.
        for document in ["- not\n- a\n- mapping\n", "- 1"] {
            assert!(
                upsert_section_values(document, "section", &[("key", string_value("v"))]).is_err(),
                "must refuse root {document:?}"
            );
            assert!(
                upsert_dotted_values(document, &[("a.b".to_owned(), string_value("v"))]).is_err(),
                "must refuse root {document:?}"
            );
        }
    }

    /// Inserted lines follow the document's own line endings: an upsert into
    /// a CRLF file must not introduce mixed terminators.
    #[test]
    fn upsert_inserts_crlf_lines_into_crlf_documents() -> Result<(), Box<dyn std::error::Error>> {
        let inserted =
            upsert_dotted_values("a:\r\n  b: 1\r\n", &[("a.c".to_owned(), string_value("v"))])?;
        assert_eq!(inserted, "a:\r\n  b: 1\r\n  c: v\r\n");

        let new_section =
            upsert_section_values("x: 1\r\n", "section", &[("key", string_value("v"))])?;
        assert_eq!(new_section, "x: 1\r\nsection:\r\n  key: v\r\n");

        let new_chain = upsert_dotted_values("x: 1\r\n", &[("a.b".to_owned(), string_value("v"))])?;
        assert_eq!(new_chain, "x: 1\r\na:\r\n  b: v\r\n");
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
        assert!(upsert_section_values("", "section", &[("key", nested.clone())]).is_err());
        assert!(
            upsert_section_values("", "section", &[("key", string_value("two\nlines"))]).is_err()
        );
        assert!(upsert_dotted_values("", &[("a.b".to_owned(), nested)]).is_err());
        assert!(
            upsert_dotted_values("", &[("a.b".to_owned(), string_value("two\nlines"))]).is_err()
        );
    }

    #[test]
    fn nested_upsert_rewrites_and_inserts_deep_paths_preserving_comments()
    -> Result<(), Box<dyn std::error::Error>> {
        let document = "# banner\nsecurity:\n  oauth2: # opener\n    clientSecret: old # keep\n  \
saml2:\n    enabled: false\n\n# tail banner\nui:\n  appName: Old\n";
        let updated = upsert_dotted_values(
            document,
            &[
                (
                    "security.oauth2.clientSecret".to_owned(),
                    string_value("new"),
                ),
                (
                    "security.oauth2.client.scope".to_owned(),
                    string_value("openid"),
                ),
                ("ui.appName".to_owned(), string_value("New")),
            ],
        )?;
        // The existing leaf is rewritten in place, the missing `client.scope`
        // chain is inserted at the end of oauth2's children, and every
        // comment byte (banner, opener, inline, tail) is preserved.
        assert_eq!(
            updated,
            "# banner\nsecurity:\n  oauth2: # opener\n    clientSecret: new # keep\n    client:\n      \
scope: openid\n  saml2:\n    enabled: false\n\n# tail banner\nui:\n  appName: New\n"
        );
        Ok(())
    }

    #[test]
    fn nested_upsert_reuses_relaxed_spellings_at_every_level()
    -> Result<(), Box<dyn std::error::Error>> {
        let document = "security:\n  OAuth2:\n    clientsecret: old\n";
        let updated = upsert_dotted_values(
            document,
            &[(
                "security.oauth2.clientSecret".to_owned(),
                string_value("new"),
            )],
        )?;
        assert_eq!(updated, "security:\n  OAuth2:\n    clientsecret: new\n");
        Ok(())
    }

    #[test]
    fn nested_upsert_inserts_after_deeper_sibling_blocks() -> Result<(), Box<dyn std::error::Error>>
    {
        let document = "a:\n  b:\n    c: 1\n";
        let updated = upsert_dotted_values(document, &[("a.d".to_owned(), string_value("v"))])?;
        assert_eq!(updated, "a:\n  b:\n    c: 1\n  d: v\n");
        Ok(())
    }

    #[test]
    fn nested_upsert_repairs_a_stray_scalar_on_the_path() -> Result<(), Box<dyn std::error::Error>>
    {
        // `b` holds a stray scalar: repaired into an opener (value dropped,
        // matching the serde writer this replaces), then the chain descends.
        let document = "a:\n  b: 5 # note\n";
        let updated = upsert_dotted_values(document, &[("a.b.c".to_owned(), string_value("v"))])?;
        assert_eq!(updated, "a:\n  b: # note\n    c: v\n");
        let parsed: serde_yaml::Value = serde_yaml::from_str(&updated)?;
        assert_eq!(parsed["a"]["b"]["c"], string_value("v"));
        Ok(())
    }

    #[test]
    fn nested_upsert_refuses_flow_collections_anywhere_on_the_path() {
        // A flow mapping on the path is data that block children would
        // destroy; a nested block mapping cannot become a scalar leaf.
        for (document, path) in [
            ("a:\n  b: {x: 1}\n", "a.b.c"),
            ("a:\n  b: [1, 2]\n", "a.b.c"),
            ("a:\n  b:\n    c: 1\n", "a.b"),
        ] {
            let result = upsert_dotted_values(document, &[(path.to_owned(), string_value("v"))]);
            assert!(result.is_err(), "must refuse {path} in {document:?}");
        }
    }

    #[test]
    fn nested_upsert_creates_missing_chains_and_sibling_leaves()
    -> Result<(), Box<dyn std::error::Error>> {
        let created = upsert_dotted_values(
            "",
            &[
                ("a.b".to_owned(), serde_yaml::Value::Number(1.into())),
                ("a.c.d".to_owned(), serde_yaml::Value::Number(2.into())),
            ],
        )?;
        // The second path finds the chain the first one inserted and joins it
        // as a sibling instead of duplicating `a`.
        assert_eq!(created, "a:\n  b: 1\n  c:\n    d: 2\n");

        // Appending after a document without a final newline stays valid.
        let appended = upsert_dotted_values("x: 1", &[("a.b".to_owned(), string_value("v"))])?;
        assert_eq!(appended, "x: 1\na:\n  b: v\n");
        Ok(())
    }

    #[test]
    fn nested_upsert_rejects_unsafe_path_segments() {
        for path in ["a..b", ".a", "a.", "a.b c", "a.{b}", "a.b:c"] {
            let result = upsert_dotted_values("", &[(path.to_owned(), string_value("v"))]);
            assert!(result.is_err(), "must refuse path {path:?}");
        }
    }
}
