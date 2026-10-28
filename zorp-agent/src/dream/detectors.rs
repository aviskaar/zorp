//! The Dreaming-mode detectors.
//!
//! Every detector here is code over code-derived columns, the same split
//! `critique` and the aryabhatta detectors use (ADR 0063). A detector reads
//! conversation records and returns [`Pattern`]s: a kind, the ids of the
//! conversations the pattern is drawn from, a neutral code-written summary,
//! and the verbatim messages behind it. It never writes a sentence about the
//! person, and it never reads a model-authored column. The model, if one is
//! used at all, only words a pattern afterwards (see the phraser).

use serde::{Deserialize, Serialize};

/// One tool call a conversation made, with the status derived in code from
/// the stored result (not from anything a model said about it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolEvent {
    /// The command or tool name, normalized for grouping.
    pub label: String,
    /// `ok`, `error`, or `cancelled`, derived from the stored result.
    pub status: String,
}

/// What a detector reads about one conversation. All of it is code-derived:
/// the first user message is the verbatim `task`, the project is a column,
/// the timestamp is the stored clock, and the tool events carry a
/// code-derived status. Nothing here is model-authored text a detector would
/// be reasoning over.
#[derive(Clone, Debug)]
pub struct ConvRecord {
    pub session_id: String,
    pub created: i64,
    pub project_id: Option<String>,
    pub first_user_message: String,
    pub tool_events: Vec<ToolEvent>,
}

/// A pattern a detector found. The evidence is references plus the verbatim
/// messages; the `summary` is a neutral code-written line used to prompt a
/// phraser, never stored as the suggestion.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pattern {
    pub kind: String,
    pub evidence_ids: Vec<String>,
    pub summary: String,
    pub evidence_excerpts: Vec<String>,
}

/// How many conversations make a group worth suggesting about. Two is enough
/// for a repeated failure (it happened more than once); recurring requests
/// and unfiled clusters want a little more signal.
const RECURRING_MIN: usize = 3;
const CLUSTER_MIN: usize = 2;
const FAILURE_MIN: usize = 2;

/// Run every detector over `records` and return all patterns found.
pub fn detect_all(records: &[ConvRecord]) -> Vec<Pattern> {
    let mut out = Vec::new();
    out.extend(recurring_requests(records));
    out.extend(unfiled_clusters(records));
    out.extend(repeated_failures(records));
    out
}

/// Near-identical first messages across several conversations: something the
/// person asks for again and again, which a flavor or a saved prompt could
/// make one click.
pub fn recurring_requests(records: &[ConvRecord]) -> Vec<Pattern> {
    let mut groups: std::collections::BTreeMap<String, Vec<&ConvRecord>> = Default::default();
    for record in records {
        let key = normalize(&record.first_user_message);
        if key.is_empty() {
            continue;
        }
        groups.entry(key).or_default().push(record);
    }
    let mut out = Vec::new();
    for (_, group) in groups {
        if group.len() < RECURRING_MIN {
            continue;
        }
        out.push(Pattern {
            kind: "recurring_request".into(),
            evidence_ids: group.iter().map(|r| r.session_id.clone()).collect(),
            summary: format!(
                "{} conversations open with nearly the same request",
                group.len()
            ),
            evidence_excerpts: group.iter().map(|r| r.first_user_message.clone()).collect(),
        });
    }
    out
}

/// Conversations that look alike and are in no project: candidates to file
/// together. The real feature narrows this with embedding similarity over the
/// recall index; this code-only pass groups by the same normalized first
/// message, which is the conservative subset.
pub fn unfiled_clusters(records: &[ConvRecord]) -> Vec<Pattern> {
    let mut groups: std::collections::BTreeMap<String, Vec<&ConvRecord>> = Default::default();
    for record in records {
        if record.project_id.is_some() {
            continue;
        }
        let key = normalize(&record.first_user_message);
        if key.is_empty() {
            continue;
        }
        groups.entry(key).or_default().push(record);
    }
    let mut out = Vec::new();
    for (_, group) in groups {
        if group.len() < CLUSTER_MIN {
            continue;
        }
        out.push(Pattern {
            kind: "unfiled_cluster".into(),
            evidence_ids: group.iter().map(|r| r.session_id.clone()).collect(),
            summary: format!(
                "{} unfiled conversations are about the same thing",
                group.len()
            ),
            evidence_excerpts: group.iter().map(|r| r.first_user_message.clone()).collect(),
        });
    }
    out
}

/// The same command failing across more than one conversation: a candidate
/// for a saved fix. Grouped by the command label, counting only the
/// conversations where it carried an `error` status.
pub fn repeated_failures(records: &[ConvRecord]) -> Vec<Pattern> {
    let mut by_command: std::collections::BTreeMap<String, Vec<&ConvRecord>> = Default::default();
    for record in records {
        let failed: std::collections::BTreeSet<&str> = record
            .tool_events
            .iter()
            .filter(|e| e.status == "error")
            .map(|e| e.label.as_str())
            .collect();
        for command in failed {
            by_command
                .entry(command.to_string())
                .or_default()
                .push(record);
        }
    }
    let mut out = Vec::new();
    for (command, group) in by_command {
        if group.len() < FAILURE_MIN {
            continue;
        }
        out.push(Pattern {
            kind: "repeated_failure".into(),
            evidence_ids: group.iter().map(|r| r.session_id.clone()).collect(),
            summary: format!(
                "the command `{command}` failed in {} conversations",
                group.len()
            ),
            evidence_excerpts: group.iter().map(|r| r.first_user_message.clone()).collect(),
        });
    }
    out
}

/// Lowercase, collapse whitespace, drop surrounding punctuation, so "Run the
/// Monday report." and "run the monday report" group together. Deliberately
/// simple: a near-duplicate check, not a model.
fn normalize(text: &str) -> String {
    let mut out = String::new();
    let mut last_was_space = true;
    for ch in text.trim().chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_was_space = false;
        } else if (ch.is_whitespace() || ch.is_ascii_punctuation()) && !last_was_space {
            out.push(' ');
            last_was_space = true;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(id: &str, msg: &str, project: Option<&str>) -> ConvRecord {
        ConvRecord {
            session_id: id.into(),
            created: 0,
            project_id: project.map(str::to_string),
            first_user_message: msg.into(),
            tool_events: Vec::new(),
        }
    }

    fn with_failures(mut c: ConvRecord, commands: &[&str]) -> ConvRecord {
        c.tool_events = commands
            .iter()
            .map(|label| ToolEvent {
                label: label.to_string(),
                status: "error".into(),
            })
            .collect();
        c
    }

    #[test]
    fn recurring_needs_three_near_identical_requests() {
        let records = vec![
            conv("a", "Run the Monday report.", None),
            conv("b", "run the monday report", None),
            conv("c", "Run the Monday report!!", None),
            conv("d", "something else entirely", None),
        ];
        let patterns = recurring_requests(&records);
        assert_eq!(patterns.len(), 1, "{patterns:?}");
        assert_eq!(patterns[0].kind, "recurring_request");
        assert_eq!(patterns[0].evidence_ids.len(), 3);
        // Two of a kind is not enough.
        let two = vec![conv("a", "do x", None), conv("b", "do x", None)];
        assert!(recurring_requests(&two).is_empty());
    }

    #[test]
    fn unfiled_clusters_ignore_filed_conversations() {
        let records = vec![
            conv("a", "about project zorp", None),
            conv("b", "about project zorp", None),
            conv("c", "about project zorp", Some("p1")),
        ];
        let patterns = unfiled_clusters(&records);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].evidence_ids, vec!["a", "b"]);
    }

    #[test]
    fn repeated_failures_group_by_command_across_sessions() {
        let records = vec![
            with_failures(conv("a", "build it", None), &["cargo build", "ls"]),
            with_failures(conv("b", "build again", None), &["cargo build"]),
            with_failures(conv("c", "unrelated", None), &["npm test"]),
        ];
        let patterns = repeated_failures(&records);
        assert_eq!(patterns.len(), 1, "{patterns:?}");
        assert_eq!(patterns[0].kind, "repeated_failure");
        assert!(patterns[0].summary.contains("cargo build"));
        assert_eq!(patterns[0].evidence_ids, vec!["a", "b"]);
    }

    #[test]
    fn detect_all_runs_every_detector() {
        let records = vec![
            conv("a", "weekly digest", None),
            conv("b", "weekly digest", None),
            conv("c", "weekly digest", None),
        ];
        let patterns = detect_all(&records);
        // Recurring (>=3) and unfiled cluster (>=2) both fire on this group.
        assert!(patterns.iter().any(|p| p.kind == "recurring_request"));
        assert!(patterns.iter().any(|p| p.kind == "unfiled_cluster"));
    }
}
