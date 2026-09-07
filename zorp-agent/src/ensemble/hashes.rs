//! What the reviewers are watched against, decided in code.
//!
//! The loop hashes every file the main run recorded as changed and every
//! file under the paths the instruction names, before the reviewers start
//! and after each one finishes. A reviewer whose run altered any of them
//! is dropped. The same hashes key the memoized verdicts and flip a
//! finding from open to addressed. Nothing here reads a word of model text
//! except tool-call arguments, and those only to see which watched paths
//! they mention.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::model::Message;

/// Files hashed per directory the instruction names, at most. A task that
/// names `/app` names its whole data tree, and hashing gigabytes several
/// times a round is not a check anybody waits for.
// ponytail: flat cap on a sorted walk so the same files are picked every
// time; raise it or exclude data directories if a task needs it.
const FILES_PER_DIR: usize = 200;

/// A path string as the tools saw it, mapped to its content hash, or
/// `None` when the file is missing or unreadable.
pub type Snapshot = BTreeMap<String, Option<String>>;

/// The path as the tools saw it, made absolute against the workspace.
pub fn resolve(root: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

/// Path-like tokens in the instruction: absolute paths, and relative ones
/// with at least one directory and an extension. Trailing punctuation is
/// the sentence's, not the path's. A scheme-qualified URL is matched and
/// dropped whole, first: `regex` has no lookbehind, so without it the
/// absolute-path alternative starts happily on the second slash of a
/// URL's `//` and reads straight through the host into the route.
pub fn named_paths(instruction: &str) -> BTreeSet<String> {
    let re = regex::Regex::new(
        r"(?:[A-Za-z][\w+.-]*://\S+)|(?:/(?:[\w.-]+/)*[\w.-]+)|(?:\b(?:[\w-]+/)+[\w-]+\.[A-Za-z0-9]+)",
    )
    .expect("a literal regex");
    re.find_iter(instruction)
        .map(|m| m.as_str())
        .filter(|s| !s.contains("://"))
        .map(|s| {
            s.trim_end_matches(['.', ',', ':', ';', ')', '\'', '"'])
                .to_string()
        })
        .filter(|s| s.len() > 1)
        .collect()
}

/// Every path the loop watches: what the main run changed and what the
/// instruction names, with named directories expanded to their files.
/// A named file that does not exist is watched too, so its later creation
/// counts as a change.
pub fn watched(root: &Path, instruction: &str, changed: &[String]) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = changed.iter().cloned().collect();
    for token in named_paths(instruction) {
        let abs = resolve(root, &token);
        if abs.is_dir() {
            let mut files = Vec::new();
            walk(&abs, &mut files);
            files.sort();
            for f in files.into_iter().take(FILES_PER_DIR) {
                out.insert(f.display().to_string());
            }
        } else {
            out.insert(token);
        }
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= FILES_PER_DIR {
            return;
        }
        let p = entry.path();
        let hidden = p
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Hash every watched file as it is right now.
pub fn snapshot(root: &Path, watched: &BTreeSet<String>) -> Snapshot {
    watched
        .iter()
        .map(|p| (p.clone(), hash_file(&resolve(root, p))))
        .collect()
}

fn hash_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(&bytes)))
}

/// Paths whose hash differs between two snapshots, sorted.
pub fn changed(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let keys: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    keys.into_iter()
        .filter(|k| before.get(*k) != after.get(*k))
        .cloned()
        .collect()
}

/// The watched paths a reviewer's tool calls mention. Read from the
/// transcript's tool-call arguments in code; the reviewer is never asked
/// what it looked at. A path the reviewer spelled differently from the
/// watched form is missed, which only costs a memo hit, never a check.
pub fn examined(transcript: &[Message], watched: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for m in transcript.iter().filter(|m| m.role == "assistant") {
        for call in &m.tool_calls {
            let args = call.arguments.to_string();
            for p in watched {
                if args.contains(p.as_str()) {
                    out.insert(p.clone());
                }
            }
        }
    }
    out
}

/// The snapshot narrowed to some paths: a memo key.
pub fn restrict(snapshot: &Snapshot, paths: &BTreeSet<String>) -> Snapshot {
    snapshot
        .iter()
        .filter(|(k, _)| paths.contains(*k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// True while every file in the key still has the hash the key recorded.
pub fn still_holds(key: &Snapshot, current: &Snapshot) -> bool {
    key.iter().all(|(k, v)| current.get(k) == Some(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ToolCall;
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> Message {
        Message::assistant_with_calls(
            "",
            vec![ToolCall {
                id: "c1".to_string(),
                name: name.to_string(),
                arguments: args,
            }],
        )
    }

    #[test]
    fn named_paths_picks_absolute_and_relative_paths_and_nothing_else() {
        let text = "Write /app/results/summary.json and results/out.csv. \
                    Use python3 and numpy. See https://example.com/docs.";
        let paths = named_paths(text);
        // Exact-set equality, not a handful of contains() checks: the only
        // things a run has to hash are the two real paths. A URL is not a
        // watched file, however path-shaped its host and route look.
        assert_eq!(
            paths,
            [
                "/app/results/summary.json".to_string(),
                "results/out.csv".to_string(),
            ]
            .into()
        );
    }

    #[test]
    fn watched_expands_a_named_directory_and_keeps_a_missing_named_file() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir_all(out.join("sub")).unwrap();
        std::fs::write(out.join("a.csv"), "1").unwrap();
        std::fs::write(out.join("sub").join("b.csv"), "2").unwrap();
        let instruction = format!(
            "Put everything under {} and write {}/missing.json.",
            out.display(),
            dir.path().display()
        );
        let watched = watched(dir.path(), &instruction, &["notes.txt".to_string()]);
        assert!(watched.contains(&out.join("a.csv").display().to_string()));
        assert!(watched.contains(&out.join("sub").join("b.csv").display().to_string()));
        assert!(watched.contains(&format!("{}/missing.json", dir.path().display())));
        assert!(watched.contains("notes.txt"));
    }

    #[test]
    fn a_snapshot_sees_a_change_and_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "one").unwrap();
        let set: BTreeSet<String> = ["a.txt".to_string(), "gone.txt".to_string()].into();
        let before = snapshot(dir.path(), &set);
        assert!(before["a.txt"].is_some());
        assert!(before["gone.txt"].is_none());
        assert!(changed(&before, &before).is_empty());

        std::fs::write(dir.path().join("a.txt"), "two").unwrap();
        std::fs::write(dir.path().join("gone.txt"), "here").unwrap();
        let after = snapshot(dir.path(), &set);
        assert_eq!(changed(&before, &after), vec!["a.txt", "gone.txt"]);
    }

    #[test]
    fn examined_is_the_watched_paths_a_tool_call_mentions() {
        let set: BTreeSet<String> = ["out/a.csv".to_string(), "out/b.csv".to_string()].into();
        let transcript = vec![
            Message::user("review"),
            call("read_file", json!({"path": "/w/out/a.csv"})),
            call("run_command", json!({"command": "wc -l notes.txt"})),
        ];
        let seen = examined(&transcript, &set);
        assert_eq!(seen, ["out/a.csv".to_string()].into());
    }

    #[test]
    fn a_memo_key_holds_while_its_files_are_unchanged() {
        let mut current: Snapshot = BTreeMap::new();
        current.insert("a".to_string(), Some("h1".to_string()));
        current.insert("b".to_string(), Some("h2".to_string()));
        let key = restrict(&current, &["a".to_string()].into());
        assert!(still_holds(&key, &current));
        current.insert("b".to_string(), Some("h3".to_string()));
        assert!(still_holds(&key, &current), "b is not in the key");
        current.insert("a".to_string(), None);
        assert!(!still_holds(&key, &current));
    }
}
