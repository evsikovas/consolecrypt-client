//! Local-only, in-memory scrollback ring buffer. Never persisted or synced;
//! zeroized when cleared or dropped (terminal content is sensitive).

use std::collections::VecDeque;
use zeroize::Zeroize;

/// Byte ring buffer with a fixed capacity.
pub struct Scrollback {
    buf: VecDeque<u8>,
    capacity: usize,
    /// Total bytes ever written (for UI offsets).
    written: u64,
}

impl std::fmt::Debug for Scrollback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scrollback")
            .field("len", &self.buf.len())
            .field("capacity", &self.capacity)
            .field("written", &self.written)
            .finish()
    }
}

impl Scrollback {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(capacity.min(64 * 1024)),
            capacity: capacity.max(1),
            written: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn total_written(&self) -> u64 {
        self.written
    }

    /// Append output, evicting the oldest bytes. After eviction the buffer
    /// is advanced to the next line start (within 4 KiB) so a replay does not
    /// begin in the middle of an escape sequence or line.
    pub fn push(&mut self, data: &[u8]) {
        self.written += data.len() as u64;
        let truncated = data.len() > self.capacity;
        let data = if truncated {
            &data[data.len() - self.capacity..]
        } else {
            data
        };
        let overflow = (self.buf.len() + data.len()).saturating_sub(self.capacity);
        if overflow > 0 {
            self.evict(overflow);
        }
        self.buf.extend(data);
        if overflow > 0 || truncated {
            // realign to a line boundary (keep at least something)
            let limit = self.buf.len().min(4096);
            if let Some(pos) = self.buf.iter().take(limit).position(|b| *b == b'\n') {
                if pos + 1 < self.buf.len() {
                    self.evict(pos + 1);
                }
            }
        }
    }

    fn evict(&mut self, n: usize) {
        let n = n.min(self.buf.len());
        for b in self.buf.range_mut(..n) {
            *b = 0;
        }
        self.buf.drain(..n);
    }

    /// Copy of the current content.
    pub fn snapshot(&self) -> Vec<u8> {
        let (a, b) = self.buf.as_slices();
        let mut v = Vec::with_capacity(a.len() + b.len());
        v.extend_from_slice(a);
        v.extend_from_slice(b);
        v
    }

    /// Last `n` bytes.
    pub fn tail(&self, n: usize) -> Vec<u8> {
        let skip = self.buf.len().saturating_sub(n);
        self.buf.iter().skip(skip).copied().collect()
    }

    /// Wipe everything.
    pub fn clear(&mut self) {
        let (a, b) = self.buf.as_mut_slices();
        a.zeroize();
        b.zeroize();
        self.buf.clear();
    }
}

impl Drop for Scrollback {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_latest_bytes() {
        let mut s = Scrollback::new(10);
        s.push(b"abc");
        s.push(b"def");
        assert_eq!(s.snapshot(), b"abcdef");
        s.push(b"ghijkl");
        assert_eq!(s.len(), 10);
        // no newline in the data: nothing to realign to
        assert_eq!(s.snapshot(), b"cdefghijkl");
        s.push(b"0123456789ABCDEF");
        assert_eq!(s.snapshot(), b"6789ABCDEF");
        assert_eq!(s.total_written(), 28);
        assert_eq!(s.tail(3), b"DEF");
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn eviction_realigns_to_line_start() {
        let mut s = Scrollback::new(16);
        s.push(b"line1\nline2\nline3\n");
        let snap = s.snapshot();
        assert!(
            snap.starts_with(b"line"),
            "{:?}",
            String::from_utf8_lossy(&snap)
        );
        assert!(snap.ends_with(b"line3\n"));
    }
}
