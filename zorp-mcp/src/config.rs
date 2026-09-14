use crate::error::McpError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    Stdio,
    StreamableHttp,
    Sse,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    Sandbox,
    Trusted,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub name: String,
    pub transport: TransportKind,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub trust: TrustLevel,
    pub timeout_secs: Option<u64>,
}

/// One server as it may be shown outside this process.
///
/// The whole point of the type is the two maps that are not on it. `env`
/// and `headers` are where a token goes, and a listing that carried their
/// values would put a credential on a web page because somebody wanted to
/// see which servers were configured.
///
/// Key names are kept, because they are the useful half: seeing that a
/// server wants `GITHUB_TOKEN` tells a person what to set without telling
/// anybody what it is.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerSummary {
    pub name: String,
    pub transport: TransportKind,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    /// The names of the environment variables this server is given. Never
    /// their values.
    pub env_keys: Vec<String>,
    /// The names of the headers this server is sent. Never their values.
    pub header_keys: Vec<String>,
    pub trust: TrustLevel,
    pub timeout_secs: Option<u64>,
}

impl ServerConfig {
    /// This server with every secret-bearing value left behind.
    ///
    /// **The destructuring is the guarantee and must stay exhaustive.**
    /// Adding a field to `ServerConfig` makes this function stop compiling,
    /// which forces a decision about whether the new field may be shown.
    /// A version of this that read fields through `self.` would silently
    /// omit a new one, and the failure mode of a redaction that silently
    /// omits is that somebody adds `token: String` and nothing notices.
    pub fn redacted(&self) -> ServerSummary {
        let ServerConfig {
            name,
            transport,
            command,
            args,
            env,
            url,
            headers,
            trust,
            timeout_secs,
        } = self;
        // Sorted, so a listing does not reorder itself between two reads of
        // the same file for want of a stable hash map iteration order.
        let mut env_keys: Vec<String> = env.keys().cloned().collect();
        env_keys.sort();
        let mut header_keys: Vec<String> = headers.keys().cloned().collect();
        header_keys.sort();
        ServerSummary {
            name: name.clone(),
            transport: transport.clone(),
            command: command.clone(),
            args: args.clone(),
            url: url.clone(),
            env_keys,
            header_keys,
            trust: trust.clone(),
            timeout_secs: *timeout_secs,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct McpConfig {
    pub servers: Vec<ServerConfig>,
}

#[derive(Deserialize)]
struct McpConfigToml {
    #[serde(default, rename = "server")]
    servers: Vec<ServerConfig>,
}

/// A TOML parse failure said without quoting the file back.
///
/// `toml::de::Error`'s `Display` renders the offending source line
/// verbatim under a caret. This file holds `headers` and `env`, which is
/// where a `Bearer` token or an API key lives, so a malformed line right
/// there puts the secret into an error string that a caller will log, show
/// in a settings pane, or paste into a bug report. `ServerConfig::redacted`
/// keeps secrets out of the success path and would be pointless if the
/// failure path handed them out.
///
/// The line and column are what makes the message useful and neither is
/// secret, so both are kept and only the quoted body is dropped. The span
/// is a byte offset into the same text, so the position is counted here
/// rather than read off the rendering.
fn parse_error(source: &str, e: &toml::de::Error) -> String {
    let Some(span) = e.span() else {
        return e.message().to_string();
    };
    let start = span.start.min(source.len());
    let line = source[..start].matches('\n').count() + 1;
    let column = source[..start]
        .rfind('\n')
        .map_or(start, |nl| start - nl - 1)
        + 1;
    format!("line {line}, column {column}: {}", e.message())
}

impl McpConfig {
    pub fn empty() -> Self {
        McpConfig { servers: vec![] }
    }

    pub fn from_toml_str(s: &str) -> Result<Self, McpError> {
        let t: McpConfigToml =
            toml::from_str(s).map_err(|e| McpError::Config(parse_error(s, &e)))?;
        Ok(McpConfig { servers: t.servers })
    }

    pub fn from_file(path: &Path) -> Result<Self, McpError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml_str(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::empty()),
            Err(e) => Err(McpError::Config(format!("{}: {e}", path.display()))),
        }
    }

    pub fn merge_from(&mut self, other: McpConfig) {
        for incoming in other.servers {
            if let Some(existing) = self.servers.iter_mut().find(|s| s.name == incoming.name) {
                *existing = incoming;
            } else {
                self.servers.push(incoming);
            }
        }
    }

    pub fn from_env_var(var: &str) -> Result<Self, McpError> {
        let raw = match std::env::var(var) {
            Ok(v) if !v.trim().is_empty() => v,
            _ => return Ok(Self::empty()),
        };
        let servers: Vec<ServerConfig> = serde_json::from_str(&raw)
            .map_err(|e| McpError::Config(format!("{var} parse error: {e}")))?;
        Ok(McpConfig { servers })
    }

    pub fn from_env() -> Result<Self, McpError> {
        Self::from_env_var("ZORP_MCP_SERVERS")
    }

    pub fn merged(mut file: McpConfig, env: McpConfig, cli: McpConfig) -> McpConfig {
        file.merge_from(env);
        file.merge_from(cli);
        file
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &str = r#"
[[server]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
trust = "sandbox"

[[server]]
name = "github"
transport = "streamable_http"
url = "https://api.githubcopilot.com/mcp/"
trust = "trusted"
timeout_secs = 60
"#;
    #[test]
    fn parses_two_servers() {
        let cfg = McpConfig::from_toml_str(SAMPLE).unwrap();
        assert_eq!(cfg.servers.len(), 2);
    }
    #[test]
    fn stdio_server_parsed() {
        let cfg = McpConfig::from_toml_str(SAMPLE).unwrap();
        let fs = &cfg.servers[0];
        assert_eq!(fs.name, "filesystem");
        assert!(matches!(fs.transport, TransportKind::Stdio));
        assert_eq!(fs.command.as_deref(), Some("npx"));
        assert!(matches!(fs.trust, TrustLevel::Sandbox));
    }
    #[test]
    fn streamable_http_server_parsed() {
        let cfg = McpConfig::from_toml_str(SAMPLE).unwrap();
        let gh = &cfg.servers[1];
        assert!(matches!(gh.transport, TransportKind::StreamableHttp));
        assert_eq!(
            gh.url.as_deref(),
            Some("https://api.githubcopilot.com/mcp/")
        );
        assert!(matches!(gh.trust, TrustLevel::Trusted));
        assert_eq!(gh.timeout_secs, Some(60));
    }
    #[test]
    fn missing_file_returns_empty() {
        let cfg = McpConfig::from_file(std::path::Path::new("/nonexistent/mcp.toml")).unwrap();
        assert!(cfg.servers.is_empty());
    }
    #[test]
    fn from_env_var_parses_json_array() {
        std::env::set_var(
            "ZORP_MCP_TEST_ABC",
            r#"[{"name":"mem","transport":"stdio","command":"npx","args":["-y","server-memory"],"trust":"sandbox"}]"#,
        );
        let cfg = McpConfig::from_env_var("ZORP_MCP_TEST_ABC").unwrap();
        assert_eq!(cfg.servers.len(), 1);
        assert_eq!(cfg.servers[0].name, "mem");
        std::env::remove_var("ZORP_MCP_TEST_ABC");
    }
    #[test]
    fn merged_env_overrides_file_by_name() {
        let file = McpConfig::from_toml_str(
            "[[server]]\nname=\"fs\"\ntransport=\"stdio\"\ncommand=\"npx\"\ntrust=\"sandbox\"\n",
        )
        .unwrap();
        let env = McpConfig::from_toml_str(
            "[[server]]\nname=\"fs\"\ntransport=\"stdio\"\ncommand=\"uvx\"\ntrust=\"trusted\"\n",
        )
        .unwrap();
        let merged = McpConfig::merged(file, env, McpConfig::empty());
        assert_eq!(merged.servers.len(), 1);
        assert_eq!(merged.servers[0].command.as_deref(), Some("uvx"));
    }
}

#[cfg(test)]
mod redaction_tests {
    use super::*;

    fn server_with_secrets() -> ServerConfig {
        let mut env = HashMap::new();
        env.insert("GITHUB_TOKEN".to_string(), "ghp_REALSECRET".to_string());
        env.insert("API_KEY".to_string(), "sk-REALSECRET".to_string());
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer REALSECRET".to_string());
        ServerConfig {
            name: "github".to_string(),
            transport: TransportKind::StreamableHttp,
            command: None,
            args: vec![],
            env,
            url: Some("https://api.example.com/mcp".to_string()),
            headers,
            trust: TrustLevel::Sandbox,
            timeout_secs: Some(30),
        }
    }

    /// The one that matters. A value from either map reaching the summary
    /// is a credential on a web page.
    #[test]
    fn no_env_or_header_value_survives_redaction() {
        let summary = serde_json::to_string(&server_with_secrets().redacted()).unwrap();
        assert!(!summary.contains("REALSECRET"), "{summary}");
        assert!(!summary.contains("ghp_"), "{summary}");
        assert!(!summary.contains("Bearer"), "{summary}");
    }

    /// The useful half is kept. Knowing a server wants `GITHUB_TOKEN` tells
    /// somebody what to set without telling anybody what it is.
    /// A malformed line in this file is a line that may hold a token.
    ///
    /// `toml`'s own error renders the offending source line verbatim, so
    /// the failure path was handing out exactly what `redacted` keeps off
    /// the success path. The position survives because it is what makes
    /// the message worth printing and it is not a secret.
    #[test]
    fn a_parse_failure_does_not_quote_the_file_back() {
        let malformed = "[[servers]]\nname = \"gh\"\ncommand = \"x\"\n\
                         headers = { Authorization = \"Bearer ghp_REALSECRET\", }\n";
        let err = McpConfig::from_toml_str(malformed)
            .expect_err("that is not valid toml")
            .to_string();

        assert!(!err.contains("ghp_REALSECRET"), "{err}");
        assert!(!err.contains("Bearer"), "{err}");
        assert!(!err.contains("Authorization"), "{err}");
        assert!(
            err.contains("line 4"),
            "the position is the useful half: {err}"
        );
    }

    /// The same for the environment variable, whose whole value is one
    /// line of JSON holding the same fields.
    #[test]
    fn an_env_parse_failure_does_not_quote_the_value_back() {
        let key = "ZORP_MCP_TEST_LEAK";
        std::env::set_var(
            key,
            r#"[{"name":"gh","headers":{"Authorization":"Bearer ghp_REALSECRET"},}]"#,
        );
        let err = McpConfig::from_env_var(key)
            .expect_err("that is not valid json")
            .to_string();
        std::env::remove_var(key);

        assert!(!err.contains("ghp_REALSECRET"), "{err}");
    }

    #[test]
    fn the_key_names_are_kept_because_they_are_what_helps() {
        let summary = server_with_secrets().redacted();
        assert_eq!(summary.env_keys, vec!["API_KEY", "GITHUB_TOKEN"]);
        assert_eq!(summary.header_keys, vec!["Authorization"]);
        assert_eq!(summary.name, "github");
        assert_eq!(summary.url.as_deref(), Some("https://api.example.com/mcp"));
    }

    /// A listing that reordered itself between two reads of the same file
    /// would look like the configuration had changed.
    #[test]
    fn key_names_come_back_in_a_stable_order() {
        let server = server_with_secrets();
        assert_eq!(server.redacted().env_keys, server.redacted().env_keys);
    }

    /// A command and its arguments are shown, because that is what somebody
    /// checking a configured server needs to see. A secret passed as an
    /// argument is a mistake this cannot fix and must not pretend to: the
    /// place for one is `env`, and the listing shows the command so that
    /// mistake is visible rather than hidden.
    #[test]
    fn a_stdio_command_and_its_arguments_are_shown() {
        let server = ServerConfig {
            name: "fs".to_string(),
            transport: TransportKind::Stdio,
            command: Some("npx".to_string()),
            args: vec!["-y".to_string(), "server-filesystem".to_string()],
            env: HashMap::new(),
            url: None,
            headers: HashMap::new(),
            trust: TrustLevel::Sandbox,
            timeout_secs: None,
        };
        let summary = server.redacted();
        assert_eq!(summary.command.as_deref(), Some("npx"));
        assert_eq!(summary.args.len(), 2);
    }
}
