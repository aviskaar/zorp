use crate::tools::{cap_output, Context, Tool, ToolError, ToolOutput, ToolResult};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;

pub struct TakeNote;

impl Tool for TakeNote {
    fn name(&self) -> &str {
        "take_note"
    }

    fn description(&self) -> &str {
        "Create or append to a markdown note in the knowledge base (.zorp/notes/ directory)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {"type":"string","description":"title of the note (used for filename)"},
                "content": {"type":"string","description":"content to append to the note"}
            },
            "required": ["title", "content"]
        })
    }

    fn run(&self, args: &Value, cx: &mut Context) -> ToolResult {
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("take_note: 'title' is required"))?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("take_note: 'content' is required"))?;

        let notes_dir = cx.repo_root.join(".zorp").join("notes");
        if !notes_dir.exists() {
            fs::create_dir_all(&notes_dir).map_err(|e| {
                ToolError::new(format!("failed to create .zorp/notes directory: {e}"))
            })?;
        }

        let filename = format!("{}.md", title.replace(|c: char| !c.is_alphanumeric(), "-"));
        let file_path = notes_dir.join(&filename);
        let rel_path = format!(".zorp/notes/{}", filename);

        let before = fs::read_to_string(&file_path).ok();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| ToolError::new(format!("failed to open note {}: {}", filename, e)))?;

        writeln!(file, "{}", content)
            .map_err(|e| ToolError::new(format!("failed to write to note {}: {}", filename, e)))?;

        drop(file);
        let after = fs::read_to_string(&file_path).unwrap_or_default();
        cx.record_change(&rel_path, before, after);

        let msg = format!("appended to {}", rel_path);
        Ok(ToolOutput::new(msg.clone(), msg))
    }
}

pub struct SearchNotes;

impl Tool for SearchNotes {
    fn name(&self) -> &str {
        "search_notes"
    }

    fn description(&self) -> &str {
        "Search through knowledge base notes (.zorp/notes/ directory)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type":"string","description":"text to search for in notes"}
            },
            "required": ["query"]
        })
    }

    fn run(&self, args: &Value, cx: &mut Context) -> ToolResult {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::new("search_notes: 'query' is required"))?;

        let notes_dir = cx.repo_root.join(".zorp").join("notes");
        if !notes_dir.exists() {
            return Ok(ToolOutput::new("no matches".to_string(), "0 matches"));
        }

        let mut hits: Vec<String> = Vec::new();
        'walk: for dent in ignore::WalkBuilder::new(&notes_dir)
            .require_git(false)
            .standard_filters(true)
            .build()
        {
            let dent = match dent {
                Ok(d) => d,
                Err(_) => continue,
            };
            if !dent.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            if dent.path().extension().is_none_or(|ext| ext != "md") {
                continue;
            }
            let text = match std::fs::read_to_string(dent.path()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let shown = dent
                .path()
                .strip_prefix(&cx.repo_root)
                .unwrap_or(dent.path())
                .display()
                .to_string();

            // `take_note` writes the title into the filename and only the
            // body into the file, so a note whose distinguishing word is its
            // title has nothing to match on inside the text. Record the file
            // itself in that case, but only when no line matched, so one note
            // is one result.
            let mut matched_a_line = false;
            for (i, line) in text.lines().enumerate() {
                if line.contains(query) {
                    matched_a_line = true;
                    hits.push(format!("{}:{}: {}", shown, i + 1, line.trim_end()));
                    if hits.len() >= 200 {
                        break 'walk;
                    }
                }
            }
            if !matched_a_line
                && dent
                    .path()
                    .file_stem()
                    .is_some_and(|stem| stem.to_str().is_some_and(|stem| stem.contains(query)))
            {
                hits.push(format!("{shown}: (title match)"));
                if hits.len() >= 200 {
                    break 'walk;
                }
            }
        }

        let n = hits.len();
        let content = if hits.is_empty() {
            "no matches".to_string()
        } else {
            hits.join("\n")
        };
        Ok(ToolOutput::new(
            cap_output(&content, 32_000),
            format!("'{}' ({} matches)", query, n),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::cancel_token;
    use tempfile::tempdir;

    #[test]
    fn take_note_creates_and_appends() {
        let dir = tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());

        // Create note
        let out = TakeNote
            .run(
                &json!({"title": "Test Note", "content": "hello world"}),
                &mut cx,
            )
            .unwrap();
        assert!(out.content.contains("Test-Note.md"));

        let path = dir.path().join(".zorp").join("notes").join("Test-Note.md");
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world\n");
        assert!(
            !dir.path().join(".qkb").exists(),
            "notes must not resurrect quecto's .qkb directory"
        );

        // Append to note
        TakeNote
            .run(
                &json!({"title": "Test Note", "content": "second line"}),
                &mut cx,
            )
            .unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "hello world\nsecond line\n"
        );
    }

    #[test]
    fn search_notes_finds_matches() {
        let dir = tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());

        TakeNote
            .run(
                &json!({"title": "Rust Tips", "content": "Use cargo clippy"}),
                &mut cx,
            )
            .unwrap();
        TakeNote
            .run(
                &json!({"title": "Cargo", "content": "cargo build --release"}),
                &mut cx,
            )
            .unwrap();

        let out = SearchNotes
            .run(&json!({"query": "cargo"}), &mut cx)
            .unwrap();
        assert!(out.content.contains("Rust-Tips.md"));
        assert!(out.content.contains("Cargo.md"));
    }

    /// `take_note` puts the title in the filename and only the body in the
    /// file, so searching by the title, the most natural way to look a note
    /// up, used to be the one way that could not work.
    #[test]
    fn search_notes_matches_the_title_not_only_the_body() {
        let dir = tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());

        TakeNote
            .run(
                &json!({"title": "build-marker-8802", "content": "is the build id"}),
                &mut cx,
            )
            .unwrap();

        let out = SearchNotes
            .run(&json!({"query": "build-marker-8802"}), &mut cx)
            .unwrap();
        assert!(
            out.content.contains("build-marker-8802.md"),
            "a note must be findable by its title: {}",
            out.content
        );
        assert_eq!(out.summary, "'build-marker-8802' (1 matches)");
    }

    /// A title hit and a body hit for the same note are one note, not two.
    #[test]
    fn a_note_matching_in_both_title_and_body_is_counted_once() {
        let dir = tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        TakeNote
            .run(
                &json!({"title": "cargo", "content": "cargo build"}),
                &mut cx,
            )
            .unwrap();
        let out = SearchNotes
            .run(&json!({"query": "cargo"}), &mut cx)
            .unwrap();
        assert_eq!(out.summary, "'cargo' (1 matches)", "{}", out.content);
    }

    /// Notes live under `.zorp/`, which a project may well gitignore
    /// wholesale to keep the research stack's DuckDB file out of the repo.
    /// zorp's own notes are still zorp's to read.
    #[test]
    fn search_notes_finds_notes_in_a_gitignored_zorp_dir() {
        let dir = tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        fs::write(dir.path().join(".gitignore"), ".zorp/\n").unwrap();

        TakeNote
            .run(
                &json!({"title": "Buried", "content": "findme in a gitignored dir"}),
                &mut cx,
            )
            .unwrap();

        let out = SearchNotes
            .run(&json!({"query": "findme"}), &mut cx)
            .unwrap();
        assert!(
            out.content.contains("Buried.md"),
            "gitignoring .zorp/ must not hide notes from search_notes: {}",
            out.content
        );
    }
}
