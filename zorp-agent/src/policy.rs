use crate::model::ToolCall;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    Allow,
    Ask,
    Deny(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Preset {
    ReadOnly,
    Editor,
    Full,
}

impl Preset {
    pub fn parse(name: &str) -> Option<Preset> {
        match name.trim().to_ascii_lowercase().as_str() {
            "read-only" | "read_only" | "readonly" => Some(Preset::ReadOnly),
            "editor" => Some(Preset::Editor),
            "full" => Some(Preset::Full),
            _ => None,
        }
    }
}

/// Per-operation approval policy. Reads are always allowed and unknown tools are
/// always denied; the `run_command` denylist always denies regardless of preset.
/// `mcp__`-prefixed tools (external MCP server tools, discovered at runtime and
/// not nameable ahead of time) are always `Ask` rather than a hard deny, so they
/// route through the same `ApprovalMode` gate as everything else: denied under
/// `NonInteractive`, prompted under `Interactive`, run under `AutoApprove`.
/// This `ApprovalMode` gate is currently the *only* enforcement on MCP tool
/// execution. Per-server trust (`trust = "sandbox"` vs `"trusted"` in server
/// config) is parsed but not enforced anywhere at the tool-call boundary today:
/// `TrustLevel` is stored on `McpServer` and never read again, and zorp-mcp's
/// TOFU/trust-store layer (`zorp-mcp/src/tofu.rs`, `McpTofuStore`) is only ever
/// constructed in its own test module, not on any production connect/discover/
/// call_tool path. A `sandbox`-trust server's tools get identical treatment to
/// a `trusted` server's under `AutoApprove` today. TODO: wire trust enforcement
/// into the tool-call path before treating `trust = "sandbox"` as meaningful.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Policy {
    edit: Decision,
    run: Decision,
    /// Canonicalized sandbox repo root, when known. Path checks in the
    /// denylist allow absolute targets under it. Without a root, every
    /// absolute destructive or redirect target denies (fail closed).
    repo_root: Option<PathBuf>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy::from_preset(Preset::ReadOnly)
    }
}

fn parse_decision(word: &str) -> Option<Decision> {
    match word.trim().to_ascii_lowercase().as_str() {
        "allow" => Some(Decision::Allow),
        "ask" => Some(Decision::Ask),
        "deny" => Some(Decision::Deny("denied by flavor policy".to_string())),
        _ => None,
    }
}

impl Policy {
    pub fn from_preset(preset: Preset) -> Policy {
        match preset {
            Preset::ReadOnly => Policy {
                edit: Decision::Ask,
                run: Decision::Ask,
                repo_root: None,
            },
            Preset::Editor => Policy {
                edit: Decision::Allow,
                run: Decision::Ask,
                repo_root: None,
            },
            Preset::Full => Policy {
                edit: Decision::Allow,
                run: Decision::Allow,
                repo_root: None,
            },
        }
    }

    /// Whether this policy denies the edit tools while leaving the shell
    /// open. That combination reads as "the agent cannot write" and is not:
    /// `run_command` can redirect into any path inside the repo, which the
    /// denylist permits by design. Callers surface this so the gap is seen
    /// when the policy is written, not after something got written.
    pub fn write_barrier_is_porous(&self) -> bool {
        matches!(self.edit, Decision::Deny(_)) && !matches!(self.run, Decision::Deny(_))
    }

    /// Attach the sandbox repo root so denylist path checks can allow
    /// absolute targets that stay inside it. Canonicalized once here to
    /// match `Sandbox::new`; all later checks are lexical string analysis.
    /// `build_policy` calls this for every policy it constructs, so in
    /// production an absolute target under the root is approval-gated
    /// rather than denied. A policy built without a root, as in some
    /// tests, denies every absolute destructive or redirect target.
    pub fn with_repo_root(mut self, root: impl Into<PathBuf>) -> Policy {
        let root = root.into();
        self.repo_root = Some(root.canonicalize().unwrap_or(root));
        self
    }

    /// Apply one `[approval]` override key. Unknown operations or decisions are
    /// ignored (a manifest typo cannot silently loosen policy).
    pub fn with_override(mut self, op: &str, decision: &str) -> Policy {
        let Some(decision) = parse_decision(decision) else {
            return self;
        };
        match op.trim().to_ascii_lowercase().as_str() {
            "write_file" | "apply_patch" | "edit" => self.edit = decision,
            "run_command" => self.run = decision,
            _ => {}
        }
        self
    }

    pub fn decide(&self, call: &ToolCall) -> Decision {
        match call.name.as_str() {
            "read_file"
            | "list_files"
            | "search_text"
            | "git_diff"
            | "git_status"
            | "search_notes"
            | "list_background_processes"
            | "monitor_subagents" => Decision::Allow,
            "write_file" | "apply_patch" | "take_note" => self.edit.clone(),
            "run_command"
            | "start_background_process"
            | "kill_background_process"
            | "spawn_subagent"
            | "cancel_subagent" => {
                let command = call
                    .arguments
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if let Some(reason) = deny_reason(command, self.repo_root.as_deref()) {
                    Decision::Deny(reason)
                } else {
                    self.run.clone()
                }
            }
            name if name.starts_with("mcp__") => Decision::Ask,
            _ => Decision::Deny(format!(
                "tool '{}' is not permitted by the built-in policy",
                call.name
            )),
        }
    }
}

fn deny_reason(command: &str, repo_root: Option<&Path>) -> Option<String> {
    let normalized = command.to_ascii_lowercase();
    let denied = "command matches the hard denylist".to_string();
    if normalized.contains('`') {
        return Some(denied);
    }
    // Command and process substitution bodies run whatever they contain,
    // so each body is analyzed like an `sh -c` payload. Unbalanced
    // substitution syntax fails closed.
    match substitution_bodies(&normalized) {
        Err(()) => return Some(denied),
        Ok(bodies) => {
            for body in bodies {
                if deny_reason(&body, repo_root).is_some() {
                    return Some(denied);
                }
            }
        }
    }
    let forbidden = tokenize_command(&normalized)
        .map(|segments| {
            segments
                .iter()
                .any(|words| segment_is_forbidden(words, repo_root))
        })
        .unwrap_or(true);
    forbidden.then_some(denied)
}

/// Collect the bodies of `$(...)`, `<(...)`, and `>(...)` substitutions.
/// Nested substitutions inside a body are handled by the recursive
/// `deny_reason` call on that body. Returns `Err` on unbalanced syntax.
fn substitution_bodies(command: &str) -> Result<Vec<String>, ()> {
    let chars: Vec<char> = command.chars().collect();
    let mut bodies = Vec::new();
    let mut index = 0;
    while index + 1 < chars.len() {
        let opens = matches!(chars[index], '$' | '<' | '>') && chars[index + 1] == '(';
        if !opens {
            index += 1;
            continue;
        }
        let mut depth = 1usize;
        let mut end = index + 2;
        while end < chars.len() && depth > 0 {
            match chars[end] {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            end += 1;
        }
        if depth != 0 {
            return Err(());
        }
        bodies.push(chars[index + 2..end - 1].iter().collect());
        index = end;
    }
    Ok(bodies)
}

fn segment_is_forbidden(words: &[String], repo_root: Option<&Path>) -> bool {
    if redirects_escape_repo(words, repo_root) {
        return true;
    }
    let Some(words) = unwrap_common_wrappers(words) else {
        return true;
    };
    if words.is_empty() {
        return false;
    }
    let executable = executable_name(&words[0]);
    if executable == "eval" {
        return true;
    }
    if matches!(executable, "sh" | "bash" | "zsh") {
        if let Some(index) = words
            .iter()
            .position(|word| word == "-c" || (word.starts_with('-') && word[1..].contains('c')))
        {
            let Some(payload) = words.get(index + 1) else {
                return true;
            };
            return deny_reason(payload, repo_root).is_some();
        }
    }
    let destructive_rm = executable == "rm"
        && rm_has_recursive_force(words)
        && words
            .iter()
            .skip(1)
            .filter(|word| !word.starts_with('-'))
            .any(|word| path_escapes_repo(word, repo_root));
    executable == "sudo"
        || destructive_rm
        || executable.starts_with("mkfs")
        || executable == "fdisk"
        || (executable == "diskutil" && words.iter().any(|word| word == "erasedisk"))
        || git_subcommand(words) == Some("push")
}

/// Recursive plus force in any spelling: `-rf`, `-fr`, `-r -f`, combined
/// short flags, or the long forms. Input is already lowercased, so `-R`
/// arrives as `-r`.
fn rm_has_recursive_force(words: &[String]) -> bool {
    let has_flag = |short: char, long: &str| {
        words.iter().any(|word| {
            word == long
                || (word.starts_with('-') && !word.starts_with("--") && word[1..].contains(short))
        })
    };
    has_flag('r', "--recursive") && has_flag('f', "--force")
}

/// Deny redirects whose target lies outside the repo. Redirect operators
/// are distinct tokens after `tokenize_command`, so this walks operator
/// and target pairs. A missing target fails closed.
fn redirects_escape_repo(words: &[String], repo_root: Option<&Path>) -> bool {
    let mut index = 0;
    while index < words.len() {
        if !is_redirect_operator(&words[index]) {
            index += 1;
            continue;
        }
        let Some(target) = words.get(index + 1) else {
            return true;
        };
        if is_redirect_operator(target) || redirect_target_escapes(target, repo_root) {
            return true;
        }
        index += 2;
    }
    false
}

fn is_redirect_operator(word: &str) -> bool {
    let rest = word.trim_start_matches(|ch: char| ch.is_ascii_digit());
    !rest.is_empty()
        && rest.starts_with('>')
        && rest.chars().all(|ch| matches!(ch, '>' | '|' | '&'))
}

fn redirect_target_escapes(target: &str, repo_root: Option<&Path>) -> bool {
    // Discarding output is always fine.
    if target == "/dev/null" {
        return false;
    }
    // File descriptor duplication such as `2>&1`.
    if !target.is_empty() && target.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    // Process substitution body, already analyzed by `substitution_bodies`.
    if target.starts_with('(') {
        return false;
    }
    path_escapes_repo(target, repo_root)
}

/// Lexical check only, no filesystem access. A target escapes when it
/// starts with `~` or a variable expansion, is an absolute path not under
/// the repo root, or climbs above the current directory with `..`.
fn path_escapes_repo(target: &str, repo_root: Option<&Path>) -> bool {
    if target.starts_with('~') || target.starts_with('$') {
        return true;
    }
    if target.starts_with('/') {
        let Some(root) = repo_root else {
            return true;
        };
        return !absolute_stays_under(target, root);
    }
    relative_escapes(target)
}

fn absolute_stays_under(target: &str, root: &Path) -> bool {
    let mut parts: Vec<&str> = Vec::new();
    for component in target.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    // Climbing above the filesystem root fails closed.
                    return false;
                }
            }
            part => parts.push(part),
        }
    }
    // The command text was lowercased before tokenizing, so compare the
    // root case-insensitively as well.
    let root_parts: Vec<String> = root
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect();
    parts.len() >= root_parts.len()
        && root_parts
            .iter()
            .zip(parts.iter())
            .all(|(root_part, part)| root_part == part)
}

fn relative_escapes(target: &str) -> bool {
    let mut depth: i32 = 0;
    for component in target.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => depth += 1,
        }
    }
    false
}

fn executable_name(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn unwrap_execution_wrappers(mut words: &[String]) -> Option<&[String]> {
    while let Some(name) = words.first().map(|word| executable_name(word)) {
        if name == "exec" {
            let mut index = 1;
            while let Some(option) = words.get(index).map(String::as_str) {
                match option {
                    "--" => {
                        index += 1;
                        break;
                    }
                    "-a" => {
                        words.get(index + 1)?;
                        index += 2;
                    }
                    "-c" | "-l" => index += 1,
                    option if option.starts_with('-') => return None,
                    _ => break,
                }
            }
            words = &words[index.min(words.len())..];
            continue;
        }
        if name != "command" {
            break;
        }
        let mut index = 1;
        while words.get(index).is_some_and(|word| word.starts_with('-')) {
            index += 1;
        }
        words = &words[index.min(words.len())..];
    }
    Some(words)
}

fn unwrap_common_wrappers(mut words: &[String]) -> Option<&[String]> {
    loop {
        let previous_len = words.len();
        while words.first().is_some_and(|word| is_assignment(word)) {
            words = &words[1..];
        }
        words = unwrap_execution_wrappers(words)?;
        words = command_after_env(words);
        if words.len() == previous_len {
            return Some(words);
        }
    }
}

fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn command_after_env(words: &[String]) -> &[String] {
    if words.first().map(|word| executable_name(word)) != Some("env") {
        return words;
    }
    let mut index = 1;
    while let Some(word) = words.get(index) {
        let takes_value = matches!(
            word.as_str(),
            "-u" | "--unset" | "-c" | "--chdir" | "--argv0" | "-s" | "--split-string"
        );
        if takes_value {
            index += 2;
        } else if word.starts_with('-') || word.contains('=') {
            index += 1;
        } else {
            break;
        }
    }
    &words[index.min(words.len())..]
}

fn git_subcommand(words: &[String]) -> Option<&str> {
    if words.first().map(|word| executable_name(word)) != Some("git") {
        return None;
    }
    let mut index = 1;
    while let Some(word) = words.get(index) {
        if matches!(
            word.as_str(),
            "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--super-prefix"
        ) {
            index += 2;
        } else if word.starts_with('-') {
            index += 1;
        } else {
            return Some(word.as_str());
        }
    }
    None
}

fn tokenize_command(command: &str) -> Result<Vec<Vec<String>>, ()> {
    let mut segments = vec![Vec::new()];
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            word.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            if ch == active {
                quote = None;
            } else {
                word.push(ch);
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if ch == '>' {
            // Redirect operators become distinct tokens. A pending word of
            // digits is a file descriptor prefix, as in `2>`, and stays
            // attached to the operator.
            let mut operator = if !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_digit())
            {
                std::mem::take(&mut word)
            } else {
                if !word.is_empty() {
                    segments.last_mut().unwrap().push(std::mem::take(&mut word));
                }
                String::new()
            };
            operator.push('>');
            while let Some(next) = chars.peek() {
                if matches!(next, '>' | '|' | '&') {
                    operator.push(*next);
                    chars.next();
                } else {
                    break;
                }
            }
            segments.last_mut().unwrap().push(operator);
        } else if ch.is_whitespace() || matches!(ch, ';' | '&' | '|') {
            if !word.is_empty() {
                segments.last_mut().unwrap().push(std::mem::take(&mut word));
            }
            if matches!(ch, ';' | '&' | '|' | '\n') && !segments.last().unwrap().is_empty() {
                segments.push(Vec::new());
            }
        } else {
            word.push(ch);
        }
    }
    if escaped || quote.is_some() {
        return Err(());
    }
    if !word.is_empty() {
        segments.last_mut().unwrap().push(word);
    }
    segments.retain(|segment| !segment.is_empty());
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(name: &str, arguments: Value) -> ToolCall {
        ToolCall {
            id: "1".into(),
            name: name.into(),
            arguments,
        }
    }

    #[test]
    fn reads_are_allowed_and_mutations_ask() {
        let p = Policy::default();
        assert!(matches!(
            p.decide(&call("read_file", json!({}))),
            Decision::Allow
        ));
        assert!(matches!(
            p.decide(&call("write_file", json!({}))),
            Decision::Ask
        ));
        assert!(matches!(
            p.decide(&call("apply_patch", json!({}))),
            Decision::Ask
        ));
        assert!(matches!(
            p.decide(&call("run_command", json!({"command":"cargo test"}))),
            Decision::Ask
        ));
    }

    #[test]
    fn unknown_and_dangerous_commands_are_denied() {
        let p = Policy::default();
        assert!(matches!(
            p.decide(&call("custom", json!({}))),
            Decision::Deny(_)
        ));
        for command in [
            "sudo true",
            "rm -rf /",
            "mkfs.ext4 /dev/sda",
            "fdisk /dev/sda",
            "diskutil eraseDisk APFS X disk2",
            "git push origin main",
            "echo x > /tmp/x",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn compound_wrapped_and_split_flag_commands_are_denied() {
        let p = Policy::default();
        for command in [
            "echo ok; sudo true",
            "env git push origin main",
            "env FOO=bar git push origin main",
            "git -C repo push",
            "cd /tmp && fdisk /dev/sda",
            "rm -r -f /",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn path_qualified_and_common_wrapper_bypasses_are_denied() {
        let p = Policy::default();
        for command in [
            "/usr/bin/sudo true",
            "/usr/bin/git push origin main",
            "command sudo true",
            "command /usr/bin/git push origin main",
            "command env FOO=bar /usr/bin/git push origin main",
            "sh -c 'git push origin main'",
            "/bin/bash -c \"sudo true\"",
            "zsh -c 'echo ok; git push origin main'",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn ambiguous_shell_wrappers_are_denied_conservatively() {
        let p = Policy::default();
        for command in [
            "sh -c",
            "bash -c 'git push origin main",
            "command -- sudo true",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn assignment_variable_exec_and_eval_bypasses_are_denied() {
        let p = Policy::default();
        for command in [
            "FOO=1 /usr/bin/sudo true",
            "FOO=1 /usr/bin/git push origin main",
            "exec sudo true",
            "/usr/bin/exec /usr/bin/git push origin main",
            "eval 'git push origin main'",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn benign_leading_assignment_remains_approval_gated() {
        let p = Policy::default();
        assert!(matches!(
            p.decide(&call(
                "run_command",
                json!({"command":"RUST_LOG=debug cargo test"})
            )),
            Decision::Ask
        ));
    }

    #[test]
    fn exec_argv0_option_cannot_hide_dangerous_executable() {
        let p = Policy::default();
        for command in [
            "exec -a harmless sudo true",
            "exec -a harmless /usr/bin/git push origin main",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
        assert!(matches!(
            p.decide(&call(
                "run_command",
                json!({"command":"exec -a cargo cargo test"})
            )),
            Decision::Ask
        ));
    }

    #[test]
    fn editor_preset_allows_edits_but_still_asks_run() {
        let p = Policy::from_preset(Preset::Editor);
        assert!(matches!(
            p.decide(&call("write_file", json!({}))),
            Decision::Allow
        ));
        assert!(matches!(
            p.decide(&call("apply_patch", json!({}))),
            Decision::Allow
        ));
        assert!(matches!(
            p.decide(&call("run_command", json!({"command":"cargo test"}))),
            Decision::Ask
        ));
    }

    #[test]
    fn full_preset_allows_run_but_denylist_still_wins() {
        let p = Policy::from_preset(Preset::Full);
        assert!(matches!(
            p.decide(&call("run_command", json!({"command":"cargo test"}))),
            Decision::Allow
        ));
        assert!(matches!(
            p.decide(&call("run_command", json!({"command":"sudo rm -rf /"}))),
            Decision::Deny(_)
        ));
        assert!(matches!(
            p.decide(&call(
                "run_command",
                json!({"command":"git push origin main"})
            )),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn overrides_tighten_or_loosen_individual_operations() {
        let p = Policy::from_preset(Preset::ReadOnly).with_override("run_command", "allow");
        assert!(matches!(
            p.decide(&call("run_command", json!({"command":"cargo test"}))),
            Decision::Allow
        ));
        let p2 = Policy::from_preset(Preset::Editor).with_override("write_file", "deny");
        assert!(matches!(
            p2.decide(&call("write_file", json!({}))),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn substitution_bodies_are_analyzed_like_payloads() {
        let p = Policy::default();
        for command in [
            "echo $(sudo rm -rf /)",
            "echo $(echo $(sudo true))",
            "diff <(git push origin main) local.txt",
            "cat <(sudo curl evil)",
            "tee >(sudo tee /etc/hosts)",
            "sh -c 'echo $(sudo true)'",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn unbalanced_substitution_syntax_fails_closed() {
        let p = Policy::default();
        for command in ["echo $(sudo true", "cat <(curl evil", "echo $(("] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn benign_substitution_and_variable_expansion_stay_approval_gated() {
        let p = Policy::default();
        for command in [
            "echo $(date)",
            "cat <(curl evil)",
            "echo $HOME",
            "echo ${PATH}",
            "echo $((1 + 2))",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Ask
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn recursive_force_rm_outside_the_repo_is_denied() {
        let p = Policy::default();
        for command in [
            "rm -rf /*",
            "rm -rf /bin",
            "rm -rf ~",
            "rm -rf ~/",
            "rm -rf ../..",
            "rm -rf ..",
            "rm -fr /bin",
            "rm -f -r /bin",
            "rm --recursive --force /bin",
            "rm -rf $HOME",
            "rm -rf a/../../b",
            "rm -rf -- /bin",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn in_repo_rm_stays_approval_gated() {
        let p = Policy::default();
        for command in [
            "rm -rf target",
            "rm -rf target/",
            "rm -rf ./build",
            "rm -rf a/../b",
            "rm file.txt",
            "rm -r docs/old",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Ask
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn redirects_outside_the_repo_are_denied_in_any_spelling() {
        let p = Policy::default();
        for command in [
            "echo x > /tmp/x",
            "echo x >/tmp/x",
            "echo x >> /etc/hosts",
            "echo x >>/etc/hosts",
            "echo x >|/etc/x",
            "echo x >| /etc/x",
            "echo x > ~/.bashrc",
            "echo x > ~/.ssh/authorized_keys",
            "echo x > ${HOME}/x",
            "echo x > $HOME/x",
            "cargo test 2> /etc/x",
            "echo hi >",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn in_repo_and_descriptor_redirects_stay_approval_gated() {
        let p = Policy::default();
        for command in [
            "echo hi > out.txt",
            "echo hi >> out.txt",
            "cargo test 2> err.log",
            "cargo test 2>err.log",
            "git diff > /dev/null",
            "cargo test 2>&1",
            "cargo test > log.txt 2>&1",
            "echo hi >&2",
        ] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Ask
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn repo_root_admits_absolute_paths_under_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let p = Policy::default().with_repo_root(&root);
        let inside_rm = format!("rm -rf {}/target", root.display());
        let inside_redirect = format!("echo hi > {}/out.txt", root.display());
        for command in [inside_rm.as_str(), inside_redirect.as_str()] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Ask
                ),
                "{command}"
            );
        }
        let escape_rm = format!("rm -rf {}/../elsewhere", root.display());
        for command in ["rm -rf /bin", "echo x > /etc/hosts", escape_rm.as_str()] {
            assert!(
                matches!(
                    p.decide(&call("run_command", json!({"command":command}))),
                    Decision::Deny(_)
                ),
                "{command}"
            );
        }
    }

    #[test]
    fn preset_parse_accepts_known_names() {
        assert!(matches!(Preset::parse("read-only"), Some(Preset::ReadOnly)));
        assert!(matches!(Preset::parse("editor"), Some(Preset::Editor)));
        assert!(matches!(Preset::parse("full"), Some(Preset::Full)));
        assert!(Preset::parse("bogus").is_none());
    }

    /// Denying the edit tools while `run_command` stays open is not a write
    /// barrier: a shell redirect inside the repo writes just as well. Users
    /// who deny one write path reasonably think they denied writing.
    #[test]
    fn a_denied_edit_with_an_open_shell_is_reported_as_porous() {
        let denied_edit = Policy::default().with_override("write_file", "deny");
        assert!(denied_edit.write_barrier_is_porous());
    }

    #[test]
    fn denying_both_is_not_porous() {
        let both = Policy::default()
            .with_override("write_file", "deny")
            .with_override("run_command", "deny");
        assert!(!both.write_barrier_is_porous());
    }

    #[test]
    fn a_policy_that_denies_no_writes_claims_no_barrier() {
        assert!(!Policy::from_preset(Preset::Full).write_barrier_is_porous());
        assert!(!Policy::from_preset(Preset::ReadOnly).write_barrier_is_porous());
    }

    #[test]
    fn monitor_subagents_is_always_allowed() {
        let p = Policy::from_preset(Preset::ReadOnly);
        let call = ToolCall {
            id: "1".into(),
            name: "monitor_subagents".into(),
            arguments: serde_json::json!({}),
        };
        assert_eq!(p.decide(&call), Decision::Allow);
    }

    #[test]
    fn spawn_and_cancel_subagent_follow_the_run_decision() {
        let read_only = Policy::from_preset(Preset::ReadOnly);
        let full = Policy::from_preset(Preset::Full);
        for name in ["spawn_subagent", "cancel_subagent"] {
            let call = ToolCall {
                id: "1".into(),
                name: name.into(),
                arguments: serde_json::json!({}),
            };
            assert_eq!(read_only.decide(&call), Decision::Ask);
            assert_eq!(full.decide(&call), Decision::Allow);
        }
    }

    #[test]
    fn mcp_prefixed_tools_are_asked_not_hard_denied() {
        // Any preset: mcp__ tools route through ApprovalMode (Ask), not the
        // catch-all Deny that applies to genuinely unrecognized tool names.
        // Without this, an agent running with ApprovalMode::AutoApprove
        // (e.g. `zorp-agent validate --yes`, or this test's own use of
        // AutoApprove) could never actually invoke a discovered MCP tool.
        for preset in [Preset::ReadOnly, Preset::Editor, Preset::Full] {
            let p = Policy::from_preset(preset);
            let call = ToolCall {
                id: "1".into(),
                name: "mcp__stub__search".into(),
                arguments: serde_json::json!({}),
            };
            assert_eq!(
                p.decide(&call),
                Decision::Ask,
                "preset {preset:?} should Ask for mcp__ tools"
            );
        }
    }

    #[test]
    fn non_mcp_unknown_tools_are_still_hard_denied() {
        let p = Policy::from_preset(Preset::Full);
        let call = ToolCall {
            id: "1".into(),
            name: "totally_unknown_tool".into(),
            arguments: serde_json::json!({}),
        };
        assert!(matches!(p.decide(&call), Decision::Deny(_)));
    }
}
