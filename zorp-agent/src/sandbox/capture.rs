//! Bounded output capture with streaming secret redaction.
//!
//! Reads a child stream in chunks, redacts secret values (including
//! secrets that straddle read boundaries), and keeps a head and tail
//! window of the output within a byte cap.

use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub(super) struct BoundedCapture {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    total: usize,
    cap: usize,
}

impl BoundedCapture {
    fn new(cap: usize) -> Self {
        Self {
            head: Vec::with_capacity(cap / 2),
            tail: VecDeque::with_capacity(cap - cap / 2),
            total: 0,
            cap,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        let head_cap = self.cap / 2;
        let tail_cap = self.cap - head_cap;
        self.total = self.total.saturating_add(bytes.len());
        let mut bytes = bytes;
        if self.head.len() < head_cap {
            let take = (head_cap - self.head.len()).min(bytes.len());
            self.head.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
        }
        if tail_cap == 0 {
            return;
        }
        // Only the last tail_cap bytes can survive; drop the rest up front
        // so a large run costs one drain plus one extend.
        if bytes.len() >= tail_cap {
            self.tail.clear();
            self.tail.extend(&bytes[bytes.len() - tail_cap..]);
            return;
        }
        let overflow = (self.tail.len() + bytes.len()).saturating_sub(tail_cap);
        if overflow > 0 {
            self.tail.drain(..overflow);
        }
        self.tail.extend(bytes);
    }

    fn retained_len(&self) -> usize {
        self.head.len() + self.tail.len()
    }

    fn omitted(&self) -> usize {
        self.total.saturating_sub(self.retained_len())
    }

    #[cfg(test)]
    fn retained_bytes(&self) -> Vec<u8> {
        self.head
            .iter()
            .copied()
            .chain(self.tail.iter().copied())
            .collect()
    }
}

pub(super) fn capture_stream(
    mut reader: impl Read,
    cap: usize,
    secrets: Arc<Vec<Vec<u8>>>,
    eof: Arc<AtomicBool>,
    finalize_delay: Duration,
) -> io::Result<BoundedCapture> {
    let mut capture = BoundedCapture::new(cap);
    let mut pending = Vec::new();
    let overlap = secrets
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(1)
        .saturating_sub(1);
    // First-byte table: a byte can only start a secret when its entry is
    // set, so runs of non-matching bytes skip the per-secret scan.
    let mut first_bytes = [false; 256];
    for secret in secrets.iter() {
        if let Some(&byte) = secret.first() {
            first_bytes[byte as usize] = true;
        }
    }
    let mut buffer = [0u8; 4096];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            eof.store(true, Ordering::SeqCst);
            if !finalize_delay.is_zero() {
                thread::sleep(finalize_delay);
            }
            let process_len = pending.len();
            redact_pending(&mut pending, process_len, &secrets, &first_bytes, &mut capture);
            break;
        }
        pending.extend_from_slice(&buffer[..count]);
        let process_len = pending.len().saturating_sub(overlap);
        redact_pending(&mut pending, process_len, &secrets, &first_bytes, &mut capture);
    }
    Ok(capture)
}

fn redact_pending(
    pending: &mut Vec<u8>,
    process_len: usize,
    secrets: &[Vec<u8>],
    first_bytes: &[bool; 256],
    capture: &mut BoundedCapture,
) {
    let mut position = 0;
    let mut run_start = 0;
    while position < process_len {
        if first_bytes[pending[position] as usize] {
            if let Some(secret) = secrets
                .iter()
                .find(|secret| !secret.is_empty() && pending[position..].starts_with(secret))
            {
                if run_start < position {
                    capture.push(&pending[run_start..position]);
                }
                capture.push(b"[REDACTED]");
                position += secret.len();
                run_start = position;
                continue;
            }
        }
        position += 1;
    }
    if run_start < position {
        capture.push(&pending[run_start..position]);
    }
    pending.drain(..position);
}

pub(super) fn render_capture(capture: &BoundedCapture, cap: usize) -> String {
    if capture.omitted() == 0 {
        let bytes: Vec<u8> = capture
            .head
            .iter()
            .copied()
            .chain(capture.tail.iter().copied())
            .collect();
        return cap_output_head_tail(&String::from_utf8_lossy(&bytes), cap);
    }
    let head = decode_truncated_head(&capture.head);
    let tail_bytes: Vec<u8> = capture.tail.iter().copied().collect();
    let tail = decode_truncated_tail(&tail_bytes);
    cap_separate_head_tail(&head, &tail, capture.omitted(), cap)
}

fn cap_output_head_tail(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_string();
    }
    let half = cap / 2;
    let head_end = nearest_char_boundary(text, half);
    let tail_start = next_char_boundary(text, text.len() - (cap - half));
    cap_separate_head_tail(
        &text[..head_end],
        &text[tail_start..],
        tail_start - head_end,
        cap,
    )
}

fn cap_separate_head_tail(head: &str, tail: &str, omitted: usize, cap: usize) -> String {
    if cap < "truncated".len() {
        return head[..nearest_char_boundary(head, cap)].to_string();
    }
    let marker = format!("\n[… {omitted} bytes truncated …]\n");
    let marker = if marker.len() <= cap {
        marker
    } else {
        "truncated".into()
    };
    let payload_cap = cap.saturating_sub(marker.len());
    let head_end = nearest_char_boundary(head, (payload_cap / 2).min(head.len()));
    let tail_bytes = (payload_cap - payload_cap / 2).min(tail.len());
    let tail_start = next_char_boundary(tail, tail.len() - tail_bytes);
    format!("{}{}{}", &head[..head_end], marker, &tail[tail_start..])
}

fn decode_truncated_head(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(error) if error.error_len().is_none() => {
            String::from_utf8_lossy(&bytes[..error.valid_up_to()]).into_owned()
        }
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn decode_truncated_tail(bytes: &[u8]) -> String {
    for start in 0..bytes.len().min(4) {
        if let Ok(text) = std::str::from_utf8(&bytes[start..]) {
            return text.to_string();
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn nearest_char_boundary(text: &str, at: usize) -> usize {
    (0..=at.min(text.len()))
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0)
}

fn next_char_boundary(text: &str, at: usize) -> usize {
    (at.min(text.len())..=text.len())
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn streaming_redaction_retains_bounded_output_and_cross_chunk_secrets() {
        let mut input = vec![b'x'; 4095];
        input.extend_from_slice(b"cross-boundary-secret");
        input.extend(std::iter::repeat_n(b'y', 4096));
        let expected_redacted_len =
            input.len() - b"cross-boundary-secret".len() + b"[REDACTED]".len();
        let secrets = Arc::new(vec![b"cross-boundary-secret".to_vec()]);
        let eof = Arc::new(AtomicBool::new(false));

        let capture =
            capture_stream(Cursor::new(input), 64, secrets, eof.clone(), Duration::ZERO).unwrap();

        assert!(eof.load(Ordering::SeqCst));
        assert!(capture.retained_len() <= 64);
        assert!(capture.omitted() > 0);
        assert_eq!(capture.total, expected_redacted_len);
        assert!(!capture
            .retained_bytes()
            .windows(21)
            .any(|w| w == b"cross-boundary-secret"));
    }

    #[test]
    fn large_output_without_secret_bytes_passes_through_unchanged() {
        // 1 MiB of a byte that never starts any secret; the whole stream
        // must be copied through untouched with nothing redacted.
        let input = vec![b'x'; 1 << 20];
        let secrets = Arc::new(vec![b"secret-value".to_vec(), b"token-value".to_vec()]);
        let eof = Arc::new(AtomicBool::new(false));

        let capture = capture_stream(
            Cursor::new(input.clone()),
            input.len() * 2,
            secrets,
            eof,
            Duration::ZERO,
        )
        .unwrap();

        assert_eq!(capture.total, input.len());
        assert_eq!(capture.omitted(), 0);
        assert_eq!(capture.retained_bytes(), input);
    }

    #[test]
    fn secret_inside_large_output_is_still_redacted() {
        let mut input = vec![b'x'; 100_000];
        input.extend_from_slice(b"secret-value");
        input.extend(vec![b'y'; 100_000]);
        let secrets = Arc::new(vec![b"secret-value".to_vec()]);
        let eof = Arc::new(AtomicBool::new(false));

        let capture = capture_stream(
            Cursor::new(input.clone()),
            input.len() * 2,
            secrets,
            eof,
            Duration::ZERO,
        )
        .unwrap();

        let retained = capture.retained_bytes();
        assert!(!retained
            .windows(b"secret-value".len())
            .any(|w| w == b"secret-value"));
        let redacted: Vec<u8> = retained
            .windows(b"[REDACTED]".len())
            .filter(|w| *w == b"[REDACTED]")
            .take(1)
            .flatten()
            .copied()
            .collect();
        assert_eq!(redacted, b"[REDACTED]");
        assert_eq!(
            capture.total,
            input.len() - b"secret-value".len() + b"[REDACTED]".len()
        );
    }
}
