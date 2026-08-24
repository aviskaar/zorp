//! zorp — the smallest harness of all time.
//! Core: zorp_raw / zorp_stream / zorp_to / zorp, plus small pub helpers
//! (build_body, join_url, env_config, extract_content, init_exports) reused by the
//! binary and future companion crates.

use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

/// Shared boxed error: every fallible fn returns this. Both ureq::Error and
/// serde_json::Error satisfy it, so `?` composes and errors cross into async tasks.
pub type BoxErr = Box<dyn std::error::Error + Send + Sync>;

/// Build an OpenAI-style chat body: optional system message + one user message.
pub fn build_body(system: Option<&str>, prompt: &str, model: &str) -> Value {
    let mut messages = Vec::new();
    if let Some(s) = system {
        messages.push(json!({"role": "system", "content": s}));
    }
    messages.push(json!({"role": "user", "content": prompt}));
    json!({"model": model, "messages": messages})
}

/// Join a base URL and a path with exactly one slash, tolerating trailing/leading
/// slashes on either side (so `…/v1` and `…/v1/` both work).
pub fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Extract assistant text from a buffered chat response. Errors only when there
/// are no choices; a present-but-null/absent content yields "" (tool-call turns).
pub fn extract_content(resp: &Value) -> Result<String, BoxErr> {
    let choices = resp
        .get("choices")
        .and_then(|c| c.as_array())
        .filter(|a| !a.is_empty())
        .ok_or("no choices in response")?;
    Ok(choices[0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}

/// Parse one SSE `data:` payload into its `choices[0].delta` object.
/// Returns None for `[DONE]`, unparseable JSON, or a chunk without a delta.
pub(crate) fn parse_sse_delta(data: &str) -> Option<Value> {
    if data == "[DONE]" {
        return None;
    }
    let mut chunk: Value = serde_json::from_str(data).ok()?;
    // Take the delta out of the parsed chunk instead of deep-cloning it; the
    // rest of the chunk is dropped anyway.
    Some(
        chunk
            .get_mut("choices")?
            .get_mut(0)?
            .get_mut("delta")?
            .take(),
    )
}

/// Shared HTTP agent: built once, cloned per use (a cheap Arc clone). The
/// config is fixed (no per-call knobs), so a single static is enough.
static AGENT: OnceLock<ureq::Agent> = OnceLock::new();

/// Seconds of silence to wait for before giving up on a model, and the
/// variable that overrides it.
///
/// Not 60. A local model that is not resident has to be loaded before it can
/// emit a token, and on ordinary hardware that takes longer than a minute: a
/// cold 4B on an Apple laptop measured 131 seconds to first response. At 60
/// the first message a new user ever sends fails, which reads as "this is
/// broken" rather than "the model is loading".
///
/// 900 and not 300, because an agent loop multiplies this. One attempt is up
/// to 40 model calls and any one of them exceeding the bound kills the whole
/// attempt, so a per-request stall rate of `p` leaves `(1 - p)^40` attempts
/// alive: one request in twenty stalling is seven attempts in eight lost, and
/// one in ten is ninety-nine in a hundred. Measured, at 180 seconds: 9 usable
/// forecasts out of 300 attempts, against 76 out of 123 before any bound
/// existed. That is roughly one request in twelve going quiet for longer than
/// three minutes, which is unremarkable for a reasoning model behind a
/// gateway and catastrophic once it is raised to the fortieth power.
///
/// The number is chosen with the asymmetry in mind rather than from a
/// distribution nobody has. Too long costs one wait on a socket nobody is
/// coming back to; too short costs the run. 900 is five times the value
/// measured to be catastrophic and still catches the 3 hours 18 minutes that
/// put a bound here in the first place, thirteen times over.
///
/// It is also a number that can be wrong safely now, which is the part that
/// matters. Exceeding it used to be silent, so being wrong about it looked
/// like a bad model. See `docs/DECISIONS.md` (2026-08-23).
pub const DEFAULT_READ_TIMEOUT_SECS: u64 = 900;
pub const READ_TIMEOUT_VAR: &str = "ZORP_HTTP_TIMEOUT_SECS";

/// The read timeout in force, in seconds. Public so a caller that has to
/// explain a timeout can name the number the user would have to change.
pub fn read_timeout_secs() -> u64 {
    std::env::var(READ_TIMEOUT_VAR)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_READ_TIMEOUT_SECS)
}

/// The one HTTP agent in the workspace, and the one place timeouts are set.
///
/// Public because it has to be. It was private, and the streaming path in
/// `zorp-agent` reached for `ureq::agent()` instead, which is ureq's default
/// agent: no connect timeout and no read timeout. Every real model call
/// streams, so every real model call ran with no bound at all. A 200 sample
/// calibration run against OpenRouter sat at 0% CPU for 3 hours 18 minutes on
/// an established socket and had to be killed. Two ways to configure one
/// thing are one too many, so there is now one function and callers share it.
///
/// `timeout_read` is per read call, not per request. On a streamed response
/// that makes it an idle timeout: it bounds silence between chunks and says
/// nothing about how long an answer may take, so a model that keeps producing
/// tokens for an hour is fine and a model that stops mid-sentence is not.
///
/// Idle pooling is off because ureq 2.12.1 clears socket timeouts when it
/// returns a connection to the pool and does not restore them before the next
/// request waits for response headers. The body reader restores the timeout,
/// but it does not exist yet in that unbounded window. A fresh connection for
/// every model call keeps the per-read bound armed from the first response
/// byte through the last.
pub fn http_agent() -> ureq::Agent {
    AGENT
        .get_or_init(|| {
            ureq::AgentBuilder::new()
                // Connecting is a different problem from waiting for tokens.
                // A host that will not accept a connection in 30 seconds is
                // down, and making the user wait longer to hear that is not
                // kind.
                .timeout_connect(Duration::from_secs(30))
                .timeout_read(Duration::from_secs(read_timeout_secs()))
                .max_idle_connections(0)
                .build()
        })
        .clone()
}

/// Statuses a request is ever sent a second time for, and what to call each
/// one in a sentence a person will read.
///
/// Both mean the same thing in different words: the provider did not take the
/// request. Nothing was generated, nothing was charged, and what it is asking
/// for is to be asked again shortly. 429 is the measured case. A 250 crate
/// calibration run against OpenRouter's free tier threw away 25 of its first
/// 48 attempts to one, every body saying "Please retry shortly", and an
/// attempt is an agent loop of up to 40 model calls, so one 429 anywhere in
/// it destroys the whole attempt and everything it had gathered.
///
/// 502 and 504 are the interesting omission and they are left out on purpose.
/// Both mean the request was forwarded and something went wrong after that,
/// so a second send can duplicate work an upstream may already have done and
/// charged for, and it has no more reason to succeed than the first did. 400,
/// 401 and 404 are left out for the plainer reason: they will not get better.
/// Retrying them turns a misconfiguration into a slow misconfiguration, which
/// reads like a network problem and costs somebody an afternoon.
pub fn retry_reason(status: u16) -> Option<&'static str> {
    match status {
        429 => Some("rate limited"),
        503 => Some("unavailable"),
        _ => None,
    }
}

/// How many times one request may be sent, counting the first, and the
/// variable that overrides it. 1 turns retrying off.
pub const DEFAULT_RETRY_ATTEMPTS: u32 = 4;
pub const RETRY_ATTEMPTS_VAR: &str = "ZORP_RETRY_ATTEMPTS";

/// The most wall clock retrying may add to one request, summed over its
/// waits, and the variable that overrides it. 0 turns retrying off.
///
/// Two bounds rather than one because they stop different things. The count
/// stops a provider that says no instantly and keeps saying it. The budget
/// stops a provider that says `Retry-After: 600`, which is a legitimate thing
/// for a provider to say and not something a foreground request can honour.
///
/// Both numbers are picked with the browser in mind and not the batch run,
/// because the batch run can afford either and the person cannot. Half a
/// minute is inside the range a model answer already takes, so a turn that
/// was rate limited and recovered looks like a slow turn. A minute or two of
/// a spinner does not look like anything except broken, and at that point the
/// retrying is the outage rather than the cure. The batch case sets the same
/// ceiling from the other side: an attempt is up to 40 model calls, so half a
/// minute each is 20 minutes added to one attempt in the worst case anyone
/// would ever see, and the measured rate limiting is nothing like every call.
pub const DEFAULT_RETRY_BUDGET_SECS: u64 = 30;
pub const RETRY_BUDGET_VAR: &str = "ZORP_RETRY_BUDGET_SECS";

/// The wait before the first retry, doubled for each retry after it.
const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);

/// The most any single backoff may grow to. Only reachable if the attempt
/// count is raised well past its default.
const RETRY_MAX_DELAY: Duration = Duration::from_secs(8);

/// How much is added on top of a `Retry-After` the provider named.
const RETRY_AFTER_JITTER: Duration = Duration::from_millis(250);

/// The bound on retrying: how many sends, and how long they may add in total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Sends allowed for one request, counting the first.
    pub attempts: u32,
    /// The most time this may add to one request, summed over its waits.
    pub budget: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: DEFAULT_RETRY_ATTEMPTS,
            budget: Duration::from_secs(DEFAULT_RETRY_BUDGET_SECS),
        }
    }
}

impl RetryPolicy {
    /// The policy in force. Read per request rather than cached, so a test
    /// can set it and a long-running process can be told to stop retrying
    /// without being restarted.
    pub fn from_env() -> Self {
        let default = Self::default();
        Self {
            attempts: std::env::var(RETRY_ATTEMPTS_VAR)
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                // 0 and 1 both mean "send it once". Nonsense means the
                // default, the same way the read timeout treats it.
                .map(|attempts| attempts.max(1))
                .unwrap_or(default.attempts),
            budget: std::env::var(RETRY_BUDGET_VAR)
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(default.budget),
        }
    }

    /// How long to wait before retry number `retry`, or `None` to stop.
    ///
    /// `retry` counts retries and not sends, so the first retry is 1.
    /// `spent` is what the earlier waits on this same request already cost.
    /// `asked` is the provider's own `Retry-After`, when it sent one.
    ///
    /// A `Retry-After` is a floor and not a suggestion. The provider knows
    /// when its own window reopens and a client that guesses over the top of
    /// being told is a client that comes back early and gets refused for it.
    /// The jitter goes on top rather than inside for that reason: never less
    /// than the number given, and never the same instant as everybody else
    /// who was given it.
    ///
    /// Refusing outright rather than clamping a `Retry-After` that will not
    /// fit the budget is deliberate. Waiting less than the provider asked is
    /// the one thing worse than not waiting at all: it spends a send that
    /// cannot succeed and adds load to something already shedding it.
    pub fn delay(&self, retry: u32, spent: Duration, asked: Option<Duration>) -> Option<Duration> {
        if retry >= self.attempts {
            return None;
        }
        let wait = match asked {
            Some(asked) => asked.saturating_add(jitter(RETRY_AFTER_JITTER)),
            None => {
                // Clamped, not because a caller should count from zero or
                // ask for the thousandth retry, but because a shift is a
                // sharp thing to leave a public argument in front of.
                let doublings = retry.clamp(1, 16) - 1;
                let ceiling = RETRY_BASE_DELAY
                    .saturating_mul(2u32.saturating_pow(doublings))
                    .min(RETRY_MAX_DELAY);
                ceiling / 2 + jitter(ceiling / 2)
            }
        };
        if spent.saturating_add(wait) > self.budget {
            return None;
        }
        Some(wait)
    }
}

/// A random slice of `span`, from none of it to all of it.
///
/// Jitter is not decoration. An attempt is up to 40 model calls in a row and
/// a calibration run is several attempts at once, so a set of callers all
/// told "come back in a second" all come back in the same second and rate
/// limit each other again. A backoff schedule with no random component
/// preserves whatever collision produced it, and doubling the wait each time
/// preserves it at a slower tempo.
///
/// No `rand` dependency for this. The core has two dependencies, and what is
/// needed here is "not identical between processes", which the standard
/// library's randomly seeded hasher and the clock cover between them.
fn jitter(span: Duration) -> Duration {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default(),
    );
    // 53 bits is the whole of an f64 mantissa, so this is uniform over [0, 1).
    let fraction = (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64;
    span.mul_f64(fraction)
}

/// The provider's own `Retry-After`, when it sent one and it is a count of
/// seconds.
///
/// The header has an HTTP-date form too. It is ignored rather than parsed:
/// the core has two dependencies and a date parser would be the largest thing
/// in the crate, a missing header already has a sensible answer in the
/// backoff, and the providers this talks to send the seconds form.
fn retry_after(resp: &ureq::Response) -> Option<Duration> {
    let secs: u64 = resp.header("Retry-After")?.trim().parse().ok()?;
    Some(Duration::from_secs(secs))
}

/// How much of a non-2xx response body gets included in the error message.
/// Enough for any real provider error, small enough to never bloat a message.
const ERROR_BODY_CAP: u64 = 8 * 1024;

/// Send the request, send it again while the provider is asking to be asked
/// again, and turn a status it will not take back into an error that quotes
/// the body. Providers put the useful part ("invalid api key", "model not
/// found", "context length exceeded") in the body, and ureq's own Display
/// drops it.
///
/// Public because `zorp-agent`'s streaming path has to build and send its own
/// request, and it needs both of these behaviors to be the same ones. It had
/// its own copy of the error handling with a comment saying it mirrored this
/// function, and a copy that has to be kept in step by hand is how the
/// streaming path ended up without a timeout for as long as it did.
///
/// Retrying stops here, before the response body exists, and that is the only
/// place it can safely happen. Once bytes have been handed to a caller a
/// second send would replay the start of one answer over the middle of
/// another, so nothing above this line ever retries.
pub fn send_json(req: ureq::Request, body: Value) -> Result<ureq::Response, BoxErr> {
    let policy = RetryPolicy::from_env();
    let mut sent = 0u32;
    let mut waited = Duration::ZERO;
    loop {
        sent += 1;
        // The request is cloned rather than consumed because it may be sent
        // again: it is a URL, a method and a few headers, so the copy is
        // nothing. The body is passed by reference and never copied at all,
        // which matters because a body here is a whole conversation.
        match req.clone().send_json(&body) {
            Ok(resp) => return Ok(resp),
            Err(ureq::Error::Status(code, resp)) => {
                // Nothing to retry, or nothing left to retry with. Either way
                // the caller gets the status and the body behind it.
                let plan = retry_reason(code)
                    .and_then(|why| Some((why, policy.delay(sent, waited, retry_after(&resp))?)));
                let Some((why, wait)) = plan else {
                    return Err(status_error(code, resp, sent, waited));
                };
                // Loud, because a retry nobody can see is a run that got
                // slower for no stated reason. One line per retry on stderr,
                // which is where everything else in the workspace says this
                // kind of thing and where a browser session's server log
                // already goes.
                eprintln!(
                    "zorp: {}: {why} (status code {code}), waiting {:.1}s and \
                     sending again (try {} of {})",
                    resp.get_url(),
                    wait.as_secs_f64(),
                    sent + 1,
                    policy.attempts
                );
                std::thread::sleep(wait);
                waited += wait;
            }
            Err(e) => return Err(transport_error(e)),
        }
    }
}

/// Keep a response-header timeout as legible as a response-body timeout.
///
/// ureq preserves the timed-out `io::Error` while it reads headers, so this
/// path can use the error chain instead of the clock that `stream_sse` needs
/// after ureq's chunk decoder has discarded the original error kind.
fn transport_error(error: ureq::Error) -> BoxErr {
    if error.kind() == ureq::ErrorKind::Io && error_chain_timed_out(&error) {
        let limit = read_timeout_secs();
        format!(
            "the provider sent nothing for {limit} seconds while zorp waited \
             for response headers; set {READ_TIMEOUT_VAR} to wait longer \
             (the transport said: {error})"
        )
        .into()
    } else {
        error.into()
    }
}

fn error_chain_timed_out(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::TimedOut)
        {
            return true;
        }
        current = error.source();
    }
    false
}

/// The error a status the provider will not take back becomes.
///
/// The shape of the first half is unchanged, because other things match on
/// it. What is new is the tail: a request that was sent more than once says
/// so, in words that name what happened and the two variables that bound it.
/// A run losing half its attempts to rate limiting should be able to say that
/// from one error line, rather than from somebody noticing a tally later.
fn status_error(code: u16, resp: ureq::Response, sent: u32, waited: Duration) -> BoxErr {
    let url = resp.get_url().to_string();
    let mut bytes = Vec::new();
    use std::io::Read;
    let _ = resp
        .into_reader()
        .take(ERROR_BODY_CAP)
        .read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim();
    let mut message = if text.is_empty() {
        format!("{url}: status code {code}")
    } else {
        format!("{url}: status code {code}: {text}")
    };
    if let Some(reason) = retry_reason(code) {
        message.push_str(&format!(
            " (still {reason} after {sent} {}, {:.1}s of waiting; \
             {RETRY_ATTEMPTS_VAR} and {RETRY_BUDGET_VAR} bound this)",
            if sent == 1 { "try" } else { "tries" },
            waited.as_secs_f64(),
        ));
    }
    message.into()
}

/// Buffered primitive: POST an arbitrary JSON body to an arbitrary URL with
/// arbitrary headers; return the full parsed response. No path/auth/shape opinions.
/// A non-2xx status becomes an error that includes the response body.
pub fn zorp_raw(url: &str, headers: &[(&str, &str)], body: Value) -> Result<Value, BoxErr> {
    let mut req = http_agent().post(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let resp = send_json(req, body)?;
    let value: Value = resp.into_json()?;
    Ok(value)
}

/// Read the four env knobs, applying defaults for base_url and model.
pub fn env_config() -> (String, Option<String>, String, Option<String>) {
    let base =
        std::env::var("ZORP_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let key = std::env::var("ZORP_API_KEY").ok();
    let model = std::env::var("ZORP_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
    let system = std::env::var("ZORP_SYSTEM").ok();
    (base, key, model, system)
}

/// Convenience: build a single-user-message body, POST to <base_url>/chat/completions
/// with optional Bearer auth, return the assistant text ("" on a tool-only turn).
pub fn zorp_to(
    prompt: &str,
    base_url: &str,
    api_key: Option<&str>,
    model: &str,
) -> Result<String, BoxErr> {
    let url = join_url(base_url, "chat/completions");
    let body = build_body(None, prompt, model);
    let auth = api_key.map(|k| format!("Bearer {k}"));
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if let Some(a) = &auth {
        headers.push(("Authorization", a.as_str()));
    }
    let resp = zorp_raw(&url, &headers, body)?;
    extract_content(&resp)
}

/// Ergonomic: read env config (incl. optional ZORP_SYSTEM), send, return text.
pub fn zorp(prompt: &str) -> Result<String, BoxErr> {
    let (base, key, model, system) = env_config();
    let url = join_url(&base, "chat/completions");
    let body = build_body(system.as_deref(), prompt, &model);
    let auth = key.map(|k| format!("Bearer {k}"));
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if let Some(a) = &auth {
        headers.push(("Authorization", a.as_str()));
    }
    let resp = zorp_raw(&url, &headers, body)?;
    extract_content(&resp)
}

/// Streaming primitive: force stream:true, POST, and deliver each SSE chunk's
/// `choices[0].delta` to `on_delta`; accumulate delta.content into the return String.
/// If the server ignores streaming (no `data:` frames), fall back to buffered: parse
/// the whole body and deliver one synthetic {"content": …} delta — never silent-empty.
pub fn zorp_stream(
    url: &str,
    headers: &[(&str, &str)],
    mut body: Value,
    mut on_delta: impl FnMut(&Value),
) -> Result<String, BoxErr> {
    if let Some(obj) = body.as_object_mut() {
        obj.insert("stream".to_string(), Value::Bool(true));
    }
    let mut req = http_agent().post(url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let resp = send_json(req, body)?;

    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(resp.into_reader());
    // One line buffer reused across frames: read_line + clear instead of a
    // fresh String allocation per SSE frame.
    let mut line = String::new();
    let mut acc = String::new();

    // Find the first non-empty line to decide SSE vs buffered.
    let first = loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            // An empty 200 body must not silently succeed. Surface it (the
            // spec's "never silent-empty" guarantee).
            return Err("empty response body".into());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // SSE comment/heartbeat lines start with ':'. Some proxies emit one
        // before the first data frame. Skip them so they don't trigger the
        // non-SSE fallback (a stray comment must not hard-fail a real stream).
        if trimmed.starts_with(':') {
            continue;
        }
        break trimmed;
    };

    if let Some(payload) = first.strip_prefix("data:") {
        // SSE path: process the first frame, then the rest.
        handle_frame(payload.trim(), &mut acc, &mut on_delta);
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(payload) = trimmed.strip_prefix("data:") {
                let payload = payload.trim();
                if payload == "[DONE]" {
                    break;
                }
                handle_frame(payload, &mut acc, &mut on_delta);
            }
        }
    } else {
        // Non-SSE fallback: reassemble the whole body and parse as buffered.
        // The joining '\n' can double up with read_line's kept newline; JSON
        // ignores the extra whitespace.
        let mut whole = first.to_string();
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            whole.push('\n');
            whole.push_str(&line);
        }
        let resp: Value = serde_json::from_str(&whole)?;
        let content = extract_content(&resp)?;
        on_delta(&json!({"content": content}));
        acc.push_str(&content);
    }
    Ok(acc)
}

fn handle_frame(payload: &str, acc: &mut String, on_delta: &mut impl FnMut(&Value)) {
    if let Some(delta) = parse_sse_delta(payload) {
        if let Some(t) = delta.get("content").and_then(|v| v.as_str()) {
            acc.push_str(t);
        }
        on_delta(&delta);
    }
}

/// Interactive env bootstrap: prompt (on `prompts`) for each knob, read answers
/// from `input`, and return the (var, value) pairs the user actually set (blanks
/// skipped). The binary prints these as `export VAR="value"`; the agent's wizard
/// reuses it for its first section.
pub fn init_exports(
    input: &mut impl std::io::BufRead,
    prompts: &mut impl std::io::Write,
) -> std::io::Result<Vec<(String, String)>> {
    let fields = [
        ("ZORP_BASE_URL", "Base URL [http://localhost:11434/v1]: "),
        ("ZORP_API_KEY", "API key (blank for none): "),
        ("ZORP_MODEL", "Model [gpt-4o]: "),
        ("ZORP_SYSTEM", "System prompt (blank for none): "),
    ];
    let mut out = Vec::new();
    for (var, prompt) in fields {
        write!(prompts, "{prompt}")?;
        prompts.flush()?;
        let mut line = String::new();
        input.read_line(&mut line)?;
        let val = line.trim();
        if !val.is_empty() {
            out.push((var.to_string(), val.to_string()));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_body_user_only() {
        let b = build_body(None, "hi", "m");
        assert_eq!(b["model"], "m");
        assert_eq!(b["messages"].as_array().unwrap().len(), 1);
        assert_eq!(b["messages"][0]["role"], "user");
        assert_eq!(b["messages"][0]["content"], "hi");
    }

    #[test]
    fn build_body_with_system() {
        let b = build_body(Some("sys"), "hi", "m");
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "sys");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn join_url_variants() {
        assert_eq!(
            join_url("http://x/v1", "chat/completions"),
            "http://x/v1/chat/completions"
        );
        assert_eq!(
            join_url("http://x/v1/", "chat/completions"),
            "http://x/v1/chat/completions"
        );
        assert_eq!(
            join_url("http://x/v1", "/chat/completions"),
            "http://x/v1/chat/completions"
        );
    }

    #[test]
    fn extract_content_ok() {
        let r = json!({"choices":[{"message":{"content":"hello"}}]});
        assert_eq!(extract_content(&r).unwrap(), "hello");
    }

    #[test]
    fn extract_content_null_is_empty() {
        let r = json!({"choices":[{"message":{"tool_calls":[]}}]});
        assert_eq!(extract_content(&r).unwrap(), "");
    }

    #[test]
    fn extract_content_no_choices_errs() {
        let r = json!({"error":"x"});
        assert!(extract_content(&r).is_err());
    }

    #[test]
    fn parse_sse_delta_content() {
        let d = parse_sse_delta(r#"{"choices":[{"delta":{"content":"hi"}}]}"#).unwrap();
        assert_eq!(d["content"], "hi");
    }

    #[test]
    fn parse_sse_delta_done_none() {
        assert!(parse_sse_delta("[DONE]").is_none());
    }

    #[test]
    fn parse_sse_delta_bad_json_none() {
        assert!(parse_sse_delta("not json").is_none());
    }

    #[test]
    fn parse_sse_delta_no_delta_none() {
        assert!(parse_sse_delta(r#"{"choices":[{}]}"#).is_none());
    }

    #[test]
    fn init_exports_skips_blanks() {
        use std::io::Cursor;
        // base set, key blank, model set, system blank
        let mut input = Cursor::new("http://localhost:11434/v1\n\nqwen\n\n");
        let mut prompts = Vec::new();
        let pairs = init_exports(&mut input, &mut prompts).unwrap();
        assert_eq!(
            pairs,
            vec![
                (
                    "ZORP_BASE_URL".to_string(),
                    "http://localhost:11434/v1".to_string()
                ),
                ("ZORP_MODEL".to_string(), "qwen".to_string()),
            ]
        );
    }
}

/// The arithmetic of the bound, tested where it costs no wall clock.
///
/// Every case here hands [`RetryPolicy`] an explicit policy rather than
/// setting an environment variable, so nothing in this module can race
/// anything else in the suite and no test has to sleep to find out what the
/// schedule would have been.
#[cfg(test)]
mod retry_tests {
    use super::*;

    const ONE_MINUTE: Duration = Duration::from_secs(60);

    fn policy(attempts: u32) -> RetryPolicy {
        RetryPolicy {
            attempts,
            budget: ONE_MINUTE,
        }
    }

    #[test]
    fn only_a_provider_asking_to_be_asked_again_is_retried() {
        assert_eq!(retry_reason(429), Some("rate limited"));
        assert_eq!(retry_reason(503), Some("unavailable"));
        for refused in [400, 401, 403, 404, 422, 500, 502, 504] {
            assert_eq!(retry_reason(refused), None, "for {refused}");
        }
    }

    /// Four sends means three retries, and the fourth retry is where it
    /// stops. Off-by-one here is the difference between the bound and no
    /// bound at all.
    #[test]
    fn the_attempt_count_is_a_count_of_sends() {
        let policy = policy(4);
        for retry in 1..=3 {
            assert!(
                policy.delay(retry, Duration::ZERO, None).is_some(),
                "retry {retry} was refused inside the bound"
            );
        }
        assert!(policy.delay(4, Duration::ZERO, None).is_none());
        // 1 is the off switch: the request is sent once and never again.
        let once = super::RetryPolicy {
            attempts: 1,
            budget: ONE_MINUTE,
        };
        assert!(once.delay(1, Duration::ZERO, None).is_none());
    }

    /// The backoff grows, and every wait stays inside the window its retry
    /// number allows. Bounds rather than an exact number, because the jitter
    /// is the point.
    #[test]
    fn the_backoff_doubles_and_is_never_the_same_wait_twice() {
        let policy = policy(8);
        for retry in 1..=4u32 {
            let ceiling = RETRY_BASE_DELAY * 2u32.pow(retry - 1);
            for _ in 0..64 {
                let wait = policy.delay(retry, Duration::ZERO, None).unwrap();
                assert!(
                    wait >= ceiling / 2 && wait <= ceiling,
                    "retry {retry} waited {wait:?}, outside [{:?}, {ceiling:?}]",
                    ceiling / 2
                );
            }
        }
        // Sixty four draws landing on one value would mean no jitter at all,
        // which is the failure this is here to catch.
        let waits: std::collections::BTreeSet<_> = (0..64)
            .filter_map(|_| policy.delay(1, Duration::ZERO, None))
            .collect();
        assert!(waits.len() > 1, "every backoff was the same wait");
    }

    /// What the provider asked for is a floor, never a ceiling.
    #[test]
    fn a_retry_after_is_waited_out_in_full() {
        let policy = policy(4);
        let asked = Duration::from_secs(3);
        for _ in 0..32 {
            let wait = policy.delay(1, Duration::ZERO, Some(asked)).unwrap();
            assert!(
                wait >= asked,
                "waited {wait:?}, less than the {asked:?} asked"
            );
            assert!(
                wait <= asked + RETRY_AFTER_JITTER,
                "waited {wait:?}, well past the {asked:?} asked"
            );
        }
    }

    /// The other bound. A provider that asks for longer than the budget is
    /// not haggled with, it is believed and given up on.
    #[test]
    fn a_wait_that_will_not_fit_the_budget_stops_the_retrying() {
        let policy = RetryPolicy {
            attempts: 4,
            budget: Duration::from_secs(30),
        };
        assert!(policy
            .delay(1, Duration::ZERO, Some(Duration::from_secs(600)))
            .is_none());
        // And the budget counts what earlier retries already spent, so a
        // request cannot creep past it one wait at a time.
        assert!(policy
            .delay(1, Duration::from_secs(29), Some(Duration::from_secs(5)))
            .is_none());
        assert!(policy
            .delay(1, Duration::from_secs(20), Some(Duration::from_secs(5)))
            .is_some());
    }

    /// A budget of zero is the off switch that does not need the count
    /// changed, and it must not be reachable by accident: an unset or
    /// nonsense variable means the default and not "never retry".
    #[test]
    fn the_bound_comes_from_the_environment_and_falls_back_to_the_default() {
        let zero = RetryPolicy {
            attempts: 4,
            budget: Duration::ZERO,
        };
        assert!(zero.delay(1, Duration::ZERO, None).is_none());
        assert_eq!(
            RetryPolicy::default(),
            RetryPolicy {
                attempts: DEFAULT_RETRY_ATTEMPTS,
                budget: Duration::from_secs(DEFAULT_RETRY_BUDGET_SECS),
            }
        );
    }
}

#[cfg(test)]
mod timeout_tests {
    use super::*;
    use std::sync::Mutex;

    static ENV: Mutex<()> = Mutex::new(());

    #[test]
    fn the_default_read_timeout_outlasts_a_cold_model_load() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(READ_TIMEOUT_VAR);
        // A cold 4B measured 131 seconds to first response. The default has to
        // clear that with room, or a new user's first message fails.
        assert!(read_timeout_secs() >= 180, "got {}", read_timeout_secs());
    }

    #[test]
    fn the_read_timeout_is_overridable() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(READ_TIMEOUT_VAR, "42");
        assert_eq!(read_timeout_secs(), 42);
        std::env::remove_var(READ_TIMEOUT_VAR);
    }

    /// Nonsense and zero fall back rather than producing an agent that times
    /// out instantly.
    #[test]
    fn a_bad_override_falls_back_to_the_default() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        for bad in ["0", "-5", "soon", ""] {
            std::env::set_var(READ_TIMEOUT_VAR, bad);
            assert_eq!(
                read_timeout_secs(),
                DEFAULT_READ_TIMEOUT_SECS,
                "for {bad:?}"
            );
        }
        std::env::remove_var(READ_TIMEOUT_VAR);
    }
}
