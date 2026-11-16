//! Bounded canonical RFC4648 URL alphabet encoding; no signing or authority.
use super::{ReaderError, ReaderResult};
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
pub fn reader_base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 4 + 2) / 3);
    for part in bytes.chunks(3) {
        let a = part[0];
        let b = *part.get(1).unwrap_or(&0);
        let c = *part.get(2).unwrap_or(&0);
        out.push(ALPHABET[(a >> 2) as usize] as char);
        out.push(ALPHABET[((a & 3) << 4 | b >> 4) as usize] as char);
        if part.len() > 1 {
            out.push(ALPHABET[((b & 15) << 2 | c >> 6) as usize] as char);
        }
        if part.len() > 2 {
            out.push(ALPHABET[(c & 63) as usize] as char);
        }
    }
    out
}
pub fn reader_decode_base64url(text: &str, max: usize) -> ReaderResult<Vec<u8>> {
    if text.len() > max.saturating_mul(4).saturating_add(2) / 3 || text.len() % 4 == 1 {
        return Err(ReaderError::InvalidWire);
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut bits = 0u32;
    let mut available = 0;
    for byte in text.bytes() {
        let value = ALPHABET
            .iter()
            .position(|b| *b == byte)
            .ok_or(ReaderError::InvalidWire)? as u32;
        bits = (bits << 6) | value;
        available += 6;
        if available >= 8 {
            available -= 8;
            out.push((bits >> available) as u8);
            bits &= (1 << available) - 1;
        }
    }
    if bits != 0 || out.len() > max || reader_base64url(&out) != text {
        return Err(ReaderError::InvalidWire);
    }
    Ok(out)
}
