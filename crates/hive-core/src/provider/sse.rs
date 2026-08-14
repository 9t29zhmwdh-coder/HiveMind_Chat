//! Line and server-sent-event framing for streamed provider responses.
//!
//! Anthropic and the OpenAI dialect stream SSE, Ollama streams newline-delimited
//! JSON. Both are line-oriented, so one framing layer serves all three.

use std::collections::VecDeque;

use futures::{Stream, StreamExt};

use crate::error::{HiveError, Result};

struct LineState<S> {
    inner: S,
    /// Bytes received but not yet terminated by a newline.
    buffer: Vec<u8>,
    ready: VecDeque<String>,
    finished: bool,
}

/// Splits a byte stream into lines.
///
/// Buffering happens on raw bytes rather than on decoded text: a chunk boundary
/// can fall inside a multi-byte character, and decoding per chunk would corrupt
/// it. A newline never appears inside a UTF-8 sequence, so splitting first and
/// decoding per line is safe.
pub(crate) fn line_stream<S, B, E>(inner: S) -> impl Stream<Item = Result<String>> + Send
where
    S: Stream<Item = std::result::Result<B, E>> + Send + 'static,
    B: AsRef<[u8]> + Send,
    E: Into<HiveError> + Send,
{
    let state = LineState {
        inner: Box::pin(inner),
        buffer: Vec::new(),
        ready: VecDeque::new(),
        finished: false,
    };

    futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(line) = state.ready.pop_front() {
                return Some((Ok(line), state));
            }
            if state.finished {
                return None;
            }
            match state.inner.next().await {
                Some(Ok(chunk)) => {
                    state.buffer.extend_from_slice(chunk.as_ref());
                    drain_lines(&mut state.buffer, &mut state.ready);
                }
                Some(Err(err)) => {
                    state.finished = true;
                    return Some((Err(err.into()), state));
                }
                None => {
                    state.finished = true;
                    flush_remainder(&mut state.buffer, &mut state.ready);
                }
            }
        }
    })
}

fn drain_lines(buffer: &mut Vec<u8>, ready: &mut VecDeque<String>) {
    while let Some(index) = buffer.iter().position(|b| *b == b'\n') {
        let line: Vec<u8> = buffer.drain(..=index).collect();
        ready.push_back(decode_line(&line[..line.len() - 1]));
    }
}

fn flush_remainder(buffer: &mut Vec<u8>, ready: &mut VecDeque<String>) {
    if buffer.is_empty() {
        return;
    }
    ready.push_back(decode_line(buffer));
    buffer.clear();
}

fn decode_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\r')
        .to_string()
}

/// Extracts the payload of an SSE `data:` line.
///
/// Returns `None` for keep-alive comments, event-name lines, blank separators and
/// the `[DONE]` sentinel, so callers only ever see parseable payloads.
pub(crate) fn sse_payload(line: &str) -> Option<&str> {
    let payload = line.strip_prefix("data:")?.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }
    Some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    fn chunks(
        parts: &[&[u8]],
    ) -> impl Stream<Item = std::result::Result<Vec<u8>, HiveError>> + Send + 'static {
        let owned: Vec<Vec<u8>> = parts.iter().map(|p| p.to_vec()).collect();
        stream::iter(owned.into_iter().map(Ok))
    }

    #[tokio::test]
    async fn splits_lines_across_chunk_boundaries() {
        let lines: Vec<String> = line_stream(chunks(&[b"one\ntw", b"o\nthree"]))
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(lines, vec!["one", "two", "three"]);
    }

    #[tokio::test]
    async fn multi_byte_characters_survive_a_split_chunk() {
        // "grüezi" with the two bytes of 'ü' arriving in different chunks.
        let lines: Vec<String> = line_stream(chunks(&[b"gr\xc3", b"\xbcezi\n"]))
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(lines, vec!["grüezi"]);
    }

    #[tokio::test]
    async fn carriage_returns_are_stripped() {
        let lines: Vec<String> = line_stream(chunks(&[b"data: x\r\n"]))
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(lines, vec!["data: x"]);
    }

    #[test]
    fn sse_control_lines_are_ignored() {
        assert_eq!(sse_payload("data: {\"a\":1}"), Some("{\"a\":1}"));
        assert_eq!(sse_payload("data: [DONE]"), None);
        assert_eq!(sse_payload("event: message_stop"), None);
        assert_eq!(sse_payload(": keep-alive"), None);
        assert_eq!(sse_payload(""), None);
    }
}
