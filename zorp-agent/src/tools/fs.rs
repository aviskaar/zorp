use crate::tools::{
    cap_output, Context, Tool, ToolError, ToolOutput, ToolResult, LIST_FILES_OUTPUT_CAP,
    READ_FILE_OUTPUT_CAP,
};
use serde_json::{json, Value};

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn guideline(&self) -> Option<&str> {
        Some("Read files with read_file, not cat or sed in run_command, and follow its start_line hint to page a long file.")
    }

    fn description(&self) -> &str {
        "Read a UTF-8 text file in the repository. Optional 1-based start_line/end_line select a range. A long file comes back in pages of whole lines, each ending with the start_line to continue from."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type":"string","description":"repo-relative file path"},
                "start_line": {"type":"integer"},
                "end_line": {"type":"integer"}
            },
            "required": ["path"]
        })
    }

    fn run(&self, args: &Value, cx: &mut Context) -> ToolResult {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("read_file: 'path' is required"))?;
        let full = cx.resolve_existing(path)?;
        let text =
            std::fs::read_to_string(&full).map_err(|e| ToolError::new(format!("{path}: {e}")))?;

        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        let start = args.get("start_line").and_then(|v| v.as_u64());
        let end = args.get("end_line").and_then(|v| v.as_u64());
        if start.is_none() && end.is_none() && text.len() <= READ_FILE_OUTPUT_CAP {
            return Ok(ToolOutput::new(text, format!("{path} ({total} lines)")));
        }
        let s = start.unwrap_or(1).max(1) as usize;
        let e = (end.unwrap_or(total as u64) as usize).min(total);
        Ok(read_page(
            path,
            lines.get(s.saturating_sub(1)..e).unwrap_or(&[]),
            s,
            total,
        ))
    }
}

/// Whole lines from `first` on, up to the output cap, and when the cap cuts
/// them short, the exact `start_line` to continue from. A cut in the middle
/// of a line with "N bytes truncated" left a small model guessing where the
/// next page began, so it guessed, re-read, or stopped.
fn read_page(path: &str, lines: &[&str], first: usize, total: usize) -> ToolOutput {
    let mut out = String::new();
    let mut shown = 0;
    for line in lines {
        if !out.is_empty() && out.len() + line.len() + 1 > READ_FILE_OUTPUT_CAP {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        shown += 1;
    }
    if shown == 0 {
        return ToolOutput::new(String::new(), format!("{path} (0 lines)"));
    }
    let last = first + shown - 1;
    // Only a single line longer than the cap gets here over it.
    let mut out = cap_output(&out, READ_FILE_OUTPUT_CAP);
    if shown < lines.len() {
        out.push_str(&format!(
            "\n\n[Showing lines {first}-{last} of {total}. Call read_file with start_line={} to continue.]",
            last + 1
        ));
    }
    ToolOutput::new(out, format!("{path} (lines {first}-{last} of {total})"))
}

pub struct ListFiles;

impl Tool for ListFiles {
    fn name(&self) -> &str {
        "list_files"
    }

    fn guideline(&self) -> Option<&str> {
        Some("Look at a directory with list_files before guessing paths; it shows one level unless you pass depth.")
    }

    fn description(&self) -> &str {
        "List the entries directly under a repo-relative directory (default the repo root), sorted, with '/' after each directory. Pass depth 2 or 3 to look further down. Respects .gitignore."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type":"string","description":"repo-relative directory (default '.')"},
                "depth": {"type":"integer","description":"how many levels to list, 1 to 3 (default 1)"}
            },
            "required": []
        })
    }

    fn run(&self, args: &Value, cx: &mut Context) -> ToolResult {
        let rel = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let base = cx.resolve_existing(rel)?;
        // One level by default, like `ls`. Two levels of a working directory
        // full of result files buried the one script a person named under
        // hundreds of lines and a truncation marker.
        let depth = args
            .get("depth")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .clamp(1, 3) as usize;
        let mut entries = Vec::new();
        for dent in ignore::WalkBuilder::new(&base)
            .require_git(false)
            .standard_filters(true)
            .max_depth(Some(depth))
            .build()
        {
            let dent = match dent {
                Ok(d) => d,
                Err(_) => continue,
            };
            if dent.depth() == 0 {
                continue;
            }
            let shown = dent
                .path()
                .strip_prefix(&cx.repo_root)
                .unwrap_or(dent.path());
            let is_dir = dent.file_type().is_some_and(|t| t.is_dir());
            let slash = if is_dir { "/" } else { "" };
            entries.push(format!("{}{slash}", shown.display()));
            if entries.len() >= 500 {
                break;
            }
        }
        entries.sort();
        let n = entries.len();
        Ok(ToolOutput::new(
            cap_output(&entries.join("\n"), LIST_FILES_OUTPUT_CAP),
            format!("{} ({} entries)", rel, n),
        ))
    }
}

/// Create a new file or overwrite an existing one with complete contents.
pub struct WriteFile;

impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Create a new file or overwrite an existing one with the given content. Missing parent directories are created. For a targeted edit, prefer apply_patch."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type":"string","description":"repo-relative file path"},
                "content": {"type":"string","description":"the full new file contents"}
            },
            "required": ["path", "content"]
        })
    }

    fn run(&self, args: &Value, cx: &mut Context) -> ToolResult {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("write_file: 'path' is required"))?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("write_file: 'content' is required"))?;
        let abs = cx.resolve_for_create(path)?;
        let before = std::fs::read_to_string(&abs).ok();
        std::fs::write(&abs, content).map_err(|e| ToolError::new(format!("{path}: {e}")))?;
        cx.record_change(path, before.clone(), content.to_string());
        let lines = content.lines().count();
        let verb = if before.is_some() {
            "overwrote"
        } else {
            "created"
        };
        Ok(ToolOutput::new(
            format!("{verb} {path} ({lines} lines)"),
            format!("{verb} {path} ({lines} lines)"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::cancel_token;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn read_file_returns_contents() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "hello\nworld\n").unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let out = ReadFile.run(&json!({"path":"a.txt"}), &mut cx).unwrap();
        assert_eq!(out.content, "hello\nworld\n");
    }

    #[test]
    fn read_file_honors_line_range() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let out = ReadFile
            .run(
                &json!({"path":"a.txt","start_line":2,"end_line":3}),
                &mut cx,
            )
            .unwrap();
        assert_eq!(out.content, "two\nthree");
    }

    #[test]
    fn read_file_caps_a_whole_file_but_returns_a_requested_range_in_full() {
        let dir = tempdir().unwrap();
        let text = (1..=20_000)
            .map(|line| format!("{line:08}\n"))
            .collect::<String>();
        fs::write(dir.path().join("numbers.csv"), &text).unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());

        let whole = ReadFile
            .run(&json!({"path":"numbers.csv"}), &mut cx)
            .unwrap();
        assert!(
            whole.content.len() <= 16 * 1024 + 256,
            "{}",
            whole.content.len()
        );
        assert!(whole.content.contains("start_line"), "{}", whole.content);

        let ranged = ReadFile
            .run(
                &json!({"path":"numbers.csv", "start_line": 10, "end_line": 12}),
                &mut cx,
            )
            .unwrap();
        assert_eq!(ranged.content, "00000010\n00000011\n00000012");
    }

    #[test]
    fn a_long_file_pages_by_whole_lines_and_says_where_to_continue() {
        let dir = tempdir().unwrap();
        let line = "x".repeat(99);
        let body = vec![line.as_str(); 400].join("\n");
        fs::write(dir.path().join("big.txt"), &body).unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let out = ReadFile.run(&json!({"path":"big.txt"}), &mut cx).unwrap();
        let (page, hint) = out.content.split_once("\n\n[").unwrap();
        assert!(page.lines().all(|l| l == line), "a line was cut");
        let shown = page.lines().count();
        assert!(hint.contains(&format!("lines 1-{shown} of 400")), "{hint}");
        assert!(
            hint.contains(&format!("start_line={}", shown + 1)),
            "{hint}"
        );
        let next = ReadFile
            .run(&json!({"path":"big.txt","start_line": shown + 1}), &mut cx)
            .unwrap();
        assert!(next.content.contains(&format!("lines {}-", shown + 1)));
        let small = ReadFile
            .run(
                &json!({"path":"big.txt","start_line":2,"end_line":3}),
                &mut cx,
            )
            .unwrap();
        assert_eq!(small.content, format!("{line}\n{line}"));
    }

    #[test]
    fn read_file_missing_is_error() {
        let dir = tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        assert!(ReadFile.run(&json!({"path":"nope.txt"}), &mut cx).is_err());
    }

    #[test]
    fn list_files_lists_entries_gitignore_aware() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("kept.txt"), "x").unwrap();
        fs::write(dir.path().join("ignored.txt"), "x").unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let out = ListFiles.run(&json!({}), &mut cx).unwrap();
        assert!(out.content.contains("kept.txt"));
        assert!(!out.content.contains("ignored.txt"));
    }

    #[test]
    fn list_files_is_one_level_with_slashed_dirs_unless_asked_deeper() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub/deeper")).unwrap();
        fs::write(dir.path().join("sub/inner.txt"), "x").unwrap();
        fs::write(dir.path().join("top.py"), "x").unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let out = ListFiles.run(&json!({}), &mut cx).unwrap();
        assert_eq!(out.content, "sub/\ntop.py");
        let deeper = ListFiles.run(&json!({"depth": 2}), &mut cx).unwrap();
        assert!(
            deeper.content.contains("sub/inner.txt"),
            "{}",
            deeper.content
        );
        assert!(deeper.content.contains("sub/deeper/"), "{}", deeper.content);
    }

    #[test]
    fn write_file_creates_and_records() {
        let dir = tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let out = WriteFile
            .run(&json!({"path":"new.txt","content":"hello\n"}), &mut cx)
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("new.txt")).unwrap(),
            "hello\n"
        );
        assert!(out.content.contains("created"));
        assert_eq!(cx.changes().len(), 1);
        assert_eq!(cx.changes()[0].before, None);
    }

    #[test]
    fn write_file_overwrites_and_records_before() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "old").unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let out = WriteFile
            .run(&json!({"path":"a.txt","content":"new"}), &mut cx)
            .unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("a.txt")).unwrap(), "new");
        assert!(out.content.contains("overwrote"));
        assert_eq!(cx.changes()[0].before, Some("old".to_string()));
    }

    #[test]
    fn write_file_rejects_escape() {
        let dir = tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        assert!(WriteFile
            .run(&json!({"path":"../evil.txt","content":"x"}), &mut cx)
            .is_err());
    }
}
