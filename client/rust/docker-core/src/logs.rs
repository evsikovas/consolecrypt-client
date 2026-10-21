//! Log stream decoding: the engine multiplexes stdout/stderr with 8-byte
//! frame headers unless the container has a TTY, where the stream is raw.
use crate::client::LogLine;

/// Turns frames (or raw bytes) into complete lines; a partial line waits for
/// the rest of its bytes.
pub(crate) struct LogDecoder {
    multiplexed: bool,
    timestamps: bool,
    buffer: Vec<u8>,
    pending: [Vec<u8>; 2],
}

const MAX_LINE: usize = 64 * 1024;

impl LogDecoder {
    pub fn new(multiplexed: bool, timestamps: bool) -> Self {
        Self {
            multiplexed,
            timestamps,
            buffer: Vec::new(),
            pending: [Vec::new(), Vec::new()],
        }
    }

    pub fn push(&mut self, chunk: &[u8], out: &mut Vec<LogLine>) {
        if !self.multiplexed {
            self.text(0, chunk, out);
            return;
        }
        self.buffer.extend_from_slice(chunk);
        loop {
            if self.buffer.len() < 8 {
                return;
            }
            let size = u32::from_be_bytes([
                self.buffer[4],
                self.buffer[5],
                self.buffer[6],
                self.buffer[7],
            ]) as usize;
            if self.buffer.len() < 8 + size {
                return;
            }
            let stream = usize::from(self.buffer[0] == 2);
            let payload: Vec<u8> = self.buffer[8..8 + size].to_vec();
            self.buffer.drain(..8 + size);
            self.text(stream, &payload, out);
        }
    }

    /// Emits what is left (a last line without a newline).
    pub fn finish(&mut self, out: &mut Vec<LogLine>) {
        for stream in 0..2 {
            if !self.pending[stream].is_empty() {
                let line = std::mem::take(&mut self.pending[stream]);
                out.push(self.line(stream, &line));
            }
        }
    }

    fn text(&mut self, stream: usize, bytes: &[u8], out: &mut Vec<LogLine>) {
        for &byte in bytes {
            if byte == b'\n' {
                let line = std::mem::take(&mut self.pending[stream]);
                out.push(self.line(stream, &line));
            } else if self.pending[stream].len() < MAX_LINE {
                self.pending[stream].push(byte);
            }
        }
    }

    fn line(&self, stream: usize, bytes: &[u8]) -> LogLine {
        let text = String::from_utf8_lossy(bytes);
        let text = text.strip_suffix('\r').unwrap_or(&text);
        let (timestamp, text) = match (self.timestamps, text.split_once(' ')) {
            (true, Some((stamp, rest))) if stamp.len() >= 20 && stamp.as_bytes()[4] == b'-' => {
                (Some(stamp.to_owned()), rest.to_owned())
            }
            _ => (None, text.to_owned()),
        };
        LogLine {
            stream: if stream == 1 { "stderr" } else { "stdout" },
            text,
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(stream: u8, text: &str) -> Vec<u8> {
        let mut out = vec![stream, 0, 0, 0];
        out.extend_from_slice(&(text.len() as u32).to_be_bytes());
        out.extend_from_slice(text.as_bytes());
        out
    }

    #[test]
    fn multiplexed_frames_split_into_lines_across_chunks() {
        let mut decoder = LogDecoder::new(true, false);
        let mut bytes = frame(1, "hello\nwor");
        bytes.extend(frame(2, "oops\n"));
        bytes.extend(frame(1, "ld\n"));
        let mut out = Vec::new();
        for chunk in bytes.chunks(3) {
            decoder.push(chunk, &mut out);
        }
        let lines: Vec<_> = out.iter().map(|l| (l.stream, l.text.as_str())).collect();
        assert_eq!(
            lines,
            [("stdout", "hello"), ("stderr", "oops"), ("stdout", "world")]
        );
    }

    #[test]
    fn raw_tty_streams_and_timestamps() {
        let mut decoder = LogDecoder::new(false, true);
        let mut out = Vec::new();
        decoder.push(
            b"2026-10-21T08:00:00.123456789Z started\r\npartial",
            &mut out,
        );
        decoder.finish(&mut out);
        assert_eq!(
            out[0].timestamp.as_deref(),
            Some("2026-10-21T08:00:00.123456789Z")
        );
        assert_eq!(out[0].text, "started");
        assert_eq!(out[1].text, "partial");
    }
}
