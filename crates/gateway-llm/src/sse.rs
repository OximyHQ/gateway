//! Minimal SSE frame decoder shared by streaming transports. Accumulates bytes,
//! emits the concatenated `data:` payload for each event terminated by a blank
//! line. Ignores comment lines (`:`...) and non-`data:` fields. The literal
//! `[DONE]` sentinel is surfaced as `SseEvent::Done`. Splitting raw bytes off the
//! wire from chunk PARSING keeps each transport's parser pure and unit-testable.

use bytes::{Bytes, BytesMut};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    /// A data payload (the concatenated `data:` lines of one event).
    Data(String),
    /// The terminal `data: [DONE]` sentinel.
    Done,
}

/// Stateful accumulator. Feed it wire bytes; drain complete events.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: BytesMut,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push raw bytes and return any newly-complete events, in order.
    pub fn push(&mut self, chunk: Bytes) -> Vec<SseEvent> {
        self.buf.extend_from_slice(&chunk);
        let mut out = Vec::new();
        // Normalize on `\n`; an event ends at a blank line (`\n\n`).
        while let Some(pos) = find_event_boundary(&self.buf) {
            let raw = self.buf.split_to(pos);
            // drop the boundary bytes (1 or 2 newlines)
            let drop = boundary_len(&self.buf);
            let _ = self.buf.split_to(drop);
            if let Some(ev) = parse_event(&raw) {
                out.push(ev);
            }
        }
        out
    }
}

/// Index of the first event boundary (end of a `data:` block), or None.
/// Returns the index JUST BEFORE the blank-line separator so `split_to(pos)`
/// leaves the data without a trailing newline.
fn find_event_boundary(buf: &BytesMut) -> Option<usize> {
    let s = buf.as_ref();
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\n' {
            // LF-only: "\n\n"
            if i + 1 < s.len() && s[i + 1] == b'\n' {
                return Some(i);
            }
            // CRLF: "\n\r\n" (the \r\n\r\n case, after the first \r was before i)
            if i + 2 < s.len() && s[i + 1] == b'\r' && s[i + 2] == b'\n' {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Length of the boundary newlines at the FRONT of `buf` to discard.
fn boundary_len(buf: &BytesMut) -> usize {
    let s = buf.as_ref();
    if s.starts_with(b"\r\n\r\n") {
        4
    } else if s.starts_with(b"\n\r\n") {
        // CRLF: the split point was the first \n of \n\r\n
        3
    } else if s.starts_with(b"\n\n") {
        2
    } else if s.starts_with(b"\n") {
        1
    } else {
        0
    }
}

/// Parse one raw event block into a data payload (ignoring comments/other fields).
fn parse_event(raw: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(raw);
    let mut data_lines = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let payload = data_lines.join("\n");
    if payload.trim() == "[DONE]" {
        Some(SseEvent::Done)
    } else {
        Some(SseEvent::Data(payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_event() {
        let mut d = SseDecoder::new();
        let ev = d.push(Bytes::from_static(b"data: {\"a\":1}\n\n"));
        assert_eq!(ev, vec![SseEvent::Data("{\"a\":1}".into())]);
    }

    #[test]
    fn split_across_chunks() {
        let mut d = SseDecoder::new();
        assert!(d.push(Bytes::from_static(b"data: {\"a")).is_empty());
        let ev = d.push(Bytes::from_static(b"\":1}\n\n"));
        assert_eq!(ev, vec![SseEvent::Data("{\"a\":1}".into())]);
    }

    #[test]
    fn done_sentinel_recognized() {
        let mut d = SseDecoder::new();
        let ev = d.push(Bytes::from_static(b"data: [DONE]\n\n"));
        assert_eq!(ev, vec![SseEvent::Done]);
    }

    #[test]
    fn comments_and_keepalives_ignored() {
        let mut d = SseDecoder::new();
        let ev = d.push(Bytes::from_static(b": keepalive\n\ndata: {}\n\n"));
        assert_eq!(ev, vec![SseEvent::Data("{}".into())]);
    }

    #[test]
    fn multiple_events_one_chunk() {
        let mut d = SseDecoder::new();
        let ev = d.push(Bytes::from_static(b"data: 1\n\ndata: 2\n\n"));
        assert_eq!(
            ev,
            vec![SseEvent::Data("1".into()), SseEvent::Data("2".into())]
        );
    }

    #[test]
    fn crlf_line_endings() {
        let mut d = SseDecoder::new();
        let ev = d.push(Bytes::from_static(b"data: x\r\n\r\n"));
        assert_eq!(ev, vec![SseEvent::Data("x".into())]);
    }
}
