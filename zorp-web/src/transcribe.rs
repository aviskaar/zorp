//! Sending recorded speech to a transcription endpoint the user chose.
//!
//! The wire format is OpenAI's `/audio/transcriptions`: a multipart upload
//! with the audio under `file`. whisper.cpp's bundled server, speaches,
//! LocalAI and OpenAI itself all read that shape, which is the reason this
//! is a URL setting rather than a bundled binary. zorp does not install a
//! speech model, spawn a process, or ship weights; it posts to an address.
//!
//! Two things it deliberately does not do. It never sends an API key, so
//! the credential configured for the chat provider cannot end up on a
//! different machine along with a recording of someone's voice. And it
//! never writes the audio anywhere: the bytes live in the request that
//! carried them and are dropped when it ends.

use std::time::Duration;

/// Largest recording this will forward, and the limit the route's body
/// extractor is built with. About four minutes of the 16 kHz mono the
/// browser sends, which is twice the cap the browser applies to itself.
pub const MAX_AUDIO_BYTES: usize = 8 * 1024 * 1024;

/// Short, like the models probe: a transcription server that will not
/// accept a connection in a few seconds is not running.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Long, unlike the models probe. Whisper on a CPU spends real seconds per
/// second of audio, and the person who just spoke is waiting on exactly
/// this call. Cutting it off at a browser-ish timeout would turn a slow
/// machine into a broken feature.
const READ_TIMEOUT: Duration = Duration::from_secs(180);

/// Whether a body is plausibly the WAV the browser said it was sending.
///
/// Cheap, and it catches the case that matters: something other than the
/// zorp UI posting a JSON body or a text file to this endpoint and having
/// it forwarded to the transcription server as audio.
pub fn looks_like_wav(bytes: &[u8]) -> bool {
    bytes.len() > 44 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE"
}

/// Post audio to `{base_url}/audio/transcriptions` and return the text.
///
/// `Err` carries a sentence fit to show a person, because every failure
/// here is one they can act on: the server is not running, no model is
/// loaded, or the address points at something that is not a transcription
/// server at all.
pub fn transcribe(base_url: &str, model: &str, wav: &[u8]) -> Result<String, String> {
    let url = zorp_agent::join_url(base_url, "audio/transcriptions");
    let boundary = boundary_absent_from(wav);
    let body = multipart(&boundary, model, wav);

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .build();

    match agent
        .post(&url)
        .set(
            "content-type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_bytes(&body)
    {
        Ok(response) => {
            let text = response
                .into_string()
                .map_err(|e| format!("{url} answered with a body that could not be read: {e}"))?;
            // A 2xx that is not a transcript means this address is not a
            // transcription server. Its body is not echoed: it is a web
            // page or an unrelated API, and quoting it back reads like it
            // came from the recording.
            let parsed: serde_json::Value = serde_json::from_str(&text)
                .map_err(|_| format!("{url} answered, but not with a transcript. Check that it is an OpenAI-compatible transcription endpoint."))?;
            parsed
                .get("text")
                .and_then(|t| t.as_str())
                .map(str::to_string)
                .ok_or_else(|| format!("{url} answered with JSON that has no \"text\" field."))
        }
        // A failure status is the one case where the endpoint's own words
        // are the useful part, so they are passed through, trimmed.
        Err(ureq::Error::Status(code, response)) => Err(format!(
            "{url}: status code {code}: {}",
            snippet(&response.into_string().unwrap_or_default())
        )),
        // ureq's transport errors already start with the URL.
        Err(e) => Err(e.to_string()),
    }
}

/// One multipart body: the audio, the model name, and a request for JSON.
fn multipart(boundary: &str, model: &str, wav: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(wav.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\n\
             Content-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav);
    body.extend_from_slice(
        format!(
            "\r\n--{boundary}\r\n\
             Content-Disposition: form-data; name=\"model\"\r\n\r\n\
             {model}\r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"response_format\"\r\n\r\n\
             json\r\n\
             --{boundary}--\r\n"
        )
        .as_bytes(),
    );
    body
}

/// A boundary that does not occur in the payload.
///
/// Audio is arbitrary bytes, so a boundary that happened to appear inside
/// it would split the upload in the wrong place and the endpoint would
/// transcribe half a sentence. Checking is cheaper than reasoning about how
/// unlikely that is.
fn boundary_absent_from(wav: &[u8]) -> String {
    let pid = std::process::id();
    let mut candidate = String::new();
    for salt in 0..64u32 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        candidate = format!("----zorpaudio{pid:08x}{nanos:08x}{salt:02x}");
        if !contains(wav, candidate.as_bytes()) {
            break;
        }
    }
    candidate
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// One line of an upstream error, short enough to read in a chat bubble.
fn snippet(body: &str) -> String {
    let flattened: String = body
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = flattened.trim();
    if trimmed.chars().count() <= 200 {
        return trimmed.to_string();
    }
    trimmed.chars().take(200).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_is_recognised_and_other_bodies_are_not() {
        let mut wav = b"RIFF____WAVEfmt ".to_vec();
        wav.extend(std::iter::repeat(0u8).take(64));
        assert!(looks_like_wav(&wav));
        assert!(!looks_like_wav(b"{\"message\":\"not audio\"}"));
        assert!(!looks_like_wav(b""));
        // Header only, no samples: nothing was recorded.
        assert!(!looks_like_wav(&wav[..44.min(wav.len())]));
    }

    #[test]
    fn the_multipart_body_names_the_parts_the_endpoint_looks_for() {
        let body = multipart("BOUND", "whisper-1", b"RIFFdata");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--BOUND\r\n"));
        assert!(text.contains("name=\"file\"; filename=\"speech.wav\""));
        assert!(text.contains("name=\"model\""));
        assert!(text.contains("whisper-1"));
        assert!(text.contains("RIFFdata"));
        assert!(text.ends_with("--BOUND--\r\n"));
    }

    /// The audio is binary, and a boundary hidden inside it would cut the
    /// upload short at a point the endpoint would happily accept.
    #[test]
    fn the_boundary_never_appears_inside_the_audio() {
        let first = boundary_absent_from(b"");
        let mut hostile = b"RIFF".to_vec();
        hostile.extend_from_slice(first.as_bytes());
        let second = boundary_absent_from(&hostile);
        assert!(
            !contains(&hostile, second.as_bytes()),
            "picked a boundary that is already in the audio"
        );
    }

    #[test]
    fn an_upstream_error_is_shortened_rather_than_pasted_whole() {
        let long = "x".repeat(1000);
        assert!(snippet(&long).chars().count() <= 201);
        assert_eq!(snippet("  no model loaded\n"), "no model loaded");
    }
}
