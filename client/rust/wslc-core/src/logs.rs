//! Bounded log buffers and incremental UTF-8 decoding used by live readers.
use std::collections::VecDeque;

pub const MAX_LINES: usize = 10_000;
pub const MAX_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_RECORD_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct LogRecord {
    pub text: String,
    pub truncated: bool,
}

#[derive(Default)]
pub struct LogBuffer {
    lines: VecDeque<LogRecord>,
    bytes: usize,
    pub evicted: u64,
}

impl LogBuffer {
    pub fn push(&mut self, mut text: String) {
        let truncated = text.len() > MAX_RECORD_BYTES;
        if truncated {
            let mut end = MAX_RECORD_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        self.bytes += text.len() + 1; // Include the line separator in text storage budget.
        self.lines.push_back(LogRecord { text, truncated });
        while self.lines.len() > MAX_LINES || self.bytes > MAX_BYTES {
            if let Some(line) = self.lines.pop_front() {
                self.bytes -= line.text.len() + 1;
                self.evicted += 1;
            }
        }
    }
    pub fn records(&self) -> &VecDeque<LogRecord> {
        &self.lines
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn clear(&mut self) {
        self.lines.clear();
        self.bytes = 0;
        self.evicted = 0;
    }
}

/// Assemble byte chunks before UTF-8 decoding, including split Cyrillic characters.
/// Oversized lines are drained to newline with one truncation mark, without growing memory.
#[derive(Default)]
pub struct LineDecoder {
    pending: Vec<u8>,
    truncated: bool,
}

impl LineDecoder {
    pub fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(LogRecord)) {
        for &byte in bytes {
            if byte == b'\n' {
                emit(self.finish_line());
            } else if self.pending.len() < MAX_RECORD_BYTES {
                self.pending.push(byte);
            } else {
                self.truncated = true;
            }
        }
    }
    fn finish_line(&mut self) -> LogRecord {
        if self.pending.last() == Some(&b'\r') {
            self.pending.pop();
        }
        // Drop a partially truncated final code point instead of introducing a fake character.
        if self.truncated {
            while let Err(e) = std::str::from_utf8(&self.pending) {
                if e.error_len().is_some() {
                    break;
                }
                self.pending.truncate(e.valid_up_to());
            }
        }
        let result = LogRecord {
            text: String::from_utf8_lossy(&self.pending).into(),
            truncated: self.truncated,
        };
        self.pending.clear();
        self.truncated = false;
        result
    }
    pub fn finish(&mut self) -> Option<LogRecord> {
        if self.pending.is_empty() && !self.truncated {
            None
        } else {
            Some(self.finish_line())
        }
    }
}
