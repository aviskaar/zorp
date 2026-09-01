//! Finding the JSON a model meant as its answer.
//!
//! Both things `investigate` reads back from a model, the forecast and
//! the attempt result, are one JSON object at the end of an answer, and
//! both used to be thrown away over punctuation. These are the
//! extractors that stopped that happening, written for the forecast
//! first (`docs/DECISIONS.md`, 2026-08-23) and shared with the result
//! parser after the same failure was measured on it
//! (`docs/DECISIONS.md`, 2026-09-01). `validate` reads one object
//! back the same way and had the same defect, so it shares them too.
//!
//! They find candidates and judge none of them. Every span they return
//! still faces the caller's own shape check and then every coherence
//! check the caller has, so widening where an object may be found does
//! not widen what counts as one. An answer that cannot be read must
//! never become one that was invented.

/// Every fenced block body in `text`, in order.
///
/// A fence left open at end of input still yields its body. The model is
/// asked for its object last, so a truncated answer loses its closing
/// fence and nothing else, and dropping a body that parses because three
/// backticks never arrived is throwing away the answer over punctuation.
/// That was 8 of the 25 discarded attempts in the registry run, all of
/// them reported as "no fenced json block in the forecast".
pub(crate) fn fenced_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut inside: Option<Vec<&str>> = None;
    for line in text.lines() {
        let fence = line.trim_start().starts_with("```");
        match (&mut inside, fence) {
            (None, true) => inside = Some(Vec::new()),
            (Some(body), true) => {
                blocks.push(body.join("\n"));
                inside = None;
            }
            (Some(body), false) => body.push(line),
            (None, false) => {}
        }
    }
    if let Some(body) = inside {
        blocks.push(body.join("\n"));
    }
    blocks
}

/// Every balanced `{...}` span in `text`, in order.
///
/// The fallback for a model that answers with the right object and no
/// backticks. Balanced rather than regular: the objects read here are
/// flat today, but a scan that stops at the first `}` would silently
/// truncate the moment one nests, and a truncated object parses as
/// nothing rather than as something wrong. Quotes and escapes are
/// tracked so a brace inside a string cannot open or close a span.
pub(crate) fn bare_objects(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = balanced_end(bytes, i) {
                spans.push(text[i..end].to_string());
                i = end;
                continue;
            }
        }
        i += 1;
    }
    spans
}

/// The index just past the `}` closing the object that opens at `start`,
/// or `None` when it never closes.
fn balanced_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, byte) in bytes.iter().enumerate().skip(start) {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unclosed_final_fence_still_yields_its_body() {
        let blocks = fenced_blocks("here you go\n```json\n{\"a\": 1}");
        assert_eq!(blocks, vec!["{\"a\": 1}"]);
    }

    #[test]
    fn a_brace_inside_a_string_neither_opens_nor_closes_a_span() {
        let spans = bare_objects(r#"{"summary": "a } and a {", "metric_value": 1}"#);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].ends_with("\"metric_value\": 1}"));
    }

    #[test]
    fn a_nested_object_is_not_truncated_at_its_first_brace() {
        let spans = bare_objects(r#"{"outer": {"inner": 1}, "metric_value": 2}"#);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].ends_with("\"metric_value\": 2}"));
    }

    #[test]
    fn an_object_that_never_closes_yields_nothing() {
        assert!(bare_objects("{\"metric_value\": 1").is_empty());
    }
}
