//! Streaming an OpenAI-compatible completion, in pieces that can be tested
//! without a socket.
//!
//! Three parts, deliberately separate:
//!
//! - [`SseDecoder`] turns arbitrary byte chunks into whole `data:` payloads.
//! - [`ThinkGate`] decides which of those characters a user may see.
//! - [`DeltaAccumulator`] rebuilds the finished message.
//!
//! See `docs/superpowers/specs/2026-08-18-streaming-responses-design.md`.

use crate::model::{parse_assistant_completion, ModelCompletion};
use crate::sandbox::CancelToken;
use crate::BoxErr;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// What abandoning a half-arrived response reports.
///
/// An error rather than a third `StreamOutcome`, because there is nothing
/// worth handing back. A response cut off partway is a message the model
/// never finished saying: its text stops mid-sentence and its tool calls may
/// be half-parsed JSON. Anything built from it would have to be repaired by
/// guesswork, and a guessed tool call is worse than no answer.
///
/// Callers tell this apart from a real transport failure by reading the
/// cancel token they own, not by matching on this string.
pub const CANCELLED: &str = "cancelled while the model was still replying";

fn cancelled(cancel: Option<&CancelToken>) -> bool {
    cancel.is_some_and(|c| c.load(Ordering::SeqCst))
}

/// What a stream that stopped before the provider had finished reports.
///
/// A separate sentence from [`CANCELLED`] because it is a separate thing. A
/// cancel is somebody deciding to stop; this is the provider stopping without
/// saying so, and the two need different answers from whoever reads the line.
pub const TRUNCATED: &str = "the stream ended before the provider said it had finished";

/// How far short of the read timeout a failed read may measure and still be
/// counted as that timeout.
///
/// The clock here starts just before the read is issued and the socket's own
/// timer starts a few instructions later, so a real timeout normally measures
/// a shade over the limit and needs no slack at all. This is for the case
/// where it does not: a coarse clock, or a thread descheduled between the two.
/// Small enough that no other failure can reach it, because the only way to
/// have been idle for nearly the whole limit is to have been idle.
const TIMEOUT_SLACK: Duration = Duration::from_millis(250);

/// Say which failure this was, in words a person can act on.
///
/// Whoever reads this line wants two things: that the provider went quiet
/// rather than refused, and which knob buys more patience. ureq's own words
/// give them neither, and on one of the two body framings it gives them
/// worse than neither.
///
/// A close-delimited body is read straight off the socket, so a read timeout
/// arrives as `TimedOut` carrying "timed out reading response". A chunked one
/// is read through a decoder that consumes the chunk body and then reads the
/// framing bytes around it with separate calls, and when one of those fails
/// the decoder throws the reason away and reports `InvalidInput`, "Error
/// while decoding chunks". That is the framing every OpenAI-compatible
/// endpoint behind a CDN uses, so in practice the timeout that mattered was
/// the one that never said its own name. Grepping a 300 attempt log for
/// "timeout" matched nothing, and the run looked like a model problem.
///
/// So the clock decides and not the kind. A read that failed after the socket
/// had been silent for as long as the limit was the limit, whatever ureq
/// chose to call it, and the transport's own words are kept on the end rather
/// than dropped.
fn read_error(e: std::io::Error, quiet_for: Duration) -> BoxErr {
    let limit = zorp::read_timeout_secs();
    let timed_out = e.kind() == std::io::ErrorKind::TimedOut
        || quiet_for + TIMEOUT_SLACK >= Duration::from_secs(limit);
    if timed_out {
        format!(
            "the provider sent nothing for {limit} seconds and the stream was \
             abandoned; set {} to wait longer (the transport said: {e})",
            zorp::READ_TIMEOUT_VAR
        )
        .into()
    } else {
        e.into()
    }
}

/// Did this payload say the provider had finished?
///
/// Two shapes count. `[DONE]` is the sentinel an OpenAI-compatible endpoint
/// writes as the last event, and a non-empty `finish_reason` is the field it
/// sets on the last choice. A provider sends one, the other, or both; a
/// stream carrying neither stopped rather than ended.
///
/// This is not the second interpreter the module doc warns about. It asks
/// whether the stream is over, not what the message says, and it reads no
/// field that goes into the answer. [`DeltaAccumulator`] remains the only
/// place that decides what the model actually said.
///
/// The substring check before the parse is not a micro-optimization worth
/// hiding: a long answer is thousands of content deltas and every one of them
/// would otherwise be parsed twice, once here and once in the accumulator.
fn signals_completion(payload: &str) -> bool {
    let payload = payload.trim();
    if payload == "[DONE]" {
        return true;
    }
    if !payload.contains("finish_reason") {
        return false;
    }
    serde_json::from_str::<Value>(payload)
        .ok()
        .and_then(|value| {
            let reason = value
                .get("choices")?
                .as_array()?
                .first()?
                .get("finish_reason")?
                .as_str()?;
            Some(!reason.is_empty())
        })
        .unwrap_or(false)
}

/// The error a stream that stopped short reports.
fn truncated(events: usize) -> BoxErr {
    format!(
        "{TRUNCATED}: {events} events arrived and then the response ended, \
         with no [DONE] and no finish_reason, so the answer is cut off"
    )
    .into()
}

/// What the provider actually did when asked to stream.
pub enum StreamOutcome {
    /// It streamed. Payloads were handed to the callback as they arrived.
    Streamed,
    /// It ignored `stream` and answered with one whole JSON body.
    ///
    /// Not a rare case worth ignoring: proxies, gateways, mocks and older
    /// local runtimes all do this. Treating their reply as an empty stream
    /// would turn a working endpoint into one that silently answers nothing,
    /// which is the worst shape of failure available here.
    Buffered(Value),
}

/// POST `body` and hand each `data:` payload to `on_payload` as it arrives.
///
/// Separate from `zorp::zorp_raw` rather than folded into it: that function
/// reads the whole response into a `Value`, which is the one thing streaming
/// cannot do.
///
/// `cancel` is checked between reads, so a caller that raises it stops
/// waiting on the model within one chunk instead of at the end of the
/// response. `None` reads to the end exactly as this always did.
///
/// A cancel cannot interrupt a socket that has gone quiet, because the check
/// sits between blocking reads rather than inside one. This used to say that
/// a model producing an answer sends something several times a second, so a
/// quiet socket was not the case worth covering. That was wrong, and it was
/// wrong in the way that costs the most: a 200 sample calibration run against
/// OpenRouter stopped at attempt 123 and then sat at 0% CPU for 3 hours 18
/// minutes, connection still established, nothing arriving, nobody there to
/// press stop. What bounds that is the read timeout on the shared agent, not
/// the cancel token. `zorp::http_agent` sets it, `ZORP_HTTP_TIMEOUT_SECS`
/// overrides it, and because ureq applies it per read it behaves here as an
/// idle timeout: a long answer that keeps producing tokens is never cut off,
/// and a silence longer than the timeout ends as an error.
///
/// **A stream that ends without the provider saying it had finished is an
/// error too, and that is the more important half.** A gateway that hits its
/// own idle limit does not hold the socket open, it ends the response, and a
/// response that ends cleanly halfway through an answer is not a transport
/// failure at all: the bytes that arrived were well formed and the body was
/// closed properly. It used to come back as `Ok`, carrying whatever text had
/// arrived, and from above that is indistinguishable from a model that
/// answered badly. A 300 attempt calibration run recorded 286 discards as "no
/// fenced json block" and zero as "agent error", and the whole log did not
/// contain the word timeout once. See `docs/DECISIONS.md` (2026-08-23).
///
/// **A provider that will not take the request is asked again, and one that
/// has started answering never is.** The retrying lives in `zorp::send_json`
/// and stops the moment a response exists, which is the only place it can
/// safely happen on this path. A 429 arrives before a single byte of body, so
/// sending again is clean: nothing reached `on_payload` and nothing was
/// generated upstream. A failure part way through a stream is the opposite.
/// Payloads have already gone to the caller, and in the browser that means
/// text already on somebody's screen, so a second send would replay the
/// beginning of a fresh answer over the middle of the abandoned one. The
/// truncation error above is therefore an error and stays one.
pub fn stream_sse(
    url: &str,
    headers: &[(&str, &str)],
    body: Value,
    cancel: Option<&CancelToken>,
    on_payload: &mut dyn FnMut(&str),
) -> Result<StreamOutcome, BoxErr> {
    // The shared agent, not `ureq::agent()`. ureq's default agent has no
    // timeouts of any kind, which is how a streamed call could wait forever
    // on a provider gone quiet while every buffered call was bounded.
    let mut req = zorp::http_agent()
        .post(url)
        .set("Accept", "text/event-stream");
    for (k, v) in headers {
        req = req.set(k, v);
    }
    // The core's sender, not `req.send_json` directly. This used to be a copy
    // of the core's error handling with a comment saying it mirrored it, and
    // the copy is now the thing itself, so a failing stream reads the same as
    // a failing buffered call and a rate limited one is retried the same way.
    let resp = zorp::send_json(req, body)?;

    let streaming = resp.content_type().contains("event-stream");
    let mut reader = resp.into_reader();

    if !streaming {
        // Asked to stream, answered with a document. Hand it back whole.
        //
        // Read in pieces rather than `read_to_end`, for the one reason that
        // `read_to_end` cannot be interrupted. An endpoint that ignores
        // `stream` still takes as long to think as one that does not, so a
        // stop pressed against this shape of reply has to work too, or the
        // button means different things depending on which proxy is in the
        // way.
        let mut raw = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            if cancelled(cancel) {
                return Err(CANCELLED.into());
            }
            let since_last_byte = Instant::now();
            let read = reader
                .read(&mut chunk)
                .map_err(|e| read_error(e, since_last_byte.elapsed()))?;
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&raw) {
            return Ok(StreamOutcome::Buffered(value));
        }
        // Streamed anyway, under some other content type. Decode it rather
        // than failing on a technicality about a header.
        let mut decoder = SseDecoder::new();
        let mut events = 0usize;
        let mut finished = false;
        for payload in decoder.push(&raw) {
            events += 1;
            finished |= signals_completion(&payload);
            on_payload(&payload);
        }
        if let Some(payload) = decoder.finish() {
            events += 1;
            finished |= signals_completion(&payload);
            on_payload(&payload);
        }
        if events == 0 {
            return Err(format!(
                "{url}: answer was neither JSON nor an event stream ({} bytes)",
                raw.len()
            )
            .into());
        }
        return if finished {
            Ok(StreamOutcome::Streamed)
        } else {
            Err(truncated(events))
        };
    }

    let mut decoder = SseDecoder::new();
    let mut buf = [0u8; 4096];
    let mut events = 0usize;
    let mut finished = false;
    loop {
        // Between reads, which is the only place a synchronous reader offers.
        // One chunk of latency, against a response that otherwise has to
        // finish before anything can stop it.
        if cancelled(cancel) {
            return Err(CANCELLED.into());
        }
        // Restarted per read, so it measures silence rather than the length
        // of the answer. `read_error` needs the second of those and would be
        // wrong about every long healthy response if handed the first.
        let since_last_byte = Instant::now();
        let read = reader
            .read(&mut buf)
            .map_err(|e| read_error(e, since_last_byte.elapsed()))?;
        if read == 0 {
            break;
        }
        for payload in decoder.push(&buf[..read]) {
            events += 1;
            finished |= signals_completion(&payload);
            on_payload(&payload);
        }
    }
    if let Some(payload) = decoder.finish() {
        events += 1;
        finished |= signals_completion(&payload);
        on_payload(&payload);
    }
    // The events that did arrive were delivered on the way past, and that is
    // deliberate even when the answer turns out to be cut off: a caller that
    // renders deltas has already shown them, and pretending the response
    // never started would only make the transcript disagree with the screen.
    // What must not happen is this returning `Ok`.
    if finished {
        Ok(StreamOutcome::Streamed)
    } else {
        Err(truncated(events))
    }
}

/// Frames an SSE body.
///
/// Bytes rather than `&str` because chunk boundaries land wherever the
/// network puts them, including halfway through a multi-byte character. Only
/// whole lines are decoded, so a split character waits in the buffer instead
/// of becoming a replacement char in the middle of someone's answer.
#[derive(Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
    current: Vec<String>,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk, get back every `data:` payload it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(newline) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches('\n').trim_end_matches('\r');
            if line.is_empty() {
                if let Some(payload) = self.take_event() {
                    out.push(payload);
                }
            } else if let Some(rest) = line.strip_prefix("data:") {
                self.current
                    .push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            }
            // Anything else is a comment (`: keep-alive`) or a field this
            // decoder does not use (`event:`, `id:`, `retry:`). Ignored, not
            // an error: a keep-alive must not look like a malformed payload.
        }
        out
    }

    /// Emit a final event for a server that closed without a blank line.
    pub fn finish(&mut self) -> Option<String> {
        if !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches('\r');
            if let Some(rest) = line.strip_prefix("data:") {
                self.current
                    .push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            }
        }
        self.take_event()
    }

    fn take_event(&mut self) -> Option<String> {
        if self.current.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut self.current).join("\n"))
    }
}

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

/// Splits a character stream into what may be shown and what is reasoning.
///
/// The buffered path runs `extract_think_tags` before anyone sees the
/// content, so a qwen-family model's chain of thought never reaches the
/// browser today. Streaming raw content deltas would put it on screen,
/// formatted as the answer. This keeps the two paths honest with each other.
///
/// The tags themselves arrive split across chunks as readily as anything
/// else, so text that could still turn out to be the start of a tag is
/// withheld until it is known one way or the other.
#[derive(Default)]
pub struct ThinkGate {
    pending: String,
    inside: bool,
}

impl ThinkGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `(visible, reasoning)` for this chunk. Either may be empty.
    pub fn push(&mut self, text: &str) -> (String, String) {
        self.pending.push_str(text);
        let mut visible = String::new();
        let mut reasoning = String::new();
        loop {
            let looking_for = if self.inside { CLOSE } else { OPEN };
            if let Some(at) = self.pending.find(looking_for) {
                let before: String = self.pending[..at].to_string();
                if self.inside {
                    reasoning.push_str(&before);
                } else {
                    visible.push_str(&before);
                }
                self.pending = self.pending[at + looking_for.len()..].to_string();
                self.inside = !self.inside;
                continue;
            }
            // No whole tag. Release everything except a tail that could still
            // become one.
            let hold = partial_tag_len(&self.pending, looking_for);
            let release: String = self.pending[..self.pending.len() - hold].to_string();
            if self.inside {
                reasoning.push_str(&release);
            } else {
                visible.push_str(&release);
            }
            self.pending = self.pending[self.pending.len() - hold..].to_string();
            break;
        }
        (visible, reasoning)
    }

    /// Release whatever is still held. A tag that never completed was not a
    /// tag, so it is ordinary text.
    pub fn flush(&mut self) -> (String, String) {
        let rest = std::mem::take(&mut self.pending);
        if self.inside {
            (String::new(), rest)
        } else {
            (rest, String::new())
        }
    }
}

/// Length of the longest suffix of `text` that is a proper prefix of `tag`.
fn partial_tag_len(text: &str, tag: &str) -> usize {
    // Inclusive: a chunk can end with the whole of what it has seen of the
    // tag so far ("<thi"), and that is the case worth holding. A suffix as
    // long as the tag itself cannot reach here, because `find` matched first.
    let max = tag.len().min(text.len());
    // Longest first: holding more is the safe direction.
    (1..=max)
        .rev()
        .find(|n| text.is_char_boundary(text.len() - n) && tag.starts_with(&text[text.len() - n..]))
        .unwrap_or(0)
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// Rebuilds a finished message from streamed chunks.
///
/// `finish` deliberately reassembles a response shaped like a buffered one
/// and hands it to `parse_assistant_completion` rather than parsing again
/// here. Streaming is a transport detail; it must not become a second place
/// where tool arguments, think tags or finish reasons are interpreted, or the
/// two paths will disagree eventually and only one of them will have tests.
#[derive(Default)]
pub struct DeltaAccumulator {
    content: String,
    reasoning: String,
    tool_calls: BTreeMap<usize, PartialToolCall>,
    finish_reason: String,
    usage: Option<Value>,
    gate: ThinkGate,
}

impl DeltaAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply one `data:` payload, returning the text a user may see.
    ///
    /// `[DONE]` and anything unparseable yield nothing rather than failing.
    /// A provider that appends a sentinel or a stray keep-alive should not
    /// take down a turn that has already produced an answer.
    pub fn apply(&mut self, payload: &str) -> Option<String> {
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            return None;
        }
        let value: Value = serde_json::from_str(payload).ok()?;
        if let Some(usage) = value.get("usage").filter(|u| !u.is_null()) {
            self.usage = Some(usage.clone());
        }
        let choice = value.get("choices")?.as_array()?.first()?;
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = reason.to_string();
        }
        let delta = choice.get("delta")?;

        // Providers that expose reasoning as its own field never wrap it in
        // tags, so it bypasses the gate.
        for field in ["reasoning_content", "reasoning", "thinking"] {
            if let Some(text) = delta.get(field).and_then(Value::as_str) {
                self.reasoning.push_str(text);
            }
        }

        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let entry = self.tool_calls.entry(index).or_default();
                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    if !id.is_empty() {
                        entry.id = id.to_string();
                    }
                }
                if let Some(func) = call.get("function") {
                    if let Some(name) = func.get("name").and_then(Value::as_str) {
                        if !name.is_empty() {
                            entry.name = name.to_string();
                        }
                    }
                    if let Some(args) = func.get("arguments").and_then(Value::as_str) {
                        entry.arguments.push_str(args);
                    }
                }
            }
        }

        let text = delta.get("content").and_then(Value::as_str)?;
        if text.is_empty() {
            return None;
        }
        let (visible, reasoning) = self.gate.push(text);
        self.reasoning.push_str(&reasoning);
        self.content.push_str(&visible);
        (!visible.is_empty()).then_some(visible)
    }

    /// The message the stream described, parsed the same way a buffered
    /// response would be.
    pub fn finish(mut self) -> Result<ModelCompletion, BoxErr> {
        let (visible, reasoning) = self.gate.flush();
        self.content.push_str(&visible);
        self.reasoning.push_str(&reasoning);

        let mut message = json!({ "content": self.content });
        if !self.reasoning.trim().is_empty() {
            message["reasoning_content"] = json!(self.reasoning);
        }
        if !self.tool_calls.is_empty() {
            let calls: Vec<Value> = self
                .tool_calls
                .into_values()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "type": "function",
                        "function": { "name": c.name, "arguments": c.arguments },
                    })
                })
                .collect();
            message["tool_calls"] = Value::Array(calls);
        }
        let mut response = json!({
            "choices": [{ "message": message, "finish_reason": self.finish_reason }]
        });
        if let Some(usage) = self.usage {
            response["usage"] = usage;
        }
        parse_assistant_completion(&response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn decode(chunks: &[&str]) -> Vec<String> {
        let mut d = SseDecoder::new();
        let mut out = Vec::new();
        for c in chunks {
            out.extend(d.push(c.as_bytes()));
        }
        out.extend(d.finish());
        out
    }

    #[test]
    fn a_whole_event_decodes_to_its_payload() {
        assert_eq!(decode(&["data: {\"a\":1}\n\n"]), vec!["{\"a\":1}"]);
    }

    /// The reason this is a decoder and not a line split.
    #[test]
    fn an_event_split_across_chunks_still_decodes_once() {
        assert_eq!(decode(&["data: {\"a\":", "1}\n", "\n"]), vec!["{\"a\":1}"]);
    }

    #[test]
    fn a_multi_byte_character_split_across_chunks_is_not_corrupted() {
        let text = "data: {\"c\":\"é\"}\n\n";
        let bytes = text.as_bytes();
        // Split inside the two-byte 'é'.
        let at = text.find('é').unwrap() + 1;
        let mut d = SseDecoder::new();
        let mut out = d.push(&bytes[..at]);
        out.extend(d.push(&bytes[at..]));
        assert_eq!(out, vec!["{\"c\":\"é\"}"]);
    }

    #[test]
    fn keep_alive_comments_and_other_fields_are_not_payloads() {
        assert_eq!(
            decode(&[": keep-alive\n\nevent: ping\nid: 7\n\ndata: real\n\n"]),
            vec!["real"]
        );
    }

    #[test]
    fn carriage_returns_are_tolerated() {
        assert_eq!(decode(&["data: x\r\n\r\n"]), vec!["x"]);
    }

    #[test]
    fn a_server_that_closes_without_a_blank_line_still_yields_its_last_event() {
        assert_eq!(decode(&["data: last"]), vec!["last"]);
    }

    fn gate(chunks: &[&str]) -> (String, String) {
        let mut g = ThinkGate::new();
        let (mut vis, mut rea) = (String::new(), String::new());
        for c in chunks {
            let (v, r) = g.push(c);
            vis.push_str(&v);
            rea.push_str(&r);
        }
        let (v, r) = g.flush();
        vis.push_str(&v);
        rea.push_str(&r);
        (vis, rea)
    }

    #[test]
    fn ordinary_text_is_visible() {
        assert_eq!(
            gate(&["hello ", "world"]),
            ("hello world".into(), "".into())
        );
    }

    /// The whole reason the gate exists.
    #[test]
    fn a_think_block_never_becomes_visible_text() {
        let (visible, reasoning) = gate(&["<think>plotting</think>answer"]);
        assert_eq!(visible, "answer");
        assert_eq!(reasoning, "plotting");
    }

    /// The case a naive implementation gets wrong: the tag itself is split by
    /// the network, so a substring check per chunk never sees it.
    #[test]
    fn a_think_tag_split_across_chunks_is_still_caught() {
        let (visible, reasoning) = gate(&["<thi", "nk>secret</thi", "nk>said"]);
        assert_eq!(
            visible, "said",
            "the model's reasoning leaked into the answer"
        );
        assert_eq!(reasoning, "secret");
    }

    #[test]
    fn text_that_merely_starts_like_a_tag_is_still_shown() {
        assert_eq!(
            gate(&["<thing> and <b>"]),
            ("<thing> and <b>".into(), "".into())
        );
    }

    #[test]
    fn an_unclosed_think_tag_keeps_its_text_out_of_the_answer() {
        let (visible, reasoning) = gate(&["before <think>never closed"]);
        assert_eq!(visible, "before ");
        assert_eq!(reasoning, "never closed");
    }

    /// Withholding a possible tag must not lose the characters.
    #[test]
    fn a_dangling_partial_tag_is_released_on_flush() {
        assert_eq!(gate(&["done<thi"]), ("done<thi".into(), "".into()));
    }

    fn chunk(delta: Value) -> String {
        json!({ "choices": [{ "delta": delta }] }).to_string()
    }

    #[test]
    fn content_deltas_accumulate_and_are_returned_as_they_arrive() {
        let mut acc = DeltaAccumulator::new();
        assert_eq!(
            acc.apply(&chunk(json!({"content": "he"}))).as_deref(),
            Some("he")
        );
        assert_eq!(
            acc.apply(&chunk(json!({"content": "llo"}))).as_deref(),
            Some("llo")
        );
        let done = acc.finish().unwrap();
        assert_eq!(done.message.content, "hello");
    }

    #[test]
    fn the_done_sentinel_and_junk_are_ignored_rather_than_fatal() {
        let mut acc = DeltaAccumulator::new();
        assert_eq!(acc.apply("[DONE]"), None);
        assert_eq!(acc.apply("not json"), None);
        assert_eq!(acc.apply(""), None);
        assert_eq!(acc.finish().unwrap().message.content, "");
    }

    /// Arguments arrive as string fragments keyed by index. Joining them
    /// wrongly does not look like a streaming bug, it looks like the agent
    /// calling a tool with truncated arguments.
    #[test]
    fn tool_call_fragments_are_joined_by_index() {
        let mut acc = DeltaAccumulator::new();
        acc.apply(&chunk(json!({"tool_calls": [
            {"index": 0, "id": "call_a", "function": {"name": "read_file", "arguments": "{\"pa"}}
        ]})));
        acc.apply(&chunk(json!({"tool_calls": [
            {"index": 0, "function": {"arguments": "th\":\"a.txt\"}"}}
        ]})));
        let done = acc.finish().unwrap();
        assert_eq!(done.message.tool_calls.len(), 1);
        let call = &done.message.tool_calls[0];
        assert_eq!(call.id, "call_a");
        assert_eq!(call.name, "read_file");
        assert_eq!(call.arguments, json!({"path": "a.txt"}));
    }

    #[test]
    fn two_tool_calls_stay_separate_and_keep_their_order() {
        let mut acc = DeltaAccumulator::new();
        acc.apply(&chunk(json!({"tool_calls": [
            {"index": 1, "id": "b", "function": {"name": "second", "arguments": "{}"}},
            {"index": 0, "id": "a", "function": {"name": "first", "arguments": "{}"}}
        ]})));
        let calls = acc.finish().unwrap().message.tool_calls;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "first");
        assert_eq!(calls[1].name, "second");
    }

    #[test]
    fn a_reasoning_field_is_captured_without_becoming_visible() {
        let mut acc = DeltaAccumulator::new();
        assert_eq!(acc.apply(&chunk(json!({"reasoning_content": "hmm"}))), None);
        acc.apply(&chunk(json!({"content": "answer"})));
        let done = acc.finish().unwrap();
        assert_eq!(done.message.content, "answer");
        assert_eq!(done.message.reasoning_content.as_deref(), Some("hmm"));
    }

    #[test]
    fn the_finish_reason_survives_the_stream() {
        let mut acc = DeltaAccumulator::new();
        acc.apply(&chunk(json!({"content": "x"})));
        acc.apply(&json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}).to_string());
        assert_eq!(acc.finish().unwrap().message.finish_reason, "tool_calls");
    }

    /// The property worth guarding hardest: a streamed turn and a buffered
    /// turn of the same answer must produce the same message, think tags and
    /// all.
    #[test]
    fn a_streamed_turn_equals_the_buffered_turn_it_describes() {
        let mut acc = DeltaAccumulator::new();
        for piece in ["<think>", "quietly", "</think>", "The ", "answer."] {
            acc.apply(&chunk(json!({ "content": piece })));
        }
        acc.apply(&json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}).to_string());
        let streamed = acc.finish().unwrap();

        let buffered = parse_assistant_completion(&json!({
            "choices": [{
                "message": {"content": "<think>quietly</think>The answer."},
                "finish_reason": "stop"
            }]
        }))
        .unwrap();

        assert_eq!(streamed.message, buffered.message);
    }

    /* ---- cancelling a response that is still arriving ---- */

    /// A server that answers slowly and for a long time, which is what a local
    /// model writing a long answer is. `content_type` picks which branch of
    /// `stream_sse` gets exercised: an event stream, or a document from an
    /// endpoint that ignored `stream`.
    ///
    /// Every piece is flushed on its own so the reader really does wake up
    /// many times over the life of the response. A stub that wrote the whole
    /// body at once would let a cancel that never fires still look prompt.
    fn drip_server(content_type: &str, pieces: Vec<String>, gap: Duration) -> std::net::SocketAddr {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let content_type = content_type.to_string();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // The whole request, headers and body, before answering. Not
            // because the body is interesting here, but because closing a
            // socket that still has unread bytes on it sends RST rather than
            // FIN, and the client then reports a connection reset instead of
            // the clean end of a response.
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            let header_end = loop {
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0, "request ended before headers");
                request.extend_from_slice(&buffer[..read]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let content_length = String::from_utf8_lossy(&request[..header_end])
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length: "))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0, "request ended before body");
                request.extend_from_slice(&buffer[..read]);
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.flush();
            for piece in pieces {
                // A client that hung up is the expected end of this stub, not
                // a failure: that is exactly what cancelling looks like from
                // the server's side.
                if write!(stream, "{piece}").is_err() || stream.flush().is_err() {
                    return;
                }
                std::thread::sleep(gap);
            }
        });
        address
    }

    /// The bug this exists for.
    ///
    /// The agent checks its cancel token between steps and around tool calls,
    /// so a stop pressed while a model was mid-response did nothing until the
    /// response finished. Against a local model writing a long answer that is
    /// minutes, during which the button says stop and the run carries on.
    ///
    /// The cancel is raised from inside the callback so the test does not race
    /// a timer: the third payload trips it, and everything after that is the
    /// read loop's business.
    #[test]
    fn a_cancel_raised_mid_stream_stops_reading_instead_of_finishing_the_response() {
        let pieces: Vec<String> = (0..300)
            .map(|i| format!("data: {}\n\n", json!({"choices":[{"delta":{"content":i}}]})))
            .collect();
        let address = drip_server("text/event-stream", pieces, Duration::from_millis(20));

        let cancel = Arc::new(AtomicBool::new(false));
        let mut seen = 0usize;
        let started = std::time::Instant::now();
        let outcome = stream_sse(
            &format!("http://{address}/v1/chat/completions"),
            &[],
            json!({"stream": true}),
            Some(&cancel),
            &mut |_payload| {
                seen += 1;
                if seen == 3 {
                    cancel.store(true, Ordering::SeqCst);
                }
            },
        );
        let elapsed = started.elapsed();

        assert!(
            outcome.is_err(),
            "a cancelled response came back as a finished one, so the agent \
             will treat a stopped turn as an answer"
        );
        // The server needs six seconds to say everything it has. Anything in
        // that region means the read loop sat there until the model was done.
        assert!(
            elapsed < Duration::from_secs(3),
            "the stream took {elapsed:?} to notice the cancel"
        );
        assert!(
            seen < 100,
            "the read loop kept consuming the response after the cancel: {seen} payloads"
        );
    }

    /// The same promise for the endpoint that ignores `stream` and answers
    /// with one document. It is a different branch with its own read, and a
    /// stop that works on one shape of reply and not the other is a stop
    /// nobody can rely on.
    #[test]
    fn a_cancel_also_abandons_a_document_from_an_endpoint_that_ignored_stream() {
        // Long enough that reading it takes many reads, slow enough that the
        // cancel lands in the middle of it.
        let pieces: Vec<String> = (0..300).map(|i| format!("{{\"filler{i}\":1}},")).collect();
        let address = drip_server("application/json", pieces, Duration::from_millis(20));

        let cancel = Arc::new(AtomicBool::new(false));
        cancel.store(true, Ordering::SeqCst);
        let started = std::time::Instant::now();
        let outcome = stream_sse(
            &format!("http://{address}/v1/chat/completions"),
            &[],
            json!({"stream": true}),
            Some(&cancel),
            &mut |_payload| {},
        );

        assert!(outcome.is_err(), "a cancelled document read to the end");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the document branch ignored the cancel"
        );
    }

    /// Nothing to cancel with is the ordinary case for every caller that is
    /// not an agent, and it must behave exactly as it did before.
    #[test]
    fn without_a_token_a_stream_still_reads_to_the_end() {
        let mut pieces: Vec<String> = ["he", "llo"]
            .iter()
            .map(|t| format!("data: {}\n\n", json!({"choices":[{"delta":{"content":t}}]})))
            .collect();
        // The provider says it has finished, because that is what a provider
        // does and because a stream without it is now an error in its own
        // right. See `a_stream_that_stops_without_saying_so_is_not_an_answer`.
        pieces.push("data: [DONE]\n\n".to_string());
        let address = drip_server("text/event-stream", pieces, Duration::from_millis(1));

        let mut seen = 0usize;
        let outcome = stream_sse(
            &format!("http://{address}/v1/chat/completions"),
            &[],
            json!({"stream": true}),
            None,
            &mut |_payload| seen += 1,
        );
        assert!(outcome.is_ok(), "{:?}", outcome.err());
        assert_eq!(seen, 3);
    }

    /* ---- a response that stopped, told apart from one that finished ---- */

    /// The bug that cost nine hours: this used to be `Ok`.
    ///
    /// A response that ends cleanly halfway through an answer is not a
    /// transport failure, so nothing below noticed, and the half answer went
    /// up the stack looking exactly like a model that replied badly.
    #[test]
    fn a_stream_that_stops_without_saying_so_is_not_an_answer() {
        let pieces = vec![format!(
            "data: {}\n\n",
            json!({"choices":[{"delta":{"content":"half an ans"}}]})
        )];
        let address = drip_server("text/event-stream", pieces, Duration::from_millis(1));

        let mut seen = 0usize;
        let outcome = stream_sse(
            &format!("http://{address}/v1/chat/completions"),
            &[],
            json!({"stream": true}),
            None,
            &mut |_payload| seen += 1,
        );
        let error = outcome
            .err()
            .expect("a truncated answer came back as a finished one")
            .to_string();
        assert!(error.starts_with(TRUNCATED), "{error}");
        assert_eq!(seen, 1, "the event that did arrive was not delivered");
    }

    /// A `finish_reason` is the other way a provider says it is done, and a
    /// provider that sends one without a `[DONE]` after it is ordinary.
    #[test]
    fn a_finish_reason_is_enough_to_call_a_stream_finished() {
        let pieces = vec![format!(
            "data: {}\n\n",
            json!({"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]})
        )];
        let address = drip_server("text/event-stream", pieces, Duration::from_millis(1));

        let outcome = stream_sse(
            &format!("http://{address}/v1/chat/completions"),
            &[],
            json!({"stream": true}),
            None,
            &mut |_payload| {},
        );
        assert!(outcome.is_ok(), "{:?}", outcome.err());
    }

    /// `finish_reason: null` arrives on every intermediate chunk from some
    /// providers. Reading it as an ending would undo the whole check.
    #[test]
    fn a_null_finish_reason_does_not_end_a_stream() {
        assert!(!signals_completion(
            &json!({"choices":[{"delta":{"content":"x"},"finish_reason":null}]}).to_string()
        ));
        assert!(!signals_completion(
            &json!({"choices":[{"delta":{"content":"x"},"finish_reason":""}]}).to_string()
        ));
        assert!(signals_completion(
            &json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}).to_string()
        ));
        assert!(signals_completion("[DONE]"));
        assert!(signals_completion(" [DONE] "));
        assert!(!signals_completion(
            &json!({"choices":[{"delta":{"content":"x"}}]}).to_string()
        ));
    }
}
