//! OS-level sandbox modes for the agent's shell commands.
//!
//! The `run_command` denylist and the approval presets decide what the model
//! is allowed to ask for. Neither stops a command that was allowed from
//! reading `~/.ssh` or opening a socket: a `cat ~/.ssh/id_rsa` the person
//! approved is still a `cat ~/.ssh/id_rsa`. This module wraps the command in
//! [anthropics/sandbox-runtime](https://github.com/anthropics/sandbox-runtime)
//! (`srt`), which enforces filesystem and network restrictions on the process
//! itself, so leaving zorp working is safe in a way an allow-list of command
//! shapes can never be.
//!
//! Three rules hold this up and none is negotiable.
//!
//! **A mode narrows and never loosens.** The config `srt` runs under is
//! generated here, per run, from the mode. It is never read from a file the
//! model can write, for the same reason `base_url` and `provider` are
//! privileged (ADR 0123): a workspace file that could widen the sandbox would
//! be a sandbox the sandboxed process controls. The denylist and the approval
//! gate still apply inside the sandbox; this is a second wall behind them, not
//! a replacement for either.
//!
//! **Asked for and missing is loud.** If a mode other than `off` is requested
//! and `srt` (or, on Linux, `bwrap`, or `ripgrep` anywhere) is not on `PATH`,
//! the command is refused with a sentence naming what is missing. It is never
//! quietly run unsandboxed: a person who asked for a wall and got none, and
//! was not told, is worse off than one who got an error.
//!
//! **The agent cannot call zorp's own server from inside the sandbox**
//! (ADR 0053). The network is denied by default, and the one mode that opens
//! it, `build`, opens it only to named package registries, so loopback to
//! `zorp-web` stays denied there too. The in-code own-server deny still runs
//! regardless.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The environment variable that selects the mode when no flag is passed.
pub const SANDBOX_VAR: &str = "ZORP_SANDBOX";

/// What the agent's shell commands run under.
///
/// Ordered loosest to strictest, which `PartialOrd`/`Ord` derive follows:
/// `Off < Build < Workspace < ReadOnly`. `Build` opens the network to
/// registries where `Workspace` denies it, so `Build` is the looser of the
/// two; `ReadOnly` denies writes entirely, so it is strictest. The order is
/// what lets a flavor tighten the sandbox freely and loosen it only once
/// trusted (see `Flavor::sandbox_loosens`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxMode {
    /// No sandbox. Today's behavior, and the default until this has soaked.
    #[default]
    Off,
    /// Reads allowed except credential stores, writes to the workspace and
    /// its `scratch`, and the network open only to package registries.
    Build,
    /// Reads allowed except credential stores, writes to the workspace and
    /// its `scratch`, no network.
    Workspace,
    /// No writes and no network, for panel and ensemble reviewers, which
    /// already get read-only tools.
    ReadOnly,
}

impl SandboxMode {
    /// The spelling used on the CLI, in settings, and on the wire. The
    /// inverse of `parse`.
    pub fn as_str(self) -> &'static str {
        match self {
            SandboxMode::Off => "off",
            SandboxMode::Build => "build",
            SandboxMode::Workspace => "workspace",
            SandboxMode::ReadOnly => "read-only",
        }
    }

    /// Parse a mode from a flag, the environment, a flavor, or the settings
    /// file. `readonly` is accepted as a spelling of `read-only` because it
    /// is the one a person reaches for without thinking.
    pub fn parse(value: &str) -> Result<SandboxMode, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => Ok(SandboxMode::Off),
            "build" => Ok(SandboxMode::Build),
            "workspace" => Ok(SandboxMode::Workspace),
            "read-only" | "readonly" => Ok(SandboxMode::ReadOnly),
            other => Err(format!(
                "{other:?} is not a sandbox mode; use off, workspace, build or read-only"
            )),
        }
    }

    /// Whether this mode runs commands under `srt` at all.
    pub fn is_off(self) -> bool {
        matches!(self, SandboxMode::Off)
    }
}

impl std::fmt::Display for SandboxMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The package registries `build` opens the network to, and nothing else.
///
/// The four named in the issue: crates.io, npm, PyPI and GitHub. Each is
/// spelled out as the hosts a real build actually contacts, because an
/// allowlist that names `crates.io` but not `static.crates.io` is an
/// allowlist that fails the first `cargo build`. Loopback is deliberately
/// not here, so `build` cannot reach `zorp-web` any more than the other
/// modes can.
const REGISTRY_DOMAINS: &[&str] = &[
    // crates.io
    "crates.io",
    "index.crates.io",
    "static.crates.io",
    // npm
    "registry.npmjs.org",
    // PyPI
    "pypi.org",
    "files.pythonhosted.org",
    // GitHub (sources, release tarballs, the API, raw files)
    "github.com",
    "api.github.com",
    "codeload.github.com",
    "objects.githubusercontent.com",
    "raw.githubusercontent.com",
];

/// Credential stores that are never readable inside a sandbox, relative to
/// the home directory. A read mode allows reads by default, so these are the
/// exceptions that keep "reads allowed" from meaning "reads your keys".
const CREDENTIAL_DIRS: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".kube",
    ".config/gcloud",
    ".config/gh",
    ".docker",
    ".netrc",
    // macOS keychains live under the login library.
    "Library/Keychains",
];

/// Resolve the effective mode from the config chain: flag, then the
/// environment, then the flavor, then the saved settings, then `off`.
///
/// This is the same order the rest of the settings resolve in (ADR 0119),
/// which is also what keeps the sandbox from being widened by a file the
/// model can write: the flavor sits below the flag and the environment, so a
/// flavor can never override a mode a person set there, and a project
/// flavor's loosening sandbox has already been dropped by the trust gate
/// before it reaches this `flavor` argument. The one a flavor can still carry
/// is a tightening, which is allowed to win over the saved default on
/// purpose.
pub fn resolve(
    flag: Option<&str>,
    env: Option<&str>,
    flavor: Option<SandboxMode>,
    saved: Option<&str>,
) -> Result<SandboxMode, String> {
    let named = |v: Option<&str>| {
        v.map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    if let Some(v) = named(flag) {
        return SandboxMode::parse(&v);
    }
    if let Some(v) = named(env) {
        return SandboxMode::parse(&v);
    }
    if let Some(mode) = flavor {
        return Ok(mode);
    }
    if let Some(v) = named(saved) {
        return SandboxMode::parse(&v);
    }
    Ok(SandboxMode::Off)
}

/// The `srt` settings for a mode, as the JSON `srt --settings` reads.
///
/// `workspace` is the directory writes are confined to, with `scratch`
/// under it named explicitly so the sentence in the system prompt stays
/// true. Returns `None` for `Off`, which never generates settings because it
/// never runs `srt`.
pub fn settings_json(mode: SandboxMode, workspace: &Path) -> Option<serde_json::Value> {
    if mode.is_off() {
        return None;
    }
    let deny_read: Vec<String> = credential_paths()
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let (allow_write, allowed_domains): (Vec<String>, Vec<String>) = match mode {
        SandboxMode::Off => unreachable!("handled above"),
        SandboxMode::ReadOnly => (Vec::new(), Vec::new()),
        SandboxMode::Workspace => (workspace_writes(workspace), Vec::new()),
        SandboxMode::Build => (
            workspace_writes(workspace),
            REGISTRY_DOMAINS.iter().map(|d| d.to_string()).collect(),
        ),
    };
    Some(serde_json::json!({
        "filesystem": {
            // Writes are denied by default; this is the whole of what a mode
            // opens. Reads are allowed by default, minus the credential
            // stores named here.
            "allowWrite": allow_write,
            "denyRead": deny_read,
        },
        "network": {
            // Denied by default. `build` names the registries and nothing
            // else; every other mode leaves this empty, which denies all of
            // it, loopback to zorp-web included.
            "allowedDomains": allowed_domains,
        },
    }))
}

/// The directories a workspace mode may write to: the workspace itself and
/// its `scratch`, which is where generated files go.
fn workspace_writes(workspace: &Path) -> Vec<String> {
    vec![
        workspace.to_string_lossy().into_owned(),
        workspace.join("scratch").to_string_lossy().into_owned(),
    ]
}

/// The absolute credential paths under this user's home, skipping the list
/// entirely when there is no home to anchor it to.
fn credential_paths() -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    CREDENTIAL_DIRS.iter().map(|rel| home.join(rel)).collect()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// Whether `srt` and everything it needs is present, and the version when it
/// is. Observed by running the real binaries, the way the endpoint probe
/// calls the real endpoint, so `doctor` and `/api/capabilities` cannot report
/// a sandbox that would not actually run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SrtStatus {
    /// `srt` answered `--version`.
    pub present: bool,
    /// The reported version, when `srt` is present.
    pub version: Option<String>,
    /// The tools a sandboxed run needs that were not found: any of `srt`,
    /// `bwrap` (Linux only) and `ripgrep`. Empty when a sandbox could run.
    pub missing: Vec<String>,
}

impl SrtStatus {
    /// Whether a non-`off` mode could run right now: `srt` is present and
    /// nothing it needs is missing.
    pub fn usable(&self) -> bool {
        self.present && self.missing.is_empty()
    }

    /// One line for `doctor` and tooltips.
    pub fn detail(&self) -> String {
        if self.usable() {
            match &self.version {
                Some(v) => format!("srt {v}"),
                None => "srt present".to_string(),
            }
        } else {
            format!("missing: {}", self.missing.join(", "))
        }
    }
}

/// Look for `srt`, `ripgrep`, and on Linux `bwrap`, by running them.
///
/// `srt --version` rather than a `PATH` walk, because an `srt` that is on
/// `PATH` but cannot run is not an `srt` a mode can use, and the version is
/// worth reporting. `rg --version` and `bwrap --version` the same way. The
/// probe is cheap and only runs on a `doctor` or a capabilities request.
pub fn detect() -> SrtStatus {
    let version = tool_version("srt");
    let present = version.is_some();
    let mut missing = Vec::new();
    if !present {
        missing.push("srt".to_string());
    }
    if tool_version("rg").is_none() {
        missing.push("ripgrep".to_string());
    }
    // bubblewrap is how srt confines a process on Linux. macOS uses
    // sandbox-exec, which ships with the OS, so there is nothing to detect
    // there.
    if cfg!(target_os = "linux") && tool_version("bwrap").is_none() {
        missing.push("bwrap".to_string());
    }
    SrtStatus {
        present,
        version,
        missing,
    }
}

/// Run `<tool> --version` and return its first line of output, or `None` if
/// the tool is not there or did not answer cleanly.
fn tool_version(tool: &str) -> Option<String> {
    let output = Command::new(tool).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.lines().next().unwrap_or("").trim().to_string())
}

/// The program and arguments that run `command` under `srt` with the given
/// settings file, as `srt --settings <file> -- <shell> -c <command>`.
///
/// Returned as a vector so the caller builds one `Command` around it inside
/// the existing `ProcessGroup`: `srt` is wrapped by the group, it does not
/// replace it.
pub fn wrap(settings_file: &Path, shell: &Path, command: &str) -> Vec<OsString> {
    vec![
        OsString::from("--settings"),
        settings_file.as_os_str().to_owned(),
        OsString::from("--"),
        shell.as_os_str().to_owned(),
        OsString::from("-c"),
        OsString::from(command),
    ]
}

/// The program `wrap`'s arguments are passed to.
pub const SRT_BIN: &str = "srt";

/// Turn `srt`'s own refusal into a denial that names the rule, the way a
/// denylist refusal does (ADR 0095). Returns `None` when the output does not
/// look like a sandbox refusal, so a real command error is left alone.
pub fn denial_from_output(mode: SandboxMode, stderr: &str) -> Option<String> {
    let lowered = stderr.to_ascii_lowercase();
    if lowered.contains("blocked by network allowlist")
        || (lowered.contains("network") && lowered.contains("denied"))
    {
        return Some(format!(
            "sandbox ({mode}) denied network access: the network is denied unless the \
             build mode's registry allowlist covers the host"
        ));
    }
    if lowered.contains("operation not permitted")
        || lowered.contains("permission denied")
        || lowered.contains("read-only file system")
    {
        return Some(format!(
            "sandbox ({mode}) denied a filesystem operation: writes are allowed only under \
             the workspace, and credential stores are not readable"
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // HOME is process-global; credential paths read it, so tests that set it
    // take turns.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn modes_parse_and_round_trip() {
        for (text, mode) in [
            ("off", SandboxMode::Off),
            ("none", SandboxMode::Off),
            ("build", SandboxMode::Build),
            ("workspace", SandboxMode::Workspace),
            ("read-only", SandboxMode::ReadOnly),
            ("readonly", SandboxMode::ReadOnly),
            ("  Workspace ", SandboxMode::Workspace),
        ] {
            assert_eq!(SandboxMode::parse(text).unwrap(), mode, "{text}");
        }
        // The canonical spelling round trips through `as_str`.
        for mode in [
            SandboxMode::Off,
            SandboxMode::Build,
            SandboxMode::Workspace,
            SandboxMode::ReadOnly,
        ] {
            assert_eq!(SandboxMode::parse(mode.as_str()).unwrap(), mode);
        }
        assert!(SandboxMode::parse("loose").is_err());
    }

    /// The ordering is what lets a flavor tighten freely and loosen only once
    /// trusted, so it is pinned rather than left to the derive.
    #[test]
    fn strictness_runs_from_off_to_read_only() {
        assert!(SandboxMode::Off < SandboxMode::Build);
        assert!(SandboxMode::Build < SandboxMode::Workspace);
        assert!(SandboxMode::Workspace < SandboxMode::ReadOnly);
    }

    #[test]
    fn off_generates_no_settings() {
        assert!(settings_json(SandboxMode::Off, std::path::Path::new("/ws")).is_none());
    }

    #[test]
    fn workspace_writes_the_workspace_and_scratch_and_opens_no_network() {
        let ws = std::path::Path::new("/ws");
        let json = settings_json(SandboxMode::Workspace, ws).unwrap();
        let writes = json["filesystem"]["allowWrite"].as_array().unwrap();
        let writes: Vec<String> = writes
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        // Built with `join` so the separator matches the platform the test
        // runs on; the Windows sandbox job runs these too.
        let scratch = ws.join("scratch").to_string_lossy().into_owned();
        assert!(
            writes.contains(&ws.to_string_lossy().into_owned()),
            "{writes:?}"
        );
        assert!(writes.contains(&scratch), "{writes:?}");
        assert!(json["network"]["allowedDomains"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn read_only_writes_nothing_and_opens_no_network() {
        let json = settings_json(SandboxMode::ReadOnly, std::path::Path::new("/ws")).unwrap();
        assert!(json["filesystem"]["allowWrite"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(json["network"]["allowedDomains"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn build_opens_registries_and_never_loopback() {
        let json = settings_json(SandboxMode::Build, std::path::Path::new("/ws")).unwrap();
        let domains = json["network"]["allowedDomains"].as_array().unwrap();
        let domains: Vec<&str> = domains.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(domains.contains(&"crates.io"), "{domains:?}");
        assert!(domains.contains(&"registry.npmjs.org"), "{domains:?}");
        assert!(domains.contains(&"pypi.org"), "{domains:?}");
        assert!(domains.contains(&"github.com"), "{domains:?}");
        // ADR 0053: the agent cannot call zorp's own server from the sandbox.
        // Even the one mode that opens the network must not allow loopback.
        for d in &domains {
            assert!(!d.contains("localhost"), "{domains:?}");
            assert!(!d.contains("127.0.0.1"), "{domains:?}");
        }
    }

    #[test]
    fn credential_stores_are_unreadable_when_there_is_a_home() {
        let _lock = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", "/home/someone");
        let json = settings_json(SandboxMode::Workspace, std::path::Path::new("/ws")).unwrap();
        let deny: Vec<String> = json["filesystem"]["denyRead"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        // `join` so the expected separator matches the platform under test.
        let expect = |rel: &str| {
            std::path::Path::new("/home/someone")
                .join(rel)
                .to_string_lossy()
                .into_owned()
        };
        assert!(deny.contains(&expect(".ssh")), "{deny:?}");
        assert!(deny.contains(&expect(".aws")), "{deny:?}");
        match previous {
            Some(p) => std::env::set_var("HOME", p),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn resolve_follows_the_config_chain() {
        // Flag wins over everything.
        assert_eq!(
            resolve(
                Some("build"),
                Some("off"),
                Some(SandboxMode::ReadOnly),
                Some("workspace")
            )
            .unwrap(),
            SandboxMode::Build
        );
        // Then the environment.
        assert_eq!(
            resolve(
                None,
                Some("read-only"),
                Some(SandboxMode::Off),
                Some("workspace")
            )
            .unwrap(),
            SandboxMode::ReadOnly
        );
        // Then the flavor.
        assert_eq!(
            resolve(None, None, Some(SandboxMode::Workspace), Some("off")).unwrap(),
            SandboxMode::Workspace
        );
        // Then the saved setting.
        assert_eq!(
            resolve(None, None, None, Some("build")).unwrap(),
            SandboxMode::Build
        );
        // Then off.
        assert_eq!(resolve(None, None, None, None).unwrap(), SandboxMode::Off);
        // Blank strings are skipped, not parsed as empty.
        assert_eq!(
            resolve(Some("  "), None, None, Some("workspace")).unwrap(),
            SandboxMode::Workspace
        );
        // A bad value is an error, not a silent fall-through.
        assert!(resolve(Some("loose"), None, None, None).is_err());
    }

    #[test]
    fn wrap_builds_the_srt_invocation() {
        let argv = wrap(
            std::path::Path::new("/tmp/s.json"),
            std::path::Path::new("/bin/sh"),
            "echo hi",
        );
        let argv: Vec<String> = argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            argv,
            vec![
                "--settings",
                "/tmp/s.json",
                "--",
                "/bin/sh",
                "-c",
                "echo hi"
            ]
        );
    }

    #[test]
    fn denials_name_the_rule_and_leave_ordinary_errors_alone() {
        assert!(
            denial_from_output(SandboxMode::Build, "blocked by network allowlist")
                .unwrap()
                .contains("network")
        );
        assert!(
            denial_from_output(SandboxMode::Workspace, "open: Operation not permitted")
                .unwrap()
                .contains("filesystem")
        );
        // A plain command failure is not a sandbox refusal.
        assert!(denial_from_output(SandboxMode::Workspace, "No such file or directory").is_none());
    }
}
