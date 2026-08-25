//! The job file: what a scheduled job is, and everything about it that is
//! checked the moment it is written rather than at 3am when it fires.

use crate::schedule::Schedule;
use crate::BoxErr;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// How much a scheduled job is allowed to do.
///
/// There is no `Full` variant, on purpose. `Preset::Full` in `zorp-agent`
/// pre-approves `run_command`, and a scheduled run has nobody watching it.
/// Leaving the variant out means no code path anywhere can produce one for
/// a job, which is a stronger guarantee than validating it away.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum JobPreset {
    /// Read files, search, answer. Every mutating tool is refused.
    #[default]
    ReadOnly,
    /// The above plus `write_file` and `apply_patch`, confined to the
    /// job's working directory by the agent's sandbox. Still no shell.
    Editor,
}

impl JobPreset {
    pub fn name(&self) -> &'static str {
        match self {
            JobPreset::ReadOnly => "read-only",
            JobPreset::Editor => "editor",
        }
    }
}

/// What to do about occurrences that went by while nothing was running.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OnMissed {
    /// One catch-up run now, however many were missed. Waking a laptop
    /// should not start six agents at once.
    #[default]
    RunOnce,
    /// Every missed occurrence in order, up to `max_catchup`.
    RunAll,
    /// Run the newest outstanding occurrence only while it is still
    /// fresh, and nothing at all once it has gone stale. For jobs where a
    /// late answer is worse than no answer. `stale_after` sets the line.
    Skip,
}

impl OnMissed {
    pub fn name(&self) -> &'static str {
        match self {
            OnMissed::RunOnce => "run-once",
            OnMissed::RunAll => "run-all",
            OnMissed::Skip => "skip",
        }
    }
}

/// One job, validated. Constructed only through `JobFile::parse`, so every
/// value in here has already been checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Job {
    pub name: String,
    pub schedule: Schedule,
    pub workdir: PathBuf,
    pub prompt: String,
    pub preset: JobPreset,
    pub on_missed: OnMissed,
    pub max_catchup: u32,
    pub requires_env: Vec<String>,
    pub max_steps: Option<usize>,
    pub timeout_secs: i64,
    /// How late an occurrence may be and still be worth running. Only
    /// `OnMissed::Skip` consults it; the other two policies run the
    /// newest outstanding occurrence however old it is.
    pub stale_after_secs: i64,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JobFile {
    pub jobs: Vec<Job>,
}

/// A run that has held its lock this long with a live process is treated
/// as wedged. Generous, because a research run can legitimately take
/// hours; a job that legitimately takes longer raises its own `timeout`.
const DEFAULT_TIMEOUT_SECS: i64 = 24 * 3600;

/// Ceiling on catch-up runs under `run-all`, so a laptop that was off for
/// a month cannot queue a month of work.
const DEFAULT_MAX_CATCHUP: u32 = 24;

/// Default freshness window for `on_missed = "skip"`. It has to be at
/// least as long as the interval the OS timer calls `run-due` on, or a
/// `skip` job would never run at all. An hour is comfortably above the
/// five minutes zorp suggests.
const DEFAULT_STALE_AFTER_SECS: i64 = 3600;

/// The TOML shape, before validation. `deny_unknown_fields` is doing real
/// work here: it is what makes `api_key = "..."` in a job file a parse
/// error rather than an ignored line. Nothing on this struct can hold a
/// credential, which is `zorp-web/src/settings.rs`'s precedent applied to
/// a file people commit.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    #[serde(default)]
    job: Vec<RawJob>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawJob {
    name: String,
    schedule: String,
    workdir: String,
    prompt: String,
    approval: Option<String>,
    on_missed: Option<String>,
    max_catchup: Option<u32>,
    requires_env: Option<Vec<String>>,
    max_steps: Option<usize>,
    timeout: Option<String>,
    stale_after: Option<String>,
    enabled: Option<bool>,
}

/// The same rule `--flavor` names follow: one normal path component. A job
/// name becomes a directory under the state root, so `../` in one would
/// write run records wherever it pointed.
fn is_valid_job_name(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(std::path::Component::Normal(_)), None)
    )
}

fn is_env_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn parse_preset(name: &str) -> Result<JobPreset, String> {
    match name {
        "read-only" | "read_only" | "readonly" => Ok(JobPreset::ReadOnly),
        "editor" => Ok(JobPreset::Editor),
        "full" => Err(
            "approval = \"full\" is not available to a scheduled job, because it \
             pre-approves run_command and a scheduled run has nobody watching it. \
             Use \"read-only\" or \"editor\""
                .to_string(),
        ),
        other => Err(format!(
            "'{other}' is not a job approval preset; use \"read-only\" or \"editor\""
        )),
    }
}

fn parse_on_missed(name: &str) -> Result<OnMissed, String> {
    match name {
        "run-once" | "run_once" => Ok(OnMissed::RunOnce),
        "run-all" | "run_all" => Ok(OnMissed::RunAll),
        "skip" => Ok(OnMissed::Skip),
        other => Err(format!(
            "'{other}' is not a missed-run policy; use \"run-once\", \"run-all\" or \"skip\""
        )),
    }
}

impl RawJob {
    fn validate(self) -> Result<Job, String> {
        let name = self.name.trim().to_string();
        if !is_valid_job_name(&name) {
            return Err(format!(
                "job name '{name}' must be a single path component, with no '/' or '..'"
            ));
        }
        // Everything below names the job, because a file with six jobs in
        // it and an error that does not say which one is barely an error.
        let blame = |message: String| format!("job '{name}': {message}");
        let schedule = Schedule::parse(&self.schedule).map_err(|e| blame(e.0))?;
        let workdir = PathBuf::from(&self.workdir);
        if !workdir.is_absolute() {
            return Err(blame(format!(
                "workdir '{}' must be an absolute path; a scheduled run has no \
                 current directory to be relative to",
                self.workdir
            )));
        }
        if self.prompt.trim().is_empty() {
            return Err(blame("prompt is empty".to_string()));
        }
        let preset = match self.approval.as_deref() {
            None => JobPreset::default(),
            Some(name) => parse_preset(name.trim()).map_err(blame)?,
        };
        let on_missed = match self.on_missed.as_deref() {
            None => OnMissed::default(),
            Some(name) => parse_on_missed(name.trim()).map_err(blame)?,
        };
        let requires_env = self.requires_env.unwrap_or_default();
        for entry in &requires_env {
            if !is_env_var_name(entry) {
                return Err(blame(format!(
                    "requires_env entry '{entry}' is not a variable name. It lists \
                     names only: a scheduled run reads values from the environment, \
                     never from this file"
                )));
            }
        }
        let timeout_secs = match self.timeout.as_deref() {
            None => DEFAULT_TIMEOUT_SECS,
            Some(text) => parse_duration(text).ok_or_else(|| {
                blame(format!(
                    "timeout '{text}' is not a duration; write it as 90s, 30m, 2h or 1d"
                ))
            })?,
        };
        let stale_after_secs = match self.stale_after.as_deref() {
            None => DEFAULT_STALE_AFTER_SECS,
            Some(text) => parse_duration(text).ok_or_else(|| {
                blame(format!(
                    "stale_after '{text}' is not a duration; write it as 90s, 30m, 2h or 1d"
                ))
            })?,
        };
        Ok(Job {
            name,
            schedule,
            workdir,
            prompt: self.prompt,
            preset,
            on_missed,
            max_catchup: self.max_catchup.unwrap_or(DEFAULT_MAX_CATCHUP),
            requires_env,
            max_steps: self.max_steps,
            timeout_secs,
            stale_after_secs,
            enabled: self.enabled.unwrap_or(true),
        })
    }
}

/// Turn a TOML error into one that names the line but never quotes it.
///
/// `toml`'s own `Display` renders the offending source line under a caret.
/// That is friendly right up to the point where the offending line is
/// `api_key = "sk-live-..."`, and the error goes to stderr, which for a
/// scheduled run means cron mail, a systemd journal, or a log file. The
/// line number is the useful half; the text of the line is the half that
/// leaks.
fn redact_toml_error(text: &str, error: toml::de::Error) -> String {
    // Counted over bytes rather than by slicing, because a span start is
    // not guaranteed to land on a character boundary and a panic here
    // would be a worse failure than a missing line number.
    let line = error.span().map(|span| {
        text.as_bytes()[..span.start.min(text.len())]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1
    });
    match line {
        Some(line) => format!("line {line}: {}", error.message()),
        None => error.message().to_string(),
    }
}

impl JobFile {
    pub fn parse(text: &str) -> Result<JobFile, BoxErr> {
        let raw: RawFile =
            toml::from_str(text).map_err(|e| BoxErr::from(redact_toml_error(text, e)))?;
        let mut jobs = Vec::with_capacity(raw.job.len());
        let mut seen = BTreeSet::new();
        for entry in raw.job {
            let job = entry.validate()?;
            if !seen.insert(job.name.clone()) {
                return Err(format!(
                    "job '{}' is defined twice; two jobs with one name would share \
                     a lock and a history file",
                    job.name
                )
                .into());
            }
            jobs.push(job);
        }
        Ok(JobFile { jobs })
    }

    /// Load a job file, if it exists. Missing file means no jobs, which is
    /// the normal state for a user who has not written any.
    pub fn load(path: &Path) -> Result<Option<JobFile>, BoxErr> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(JobFile::parse(&text)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Box::new(e)),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Job> {
        self.jobs.iter().find(|job| job.name == name)
    }
}

/// Where a job file lives. User scope only. See `docs/DECISIONS.md`
/// (2026-08-18) for why there is no project-scope job file.
pub fn job_file_path(home: &Path) -> PathBuf {
    home.join(".config").join("zorp").join("jobs.toml")
}

/// A duration written the way a person writes one: `90s`, `30m`, `2h`,
/// `1d`. Anything else is `None`, and callers turn that into a loud error.
pub fn parse_duration(text: &str) -> Option<i64> {
    let text = text.trim();
    let (count, unit) = text.split_at(text.len().checked_sub(1)?);
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return None,
    };
    let count: i64 = count.parse().ok()?;
    (count > 0).then_some(count * multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::Weekday;

    const MINIMAL: &str = r#"
[[job]]
name = "paper-refiner"
schedule = "weekly on monday at 09:00"
workdir = "/srv/papers"
prompt = "Re-read draft.md and list what is unsupported."
"#;

    #[test]
    fn a_minimal_job_parses_and_defaults_to_the_narrowest_settings() {
        let file = JobFile::parse(MINIMAL).unwrap();
        let job = file.get("paper-refiner").unwrap();
        assert_eq!(
            job.schedule,
            Schedule::Weekly {
                weekday: Weekday::Monday,
                minute_of_day: 540
            }
        );
        assert_eq!(job.workdir, PathBuf::from("/srv/papers"));
        assert!(job.enabled);
        // The three defaults that matter. Anything wider has to be typed
        // out by a human in the file.
        assert_eq!(job.preset, JobPreset::ReadOnly);
        assert_eq!(job.on_missed, OnMissed::RunOnce);
        assert_eq!(job.max_steps, None);
    }

    #[test]
    fn every_optional_field_parses() {
        let file = JobFile::parse(
            r#"
[[job]]
name = "suggestor"
schedule = "daily at 07:30"
workdir = "/srv/repo"
prompt = "Propose three alternative angles."
approval = "editor"
on_missed = "run-all"
max_catchup = 3
requires_env = ["ZORP_API_KEY"]
max_steps = 12
timeout = "90m"
stale_after = "15m"
enabled = false
"#,
        )
        .unwrap();
        let job = file.get("suggestor").unwrap();
        assert_eq!(job.preset, JobPreset::Editor);
        assert_eq!(job.on_missed, OnMissed::RunAll);
        assert_eq!(job.max_catchup, 3);
        assert_eq!(job.requires_env, vec!["ZORP_API_KEY".to_string()]);
        assert_eq!(job.max_steps, Some(12));
        assert_eq!(job.timeout_secs, 90 * 60);
        assert_eq!(job.stale_after_secs, 15 * 60);
        assert!(!job.enabled);
    }

    /// The whole point of the feature's safety argument. A job file must
    /// not be able to ask for a preset that pre-approves the shell.
    #[test]
    fn a_job_cannot_ask_for_the_full_preset() {
        let err = JobFile::parse(
            r#"
[[job]]
name = "x"
schedule = "daily at 09:00"
workdir = "/srv/repo"
prompt = "p"
approval = "full"
"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("full"), "{err}");
        assert!(
            err.contains("read-only") && err.contains("editor"),
            "the error must name what is allowed instead: {err}"
        );
    }

    #[test]
    fn unknown_approval_names_are_rejected_rather_than_falling_back() {
        for name in ["", "admin", "readonly-ish", "READ ONLY"] {
            let text = format!(
                "[[job]]\nname=\"x\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"p\"\napproval=\"{name}\""
            );
            assert!(JobFile::parse(&text).is_err(), "{name} should be rejected");
        }
    }

    /// A job file is a document a user may well commit to a repository.
    /// It follows `zorp-web`'s settings precedent: there is no field that
    /// can carry a credential, so an attempt to add one fails to parse.
    #[test]
    fn a_job_file_cannot_carry_a_secret() {
        for key in ["api_key", "token", "password", "env", "secret"] {
            let text = format!(
                "[[job]]\nname=\"x\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"p\"\n{key}=\"sk-live-abcdef\""
            );
            let err = JobFile::parse(&text).unwrap_err().to_string();
            assert!(err.contains(key), "{key} should be refused by name: {err}");
            assert!(
                !err.contains("sk-live-abcdef"),
                "the error must not echo the value back: {err}"
            );
        }
    }

    /// `requires_env` names variables so a run can fail loudly when cron's
    /// near-empty environment is missing one. Names only: anything with a
    /// value in it is the shape a leaked secret would take.
    #[test]
    fn requires_env_takes_names_and_refuses_anything_carrying_a_value() {
        for entry in ["ZORP_API_KEY=sk-live", "ZORP API KEY", "", "lower_case=1"] {
            let text = format!(
                "[[job]]\nname=\"x\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"p\"\nrequires_env=[\"{entry}\"]"
            );
            assert!(
                JobFile::parse(&text).is_err(),
                "requires_env should refuse '{entry}'"
            );
        }
    }

    /// Redacting the offending line must not cost the line number, which
    /// is the half of a TOML error that helps.
    #[test]
    fn a_toml_error_still_says_where_it_was() {
        let err = JobFile::parse("[[job]]\nname = \"x\"\nnope = 1\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 3"), "{err}");
        assert!(err.contains("nope"), "{err}");
    }

    #[test]
    fn an_unparseable_schedule_fails_when_the_file_is_read_not_when_it_fires() {
        let err = JobFile::parse(
            r#"
[[job]]
name = "x"
schedule = "whenever"
workdir = "/srv/repo"
prompt = "p"
"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("whenever"), "{err}");
        assert!(err.contains('x'), "the error must name the job: {err}");
    }

    #[test]
    fn structural_mistakes_are_refused() {
        let cases = [
            // No name.
            "[[job]]\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"p\"",
            // Name that would escape the state directory.
            "[[job]]\nname=\"../evil\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"p\"",
            "[[job]]\nname=\"a/b\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"p\"",
            "[[job]]\nname=\"\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"p\"",
            // Relative working directory: a scheduled run has no
            // meaningful current directory to be relative to.
            "[[job]]\nname=\"x\"\nschedule=\"daily at 09:00\"\nworkdir=\"repo\"\nprompt=\"p\"",
            // Empty prompt.
            "[[job]]\nname=\"x\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"   \"",
            // Missing prompt entirely.
            "[[job]]\nname=\"x\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"",
            // Nonsense timeout.
            "[[job]]\nname=\"x\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"p\"\ntimeout=\"soon\"",
            // Nonsense freshness window.
            "[[job]]\nname=\"x\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"p\"\nstale_after=\"a while\"",
            // Nonsense missed-run policy.
            "[[job]]\nname=\"x\"\nschedule=\"daily at 09:00\"\nworkdir=\"/s\"\nprompt=\"p\"\non_missed=\"maybe\"",
        ];
        for text in cases {
            assert!(JobFile::parse(text).is_err(), "should be refused:\n{text}");
        }
    }

    #[test]
    fn duplicate_job_names_are_refused_because_they_share_state() {
        let text = format!("{MINIMAL}\n{MINIMAL}");
        let err = JobFile::parse(&text).unwrap_err().to_string();
        assert!(err.contains("paper-refiner"), "{err}");
    }

    #[test]
    fn an_empty_file_is_a_valid_file_with_no_jobs() {
        assert_eq!(JobFile::parse("").unwrap().jobs.len(), 0);
        assert!(JobFile::parse("").unwrap().get("anything").is_none());
    }

    #[test]
    fn load_returns_none_for_a_missing_file_and_errors_on_a_bad_one() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("jobs.toml");
        assert!(JobFile::load(&missing).unwrap().is_none());
        std::fs::write(&missing, "[[job]]\nname=\"x\"").unwrap();
        assert!(JobFile::load(&missing).is_err());
        std::fs::write(&missing, MINIMAL).unwrap();
        assert_eq!(JobFile::load(&missing).unwrap().unwrap().jobs.len(), 1);
    }

    #[test]
    fn the_job_file_lives_under_the_users_config_directory() {
        assert_eq!(
            job_file_path(Path::new("/home/u")),
            PathBuf::from("/home/u/.config/zorp/jobs.toml")
        );
    }

    #[test]
    fn durations_parse_in_the_units_a_person_would_write() {
        assert_eq!(parse_duration("90s"), Some(90));
        assert_eq!(parse_duration("30m"), Some(1800));
        assert_eq!(parse_duration("2h"), Some(7200));
        assert_eq!(parse_duration("1d"), Some(86_400));
        assert_eq!(parse_duration(" 2h "), Some(7200));
        for bad in ["", "2", "h", "-1h", "2w", "2 h", "0h"] {
            assert_eq!(parse_duration(bad), None, "{bad}");
        }
    }
}
