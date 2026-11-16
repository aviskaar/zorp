use crate::tools::{cap_output, Context, Tool, ToolError, ToolOutput, ToolResult};
use serde_json::{json, Value};
use std::collections::HashMap;

/// One search/replace block targeting a file.
pub(crate) struct PatchBlock {
    pub path: String,
    pub search: String,
    pub replace: String,
}

/// Parse zero or more search/replace blocks from patch text.
pub(crate) fn parse_patch(text: &str) -> Vec<PatchBlock> {
    let mut blocks = Vec::new();
    let mut lines = text.lines();
    let mut pending = lines.next();

    while let Some(line) = pending {
        pending = lines.next();
        let path = match line.strip_prefix("------ ") {
            Some(path) => path.trim().to_string(),
            None => continue,
        };

        if pending.map(|next| next.trim_end()) != Some("<<<<<<< SEARCH") {
            continue;
        }
        pending = lines.next();

        let mut search = Vec::new();
        let mut saw_divider = false;
        while let Some(current) = pending {
            pending = lines.next();
            if current.trim_end() == "=======" {
                saw_divider = true;
                break;
            }
            search.push(current);
        }
        if !saw_divider {
            break;
        }

        let mut replace = Vec::new();
        let mut saw_end = false;
        while let Some(current) = pending {
            pending = lines.next();
            if current.trim_end() == ">>>>>>> REPLACE" {
                saw_end = true;
                break;
            }
            replace.push(current);
        }
        if !saw_end {
            break;
        }

        blocks.push(PatchBlock {
            path,
            search: search.join("\n"),
            replace: replace.join("\n"),
        });
    }

    blocks
}

/// Reasons an exact search/replace apply can fail.
#[derive(Debug)]
pub(crate) enum ApplyErr {
    NotFound,
    Ambiguous(usize),
}

/// Replace the unique exact occurrence of `search` with `replace`, or, when
/// there is none, the unique run of whole lines that matches it once
/// trailing spaces and typographic quotes and dashes are set aside.
pub(crate) fn apply_to_text(
    content: &str,
    search: &str,
    replace: &str,
) -> Result<String, ApplyErr> {
    match content.matches(search).count() {
        0 => apply_loosely(content, search, replace),
        1 => Ok(content.replacen(search, replace, 1)),
        count => Err(ApplyErr::Ambiguous(count)),
    }
}

/// A line as a model is likely to have retyped it: no trailing whitespace,
/// curly quotes and Unicode dashes as their ASCII forms. Leading whitespace
/// is kept, because indentation is meaning in Python and YAML.
fn loose(line: &str) -> String {
    line.trim_end()
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            '\u{00A0}' => ' ',
            c => c,
        })
        .collect()
}

/// The fallback: compare line by line in the loose form and replace the
/// original lines that matched, so nothing outside them is rewritten. Small
/// local models drop a trailing space or straighten a quote often enough
/// that an exact-only match cost them a retry on most edits.
fn apply_loosely(content: &str, search: &str, replace: &str) -> Result<String, ApplyErr> {
    let wanted: Vec<String> = search.lines().map(loose).collect();
    if wanted.iter().all(|l| l.is_empty()) {
        return Err(ApplyErr::NotFound);
    }
    // Byte span of every line, line ending included.
    let mut spans = Vec::new();
    let mut at = 0;
    for piece in content.split_inclusive('\n') {
        spans.push((at, at + piece.len()));
        at += piece.len();
    }
    let have: Vec<String> = spans
        .iter()
        .map(|&(a, b)| loose(content[a..b].trim_end_matches(['\n', '\r'])))
        .collect();
    let hits: Vec<usize> = (0..have.len().saturating_sub(wanted.len() - 1))
        .filter(|&i| have[i..i + wanted.len()] == wanted[..])
        .collect();
    match hits.as_slice() {
        [] => Err(ApplyErr::NotFound),
        [i] => {
            let start = spans[*i].0;
            let last = spans[i + wanted.len() - 1];
            let line = &content[last.0..last.1];
            let ending = &line[line.trim_end_matches(['\n', '\r']).len()..];
            let mut out = String::with_capacity(content.len() + replace.len());
            out.push_str(&content[..start]);
            out.push_str(replace.trim_end_matches(['\n', '\r']));
            out.push_str(ending);
            out.push_str(&content[last.1..]);
            Ok(out)
        }
        many => Err(ApplyErr::Ambiguous(many.len())),
    }
}

/// Count approximate added/removed lines for summaries.
pub(crate) fn line_delta(before: &str, after: &str) -> (usize, usize) {
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for line in before.lines() {
        *counts.entry(line).or_default() -= 1;
    }
    for line in after.lines() {
        *counts.entry(line).or_default() += 1;
    }

    let mut added = 0usize;
    let mut removed = 0usize;
    for count in counts.values() {
        if *count > 0 {
            added += *count as usize;
        } else if *count < 0 {
            removed += (-*count) as usize;
        }
    }
    (added, removed)
}

/// Edit files using exact search/replace blocks applied in order.
pub struct ApplyPatch;

impl Tool for ApplyPatch {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn guideline(&self) -> Option<&str> {
        Some("Change part of a file with apply_patch; use write_file only for a new file or a full rewrite.")
    }

    fn description(&self) -> &str {
        "Edit files using search/replace blocks; several blocks, for one file or many, can go in one call. Format per block: a line '------ <path>', then '<<<<<<< SEARCH', the exact text to find, '=======', the replacement, '>>>>>>> REPLACE'. The SEARCH text must match uniquely; whole lines that differ only in trailing spaces or curly quotes and dashes still match. Keep SEARCH short. An empty SEARCH creates or overwrites the file."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "patch": {"type":"string","description":"one or more search/replace blocks"}
            },
            "required": ["patch"]
        })
    }

    fn run(&self, args: &Value, cx: &mut Context) -> ToolResult {
        let patch = args
            .get("patch")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolError::new("apply_patch: 'patch' is required"))?;
        let blocks = parse_patch(patch);
        if blocks.is_empty() {
            return Err(ToolError::new(
                "no valid search/replace blocks found. Each block is: '------ <path>', '<<<<<<< SEARCH', text, '=======', text, '>>>>>>> REPLACE'.",
            ));
        }

        let mut applied = 0usize;
        let mut lines = Vec::new();
        for block in &blocks {
            let (ok, message) = apply_block(cx, block);
            if ok {
                applied += 1;
            }
            lines.push(message);
        }

        Ok(ToolOutput::new(
            cap_output(&lines.join("\n"), 16_000),
            format!("{applied}/{} blocks applied", blocks.len()),
        ))
    }
}

fn apply_block(cx: &mut Context, block: &PatchBlock) -> (bool, String) {
    if block.search.is_empty() {
        let abs = match cx.resolve_for_create(&block.path) {
            Ok(path) => path,
            Err(err) => return (false, format!("{}: {}", block.path, err.message)),
        };
        let before = std::fs::read_to_string(&abs).ok();
        let has_crlf = before.as_deref().unwrap_or("").contains("\r\n");
        let replace = if has_crlf {
            block.replace.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            block.replace.clone()
        };
        if let Err(err) = std::fs::write(&abs, &replace) {
            return (false, format!("{}: write failed: {err}", block.path));
        }
        let verb = if before.is_some() {
            "overwrote"
        } else {
            "created"
        };
        let line_count = replace.lines().count();
        cx.record_change(block.path.clone(), before, replace);
        return (true, format!("{}: {verb} ({line_count} lines)", block.path));
    }

    let abs = match cx.resolve_existing(&block.path) {
        Ok(path) => path,
        Err(err) => return (false, format!("{}: {}", block.path, err.message)),
    };
    let content = match std::fs::read_to_string(&abs) {
        Ok(content) => content,
        Err(err) => return (false, format!("{}: {err}", block.path)),
    };

    let has_crlf = content.contains("\r\n");
    let (search, replace) = if has_crlf {
        (
            block.search.replace("\r\n", "\n").replace('\n', "\r\n"),
            block.replace.replace("\r\n", "\n").replace('\n', "\r\n"),
        )
    } else {
        (block.search.clone(), block.replace.clone())
    };

    match apply_to_text(&content, &search, &replace) {
        Err(ApplyErr::NotFound) => (
            false,
            format!(
                "{}: SEARCH not found. Re-read the file and retry with exact text",
                block.path
            ),
        ),
        Err(ApplyErr::Ambiguous(matches)) => (
            false,
            format!(
                "{}: SEARCH matches {matches} places. Include more surrounding context",
                block.path
            ),
        ),
        Ok(new_content) => {
            if let Err(err) = std::fs::write(&abs, &new_content) {
                return (false, format!("{}: write failed: {err}", block.path));
            }
            let (added, removed) = line_delta(&content, &new_content);
            cx.record_change(block.path.clone(), Some(content), new_content);
            (
                true,
                format!("{}: applied (+{added} -{removed})", block.path),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::cancel_token;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn near_miss_lines_still_apply_and_leave_the_rest_alone() {
        let content = "def f():\n    print(\u{201C}hi\u{201D})   \n    return 1\nend\n";
        let out = apply_to_text(
            content,
            "    print(\"hi\")\n    return 1",
            "    print(\"bye\")\n    return 2",
        )
        .unwrap();
        assert_eq!(out, "def f():\n    print(\"bye\")\n    return 2\nend\n");
    }

    #[test]
    fn a_loose_match_never_ignores_indentation_or_picks_between_two() {
        assert!(matches!(
            apply_to_text("if x:\n    y = 1\n", "y = 1 ", "y = 2"),
            Err(ApplyErr::NotFound)
        ));
        assert!(matches!(
            apply_to_text(
                "x = \u{2018}a\u{2019}\nx = \u{2019}a\u{2018}\n",
                "x = 'a'",
                "x = 1"
            ),
            Err(ApplyErr::Ambiguous(2))
        ));
    }

    const PATCH: &str = "\
------ src/a.rs
<<<<<<< SEARCH
let x = 1;
=======
let x = 2;
>>>>>>> REPLACE";

    #[test]
    fn parse_patch_reads_one_block() {
        let blocks = parse_patch(PATCH);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].path, "src/a.rs");
        assert_eq!(blocks[0].search, "let x = 1;");
        assert_eq!(blocks[0].replace, "let x = 2;");
    }

    #[test]
    fn parse_patch_reads_multiple_blocks() {
        let two =
            format!("{PATCH}\n------ src/b.rs\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE");
        let blocks = parse_patch(&two);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1].path, "src/b.rs");
    }

    #[test]
    fn apply_to_text_replaces_single_match() {
        let out = apply_to_text("let x = 1;\n", "let x = 1;", "let x = 2;").unwrap();
        assert_eq!(out, "let x = 2;\n");
    }

    #[test]
    fn apply_to_text_not_found() {
        assert!(matches!(
            apply_to_text("nope", "zzz", "q"),
            Err(ApplyErr::NotFound)
        ));
    }

    #[test]
    fn apply_to_text_ambiguous() {
        assert!(matches!(
            apply_to_text("x\nx\n", "x", "y"),
            Err(ApplyErr::Ambiguous(2))
        ));
    }

    #[test]
    fn line_delta_counts_changes() {
        let (a, r) = line_delta("one\ntwo\n", "one\ntwo\nthree\n");
        assert_eq!((a, r), (1, 0));
    }

    #[test]
    fn apply_patch_edits_and_records() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "let x = 1;\n").unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let out = ApplyPatch.run(&json!({"patch": PATCH}), &mut cx).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
            "let x = 2;\n"
        );
        assert!(out.content.contains("applied"));
        assert_eq!(cx.changes().len(), 1);
        assert_eq!(cx.changes()[0].before, Some("let x = 1;\n".to_string()));
    }

    #[test]
    fn apply_patch_reports_not_found_without_writing() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.rs"), "let y = 9;\n").unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let out = ApplyPatch.run(&json!({"patch": PATCH}), &mut cx).unwrap();
        assert!(out.content.contains("not found"));
        assert_eq!(
            fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
            "let y = 9;\n"
        );
        assert_eq!(cx.changes().len(), 0);
    }

    #[test]
    fn apply_patch_empty_search_creates_file() {
        let dir = tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let create = "------ new.rs\n<<<<<<< SEARCH\n=======\nfn main() {}\n>>>>>>> REPLACE";
        let out = ApplyPatch.run(&json!({"patch": create}), &mut cx).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("new.rs")).unwrap(),
            "fn main() {}"
        );
        assert!(out.content.contains("created"));
    }

    #[test]
    fn apply_patch_no_blocks_is_error() {
        let dir = tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        assert!(ApplyPatch
            .run(&json!({"patch":"garbage with no blocks"}), &mut cx)
            .is_err());
    }

    #[test]
    fn apply_patch_crlf_compatibility() {
        // File on disk uses CRLF line endings.
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        // Write the file with CRLF endings.
        fs::write(dir.path().join("src/a.rs"), "let x = 1;\r\nlet y = 2;\r\n").unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        // Patch text uses plain LF (as is typical from the model).
        let patch =
            "------ src/a.rs\n<<<<<<< SEARCH\nlet x = 1;\n=======\nlet x = 42;\n>>>>>>> REPLACE";
        let out = ApplyPatch.run(&json!({"patch": patch}), &mut cx).unwrap();
        assert!(
            out.content.contains("applied"),
            "patch did not apply: {}",
            out.content
        );
        let result = fs::read_to_string(dir.path().join("src/a.rs")).unwrap();
        // The file must keep CRLF endings and must not contain \r\r\n.
        assert!(
            !result.contains("\r\r\n"),
            "double CRLF detected in output: {:?}",
            result
        );
        assert_eq!(result, "let x = 42;\r\nlet y = 2;\r\n");
    }

    #[test]
    fn apply_patch_crlf_double_conversion() {
        // File on disk uses CRLF line endings.
        // The patch blocks themselves already contain \r\n (Windows-pasted patch),
        // which should NOT produce \r\r\n in the output.
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/b.rs"),
            "fn foo() {\r\n    let x = 1;\r\n}\r\n",
        )
        .unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        // Patch blocks already contain \r\n inside them (simulating a Windows-pasted patch).
        let patch = "------ src/b.rs\n<<<<<<< SEARCH\nfn foo() {\r\n    let x = 1;\r\n}\n=======\nfn foo() {\r\n    let x = 99;\r\n}\n>>>>>>> REPLACE";
        let out = ApplyPatch.run(&json!({"patch": patch}), &mut cx).unwrap();
        assert!(
            out.content.contains("applied"),
            "patch did not apply: {}",
            out.content
        );
        let result = fs::read_to_string(dir.path().join("src/b.rs")).unwrap();
        // Must not have double CRLF.
        assert!(
            !result.contains("\r\r\n"),
            "double CRLF detected in output: {:?}",
            result
        );
        assert_eq!(result, "fn foo() {\r\n    let x = 99;\r\n}\r\n");
    }
}
