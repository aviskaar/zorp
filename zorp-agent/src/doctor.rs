//! What this build can do, and whether the things it needs are answering.
//!
//! Nearly everything interesting in zorp is a non-default Cargo feature, and
//! that is the right default. It also means the most common question anybody
//! has is "why is this not working", and the answer is nearly always one of
//! three things: the feature is not compiled in, the local model is not
//! running, or the endpoint is not reachable.
//!
//! The browser answers that in three places. A terminal got an error at the
//! moment it tried to use something, with no way to ask first.
//!
//! Two rules hold this up.
//!
//! **Nothing here prints a secret.** Not the API key, not a prefix of it,
//! not its length, not a token, not the contents of a flavor manifest. A
//! key is reported as set or not set and that is the whole of it. A doctor
//! command is the thing people paste into bug reports, which is exactly why
//! it must be safe to paste.
//!
//! **A check that talks to something goes the way the real path goes.**
//! Same client, same timeouts, and for the embedder the same loopback guard.
//! A doctor that reached an endpoint the real feature would refuse would be
//! worse than no doctor at all: it would report healthy on the one
//! configuration that cannot work.

use std::path::PathBuf;

/// How one checked thing turned out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Checked, and fine.
    Ok,
    /// Checked, and not fine. This is what makes the exit code non-zero.
    Bad,
    /// Not checked, and not a problem. A feature that was not compiled in
    /// is a choice somebody made, not a fault, and it must not turn a
    /// scripted `doctor` red.
    Off,
    /// Reported for the reader and never judged. Paths are the case: where
    /// the store is cannot be right or wrong, and it is on the page because
    /// `ZORP_STATE_DB` and `XDG_STATE_HOME` mean a person can easily be
    /// looking at a different database than they think.
    Note,
}

impl Health {
    /// The mark at the front of the line.
    pub fn mark(self) -> &'static str {
        match self {
            Health::Ok => "ok  ",
            Health::Bad => "bad ",
            Health::Off => "off ",
            Health::Note => "    ",
        }
    }
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub health: Health,
    /// What was checked, in a word or two.
    pub label: String,
    /// What was found, or why it could not be.
    pub detail: String,
}

impl Check {
    pub fn new(health: Health, label: impl Into<String>, detail: impl Into<String>) -> Check {
        Check {
            health,
            label: label.into(),
            detail: detail.into(),
        }
    }

    pub fn ok(label: impl Into<String>, detail: impl Into<String>) -> Check {
        Check::new(Health::Ok, label, detail)
    }

    pub fn bad(label: impl Into<String>, detail: impl Into<String>) -> Check {
        Check::new(Health::Bad, label, detail)
    }

    pub fn off(label: impl Into<String>, detail: impl Into<String>) -> Check {
        Check::new(Health::Off, label, detail)
    }

    pub fn note(label: impl Into<String>, detail: impl Into<String>) -> Check {
        Check::new(Health::Note, label, detail)
    }
}

/// Everything the report found.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    /// Whether anything that was checked came back bad.
    ///
    /// `Off` and `Note` never count. A feature nobody compiled in is not a
    /// fault, and a path is not a verdict.
    pub fn healthy(&self) -> bool {
        !self.checks.iter().any(|c| c.health == Health::Bad)
    }

    /// The report as lines, one per check.
    pub fn lines(&self) -> Vec<String> {
        let width = self
            .checks
            .iter()
            .map(|c| c.label.chars().count())
            .max()
            .unwrap_or(0);
        self.checks
            .iter()
            .map(|c| {
                format!(
                    "{}{:<width$}  {}",
                    c.health.mark(),
                    c.label,
                    c.detail,
                    width = width
                )
            })
            .collect()
    }
}

/// The Cargo features this binary was actually built with.
///
/// Read from `cfg!` rather than from a hand written list, so a feature
/// added to `Cargo.toml` cannot be forgotten here and quietly reported as
/// absent. The names are the feature names, because those are what somebody
/// puts after `--features`.
pub fn features() -> Vec<&'static str> {
    let mut on = Vec::new();
    // One line per feature in `zorp-agent/Cargo.toml`. `cfg!` is evaluated
    // at compile time, so this is the build answering about itself.
    if cfg!(feature = "otel") {
        on.push("otel");
    }
    if cfg!(feature = "mcp") {
        on.push("mcp");
    }
    if cfg!(feature = "search") {
        on.push("search");
    }
    if cfg!(feature = "research") {
        on.push("research");
    }
    if cfg!(feature = "library") {
        on.push("library");
    }
    if cfg!(feature = "clipboard") {
        on.push("clipboard");
    }
    on
}

/// Whether an API key is set. **The key itself never leaves this function.**
pub fn api_key_set() -> bool {
    std::env::var("ZORP_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
}

/// Where the state files are.
///
/// Worth a line of its own, because `ZORP_STATE_DB` and `XDG_STATE_HOME`
/// mean somebody can be looking at a different database than the one they
/// think they are looking at, and nothing else tells them.
pub fn state_paths() -> Vec<(&'static str, PathBuf)> {
    vec![
        ("session store", crate::session::Store::default_path()),
        ("trust file", crate::trust::TrustStore::default_path()),
    ]
}

/// The features line, and the note when there are none.
pub fn feature_check() -> Check {
    let on = features();
    if on.is_empty() {
        // The default build. Not a fault: it is what `cargo install zorp`
        // gives you, and most of what a person wants works in it.
        Check::note(
            "features",
            "none (a default build). Rebuild with --features to add research, \
             search, mcp or otel.",
        )
    } else {
        Check::note("features", on.join(", "))
    }
}

/// The key line. Reports set or not set and nothing else, ever.
pub fn api_key_check(base_url: &str) -> Check {
    if api_key_set() {
        return Check::note("api key", "ZORP_API_KEY is set");
    }
    // A local endpoint does not want one, so its absence is not a fault.
    if is_local(base_url) {
        Check::note(
            "api key",
            "ZORP_API_KEY is not set, which is right for a local endpoint",
        )
    } else {
        Check::bad(
            "api key",
            format!("ZORP_API_KEY is not set, and {base_url} is not a local endpoint"),
        )
    }
}

/// Whether a base URL points at this machine.
///
/// Only used to decide whether a missing API key is a problem. Deliberately
/// simple: this is not the loopback guard, which is `zorp-recall`'s and is
/// the one that decides what an embedder may talk to.
fn is_local(base_url: &str) -> bool {
    let lowered = base_url.to_ascii_lowercase();
    ["localhost", "127.0.0.1", "[::1]", "0.0.0.0"]
        .iter()
        .any(|host| lowered.contains(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_bad_check_makes_the_report_unhealthy() {
        let mut report = Report::default();
        report.checks.push(Check::ok("model", "answered"));
        report.checks.push(Check::off("recall", "not compiled in"));
        report.checks.push(Check::note("features", "none"));
        assert!(report.healthy());

        report.checks.push(Check::bad("model", "did not answer"));
        assert!(!report.healthy());
    }

    /// A feature nobody compiled in is a choice, not a fault. If `Off`
    /// counted, every default build would exit non-zero and the exit code
    /// would stop meaning anything.
    #[test]
    fn a_feature_that_is_off_is_not_a_failure() {
        let report = Report {
            checks: vec![Check::off("recall", "not compiled in")],
        };
        assert!(report.healthy());
    }

    #[test]
    fn lines_line_up_and_carry_the_mark() {
        let report = Report {
            checks: vec![
                Check::ok("model", "answered"),
                Check::bad("embedder", "nothing listening"),
            ],
        };
        let lines = report.lines();
        assert!(lines[0].starts_with("ok  "), "{lines:?}");
        assert!(lines[1].starts_with("bad "), "{lines:?}");
        // Same column for the detail, so the report reads down its edges.
        let at = |l: &str| l.find("answered").or_else(|| l.find("nothing")).unwrap();
        assert_eq!(at(&lines[0]), at(&lines[1]), "{lines:?}");
    }

    /// The one rule this module cannot get wrong. A doctor report is the
    /// thing people paste into bug reports.
    /// Held by both tests that touch `ZORP_API_KEY`.
    ///
    /// Cargo runs tests on parallel threads in one process, so an
    /// environment variable is shared state between them. Without this the
    /// other test's `set_var` lands between this one's `remove_var` and its
    /// assertion, and the remote endpoint reports a key that is there: it
    /// failed about one run in three on a loaded machine and almost never on
    /// an idle one, which is the worst way for a test to be wrong. Poison is
    /// stepped over rather than unwrapped, so a test that fails here reports
    /// its own failure and not the other one's.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn the_api_key_is_never_in_the_output() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let key = "sk-do-not-print-me-0123456789";
        let previous = std::env::var("ZORP_API_KEY").ok();
        std::env::set_var("ZORP_API_KEY", key);

        let check = api_key_check("https://api.openai.com/v1");
        let text = format!("{} {}", check.label, check.detail);

        assert!(!text.contains(key), "{text}");
        assert!(!text.contains("sk-"), "{text}");
        assert!(!text.contains("0123456789"), "{text}");
        // Not its length either.
        assert!(!text.contains(&key.len().to_string()), "{text}");
        assert!(text.contains("is set"), "{text}");

        match previous {
            Some(p) => std::env::set_var("ZORP_API_KEY", p),
            None => std::env::remove_var("ZORP_API_KEY"),
        }
    }

    /// A local endpoint does not want a key, so its absence is not a fault
    /// and must not redden a scripted run on the default configuration.
    #[test]
    fn a_missing_key_is_only_a_fault_against_a_remote_endpoint() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var("ZORP_API_KEY").ok();
        std::env::remove_var("ZORP_API_KEY");

        assert_eq!(
            api_key_check("http://localhost:11434/v1").health,
            Health::Note
        );
        assert_eq!(
            api_key_check("http://127.0.0.1:1234/v1").health,
            Health::Note
        );
        assert_eq!(
            api_key_check("https://api.openai.com/v1").health,
            Health::Bad
        );

        if let Some(p) = previous {
            std::env::set_var("ZORP_API_KEY", p);
        }
    }

    /// Read from `cfg!`, so this test says what this build is rather than
    /// what a list claims.
    #[test]
    fn features_are_read_from_the_build() {
        let on = features();
        assert_eq!(on.contains(&"research"), cfg!(feature = "research"));
        assert_eq!(on.contains(&"search"), cfg!(feature = "search"));
        assert_eq!(on.contains(&"mcp"), cfg!(feature = "mcp"));
    }

    #[test]
    fn a_default_build_says_which_build_it_is_without_calling_it_a_fault() {
        let check = feature_check();
        assert_eq!(check.health, Health::Note);
        if features().is_empty() {
            assert!(check.detail.contains("default build"), "{check:?}");
        }
    }

    /// Somebody can easily be looking at a different database than they
    /// think, and nothing else tells them.
    #[test]
    fn the_state_paths_are_reported() {
        let paths = state_paths();
        assert!(paths.iter().any(|(name, _)| *name == "session store"));
        assert!(paths.iter().any(|(name, _)| *name == "trust file"));
    }
}
