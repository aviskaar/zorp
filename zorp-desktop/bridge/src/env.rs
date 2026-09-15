use std::process::Command;

pub fn parse_shell_path(output: &str, current: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    if current.is_empty() || trimmed.len() > current.len() {
        Some(trimmed.to_string())
    } else {
        None
    }
}

pub fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut cmd = Command::new(shell);
    cmd.arg("-lc").arg("printf %s \"$PATH\"");

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let current = std::env::var("PATH").unwrap_or_default();
    parse_shell_path(&stdout, &current)
}

pub fn repair_path() -> bool {
    if let Some(new_path) = login_shell_path() {
        std::env::set_var("PATH", new_path);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shell_path_ignores_empty() {
        assert_eq!(parse_shell_path("", "/usr/bin:/bin"), None);
        assert_eq!(parse_shell_path("   \n", "/usr/bin:/bin"), None);
    }

    #[test]
    fn parse_shell_path_accepts_richer_path() {
        let current = "/usr/bin:/bin";
        let richer = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin";
        assert_eq!(parse_shell_path(richer, current), Some(richer.to_string()));
    }

    #[test]
    fn parse_shell_path_rejects_shorter_or_equal_path() {
        let current = "/opt/homebrew/bin:/usr/bin:/bin";
        let shorter = "/usr/bin:/bin";
        assert_eq!(parse_shell_path(shorter, current), None);
        assert_eq!(parse_shell_path(current, current), None);
    }
}
