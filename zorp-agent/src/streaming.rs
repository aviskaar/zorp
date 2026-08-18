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
use crate::BoxErr;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;

/// How much of an error body to quote back. Matches the core's cap.
const ERROR_BODY_CAP: u64 = 8 * 1024;

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
pub fn stream_sse(
    url: &str,
    headers: &[(&str, &str)],
    body: Value,
    on_payload: &mut dyn FnMut(&str),
) -> Result<StreamOutcome, BoxErr> {
    let mut req = ureq::agent().post(url).set("Accept", "text/event-stream");
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let resp = match req.send_json(body) {
        Ok(resp) => resp,
        // Mirrors the core's error shape, so a failing stream reads the same
        // as a failing buffered call rather than like a new kind of outage.
        Err(ureq::Error::Status(code, resp)) => {
            let url = resp.get_url().to_string();
            let mut bytes = Vec::new();
            let _ = resp
                .into_reader()
                .take(ERROR_BODY_CAP)
                .read_to_end(&mut bytes);
            let text = String::from_utf8_lossy(&bytes);
            let text = text.trim();
            return Err(if text.is_empty() {
                format!("{url}: status code {code}").into()
            } else {
                format!("{url}: status code {code}: {text}").into()
            });
        }
        Err(e) => return Err(e.into()),
    };

    let streaming = resp.content_type().contains("event-stream");
    let mut reader = resp.into_reader();

    if !streaming {
        // Asked to stream, answered with a document. Hand it back whole.
        let mut raw = Vec::new();
        reader.read_to_end(&mut raw)?;
        if let Ok(value) = serde_json::from_slice::<Value>(&raw) {
            return Ok(StreamOutcome::Buffered(value));
        }
        // Streamed anyway, under some other content type. Decode it rather
        // than failing on a technicality about a header.
        let mut decoder = SseDecoder::new();
        let mut any = false;
        for payload in decoder.push(&raw) {
            any = true;
            on_payload(&payload);
        }
        if let Some(payload) = decoder.finish() {
            any = true;
            on_payload(&payload);
        }
        return if any {
            Ok(StreamOutcome::Streamed)
        } else {
            Err(format!(
                "{url}: answer was neither JSON nor an event stream ({} bytes)",
                raw.len()
            )
            .into())
        };
    }

    let mut decoder = SseDecoder::new();
    let mut buf = [0u8; 4096];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        for payload in decoder.push(&buf[..read]) {
            on_payload(&payload);
        }
    }
    if let Some(payload) = decoder.finish() {
        on_payload(&payload);
    }
    Ok(StreamOutcome::Streamed)
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
}
