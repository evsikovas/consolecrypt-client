//! Compose files as shown without the vault passphrase: values of secret
//! looking `environment`, `labels` and build `args` entries, secret flags of
//! `command`, `entrypoint` and `healthcheck.test`, URL passwords, PEM keys
//! and tokens are hidden. Line by line, so every line keeps its number and
//! everything else stays as written. Not a YAML parser: indentation is
//! enough to know which block a line belongs to.
use crate::secrets::{apply, mask_argv, maskable, mysql_image, secret_ranges, sensitive_key, MASK};
use std::ops::Range;

/// Blocks whose `KEY: value` / `- KEY=value` entries are variables.
const VARIABLE_BLOCKS: &[&str] = &["environment", "labels", "args"];
/// Compose's own keys that merely look secret.
const KEYWORDS: &[&str] = &["secrets", "credential_spec"];

/// A block scalar (`key: |`) whose lines follow.
struct Block {
    indent: usize,
    hide: bool,
}

/// A block sequence of arguments (`command:` then `- arg` lines).
struct Arguments {
    indent: usize,
    items: Vec<(usize, Range<usize>)>,
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// The end of a plain or quoted scalar starting at `start` (before a
/// trailing ` #` comment).
fn scalar_end(line: &str, start: usize) -> usize {
    let rest = &line[start..];
    for quote in ['"', '\''] {
        if rest.starts_with(quote) {
            return rest[1..]
                .find(quote)
                .map_or(line.len(), |i| start + 1 + i + 1);
        }
    }
    let end = rest.find(" #").map_or(line.len(), |i| start + i);
    start + line[start..end].trim_end().len()
}

/// The text inside a scalar's quotes, after an anchor or tag.
fn scalar_value(line: &str, mut start: usize, end: usize) -> Range<usize> {
    while start < end && matches!(line.as_bytes()[start], b'&' | b'!') {
        match line[start..end].find(' ') {
            Some(space) => start += space + 1,
            None => return end..end,
        }
        while start < end && line.as_bytes()[start] == b' ' {
            start += 1;
        }
    }
    let slice = &line[start..end];
    for quote in ['"', '\''] {
        if slice.len() >= 2 && slice.starts_with(quote) && slice.ends_with(quote) {
            return start + 1..end - 1;
        }
    }
    start..end
}

/// `key: value` at `start`: the key and where the value begins (`None` for
/// a plain scalar).
fn mapping(line: &str, start: usize) -> Option<(String, usize)> {
    let rest = &line[start..];
    let (key, after) = match rest.chars().next()? {
        quote @ ('"' | '\'') => {
            let close = rest[1..].find(quote)? + 1;
            (rest[1..close].to_owned(), start + close + 1)
        }
        '[' | '{' | '#' => return None,
        _ => {
            let colon = rest
                .match_indices(':')
                .map(|(i, _)| i)
                .find(|i| matches!(rest.as_bytes().get(i + 1), None | Some(b' ')))?;
            if rest[..colon].contains(['"', '\'', '[', '{']) {
                return None;
            }
            (rest[..colon].trim().to_owned(), start + colon)
        }
    };
    if line.as_bytes().get(after) != Some(&b':') {
        return None;
    }
    let mut value = after + 1;
    while value < line.len() && line.as_bytes()[value] == b' ' {
        value += 1;
    }
    Some((key, value))
}

/// Items of a flow sequence `[a, "b", 'c']` starting at `start`.
fn flow_items(line: &str, start: usize) -> Vec<Range<usize>> {
    let mut items = Vec::new();
    let bytes = line.as_bytes();
    let mut index = start + 1;
    loop {
        while index < bytes.len() && bytes[index] == b' ' {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b']' {
            break;
        }
        let item_start = index;
        if matches!(bytes[index], b'"' | b'\'') {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() && bytes[index] != quote {
                index += if bytes[index] == b'\\' && quote == b'"' {
                    2
                } else {
                    1
                };
            }
            index = (index + 1).min(bytes.len());
        } else {
            while index < bytes.len() && !matches!(bytes[index], b',' | b']') {
                index += 1;
            }
        }
        let item_end = item_start + line[item_start..index].trim_end().len();
        items.push(scalar_value(line, item_start, item_end));
        while index < bytes.len() && bytes[index] != b',' && bytes[index] != b']' {
            index += 1;
        }
        if index < bytes.len() && bytes[index] == b',' {
            index += 1;
        }
    }
    items
}

/// Replaces `items` of `line` by `masked` texts (same order).
fn replace_items(line: &str, items: &[Range<usize>], masked: &[String]) -> String {
    let mut out = String::with_capacity(line.len());
    let mut cursor = 0;
    for (range, text) in items.iter().zip(masked) {
        out.push_str(&line[cursor..range.start]);
        out.push_str(text);
        cursor = range.end;
    }
    out.push_str(&line[cursor..]);
    out
}

fn argv_masked(texts: &[String], mysql: bool) -> Option<Vec<String>> {
    let (masked, found) = mask_argv(texts, mysql);
    found.then_some(masked)
}

/// `KEY=value` (list form of variables): the value range when the key is
/// secret.
fn assignment_value(line: &str, item: Range<usize>) -> Option<Range<usize>> {
    let text = &line[item.clone()];
    let (key, _) = text.split_once('=')?;
    if !sensitive_key(key.trim()) {
        return None;
    }
    Some(item.start + key.len() + 1..item.end)
}

/// Hides secrets of a compose file without changing its lines: `(text,
/// anything hidden)`.
pub fn mask_compose(text: &str) -> (String, bool) {
    let mysql = text.lines().any(|line| {
        let line = line.trim_start().trim_start_matches("- ");
        line.strip_prefix("image:").is_some_and(mysql_image)
    });
    // Lines without their terminators; terminators are put back as found.
    let mut lines: Vec<String> = Vec::new();
    let mut endings: Vec<&str> = Vec::new();
    for piece in text.split_inclusive('\n') {
        let body = piece.trim_end_matches('\n').trim_end_matches('\r');
        lines.push(body.to_owned());
        endings.push(&piece[body.len()..]);
    }
    let mut found = false;
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut block: Option<Block> = None;
    let mut arguments: Option<Arguments> = None;
    let mut pem = false;

    let finish_arguments = |arguments: Option<Arguments>,
                            lines: &mut Vec<String>,
                            found: &mut bool| {
        let Some(arguments) = arguments else { return };
        let texts: Vec<String> = arguments
            .items
            .iter()
            .map(|(index, range)| lines[*index][range.clone()].to_owned())
            .collect();
        if let Some(masked) = argv_masked(&texts, mysql) {
            *found = true;
            for ((index, range), text) in arguments.items.iter().zip(masked) {
                let line = &lines[*index];
                lines[*index] = format!("{}{}{}", &line[..range.start], text, &line[range.end..]);
            }
        }
    };

    for index in 0..lines.len() {
        let line = lines[index].clone();
        let indent = indent_of(&line);
        let body = &line[indent..];
        let blank = body.trim().is_empty();

        if pem {
            if !blank {
                let end = body
                    .find("-----END")
                    .and_then(|i| body[i + 8..].find("-----").map(|j| indent + i + 8 + j + 5))
                    .unwrap_or(line.len());
                pem = !body.contains("-----END");
                lines[index] = format!("{}{MASK}{}", &line[..indent], &line[end..]);
                found = true;
            }
            continue;
        }
        if let Some(current) = &block {
            if blank || indent > current.indent {
                if current.hide && !blank {
                    lines[index] = format!("{}{MASK}", &line[..indent]);
                    found = true;
                } else if !blank {
                    let (masked, hidden) = apply(&line, secret_ranges(&line, mysql));
                    found |= hidden;
                    pem = opens_pem(&line);
                    lines[index] = masked;
                }
                continue;
            }
            block = None;
        }
        if let Some(current) = &mut arguments {
            let item = body == "-" || body.starts_with("- ");
            if blank || indent > current.indent || (item && indent == current.indent) {
                if item {
                    let start = (indent + 2).min(line.len());
                    let end = scalar_end(&line, start);
                    current.items.push((index, scalar_value(&line, start, end)));
                }
                continue;
            }
            finish_arguments(arguments.take(), &mut lines, &mut found);
        }
        if blank || body.starts_with('#') {
            let (masked, hidden) = apply(&line, secret_ranges(&line, mysql));
            found |= hidden;
            lines[index] = masked;
            continue;
        }

        let dash = body == "-" || body.starts_with("- ");
        // Where the content (a key or a scalar) starts.
        let mut start = indent;
        if dash {
            start += 1;
            while start < line.len() && line.as_bytes()[start] == b' ' {
                start += 1;
            }
            stack.retain(|(level, _)| *level <= indent);
        } else {
            stack.retain(|(level, _)| *level < indent);
        }
        let parent = stack.last().map(|(_, key)| key.to_ascii_lowercase());
        let mut ranges: Vec<Range<usize>> = Vec::new();

        match mapping(&line, start) {
            Some((key, value)) => {
                if dash {
                    stack.retain(|(level, _)| *level < start);
                }
                stack.push((start, key.clone()));
                let lower = key.to_ascii_lowercase();
                let arguments_key = matches!(lower.as_str(), "command" | "entrypoint")
                    || (lower == "test" && parent.as_deref() == Some("healthcheck"));
                let rest = &line[value..];
                let secret_key = sensitive_key(&key) && !KEYWORDS.contains(&lower.as_str());
                if rest.is_empty() || rest.starts_with('#') {
                    if arguments_key {
                        arguments = Some(Arguments {
                            indent: start,
                            items: Vec::new(),
                        });
                    }
                } else if rest.starts_with(['|', '>']) {
                    block = Some(Block {
                        indent: start,
                        hide: secret_key,
                    });
                } else if arguments_key && rest.starts_with('[') {
                    let items = flow_items(&line, value);
                    let texts: Vec<String> =
                        items.iter().map(|r| line[r.clone()].to_owned()).collect();
                    if let Some(masked) = argv_masked(&texts, mysql) {
                        found = true;
                        lines[index] = replace_items(&line, &items, &masked);
                        continue;
                    }
                } else if secret_key && !rest.starts_with(['[', '{', '*']) {
                    let end = scalar_end(&line, value);
                    let inner = scalar_value(&line, value, end);
                    if maskable(&line[inner.clone()]) {
                        ranges.push(inner);
                    }
                }
                for range in secret_ranges(rest, mysql) {
                    ranges.push(value + range.start..value + range.end);
                }
            }
            None => {
                let end = scalar_end(&line, start);
                let item = scalar_value(&line, start, end);
                let variables = dash
                    && parent
                        .as_deref()
                        .is_some_and(|p| VARIABLE_BLOCKS.contains(&p));
                if variables {
                    if let Some(value) = assignment_value(&line, item.clone()) {
                        if maskable(&line[value.clone()]) {
                            ranges.push(value);
                        }
                    }
                }
                let rest = &line[start..];
                for range in secret_ranges(rest, mysql) {
                    ranges.push(start + range.start..start + range.end);
                }
            }
        }
        let (masked, hidden) = apply(&line, ranges);
        found |= hidden;
        pem = opens_pem(&line);
        lines[index] = masked;
    }
    finish_arguments(arguments.take(), &mut lines, &mut found);

    let mut out = String::with_capacity(text.len());
    for (line, ending) in lines.iter().zip(endings) {
        out.push_str(line);
        out.push_str(ending);
    }
    (out, found)
}

/// A private key starts on this line and ends on a later one.
fn opens_pem(line: &str) -> bool {
    line.find("-----BEGIN ").is_some_and(|start| {
        let rest = &line[start..];
        rest.contains("PRIVATE KEY") && !rest.contains("-----END")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPOSE: &str = r#"services:
  db:
    image: postgres:16
    environment:
      POSTGRES_USER: shop
      POSTGRES_PASSWORD: hunter2
      POSTGRES_DB: "shop"
      DATABASE_URL: postgres://shop:hunter2@db:5432/shop
      API_TOKEN: ${API_TOKEN}
    labels:
      - "traefik.http.middlewares.auth.basicauth.users=admin:$$apr1$$x"
      - "com.example.team=shop"
  cache:
    image: redis:7
    command: ["redis-server", "--requirepass", "hunter2", "--appendonly", "yes"]
    healthcheck:
      test: ["CMD-SHELL", "redis-cli -a hunter2 ping"]
  web:
    image: shop/web
    command:
      - serve
      - --api-key
      - hunter2
    environment:
      - SECRET_KEY_BASE=hunter2 # rotated monthly
      - PORT=8080
    build:
      args:
        NPM_TOKEN: hunter2
    secrets:
      - db_password
    x-tls: |
      -----BEGIN PRIVATE KEY-----
      MIIEvQIBADANBg
      -----END PRIVATE KEY-----
secrets:
  db_password:
    file: ./db_password.txt
"#;

    #[test]
    fn hides_values_and_keeps_lines() {
        let (masked, found) = mask_compose(COMPOSE);
        assert!(found);
        assert!(!masked.contains("hunter2"), "{masked}");
        assert!(!masked.contains("MIIEvQ"), "{masked}");
        assert!(!masked.contains("apr1"), "{masked}");
        assert_eq!(masked.lines().count(), COMPOSE.lines().count());
        let lines: Vec<&str> = masked.lines().collect();
        assert_eq!(lines[4], "      POSTGRES_USER: shop");
        assert_eq!(lines[5], "      POSTGRES_PASSWORD: ••••••");
        assert_eq!(lines[6], "      POSTGRES_DB: \"shop\"");
        assert_eq!(
            lines[7],
            "      DATABASE_URL: postgres://shop:••••••@db:5432/shop"
        );
        assert_eq!(lines[8], "      API_TOKEN: ${API_TOKEN}");
        assert_eq!(
            lines[10],
            "      - \"traefik.http.middlewares.auth.basicauth.users=••••••\""
        );
        assert_eq!(lines[11], "      - \"com.example.team=shop\"");
        assert_eq!(
            lines[14],
            "    command: [\"redis-server\", \"--requirepass\", \"••••••\", \"--appendonly\", \"yes\"]"
        );
        assert_eq!(
            lines[16],
            "      test: [\"CMD-SHELL\", \"redis-cli -a •••••• ping\"]"
        );
        assert_eq!(lines[21], "      - --api-key");
        assert_eq!(lines[22], "      - ••••••");
        assert_eq!(
            lines[24],
            "      - SECRET_KEY_BASE=•••••• # rotated monthly"
        );
        assert_eq!(lines[25], "      - PORT=8080");
        assert_eq!(lines[28], "        NPM_TOKEN: ••••••");
        assert_eq!(lines[30], "      - db_password");
        assert_eq!(lines[32], "      ••••••");
        assert_eq!(lines[33], "      ••••••");
        assert_eq!(lines[34], "      ••••••");
        assert_eq!(lines[36], "  db_password:");
        assert_eq!(lines[37], "    file: ./db_password.txt");
        // Masking is stable.
        assert_eq!(mask_compose(&masked), (masked.clone(), false));
    }

    #[test]
    fn nothing_secret_stays_byte_for_byte() {
        let plain = "services:\r\n  web:\r\n    image: nginx\r\n    ports: [\"8080:80\"]\r\n    command: nginx -g 'daemon off;'\r\n";
        assert_eq!(mask_compose(plain), (plain.to_owned(), false));
        assert_eq!(mask_compose(""), (String::new(), false));
    }

    #[test]
    fn mysql_images_hide_dash_p_and_secret_keys_anywhere() {
        let text = "x-env: &env\n  DB_PASSWORD: 'hunter2'\nservices:\n  db:\n    image: mariadb:11\n    healthcheck:\n      test: mariadb-admin ping -uroot -phunter2\n    command: --default-authentication-plugin=x\n  app:\n    environment: *env\n    entrypoint: >\n      sh -c 'exec app --password hunter2'\n";
        let (masked, found) = mask_compose(text);
        assert!(found);
        assert!(!masked.contains("hunter2"), "{masked}");
        assert!(masked.contains("  DB_PASSWORD: '••••••'"), "{masked}");
        assert!(
            masked.contains("test: mariadb-admin ping -uroot -p••••••"),
            "{masked}"
        );
        assert!(masked.contains("environment: *env"), "{masked}");
    }
}
