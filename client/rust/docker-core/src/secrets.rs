//! What counts as a secret in Docker data and how it is hidden. One masker
//! serves every view (inspect, command lines, labels, compose files, copied
//! log text), so nothing secret-looking reaches the UI without the vault
//! passphrase. Deterministic and allocation-light; no regular expressions.
//!
//! A secret is:
//! * the value of a variable or label whose name looks secret
//!   ([`sensitive_key`]);
//! * the password of a URL (`scheme://user:password@host`);
//! * the value of a secret command-line flag (`--password x`,
//!   `--requirepass=x`, MySQL's `-px`);
//! * a PEM private key block;
//! * a known token format (GitHub, GitLab, Slack, OpenAI/Stripe style
//!   `sk-`, AWS access key ids, JWTs).
//!
//! Variable references (`$NAME`, `${NAME}`) are not secrets and stay.
use serde_json::Value;
use std::ops::Range;

/// What a hidden value becomes. Its length never depends on the secret.
pub const MASK: &str = "••••••";

/// Substrings that make a name secret (the historic list).
const SECRET_PARTS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "pwd",
    "token",
    "secret",
    "api_key",
    "apikey",
    "private_key",
    "privatekey",
    "credential",
    "access_key",
    "auth",
];

/// Whole words (split at `_`, `-`, `.`, and camelCase) that make a name
/// secret: `STRIPE_KEY`, `SENTRY_DSN`, `sessionCookie`.
const SECRET_WORDS: &[&str] = &[
    "key",
    "dsn",
    "cert",
    "private",
    "privkey",
    "session",
    "cookie",
    "salt",
    "signature",
    "pass",
    "jwt",
];

/// Names that hold where a secret is, not the secret (`*_PASSWORD_FILE`).
const PATH_SUFFIXES: &[&str] = &["file", "path", "dir"];

/// Command-line programs of MySQL and MariaDB, where `-p<password>` is the
/// password (elsewhere `-p` is usually a port).
const MYSQL_PROGRAMS: &[&str] = &[
    "mysql",
    "mysqld",
    "mysqladmin",
    "mysqldump",
    "mysqlcheck",
    "mysqlimport",
    "mysqlshow",
    "mysqlpump",
    "mysqlsh",
    "mariadb",
    "mariadbd",
    "mariadb-admin",
    "mariadb-dump",
    "mariadb-check",
    "mariadb-import",
    "mariadb-show",
];

fn words(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for ch in name.chars() {
        if !ch.is_ascii_alphanumeric() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            previous_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() && previous_lower && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        previous_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        current.push(ch.to_ascii_lowercase());
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Whether a variable, label, key or flag name looks secret (any case).
pub fn sensitive_key(name: &str) -> bool {
    let parts = words(name);
    if parts
        .last()
        .is_some_and(|last| PATH_SUFFIXES.contains(&last.as_str()))
        && parts.len() > 1
    {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    SECRET_PARTS.iter().any(|part| lower.contains(part))
        || parts
            .iter()
            .any(|word| SECRET_WORDS.contains(&word.as_str()))
}

/// `--password`, `--requirepass`, `--api-key`… (dashes optional).
fn secret_flag(flag: &str) -> bool {
    let name = flag
        .trim_start_matches('-')
        .to_ascii_lowercase()
        .replace('-', "_");
    if name.is_empty() {
        return false;
    }
    let parts = words(&name);
    if parts.len() > 1
        && parts
            .last()
            .is_some_and(|last| PATH_SUFFIXES.contains(&last.as_str()))
    {
        return false;
    }
    matches!(
        name.as_str(),
        "pass" | "key" | "requirepass" | "masterauth" | "apikey" | "api_key"
    ) || [
        "password",
        "passwd",
        "secret",
        "token",
        "private_key",
        "access_key",
    ]
    .iter()
    .any(|part| name.contains(part))
}

fn program(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn is_mysql_program(word: &str) -> bool {
    MYSQL_PROGRAMS.contains(&program(word))
}

/// `redis-cli -a <password>`.
fn is_redis_cli(word: &str) -> bool {
    program(word) == "redis-cli"
}

/// Images whose command lines take MySQL's `-p<password>`.
pub fn mysql_image(image: &str) -> bool {
    let image = image.to_ascii_lowercase();
    ["mysql", "mariadb", "percona"]
        .iter()
        .any(|name| image.contains(name))
}

/// `$NAME`, `${NAME}`, `${NAME?message}`, `${NAME:?message}`: resolved
/// elsewhere, not a secret itself.
fn variable_reference(value: &str) -> bool {
    let name_ok = |name: &str| {
        let bytes = name.as_bytes();
        !bytes.is_empty()
            && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
            && bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    };
    if let Some(inner) = value.strip_prefix("${").and_then(|v| v.strip_suffix('}')) {
        let name = inner
            .split_once(":?")
            .or_else(|| inner.split_once('?'))
            .map_or(inner, |(name, _)| name);
        return name_ok(name);
    }
    value.strip_prefix('$').is_some_and(name_ok)
}

/// Whether `value` should be hidden at all.
pub(crate) fn maskable(value: &str) -> bool {
    !value.is_empty() && value != MASK && !variable_reference(value)
}

/// The part of `range` inside matching quotes, if it is quoted.
fn unquoted(text: &str, range: Range<usize>) -> Range<usize> {
    let slice = &text[range.clone()];
    for quote in ['"', '\''] {
        if slice.len() >= 2 && slice.starts_with(quote) && slice.ends_with(quote) {
            return range.start + 1..range.end - 1;
        }
    }
    range
}

/// Whitespace-separated words; a quote that opens a word or a value
/// (`='…'`, `-p'…'`) keeps spaces inside one word.
fn tokens(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = None;
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for (index, ch) in text.char_indices() {
        match quote {
            Some(open) => {
                if ch == open {
                    quote = None;
                }
            }
            None if ch.is_whitespace() => {
                if let Some(begin) = start.take() {
                    out.push(begin..index);
                }
            }
            None => {
                let begin = *start.get_or_insert(index);
                let opens =
                    begin == index || matches!(previous, '=' | ':') || &text[begin..index] == "-p";
                if (ch == '"' || ch == '\'') && opens {
                    quote = Some(ch);
                }
            }
        }
        previous = ch;
    }
    if let Some(begin) = start {
        out.push(begin..text.len());
    }
    out
}

fn push(ranges: &mut Vec<Range<usize>>, text: &str, range: Range<usize>) {
    if range.start < range.end && maskable(&text[range.clone()]) {
        ranges.push(range);
    }
}

/// PEM private key blocks, whole.
fn pem_blocks(text: &str, ranges: &mut Vec<Range<usize>>) {
    let mut from = 0;
    while let Some(found) = text[from..].find("-----BEGIN ") {
        let start = from + found;
        let header_end = text[start + 11..].find("-----").map(|i| start + 11 + i + 5);
        let Some(header_end) = header_end else { break };
        if !text[start..header_end].contains("PRIVATE KEY") {
            from = header_end;
            continue;
        }
        let end = text[header_end..]
            .find("-----END")
            .and_then(|i| {
                let label = header_end + i + 8;
                text[label..].find("-----").map(|j| label + j + 5)
            })
            .unwrap_or(text.len());
        ranges.push(start..end);
        from = end;
    }
}

const URL_STOP: &[char] = &['"', '\'', '`', '<', '>'];

/// Passwords of `scheme://user:password@host` URLs.
fn url_passwords(text: &str, ranges: &mut Vec<Range<usize>>) {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(found) = text[from..].find("://") {
        let marker = from + found;
        from = marker + 3;
        // A scheme: a letter, then letters, digits, `+`, `.`, `-`.
        let mut scheme_start = marker;
        while scheme_start > 0
            && (bytes[scheme_start - 1].is_ascii_alphanumeric()
                || matches!(bytes[scheme_start - 1], b'+' | b'.' | b'-'))
        {
            scheme_start -= 1;
        }
        if scheme_start == marker || !bytes[scheme_start].is_ascii_alphabetic() {
            continue;
        }
        let rest = &text[from..];
        let region_end = rest
            .find(|c: char| c.is_whitespace() || URL_STOP.contains(&c))
            .unwrap_or(rest.len());
        let region = &rest[..region_end];
        let authority_end = region.find(['/', '?', '#']).unwrap_or(region.len());
        let at = region[..authority_end].rfind('@').or_else(|| {
            // An unencoded `/` in the password ends the authority early.
            region.find('@').filter(|at| {
                let userinfo = &region[..*at];
                !userinfo.contains(['?', '#'])
                    && userinfo
                        .split_once(':')
                        .is_some_and(|(user, _)| !user.contains('/'))
            })
        });
        let Some(at) = at else { continue };
        let Some(colon) = region[..at].find(':') else {
            continue;
        };
        push(ranges, text, from + colon + 1..from + at);
    }
}

/// Values written as `name=value`, `name: value`, `"name": "value"` where
/// the name looks secret.
fn named_values(text: &str, ranges: &mut Vec<Range<usize>>) {
    let bytes = text.as_bytes();
    let ident = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-');
    let mut index = 0;
    while index < bytes.len() {
        if !ident(bytes[index]) || (index > 0 && ident(bytes[index - 1])) {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && ident(bytes[index]) {
            index += 1;
        }
        let name = &text[start..index];
        let mut cursor = index;
        if cursor < bytes.len() && matches!(bytes[cursor], b'"' | b'\'') {
            cursor += 1;
        }
        while cursor < bytes.len() && bytes[cursor] == b' ' {
            cursor += 1;
        }
        let Some(&separator) = bytes.get(cursor) else {
            break;
        };
        if separator != b'=' && separator != b':' {
            continue;
        }
        // `scheme://` is not a key-value pair.
        if separator == b':' && text[cursor..].starts_with("://") {
            continue;
        }
        if !sensitive_key(name) {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor] == b' ' {
            cursor += 1;
        }
        let value_start = cursor;
        let value_end = match bytes.get(cursor) {
            Some(b'$') if text[cursor..].starts_with("${") => text[cursor..]
                .find('}')
                .map_or(text.len(), |i| cursor + i + 1),
            Some(&quote @ (b'"' | b'\'')) => text[cursor + 1..]
                .find(quote as char)
                .map_or(text.len(), |i| cursor + 1 + i + 1),
            Some(_) => {
                let mut end = cursor;
                while end < bytes.len()
                    && !bytes[end].is_ascii_whitespace()
                    && !matches!(
                        bytes[end],
                        b',' | b';' | b'&' | b'}' | b']' | b')' | b'"' | b'\'' | b'`'
                    )
                {
                    end += 1;
                }
                // `Authorization: Bearer <token>`.
                let word = text[cursor..end].to_ascii_lowercase();
                if matches!(word.as_str(), "bearer" | "basic" | "token")
                    && bytes.get(end) == Some(&b' ')
                {
                    let mut next = end + 1;
                    while next < bytes.len() && !bytes[next].is_ascii_whitespace() {
                        next += 1;
                    }
                    push(ranges, text, end + 1..next);
                    index = next;
                    continue;
                }
                end
            }
            None => break,
        };
        push(ranges, text, unquoted(text, value_start..value_end));
        index = value_end.max(index);
    }
}

/// `--flag=value`, `--flag value`, (MySQL) `-pvalue` and (redis-cli)
/// `-a value`.
fn flag_values(text: &str, mysql: bool, ranges: &mut Vec<Range<usize>>) {
    let words = tokens(text);
    let mysql = mysql || words.iter().any(|w| is_mysql_program(&text[w.clone()]));
    let redis = words.iter().any(|w| is_redis_cli(&text[w.clone()]));
    for (index, word) in words.iter().enumerate() {
        let token = &text[word.clone()];
        if redis && token == "-a" {
            if let Some(next) = words.get(index + 1) {
                push(ranges, text, unquoted(text, next.clone()));
            }
        } else if let Some(flag) = token.strip_prefix("--") {
            match flag.split_once('=') {
                Some((name, _)) if secret_flag(name) => {
                    let value = word.start + 2 + name.len() + 1..word.end;
                    push(ranges, text, unquoted(text, value));
                }
                Some(_) => {}
                None if secret_flag(flag) => {
                    if let Some(next) = words.get(index + 1) {
                        if !text[next.clone()].starts_with('-') {
                            push(ranges, text, unquoted(text, next.clone()));
                        }
                    }
                }
                None => {}
            }
        } else if mysql && token.len() > 2 && token.starts_with("-p") {
            push(ranges, text, unquoted(text, word.start + 2..word.end));
        } else {
            // A quoted script (`sh -c 'app --password x'`).
            let inner = unquoted(text, word.clone());
            if inner != *word && text[inner.clone()].contains(' ') {
                let mut nested = Vec::new();
                flag_values(&text[inner.clone()], mysql, &mut nested);
                ranges.extend(
                    nested
                        .into_iter()
                        .map(|r| inner.start + r.start..inner.start + r.end),
                );
            }
        }
    }
}

fn boundary(bytes: &[u8], index: usize) -> bool {
    index == 0
        || !(bytes[index - 1].is_ascii_alphanumeric() || matches!(bytes[index - 1], b'_' | b'-'))
}

fn run(bytes: &[u8], from: usize, allowed: impl Fn(u8) -> bool) -> usize {
    let mut end = from;
    while end < bytes.len() && allowed(bytes[end]) {
        end += 1;
    }
    end
}

/// Known token formats anywhere in the text.
fn token_formats(text: &str, ranges: &mut Vec<Range<usize>>) {
    let bytes = text.as_bytes();
    let alnum = |b: u8| b.is_ascii_alphanumeric();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let dashed = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    // (prefix, body characters, shortest body).
    type Body = fn(u8) -> bool;
    let formats: [(&str, Body, usize); 16] = [
        (
            "github_pat_",
            |b| b.is_ascii_alphanumeric() || b == b'_',
            20,
        ),
        ("ghp_", |b| b.is_ascii_alphanumeric(), 20),
        ("gho_", |b| b.is_ascii_alphanumeric(), 20),
        ("ghu_", |b| b.is_ascii_alphanumeric(), 20),
        ("ghs_", |b| b.is_ascii_alphanumeric(), 20),
        ("ghr_", |b| b.is_ascii_alphanumeric(), 20),
        (
            "glpat-",
            |b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-',
            20,
        ),
        ("xoxa-", |b| b.is_ascii_alphanumeric() || b == b'-', 10),
        ("xoxb-", |b| b.is_ascii_alphanumeric() || b == b'-', 10),
        ("xoxp-", |b| b.is_ascii_alphanumeric() || b == b'-', 10),
        ("xoxr-", |b| b.is_ascii_alphanumeric() || b == b'-', 10),
        ("xoxs-", |b| b.is_ascii_alphanumeric() || b == b'-', 10),
        ("sk_live_", |b| b.is_ascii_alphanumeric(), 10),
        ("sk_test_", |b| b.is_ascii_alphanumeric(), 10),
        ("rk_live_", |b| b.is_ascii_alphanumeric(), 10),
        ("rk_test_", |b| b.is_ascii_alphanumeric(), 10),
    ];
    let mut index = 0;
    while index < bytes.len() {
        if !boundary(bytes, index) || !bytes[index].is_ascii_alphabetic() {
            index += 1;
            continue;
        }
        let rest = &text[index..];
        let mut matched = None;
        for (prefix, body, min) in &formats {
            if rest.starts_with(prefix) {
                let end = run(bytes, index + prefix.len(), body);
                if end - index - prefix.len() >= *min {
                    matched = Some(end);
                }
                break;
            }
        }
        if matched.is_none() && rest.starts_with("sk-") {
            // OpenAI-style keys: long and mixed (avoids `sk-learn-…` names).
            let end = run(bytes, index + 3, dashed);
            let body = &bytes[index + 3..end];
            if body.len() >= 20
                && body.iter().any(u8::is_ascii_uppercase)
                && body.iter().any(u8::is_ascii_lowercase)
                && body.iter().any(u8::is_ascii_digit)
            {
                matched = Some(end);
            }
        }
        if matched.is_none() && (rest.starts_with("AKIA") || rest.starts_with("ASIA")) {
            let end = run(bytes, index + 4, |b| {
                b.is_ascii_uppercase() || b.is_ascii_digit()
            });
            if end - index - 4 == 16 && !bytes.get(end).copied().is_some_and(alnum) {
                matched = Some(end);
            }
        }
        if matched.is_none() && rest.starts_with("eyJ") {
            // header.payload.signature (base64url).
            let header = run(bytes, index, dashed);
            if header - index >= 10 && bytes.get(header) == Some(&b'.') {
                let payload = run(bytes, header + 1, dashed);
                if payload - header > 10 && bytes.get(payload) == Some(&b'.') {
                    matched = Some(run(bytes, payload + 1, dashed));
                }
            }
        }
        match matched {
            Some(end) => {
                ranges.push(index..end);
                index = end;
            }
            None => index = run(bytes, index, word).max(index + 1),
        }
    }
}

/// Applies `ranges` (overlaps merged) and reports whether any was hidden.
pub(crate) fn apply(text: &str, mut ranges: Vec<Range<usize>>) -> (String, bool) {
    ranges.retain(|range| range.start < range.end && &text[range.clone()] != MASK);
    if ranges.is_empty() {
        return (text.to_owned(), false);
    }
    ranges.sort_by_key(|range| (range.start, std::cmp::Reverse(range.end)));
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for range in ranges {
        if range.end <= cursor {
            continue;
        }
        let start = range.start.max(cursor);
        out.push_str(&text[cursor..start]);
        if start == range.start {
            out.push_str(MASK);
        }
        cursor = range.end;
    }
    out.push_str(&text[cursor..]);
    (out, true)
}

/// Byte ranges of `text` that [`mask_text`] hides.
pub(crate) fn secret_ranges(text: &str, mysql: bool) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    pem_blocks(text, &mut ranges);
    url_passwords(text, &mut ranges);
    named_values(text, &mut ranges);
    flag_values(text, mysql, &mut ranges);
    token_formats(text, &mut ranges);
    ranges
}

/// Masks secrets in free text; `mysql` treats `-p<value>` as a password
/// even without a MySQL program name in the text.
pub fn mask_text(text: &str, mysql: bool) -> (String, bool) {
    apply(text, secret_ranges(text, mysql))
}

/// Masks secrets in free text: `(masked, anything hidden)`.
pub fn mask_secrets(text: &str) -> (String, bool) {
    mask_text(text, false)
}

/// Whether the text holds anything [`mask_secrets`] would hide.
pub fn contains_secret(text: &str) -> bool {
    mask_secrets(text).1
}

/// An argument vector (`Cmd`, `Entrypoint`, `Args`, a healthcheck test):
/// a secret flag hides the next argument, and every argument is masked as
/// text (`sh -c` scripts).
pub fn mask_argv(args: &[String], mysql: bool) -> (Vec<String>, bool) {
    let mysql = mysql || args.iter().any(|arg| is_mysql_program(arg));
    let redis = args.iter().any(|arg| is_redis_cli(arg));
    let mut found = false;
    let mut out: Vec<String> = Vec::with_capacity(args.len());
    let mut hide_next = false;
    for arg in args {
        if hide_next && !arg.starts_with('-') && maskable(arg) {
            out.push(MASK.to_owned());
            found = true;
            hide_next = false;
            continue;
        }
        hide_next = (arg.starts_with("--") && !arg.contains('=') && secret_flag(arg))
            || (redis && arg == "-a");
        let (masked, hidden) = mask_text(arg, mysql);
        found |= hidden;
        out.push(masked);
    }
    (out, found)
}

fn mask_value_argv(value: &mut Value, mysql: bool) -> bool {
    let Some(items) = value.as_array_mut() else {
        return false;
    };
    let args: Vec<String> = items
        .iter()
        .map(|item| item.as_str().unwrap_or_default().to_owned())
        .collect();
    let (masked, found) = mask_argv(&args, mysql);
    if found {
        for (item, text) in items.iter_mut().zip(masked) {
            if item.is_string() {
                *item = Value::String(text);
            }
        }
    }
    found
}

fn mask_strings(value: &mut Value, mysql: bool) -> bool {
    match value {
        Value::String(text) => {
            let (masked, found) = mask_text(text, mysql);
            if found {
                *text = masked;
            }
            found
        }
        Value::Array(items) => items
            .iter_mut()
            .fold(false, |found, item| mask_strings(item, mysql) | found),
        Value::Object(map) => map
            .values_mut()
            .fold(false, |found, item| mask_strings(item, mysql) | found),
        _ => false,
    }
}

/// `docker inspect` of a container with every secret hidden (see the
/// module docs); `true` when something was.
pub fn mask_inspect(inspect: &mut Value) -> bool {
    let mysql = inspect
        .pointer("/Config/Image")
        .and_then(Value::as_str)
        .is_some_and(mysql_image);
    let mut found = false;
    if let Some(env) = inspect
        .pointer_mut("/Config/Env")
        .and_then(Value::as_array_mut)
    {
        for entry in env {
            if let Some(text) = entry.as_str() {
                if let Some((key, value)) = text.split_once('=') {
                    if sensitive_key(key) && maskable(value) {
                        *entry = Value::String(format!("{key}={MASK}"));
                        found = true;
                    }
                }
            }
        }
    }
    if let Some(labels) = inspect
        .pointer_mut("/Config/Labels")
        .and_then(Value::as_object_mut)
    {
        for (key, value) in labels.iter_mut() {
            if sensitive_key(key) && value.as_str().is_some_and(maskable) {
                *value = Value::String(MASK.to_owned());
                found = true;
            }
        }
    }
    for pointer in [
        "/Config/Cmd",
        "/Config/Entrypoint",
        "/Args",
        "/Config/Healthcheck/Test",
    ] {
        if let Some(argv) = inspect.pointer_mut(pointer) {
            found |= mask_value_argv(argv, mysql);
        }
    }
    found | mask_strings(inspect, mysql)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// (input, expected); unchanged inputs are the false positives that
    /// must stay visible.
    const CASES: &[(&str, &str)] = &[
        // Names.
        ("POSTGRES_PASSWORD=hunter2", "POSTGRES_PASSWORD=••••••"),
        ("API_TOKEN=abc123", "API_TOKEN=••••••"),
        ("STRIPE_KEY=rk_abc", "STRIPE_KEY=••••••"),
        ("SENTRY_DSN=https://k@o1.ingest.io/2", "SENTRY_DSN=••••••"),
        ("SESSION_COOKIE=xyz", "SESSION_COOKIE=••••••"),
        ("PASSWORD_SALT=s4lt", "PASSWORD_SALT=••••••"),
        ("DB_PASS=hunter2", "DB_PASS=••••••"),
        (
            "export DB_PASSWORD=\"a b\" && run",
            "export DB_PASSWORD=\"••••••\" && run",
        ),
        ("password: hunter2", "password: ••••••"),
        (
            "{\"password\":\"hunter2\",\"user\":\"bob\"}",
            "{\"password\":\"••••••\",\"user\":\"bob\"}",
        ),
        (
            "GET /login?user=bob&token=abc&x=1",
            "GET /login?user=bob&token=••••••&x=1",
        ),
        (
            "Authorization: Bearer abc.def",
            "Authorization: Bearer ••••••",
        ),
        // URLs.
        (
            "DATABASE_URL=postgres://u:secret@db/x",
            "DATABASE_URL=postgres://u:••••••@db/x",
        ),
        (
            "redis://:secret@cache:6379/0",
            "redis://:••••••@cache:6379/0",
        ),
        (
            "mongodb+srv://admin:p%40ss@cluster0.example.net/app",
            "mongodb+srv://admin:••••••@cluster0.example.net/app",
        ),
        ("amqp://guest:pa/ss@mq:5672", "amqp://guest:••••••@mq:5672"),
        (
            "connect to \"mysql://root:toor@db:3306\" now",
            "connect to \"mysql://root:••••••@db:3306\" now",
        ),
        // Flags.
        (
            "redis-server --requirepass secret",
            "redis-server --requirepass ••••••",
        ),
        (
            "app --password=secret --port 80",
            "app --password=•••••• --port 80",
        ),
        ("app --api-key 'my key' -v", "app --api-key '••••••' -v"),
        ("vault login --token s.abcdef", "vault login --token ••••••"),
        (
            "cli --key k3y --secret s3cr3t",
            "cli --key •••••• --secret ••••••",
        ),
        ("app --db-password=x", "app --db-password=••••••"),
        (
            "mysqladmin ping -h localhost -uroot -psecret",
            "mysqladmin ping -h localhost -uroot -p••••••",
        ),
        (
            "/usr/bin/mysql -p'pa ss' shop",
            "/usr/bin/mysql -p'••••••' shop",
        ),
        // PEM.
        (
            "key: -----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY----- tail",
            "key: •••••• tail",
        ),
        ("-----BEGIN OPENSSH PRIVATE KEY-----\nb3Bl", "••••••"),
        // Token formats.
        (
            "token ghp_abcdefghijklmnopqrstuvwxyz0123456789 used",
            "token •••••• used",
        ),
        ("github_pat_11ABCDEFG0123456789_abcdefghijklmnop", "••••••"),
        ("glpat-abcdefghij0123456789", "••••••"),
        ("slack xoxb-1234-5678-abcdef", "slack ••••••"),
        ("OPENAI=sk-proj-Ab3dEf6hIj9kLm2nOp5q", "OPENAI=••••••"),
        ("aws AKIAIOSFODNN7EXAMPLE id", "aws •••••• id"),
        (
            "jwt=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.sig_abc",
            "jwt=••••••",
        ),
        (
            "bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.c2ln",
            "bearer ••••••",
        ),
        // Not secrets.
        ("PORT=5432", "PORT=5432"),
        ("NODE_ENV=production", "NODE_ENV=production"),
        (
            "PATH=/usr/local/bin:/usr/bin",
            "PATH=/usr/local/bin:/usr/bin",
        ),
        (
            "KEYCLOAK_URL=http://keycloak:8080",
            "KEYCLOAK_URL=http://keycloak:8080",
        ),
        ("MONKEY=banana", "MONKEY=banana"),
        ("POSTGRES_USER=shop", "POSTGRES_USER=shop"),
        (
            "POSTGRES_PASSWORD_FILE=/run/secrets/db",
            "POSTGRES_PASSWORD_FILE=/run/secrets/db",
        ),
        (
            "POSTGRES_PASSWORD=${POSTGRES_PASSWORD}",
            "POSTGRES_PASSWORD=${POSTGRES_PASSWORD}",
        ),
        (
            "DB_URL=postgres://db:5432/shop",
            "DB_URL=postgres://db:5432/shop",
        ),
        (
            "https://user@example.com/path",
            "https://user@example.com/path",
        ),
        (
            "ssh://git@github.com/org/repo.git",
            "ssh://git@github.com/org/repo.git",
        ),
        ("https://example.com/a:b@c", "https://example.com/a:b@c"),
        ("docker run -p 8080:80 nginx", "docker run -p 8080:80 nginx"),
        (
            "app --key-file /etc/ssl/key.pem",
            "app --key-file /etc/ssl/key.pem",
        ),
        ("app --password --verbose", "app --password --verbose"),
        (
            "sk-learn-notebook-container-01",
            "sk-learn-notebook-container-01",
        ),
        (
            "disk-AbcdefghijKLMNOP0123456789",
            "disk-AbcdefghijKLMNOP0123456789",
        ),
        ("AKIA1234", "AKIA1234"),
        (
            "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        ),
        (
            "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----",
            "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----",
        ),
        (
            "docker-entrypoint.sh postgres",
            "docker-entrypoint.sh postgres",
        ),
        ("password: ••••••", "password: ••••••"),
        ("", ""),
    ];

    #[test]
    fn table() {
        assert!(CASES.len() >= 40);
        for (input, expected) in CASES {
            let (masked, found) = mask_secrets(input);
            assert_eq!(&masked, expected, "input: {input:?}");
            assert_eq!(found, input != expected, "found flag for {input:?}");
            // Deterministic and idempotent.
            assert_eq!(mask_secrets(&masked), (masked.clone(), false), "{input:?}");
        }
    }

    #[test]
    fn names() {
        for secret in [
            "password",
            "DB_PASSWD",
            "apiKey",
            "stripeKey",
            "SENTRY_DSN",
            "TLS_CERT",
            "PRIVATE",
            "session_id",
            "Cookie",
            "HMAC_SIGNATURE",
            "JWT",
            "SSH_AUTH_SOCK",
        ] {
            assert!(sensitive_key(secret), "{secret}");
        }
        for plain in [
            "PORT",
            "HOME",
            "NODE_ENV",
            "KEYCLOAK_URL",
            "MONKEY",
            "HOSTNAME",
            "PASSWORD_FILE",
            "TLS_CERT_PATH",
            "com.docker.compose.project.config_files",
            "com.docker.compose.config-hash",
        ] {
            assert!(!sensitive_key(plain), "{plain}");
        }
    }

    #[test]
    fn mysql_password_flag_needs_mysql() {
        assert_eq!(mask_text("server -psecret", false).0, "server -psecret");
        assert_eq!(mask_text("server -psecret", true).0, "server -p••••••");
        assert!(mysql_image("mariadb:11"));
        assert!(mysql_image("percona/percona-server:8"));
        assert!(!mysql_image("postgres:16"));
    }

    #[test]
    fn argv_hides_the_value_after_a_flag() {
        let argv = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            mask_argv(
                &argv(&["redis-server", "--requirepass", "secret", "--port", "6379"]),
                false
            ),
            (
                argv(&["redis-server", "--requirepass", MASK, "--port", "6379"]),
                true
            )
        );
        assert_eq!(
            mask_argv(&argv(&["mysqld", "-psecret"]), false),
            (argv(&["mysqld", "-p••••••"]), true)
        );
        assert_eq!(
            mask_argv(
                &argv(&["sh", "-c", "redis-server --requirepass $REDIS_PASSWORD"]),
                false
            ),
            (
                argv(&["sh", "-c", "redis-server --requirepass $REDIS_PASSWORD"]),
                false
            )
        );
        assert_eq!(
            mask_argv(&argv(&["nginx", "-g", "daemon off;"]), false),
            (argv(&["nginx", "-g", "daemon off;"]), false)
        );
    }

    #[test]
    fn inspect_hides_env_command_labels_and_urls() {
        let mut inspect = json!({
            "Id": "3f2a9c1d0e",
            "Path": "redis-server",
            "Args": ["--requirepass", "secret", "--appendonly", "yes"],
            "Config": {
                "Image": "redis:7",
                "Env": [
                    "POSTGRES_PASSWORD=hunter2",
                    "DATABASE_URL=postgres://u:secret@db/x",
                    "PATH=/usr/bin",
                    "API_TOKEN=",
                    "AWS_SECRET_ACCESS_KEY=x"
                ],
                "Cmd": ["redis-server", "--requirepass", "secret"],
                "Entrypoint": ["docker-entrypoint.sh"],
                "Labels": {"token": "secret", "com.docker.compose.project": "shop"},
                "Healthcheck": {"Test": ["CMD-SHELL", "redis-cli -a secret ping || exit 1"]}
            },
            "HostConfig": {"LogConfig": {"Config": {"loki-url": "https://u:secret@logs/api"}}}
        });
        assert!(mask_inspect(&mut inspect));
        let text = inspect.to_string();
        assert!(
            !text.contains("secret") && !text.contains("hunter2"),
            "{inspect:#}"
        );
        assert_eq!(
            inspect["Config"]["Env"],
            json!([
                "POSTGRES_PASSWORD=••••••",
                "DATABASE_URL=postgres://u:••••••@db/x",
                "PATH=/usr/bin",
                "API_TOKEN=",
                "AWS_SECRET_ACCESS_KEY=••••••"
            ])
        );
        assert_eq!(
            inspect["Args"],
            json!(["--requirepass", MASK, "--appendonly", "yes"])
        );
        assert_eq!(
            inspect["Config"]["Cmd"],
            json!(["redis-server", "--requirepass", MASK])
        );
        assert_eq!(inspect["Config"]["Labels"]["token"], MASK);
        assert_eq!(
            inspect["Config"]["Labels"]["com.docker.compose.project"],
            "shop"
        );
        assert_eq!(inspect["Id"], "3f2a9c1d0e");
        assert_eq!(
            inspect["Config"]["Healthcheck"]["Test"],
            json!(["CMD-SHELL", "redis-cli -a •••••• ping || exit 1"])
        );
        let mut plain = json!({"Config": {"Env": ["PORT=80"], "Cmd": ["nginx"]}});
        assert!(!mask_inspect(&mut plain));
    }
}
