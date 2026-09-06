//! One case file, read.
//!
//! A case is data and nothing else: no script runs, so what a case can do is
//! exactly what this file can express. Unknown fields are an error rather
//! than a silent skip, because a misspelled expectation that is quietly
//! dropped is a case that passes without checking anything.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;
use zorp_stub::{Ending, Reply};

/// One case: what the agent is asked, what the provider says back, and what
/// must be true afterwards.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Defaults to the file stem, which is usually the better name anyway.
    pub name: Option<String>,
    /// Prose for whoever reads the file. The runner never looks at it.
    pub about: Option<String>,
    pub agent: AgentSpec,
    /// The provider's replies, in order. The last one answers every further
    /// request, which is how "a provider that always refuses" is written.
    #[serde(rename = "reply")]
    pub replies: Vec<ReplySpec>,
    #[serde(default)]
    pub expect: Expect,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSpec {
    /// The task handed to the binary.
    pub prompt: String,
    /// A directory copied into the workspace before the run, relative to
    /// the case file.
    pub fixture: Option<PathBuf>,
    /// Extra environment for the child, applied last. The harness clears
    /// every inherited `ZORP_` variable first, so this is the only way a
    /// case changes a bound the agent reads.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// One reply: what the model said, and how the bytes reached the client.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplySpec {
    /// Assistant text, delivered as one content delta.
    #[serde(default)]
    pub text: String,
    #[serde(default, rename = "tool_call")]
    pub tool_calls: Vec<ToolCallSpec>,
    /// Defaults to `tool_calls` when the reply has any and `stop` when it
    /// does not. Set it to `length` for the cut-off-by-the-provider case.
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub transport: Transport,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallSpec {
    pub name: String,
    /// The call's arguments, written as a TOML table and sent as the JSON
    /// string a provider would send.
    pub arguments: toml::Value,
    /// Defaults to `call-<n>`. Only worth setting when a case cares.
    pub id: Option<String>,
}

/// How one reply reaches the client. This is the whole reason the suite
/// drives the binary: everything here happens below the agent loop, in the
/// HTTP client, the streaming parser and the retry bound.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transport {
    #[serde(default)]
    pub kind: TransportKind,
    /// How many of the reply's own events are written before the transport
    /// misbehaves. Defaults per kind: every delta for `cut_off`, none for
    /// the rest.
    pub after: Option<usize>,
    /// The HTTP status for `status`, or the code inside the error object
    /// for `error_in_stream`.
    pub code: Option<u16>,
    /// What the provider says in an `error_in_stream` object.
    pub message: Option<String>,
    /// `metadata.provider_name` in an `error_in_stream` object. Present
    /// means a gateway relaying an upstream that failed; absent means our
    /// own request was refused, and a 404 is retried in the first case and
    /// not in the second.
    pub provider_name: Option<String>,
    /// A `Retry-After` header on a `status` reply, in seconds.
    pub retry_after: Option<u64>,
    /// The JSON body of a `status` reply. Defaults to a plain error object.
    pub body: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    /// A whole stream that says it finished.
    #[default]
    Ok,
    /// Deltas, then the body ends with no `[DONE]` and no finish reason.
    CutOff,
    /// An error object delivered inside a 200 stream.
    ErrorInStream,
    /// The socket is held open and silent. A case using this must set its
    /// own `ZORP_HTTP_TIMEOUT_SECS`; there is no default worth inheriting.
    Stall,
    /// A status line and a JSON body, with no stream at all.
    Status,
    /// Deltas, then the connection is reset with no close handshake.
    Reset,
    /// The request is read and the connection reset before a byte of reply.
    ResetBeforeHeaders,
}

impl TransportKind {
    /// The fields this kind reads. Anything else set on the transport is an
    /// error: a `retry_after` on an `error_in_stream` does nothing, and a
    /// case that thinks it does is a case that is not testing what it says.
    fn fields(self) -> &'static [&'static str] {
        match self {
            TransportKind::Ok => &[],
            TransportKind::CutOff | TransportKind::Reset | TransportKind::Stall => &["after"],
            TransportKind::ErrorInStream => &["after", "code", "message", "provider_name"],
            TransportKind::Status => &["code", "retry_after", "body"],
            TransportKind::ResetBeforeHeaders => &[],
        }
    }
}

/// What must be true when the run is over. Anything not stated is not
/// checked.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// `success` or `failure`.
    pub exit: Option<Exit>,
    /// How many connections the scripted provider received. This is the
    /// only way to tell a retry from a slow first send.
    pub connections: Option<usize>,
    #[serde(default, rename = "file")]
    pub files: Vec<FileExpect>,
    /// The `role` column of the stored transcript, in `seq` order.
    pub transcript_roles: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exit {
    Success,
    Failure,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileExpect {
    /// Relative to the workspace the agent ran in.
    pub path: String,
    /// The file's whole contents.
    pub contents: Option<String>,
    pub contains: Option<String>,
    /// The file must not exist.
    #[serde(default)]
    pub absent: bool,
}

pub fn load(path: &Path) -> anyhow::Result<Case> {
    let text =
        std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let case: Case =
        toml::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    case.check()
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    Ok(case)
}

impl Case {
    fn check(&self) -> anyhow::Result<()> {
        if self.replies.is_empty() {
            anyhow::bail!("a case needs at least one [[reply]]");
        }
        for (i, reply) in self.replies.iter().enumerate() {
            reply
                .transport
                .check()
                .map_err(|e| anyhow::anyhow!("reply {i}: {e}"))?;
        }
        for file in &self.expect.files {
            if file.absent && (file.contents.is_some() || file.contains.is_some()) {
                anyhow::bail!(
                    "{}: absent and a content expectation cannot both hold",
                    file.path
                );
            }
        }
        Ok(())
    }

    /// The script the stub serves, one entry per reply.
    pub fn script(&self) -> Vec<Reply> {
        self.replies.iter().map(ReplySpec::to_reply).collect()
    }
}

impl Transport {
    fn check(&self) -> anyhow::Result<()> {
        let allowed = self.kind.fields();
        let set: Vec<&str> = [
            ("after", self.after.is_some()),
            ("code", self.code.is_some()),
            ("message", self.message.is_some()),
            ("provider_name", self.provider_name.is_some()),
            ("retry_after", self.retry_after.is_some()),
            ("body", self.body.is_some()),
        ]
        .into_iter()
        .filter(|(_, present)| *present)
        .map(|(name, _)| name)
        .filter(|name| !allowed.contains(name))
        .collect();
        if !set.is_empty() {
            anyhow::bail!(
                "transport kind {:?} does not read {}",
                self.kind,
                set.join(", ")
            );
        }
        if self.kind == TransportKind::Status && self.code.is_none() {
            anyhow::bail!("transport kind status needs a code");
        }
        Ok(())
    }
}

impl ReplySpec {
    /// The `data:` payloads for this reply's content, without the event
    /// that says the stream finished.
    fn deltas(&self) -> Vec<String> {
        let mut events = Vec::new();
        if !self.text.is_empty() {
            events.push(json!({"choices": [{"delta": {"content": self.text}}]}).to_string());
        }
        for (i, call) in self.tool_calls.iter().enumerate() {
            let id = call.id.clone().unwrap_or_else(|| format!("call-{i}"));
            // A provider sends the arguments as a JSON string, not as an
            // object, and the accumulator concatenates them as text.
            let arguments = serde_json::to_string(&call.arguments).unwrap_or_default();
            events.push(
                json!({"choices": [{"delta": {"tool_calls": [{
                    "index": i,
                    "id": id,
                    "type": "function",
                    "function": {"name": call.name, "arguments": arguments},
                }]}}]})
                .to_string(),
            );
        }
        events
    }

    fn finish_reason(&self) -> &str {
        match &self.finish_reason {
            Some(reason) => reason,
            None if self.tool_calls.is_empty() => "stop",
            None => "tool_calls",
        }
    }

    fn to_reply(&self) -> Reply {
        let deltas = self.deltas();
        let transport = &self.transport;
        let take = |default: usize| -> Arc<[String]> {
            let n = transport.after.unwrap_or(default).min(deltas.len());
            deltas[..n].into()
        };
        match transport.kind {
            TransportKind::Ok => {
                let mut events = deltas.clone();
                events.push(
                    json!({"choices": [{"delta": {}, "finish_reason": self.finish_reason()}]})
                        .to_string(),
                );
                Reply::Scripted {
                    events: events.into(),
                    ending: Ending::Done,
                }
            }
            TransportKind::CutOff => Reply::Scripted {
                events: take(deltas.len()),
                ending: Ending::CutOff,
            },
            TransportKind::ErrorInStream => Reply::Scripted {
                events: take(0),
                ending: Ending::Error(transport.error_object().into()),
            },
            TransportKind::Stall => Reply::Scripted {
                events: take(0),
                ending: Ending::Quiet,
            },
            TransportKind::Reset => Reply::Scripted {
                events: take(0),
                ending: Ending::Reset,
            },
            TransportKind::ResetBeforeHeaders => Reply::ResetBeforeHeaders,
            TransportKind::Status => Reply::Status {
                code: transport.code.unwrap_or(500),
                retry_after: transport.retry_after,
                // `Reply::Status` holds a borrowed body because a transport
                // test writes one as a literal. A case file's body is owned,
                // and the stub outlives the run, so it is leaked on purpose.
                body: Box::leak(transport.status_body().into_boxed_str()),
            },
        }
    }
}

impl Transport {
    /// The error object an `error_in_stream` reply delivers, in the shape
    /// OpenRouter sends: `choices` empty, the code and message in `error`,
    /// and the upstream's name in `metadata.provider_name` when there is one.
    fn error_object(&self) -> String {
        let mut error = json!({
            "code": self.code.unwrap_or(502),
            "message": self.message.clone().unwrap_or_else(|| "stub error".to_string()),
        });
        if let Some(provider) = &self.provider_name {
            error["metadata"] = json!({"provider_name": provider});
        }
        json!({"choices": [], "error": error}).to_string()
    }

    fn status_body(&self) -> String {
        self.body.clone().unwrap_or_else(|| {
            json!({"error": {
                "code": self.code.unwrap_or(500),
                "message": self.message.clone().unwrap_or_else(|| "stub error".to_string()),
            }})
            .to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> anyhow::Result<Case> {
        let case: Case = toml::from_str(text)?;
        case.check()?;
        Ok(case)
    }

    const MINIMAL: &str = r#"
[agent]
prompt = "go"
[[reply]]
text = "done"
"#;

    #[test]
    fn a_minimal_case_parses_and_scripts_one_finished_stream() {
        let case = parse(MINIMAL).unwrap();
        assert_eq!(case.script().len(), 1);
        let Reply::Scripted { events, ending } = &case.script()[0] else {
            panic!("a plain reply should be a scripted stream");
        };
        assert!(matches!(ending, Ending::Done));
        assert_eq!(events.len(), 2, "one content delta and one finish event");
        assert!(events[0].contains("done"));
        assert!(events[1].contains("\"finish_reason\":\"stop\""));
    }

    #[test]
    fn an_unknown_field_is_an_error_and_not_a_silent_skip() {
        let text = format!("{MINIMAL}\n[expect]\nexti = \"success\"\n");
        let error = parse(&text).unwrap_err().to_string();
        assert!(error.contains("exti"), "{error}");
    }

    #[test]
    fn a_tool_call_becomes_a_delta_carrying_json_arguments_as_a_string() {
        let case = parse(
            r#"
[agent]
prompt = "go"
[[reply]]
[[reply.tool_call]]
name = "write_file"
arguments = { path = "a.txt", content = "hi" }
"#,
        )
        .unwrap();
        let Reply::Scripted { events, .. } = &case.script()[0] else {
            panic!("expected a scripted stream");
        };
        assert!(events[0].contains("write_file"));
        // The arguments travel as a JSON string, the way a provider sends
        // them, not as a nested object.
        assert!(events[0].contains(r#"\"path\":\"a.txt\""#), "{}", events[0]);
        assert!(
            events[1].contains("\"finish_reason\":\"tool_calls\""),
            "a reply with calls finishes as tool_calls without being told"
        );
    }

    #[test]
    fn a_transport_field_the_kind_does_not_read_is_an_error() {
        let text = format!("{MINIMAL}\n[reply.transport]\nkind = \"ok\"\nretry_after = 1\n");
        let error = parse(&text).unwrap_err().to_string();
        assert!(error.contains("retry_after"), "{error}");
    }

    #[test]
    fn a_status_reply_needs_its_code() {
        let text = format!("{MINIMAL}\n[reply.transport]\nkind = \"status\"\n");
        assert!(parse(&text).unwrap_err().to_string().contains("code"));
    }

    #[test]
    fn an_error_in_stream_defaults_to_arriving_before_any_delta() {
        let case = parse(
            r#"
[agent]
prompt = "go"
[[reply]]
text = "half an answer"
[reply.transport]
kind = "error_in_stream"
code = 502
message = "overloaded"
provider_name = "Nvidia"
"#,
        )
        .unwrap();
        let Reply::Scripted { events, ending } = &case.script()[0] else {
            panic!("expected a scripted stream");
        };
        assert!(events.is_empty(), "nothing should reach the caller first");
        let Ending::Error(payload) = ending else {
            panic!("expected an error ending");
        };
        assert!(
            payload.contains("\"provider_name\":\"Nvidia\""),
            "{payload}"
        );
    }

    #[test]
    fn a_cut_off_keeps_every_delta_and_drops_the_finish_event() {
        let case = parse(
            r#"
[agent]
prompt = "go"
[[reply]]
text = "half"
[reply.transport]
kind = "cut_off"
"#,
        )
        .unwrap();
        let Reply::Scripted { events, ending } = &case.script()[0] else {
            panic!("expected a scripted stream");
        };
        assert_eq!(events.len(), 1);
        assert!(matches!(ending, Ending::CutOff));
    }

    #[test]
    fn a_file_cannot_be_both_absent_and_checked_for_contents() {
        let text =
            format!("{MINIMAL}\n[[expect.file]]\npath = \"a\"\nabsent = true\ncontains = \"x\"\n");
        assert!(parse(&text).unwrap_err().to_string().contains("absent"));
    }
}
