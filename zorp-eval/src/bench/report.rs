//! Where bench results live, and the one query that turns them into a table.
//!
//! There is no second store. Every item is a row in the runner's `runs`
//! table, keyed by `experiment_id`, `runtime_id` and `repetition` like a
//! compat run, with `passed` NULL when there was nothing to grade. The
//! columns only a benchmark has (the outcome, why an item is unevaluable,
//! time to first token) go in `bench_results`, keyed by the same `run_id`,
//! and the table is a join of the two.
//!
//! Every cell is computed from code-derived columns: counts of an outcome
//! the runner assigned, a reason from a closed set, and timings the client
//! measured. No model is asked what it scored and no text a model wrote
//! reaches a cell; `bench_results.response` exists for a person debugging
//! a grader and nothing here reads it. The rule is the one
//! `evals/harbor/ensemble_report.py` lives under.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rusqlite::Connection;

pub fn init_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS bench_results (
            id INTEGER PRIMARY KEY,
            session TEXT NOT NULL,
            run_id TEXT NOT NULL,
            benchmark TEXT NOT NULL,
            item_id TEXT NOT NULL,
            outcome TEXT NOT NULL
                CHECK (outcome IN ('correct', 'incorrect', 'unevaluable')),
            unparsed INTEGER NOT NULL DEFAULT 0,
            reason TEXT,
            http_status INTEGER,
            detail TEXT,
            expected TEXT,
            extracted TEXT,
            response TEXT,
            prompt_tokens INTEGER,
            completion_tokens INTEGER,
            ttft_ms INTEGER,
            sends INTEGER
        )",
        (),
    )?;
    Ok(())
}

/// One item's result, as the runner writes it.
pub struct Record<'a> {
    pub session: &'a str,
    pub experiment_id: &'a str,
    pub runtime_id: &'a str,
    pub benchmark: &'a str,
    pub item_id: &'a str,
    pub repetition: u32,
    pub outcome: Outcome,
    pub unparsed: bool,
    pub reason: Option<&'a str>,
    pub http_status: Option<u16>,
    pub detail: Option<&'a str>,
    pub expected: &'a str,
    pub extracted: Option<&'a str>,
    pub response: Option<&'a str>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub ttft_ms: Option<u64>,
    pub latency_ms: u64,
    pub sends: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Correct,
    Incorrect,
    Unevaluable,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Correct => "correct",
            Outcome::Incorrect => "incorrect",
            Outcome::Unevaluable => "unevaluable",
        }
    }
}

pub fn insert(conn: &Connection, r: &Record) -> anyhow::Result<()> {
    let run_id = format!(
        "{}/{}/{}/{}/{}",
        r.session, r.runtime_id, r.benchmark, r.item_id, r.repetition
    );
    // NULL, not false, for an item with no answer: the column's third state
    // is the whole point.
    let passed = match r.outcome {
        Outcome::Correct => Some(true),
        Outcome::Incorrect => Some(false),
        Outcome::Unevaluable => None,
    };
    let tokens = match (r.prompt_tokens, r.completion_tokens) {
        (Some(p), Some(c)) => Some(p + c),
        _ => None,
    };
    conn.execute(
        "INSERT INTO runs (task_id, suite, passed, tokens, turns, latency, experiment_id, runtime_id, run_id, repetition)
         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            format!("{}/{}", r.benchmark, r.item_id),
            format!("bench:{}", r.benchmark),
            passed,
            tokens.map(|t| t as i64),
            r.latency_ms as i64,
            r.experiment_id,
            r.runtime_id,
            run_id,
            r.repetition,
        ],
    )?;
    conn.execute(
        "INSERT INTO bench_results (session, run_id, benchmark, item_id, outcome, unparsed, reason, http_status, detail, expected, extracted, response, prompt_tokens, completion_tokens, ttft_ms, sends)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        rusqlite::params![
            r.session,
            run_id,
            r.benchmark,
            r.item_id,
            r.outcome.as_str(),
            r.unparsed,
            r.reason,
            r.http_status,
            r.detail,
            r.expected,
            r.extracted,
            r.response,
            r.prompt_tokens.map(|t| t as i64),
            r.completion_tokens.map(|t| t as i64),
            r.ttft_ms.map(|t| t as i64),
            r.sends,
        ],
    )?;
    Ok(())
}

/// One cell group: one benchmark on one runtime.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub benchmark: String,
    pub runtime: String,
    /// Requests made: items times repetitions.
    pub attempted: u64,
    /// Items with an answer that was graded, right or wrong.
    pub scored: u64,
    pub unevaluable: u64,
    pub correct: u64,
    /// Scored as wrong because no answer could be read from the reply.
    pub unparsed: u64,
    /// Unevaluable items by reason code.
    pub reasons: BTreeMap<String, u64>,
    /// Medians over items answered on their first send.
    pub latency_ms: Option<u64>,
    pub ttft_ms: Option<u64>,
    pub tokens_per_sec: Option<f64>,
}

impl Row {
    /// `correct / scored`, or nothing when nothing was scored. Never 0.0 for
    /// a row with no answers in it.
    pub fn accuracy(&self) -> Option<f64> {
        (self.scored > 0).then(|| self.correct as f64 / self.scored as f64)
    }
}

/// The table for one session, in the order given: benchmarks as the cases
/// were read, runtimes as the manifest lists them.
pub fn rows(
    conn: &Connection,
    session: &str,
    benchmarks: &[String],
    runtimes: &[String],
) -> anyhow::Result<Vec<Row>> {
    let mut statement = conn.prepare(
        "SELECT r.runtime_id, b.benchmark, b.outcome, b.unparsed, b.reason,
                r.latency, b.ttft_ms, b.completion_tokens, b.sends
         FROM bench_results b JOIN runs r ON r.run_id = b.run_id
         WHERE b.session = ?1",
    )?;
    let mut by_key: BTreeMap<(String, String), (Row, Samples)> = BTreeMap::new();
    let mut query = statement.query([session])?;
    while let Some(row) = query.next()? {
        let runtime: String = row.get(0)?;
        let benchmark: String = row.get(1)?;
        let outcome: String = row.get(2)?;
        let unparsed: bool = row.get(3)?;
        let reason: Option<String> = row.get(4)?;
        let latency: Option<i64> = row.get(5)?;
        let ttft: Option<i64> = row.get(6)?;
        let completion: Option<i64> = row.get(7)?;
        let sends: Option<i64> = row.get(8)?;
        let (entry, samples) = by_key
            .entry((benchmark.clone(), runtime.clone()))
            .or_insert_with(|| {
                (
                    Row {
                        benchmark,
                        runtime,
                        ..Row::default()
                    },
                    Samples::default(),
                )
            });
        entry.attempted += 1;
        match outcome.as_str() {
            "unevaluable" => {
                entry.unevaluable += 1;
                *entry
                    .reasons
                    .entry(reason.unwrap_or_else(|| "unknown".into()))
                    .or_default() += 1;
                continue;
            }
            "correct" => {
                entry.scored += 1;
                entry.correct += 1;
            }
            _ => {
                entry.scored += 1;
                if unparsed {
                    entry.unparsed += 1;
                }
            }
        }
        // Timing only from an answer that came back on its first send: a
        // retried request's wall time includes the backoff, which is ours.
        if sends == Some(1) {
            if let Some(latency) = latency {
                samples.latency.push(latency as f64);
                if let Some(ttft) = ttft {
                    samples.ttft.push(ttft as f64);
                    let decoding = (latency - ttft) as f64 / 1000.0;
                    if let Some(tokens) = completion.filter(|_| decoding > 0.0) {
                        samples.rate.push(tokens as f64 / decoding);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for benchmark in benchmarks {
        for runtime in runtimes {
            let Some((mut row, samples)) = by_key.remove(&(benchmark.clone(), runtime.clone()))
            else {
                continue;
            };
            row.latency_ms = median(samples.latency).map(|m| m.round() as u64);
            row.ttft_ms = median(samples.ttft).map(|m| m.round() as u64);
            row.tokens_per_sec = median(samples.rate);
            out.push(row);
        }
    }
    Ok(out)
}

#[derive(Default)]
struct Samples {
    latency: Vec<f64>,
    ttft: Vec<f64>,
    rate: Vec<f64>,
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

/// The contamination caveat every table carries, in text, so no reader has
/// to rediscover it.
pub const CAVEAT: &str = "\
These benchmarks are public and old enough to be in a model's training data, so a score may \
reflect contamination rather than capability. Prompts are zero-shot, the same for every \
runtime, and graded by string match, so rows in this table compare with each other and not \
with published scores, where prompt format and grader move the number more than the model does.";

/// The table as text: a pipe table a pull request renders, then the
/// unevaluable reasons, then what the columns mean and the caveat.
pub fn render(session: &str, rows: &[Row]) -> String {
    let header = [
        "benchmark",
        "runtime",
        "attempted",
        "scored",
        "unevaluable",
        "correct",
        "accuracy",
        "unparsed",
        "p50 latency ms",
        "p50 ttft ms",
        "p50 tok/s",
    ];
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.benchmark.clone(),
                r.runtime.clone(),
                r.attempted.to_string(),
                r.scored.to_string(),
                r.unevaluable.to_string(),
                r.correct.to_string(),
                r.accuracy()
                    .map_or_else(|| "n/a".to_string(), |a| format!("{:.1}%", a * 100.0)),
                r.unparsed.to_string(),
                r.latency_ms.map_or_else(|| "-".into(), |v| v.to_string()),
                r.ttft_ms.map_or_else(|| "-".into(), |v| v.to_string()),
                r.tokens_per_sec
                    .map_or_else(|| "-".into(), |v| format!("{v:.1}")),
            ]
        })
        .collect();
    let widths: Vec<usize> = (0..header.len())
        .map(|i| {
            cells
                .iter()
                .map(|row| row[i].len())
                .chain([header[i].len()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |values: &[&str]| -> String {
        let parts: Vec<String> = values
            .iter()
            .zip(&widths)
            .enumerate()
            .map(|(i, (v, w))| {
                // Names left, numbers right.
                if i < 2 {
                    format!("{v:<w$}")
                } else {
                    format!("{v:>w$}")
                }
            })
            .collect();
        format!("| {} |", parts.join(" | "))
    };
    let mut out = String::new();
    let _ = writeln!(out, "bench session {session}");
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", line(&header));
    let rule: Vec<String> = widths
        .iter()
        .enumerate()
        .map(|(i, w)| {
            if i < 2 {
                "-".repeat(*w)
            } else {
                format!("{}:", "-".repeat(w.saturating_sub(1)))
            }
        })
        .collect();
    let rule: Vec<&str> = rule.iter().map(String::as_str).collect();
    let _ = writeln!(out, "{}", line(&rule));
    for row in &cells {
        let row: Vec<&str> = row.iter().map(String::as_str).collect();
        let _ = writeln!(out, "{}", line(&row));
    }
    let with_reasons: Vec<&Row> = rows.iter().filter(|r| !r.reasons.is_empty()).collect();
    if !with_reasons.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Unevaluable, by reason:");
        for row in with_reasons {
            let reasons: Vec<String> = row
                .reasons
                .iter()
                .map(|(reason, n)| format!("{n} {reason}"))
                .collect();
            let _ = writeln!(
                out,
                "  {} on {}: {}",
                row.benchmark,
                row.runtime,
                reasons.join(", ")
            );
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "accuracy is correct / scored. An unevaluable item is one with no answer to grade \
         (unreachable, timeout, rate limited, an HTTP or provider error, a stream that broke off, \
         a reply cut off at the token limit or declined by a filter, an empty reply). It is never \
         scored, so it never counts as wrong, and a row that scored nothing says n/a, not 0. \
         unparsed replies are scored as wrong: the model answered and no letter or number could \
         be read from it. Timings are medians over answers that came back on their first send; \
         tok/s is completion tokens over the time after the first token, where the provider \
         reported usage."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Caveat: {CAVEAT}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record<'a>(runtime: &'a str, item: &'a str, outcome: Outcome) -> Record<'a> {
        Record {
            session: "s",
            experiment_id: "e",
            runtime_id: runtime,
            benchmark: "mmlu",
            item_id: item,
            repetition: 0,
            outcome,
            unparsed: false,
            reason: (outcome == Outcome::Unevaluable).then_some("unreachable"),
            http_status: None,
            detail: None,
            expected: "A",
            extracted: None,
            response: None,
            prompt_tokens: Some(10),
            completion_tokens: Some(20),
            ttft_ms: Some(100),
            latency_ms: 1100,
            sends: 1,
        }
    }

    #[test]
    fn an_unevaluable_row_is_counted_apart_and_never_scored() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::runner::init_db(&dir.path().join("t.db")).unwrap();
        init_schema(&conn).unwrap();
        insert(&conn, &record("up", "1", Outcome::Correct)).unwrap();
        insert(&conn, &record("up", "2", Outcome::Incorrect)).unwrap();
        insert(&conn, &record("up", "3", Outcome::Unevaluable)).unwrap();
        insert(&conn, &record("down", "1", Outcome::Unevaluable)).unwrap();
        insert(&conn, &record("down", "2", Outcome::Unevaluable)).unwrap();

        let rows = rows(&conn, "s", &["mmlu".into()], &["up".into(), "down".into()]).unwrap();
        assert_eq!(rows.len(), 2);
        let up = &rows[0];
        assert_eq!(
            (up.attempted, up.scored, up.unevaluable, up.correct),
            (3, 2, 1, 1)
        );
        assert_eq!(up.accuracy(), Some(0.5));
        assert_eq!(up.tokens_per_sec, Some(20.0));
        let down = &rows[1];
        assert_eq!((down.attempted, down.scored, down.unevaluable), (2, 0, 2));
        assert_eq!(down.accuracy(), None, "no answers is not a score of zero");
        assert_eq!(down.reasons.get("unreachable"), Some(&2));

        // `passed` in the shared runs table carries the third state too.
        let nulls: i64 = conn
            .query_row("SELECT COUNT(*) FROM runs WHERE passed IS NULL", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(nulls, 3);

        let text = render("s", &rows);
        let down_line = text.lines().find(|l| l.contains("| down")).unwrap();
        assert!(down_line.contains("n/a"), "{down_line}");
        assert!(!down_line.contains("0.0%"), "{down_line}");
        assert!(text.contains("down: 2 unreachable") || text.contains("on down: 2 unreachable"));
        assert!(text.contains("contamination"));
    }

    #[test]
    fn a_retried_answer_is_scored_but_kept_out_of_the_timings() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::runner::init_db(&dir.path().join("t.db")).unwrap();
        init_schema(&conn).unwrap();
        let mut slow = record("up", "1", Outcome::Correct);
        slow.sends = 3;
        slow.latency_ms = 60_000;
        insert(&conn, &slow).unwrap();
        let rows = rows(&conn, "s", &["mmlu".into()], &["up".into()]).unwrap();
        assert_eq!(rows[0].scored, 1);
        assert_eq!(rows[0].latency_ms, None);
    }
}
