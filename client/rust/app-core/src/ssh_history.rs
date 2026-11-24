//! Read the saved history of the login shell over the current SSH connection.
pub(super) const LIMIT: usize = 65536;

// No interactive shell, startup files, PTY input or shell evaluation of history.
// The format marker is outside the bounded file suffix. Skip partial records below.
pub(super) const READ: &str = r#"sh -c '
case "${SHELL##*/}" in
  bash|sh) fmt=bash; file="${HISTFILE-$HOME/.bash_history}" ;;
  zsh) fmt=zsh; file="${HISTFILE-${ZDOTDIR:-$HOME}/.zsh_history}" ;;
  fish) fmt=fish; file="${XDG_DATA_HOME:-$HOME/.local/share}/fish/${fish_history:-fish}_history" ;;
  *) exit 2 ;;
esac
printf "%s\n" "$fmt"
[ -e "$file" ] || exit 0
[ -f "$file" ] && [ -r "$file" ] || exit 1
tail -c 65536 < "$file"
'"#;

pub(super) fn commands(output: &[u8]) -> Vec<String> {
    let Some(header) = output.iter().position(|&b| b == b'\n') else {
        return vec![];
    };
    let kind = &output[..header];
    let mut body = &output[header + 1..];
    let truncated = body.len() >= LIMIT;
    if truncated {
        let Some(first) = body.iter().position(|&b| b == b'\n') else {
            return vec![];
        };
        body = &body[first + 1..];
    }
    let lines: Vec<_> = body
        .split(|&b| b == b'\n')
        .map(std::str::from_utf8)
        .collect();
    let mut result = Vec::new();
    let timestamp = |s: &str| {
        s.strip_prefix('#')
            .is_some_and(|n| n.len() >= 9 && n.bytes().all(|c| c.is_ascii_digit()))
    };
    let timestamped_bash = kind == b"bash" && lines.iter().flatten().any(|s| timestamp(s));
    let mut skipping = truncated && (timestamped_bash || kind == b"zsh");
    for (index, line) in lines.iter().enumerate() {
        let Ok(mut command) = *line else { continue };
        let decoded;
        if timestamped_bash {
            if timestamp(command) {
                skipping = false;
                continue;
            }
            if skipping {
                continue;
            }
            skipping = true;
            // A multi-line history entry must never turn into a runnable fragment.
            if lines.get(index + 1).is_some_and(|n| !matches!(n, Ok(s) if timestamp(s) || (s.is_empty() && index + 2 == lines.len()))) {continue;}
        } else if kind == b"fish" {
            let Some(value) = command.strip_prefix("- cmd: ") else {
                continue;
            };
            // A literal backslash is doubled on disk. Never split an encoded newline
            // into separately executable commands.
            decoded = fish_single_line(value);
            let Some(value) = decoded.as_deref() else {
                continue;
            };
            command = value;
        } else if kind == b"zsh" {
            // Extended history prefixes contain the timestamp and duration.
            if let Some(rest) = command.strip_prefix(": ") {
                if let Some((meta, value)) = rest.split_once(';') {
                    if meta.split_once(':').is_some_and(|(ts, dur)| {
                        !ts.is_empty()
                            && !dur.is_empty()
                            && ts.bytes().chain(dur.bytes()).all(|b| b.is_ascii_digit())
                    }) {
                        command = value;
                        skipping = false;
                    }
                }
            }
        } else if kind != b"bash" {
            return vec![];
        }
        if !timestamped_bash && kind != b"fish" {
            let continuation = command.ends_with('\\');
            let ignore = skipping || continuation;
            skipping = continuation;
            if ignore {
                continue;
            }
        }
        if command.is_empty()
            || command.starts_with([' ', '#'])
            || command.len() > 2048
            || command.chars().any(char::is_control)
            || cc_docker_core::contains_secret(command)
        {
            continue;
        }
        if result.last().is_none_or(|last| last != command) {
            result.push(command.to_owned());
        }
    }
    result.into_iter().rev().take(100).collect()
}

fn fish_single_line(value: &str) -> Option<String> {
    let mut decoded = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.next() != Some('\\') {
            return None;
        }
        decoded.push(c);
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_persisted_records_newest_first() {
        assert_eq!(
            commands(
                b"bash\n#1700000000\nuptime\n#1700000001\necho one\necho two\n#1700000002\ndate\n"
            ),
            ["date", "uptime"]
        );
        assert_eq!(
            commands(
                b"zsh\n: 1700000000:0;pwd\n: 1700000001:1;echo \\\npart\n: 1700000002:0;uptime\n"
            ),
            ["uptime", "pwd"]
        );
        assert_eq!(
            commands(b"fish\n- cmd: uptime\n  when: 1700000000\n- cmd: echo\\nsecond\n"),
            ["uptime"]
        );
    }
    #[test]
    fn fish_restores_literal_backslashes() {
        assert_eq!(commands(b"fish\n- cmd: printf \\\\n\n"), [r"printf \n"]);
    }

    #[test]
    fn drops_private_and_partial_lines() {
        let mut data = b"bash\n".to_vec();
        data.extend(vec![b'x'; LIMIT - b"\nsudo -V\n private\n".len()]);
        data.extend(b"\nsudo -V\n private\n");
        assert_eq!(commands(&data), ["sudo -V"]);
    }
}
