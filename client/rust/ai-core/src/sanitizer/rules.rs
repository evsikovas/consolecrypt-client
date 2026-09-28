//! Detection rules for the sanitizer.
//!
//! Each detector appends [`Finding`]s (byte ranges of the *value* to replace,
//! keys and flag names are kept so the model still understands the command).
//! Overlaps are resolved afterwards by category priority, so detectors may be
//! generous.

use super::{Category, Detect, Finding};
use regex::Regex;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::ops::Range;
use std::sync::LazyLock;
use zeroize::Zeroizing;

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> =
            LazyLock::new(|| Regex::new($pat).expect("sanitizer regex must compile"));
    };
}

fn push(out: &mut Vec<Finding>, r: Range<usize>, category: Category) {
    if r.end > r.start {
        out.push(Finding {
            start: r.start,
            end: r.end,
            category,
        });
    }
}

/// Run every enabled detector.
pub(super) fn collect(text: &str, d: Detect, out: &mut Vec<Finding>) {
    // Always (all profiles).
    pem_blocks(text, out);
    base64_lines(text, out);
    urls(text, d, out);
    header_values(text, out);
    known_tokens(text, out);
    key_values(text, d, out);
    xml_values(text, d, out);
    cli_flags(text, d, out);
    tool_specific(text, out);
    // Standard + Strict.
    if d.entropy {
        high_entropy(text, out);
    }
    // Strict only.
    if d.strict {
        ipv4(text, out);
        ipv6(text, out);
        user_at_host(text, out);
        fqdns(text, out);
        home_dirs(text, out);
        ssh_config(text, out);
        sql_databases(text, out);
        positional_hosts(text, out);
    }
}

// ---------------------------------------------------------------------------
// Private keys, PEM blocks, key bodies
// ---------------------------------------------------------------------------

re!(
    PEM_PRIVATE,
    r"(?s)-{4,5}\s?BEGIN[A-Z0-9 ]*PRIVATE KEY[A-Z ]*-{4,5}.*?(?:-{4,5}\s?END[A-Z0-9 ]*PRIVATE KEY[A-Z ]*-{4,5}|\z)"
);
re!(
    PEM_ANY,
    r"(?s)-{5}BEGIN [A-Z0-9 ]+-{5}.*?(?:-{5}END [A-Z0-9 ]+-{5}|\z)"
);
re!(
    PUTTY,
    r"(?s)(?:PuTTY-User-Key-File-\d+:[^\n]*\n(?:[^\n]*\n)*?)?Private-Lines:\s*\d+.*?(?:Private-MAC:\s*[0-9a-fA-F]+|\z)"
);
re!(B64_LINE, r"(?m)^[ \t]*([A-Za-z0-9+/]{60,}={0,2})[ \t\r]*$");

fn pem_blocks(text: &str, out: &mut Vec<Finding>) {
    if !text.contains("-----") && !text.contains("---- ") && !text.contains("Private-Lines") {
        return;
    }
    for m in PEM_PRIVATE.find_iter(text) {
        push(out, m.range(), Category::PrivateKey);
    }
    for m in PEM_ANY.find_iter(text) {
        push(out, m.range(), Category::Pem);
    }
    for m in PUTTY.find_iter(text) {
        push(out, m.range(), Category::PrivateKey);
    }
}

/// Lines consisting solely of long base64 (key/cert bodies without headers,
/// e.g. a partial terminal selection of a key).
fn base64_lines(text: &str, out: &mut Vec<Finding>) {
    for c in B64_LINE.captures_iter(text) {
        if let Some(m) = c.get(1) {
            push(out, m.range(), Category::Secret);
        }
    }
}

// ---------------------------------------------------------------------------
// URLs (connection strings with credentials)
// ---------------------------------------------------------------------------

re!(
    URL,
    r#"(?i)\b(?P<scheme>[a-z][a-z0-9+.\-]*)://(?:(?P<user>[^\s:/@'"`<>\\]*)(?::(?P<pass>[^\s'"`<>\\]*))?@)?(?P<host>\[[0-9a-f:.]+\]|[^\s'"`<>/@?#:\\\]\)]+)(?::(?P<port>\d{1,5}))?(?P<path>/[^\s'"`<>?#\\\)]*)?"#
);

const DB_SCHEMES: &[&str] = &[
    "postgres",
    "postgresql",
    "mysql",
    "mariadb",
    "mongodb",
    "mongodb+srv",
    "mssql",
    "sqlserver",
    "cockroachdb",
    "clickhouse",
    "tidb",
    "oracle",
    "db2",
    "cassandra",
];

fn urls(text: &str, d: Detect, out: &mut Vec<Finding>) {
    if !text.contains("://") {
        return;
    }
    for c in URL.captures_iter(text) {
        if let Some(p) = c.name("pass") {
            if is_real_value(p.as_str()) {
                push(out, p.range(), Category::Password);
            }
        }
        if !d.strict {
            continue;
        }
        if let Some(u) = c.name("user") {
            let v = u.as_str();
            if !v.is_empty() && is_real_value(v) && !is_generic_user(v) {
                push(out, u.range(), Category::User);
            }
        }
        let host_is_userinfo = c.name("port").is_none()
            && c.name("host")
                .is_some_and(|h| text[h.end()..].starts_with(':'));
        if let Some(h) = c.name("host").filter(|_| !host_is_userinfo) {
            let raw = h.as_str();
            let inner = raw.trim_start_matches('[').trim_end_matches(']');
            if let Some(cat) = classify_host(inner) {
                let off = h.start() + (raw.len() - raw.trim_start_matches('[').len());
                push(out, off..off + inner.len(), cat);
            }
        }
        let scheme = c
            .name("scheme")
            .map(|s| s.as_str().to_ascii_lowercase())
            .unwrap_or_default();
        let scheme = scheme.rsplit(':').next().unwrap_or(&scheme).to_owned();
        if DB_SCHEMES.contains(&scheme.as_str()) {
            if let Some(p) = c.name("path") {
                let seg = p.as_str()[1..].split('/').next().unwrap_or("");
                if !seg.is_empty() && !is_system_db(seg) {
                    let s = p.start() + 1;
                    push(out, s..s + seg.len(), Category::Database);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Authorization / Cookie headers
// ---------------------------------------------------------------------------

re!(
    AUTH_HEADER,
    r#"(?i)\b(?:proxy-)?(?:authorization|cookie|set-cookie)["']?[ \t]*[:=][ \t]*["']?"#
);
re!(
    AUTH_SCHEME,
    r"(?i)^(?:bearer|basic|token|digest|negotiate|ntlm|apikey|api-key|aws4-hmac-sha256|hmac|sso-key|ssws|splunk|key)[ \t]+"
);
re!(BEARER, r"(?i)\bbearer[ \t]+([A-Za-z0-9\-._~+/]{8,}=*)");

fn header_values(text: &str, out: &mut Vec<Finding>) {
    for m in AUTH_HEADER.find_iter(text) {
        let matched = m.as_str();
        // Closing quote: the one we matched after the separator, else the
        // one right before the header name (`-H 'Authorization: …'`).
        let close = matched
            .chars()
            .last()
            .filter(|c| *c == '"' || *c == '\'')
            .or_else(|| {
                text[..m.start()]
                    .chars()
                    .next_back()
                    .filter(|c| *c == '"' || *c == '\'')
            });
        let mut start = m.end();
        if let Some(s) = AUTH_SCHEME.find(&text[start..]) {
            start += s.end();
        }
        let line_end = line_end(text, start);
        let end = match close {
            Some(q) => text[start..line_end]
                .find(q)
                .map_or(line_end, |i| start + i),
            None => line_end,
        };
        let value = text[start..end].trim_end();
        if is_real_value(value) {
            push(out, start..start + value.len(), Category::Secret);
        }
    }
    for c in BEARER.captures_iter(text) {
        if let Some(v) = c.get(1) {
            if is_real_value(v.as_str()) {
                push(out, v.range(), Category::Secret);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Known token formats
// ---------------------------------------------------------------------------

static KNOWN_TOKENS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        // OpenAI / Anthropic / DeepSeek / OpenRouter style
        r"\bsk-[A-Za-z0-9][A-Za-z0-9_\-]{19,}",
        // Stripe
        r"\b(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{16,}",
        // GitHub
        r"\bgh[pousr]_[A-Za-z0-9]{30,}",
        r"\bgithub_pat_[A-Za-z0-9_]{30,}",
        // Slack
        r"\bxox[abposre]-[A-Za-z0-9\-]{10,}",
        r"\bxapp-\d-[A-Za-z0-9\-]{10,}",
        r"https://hooks\.slack\.com/(?:services|workflows|triggers)/[A-Za-z0-9/_\-]+",
        r"https://(?:ptb\.|canary\.)?discord(?:app)?\.com/api/webhooks/\d+/[A-Za-z0-9_\-]+",
        // GitLab
        r"\bgl(?:pat|dt|rt|ptt|cbt|oas|imt|soat|ft|agent|ffct|wt)-[A-Za-z0-9_\-]{20,}",
        // Google
        r"\bAIza[0-9A-Za-z_\-]{35}",
        r"\bya29\.[0-9A-Za-z_\-]{20,}",
        r"\bGOCSPX-[0-9A-Za-z_\-]{20,}",
        // AWS access key ids
        r"\b(?:AKIA|ASIA|ABIA|ACCA|AGPA|AIDA|AIPA|ANPA|ANVA|AROA|APKA)[A-Z0-9]{16}\b",
        // Hugging Face, npm, PyPI, Docker Hub, DigitalOcean
        r"\bhf_[A-Za-z0-9]{30,}",
        r"\bnpm_[A-Za-z0-9]{36}\b",
        r"\bpypi-[A-Za-z0-9_\-]{50,}",
        r"\bdckr_pat_[A-Za-z0-9_\-]{20,}",
        r"\bdo[por]_v1_[a-f0-9]{64}\b",
        // SendGrid, Mailgun, Twilio
        r"\bSG\.[A-Za-z0-9_\-]{16,}\.[A-Za-z0-9_\-]{16,}",
        r"\bkey-[0-9a-f]{32}\b",
        r"\bSK[0-9a-f]{32}\b",
        // HashiCorp Vault / Terraform Cloud
        r"\bhv[sbr]\.[A-Za-z0-9_\-]{20,}",
        r"\b[A-Za-z0-9]{14}\.atlasv1\.[A-Za-z0-9_\-]{60,}",
        // Telegram bot token
        r"\d{8,10}:AA[A-Za-z0-9_\-]{33}\b",
        // Shopify, Atlassian, Linear, Postman, Grafana, Tailscale, age, Doppler
        r"\bshp(?:at|ss|ca|pa)_[a-fA-F0-9]{32}\b",
        r"\bATATT3[A-Za-z0-9_\-=]{50,}",
        r"\blin_api_[A-Za-z0-9]{40}\b",
        r"\bPMAK-[a-f0-9]{24}-[a-f0-9]{34}\b",
        r"\bglsa_[A-Za-z0-9_]{32,}",
        r"\bglc_[A-Za-z0-9+/=_\-]{30,}",
        r"\btskey-[a-z]+-[A-Za-z0-9\-]{20,}",
        r"\bAGE-SECRET-KEY-1[0-9A-Z]{58}\b",
        r"\bdp\.(?:pt|st|sa|ct)\.[A-Za-z0-9]{40,}",
        // JWT (header.payload.signature)
        r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]*",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("token regex must compile"))
    .collect()
});

fn known_tokens(text: &str, out: &mut Vec<Finding>) {
    for re in KNOWN_TOKENS.iter() {
        for m in re.find_iter(text) {
            push(out, m.range(), Category::Secret);
        }
    }
}

// ---------------------------------------------------------------------------
// key=value / key: value (env, YAML, INI, JSON, query strings, conn strings)
// ---------------------------------------------------------------------------

re!(
    KV,
    r#"(?P<key>[A-Za-z_][A-Za-z0-9_.\-]*)(?P<kq>\\?["']?)[ \t]*(?P<sep>:=|=>|=|:)[ \t]*"#
);
re!(
    ADO_KEYS,
    r"(?i)\b(?P<key>user\s+id|data\s+source|initial\s+catalog)\s*=\s*"
);

fn key_values(text: &str, d: Detect, out: &mut Vec<Finding>) {
    let mut pos = 0;
    while pos < text.len() {
        let Some(c) = KV.captures_at(text, pos) else {
            break;
        };
        let (Some(m), Some(key), Some(sep)) = (c.get(0), c.name("key"), c.name("sep")) else {
            break;
        };
        let next = m.end().max(pos + 1);
        // `scheme://` is a URL, not a key.
        if sep.as_str() == ":" && text[m.end()..].starts_with("//") {
            pos = next;
            continue;
        }
        // Must not start in the middle of a word.
        if text[..key.start()]
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
        {
            pos = next;
            continue;
        }
        let lower_key = key.as_str().to_ascii_lowercase();
        if matches!(
            lower_key.as_str(),
            "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
        ) {
            pos = next;
            continue;
        }
        let secret_cat = classify_secret_key(key.as_str());
        let strict_cat = if d.strict {
            classify_strict_key(key.as_str())
        } else {
            None
        };
        let Some(cat) = secret_cat.or(strict_cat) else {
            pos = next;
            continue;
        };
        let spaced =
            text[..sep.start()].ends_with([' ', '\t']) || m.as_str().ends_with([' ', '\t']);
        let line_mode = is_line_start(text, key.start())
            && (sep.as_str() == ":" || (sep.as_str() == "=" && spaced));
        let mode = if line_mode {
            Mode::LineRest
        } else {
            Mode::Token
        };
        let Some(r) = extract_value(text, m.end(), mode) else {
            pos = next;
            continue;
        };
        let value = &text[r.clone()];
        if line_mode && matches!(value.trim(), "|" | "|-" | "|+" | ">" | ">-" | ">+") {
            if cat.is_secret() {
                if let Some(block) = yaml_block(text, key.start(), r.end) {
                    push(out, block, cat);
                }
            }
            pos = r.end.max(next);
            continue;
        }
        if is_real_value(value) && value_fits(cat, key.as_str(), value) {
            match cat {
                Category::Host => {
                    if let Some(hc) = classify_host(value.trim()) {
                        push(out, r.clone(), hc);
                    }
                }
                _ => push(out, r.clone(), cat),
            }
        }
        pos = r.end.max(next);
    }
    if d.strict {
        for c in ADO_KEYS.captures_iter(text) {
            let (Some(m), Some(k)) = (c.get(0), c.name("key")) else {
                continue;
            };
            let k = k.as_str().to_ascii_lowercase();
            let cat = if k.starts_with("user") {
                Category::User
            } else if k.starts_with("data") {
                Category::Host
            } else {
                Category::Database
            };
            if let Some(r) = extract_value(text, m.end(), Mode::Token) {
                if is_real_value(&text[r.clone()]) {
                    push(out, r, cat);
                }
            }
        }
    }
}

/// Indented block following a YAML `key: |` line.
fn yaml_block(text: &str, key_start: usize, after: usize) -> Option<Range<usize>> {
    let line_start = text[..key_start].rfind('\n').map_or(0, |i| i + 1);
    let indent = key_start - line_start;
    let mut end = after;
    let mut start = None;
    let mut cursor = text[after..].find('\n').map(|i| after + i + 1)?;
    while cursor < text.len() {
        let le = line_end(text, cursor);
        let line = &text[cursor..le];
        let ind = line.len() - line.trim_start().len();
        if !line.trim().is_empty() && ind <= indent {
            break;
        }
        if !line.trim().is_empty() {
            start.get_or_insert(cursor + ind);
            end = le;
        }
        cursor = le + 1;
    }
    start.map(|s| s..end)
}

re!(
    XML,
    r"<(?P<tag>[A-Za-z_][A-Za-z0-9_.:\-]*)>(?P<val>[^<\n]{1,512})</(?P<close>[A-Za-z_][A-Za-z0-9_.:\-]*)>"
);

fn xml_values(text: &str, d: Detect, out: &mut Vec<Finding>) {
    if !text.contains("</") {
        return;
    }
    for c in XML.captures_iter(text) {
        let (Some(tag), Some(val), Some(close)) = (c.name("tag"), c.name("val"), c.name("close"))
        else {
            continue;
        };
        if tag.as_str() != close.as_str() {
            continue;
        }
        let local = tag.as_str().rsplit(':').next().unwrap_or(tag.as_str());
        let cat = classify_secret_key(local).or(if d.strict {
            classify_strict_key(local)
        } else {
            None
        });
        if let Some(cat) = cat {
            if is_real_value(val.as_str()) {
                let cat = if cat == Category::Host {
                    match classify_host(val.as_str().trim()) {
                        Some(h) => h,
                        None => continue,
                    }
                } else {
                    cat
                };
                push(out, val.range(), cat);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CLI flags with a separate value (`--password x`, `-Token x`)
// ---------------------------------------------------------------------------

re!(
    FLAG,
    r"(?:^|[\s;|&(])(?P<flag>--?[A-Za-z][A-Za-z0-9_\-]*)[ \t]+"
);

/// Tools where `-h` means host and `-d`/`-D` means database.
const DB_TOOLS_H: &[&str] = &[
    "psql",
    "pg_dump",
    "pg_dumpall",
    "pg_restore",
    "pg_isready",
    "pg_basebackup",
    "createdb",
    "dropdb",
    "vacuumdb",
    "reindexdb",
    "pgbench",
    "mysql",
    "mysqldump",
    "mysqladmin",
    "mysqlimport",
    "mysqlcheck",
    "mysqlshow",
    "mariadb",
    "mariadb-dump",
    "redis-cli",
    "clickhouse-client",
];
const DB_TOOLS_LOWER_D: &[&str] = &[
    "psql",
    "pg_dump",
    "pg_restore",
    "vacuumdb",
    "reindexdb",
    "pgbench",
    "clickhouse-client",
    "mongodump",
    "mongorestore",
    "mongoexport",
    "mongoimport",
];
const DB_TOOLS_UPPER_D: &[&str] = &[
    "mysql",
    "mysqldump",
    "mariadb",
    "mariadb-dump",
    "mysqlcheck",
];

fn cli_flags(text: &str, d: Detect, out: &mut Vec<Finding>) {
    let mut pos = 0;
    while pos < text.len() {
        let Some(c) = FLAG.captures_at(text, pos) else {
            break;
        };
        let (Some(m), Some(flag)) = (c.get(0), c.name("flag")) else {
            break;
        };
        pos = flag.end();
        let Some(r) = extract_value(text, m.end(), Mode::Token) else {
            continue;
        };
        let value = &text[r.clone()];
        if value.starts_with('-') || !is_real_value(value) {
            continue;
        }
        let raw = flag.as_str();
        let name = raw.trim_start_matches('-');
        if name.len() >= 2 {
            if let Some(cat) = classify_secret_key(name) {
                if value_fits(cat, name, value) {
                    push(out, r.clone(), cat);
                }
                continue;
            }
        }
        if !d.strict {
            continue;
        }
        let cmd = command_before(text, flag.start());
        let cmd = cmd.as_deref().unwrap_or("");
        let lname = name.to_ascii_lowercase();
        let cat = match (raw, lname.as_str()) {
            (_, "user" | "username" | "login" | "login-name" | "dbuser" | "db-user") => {
                Some(Category::User)
            }
            ("-u" | "-U", _) => Some(Category::User),
            ("-l", _) if matches!(cmd, "ssh" | "slogin" | "mosh" | "rsh") => Some(Category::User),
            (_, "host" | "hostname" | "server" | "db-host" | "dbhost") => Some(Category::Host),
            ("-h", _) if DB_TOOLS_H.contains(&cmd) => Some(Category::Host),
            ("-S", _) if matches!(cmd, "sqlcmd" | "bcp" | "osql") => Some(Category::Host),
            ("-J", _) if cmd == "ssh" => Some(Category::Host),
            (_, "dbname" | "database" | "db" | "keyspace") => Some(Category::Database),
            ("-d", _) if DB_TOOLS_LOWER_D.contains(&cmd) => Some(Category::Database),
            ("-D", _) if DB_TOOLS_UPPER_D.contains(&cmd) => Some(Category::Database),
            ("-k", _) if cmd == "cqlsh" => Some(Category::Database),
            _ => None,
        };
        match cat {
            Some(Category::User) => {
                // `user:pass` → user part only (password handled elsewhere).
                let user = value.split(':').next().unwrap_or(value);
                let user = user.split('%').next().unwrap_or(user);
                if !user.is_empty()
                    && !is_generic_user(user)
                    && !user.chars().all(|ch| ch.is_ascii_digit())
                {
                    push(out, r.start..r.start + user.len(), Category::User);
                }
            }
            Some(Category::Host) => {
                let mut off = r.start;
                for part in value.split(',') {
                    let host_part = part.rsplit('@').next().unwrap_or(part);
                    let skip = part.len() - host_part.len();
                    let host_only = host_part.split(':').next().unwrap_or(host_part);
                    if let Some(hc) = classify_host(host_only) {
                        push(out, off + skip..off + skip + host_only.len(), hc);
                    }
                    off += part.len() + 1;
                }
            }
            Some(Category::Database)
                if !is_system_db(value) && !value.chars().all(|ch| ch.is_ascii_digit()) =>
            {
                push(out, r, Category::Database);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Tool-specific password positions
// ---------------------------------------------------------------------------

const VAL: &str = r#"(?P<val>"[^"\n]*"|'[^'\n]*'|[^\s'"|;&]+)"#;

/// Post-filter for a tool rule match.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    None,
    /// `-u 1000:1000` (uid:gid) is not `user:password`.
    NotNumeric,
    /// `-b cookies.txt` is a cookie-jar file, not a cookie.
    CookieValue,
}

static TOOL_RULES: LazyLock<Vec<(Regex, Category, Filter)>> = LazyLock::new(|| {
    let rules: Vec<(String, Category)> = vec![
        // sshpass -p secret / -psecret
        (format!(r"\bsshpass\s+(?:-[a-oq-z]\s+\S+\s+)*-p\s*{VAL}"), Category::Password),
        // mysql family: attached -pSECRET (a separate word after -p is a DB name)
        (
            format!(
                r"\b(?:mysql|mysqldump|mysqladmin|mysqlimport|mysqlcheck|mysqlshow|mysqlpump|mysqlsh|mariadb|mariadb-dump|mariadb-admin)\b[^\n|;&]*?\s-p{VAL}"
            ),
            Category::Password,
        ),
        // redis-cli -a secret / --pass secret / AUTH secret
        (
            format!(r"\bredis-cli\b[^\n|;&]*?\s(?:-a|--pass|--user\s+\S+\s+--pass)\s+{VAL}"),
            Category::Password,
        ),
        (
            format!(r"(?im)(?:^|redis-cli\b[^\n]*?\s|>\s*)auth\s+(?:[A-Za-z0-9_\-]+\s+)?{VAL}\s*$"),
            Category::Password,
        ),
        // mongo tools -p secret
        (
            format!(
                r"\b(?:mongo|mongosh|mongodump|mongorestore|mongoexport|mongoimport|mongostat|mongotop)\b[^\n|;&]*?\s-p\s+{VAL}"
            ),
            Category::Password,
        ),
        // sqlcmd / bcp -P secret
        (
            format!(r"(?i)\b(?:sqlcmd|bcp|osql)\b[^\n|;&]*?\s-P\s*{VAL}"),
            Category::Password,
        ),
        // docker/podman/helm/az login -p secret
        (
            format!(
                r"\b(?:docker|podman|nerdctl|buildah|skopeo|helm\s+registry|oras|az|cf)\s+login\b[^\n|;&]*?\s-p\s+{VAL}"
            ),
            Category::Password,
        ),
        // curl/wget -u user:secret, --user, --proxy-user, -U
        (
            format!(r#"(?:^|\s)(?:-u|--user|-U|--proxy-user)[ \t=]*['"]?[^\s:'"@]+:{VAL}"#),
            Category::Password,
        ),
        // smbclient -U user%secret
        (format!(r#"(?:^|\s)-U\s*['"]?[^\s%'"]+%{VAL}"#), Category::Password),
        // echo secret | sudo -S / --password-stdin / passwd --stdin / chpasswd
        (
            format!(
                r"\b(?:echo|printf)\s+(?:-[neE]+\s+)?(?:'%s\\?n?'\s+)?{VAL}\s*\|\s*(?:sudo\s+(?:-\S+\s+)*-S|[^\n|]*--password-stdin|passwd\s+--stdin|chpasswd|[^\n|]*\s--stdin\b|sudo\s+-S)"
            ),
            Category::Password,
        ),
        (
            format!(r"(?:sudo\s+-S|--password-stdin|passwd\s+--stdin|chpasswd)[^\n]*?<<<\s*{VAL}"),
            Category::Password,
        ),
        // openssl -pass pass:secret / -passin / -passout / enc -k secret
        (
            format!(r"(?i)-pass(?:in|out|word)?\s+(?:pass|env):{VAL}"),
            Category::Password,
        ),
        (format!(r"\bopenssl\b[^\n|;&]*?\s-k\s+{VAL}"), Category::Password),
        // useradd/usermod -p HASH
        (
            format!(r"\b(?:useradd|usermod)\b[^\n|;&]*?\s-p\s+{VAL}"),
            Category::Password,
        ),
        // cmd: net user bob secret /add ; cmdkey /pass:secret
        (
            r"(?i)\bnet\s+user\s+[^\s/]+\s+(?P<val>[^\s/*][^\s]*)".to_owned(),
            Category::Password,
        ),
        (
            r"(?i)(?:^|\s)/pass(?:word)?:(?P<val>[^\s]+)".to_owned(),
            Category::Password,
        ),
        // PowerShell: ConvertTo-SecureString 'secret' -AsPlainText
        (
            format!(r"(?i)ConvertTo-SecureString\s+(?:-String\s+)?{VAL}"),
            Category::Password,
        ),
        // .netrc: machine x login y password z
        (
            r"(?i)\blogin\s+\S+\s+password\s+(?P<val>\S+)".to_owned(),
            Category::Password,
        ),
        // .pgpass: host:port:db:user:password
        (
            r"(?m)^(?:[^:\s#]+|\*):(?:\d+|\*):(?:[^:\s]+|\*):(?:[^:\s]+|\*):(?P<val>\S+)$".to_owned(),
            Category::Password,
        ),
        // vault login <token>
        (
            r"\bvault\s+login\s+(?:-\S+\s+)*(?P<val>[^\s=\-][^\s=]*)(?:\s|$)".to_owned(),
            Category::Secret,
        ),
        // aws configure set aws_secret_access_key VALUE
        (
            r"\baws\s+configure\s+set\s+(?:aws_secret_access_key|aws_session_token|secret_access_key)\s+(?P<val>\S+)"
                .to_owned(),
            Category::Secret,
        ),
        // curl -b/--cookie 'k=v'
        (
            format!(r"(?:^|\s)(?:-b|--cookie)\s+{VAL}"),
            Category::Secret,
        ),
        // git credential / http extraheader handled by header rule.
    ];
    rules
        .into_iter()
        .map(|(p, c)| {
            let filter = if p.contains("--proxy-user") {
                Filter::NotNumeric
            } else if p.contains("--cookie") {
                Filter::CookieValue
            } else {
                Filter::None
            };
            (Regex::new(&p).expect("tool regex must compile"), c, filter)
        })
        .collect()
});

re!(
    HTPASSWD,
    r"\bhtpasswd\s+(?P<flags>(?:-[A-Za-z]+\s+)*)(?P<args>[^\n|;&]*)"
);

fn tool_specific(text: &str, out: &mut Vec<Finding>) {
    for (re, cat, filter) in TOOL_RULES.iter() {
        for c in re.captures_iter(text) {
            let Some(v) = c.name("val") else { continue };
            let r = unquote_range(text, v.range());
            let value = &text[r.clone()];
            if !is_real_value(value) {
                continue;
            }
            let skip = match filter {
                Filter::None => false,
                Filter::NotNumeric => value.chars().all(|ch| ch.is_ascii_digit()),
                Filter::CookieValue => !value.contains('='),
            };
            if !skip {
                push(out, r, *cat);
            }
        }
    }
    for c in HTPASSWD.captures_iter(text) {
        let (Some(flags), Some(args)) = (c.name("flags"), c.name("args")) else {
            continue;
        };
        let f = flags.as_str();
        if !f.contains('b') {
            continue;
        }
        let words: Vec<(usize, &str)> = split_ws(args.as_str());
        let idx = if f.contains('n') { 1 } else { 2 };
        if let Some((off, w)) = words.get(idx) {
            let s = args.start() + off;
            push(out, unquote_range(text, s..s + w.len()), Category::Password);
        }
    }
}

// ---------------------------------------------------------------------------
// Generic high-entropy strings (Standard/Strict)
// ---------------------------------------------------------------------------

re!(ENTROPY_RUN, r"[A-Za-z0-9+/=_\-]{24,}");
re!(
    UUID,
    r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"
);

re!(
    PUBLIC_KEY_PREFIX,
    r"(?:ssh-(?:rsa|ed25519|dss)|ecdsa-sha2-nistp\d+|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com)\s+$|(?:SHA256|SHA1|MD5):$"
);

fn high_entropy(text: &str, out: &mut Vec<Finding>) {
    for m in ENTROPY_RUN.find_iter(text) {
        // Public SSH key bodies and key fingerprints are not secrets.
        let head_start = m.start().saturating_sub(48);
        let mut hs = head_start;
        while !text.is_char_boundary(hs) {
            hs += 1;
        }
        if PUBLIC_KEY_PREFIX.is_match(&text[hs..m.start()]) {
            continue;
        }
        let s = m.as_str();
        let body = s.trim_end_matches('=');
        if body.contains('=') {
            // `key=value` parts are judged separately (base64 only has `=`
            // as trailing padding).
            let mut off = m.start();
            for part in s.split('=') {
                if part.len() >= 24 && looks_random(part) {
                    push(out, off..off + part.len(), Category::Secret);
                }
                off += part.len() + 1;
            }
        } else if s.starts_with('/') {
            // Path: judge each segment separately.
            let mut off = m.start();
            for seg in s.split('/') {
                if seg.len() >= 24 && looks_random(seg) {
                    push(out, off..off + seg.len(), Category::Secret);
                }
                off += seg.len() + 1;
            }
        } else if looks_random(s) {
            push(out, m.range(), Category::Secret);
        }
    }
}

/// Heuristic for random tokens: long, mixed case + digits, frequent
/// character-class changes (unlike camelCase identifiers), high Shannon
/// entropy; UUIDs and pure hex (commit SHAs, digests) are exempt.
pub(crate) fn looks_random(s: &str) -> bool {
    let t = s.trim_end_matches('=');
    if t.len() < 24 || UUID.is_match(t) || t.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let (mut up, mut low, mut dig) = (0usize, 0usize, 0usize);
    for c in t.chars() {
        if c.is_ascii_uppercase() {
            up += 1;
        } else if c.is_ascii_lowercase() {
            low += 1;
        } else if c.is_ascii_digit() {
            dig += 1;
        }
    }
    let special = t.contains(['+', '/']);
    if up < 2 || low < 2 || (dig == 0 && !special) {
        return false;
    }
    let class = |c: char| -> u8 {
        if c.is_ascii_uppercase() {
            0
        } else if c.is_ascii_lowercase() {
            1
        } else if c.is_ascii_digit() {
            2
        } else {
            3
        }
    };
    let chars: Vec<char> = t.chars().collect();
    let transitions = chars
        .windows(2)
        .filter(|w| class(w[0]) != class(w[1]))
        .count();
    let ratio = transitions as f64 / (chars.len() - 1) as f64;
    ratio >= 0.35 && shannon_entropy(t) >= 3.5
}

fn shannon_entropy(s: &str) -> f64 {
    let mut counts = [0usize; 256];
    for b in s.bytes() {
        counts[usize::from(b)] += 1;
    }
    let n = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Strict: IPs, hosts, users, DB names
// ---------------------------------------------------------------------------

re!(IPV4, r"\b(?:\d{1,3}\.){3}\d{1,3}\b");
re!(
    IPV6_CAND,
    r"(?i)(?:(?:[0-9a-f]{0,4}:){2,6}(?:\d{1,3}\.){3}\d{1,3}|[0-9a-f]{0,4}(?::[0-9a-f]{0,4}){2,7})(?:%[0-9a-z]+)?"
);

fn ipv4(text: &str, out: &mut Vec<Finding>) {
    for m in IPV4.find_iter(text) {
        let before = text[..m.start()].chars().next_back();
        let after = &text[m.end()..];
        // Part of a longer dotted number (version strings, OIDs).
        if before.is_some_and(|c| c == '.' || c.is_ascii_digit())
            || (after.starts_with('.') && after[1..].starts_with(|c: char| c.is_ascii_digit()))
        {
            continue;
        }
        if let Some(Category::Ip) = classify_host(m.as_str()) {
            push(out, m.range(), Category::Ip);
        }
    }
}

fn ipv6(text: &str, out: &mut Vec<Finding>) {
    if !text.contains(':') {
        return;
    }
    for m in IPV6_CAND.find_iter(text) {
        let s = m.as_str();
        if s.matches(':').count() < 2 || !s.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        let before = text[..m.start()].chars().next_back();
        let after = text[m.end()..].chars().next();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == ':')
            || after.is_some_and(|c| c.is_ascii_alphanumeric() || c == ':')
        {
            continue;
        }
        let addr = s.split('%').next().unwrap_or(s);
        if addr.parse::<Ipv6Addr>().is_ok() {
            if let Some(Category::Ip) = classify_host(addr) {
                push(out, m.start()..m.start() + addr.len(), Category::Ip);
            }
        }
    }
}

re!(
    USER_AT_HOST,
    r"(?P<user>[A-Za-z_][A-Za-z0-9_.\-]{0,63})@(?P<host>\[[0-9A-Fa-f:.]+\]|[A-Za-z0-9](?:[A-Za-z0-9\-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9\-]{0,61}[A-Za-z0-9])?)*)"
);

fn user_at_host(text: &str, out: &mut Vec<Finding>) {
    if !text.contains('@') {
        return;
    }
    for c in USER_AT_HOST.captures_iter(text) {
        let (Some(u), Some(h)) = (c.name("user"), c.name("host")) else {
            continue;
        };
        if text[..u.start()]
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '.' | '-' | ':' | '/'))
        {
            // URL userinfo is handled by the URL rule; inside words skip.
            continue;
        }
        let host = h.as_str();
        let lower = host.to_ascii_lowercase();
        if lower.starts_with("sha") && lower[3..].chars().all(|c| c.is_ascii_digit()) {
            continue; // image@sha256:...
        }
        if !is_generic_user(u.as_str()) {
            push(out, u.range(), Category::User);
        }
        let inner = host.trim_start_matches('[').trim_end_matches(']');
        let off = h.start() + (host.len() - host.trim_start_matches('[').len());
        let cat = if host.contains('.') || host.starts_with('[') {
            classify_host(inner)
        } else {
            // Single label after user@ (shell prompts, ssh targets).
            classify_host(inner).map(|_| Category::Host)
        };
        if let Some(cat) = cat {
            push(out, off..off + inner.len(), cat);
        }
    }
}

re!(
    DOTTED,
    r"\b[A-Za-z0-9](?:[A-Za-z0-9\-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9\-]{0,61}[A-Za-z0-9])?)+\.?"
);

/// TLDs treated as hostnames in free text. File-extension-like TLDs
/// (sh, py, rs, md, pl, so, tf, zip, app, info, …) are deliberately absent.
const TLDS: &[&str] = &[
    "com",
    "net",
    "org",
    "io",
    "dev",
    "cloud",
    "ai",
    "co",
    "biz",
    "xyz",
    "online",
    "site",
    "tech",
    "local",
    "internal",
    "lan",
    "corp",
    "home",
    "intranet",
    "intra",
    "localdomain",
    "priv",
    "svc",
    "consul",
    "example",
    "arpa",
    "ru",
    "ua",
    "by",
    "kz",
    "de",
    "uk",
    "us",
    "fr",
    "nl",
    "eu",
    "se",
    "fi",
    "dk",
    "ch",
    "at",
    "be",
    "cz",
    "sk",
    "es",
    "pt",
    "ro",
    "hu",
    "bg",
    "gr",
    "tr",
    "il",
    "jp",
    "cn",
    "kr",
    "tw",
    "hk",
    "sg",
    "au",
    "nz",
    "ca",
    "br",
    "ar",
    "mx",
    "cl",
    "za",
    "ie",
    "lt",
    "lv",
    "ee",
    "gov",
    "edu",
    "mil",
    "int",
    "su",
    "rf",
    "am",
    "ge",
    "uz",
    "vn",
    "th",
    "my",
    "ph",
    "pk",
    "ng",
    "ke",
    "eg",
    "sa",
    "ae",
    "qa",
    "cy",
    "mt",
    "lu",
    "si",
    "hr",
];

/// Public, non-identifying domains kept even under Strict (exact or
/// subdomain match).
const PUBLIC_DOMAINS: &[&str] = &[
    "github.com",
    "githubusercontent.com",
    "gitlab.com",
    "bitbucket.org",
    "docker.io",
    "docker.com",
    "quay.io",
    "ghcr.io",
    "gcr.io",
    "k8s.io",
    "kubernetes.io",
    "helm.sh",
    "pypi.org",
    "npmjs.com",
    "npmjs.org",
    "crates.io",
    "rust-lang.org",
    "golang.org",
    "go.dev",
    "example.com",
    "example.org",
    "example.net",
    "microsoft.com",
    "apple.com",
    "google.com",
    "googleapis.com",
    "ubuntu.com",
    "debian.org",
    "archlinux.org",
    "fedoraproject.org",
    "centos.org",
    "redhat.com",
    "python.org",
    "nodejs.org",
    "cloudflare.com",
    "openai.com",
    "deepseek.com",
    "ollama.com",
    "anthropic.com",
    "stackoverflow.com",
    "wikipedia.org",
    "apache.org",
    "nginx.org",
    "postgresql.org",
    "mysql.com",
    "redis.io",
    "opensearch.org",
    "elastic.co",
    "hashicorp.com",
    "terraform.io",
    "ansible.com",
    "letsencrypt.org",
    "mozilla.org",
    "w3.org",
    "ietf.org",
    "json-schema.org",
    "schema.org",
];

fn fqdns(text: &str, out: &mut Vec<Finding>) {
    if !text.contains('.') {
        return;
    }
    for m in DOTTED.find_iter(text) {
        let s = m.as_str().trim_end_matches('.');
        let before = text[..m.start()].chars().next_back();
        let after = text[m.start() + s.len()..].chars().next();
        if before.is_some_and(|c| c == '@' || c == '$' || c == '.' || c == '-' || c == '_')
            || after.is_some_and(|c| c == '(' || c == '_' || c == '-')
        {
            continue;
        }
        let Some(tld) = s.rsplit('.').next() else {
            continue;
        };
        let tld = tld.to_ascii_lowercase();
        if !TLDS.contains(&tld.as_str())
            || s.split('.').all(|l| l.chars().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        if let Some(Category::Host) = classify_host(s) {
            push(out, m.start()..m.start() + s.len(), Category::Host);
        }
    }
}

re!(
    HOME_DIR,
    r"(?i)(?:/home/|/Users/|[A-Z]:\\Users\\|/var/mail/)(?P<user>[A-Za-z0-9_][A-Za-z0-9_.\-]{0,63})"
);

fn home_dirs(text: &str, out: &mut Vec<Finding>) {
    for c in HOME_DIR.captures_iter(text) {
        if let Some(u) = c.name("user") {
            let v = u.as_str();
            let lower = v.to_ascii_lowercase();
            if !matches!(
                lower.as_str(),
                "shared" | "public" | "default" | "all users" | "default user" | "runner"
            ) && !is_generic_user(v)
            {
                push(out, u.range(), Category::User);
            }
        }
    }
}

re!(
    SSH_CONFIG,
    r"(?mi)^[ \t]*(?P<key>Host|HostName|User|ProxyJump)[ \t]+(?P<val>[^\n#]+?)[ \t\r]*$"
);

fn ssh_config(text: &str, out: &mut Vec<Finding>) {
    for c in SSH_CONFIG.captures_iter(text) {
        let (Some(k), Some(v)) = (c.name("key"), c.name("val")) else {
            continue;
        };
        let key = k.as_str().to_ascii_lowercase();
        let mut off = v.start();
        for part in v.as_str().split([' ', ',']) {
            if !part.is_empty() && part != "*" {
                if key == "user" {
                    if !is_generic_user(part) {
                        push(out, off..off + part.len(), Category::User);
                    }
                } else {
                    let host_part = part.rsplit('@').next().unwrap_or(part);
                    let host_only = host_part.split(':').next().unwrap_or(host_part);
                    let s = off + (part.len() - host_part.len());
                    if let Some(cat) = classify_host(host_only) {
                        push(out, s..s + host_only.len(), cat);
                    }
                }
            }
            off += part.len() + 1;
        }
    }
}

re!(
    SQL_USE,
    r#"(?im)^(?:[ \t]*[\w\-]*[=#>]+)?[ \t]*(?:use|\\c|\\connect)[ \t]+[`"\[]?(?P<db>[A-Za-z0-9_$\-]+)[`"\]]?[ \t]*;?[ \t\r]*$"#
);
re!(
    SQL_DDL,
    r#"(?i)\b(?:create|drop|alter)\s+(?:database|schema|keyspace)\s+(?:if\s+(?:not\s+)?exists\s+)?[`"\[]?(?P<db>[A-Za-z0-9_$\-]+)"#
);

fn sql_databases(text: &str, out: &mut Vec<Finding>) {
    for re in [&*SQL_USE, &*SQL_DDL] {
        for c in re.captures_iter(text) {
            if let Some(db) = c.name("db") {
                let kw = matches!(
                    db.as_str().to_ascii_lowercase().as_str(),
                    "if" | "not" | "exists"
                );
                if !kw && !is_system_db(db.as_str()) {
                    push(out, db.range(), Category::Database);
                }
            }
        }
    }
}

re!(
    POSITIONAL_CMD,
    r"(?m)(?:^|[;&|(]\s*)(?:sudo\s+)?(?P<cmd>ssh|slogin|mosh|telnet|sftp|scp|rsync|ping6?|traceroute6?|tracert|tracepath|mtr|nc|ncat|dig|nslookup|host|whois|nmap)(?P<args>[ \t][^\n;&|]*)"
);

/// Options that take a separate value, per command (to find the host arg).
pub(crate) fn opt_takes_value(cmd: &str, opt: &str) -> bool {
    match cmd {
        "ssh" | "slogin" | "sftp" | "scp" | "mosh" => matches!(
            opt,
            "-p" | "-i"
                | "-l"
                | "-o"
                | "-J"
                | "-F"
                | "-L"
                | "-R"
                | "-D"
                | "-W"
                | "-b"
                | "-c"
                | "-E"
                | "-e"
                | "-I"
                | "-m"
                | "-O"
                | "-Q"
                | "-S"
                | "-w"
                | "-B"
                | "-P"
        ),
        "ping" | "ping6" => matches!(opt, "-c" | "-i" | "-W" | "-w" | "-s" | "-t" | "-I" | "-Q"),
        "nc" | "ncat" => matches!(opt, "-p" | "-s" | "-w" | "-i" | "-x" | "-X"),
        "traceroute" | "traceroute6" | "mtr" => {
            matches!(opt, "-m" | "-p" | "-q" | "-w" | "-i" | "-s" | "-c")
        }
        "dig" => matches!(
            opt,
            "-p" | "-t" | "-c" | "-b" | "-f" | "-k" | "-q" | "-x" | "-y"
        ),
        _ => false,
    }
}

fn positional_hosts(text: &str, out: &mut Vec<Finding>) {
    for c in POSITIONAL_CMD.captures_iter(text) {
        let (Some(cmd), Some(args)) = (c.name("cmd"), c.name("args")) else {
            continue;
        };
        let cmd = cmd.as_str();
        let words = split_ws(args.as_str());
        let mut skip_next = false;
        let mut found_first = false;
        for (off, w) in words {
            if skip_next {
                skip_next = false;
                continue;
            }
            if w.starts_with('-') {
                skip_next = opt_takes_value(cmd, w);
                continue;
            }
            let start = args.start() + off;
            // scp/rsync: [user@]host:path
            if matches!(cmd, "scp" | "rsync") {
                if let Some(colon) = w.find(':') {
                    let hostpart = &w[..colon];
                    let host = hostpart.rsplit('@').next().unwrap_or(hostpart);
                    if host.len() > 1 && !w[colon..].starts_with("://") {
                        let s = start + (hostpart.len() - host.len());
                        if let Some(cat) = classify_host(host) {
                            push(out, s..s + host.len(), cat);
                        }
                    }
                }
                continue;
            }
            if found_first {
                break;
            }
            found_first = true;
            if w.starts_with('@') || w.contains("://") || w.starts_with(['\'', '"', '$']) {
                continue;
            }
            let host = w.rsplit('@').next().unwrap_or(w);
            let host = host.split(':').next().unwrap_or(host);
            let s = start + (w.len() - w.rsplit('@').next().unwrap_or(w).len());
            if let Some(cat) = classify_host(host) {
                push(out, s..s + host.len(), cat);
            }
        }
    }
}

/// Tokenize host-context literals (word-boundary aware; hosts case-insensitive).
pub(super) fn literals(text: &str, lits: &[(Category, Zeroizing<String>)], out: &mut Vec<Finding>) {
    if lits.is_empty() {
        return;
    }
    let lower = text.to_ascii_lowercase();
    for (cat, lit) in lits {
        let needle = lit.to_ascii_lowercase();
        let hay = if *cat == Category::User {
            text
        } else {
            lower.as_str()
        };
        let needle = if *cat == Category::User {
            lit.as_str()
        } else {
            needle.as_str()
        };
        let mut from = 0;
        while let Some(i) = hay[from..].find(needle) {
            let s = from + i;
            let e = s + needle.len();
            let ok_before = !text[..s]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
            let ok_after = !text[e..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
            if ok_before && ok_after {
                push(out, s..e, *cat);
            }
            from = e.max(s + 1);
            while from < hay.len() && !hay.is_char_boundary(from) {
                from += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Quoted value or a bare token up to a delimiter.
    Token,
    /// Quoted value or the rest of the line (YAML / INI).
    LineRest,
}

fn line_end(text: &str, from: usize) -> usize {
    text[from..]
        .find(['\n', '\r'])
        .map_or(text.len(), |i| from + i)
}

fn is_line_start(text: &str, pos: usize) -> bool {
    let ls = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let prefix = text[ls..pos].trim_start();
    let prefix = prefix
        .strip_prefix("- ")
        .or_else(|| prefix.strip_prefix("export "))
        .or_else(|| prefix.strip_prefix("set "))
        .or_else(|| prefix.strip_prefix("$env:"))
        .unwrap_or(prefix)
        .trim();
    prefix.is_empty() || prefix == "\"" || prefix == "'"
}

/// Extract the value starting at `start` (after the separator).
fn extract_value(text: &str, start: usize, mode: Mode) -> Option<Range<usize>> {
    let rest = &text[start..];
    let first = rest.chars().next()?;
    if first == '"' || first == '\'' || first == '`' {
        let body = start + 1;
        let mut i = body;
        while i < text.len() {
            let ch = text[i..].chars().next()?;
            if ch == '\\' && first != '\'' {
                i += 1;
                if let Some(n) = text[i..].chars().next() {
                    i += n.len_utf8();
                }
                continue;
            }
            if ch == first {
                return Some(body..i);
            }
            if ch == '\n' {
                break;
            }
            i += ch.len_utf8();
        }
        let e = line_end(text, body);
        return (e > body).then_some(body..e);
    }
    if rest.starts_with("\\\"") {
        let body = start + 2;
        let e = text[body..]
            .find("\\\"")
            .map_or_else(|| line_end(text, body), |i| body + i);
        return (e > body).then_some(body..e);
    }
    // Template / shell references keep their closing delimiter
    // (`{{password}}`, `${DB_PASS}`, `$(cat f)`).
    for (open, close) in [("{{", "}}"), ("${{", "}}"), ("${", "}"), ("$(", ")")] {
        if let Some(after) = rest.strip_prefix(open) {
            if let Some(i) = after.find(close) {
                let e = start + open.len() + i + close.len();
                return Some(start..e);
            }
        }
    }
    match mode {
        Mode::LineRest => {
            let e = line_end(text, start);
            let v = text[start..e].trim_end();
            (!v.is_empty()).then_some(start..start + v.len())
        }
        Mode::Token => {
            let e = rest
                .find(|c: char| {
                    c.is_whitespace()
                        || matches!(
                            c,
                            ',' | ';' | '&' | '|' | ')' | '}' | ']' | '>' | '<' | '"' | '\'' | '`'
                        )
                })
                .map_or(text.len(), |i| start + i);
            (e > start).then_some(start..e)
        }
    }
}

/// Strip surrounding quotes from a matched value range.
fn unquote_range(text: &str, r: Range<usize>) -> Range<usize> {
    let s = &text[r.clone()];
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        r.start + 1..r.end - 1
    } else {
        r
    }
}

fn split_ws(s: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            if let Some(st) = start.take() {
                out.push((st, &s[st..i]));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(st) = start {
        out.push((st, &s[st..]));
    }
    out
}

re!(
    REFERENCE,
    r"^(?:\$\{[^}]*\}|\$\([^)]*\)|\$env:[A-Za-z_][A-Za-z0-9_]*|\$[A-Z_][A-Z0-9_]*|%[A-Za-z_][A-Za-z0-9_]*%|\{\{[^}]*\}\}|\$\{\{[^}]*\}\}|!vault\b.*|<[A-Z][A-Z_]*_\d+>)$"
);

/// `false` for empty values, references (`$VAR`, `${X}`, `%X%`, `{{x}}`),
/// placeholders/hints (`<…>`), masks (`***`) and booleans/null.
pub(crate) fn is_real_value(v: &str) -> bool {
    let t = v.trim();
    if t.is_empty() || t.len() > 8192 {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true"
            | "false"
            | "null"
            | "none"
            | "nil"
            | "yes"
            | "no"
            | "on"
            | "off"
            | "~"
            | "-"
            | "required"
            | "optional"
            | "prompt"
            | "n/a"
            | "\"\""
            | "''"
            | "|"
            | ">"
            | "{"
            | "["
    ) {
        return false;
    }
    if REFERENCE.is_match(t) {
        return false;
    }
    if t.starts_with('<') && t.ends_with('>') {
        return false;
    }
    if t.chars()
        .all(|c| matches!(c, '*' | 'x' | 'X' | '.' | '•' | '…' | '#'))
    {
        return false;
    }
    true
}

/// Extra per-key value checks.
fn value_fits(cat: Category, key: &str, value: &str) -> bool {
    let k = key.to_ascii_lowercase();
    let v = value.trim();
    let is_path = v.starts_with('/') || v.starts_with("~/") || v.starts_with("./");
    match cat {
        // `PWD=/home/x` is the working directory; `Pwd=secret;` is a password.
        Category::Password if k == "pwd" => !is_path,
        Category::Secret if k.contains("credential") => !is_path,
        // Tokens/keys shorter than 6 characters are identifiers (`session:1`,
        // `token_count=3`), except for PIN/OTP-like keys.
        Category::Secret
            if v.chars().count() < 6
                && !["pin", "otp", "totp", "passcode"]
                    .iter()
                    .any(|p| k.contains(p)) =>
        {
            false
        }
        Category::User => !v.chars().all(|c| c.is_ascii_digit()) && !is_generic_user(v),
        Category::Database => !v.chars().all(|c| c.is_ascii_digit()) && !is_system_db(v),
        _ => true,
    }
}

/// Split an identifier into lowercase segments on `_ - . ` and camelCase.
fn split_key(key: &str) -> Vec<String> {
    let mut segs = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = key.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if matches!(c, '_' | '-' | '.' | ' ' | ':') {
            if !cur.is_empty() {
                segs.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if c.is_uppercase() && !cur.is_empty() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || prev.is_ascii_digit() || (prev.is_uppercase() && next_lower) {
                segs.push(std::mem::take(&mut cur));
            }
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        segs.push(cur);
    }
    segs
}

const EXCLUDED_LAST: &[&str] = &[
    "file",
    "files",
    "path",
    "dir",
    "directory",
    "name",
    "names",
    "type",
    "length",
    "len",
    "size",
    "min",
    "max",
    "policy",
    "expiry",
    "expires",
    "expiration",
    "ttl",
    "url",
    "uri",
    "endpoint",
    "enabled",
    "enable",
    "disabled",
    "required",
    "prompt",
    "mode",
    "method",
    "format",
    "field",
    "env",
    "var",
    "header",
    "count",
    "timeout",
    "version",
    "source",
    "ref",
    "command",
    "cmd",
    "helper",
    "manager",
    "store",
    "strategy",
    "provider",
    "algorithm",
    "algo",
    "encoding",
    "rotation",
    "hint",
    "label",
    "regex",
    "pattern",
    "location",
    "kind",
    "class",
    "stdin",
    "age",
    "lifetime",
    "duration",
    "interval",
    "limit",
    "flag",
    "option",
    "description",
    "help",
    "example",
    "template",
    "generator",
    "validator",
    "reset",
    "change",
    "id",
    "ids",
    "arn",
    "prefix",
    "suffix",
    "scope",
    "scopes",
    "issuer",
    "audience",
    "backend",
    "plugin",
    "cache",
    "id_token_hint",
    "keyring",
    "agent",
    "server",
    "host",
    "port",
];

const KEY_PREFIXES: &[&str] = &[
    "access",
    "secret",
    "private",
    "api",
    "master",
    "encryption",
    "encrypt",
    "signing",
    "sign",
    "account",
    "shared",
    "app",
    "client",
    "auth",
    "storage",
    "service",
    "admin",
    "root",
    "session",
    "hmac",
    "jwt",
    "cookie",
    "crypt",
    "cipher",
    "aes",
    "enc",
    "license",
    "sas",
    "primary",
    "secondary",
    "subscription",
    "consumer",
    "deploy",
    "webhook",
    "ingest",
    "write",
    "x",
    "api-",
    "routing",
    "integration",
    "mailgun",
    "stripe",
    "openai",
    "deepseek",
];

/// Whether a key/flag/tag name holds a secret, and which kind.
pub(crate) fn classify_secret_key(key: &str) -> Option<Category> {
    let segs = split_key(key);
    let last = segs.last()?.as_str();
    if EXCLUDED_LAST.contains(&last)
        || segs.first().is_some_and(|s| s == "no")
        || segs
            .iter()
            .any(|s| matches!(s.as_str(), "ask" | "stdin" | "interactive" | "tokens"))
    {
        return None;
    }
    let compact: String = segs.concat();
    if compact.contains("password")
        || compact.contains("passwd")
        || compact.contains("passphrase")
        || segs
            .iter()
            .any(|s| matches!(s.as_str(), "pass" | "pw" | "pwd" | "sshpass" | "pgpass"))
    {
        return Some(Category::Password);
    }
    let key_with_prefix = segs
        .windows(2)
        .any(|w| w[1] == "key" && KEY_PREFIXES.contains(&w[0].as_str()));
    if compact.contains("secret")
        || (compact.contains("token") && !compact.contains("tokeniz"))
        || compact.contains("apikey")
        || compact.contains("accesskey")
        || compact.contains("privatekey")
        || compact.contains("credential")
        || compact.contains("sessionid")
        || compact == "session"
        || key_with_prefix
        || segs.iter().any(|s| {
            matches!(
                s.as_str(),
                "sig"
                    | "signature"
                    | "auth"
                    | "authorization"
                    | "cookie"
                    | "bearer"
                    | "apikey"
                    | "passcode"
                    | "otp"
                    | "pin"
                    | "totp"
            )
        })
    {
        return Some(Category::Secret);
    }
    None
}

/// Strict-only keys: usernames, hosts, database names.
fn classify_strict_key(key: &str) -> Option<Category> {
    let segs = split_key(key);
    let last = segs.last()?.as_str();
    let compact: String = segs.concat();
    match last {
        "user" | "username" | "login" | "logname" | "uid" if segs.len() <= 3 => {
            Some(Category::User)
        }
        "database" | "dbname" | "db" | "keyspace" | "catalog" => Some(Category::Database),
        "host" | "hostname" | "server" | "addr" | "address" | "fqdn" => Some(Category::Host),
        _ => match compact.as_str() {
            "pguser" | "mysqluser" => Some(Category::User),
            "pgdatabase" | "pgdbname" => Some(Category::Database),
            "pghost" | "dbhost" | "mysqlhost" | "redishost" => Some(Category::Host),
            _ => None,
        },
    }
}

/// Usernames that identify nothing.
pub(crate) fn is_generic_user(u: &str) -> bool {
    matches!(
        u.trim().to_ascii_lowercase().as_str(),
        "root" | "git" | "nobody"
    )
}

fn is_system_db(db: &str) -> bool {
    matches!(
        db.trim().to_ascii_lowercase().as_str(),
        "postgres"
            | "template0"
            | "template1"
            | "mysql"
            | "information_schema"
            | "performance_schema"
            | "sys"
            | "admin"
            | "local"
            | "config"
            | "system"
            | "system_schema"
            | "master"
            | "tempdb"
            | "model"
            | "msdb"
    )
}

/// Classify a host string: `Ip` for (non-trivial) IP literals, `Host` for
/// names; `None` for loopback/wildcards/public well-known domains or things
/// that are not host names.
pub(crate) fn classify_host(h: &str) -> Option<Category> {
    let h = h.trim().trim_end_matches('.');
    if h.is_empty() || h.len() > 253 {
        return None;
    }
    if let Ok(ip) = h.parse::<Ipv4Addr>() {
        let o = ip.octets();
        let trivial = ip.is_loopback()
            || ip.is_unspecified()
            || ip.is_broadcast()
            || o[0] == 255
            || matches!(
                o,
                [8, 8, 8, 8] | [8, 8, 4, 4] | [1, 1, 1, 1] | [1, 0, 0, 1] | [9, 9, 9, 9]
            );
        return (!trivial).then_some(Category::Ip);
    }
    if let Ok(ip) = h.parse::<Ipv6Addr>() {
        return (!(ip.is_loopback() || ip.is_unspecified())).then_some(Category::Ip);
    }
    let lower = h.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower == "localhost.localdomain" {
        return None;
    }
    if PUBLIC_DOMAINS
        .iter()
        .any(|d| lower == *d || lower.ends_with(&format!(".{d}")))
    {
        return None;
    }
    let valid = lower
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
        && lower.starts_with(|c: char| c.is_ascii_alphanumeric())
        && !lower.chars().all(|c| c.is_ascii_digit() || c == '.');
    valid.then_some(Category::Host)
}

/// Command name at the start of the shell segment containing `pos`.
fn command_before(text: &str, pos: usize) -> Option<String> {
    let seg_start = text[..pos]
        .rfind(['\n', '|', ';', '&', '('])
        .map_or(0, |i| i + 1);
    let words: Vec<&str> = text[seg_start..pos].split_whitespace().collect();
    let idx = crate::shell::skip_wrappers(&words);
    words.get(idx).map(|w| crate::shell::command_name(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_splitting() {
        assert_eq!(split_key("DB_PASSWORD"), vec!["db", "password"]);
        assert_eq!(split_key("rootPassword"), vec!["root", "password"]);
        assert_eq!(split_key("APIKey"), vec!["api", "key"]);
        assert_eq!(
            split_key("X-Amz-Security-Token"),
            vec!["x", "amz", "security", "token"]
        );
        assert_eq!(
            split_key("auth.rootPassword"),
            vec!["auth", "root", "password"]
        );
    }

    #[test]
    fn key_classification() {
        use Category::*;
        for (k, want) in [
            ("password", Some(Password)),
            ("PGPASSWORD", Some(Password)),
            ("MYSQL_PWD", Some(Password)),
            ("db_pass", Some(Password)),
            ("ansible_become_pass", Some(Password)),
            ("bypass", None),
            ("compass", None),
            ("password_file", None),
            ("password-stdin", None),
            ("no-password", None),
            ("api_key", Some(Secret)),
            ("x-api-key", Some(Secret)),
            ("AccountKey", Some(Secret)),
            ("aws_secret_access_key", Some(Secret)),
            ("client_secret", Some(Secret)),
            ("secretName", None),
            ("token_type", None),
            ("max_tokens", None),
            ("access_token", Some(Secret)),
            ("tokenizer", None),
            ("key", None),
            ("ssh_key", None),
            ("public_key", None),
            ("auth", Some(Secret)),
            ("auth_enabled", None),
            ("X-Amz-Signature", Some(Secret)),
            ("author", None),
            ("keyspace", None),
        ] {
            assert_eq!(classify_secret_key(k), want, "key {k}");
        }
    }

    #[test]
    fn entropy_heuristic() {
        assert!(looks_random("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"));
        assert!(looks_random("Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MEFCQ0RFRg"));
        assert!(!looks_random("AbstractSingletonProxyFactoryBean2024v2"));
        assert!(!looks_random("3f786850e387550fdab836ed7e6dc881de23001b"));
        assert!(!looks_random("123e4567-e89b-12d3-a456-426614174000"));
        assert!(!looks_random("api-deployment-7d9f8b6c5-x2x4z-backup-01"));
        assert!(!looks_random("short1A"));
    }

    #[test]
    fn host_classification() {
        assert_eq!(classify_host("10.0.0.5"), Some(Category::Ip));
        assert_eq!(classify_host("127.0.0.1"), None);
        assert_eq!(classify_host("0.0.0.0"), None);
        assert_eq!(classify_host("fe80::1"), Some(Category::Ip));
        assert_eq!(classify_host("::1"), None);
        assert_eq!(classify_host("db.corp.internal"), Some(Category::Host));
        assert_eq!(classify_host("api.github.com"), None);
        assert_eq!(classify_host("localhost"), None);
        assert_eq!(classify_host("*"), None);
    }
}
