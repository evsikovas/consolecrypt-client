use encoding_rs::{CoderResult, Decoder, Encoding, UTF_8};
use flutter_rust_bridge::frb;

/// One streaming decoder per terminal pane. Escape sequences remain raw ASCII.
#[frb(opaque)]
pub struct TerminalDecoder {
    decoder: Decoder,
}

impl TerminalDecoder {
    #[frb(sync)]
    pub fn new(label: String) -> Self {
        Self {
            decoder: codec(&label).new_decoder_without_bom_handling(),
        }
    }

    #[frb(sync)]
    pub fn decode(&mut self, bytes: Vec<u8>, last: bool) -> String {
        let mut output = String::with_capacity(
            self.decoder
                .max_utf8_buffer_length(bytes.len())
                .unwrap_or(bytes.len()),
        );
        let mut input = bytes.as_slice();
        loop {
            let (result, read, _) = self.decoder.decode_to_string(input, &mut output, last);
            if result == CoderResult::InputEmpty {
                return output;
            }
            input = &input[read..];
            output.reserve(input.len().saturating_mul(3).max(16));
        }
    }
}

fn codec(label: &str) -> &'static Encoding {
    Encoding::for_label(label.as_bytes()).unwrap_or(UTF_8)
}

/// Reject an unrepresentable paste as a whole rather than changing a shell command.
#[frb(sync)]
pub fn encode_terminal_input(label: String, text: String) -> Option<Vec<u8>> {
    let (bytes, _, invalid) = codec(&label).encode(&text);
    (!invalid).then(|| bytes.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_multibyte_text_and_round_trip() {
        for (label, text) in [
            ("gbk", "中文"),
            ("gb18030", "𠀀"),
            ("big5", "繁體"),
            ("shift_jis", "日本語"),
            ("euc-jp", "日本語"),
            ("euc-kr", "한글"),
            ("windows-1251", "Привет"),
            ("koi8-r", "Привет"),
            ("windows-1252", "€"),
            ("iso-8859-2", "Łódź"),
            ("windows-1250", "Český"),
            ("iso-8859-15", "€"),
        ] {
            let input = format!("\x1b[34m{text}\x1b[0m");
            let encoded = encode_terminal_input(label.into(), input.clone()).unwrap();
            let mut decoder = TerminalDecoder::new(label.into());
            let mut output = String::new();
            for byte in encoded {
                output.push_str(&decoder.decode(vec![byte], false));
            }
            output.push_str(&decoder.decode(vec![], true));
            assert_eq!(output, input, "{label}");
        }
        assert!(encode_terminal_input("windows-1251".into(), "echo 😀".into()).is_none());
    }
}
