//! A deliberately small YAML subset parser for `SKILL.md` frontmatter.
//!
//! Why not a YAML crate. Frontmatter here is a handful of scalar fields, and
//! the only ones anything reads are `name`, `description`, and
//! `allowed-tools`. A full YAML parser would bring anchors, aliases, merge
//! keys, and tags along with it, which is a much larger attack surface aimed
//! at a file that arrives from anywhere, in exchange for features no skill
//! uses. This handles what real skills actually contain: plain scalars,
//! quoted scalars, folded and literal block scalars, nested maps, comments,
//! and CRLF. Anything else is reported as a parse error and the skill is
//! skipped, which is the behavior that matters: the parser fails loudly on
//! input it does not understand instead of guessing.
//!
//! What it deliberately does not do: anchors and aliases, multi-document
//! streams, flow mappings (`{a: b}`), typed scalars (everything is a string),
//! and full escape handling inside double quotes beyond `\"` and `\\`.

use std::collections::BTreeMap;

/// Line that opens frontmatter, and one of the two that can close it.
const DELIMITER: &str = "---";
/// YAML's other document-end marker, accepted as a closing delimiter.
const END_DELIMITER: &str = "...";

/// Split frontmatter from body and read the frontmatter's top level keys.
///
/// Values are returned as strings. A key whose value is a nested map is
/// recorded with an empty value and its children are dropped, so a nested
/// `description` can never shadow the real one.
pub fn parse(text: &str) -> Result<(BTreeMap<String, String>, String), String> {
    let (frontmatter, body) = split(text)?;
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let lines: Vec<&str> = frontmatter.lines().collect();
    let mut index = 0;
    let mut last_key: Option<String> = None;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            index += 1;
            continue;
        }
        if line.starts_with([' ', '\t']) {
            // A plain scalar continued on the next line. Anything indented
            // under a key with no scalar value was already consumed below, so
            // reaching here with no last key means a stray indent worth
            // ignoring rather than failing a whole skill over.
            if let Some(key) = last_key.as_ref() {
                if let Some(value) = fields.get_mut(key) {
                    if !value.is_empty() {
                        value.push(' ');
                        value.push_str(trimmed);
                    }
                }
            }
            index += 1;
            continue;
        }
        let (key, value) = trimmed
            .split_once(':')
            .ok_or_else(|| format!("malformed frontmatter line: {trimmed}"))?;
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(format!("malformed frontmatter line: {trimmed}"));
        }
        let value = value.trim();
        let literal = value.starts_with('|');
        let folded = value.starts_with('>');
        if value.is_empty() || literal || folded {
            index += 1;
            let mut block = Vec::new();
            while index < lines.len() {
                let next = lines[index];
                if next.trim().is_empty() {
                    block.push(String::new());
                    index += 1;
                    continue;
                }
                if !next.starts_with([' ', '\t']) {
                    break;
                }
                block.push(next.trim().to_string());
                index += 1;
            }
            // A nested map has no scalar value of its own. Its children are
            // its own business, never this document's top level keys. A key
            // with a bare `:` followed by prose is the other thing that shape
            // can mean, and real skills write long descriptions that way.
            let joined = if literal {
                block.join("\n").trim().to_string()
            } else if folded || !looks_like_a_map(&block) {
                block
                    .iter()
                    .filter(|l| !l.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                String::new()
            };
            fields.insert(key.clone(), unquote(&joined));
            last_key = Some(key);
            continue;
        }
        fields.insert(key.clone(), unquote(value));
        last_key = Some(key);
        index += 1;
    }
    Ok((fields, body))
}

/// Whether an indented block under a valueless key is a nested mapping
/// rather than a scalar written on the following lines. Every non-empty line
/// starting with `word:` means a mapping; one line of prose is enough to say
/// it is not. An empty block is treated as a mapping, which keeps a bare
/// `metadata:` from inventing a value.
fn looks_like_a_map(block: &[String]) -> bool {
    for line in block.iter().filter(|l| !l.is_empty()) {
        let Some((key, _)) = line.split_once(':') else {
            return false;
        };
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            return false;
        }
    }
    true
}

/// Return the frontmatter text and the body, or say why the document has no
/// usable frontmatter. Line based on purpose: a delimiter is a whole line, so
/// a body line that merely starts with dashes cannot end the frontmatter.
fn split(text: &str) -> Result<(String, String), String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut offset = 0usize;
    let mut opened = false;
    let mut start = 0usize;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches('\n').trim_end_matches('\r');
        if !opened {
            if content.trim_end() != DELIMITER {
                return Err("missing frontmatter delimiter".to_string());
            }
            opened = true;
            offset += line.len();
            start = offset;
            continue;
        }
        let marker = content.trim_end();
        if marker == DELIMITER || marker == END_DELIMITER {
            let frontmatter = text[start..offset].to_string();
            let body = text[offset + line.len()..].to_string();
            return Ok((frontmatter, body));
        }
        offset += line.len();
    }
    if opened {
        Err("missing closing frontmatter delimiter".to_string())
    } else {
        Err("missing frontmatter delimiter".to_string())
    }
}

/// Strip one layer of matching quotes, undoing the two escapes that actually
/// show up inside double quoted descriptions.
fn unquote(value: &str) -> String {
    let value = value.trim();
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if first == last && (first == b'"' || first == b'\'') {
            let inner = &value[1..value.len() - 1];
            if first == b'"' {
                return inner.replace("\\\"", "\"").replace("\\\\", "\\");
            }
            return inner.replace("''", "'");
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(text: &str) -> BTreeMap<String, String> {
        parse(text).unwrap().0
    }

    #[test]
    fn reads_flat_scalars_and_returns_the_body() {
        let (f, body) =
            parse("---\nname: demo\ndescription: does a thing\n---\nthe body\n").unwrap();
        assert_eq!(f.get("name").unwrap(), "demo");
        assert_eq!(f.get("description").unwrap(), "does a thing");
        assert_eq!(body, "the body\n");
    }

    #[test]
    fn a_colon_inside_a_value_stays_in_the_value() {
        let f = fields("---\ndescription: Use for X: it does Y\n---\nbody");
        assert_eq!(f.get("description").unwrap(), "Use for X: it does Y");
    }

    #[test]
    fn double_quoted_values_are_unquoted() {
        let f = fields("---\ndescription: \"quoted, with a comma\"\n---\nbody");
        assert_eq!(f.get("description").unwrap(), "quoted, with a comma");
    }

    #[test]
    fn single_quoted_values_are_unquoted() {
        let f = fields("---\ndescription: 'quoted'\n---\nbody");
        assert_eq!(f.get("description").unwrap(), "quoted");
    }

    #[test]
    fn an_escaped_quote_inside_a_double_quoted_value_survives() {
        let f = fields("---\ndescription: \"say \\\"hi\\\" now\"\n---\nbody");
        assert_eq!(f.get("description").unwrap(), "say \"hi\" now");
    }

    #[test]
    fn a_folded_block_scalar_joins_its_lines_with_spaces() {
        let f = fields("---\ndescription: >-\n  first line\n  second line\nname: demo\n---\nbody");
        assert_eq!(f.get("description").unwrap(), "first line second line");
        assert_eq!(f.get("name").unwrap(), "demo");
    }

    #[test]
    fn a_literal_block_scalar_keeps_its_newlines() {
        let f = fields("---\ndescription: |\n  first line\n  second line\n---\nbody");
        assert_eq!(f.get("description").unwrap(), "first line\nsecond line");
    }

    /// The case that decides whether a hand written parser is safe at all: a
    /// nested map's children must never be mistaken for top level keys, or a
    /// `metadata:` block could overwrite the description the model reads.
    #[test]
    fn nested_map_children_do_not_become_top_level_keys() {
        let f = fields(
            "---\nname: demo\ndescription: real one\nmetadata:\n  description: nested one\n  \
             version: \"1.0\"\n---\nbody",
        );
        assert_eq!(f.get("description").unwrap(), "real one");
        assert_eq!(f.get("metadata").unwrap(), "");
        assert!(!f.contains_key("version"));
    }

    /// Real skills write a long description as a quoted scalar starting on
    /// the line below the key. That is not a nested map and must not be read
    /// as one, or the description disappears and the skill is dropped.
    #[test]
    fn a_scalar_indented_under_its_key_is_read_as_the_value() {
        let f = fields(
            "---\nname: demo\ndescription:\n  \"first line\n  second line\"\nlicense: MIT\n---\nbody",
        );
        assert_eq!(f.get("description").unwrap(), "first line second line");
        assert_eq!(f.get("license").unwrap(), "MIT");
    }

    #[test]
    fn an_unquoted_scalar_indented_under_its_key_is_read_as_the_value() {
        let f = fields("---\ndescription:\n  Use this for things.\n---\nbody");
        assert_eq!(f.get("description").unwrap(), "Use this for things.");
    }

    #[test]
    fn an_indented_continuation_of_a_plain_scalar_is_folded_in() {
        let f = fields("---\ndescription: first part\n  second part\nname: demo\n---\nbody");
        assert_eq!(f.get("description").unwrap(), "first part second part");
        assert_eq!(f.get("name").unwrap(), "demo");
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let f = fields("---\n# a comment\n\nname: demo\n---\nbody");
        assert_eq!(f.get("name").unwrap(), "demo");
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_delimiter() {
        let f = fields("\u{feff}---\nname: demo\n---\nbody");
        assert_eq!(f.get("name").unwrap(), "demo");
    }

    #[test]
    fn carriage_returns_do_not_hide_the_delimiter() {
        let f = fields("---\r\nname: demo\r\n---\r\nbody");
        assert_eq!(f.get("name").unwrap(), "demo");
    }

    #[test]
    fn a_body_line_of_dashes_is_not_the_closing_delimiter() {
        let (f, body) = parse("---\nname: demo\n---\nintro\n----\nmore\n").unwrap();
        assert_eq!(f.get("name").unwrap(), "demo");
        assert!(body.contains("----"));
    }

    #[test]
    fn a_dot_delimiter_also_closes_the_frontmatter() {
        let (f, body) = parse("---\nname: demo\n...\nbody\n").unwrap();
        assert_eq!(f.get("name").unwrap(), "demo");
        assert_eq!(body, "body\n");
    }

    #[test]
    fn a_missing_opening_delimiter_is_a_clear_error() {
        let err = parse("name: demo\nbody").unwrap_err();
        assert!(err.contains("frontmatter delimiter"), "{err}");
    }

    #[test]
    fn a_missing_closing_delimiter_is_a_clear_error() {
        let err = parse("---\nname: demo\nno closer here").unwrap_err();
        assert!(err.contains("closing frontmatter delimiter"), "{err}");
    }

    #[test]
    fn a_top_level_line_without_a_colon_is_a_clear_error() {
        let err = parse("---\nname demo\n---\nbody").unwrap_err();
        assert!(err.contains("malformed frontmatter line"), "{err}");
    }

    #[test]
    fn empty_frontmatter_parses_to_no_fields() {
        let (f, body) = parse("---\n---\nbody").unwrap();
        assert!(f.is_empty());
        assert_eq!(body, "body");
    }

    /// Nothing in here may panic: the input is a file zorp did not write.
    #[test]
    fn hostile_shapes_return_errors_rather_than_panicking() {
        for text in [
            "",
            "-",
            "--",
            "---",
            "---\n",
            "---\n---",
            "---\n:\n---\nbody",
            "---\n   \n---\nbody",
            "---\ndescription: >-\n---\nbody",
            "---\n\u{feff}\n---\nbody",
        ] {
            let _ = parse(text);
        }
    }
}
