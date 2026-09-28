//! OpenSSH `known_hosts` support: parsing/import (plain and hashed `|1|`
//! host names, `@cert-authority`, `@revoked`), host/port pattern matching and
//! the pure host-key verification decision used by the native connector.

use crate::keys::fingerprint_sha256;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use cc_models::known_host::{host_pattern, KnownHost, KnownHostMarker, KnownHostSource};
use cc_models::ObjectId;
use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

/// Marker of a known_hosts line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    CertAuthority,
    Revoked,
}

/// One parsed known_hosts line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownHostsLine {
    pub line_no: usize,
    pub marker: Option<Marker>,
    /// Host pattern field exactly as written (comma list or `|1|salt|hash`).
    pub patterns: String,
    pub key_type: String,
    /// Base64 key blob.
    pub key_base64: String,
    pub comment: Option<String>,
}

impl KnownHostsLine {
    /// `SHA256:…` fingerprint of the key blob (`None` if not valid base64).
    pub fn fingerprint(&self) -> Option<String> {
        B64.decode(self.key_base64.as_bytes())
            .ok()
            .map(|b| fingerprint_sha256(&b))
    }

    /// Convert to a vault [`KnownHost`] object.
    pub fn to_known_host(&self, source: KnownHostSource) -> Option<KnownHost> {
        let fingerprint = self.fingerprint()?;
        let now = chrono::Utc::now();
        let marker = match self.marker {
            Some(Marker::CertAuthority) => Some(KnownHostMarker::CertAuthority),
            Some(Marker::Revoked) => Some(KnownHostMarker::Revoked),
            None => None,
        };
        Some(KnownHost {
            id: ObjectId::new(),
            host_pattern: self.patterns.clone(),
            key_type: self.key_type.clone(),
            public_key: self.key_base64.clone(),
            fingerprint_sha256: fingerprint,
            source,
            revoked: self.marker == Some(Marker::Revoked),
            marker,
            added_at: now,
            updated_at: now,
        })
    }
}

/// Result of parsing a whole file: good lines and per-line errors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownHostsFile {
    pub lines: Vec<KnownHostsLine>,
    /// `(line number, reason)` for lines that could not be parsed.
    pub errors: Vec<(usize, String)>,
}

/// Parse `known_hosts` text. Comments and blank lines are skipped; malformed
/// lines are reported in [`KnownHostsFile::errors`] instead of failing.
pub fn parse_known_hosts(text: &str) -> KnownHostsFile {
    let mut out = KnownHostsFile::default();
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match parse_line(line, line_no) {
            Ok(l) => out.lines.push(l),
            Err(e) => out.errors.push((line_no, e)),
        }
    }
    out
}

fn parse_line(line: &str, line_no: usize) -> Result<KnownHostsLine, String> {
    let mut fields = line.split_whitespace();
    let mut first = fields.next().ok_or("empty line")?;
    let marker = match first {
        "@cert-authority" => Some(Marker::CertAuthority),
        "@revoked" => Some(Marker::Revoked),
        m if m.starts_with('@') => return Err(format!("unknown marker {m}")),
        _ => None,
    };
    if marker.is_some() {
        first = fields.next().ok_or("missing host patterns")?;
    }
    let key_type = fields.next().ok_or("missing key type")?;
    let key_base64 = fields.next().ok_or("missing key")?;
    if B64.decode(key_base64.as_bytes()).is_err() {
        return Err("key is not valid base64".into());
    }
    if first.starts_with("|1|") && parse_hashed(first).is_none() {
        return Err("malformed hashed host name".into());
    }
    let comment: Vec<&str> = fields.collect();
    Ok(KnownHostsLine {
        line_no,
        marker,
        patterns: first.to_string(),
        key_type: key_type.to_string(),
        key_base64: key_base64.to_string(),
        comment: (!comment.is_empty()).then(|| comment.join(" ")),
    })
}

/// Import known_hosts text as vault objects (source `Imported`, or
/// `CertAuthority` for `@cert-authority` lines; `@revoked` → `revoked`).
pub fn import_known_hosts(text: &str) -> (Vec<KnownHost>, Vec<(usize, String)>) {
    let parsed = parse_known_hosts(text);
    let mut errors = parsed.errors;
    let mut hosts = Vec::new();
    for l in parsed.lines {
        match l.to_known_host(KnownHostSource::Imported) {
            Some(h) => hosts.push(h),
            None => errors.push((l.line_no, "key is not valid base64".into())),
        }
    }
    (hosts, errors)
}

/// Export vault entries as OpenSSH known_hosts text (public data only), e.g.
/// for the `UserKnownHostsFile` of the OpenSSH fallback.
pub fn export_known_hosts(entries: &[KnownHost]) -> String {
    let mut out = String::new();
    for e in entries {
        if e.revoked {
            out.push_str("@revoked ");
        } else if e.is_cert_authority() {
            out.push_str("@cert-authority ");
        }
        out.push_str(&e.host_pattern);
        out.push(' ');
        out.push_str(&e.key_type);
        out.push(' ');
        out.push_str(&e.public_key);
        out.push('\n');
    }
    out
}

fn parse_hashed(pattern: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let rest = pattern.strip_prefix("|1|")?;
    let (salt, hash) = rest.split_once('|')?;
    let salt = B64.decode(salt).ok()?;
    let hash = B64.decode(hash).ok()?;
    (hash.len() == 20).then_some((salt, hash))
}

/// Hash a host name the way `ssh-keygen -H` does (`|1|salt|hash`).
pub fn hash_host_name(host: &str, port: u16, salt: &[u8]) -> String {
    let name = host_pattern(host, port);
    let digest = hmac_sha1(salt, name.as_bytes());
    format!("|1|{}|{}", B64.encode(salt), B64.encode(digest))
}

fn hmac_sha1(key: &[u8], msg: &[u8]) -> Vec<u8> {
    // HMAC accepts keys of any length; `new_from_slice` cannot fail for it.
    match <Hmac<Sha1> as KeyInit>::new_from_slice(key) {
        Ok(mut mac) => {
            mac.update(msg);
            mac.finalize().into_bytes().to_vec()
        }
        Err(_) => Vec::new(),
    }
}

/// Glob match with `*` and `?` (case-insensitive, ASCII).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let t: Vec<char> = text.to_ascii_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Does a known_hosts host field (comma list with `!negation`, globs,
/// `[host]:port`, or a hashed `|1|…` name) match `host:port`?
///
/// OpenSSH semantics: the candidate is `host` for port 22 and `[host]:port`
/// otherwise; a negated match wins over any positive match.
pub fn pattern_matches(patterns: &str, host: &str, port: u16) -> bool {
    let candidate = host_pattern(host, port);
    if patterns.starts_with("|1|") {
        return match parse_hashed(patterns) {
            Some((salt, hash)) => {
                // Constant-time-ish compare is unnecessary: data is public.
                hmac_sha1(&salt, candidate.as_bytes()) == hash
            }
            None => false,
        };
    }
    let mut positive = false;
    for p in patterns.split(',') {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        if let Some(neg) = p.strip_prefix('!') {
            if glob_match(neg, &candidate) {
                return false;
            }
        } else if glob_match(p, &candidate) {
            positive = true;
        }
    }
    positive
}

/// Normalized key type for comparison (all RSA signature variants are `ssh-rsa`).
pub fn normalize_key_type(key_type: &str) -> &str {
    match key_type {
        "rsa-sha2-256" | "rsa-sha2-512" => "ssh-rsa",
        other => other,
    }
}

/// Outcome of checking a presented host key against known entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyCheck {
    /// Key (or a certificate signed by a trusted CA) is known.
    Trusted { via_cert_authority: bool },
    /// No entry for this key type. `other_key_types` lists key types that
    /// are known for the host (server offered a different type).
    Unknown { other_key_types: Vec<String> },
    /// An entry of the same key type exists with a different key.
    Changed { expected_fingerprints: Vec<String> },
    /// Key (or its CA) is revoked.
    Revoked,
}

/// Minimal view of a presented host certificate for [`check_host_key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedCertificate {
    /// Signing CA key blob.
    pub ca_key_blob: Vec<u8>,
    /// Certificate already validated (signature, validity window, host type,
    /// principal) against the CA key in `ca_key_blob`.
    pub valid_for_host: bool,
}

/// Pure host-key decision (no I/O). `entries` are the store's matches for
/// the host (see [`crate::KnownHostsStore::lookup`]).
pub fn check_host_key(
    entries: &[KnownHost],
    key_type: &str,
    key_blob: &[u8],
    certificate: Option<&PresentedCertificate>,
) -> HostKeyCheck {
    let blob_of = |e: &KnownHost| B64.decode(e.public_key.as_bytes()).ok();

    // 1. Revocation of the key itself or of the signing CA.
    for e in entries.iter().filter(|e| e.revoked) {
        if let Some(b) = blob_of(e) {
            if b == key_blob || certificate.is_some_and(|c| c.ca_key_blob == b) {
                return HostKeyCheck::Revoked;
            }
        }
    }

    // 2. Host certificate signed by a trusted CA.
    if let Some(cert) = certificate {
        let trusted_ca = entries.iter().any(|e| {
            !e.revoked && e.is_cert_authority() && blob_of(e).is_some_and(|b| b == cert.ca_key_blob)
        });
        if trusted_ca && cert.valid_for_host {
            return HostKeyCheck::Trusted {
                via_cert_authority: true,
            };
        }
    }

    // 3. Plain keys.
    let wanted = normalize_key_type(key_type);
    let plain: Vec<&KnownHost> = entries
        .iter()
        .filter(|e| !e.revoked && !e.is_cert_authority())
        .collect();
    if plain
        .iter()
        .any(|e| blob_of(e).is_some_and(|b| b == key_blob))
    {
        return HostKeyCheck::Trusted {
            via_cert_authority: false,
        };
    }
    let same_type: Vec<String> = plain
        .iter()
        .filter(|e| normalize_key_type(&e.key_type) == wanted)
        .map(|e| e.fingerprint_sha256.clone())
        .collect();
    if !same_type.is_empty() {
        return HostKeyCheck::Changed {
            expected_fingerprints: same_type,
        };
    }
    let mut other: Vec<String> = plain.iter().map(|e| e.key_type.clone()).collect();
    other.sort();
    other.dedup();
    HostKeyCheck::Unknown {
        other_key_types: other,
    }
}

/// New TOFU entry for an accepted key.
pub fn new_known_host(
    host: &str,
    port: u16,
    key_type: &str,
    key_blob: &[u8],
    source: KnownHostSource,
) -> KnownHost {
    let now = chrono::Utc::now();
    KnownHost {
        id: ObjectId::new(),
        host_pattern: host_pattern(host, port),
        key_type: key_type.to_string(),
        public_key: B64.encode(key_blob),
        fingerprint_sha256: fingerprint_sha256(key_blob),
        source,
        revoked: false,
        marker: None,
        added_at: now,
        updated_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED_BLOB_A: &[u8] =
        b"\x00\x00\x00\x0bssh-ed25519\x00\x00\x00\x20AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const ED_BLOB_B: &[u8] =
        b"\x00\x00\x00\x0bssh-ed25519\x00\x00\x00\x20BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

    fn entry(pattern: &str, key_type: &str, blob: &[u8], source: KnownHostSource) -> KnownHost {
        new_known_host("x", 22, key_type, blob, source).with_pattern(pattern)
    }

    trait WithPattern {
        fn with_pattern(self, p: &str) -> Self;
    }
    impl WithPattern for KnownHost {
        fn with_pattern(mut self, p: &str) -> Self {
            self.host_pattern = p.to_string();
            self
        }
    }

    #[test]
    fn glob() {
        assert!(glob_match("*.example.com", "a.example.com"));
        assert!(glob_match("*.EXAMPLE.com", "a.b.example.com"));
        assert!(!glob_match("*.example.com", "example.com"));
        assert!(glob_match("10.0.0.?", "10.0.0.7"));
        assert!(!glob_match("10.0.0.?", "10.0.0.17"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("[h]:22*", "[h]:2222"));
    }

    #[test]
    fn plain_patterns_with_ports_and_negation() {
        assert!(pattern_matches("example.com", "Example.COM", 22));
        assert!(!pattern_matches("example.com", "example.com", 2222));
        assert!(pattern_matches("[example.com]:2222", "example.com", 2222));
        assert!(pattern_matches("a,b,example.com", "example.com", 22));
        assert!(pattern_matches("*.corp", "db.corp", 22));
        assert!(!pattern_matches("*.corp,!db.corp", "db.corp", 22));
        assert!(pattern_matches("*.corp,!db.corp", "web.corp", 22));
        assert!(pattern_matches("[*.corp]:*", "web.corp", 2200));
        assert!(!pattern_matches("", "x", 22));
    }

    #[test]
    fn hashed_patterns() {
        let salt = [7u8; 20];
        let hashed = hash_host_name("server.example", 22, &salt);
        assert!(hashed.starts_with("|1|"));
        assert!(pattern_matches(&hashed, "server.example", 22));
        assert!(pattern_matches(&hashed, "SERVER.example", 22));
        assert!(!pattern_matches(&hashed, "server.example", 2222));
        assert!(!pattern_matches(&hashed, "other", 22));
        let hashed_port = hash_host_name("10.0.0.1", 2222, &salt);
        assert!(pattern_matches(&hashed_port, "10.0.0.1", 2222));
    }

    #[test]
    fn hashed_pattern_matches_independent_vectors() {
        // Vectors computed with Python's hmac/hashlib (independent of this code).
        let salt = "Hkq1R2n5S0Q8Q1l5gC4uDEwMW3o=";
        let localhost = format!("|1|{salt}|a6CMMyQVOqUhFcVoJY7zBAK+OeI=");
        assert!(pattern_matches(&localhost, "localhost", 22));
        assert!(!pattern_matches(&localhost, "localhost", 2222));
        let with_port = format!("|1|{salt}|lcglcTeJkunm6KiMrUoIY92uQOY=");
        assert!(pattern_matches(&with_port, "10.0.0.1", 2222));
        assert_eq!(
            hash_host_name("localhost", 22, &B64.decode(salt).unwrap()),
            localhost
        );
    }

    #[test]
    fn parse_file_with_markers_and_errors() {
        let blob = B64.encode(ED_BLOB_A);
        let text = format!(
            "# comment\n\
             \n\
             host1,10.0.0.1 ssh-ed25519 {blob} comment here\n\
             @cert-authority *.corp ssh-ed25519 {blob}\n\
             @revoked bad.example ssh-ed25519 {blob}\n\
             |1|Hkq1R2n5S0Q8Q1l5gC4uDEwMW3o=|mB0nLVkHMGZ2fq6mJbDqNlXtDwQ= ssh-ed25519 {blob}\n\
             @bogus x ssh-ed25519 {blob}\n\
             onlyhost\n\
             badkey ssh-ed25519 !!!notbase64\n"
        );
        let f = parse_known_hosts(&text);
        assert_eq!(f.lines.len(), 4, "{:?}", f.errors);
        assert_eq!(f.errors.len(), 3);
        assert_eq!(f.lines[0].comment.as_deref(), Some("comment here"));
        assert_eq!(f.lines[1].marker, Some(Marker::CertAuthority));
        assert_eq!(f.lines[2].marker, Some(Marker::Revoked));
        assert!(f.lines[3].patterns.starts_with("|1|"));

        let (hosts, errors) = import_known_hosts(&text);
        assert_eq!(hosts.len(), 4);
        assert_eq!(errors.len(), 3);
        assert!(hosts[1].is_cert_authority());
        assert_eq!(hosts[1].source, KnownHostSource::Imported);
        assert!(hosts[2].revoked);
        assert!(hosts[0].fingerprint_sha256.starts_with("SHA256:"));

        let exported = export_known_hosts(&hosts);
        let reparsed = parse_known_hosts(&exported);
        assert_eq!(reparsed.lines.len(), 4);
        assert!(reparsed.errors.is_empty());
        assert_eq!(reparsed.lines[1].marker, Some(Marker::CertAuthority));
        assert_eq!(reparsed.lines[2].marker, Some(Marker::Revoked));
    }

    #[test]
    fn check_trusted_unknown_changed_revoked() {
        let known = entry("x", "ssh-ed25519", ED_BLOB_A, KnownHostSource::Tofu);
        assert_eq!(
            check_host_key(std::slice::from_ref(&known), "ssh-ed25519", ED_BLOB_A, None),
            HostKeyCheck::Trusted {
                via_cert_authority: false
            }
        );
        match check_host_key(std::slice::from_ref(&known), "ssh-ed25519", ED_BLOB_B, None) {
            HostKeyCheck::Changed {
                expected_fingerprints,
            } => assert_eq!(
                expected_fingerprints,
                vec![known.fingerprint_sha256.clone()]
            ),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            check_host_key(&[], "ssh-ed25519", ED_BLOB_A, None),
            HostKeyCheck::Unknown {
                other_key_types: vec![]
            }
        );
        let rsa = entry("x", "ssh-rsa", b"rsa-blob", KnownHostSource::Imported);
        assert_eq!(
            check_host_key(&[rsa], "ssh-ed25519", ED_BLOB_A, None),
            HostKeyCheck::Unknown {
                other_key_types: vec!["ssh-rsa".into()]
            }
        );
        let mut revoked = entry("x", "ssh-ed25519", ED_BLOB_A, KnownHostSource::Manual);
        revoked.revoked = true;
        assert_eq!(
            check_host_key(&[known, revoked], "ssh-ed25519", ED_BLOB_A, None),
            HostKeyCheck::Revoked
        );
    }

    #[test]
    fn check_certificate_authority() {
        let ca = entry(
            "*",
            "ssh-ed25519",
            ED_BLOB_B,
            KnownHostSource::CertAuthority,
        );
        let cert = PresentedCertificate {
            ca_key_blob: ED_BLOB_B.to_vec(),
            valid_for_host: true,
        };
        assert_eq!(
            check_host_key(
                std::slice::from_ref(&ca),
                "ssh-ed25519",
                ED_BLOB_A,
                Some(&cert)
            ),
            HostKeyCheck::Trusted {
                via_cert_authority: true
            }
        );
        // invalid cert (e.g. wrong principal) falls back to plain-key rules
        let bad = PresentedCertificate {
            valid_for_host: false,
            ..cert.clone()
        };
        assert!(matches!(
            check_host_key(
                std::slice::from_ref(&ca),
                "ssh-ed25519",
                ED_BLOB_A,
                Some(&bad)
            ),
            HostKeyCheck::Unknown { .. }
        ));
        // A CA entry is never treated as a plain host key.
        assert!(matches!(
            check_host_key(std::slice::from_ref(&ca), "ssh-ed25519", ED_BLOB_B, None),
            HostKeyCheck::Unknown { .. }
        ));
        // revoked CA
        let mut revoked_ca = ca;
        revoked_ca.revoked = true;
        assert_eq!(
            check_host_key(&[revoked_ca], "ssh-ed25519", ED_BLOB_A, Some(&cert)),
            HostKeyCheck::Revoked
        );
    }

    #[test]
    fn rsa_variants_normalize() {
        let known = entry("x", "ssh-rsa", b"rsa-a", KnownHostSource::Tofu);
        assert!(matches!(
            check_host_key(&[known], "rsa-sha2-512", b"rsa-b", None),
            HostKeyCheck::Changed { .. }
        ));
    }
}
