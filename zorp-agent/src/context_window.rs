//! What the context window is, how full it is, and what to do when it fills.
//!
//! zorp talks to arbitrary OpenAI-compatible and Anthropic endpoints, local
//! Ollama included, and there is no reliable universal way to ask any of them
//! how large their context window is. So this module never guesses one. The
//! window is `None` until somebody says otherwise, through
//! `ZORP_CONTEXT_TOKENS` or a caller that knows. Everything here degrades to
//! the older byte cap when the window is unknown, which is what shipped
//! before and is still better than a confident wrong number.
//!
//! Three separate things live here because they are the same subject seen
//! from three sides:
//!
//! - `TokenUsage`, what the provider said the last request actually cost.
//! - `estimate_tokens`, what zorp can work out on its own when the provider
//!   said nothing. Always labelled as an estimate, never mixed with the above.
//! - `compact_tool_results` and `plan_seed`, what to elide when the
//!   transcript will not fit.
//!
//! Compaction here is deterministic and mechanical. No model writes a summary
//! of the conversation, on purpose: a summary is a second chance to
//! hallucinate, and when it is wrong the thing it replaced is no longer in the
//! request to contradict it. Eliding a tool result or an old tool-call body
//! leaves a marker saying exactly how many bytes went and where, which the
//! model can read and the user is told about.
//!
//! Nothing here writes to the store. Compaction changes what is *sent*, never
//! what was *said*. The durable transcript is evidence and must not move under
//! anybody.

use crate::model::{ContentPart, Message, MessageMetadata, MessageRecord};
use serde_json::Value;

/// Total bytes of tool-result content and assistant tool-call arguments a
/// transcript may accumulate before old bodies are elided. Generous on
/// purpose: below this the transcript is sent verbatim. This is the floor that
/// applies when no context window is configured, which is the default.
pub const TOOL_RESULT_HISTORY_BUDGET_BYTES: usize = 512 * 1024;

/// Marker prefix left in place of an elided tool-result body.
pub const ELIDED_MARKER_PREFIX: &str = "[tool result elided:";

/// Marker prefix left in place of an elided assistant tool-call argument.
pub const ELIDED_ARGUMENT_MARKER_PREFIX: &str = "[tool argument elided:";

/// String arguments at or below this size still tell the model what the tool
/// did. Larger ones can hold whole source files and are eligible for elision.
const SMALL_TOOL_ARGUMENT_BYTES: usize = 1024;

/// Body left in place of a tool result that was never recorded, so a persisted
/// assistant turn carrying tool calls is never replayed with a dangling call.
pub const MISSING_RESULT_BODY: &str =
    "[no result recorded: the run ended before this tool call returned]";

/// Rough bytes per token. Deliberately crude. Every number derived from it is
/// presented as an estimate and never as a measurement.
const BYTES_PER_TOKEN: u64 = 4;

/// Flat per-message overhead in the estimate, for role tags and framing.
const MESSAGE_OVERHEAD_TOKENS: u64 = 4;

/// Default share of the window the transcript may fill before compaction.
///
/// The transcript is not the whole request: the tool schemas, and the reply
/// the model has yet to write, share the window with it. Compacting only when
/// the transcript alone fills the window is how a run compacts and then
/// overflows anyway.
const DEFAULT_HEADROOM: f64 = 0.75;

/// Where a token count came from. The distinction is the whole point: one of
/// these is a fact about a request that happened, the other is arithmetic on
/// string lengths, and a UI that shows them identically is lying about one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageSource {
    /// The provider reported this for the request that was actually sent.
    Reported,
    /// zorp counted it from the transcript. Approximate.
    Estimated,
}

impl UsageSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            UsageSource::Reported => "reported",
            UsageSource::Estimated => "estimated",
        }
    }
}

/// Token counts a provider reported for one request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TokenUsage {
    /// Everything the request carried, prompt and transcript. This is the
    /// number that matters for "how much window is left".
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

impl TokenUsage {
    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none() && self.output_tokens.is_none()
    }
}

/// How full the window is, and how sure we are of that.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextUsage {
    pub used_tokens: u64,
    pub source: UsageSource,
    /// The window, when anybody has said what it is. `None` means unknown, and
    /// a surface showing this must say so rather than invent a denominator.
    pub limit_tokens: Option<u64>,
}

/// Parse `usage` from a provider response, in either dialect.
///
/// OpenAI says `prompt_tokens`/`completion_tokens`. Anthropic says
/// `input_tokens`/`output_tokens` and reports cache hits separately, so those
/// are added back in: cached input still occupies the window.
pub fn parse_token_usage(resp: &Value) -> Option<TokenUsage> {
    let usage = resp.get("usage").filter(|u| !u.is_null())?;
    let field = |name: &str| usage.get(name).and_then(Value::as_u64);

    let input = match (field("prompt_tokens"), field("input_tokens")) {
        (Some(t), _) => Some(t),
        (None, Some(t)) => Some(
            t + field("cache_read_input_tokens").unwrap_or(0)
                + field("cache_creation_input_tokens").unwrap_or(0),
        ),
        (None, None) => None,
    };
    let output = field("completion_tokens").or_else(|| field("output_tokens"));

    let parsed = TokenUsage {
        input_tokens: input,
        output_tokens: output,
    };
    (!parsed.is_empty()).then_some(parsed)
}

/// Serialized length of a JSON value, counted without building the string.
/// This runs over every tool call in the transcript on every estimate, and
/// tool arguments carry whole file bodies.
fn json_len(v: &Value) -> u64 {
    struct Count(u64);
    impl std::io::Write for Count {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len() as u64;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    let _ = serde_json::to_writer(&mut count, v);
    count.0
}

/// A crude token estimate for one message. Bytes over four, plus framing.
pub fn estimate_message_tokens(m: &Message) -> u64 {
    let mut bytes = m.text().len() as u64;
    for call in &m.tool_calls {
        bytes += call.name.len() as u64 + json_len(&call.arguments);
    }
    if let Some(id) = &m.tool_call_id {
        bytes += id.len() as u64;
    }
    bytes / BYTES_PER_TOKEN + MESSAGE_OVERHEAD_TOKENS
}

/// A crude token estimate for a transcript. Never shown without saying it is
/// an estimate.
pub fn estimate_tokens(messages: &[Message]) -> u64 {
    messages.iter().map(estimate_message_tokens).sum()
}

/// The size of the window and how much of it the transcript may use.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextBudget {
    /// The model's context window in tokens, when known. `None` is the honest
    /// default and disables every token-driven decision here.
    pub limit_tokens: Option<u64>,
    /// Share of the window the transcript may fill.
    pub headroom: f64,
    /// The always-on byte cap on tool results and tool-call arguments.
    pub tool_result_bytes: usize,
}

impl Default for ContextBudget {
    fn default() -> Self {
        ContextBudget {
            limit_tokens: None,
            headroom: DEFAULT_HEADROOM,
            tool_result_bytes: TOOL_RESULT_HISTORY_BUDGET_BYTES,
        }
    }
}

impl ContextBudget {
    /// Read the window from the environment. Unset, blank or unparseable all
    /// mean unknown, which is the same as not configuring it: no guess is
    /// better than a wrong one.
    pub fn from_env() -> Self {
        let limit_tokens = std::env::var("ZORP_CONTEXT_TOKENS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|n| *n > 0);
        let headroom = std::env::var("ZORP_CONTEXT_HEADROOM")
            .ok()
            .and_then(|raw| raw.trim().parse::<f64>().ok())
            .filter(|f| *f > 0.0 && *f <= 1.0)
            .unwrap_or(DEFAULT_HEADROOM);
        ContextBudget {
            limit_tokens,
            headroom,
            ..ContextBudget::default()
        }
    }

    pub fn with_limit(mut self, limit_tokens: Option<u64>) -> Self {
        self.limit_tokens = limit_tokens;
        self
    }

    /// Tokens the transcript may occupy before compaction, when the window is
    /// known at all.
    pub fn target_tokens(&self) -> Option<u64> {
        self.limit_tokens
            .map(|limit| ((limit as f64) * self.headroom) as u64)
    }
}

/// What one compaction pass threw away. Empty means the transcript was left
/// exactly as it was.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompactionReport {
    /// Tool results whose bodies were replaced with a marker.
    pub elided_tool_results: usize,
    /// Bytes of tool-result body removed.
    pub elided_bytes: usize,
    /// Assistant tool-call argument bodies replaced with a marker.
    pub elided_tool_arguments: usize,
    /// Bytes of assistant tool-call argument body removed.
    pub elided_argument_bytes: usize,
    /// Whole messages dropped from the front of a seeded transcript. Only ever
    /// non-zero on the seed path, never inside a live run.
    pub dropped_messages: usize,
}

impl CompactionReport {
    pub fn is_empty(&self) -> bool {
        self.elided_tool_results == 0
            && self.elided_tool_arguments == 0
            && self.dropped_messages == 0
    }

    /// One line for a user, because silent context loss is how an agent starts
    /// confidently contradicting itself.
    pub fn notice(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        if self.elided_tool_results > 0 {
            parts.push(format!(
                "{} older tool result{} elided ({} bytes)",
                self.elided_tool_results,
                if self.elided_tool_results == 1 {
                    ""
                } else {
                    "s"
                },
                self.elided_bytes
            ));
        }
        if self.elided_tool_arguments > 0 {
            parts.push(format!(
                "{} older tool call argument{} elided ({} bytes)",
                self.elided_tool_arguments,
                if self.elided_tool_arguments == 1 {
                    ""
                } else {
                    "s"
                },
                self.elided_argument_bytes
            ));
        }
        if self.dropped_messages > 0 {
            parts.push(format!(
                "{} older message{} dropped from this request",
                self.dropped_messages,
                if self.dropped_messages == 1 { "" } else { "s" }
            ));
        }
        Some(format!(
            "context compaction: {}. The full transcript is still on disk.",
            parts.join(", ")
        ))
    }
}

fn is_elided(m: &Message) -> bool {
    m.text().starts_with(ELIDED_MARKER_PREFIX)
}

fn tool_result_bytes(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| m.role == "tool")
        .map(|m| m.text().len())
        .sum()
}

/// Serialized bytes in assistant tool-call arguments. This includes the small
/// path and command fields too: they stay in the prompt, so they count toward
/// the floor even though only large string values are eligible for elision.
fn tool_call_argument_bytes(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| m.role == "assistant")
        .flat_map(|m| &m.tool_calls)
        .map(|call| json_len(&call.arguments) as usize)
        .sum()
}

/// True when the transcript is over either budget.
fn over_budget(messages: &[Message], budget: &ContextBudget) -> bool {
    if tool_result_bytes(messages) + tool_call_argument_bytes(messages) > budget.tool_result_bytes {
        return true;
    }
    match budget.target_tokens() {
        Some(target) => estimate_tokens(messages) > target,
        None => false,
    }
}

fn argument_marker(body_len: usize) -> String {
    format!(
        "{ELIDED_ARGUMENT_MARKER_PREFIX} {body_len} bytes. The file on disk is the source of truth; use read_file to retrieve it.]"
    )
}

/// Replace the first eligible string value in a JSON argument tree. Object
/// iteration and array order are deterministic, so a transcript is compacted
/// the same way every time.
fn elide_first_large_argument(value: &mut Value) -> Option<usize> {
    match value {
        Value::String(body)
            if body.len() > SMALL_TOOL_ARGUMENT_BYTES
                && !body.starts_with(ELIDED_ARGUMENT_MARKER_PREFIX) =>
        {
            let body_len = body.len();
            *body = argument_marker(body_len);
            Some(body_len)
        }
        Value::Array(values) => values.iter_mut().find_map(elide_first_large_argument),
        Value::Object(values) => values.values_mut().find_map(elide_first_large_argument),
        _ => None,
    }
}

fn elide_first_assistant_argument(message: &mut Message) -> Option<usize> {
    for call in &mut message.tool_calls {
        if let Some(body_len) = elide_first_large_argument(&mut call.arguments) {
            message.invalidate_body_cache();
            return Some(body_len);
        }
    }
    None
}

/// Elide the oldest tool-result bodies, then old assistant tool-call argument
/// bodies, until the transcript fits. Each leaves a marker saying how much
/// went.
///
/// Tool results belonging to the most recent assistant turn are never touched:
/// the model is mid-thought about those, and taking them away is how a run
/// starts over. Messages are only rewritten in place, never removed, because
/// the agent's recorder tracks how much it has persisted by index and a
/// shifting index would re-record or skip.
///
/// Tool results go first because they are usually where the bytes are. After
/// them, old assistant arguments go in transcript order. Tool calls from the
/// most recent assistant turn stay intact because the model is mid-thought
/// about them.
pub fn compact_tool_results(messages: &mut [Message], budget: &ContextBudget) -> CompactionReport {
    let mut report = CompactionReport::default();
    if !over_budget(messages, budget) {
        return report;
    }
    let last_assistant = messages
        .iter()
        .rposition(|m| m.role == "assistant")
        .unwrap_or(0);
    for i in 0..last_assistant {
        if !over_budget(messages, budget) {
            break;
        }
        let m = &mut messages[i];
        if m.role != "tool" || is_elided(m) {
            continue;
        }
        let body_len = m.text().len();
        let marker = format!("{ELIDED_MARKER_PREFIX} {body_len} bytes]");
        m.content = vec![ContentPart::Text(marker)];
        m.invalidate_body_cache();
        report.elided_tool_results += 1;
        report.elided_bytes += body_len;
    }

    for i in 0..last_assistant {
        while over_budget(messages, budget) {
            let Some(body_len) = elide_first_assistant_argument(&mut messages[i]) else {
                break;
            };
            report.elided_tool_arguments += 1;
            report.elided_argument_bytes += body_len;
        }
        if !over_budget(messages, budget) {
            break;
        }
    }
    report
}

/// Repair tool-call integrity in a transcript that is about to be sent.
///
/// Providers reject a request where an assistant message announces a tool call
/// with no matching result, and Anthropic rejects the reverse too. A stored
/// transcript can be truncated in exactly that way: the server was killed, or
/// the turn was cancelled, between recording the call and recording its
/// result. Replaying it unrepaired turns "reopen an old chat" into a 400 from
/// the provider with nothing on screen to explain it.
///
/// Every announced call gets a result. A call with no recorded result gets a
/// synthetic one saying so, rather than having the call quietly deleted:
/// deleting it would leave the assistant's own text claiming it did something
/// with no trace of the attempt. A result with no announced call is dropped,
/// since there is nothing it can be attached to.
pub fn repair_tool_calls(messages: Vec<Message>) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    let mut index = 0usize;
    while index < messages.len() {
        let message = messages[index].clone();
        index += 1;

        if message.role == "tool" {
            // An orphan result. Anything legitimate was already consumed by
            // the assistant message that announced it, below.
            continue;
        }
        let announced: Vec<String> = message.tool_calls.iter().map(|c| c.id.clone()).collect();
        out.push(message);
        if announced.is_empty() {
            continue;
        }

        // Collect the results that immediately follow, in the order the calls
        // were announced, and invent the missing ones.
        let mut results: Vec<Message> = Vec::new();
        while index < messages.len() && messages[index].role == "tool" {
            results.push(messages[index].clone());
            index += 1;
        }
        for id in announced {
            match results
                .iter()
                .position(|r| r.tool_call_id.as_deref() == Some(id.as_str()))
            {
                Some(at) => out.push(results.remove(at)),
                None => out.push(Message::tool_result(id, MISSING_RESULT_BODY)),
            }
        }
    }
    out
}

/// The transcript a turn should start from, plus what it cost to get there.
#[derive(Clone, Debug)]
pub struct SeedPlan {
    pub records: Vec<MessageRecord>,
    pub report: CompactionReport,
}

/// Build the transcript a new turn starts from out of what the store has.
///
/// The stored record is the input and is never modified. What comes back is
/// what should be *sent*.
///
/// Stored system messages are dropped and one current system message is put at
/// the front. The prompt is the harness's to set, not the record's to pin, and
/// a session recorded before a prompt change should not keep re-sending the
/// old one. It also means a transcript carrying several stored copies of the
/// prompt, which is what the web server used to write, collapses back to one.
///
/// Over budget, the oldest exchanges go first, whole. A user message and
/// everything that answered it leave together, so the model never sees a reply
/// to a question that is no longer there.
pub fn plan_seed(stored: Vec<MessageRecord>, system: &str, budget: &ContextBudget) -> SeedPlan {
    let mut report = CompactionReport::default();

    let mut body: Vec<Message> = stored
        .into_iter()
        .map(|record| record.message)
        .filter(|m| m.role != "system")
        .collect();
    body = repair_tool_calls(body);

    // Drop whole exchanges off the front while the rest is still too big. The
    // system message is counted in but never dropped.
    loop {
        let mut candidate = Vec::with_capacity(body.len() + 1);
        candidate.push(Message::system(system));
        candidate.extend(body.iter().cloned());
        if !over_budget(&candidate, budget) || body.is_empty() {
            break;
        }
        // One exchange: the leading user message and everything up to the next
        // one. A transcript that starts mid-exchange loses its head instead.
        let mut cut = 1usize;
        while cut < body.len() && body[cut].role != "user" {
            cut += 1;
        }
        // Never drop the whole transcript: the newest exchange is the one the
        // user is talking about. Fall through to elision instead.
        if cut >= body.len() {
            break;
        }
        body.drain(0..cut);
        report.dropped_messages += cut;
    }

    body = repair_tool_calls(body);
    let mut messages = Vec::with_capacity(body.len() + 1);
    messages.push(Message::system(system));
    messages.extend(body);

    let elision = compact_tool_results(&mut messages, budget);
    report.elided_tool_results += elision.elided_tool_results;
    report.elided_bytes += elision.elided_bytes;
    report.elided_tool_arguments += elision.elided_tool_arguments;
    report.elided_argument_bytes += elision.elided_argument_bytes;

    SeedPlan {
        records: messages
            .into_iter()
            .map(|message| MessageRecord {
                message,
                metadata: MessageMetadata::default(),
            })
            .collect(),
        report,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ToolCall;
    use serde_json::json;

    fn call(id: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path": "a.txt"}),
        }
    }

    fn write_call(id: &str, content: String) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: "write_file".to_string(),
            arguments: json!({"path": "/app/solve.py", "content": content}),
        }
    }

    fn record(m: Message) -> MessageRecord {
        MessageRecord::from(m)
    }

    #[test]
    fn openai_usage_is_read_from_prompt_and_completion_tokens() {
        let usage = parse_token_usage(&json!({
            "usage": {"prompt_tokens": 1200, "completion_tokens": 42}
        }))
        .unwrap();
        assert_eq!(usage.input_tokens, Some(1200));
        assert_eq!(usage.output_tokens, Some(42));
    }

    #[test]
    fn anthropic_usage_is_read_from_input_and_output_tokens() {
        let usage = parse_token_usage(&json!({
            "usage": {"input_tokens": 900, "output_tokens": 17}
        }))
        .unwrap();
        assert_eq!(usage.input_tokens, Some(900));
        assert_eq!(usage.output_tokens, Some(17));
    }

    /// Cached input still occupies the window, so it counts.
    #[test]
    fn anthropic_cache_tokens_count_toward_the_window() {
        let usage = parse_token_usage(&json!({
            "usage": {
                "input_tokens": 100,
                "cache_read_input_tokens": 4000,
                "cache_creation_input_tokens": 500,
                "output_tokens": 5
            }
        }))
        .unwrap();
        assert_eq!(usage.input_tokens, Some(4600));
    }

    #[test]
    fn a_response_without_usage_reports_none() {
        assert_eq!(parse_token_usage(&json!({"choices": []})), None);
        assert_eq!(parse_token_usage(&json!({"usage": null})), None);
        assert_eq!(parse_token_usage(&json!({"usage": {}})), None);
    }

    #[test]
    fn an_unknown_window_yields_no_token_target() {
        assert_eq!(ContextBudget::default().target_tokens(), None);
    }

    #[test]
    fn a_known_window_leaves_headroom() {
        let budget = ContextBudget::default().with_limit(Some(8000));
        assert_eq!(budget.target_tokens(), Some(6000));
    }

    #[test]
    fn estimating_a_transcript_scales_with_its_size() {
        let small = vec![Message::user("hello")];
        let large = vec![Message::user("hello".repeat(1000))];
        assert!(estimate_tokens(&large) > estimate_tokens(&small) * 100);
    }

    /// The bug this whole module exists to stop: a stored assistant turn that
    /// announced a tool call, with the result never recorded because the
    /// process died. Sent as-is, providers reject the request.
    #[test]
    fn a_dangling_tool_call_gets_a_result() {
        let messages = vec![
            Message::user("convert the file"),
            Message::assistant_with_calls("running pandoc", vec![call("c1")]),
        ];

        let repaired = repair_tool_calls(messages);

        assert_eq!(repaired.len(), 3);
        assert_eq!(repaired[2].role, "tool");
        assert_eq!(repaired[2].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(repaired[2].text(), MISSING_RESULT_BODY);
    }

    #[test]
    fn a_partially_answered_tool_turn_gets_the_missing_results_only() {
        let messages = vec![
            Message::assistant_with_calls("two things", vec![call("c1"), call("c2")]),
            Message::tool_result("c2", "the second one landed"),
        ];

        let repaired = repair_tool_calls(messages);

        assert_eq!(repaired.len(), 3);
        assert_eq!(repaired[1].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(repaired[1].text(), MISSING_RESULT_BODY);
        assert_eq!(repaired[2].tool_call_id.as_deref(), Some("c2"));
        assert_eq!(repaired[2].text(), "the second one landed");
    }

    #[test]
    fn a_tool_result_with_no_call_is_dropped() {
        let messages = vec![
            Message::user("hi"),
            Message::tool_result("ghost", "output from a call nobody made"),
            Message::assistant("hello"),
        ];

        let repaired = repair_tool_calls(messages);

        assert!(
            repaired.iter().all(|m| m.role != "tool"),
            "an orphan tool result survived: {:?}",
            repaired.iter().map(|m| &m.role).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_well_formed_transcript_is_left_alone() {
        let messages = vec![
            Message::user("hi"),
            Message::assistant_with_calls("looking", vec![call("c1")]),
            Message::tool_result("c1", "contents"),
            Message::assistant("done"),
        ];

        assert_eq!(repair_tool_calls(messages.clone()), messages);
    }

    #[test]
    fn seeding_puts_the_current_system_prompt_first_and_only_once() {
        let stored = vec![
            record(Message::system("an old prompt")),
            record(Message::user("first")),
            record(Message::assistant("answer")),
            record(Message::system("the same old prompt again")),
            record(Message::user("second")),
        ];

        let plan = plan_seed(stored, "the current prompt", &ContextBudget::default());

        let roles: Vec<&str> = plan
            .records
            .iter()
            .map(|r| r.message.role.as_str())
            .collect();
        assert_eq!(roles, vec!["system", "user", "assistant", "user"]);
        assert_eq!(plan.records[0].message.text(), "the current prompt");
    }

    #[test]
    fn seeding_keeps_the_conversation_in_order() {
        let stored = vec![
            record(Message::system("prompt")),
            record(Message::user("convert a.md with pandoc")),
            record(Message::assistant("converted it")),
        ];

        let plan = plan_seed(stored, "prompt", &ContextBudget::default());

        assert_eq!(plan.records[1].message.text(), "convert a.md with pandoc");
        assert_eq!(plan.records[2].message.text(), "converted it");
        assert!(plan.report.is_empty());
    }

    /// Truncation must not produce a request a provider will refuse.
    #[test]
    fn seeding_a_truncated_transcript_leaves_no_dangling_call() {
        let stored = vec![
            record(Message::system("prompt")),
            record(Message::user("do it")),
            record(Message::assistant_with_calls("on it", vec![call("c1")])),
        ];

        let plan = plan_seed(stored, "prompt", &ContextBudget::default());

        let last = &plan.records[plan.records.len() - 1].message;
        assert_eq!(last.role, "tool");
        assert_eq!(last.tool_call_id.as_deref(), Some("c1"));
    }

    /// Dropping the front of a transcript must not orphan the results of a
    /// call that was announced in a message that just left.
    #[test]
    fn dropping_old_exchanges_leaves_no_orphan_results() {
        let budget = ContextBudget::default().with_limit(Some(400));
        let stored = vec![
            record(Message::system("prompt")),
            record(Message::user("old question ".repeat(60))),
            record(Message::assistant_with_calls("old work", vec![call("c1")])),
            record(Message::tool_result("c1", "old output ".repeat(60))),
            record(Message::assistant("old answer")),
            record(Message::user("the new question")),
        ];

        let plan = plan_seed(stored, "prompt", &budget);

        assert!(plan.report.dropped_messages > 0, "nothing was dropped");
        let messages: Vec<Message> = plan.records.iter().map(|r| r.message.clone()).collect();
        assert_eq!(
            repair_tool_calls(messages.clone()),
            messages,
            "the seeded transcript still needed repair"
        );
        assert_eq!(messages.last().unwrap().text(), "the new question");
    }

    /// The newest exchange is what the user is talking about. It stays even if
    /// it alone is over budget: tool-result elision handles that case, and
    /// sending nothing is not an improvement on sending too much.
    #[test]
    fn the_newest_exchange_is_never_dropped() {
        let budget = ContextBudget::default().with_limit(Some(10));
        let stored = vec![
            record(Message::system("prompt")),
            record(Message::user("the only question ".repeat(100))),
        ];

        let plan = plan_seed(stored, "prompt", &budget);

        assert_eq!(plan.records.len(), 2);
        assert!(plan.records[1]
            .message
            .text()
            .starts_with("the only question"));
    }

    #[test]
    fn compaction_elides_the_oldest_tool_results_first() {
        let budget = ContextBudget {
            tool_result_bytes: 150,
            ..ContextBudget::default()
        };
        let mut messages = vec![
            Message::system("prompt"),
            Message::user("task"),
            Message::assistant_with_calls("", vec![]),
            Message::tool_result("c1", "x".repeat(100)),
            Message::assistant_with_calls("", vec![]),
            Message::tool_result("c2", "y".repeat(100)),
        ];

        let report = compact_tool_results(&mut messages, &budget);

        assert_eq!(messages[3].text(), "[tool result elided: 100 bytes]");
        assert_eq!(messages[5].text(), "y".repeat(100));
        assert_eq!(report.elided_tool_results, 1);
        assert_eq!(report.elided_bytes, 100);
    }

    /// Removing argument bodies must catch the old drafts that made a long
    /// write_file run grow forever, while preserving enough of the call for
    /// the model to read the file again.
    #[test]
    fn compaction_elides_old_assistant_argument_bodies_but_not_the_latest_turn() {
        let budget = ContextBudget {
            tool_result_bytes: 150,
            ..ContextBudget::default()
        };
        let old_body = "x".repeat(2_000);
        let latest_body = "y".repeat(2_000);
        let mut messages = vec![
            Message::system("prompt"),
            Message::assistant_with_calls("", vec![write_call("c1", old_body.clone())]),
            Message::assistant_with_calls("", vec![write_call("c2", latest_body.clone())]),
        ];

        let report = compact_tool_results(&mut messages, &budget);

        let old = &messages[1].tool_calls[0];
        assert_eq!(old.name, "write_file");
        assert_eq!(old.id, "c1");
        assert_eq!(old.arguments["path"], "/app/solve.py");
        let marker = old.arguments["content"].as_str().unwrap();
        assert!(
            marker.starts_with(ELIDED_ARGUMENT_MARKER_PREFIX),
            "{marker}"
        );
        assert!(marker.contains("2000 bytes"), "{marker}");
        assert!(marker.contains("source of truth"), "{marker}");
        assert!(marker.contains("read_file"), "{marker}");
        assert_eq!(messages[2].tool_calls[0].arguments["content"], latest_body);
        assert_eq!(report.elided_tool_arguments, 1);
        assert_eq!(report.elided_argument_bytes, 2_000);
    }

    #[test]
    fn compaction_leaves_argument_bodies_byte_for_byte_under_budget() {
        let calls = vec![write_call("c1", "x".repeat(2_000))];
        let mut messages = vec![
            Message::assistant_with_calls("", calls),
            Message::assistant(""),
        ];
        let original = messages.clone();

        let report = compact_tool_results(&mut messages, &ContextBudget::default());

        assert_eq!(messages, original);
        assert!(report.is_empty());
    }

    #[test]
    fn compaction_elides_tool_results_before_assistant_arguments() {
        let budget = ContextBudget {
            tool_result_bytes: 2_100,
            ..ContextBudget::default()
        };
        let mut messages = vec![
            Message::assistant_with_calls("", vec![write_call("c1", "x".repeat(2_000))]),
            Message::tool_result("r1", "r".repeat(2_000)),
            Message::assistant_with_calls("", vec![]),
        ];

        let report = compact_tool_results(&mut messages, &budget);

        assert!(messages[1].text().starts_with(ELIDED_MARKER_PREFIX));
        assert_eq!(
            messages[0].tool_calls[0].arguments["content"],
            "x".repeat(2_000)
        );
        assert_eq!(report.elided_tool_results, 1);
        assert_eq!(report.elided_tool_arguments, 0);
    }

    #[test]
    fn estimating_tokens_counts_tool_argument_bytes() {
        let without_body = Message::assistant_with_calls("", vec![write_call("c1", String::new())]);
        let with_body =
            Message::assistant_with_calls("", vec![write_call("c1", "x".repeat(4_000))]);

        assert!(
            estimate_message_tokens(&with_body) > estimate_message_tokens(&without_body) + 900,
            "tool argument bytes did not materially change the estimate"
        );
    }

    /// A token budget compacts a transcript the byte cap would wave through.
    #[test]
    fn a_token_budget_compacts_below_the_byte_cap() {
        let budget = ContextBudget::default().with_limit(Some(100));
        let mut messages = vec![
            Message::system("prompt"),
            Message::assistant_with_calls("", vec![]),
            Message::tool_result("c1", "x".repeat(2000)),
            Message::assistant("done"),
        ];
        assert!(
            tool_result_bytes(&messages) < TOOL_RESULT_HISTORY_BUDGET_BYTES,
            "this case is meant to be under the byte cap"
        );

        let report = compact_tool_results(&mut messages, &budget);

        assert_eq!(report.elided_tool_results, 1);
        assert!(messages[2].text().starts_with(ELIDED_MARKER_PREFIX));
    }

    #[test]
    fn compaction_says_what_it_took() {
        let report = CompactionReport {
            elided_tool_results: 2,
            elided_bytes: 4096,
            elided_tool_arguments: 3,
            elided_argument_bytes: 8192,
            dropped_messages: 3,
        };
        let notice = report.notice().unwrap();
        assert!(notice.contains("2 older tool results elided"), "{notice}");
        assert!(
            notice.contains("3 older tool call arguments elided"),
            "{notice}"
        );
        assert!(notice.contains("3 older messages dropped"), "{notice}");
        assert!(notice.contains("still on disk"), "{notice}");
        assert_eq!(CompactionReport::default().notice(), None);
    }
}
