//! One question to one runtime, and what came back: an answer with its
//! timing, or the reason there is no answer.
//!
//! The second half is the point of this file. A measurement that did not
//! happen is not a zero. If an endpoint that refused the connection scored
//! 0.0, "this model is bad at GPQA" and "the endpoint was down" would be the
//! same row in the table, and a table that is generated automatically would
//! propagate that quietly. So every way a request can fail to produce an
//! answer is named here as an [`Unevaluable`] with a [`Reason`], and the
//! runner records it in its own column instead of grading it.
//!
//! Model traffic goes through `zorp::http_agent` and
//! `zorp::send_json_retrying` like every other model call in the workspace,
//! so the connect timeout, the 404 rule and what a refused request becomes
//! are the same ones the agent lives under. What bench adds is a per-request
//! ceiling and a retry policy stated by the case rather than read from the
//! environment.
//!
//! Requests stream, because tok/s is only honest with the time to the first
//! token taken out of it. An OpenAI-compatible request asks for usage in the
//! stream (`stream_options.include_usage`); a provider that does not send it
//! leaves the token columns empty rather than estimated.

use std::io::BufRead;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::case::Bounds;

/// The wire format a runtime speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    OpenAi,
    Anthropic,
}

/// Everything needed to reach one runtime, resolved once before anything is
/// sent: a manifest that names a key variable nobody set fails there, not
/// four hundred requests later as four hundred 401s.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub wire: Wire,
    /// The full request URL: `<base>/chat/completions` or `<base>/messages`.
    pub url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub reasoning_mode: Option<ReasoningMode>,
}

/// The manifest's `reasoning_mode`, sent the way `zorp-agent` sends it, so a
/// bench row and a compat row for one runtime asked the provider for the same
/// thing. A copy of the agent's mapping rather than a dependency on the whole
/// agent crate; the two are small and fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningMode {
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

impl ReasoningMode {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        Ok(match text.trim().to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "minimal" => Self::Minimal,
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            "xhigh" => Self::XHigh,
            other => anyhow::bail!("unknown reasoning mode: {other}"),
        })
    }

    fn effort(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }

    /// `zorp-agent`'s `anthropic_thinking_budget`.
    fn thinking_budget(self) -> Option<u32> {
        match self {
            Self::None => None,
            Self::Minimal => Some(1024),
            Self::Low => Some(4000),
            Self::Medium => Some(10000),
            Self::High => Some(24000),
            Self::XHigh => Some(32000),
        }
    }
}

/// An answer, and how long it took.
#[derive(Debug, Clone, PartialEq)]
pub struct Answered {
    pub text: String,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    /// Send to the first streamed token, of any kind. `None` when the
    /// provider answered with one buffered body instead of a stream.
    pub ttft_ms: Option<u64>,
    pub latency_ms: u64,
    /// Sends it took, counting the first. More than one means the latency
    /// includes backoff, and the report leaves it out of the timing columns.
    pub sends: u32,
}

/// No answer, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Unevaluable {
    pub reason: Reason,
    /// What the transport or the provider said, for whoever reads the
    /// database. Never shown in the table.
    pub detail: String,
    pub http_status: Option<u16>,
    pub latency_ms: u64,
    pub sends: u32,
}

/// Why an item has no answer. A closed set, so the table's reason column is
/// code-derived and a new failure has to be named here before it can appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Nothing accepted the connection, or the name did not resolve.
    Unreachable,
    /// The case's `timeout_secs`, or the read timeout, ran out.
    Timeout,
    /// 429, still, after the case's retry policy was spent.
    RateLimited,
    /// Any other status the provider would not take back.
    HttpStatus,
    /// An error object inside a 200 body or stream.
    ProviderError,
    /// The stream ended with no `[DONE]` and no finish reason.
    Truncated,
    /// The provider said it stopped at the token limit.
    Length,
    /// The provider's own filter declined to answer.
    ContentFilter,
    /// A finished reply with no text in it.
    Empty,
    /// A body that was not the protocol it claimed to be.
    Malformed,
    /// A transport failure that is none of the above.
    Transport,
}

impl Reason {
    pub fn code(self) -> &'static str {
        match self {
            Reason::Unreachable => "unreachable",
            Reason::Timeout => "timeout",
            Reason::RateLimited => "rate_limited",
            Reason::HttpStatus => "http_status",
            Reason::ProviderError => "provider_error",
            Reason::Truncated => "truncated",
            Reason::Length => "length",
            Reason::ContentFilter => "content_filter",
            Reason::Empty => "empty",
            Reason::Malformed => "malformed",
            Reason::Transport => "transport",
        }
    }
}

/// Ask one question.
pub fn ask(endpoint: &Endpoint, bounds: &Bounds, prompt: &str) -> Result<Answered, Unevaluable> {
    let started = Instant::now();
    let mut retrying = zorp::Retrying::with_policy(bounds.retry_policy());
    let body = body(endpoint, bounds, prompt);
    let mut req = zorp::http_agent()
        .post(&endpoint.url)
        .timeout(Duration::from_secs(bounds.timeout_secs));
    match (endpoint.wire, &endpoint.api_key) {
        (Wire::OpenAi, Some(key)) => req = req.set("Authorization", &format!("Bearer {key}")),
        (Wire::Anthropic, Some(key)) => req = req.set("x-api-key", key),
        (_, None) => {}
    }
    if endpoint.wire == Wire::Anthropic {
        req = req.set("anthropic-version", "2023-06-01");
    }
    let fail = |reason, detail: String, status, retrying: &zorp::Retrying| Unevaluable {
        reason,
        detail,
        http_status: status,
        latency_ms: elapsed_ms(started),
        sends: retrying.sent(),
    };
    loop {
        // Time to first token is timed from here and latency from the start.
        // A status retried inside `send_json_retrying` puts its backoff in
        // both, which is why an answer carries `sends` and the report keeps
        // anything sent more than once out of the timing columns.
        let sent_at = Instant::now();
        let resp = match zorp::send_json_retrying(&req, &body, &mut retrying) {
            Ok(resp) => resp,
            Err(error) => {
                let (reason, status) = classify_send_error(&*error);
                return Err(fail(reason, error.to_string(), status, &retrying));
            }
        };
        // ureq's deadline runs from each send, and a read that broke at it
        // is the deadline and not a dropped connection.
        let ceiling = Duration::from_secs(bounds.timeout_secs);
        let read = match endpoint.wire {
            Wire::OpenAi => read_openai(resp, sent_at, ceiling),
            Wire::Anthropic => read_anthropic(resp, sent_at, ceiling),
        };
        let reply = match read {
            Ok(reply) => reply,
            Err(Stop::ProviderError(error, after_text)) => {
                // Refused inside the body before any text arrived: as clean
                // to send again as a status, and the same bound. After text,
                // never, the rule every streaming path in the workspace keeps.
                let said = format!("{error} inside a 200 body");
                if !after_text
                    && error.code.is_some_and(|code| {
                        retrying.again(&endpoint.url, code, error.provider.as_deref(), None, &said)
                    })
                {
                    continue;
                }
                let status = error.code;
                let reason = if status == Some(429) {
                    Reason::RateLimited
                } else {
                    Reason::ProviderError
                };
                return Err(fail(reason, error.to_string(), status, &retrying));
            }
            Err(Stop::Failed(reason, detail)) => {
                return Err(fail(reason, detail, None, &retrying));
            }
        };
        let finish = reply.finish.as_deref().unwrap_or("");
        let refused = match finish {
            "length" | "max_tokens" => Some((Reason::Length, "cut off at the token limit")),
            "content_filter" | "refusal" => Some((Reason::ContentFilter, "declined by a filter")),
            _ if reply.text.trim().is_empty() => {
                Some((Reason::Empty, "a finished reply with no text"))
            }
            _ => None,
        };
        if let Some((reason, said)) = refused {
            return Err(fail(
                reason,
                format!("{said} (finish reason {finish:?})"),
                None,
                &retrying,
            ));
        }
        return Ok(Answered {
            text: reply.text,
            prompt_tokens: reply.prompt_tokens,
            completion_tokens: reply.completion_tokens,
            ttft_ms: reply.ttft_ms,
            latency_ms: elapsed_ms(started),
            sends: retrying.sent(),
        });
    }
}

fn elapsed_ms(since: Instant) -> u64 {
    since.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

fn body(endpoint: &Endpoint, bounds: &Bounds, prompt: &str) -> Value {
    let messages = json!([{"role": "user", "content": prompt}]);
    match endpoint.wire {
        Wire::OpenAi => {
            let mut body = json!({
                "model": endpoint.model,
                "messages": messages,
                "stream": true,
                "stream_options": {"include_usage": true},
            });
            if let Some(max) = bounds.max_tokens {
                body["max_tokens"] = json!(max);
            }
            if let Some(mode) = endpoint.reasoning_mode {
                body["reasoning_effort"] = json!(mode.effort());
            }
            body
        }
        Wire::Anthropic => {
            let budget = endpoint
                .reasoning_mode
                .and_then(ReasoningMode::thinking_budget);
            let mut max = bounds.max_tokens.unwrap_or(4096);
            if let Some(budget) = budget {
                // Anthropic refuses a max_tokens at or under the budget.
                max = max.max(budget.saturating_add(1024));
            }
            let mut body = json!({
                "model": endpoint.model,
                "messages": messages,
                "max_tokens": max,
                "stream": true,
            });
            if let Some(budget) = budget {
                body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
            }
            body
        }
    }
}

/// What `send_json_retrying` said, as a reason and a status. The status
/// error's first half is a shape other code matches on ("<url>: status code
/// <n>"), and a transport error is ureq's own, so both are read, not guessed.
fn classify_send_error(
    error: &(dyn std::error::Error + Send + Sync + 'static),
) -> (Reason, Option<u16>) {
    if let Some(error) = error.downcast_ref::<ureq::Error>() {
        return match error.kind() {
            ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Dns => (Reason::Unreachable, None),
            ureq::ErrorKind::Io if timed_out(error) => (Reason::Timeout, None),
            _ => (Reason::Transport, None),
        };
    }
    let text = error.to_string();
    if let Some(code) = text
        .split_once(": status code ")
        .and_then(|(_, rest)| rest.get(..3))
        .and_then(|n| n.parse::<u16>().ok())
    {
        let reason = if code == 429 {
            Reason::RateLimited
        } else {
            Reason::HttpStatus
        };
        return (reason, Some(code));
    }
    if text.contains("sent nothing for") || text.contains("timed out") {
        return (Reason::Timeout, None);
    }
    (Reason::Transport, None)
}

fn timed_out(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(io) = error.downcast_ref::<std::io::Error>() {
            if matches!(
                io.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) {
                return true;
            }
        }
        current = error.source();
    }
    false
}

/// A reply read to its end.
struct Reply {
    text: String,
    finish: Option<String>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    ttft_ms: Option<u64>,
}

enum Stop {
    /// An error object from the provider, and whether any text came first.
    ProviderError(zorp::ProviderError, bool),
    Failed(Reason, String),
}

/// A read error part way through a body. ureq reports a read timeout on a
/// chunked body as "Error while decoding chunks", so the clock is asked too:
/// a read that failed at or past the ceiling is the ceiling.
fn read_failed(error: std::io::Error, sent_at: Instant, ceiling: Duration) -> Stop {
    let past_ceiling = sent_at.elapsed() + Duration::from_millis(250) >= ceiling;
    let reason = if past_ceiling
        || matches!(
            error.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        ) {
        Reason::Timeout
    } else {
        Reason::Transport
    };
    Stop::Failed(reason, format!("the body broke off: {error}"))
}

/// The `data:` payloads of an event stream, or the whole body when it is
/// not a stream, handed to `on` one at a time until it returns false.
fn each_payload(
    resp: ureq::Response,
    sent_at: Instant,
    ceiling: Duration,
    mut on: impl FnMut(&str, Instant) -> Result<bool, Stop>,
) -> Result<(), Stop> {
    let mut reader = std::io::BufReader::new(resp.into_reader());
    let mut line = String::new();
    let mut whole = String::new();
    let mut streaming = None;
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => return Err(read_failed(e, sent_at, ceiling)),
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') || trimmed.starts_with("event:") {
            continue;
        }
        if streaming.is_none() {
            streaming = Some(trimmed.starts_with("data:"));
        }
        if streaming == Some(true) {
            if let Some(payload) = trimmed.strip_prefix("data:") {
                if !on(payload.trim(), Instant::now())? {
                    return Ok(());
                }
            }
        } else {
            whole.push_str(&line);
        }
    }
    if streaming == Some(false) {
        on(&whole, Instant::now())?;
    }
    Ok(())
}

fn parse(payload: &str) -> Result<Value, Stop> {
    serde_json::from_str(payload).map_err(|e| {
        Stop::Failed(
            Reason::Malformed,
            format!(
                "not JSON: {e}: {}",
                payload.chars().take(200).collect::<String>()
            ),
        )
    })
}

fn read_openai(resp: ureq::Response, sent_at: Instant, ceiling: Duration) -> Result<Reply, Stop> {
    let mut reply = Reply {
        text: String::new(),
        finish: None,
        prompt_tokens: None,
        completion_tokens: None,
        ttft_ms: None,
    };
    let mut done = false;
    let mut buffered = false;
    each_payload(resp, sent_at, ceiling, |payload, at| {
        if payload == "[DONE]" {
            done = true;
            return Ok(false);
        }
        let value = parse(payload)?;
        if let Some(error) = zorp::ProviderError::in_body(&value) {
            return Err(Stop::ProviderError(error, !reply.text.is_empty()));
        }
        let choice = value.get("choices").and_then(|c| c.get(0));
        if let Some(message) = choice.and_then(|c| c.get("message")) {
            // One buffered body: a server that ignored `stream`.
            buffered = true;
            if let Some(text) = message.get("content").and_then(Value::as_str) {
                reply.text.push_str(text);
            }
        } else if let Some(delta) = choice.and_then(|c| c.get("delta")) {
            let content = delta.get("content").and_then(Value::as_str).unwrap_or("");
            let thought = ["reasoning_content", "reasoning"].iter().any(|k| {
                delta
                    .get(*k)
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
            });
            if reply.ttft_ms.is_none() && (!content.is_empty() || thought) {
                reply.ttft_ms = Some(elapsed_ms_between(sent_at, at));
            }
            reply.text.push_str(content);
        }
        if let Some(finish) = choice
            .and_then(|c| c.get("finish_reason"))
            .and_then(Value::as_str)
        {
            reply.finish = Some(finish.to_string());
        }
        if let Some(usage) = value.get("usage").filter(|u| u.is_object()) {
            reply.prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_u64);
            reply.completion_tokens = usage.get("completion_tokens").and_then(Value::as_u64);
        }
        Ok(true)
    })?;
    // A stream that ends before the provider says it has finished is not a
    // short answer: it is indistinguishable from one, which is the problem.
    if !done && reply.finish.is_none() && !buffered {
        return Err(Stop::Failed(
            Reason::Truncated,
            "the stream ended with no [DONE] and no finish reason".into(),
        ));
    }
    Ok(reply)
}

fn read_anthropic(
    resp: ureq::Response,
    sent_at: Instant,
    ceiling: Duration,
) -> Result<Reply, Stop> {
    let mut reply = Reply {
        text: String::new(),
        finish: None,
        prompt_tokens: None,
        completion_tokens: None,
        ttft_ms: None,
    };
    let mut stopped = false;
    each_payload(resp, sent_at, ceiling, |payload, at| {
        let value = parse(payload)?;
        if value.get("type").and_then(Value::as_str) == Some("error") {
            let error = zorp::ProviderError::in_body(&value).unwrap_or(zorp::ProviderError {
                code: None,
                message: payload.to_string(),
                provider: None,
            });
            return Err(Stop::ProviderError(error, !reply.text.is_empty()));
        }
        match value.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                let usage = value.get("message").and_then(|m| m.get("usage"));
                reply.prompt_tokens = usage
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(Value::as_u64);
            }
            Some("content_block_delta") => {
                let delta = value.get("delta");
                if reply.ttft_ms.is_none() {
                    reply.ttft_ms = Some(elapsed_ms_between(sent_at, at));
                }
                if let Some(text) = delta.and_then(|d| d.get("text")).and_then(Value::as_str) {
                    reply.text.push_str(text);
                }
            }
            Some("message_delta") => {
                if let Some(stop) = value
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    reply.finish = Some(stop.to_string());
                }
                if let Some(n) = value
                    .get("usage")
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(Value::as_u64)
                {
                    reply.completion_tokens = Some(n);
                }
            }
            Some("message_stop") => {
                stopped = true;
                return Ok(false);
            }
            // A buffered Messages body.
            Some("message") => {
                stopped = true;
                for block in value
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        reply.text.push_str(text);
                    }
                }
                reply.finish = value
                    .get("stop_reason")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let usage = value.get("usage");
                reply.prompt_tokens = usage
                    .and_then(|u| u.get("input_tokens"))
                    .and_then(Value::as_u64);
                reply.completion_tokens = usage
                    .and_then(|u| u.get("output_tokens"))
                    .and_then(Value::as_u64);
            }
            _ => {}
        }
        Ok(true)
    })?;
    if !stopped {
        return Err(Stop::Failed(
            Reason::Truncated,
            "the stream ended with no message_stop".into(),
        ));
    }
    Ok(reply)
}

fn elapsed_ms_between(from: Instant, to: Instant) -> u64 {
    to.saturating_duration_since(from)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use zorp_stub::{scripted_server, scripted_server_recording, Ending, Framing, Reply};

    fn scripted(events: &[serde_json::Value], ending: Ending) -> Reply {
        Reply::Scripted {
            events: events
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .into(),
            ending,
        }
    }

    fn answer(text: &str, finish: &str) -> Reply {
        scripted(
            &[
                json!({"choices": [{"delta": {"content": text}}]}),
                json!({"choices": [{"delta": {}, "finish_reason": finish}]}),
                json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 3}}),
            ],
            Ending::Done,
        )
    }

    fn endpoint(address: std::net::SocketAddr) -> Endpoint {
        Endpoint {
            wire: Wire::OpenAi,
            url: format!("http://{address}/v1/chat/completions"),
            model: "stub".into(),
            api_key: None,
            reasoning_mode: None,
        }
    }

    fn bounds(timeout_secs: u64, retry_attempts: u32) -> Bounds {
        Bounds {
            timeout_secs,
            retry_attempts,
            retry_budget_secs: 30,
            max_tokens: None,
        }
    }

    fn reason(reply: Reply, framing: Framing) -> Reason {
        let (address, _) = scripted_server(framing, vec![reply]);
        ask(&endpoint(address), &bounds(10, 1), "q")
            .unwrap_err()
            .reason
    }

    #[test]
    fn a_finished_stream_is_an_answer_with_usage_and_timing() {
        for framing in Framing::BOTH {
            let (address, _) = scripted_server(framing, vec![answer("Answer: C", "stop")]);
            let got = ask(&endpoint(address), &bounds(10, 1), "q").unwrap();
            assert_eq!(got.text, "Answer: C");
            assert_eq!(
                (got.prompt_tokens, got.completion_tokens),
                (Some(12), Some(3))
            );
            assert!(got.ttft_ms.is_some());
            assert_eq!(got.sends, 1);
        }
    }

    #[test]
    fn every_way_of_not_answering_is_named_and_none_is_an_answer() {
        for framing in Framing::BOTH {
            let cut = scripted(
                &[json!({"choices": [{"delta": {"content": "Answer: "}}]})],
                Ending::CutOff,
            );
            assert_eq!(reason(cut, framing), Reason::Truncated);
            assert_eq!(
                reason(answer("Answer: C", "length"), framing),
                Reason::Length
            );
            assert_eq!(
                reason(answer("", "content_filter"), framing),
                Reason::ContentFilter
            );
            assert_eq!(reason(answer("  ", "stop"), framing), Reason::Empty);
            let error = Ending::Error(
                json!({"choices": [], "error": {"code": 400, "message": "bad"}})
                    .to_string()
                    .into(),
            );
            assert_eq!(reason(scripted(&[], error), framing), Reason::ProviderError);
            let reset = scripted(&[], Ending::Reset);
            assert_eq!(reason(reset, framing), Reason::Transport);
        }
        let status = |code| Reply::Status {
            code,
            retry_after: None,
            body: "{\"error\":{\"message\":\"no\"}}",
        };
        assert_eq!(reason(status(429), Framing::Chunked), Reason::RateLimited);
        assert_eq!(reason(status(401), Framing::Chunked), Reason::HttpStatus);
        let (address, _) = scripted_server(Framing::Chunked, vec![status(500)]);
        let missing = ask(&endpoint(address), &bounds(10, 1), "q").unwrap_err();
        assert_eq!(missing.http_status, Some(500));
    }

    #[test]
    fn nothing_listening_is_unreachable() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let dead = Endpoint {
            url: format!("http://127.0.0.1:{port}/v1/chat/completions"),
            ..endpoint("127.0.0.1:1".parse().unwrap())
        };
        let missing = ask(&dead, &bounds(10, 1), "q").unwrap_err();
        assert_eq!(missing.reason, Reason::Unreachable, "{}", missing.detail);
    }

    #[test]
    fn a_silent_provider_is_a_timeout_at_the_case_ceiling() {
        for framing in Framing::BOTH {
            let quiet = scripted(
                &[json!({"choices": [{"delta": {"content": "Ans"}}]})],
                Ending::Quiet,
            );
            let (address, _) = scripted_server(framing, vec![quiet]);
            let started = Instant::now();
            let missing = ask(&endpoint(address), &bounds(1, 1), "q").unwrap_err();
            assert_eq!(
                missing.reason,
                Reason::Timeout,
                "{framing:?}: {}",
                missing.detail
            );
            assert!(started.elapsed() < Duration::from_secs(10));
        }
    }

    /// The case's retry policy is the one in force: two sends allowed, two
    /// connections made, and the answer says it took two.
    #[test]
    fn the_case_states_the_retry_bound() {
        let limited = Reply::Status {
            code: 429,
            retry_after: None,
            body: "{\"error\":{\"message\":\"slow down\"}}",
        };
        let (address, connections) = scripted_server(
            Framing::Chunked,
            vec![limited.clone(), answer("Answer: A", "stop")],
        );
        let got = ask(&endpoint(address), &bounds(10, 2), "q").unwrap();
        assert_eq!(got.sends, 2);
        assert_eq!(connections.load(Ordering::SeqCst), 2);

        let (address, connections) = scripted_server(Framing::Chunked, vec![limited]);
        let missing = ask(&endpoint(address), &bounds(10, 1), "q").unwrap_err();
        assert_eq!(missing.reason, Reason::RateLimited);
        assert_eq!(connections.load(Ordering::SeqCst), 1);
    }

    /// An overloaded upstream reported inside a 200 is sent again while no
    /// text has arrived, and never after.
    #[test]
    fn an_in_stream_refusal_is_retried_only_before_any_text() {
        let overloaded = json!({"choices": [], "error": {"code": 502, "message": "overloaded", "metadata": {"provider_name": "Up"}}})
            .to_string();
        let early = scripted(&[], Ending::Error(overloaded.clone().into()));
        let (address, connections) =
            scripted_server(Framing::Chunked, vec![early, answer("Answer: B", "stop")]);
        let got = ask(&endpoint(address), &bounds(10, 2), "q").unwrap();
        assert_eq!((got.text.as_str(), got.sends), ("Answer: B", 2));
        assert_eq!(connections.load(Ordering::SeqCst), 2);

        let late = scripted(
            &[json!({"choices": [{"delta": {"content": "Answer:"}}]})],
            Ending::Error(overloaded.into()),
        );
        let (address, connections) =
            scripted_server(Framing::Chunked, vec![late, answer("Answer: B", "stop")]);
        let missing = ask(&endpoint(address), &bounds(10, 2), "q").unwrap_err();
        assert_eq!(missing.reason, Reason::ProviderError);
        assert_eq!(connections.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_request_carries_the_runtime_settings() {
        let (address, _, requests) =
            scripted_server_recording(Framing::Chunked, vec![answer("Answer: A", "stop")]);
        let endpoint = Endpoint {
            api_key: Some("sk-test".into()),
            reasoning_mode: Some(ReasoningMode::Low),
            ..endpoint(address)
        };
        let mut limits = bounds(10, 1);
        limits.max_tokens = Some(64);
        ask(&endpoint, &limits, "the question").unwrap();
        let sent = requests.lock().unwrap().join("\n");
        assert!(sent.contains("Bearer sk-test"), "{sent}");
        assert!(sent.contains("\"reasoning_effort\":\"low\""), "{sent}");
        assert!(sent.contains("\"max_tokens\":64"), "{sent}");
        assert!(sent.contains("\"include_usage\":true"), "{sent}");
        assert!(sent.contains("the question"), "{sent}");
    }

    #[test]
    fn an_anthropic_stream_is_read_to_message_stop() {
        let events = [
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 20, "output_tokens": 1}}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Answer: "}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "D"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 4}}),
            json!({"type": "message_stop"}),
        ];
        let (address, _, requests) =
            scripted_server_recording(Framing::Chunked, vec![scripted(&events, Ending::CutOff)]);
        let anthropic = Endpoint {
            wire: Wire::Anthropic,
            url: format!("http://{address}/v1/messages"),
            model: "stub".into(),
            api_key: Some("k".into()),
            reasoning_mode: Some(ReasoningMode::Low),
        };
        let got = ask(&anthropic, &bounds(10, 1), "q").unwrap();
        assert_eq!(got.text, "Answer: D");
        assert_eq!(
            (got.prompt_tokens, got.completion_tokens),
            (Some(20), Some(4))
        );
        let sent = requests.lock().unwrap().join("\n").to_ascii_lowercase();
        assert!(sent.contains("x-api-key: k"), "{sent}");
        assert!(sent.contains("\"budget_tokens\":4000"), "{sent}");
        assert!(sent.contains("\"max_tokens\":5024"), "{sent}");

        // Without message_stop the same stream is truncated, not an answer.
        let (address, _) = scripted_server(
            Framing::Chunked,
            vec![scripted(&events[..4], Ending::CutOff)],
        );
        let anthropic = Endpoint {
            url: format!("http://{address}/v1/messages"),
            ..anthropic
        };
        assert_eq!(
            ask(&anthropic, &bounds(10, 1), "q").unwrap_err().reason,
            Reason::Truncated
        );
    }
}
