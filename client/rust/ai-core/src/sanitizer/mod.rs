//! Context Sanitizer (CLIENT_SPEC §15).
//!
//! Every piece of text that can reach an LLM provider passes through a
//! [`SanitizerSession`], which is the **only** way to obtain a
//! [`SanitizedText`] — and provider requests ([`crate::provider::ChatRequest`],
//! [`crate::provider::EmbeddingRequest`]) only accept `SanitizedText`. So the
//! type system guarantees that nothing unsanitized is sent, for local and
//! remote providers alike.
//!
//! What is redacted depends on the [`PrivacyProfile`]:
//!
//! | Category | Strict | Standard | Local |
//! |---|---|---|---|
//! | Private keys / PEM / PuTTY key bodies | ✔ | ✔ | ✔ |
//! | Passwords (CLI flags, env, config, URLs, …) | ✔ | ✔ | ✔ |
//! | Known-format tokens, JWT, `Authorization`, cookies, AWS keys, secret-named keys | ✔ | ✔ | ✔ |
//! | Generic high-entropy strings | ✔ | ✔ | — |
//! | IPv4/IPv6, hostnames/FQDNs, usernames, DB names | ✔ | — | — |
//!
//! Placeholders are stable within a session (`<IP_1>` always means the same
//! address). Non-secret placeholders (`IP`, `HOST`, `USER`, `DB`) can be
//! re-hydrated locally in generated commands via [`SanitizerSession::rehydrate`];
//! secret placeholders (`PRIVATE_KEY`, `PEM`, `PASSWORD`, `SECRET`) are
//! **never** re-hydrated — the session does not even keep the secret values,
//! only keyed 64-bit hashes (to keep placeholders stable).

mod rules;

use crate::context::HostContext;
pub use cc_models::ai::PrivacyProfile;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::hash::BuildHasher;
use std::sync::{Arc, LazyLock};
use zeroize::Zeroizing;

/// Kind of redacted value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// SSH/PEM/PGP/PuTTY private key material.
    PrivateKey,
    /// Other PEM blocks (certificates, CSRs, public keys).
    Pem,
    Password,
    /// Tokens, API keys, JWTs, cookies, auth headers, high-entropy strings.
    Secret,
    Ip,
    Host,
    User,
    Database,
}

impl Category {
    /// Placeholder label (`<LABEL_n>`).
    pub const fn label(self) -> &'static str {
        match self {
            Category::PrivateKey => "PRIVATE_KEY",
            Category::Pem => "PEM",
            Category::Password => "PASSWORD",
            Category::Secret => "SECRET",
            Category::Ip => "IP",
            Category::Host => "HOST",
            Category::User => "USER",
            Category::Database => "DB",
        }
    }

    /// Secret categories are never re-hydrated and never stored.
    pub const fn is_secret(self) -> bool {
        matches!(
            self,
            Category::PrivateKey | Category::Pem | Category::Password | Category::Secret
        )
    }

    fn from_label(label: &str) -> Option<Self> {
        [
            Category::PrivateKey,
            Category::Pem,
            Category::Password,
            Category::Secret,
            Category::Ip,
            Category::Host,
            Category::User,
            Category::Database,
        ]
        .into_iter()
        .find(|c| c.label() == label)
    }

    /// Overlap resolution: higher wins.
    const fn priority(self) -> u8 {
        match self {
            Category::PrivateKey => 100,
            Category::Pem => 95,
            Category::Password => 90,
            Category::Secret => 80,
            Category::User => 40,
            Category::Ip => 35,
            Category::Host => 30,
            Category::Database => 20,
        }
    }
}

/// Text that has passed the sanitizer (or is a trusted static prompt
/// constant). The only type accepted in provider requests.
///
/// `Debug` shows the length only: prompts must never be logged.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct SanitizedText(String);

impl SanitizedText {
    /// Trusted, compile-time prompt text (system prompts, instructions).
    /// Only `&'static str` is accepted so runtime user data cannot be passed
    /// in by accident.
    pub fn from_static(s: &'static str) -> Self {
        Self(s.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SanitizedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SanitizedText(len={})", self.0.len())
    }
}

/// Builds a [`SanitizedText`] from static prompt fragments and sanitized
/// parts.
#[derive(Default)]
pub struct PromptBuilder(String);

impl PromptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append trusted static text.
    pub fn push_static(&mut self, s: &'static str) -> &mut Self {
        self.0.push_str(s);
        self
    }

    /// Append sanitized text.
    pub fn push(&mut self, s: &SanitizedText) -> &mut Self {
        self.0.push_str(&s.0);
        self
    }

    /// Append a number (counts, limits).
    pub fn push_number(&mut self, n: usize) -> &mut Self {
        self.0.push_str(&n.to_string());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn build(self) -> SanitizedText {
        SanitizedText(self.0)
    }
}

impl fmt::Debug for PromptBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PromptBuilder(len={})", self.0.len())
    }
}

/// Counts of redactions per category (no values).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizeReport {
    pub counts: BTreeMap<Category, usize>,
}

impl SanitizeReport {
    pub fn total(&self) -> usize {
        self.counts.values().sum()
    }

    pub fn count(&self, c: Category) -> usize {
        self.counts.get(&c).copied().unwrap_or(0)
    }

    /// Whether any secret category was redacted.
    pub fn redacted_secrets(&self) -> bool {
        self.counts.iter().any(|(c, n)| c.is_secret() && *n > 0)
    }
}

/// Result of [`SanitizerSession::rehydrate`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rehydrated {
    /// Text with non-secret placeholders replaced by the original values.
    pub text: String,
    /// Secret placeholders left in place (e.g. `<PASSWORD_1>`); the user has
    /// to fill them in before running anything.
    pub unresolved_secrets: Vec<String>,
    /// Placeholder-looking tokens that this session never issued.
    pub unknown_placeholders: Vec<String>,
}

impl fmt::Debug for Rehydrated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rehydrated")
            .field("len", &self.text.len())
            .field("unresolved_secrets", &self.unresolved_secrets)
            .field("unknown_placeholders", &self.unknown_placeholders)
            .finish()
    }
}

/// A single detection: byte range + category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Finding {
    pub start: usize,
    pub end: usize,
    pub category: Category,
}

/// Which detector groups run.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Detect {
    pub strict: bool,
    pub entropy: bool,
}

impl Detect {
    fn for_profile(p: PrivacyProfile) -> Self {
        match p {
            PrivacyProfile::Strict => Self {
                strict: true,
                entropy: true,
            },
            PrivacyProfile::Standard => Self {
                strict: false,
                entropy: true,
            },
            PrivacyProfile::Local => Self {
                strict: false,
                entropy: false,
            },
        }
    }
}

static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([A-Z][A-Z_]*?)_(\d{1,6})>").expect("valid regex"));

/// Reversible placeholder → original map (non-secret categories only).
/// Zeroized on drop.
#[derive(Default, Clone)]
struct ReverseMap(HashMap<String, Zeroizing<String>>);

/// Stateful sanitizer for one conversation / request. Keeps placeholders
/// stable and remembers non-secret originals for [`Self::rehydrate`].
pub struct SanitizerSession {
    profile: PrivacyProfile,
    detect: Detect,
    hasher: RandomState,
    /// (category, keyed hash of value) → placeholder.
    ids: HashMap<(Category, u64), String>,
    reverse: ReverseMap,
    counters: HashMap<Category, usize>,
    literals: Vec<(Category, Zeroizing<String>)>,
    total: SanitizeReport,
}

impl fmt::Debug for SanitizerSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SanitizerSession")
            .field("profile", &self.profile)
            .field("placeholders", &self.ids.len())
            .finish_non_exhaustive()
    }
}

impl SanitizerSession {
    pub fn new(profile: PrivacyProfile) -> Self {
        Self {
            profile,
            detect: Detect::for_profile(profile),
            hasher: RandomState::new(),
            ids: HashMap::new(),
            reverse: ReverseMap::default(),
            counters: HashMap::new(),
            literals: Vec::new(),
            total: SanitizeReport::default(),
        }
    }

    pub fn profile(&self) -> PrivacyProfile {
        self.profile
    }

    /// Register host metadata so that, under `Strict`, the host's name,
    /// address and username are tokenized wherever they appear (even short
    /// names like `prod-db` that no generic rule would catch).
    pub fn add_host_context(&mut self, ctx: &HostContext) {
        if !self.detect.strict {
            return;
        }
        // Display names are user-chosen labels; tokenize them unless they
        // are loopback / well-known public names.
        let name = ctx.name.trim();
        match rules::classify_host(name) {
            Some(c) => self.add_literal(c, name),
            None if name.contains(|c: char| !c.is_ascii_alphanumeric() && !".-_".contains(c)) => {
                self.add_literal(Category::Host, name);
            }
            None => {}
        }
        if let Some(c) = rules::classify_host(&ctx.address) {
            self.add_literal(c, &ctx.address);
        }
        if let Some(u) = &ctx.username {
            if !rules::is_generic_user(u) {
                self.add_literal(Category::User, u);
            }
        }
    }

    /// Register a literal to tokenize (only effective for non-secret
    /// categories under `Strict`; secrets are always detected by rules).
    pub fn add_literal(&mut self, category: Category, value: &str) {
        let v = value.trim();
        if v.len() < 2 || category.is_secret() {
            return;
        }
        if !self
            .literals
            .iter()
            .any(|(c, l)| *c == category && l.as_str() == v)
        {
            self.literals.push((category, Zeroizing::new(v.to_owned())));
        }
    }

    /// Sanitize `text` according to the session profile.
    pub fn sanitize(&mut self, text: &str) -> SanitizedText {
        self.sanitize_with_report(text).0
    }

    /// Sanitize and report what was redacted (counts only).
    pub fn sanitize_with_report(&mut self, text: &str) -> (SanitizedText, SanitizeReport) {
        let mut findings = Vec::new();
        rules::collect(text, self.detect, &mut findings);
        if self.detect.strict {
            rules::literals(text, &self.literals, &mut findings);
        }
        let accepted = resolve_overlaps(findings);
        let mut out = String::with_capacity(text.len());
        let mut report = SanitizeReport::default();
        let mut last = 0;
        for f in accepted {
            out.push_str(&text[last..f.start]);
            let ph = self.placeholder_for(f.category, &text[f.start..f.end]);
            out.push_str(&ph);
            *report.counts.entry(f.category).or_default() += 1;
            last = f.end;
        }
        out.push_str(&text[last..]);
        for (c, n) in &report.counts {
            *self.total.counts.entry(*c).or_default() += n;
        }
        (SanitizedText(out), report)
    }

    /// Cumulative redaction counts of everything sanitized in this session.
    pub fn report(&self) -> &SanitizeReport {
        &self.total
    }

    fn placeholder_for(&mut self, category: Category, value: &str) -> String {
        let key = (category, self.hasher.hash_one(value));
        if let Some(ph) = self.ids.get(&key) {
            let same = category.is_secret()
                || self
                    .reverse
                    .0
                    .get(ph)
                    .is_some_and(|orig| orig.as_str() == value);
            if same {
                return ph.clone();
            }
        }
        let n = self.counters.entry(category).or_default();
        *n += 1;
        let ph = format!("<{}_{}>", category.label(), n);
        self.ids.entry(key).or_insert_with(|| ph.clone());
        if !category.is_secret() {
            self.reverse
                .0
                .insert(ph.clone(), Zeroizing::new(value.to_owned()));
        }
        ph
    }

    /// Replace non-secret placeholders issued by this session with their
    /// original values. Secret placeholders stay and are listed.
    pub fn rehydrate(&self, text: &str) -> Rehydrated {
        rehydrate_with(&self.reverse, text)
    }

    /// An owned, `'static` re-hydrator (for streams).
    pub fn rehydrator(&self) -> Rehydrator {
        Rehydrator {
            map: Arc::new(self.reverse.clone()),
            pending: String::new(),
        }
    }
}

fn rehydrate_with(map: &ReverseMap, text: &str) -> Rehydrated {
    let mut unresolved = Vec::new();
    let mut unknown = Vec::new();
    let out = PLACEHOLDER.replace_all(text, |c: &regex::Captures<'_>| {
        let whole = c.get(0).map_or("", |m| m.as_str());
        if let Some(orig) = map.0.get(whole) {
            return orig.to_string();
        }
        match Category::from_label(&c[1]) {
            Some(cat) if cat.is_secret() => {
                if !unresolved.iter().any(|u| u == whole) {
                    unresolved.push(whole.to_owned());
                }
            }
            Some(_) if !unknown.iter().any(|u| u == whole) => unknown.push(whole.to_owned()),
            _ => {}
        }
        whole.to_owned()
    });
    Rehydrated {
        text: out.into_owned(),
        unresolved_secrets: unresolved,
        unknown_placeholders: unknown,
    }
}

/// Incremental re-hydration of streamed model output. Holds back a possibly
/// incomplete placeholder at the end of a chunk.
#[derive(Clone)]
pub struct Rehydrator {
    map: Arc<ReverseMap>,
    pending: String,
}

impl fmt::Debug for Rehydrator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Rehydrator { .. }")
    }
}

/// Longest possible placeholder, e.g. `<PRIVATE_KEY_999999>`.
const MAX_PLACEHOLDER: usize = 24;

impl Rehydrator {
    /// Rehydrate a whole text at once.
    pub fn rehydrate(&self, text: &str) -> Rehydrated {
        rehydrate_with(&self.map, text)
    }

    /// Feed a chunk; returns text that is safe to emit now.
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        // Hold back from the last '<' if it could still become a placeholder.
        let cut = match self.pending.rfind('<') {
            Some(i)
                if self.pending.len() - i < MAX_PLACEHOLDER
                    && !self.pending[i..].contains('>')
                    && self.pending[i + 1..]
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') =>
            {
                i
            }
            _ => self.pending.len(),
        };
        let ready: String = self.pending.drain(..cut).collect();
        rehydrate_with(&self.map, &ready).text
    }

    /// Flush the remainder at end of stream.
    pub fn finish(&mut self) -> String {
        let rest = std::mem::take(&mut self.pending);
        rehydrate_with(&self.map, &rest).text
    }
}

/// One-shot sanitization with a fresh session.
pub fn sanitize(profile: PrivacyProfile, text: &str) -> SanitizedText {
    SanitizerSession::new(profile).sanitize(text)
}

/// Sanitize a provider error message before it is stored in an error value
/// (defense in depth: providers sometimes echo parts of the request).
pub(crate) fn sanitize_for_error(text: &str) -> String {
    let mut t: String = text.chars().take(300).collect();
    if text.len() > t.len() {
        t.push('…');
    }
    sanitize(PrivacyProfile::Standard, &t).into_string()
}

pub(crate) use rules::classify_secret_key as rules_classify_secret_key;
pub(crate) use rules::opt_takes_value;

/// Secret spans (all "always redact" categories plus high-entropy strings),
/// non-overlapping, in order. Used by the snippet parameterizer so secrets
/// never end up as snippet text or defaults.
pub(crate) fn secret_spans(text: &str) -> Vec<(std::ops::Range<usize>, Category)> {
    let mut findings = Vec::new();
    rules::collect(
        text,
        Detect {
            strict: false,
            entropy: true,
        },
        &mut findings,
    );
    findings.retain(|f| f.category.is_secret());
    resolve_overlaps(findings)
        .into_iter()
        .map(|f| (f.start..f.end, f.category))
        .collect()
}

/// Host/IP/user spans (the Strict-only detectors), non-overlapping.
pub(crate) fn identity_spans(text: &str) -> Vec<(std::ops::Range<usize>, Category)> {
    let mut findings = Vec::new();
    rules::collect(
        text,
        Detect {
            strict: true,
            entropy: false,
        },
        &mut findings,
    );
    findings.retain(|f| matches!(f.category, Category::Ip | Category::Host | Category::User));
    resolve_overlaps(findings)
        .into_iter()
        .map(|f| (f.start..f.end, f.category))
        .collect()
}

fn resolve_overlaps(mut f: Vec<Finding>) -> Vec<Finding> {
    f.retain(|x| x.end > x.start);
    f.sort_by(|a, b| {
        b.category
            .priority()
            .cmp(&a.category.priority())
            .then((b.end - b.start).cmp(&(a.end - a.start)))
            .then(a.start.cmp(&b.start))
    });
    let mut acc: Vec<Finding> = Vec::with_capacity(f.len());
    for x in f {
        if acc.iter().all(|y| x.end <= y.start || x.start >= y.end) {
            acc.push(x);
        }
    }
    acc.sort_by_key(|x| x.start);
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_stable_and_rehydratable() {
        let mut s = SanitizerSession::new(PrivacyProfile::Strict);
        let a = s.sanitize("ssh admin@10.1.2.3 && ping 10.1.2.3 && ping 10.9.9.9");
        assert_eq!(
            a.as_str(),
            "ssh <USER_1>@<IP_1> && ping <IP_1> && ping <IP_2>"
        );
        let b = s.sanitize("again 10.9.9.9");
        assert_eq!(b.as_str(), "again <IP_2>");
        let r = s.rehydrate("ssh <USER_1>@<IP_2> -p 22 # <PASSWORD_3> <IP_77>");
        assert_eq!(r.text, "ssh admin@10.9.9.9 -p 22 # <PASSWORD_3> <IP_77>");
        assert_eq!(r.unresolved_secrets, vec!["<PASSWORD_3>"]);
        assert_eq!(r.unknown_placeholders, vec!["<IP_77>"]);
    }

    #[test]
    fn secrets_are_never_rehydrated_or_stored() {
        let mut s = SanitizerSession::new(PrivacyProfile::Local);
        let out = s.sanitize("export PGPASSWORD=hunter2hunter2 && psql");
        assert_eq!(out.as_str(), "export PGPASSWORD=<PASSWORD_1> && psql");
        assert_eq!(
            s.sanitize("PGPASSWORD=hunter2hunter2").as_str(),
            "PGPASSWORD=<PASSWORD_1>"
        );
        let r = s.rehydrate(out.as_str());
        assert_eq!(r.text, out.as_str());
        assert_eq!(r.unresolved_secrets, vec!["<PASSWORD_1>"]);
        assert!(s.reverse.0.is_empty());
        assert!(!format!("{s:?}").contains("hunter2"));
    }

    #[test]
    fn streaming_rehydrator_handles_split_placeholders() {
        let mut s = SanitizerSession::new(PrivacyProfile::Strict);
        s.sanitize("host 192.168.7.8");
        let mut r = s.rehydrator();
        let mut out = String::new();
        for chunk in ["ping <I", "P_", "1> now <", "b>", " <PASSWORD_1", "> x <"] {
            out.push_str(&r.push(chunk));
        }
        out.push_str(&r.finish());
        assert_eq!(out, "ping 192.168.7.8 now <b> <PASSWORD_1> x <");
    }

    #[test]
    fn sanitized_text_debug_hides_content() {
        let t = sanitize(PrivacyProfile::Standard, "hello world");
        assert_eq!(format!("{t:?}"), "SanitizedText(len=11)");
        let mut b = PromptBuilder::new();
        b.push_static("a").push(&t).push_number(3);
        assert_eq!(b.build().as_str(), "ahello world3");
    }

    #[test]
    fn overlap_resolution_prefers_priority_then_length() {
        let f = vec![
            Finding {
                start: 0,
                end: 10,
                category: Category::Host,
            },
            Finding {
                start: 5,
                end: 8,
                category: Category::Password,
            },
            Finding {
                start: 12,
                end: 20,
                category: Category::Ip,
            },
            Finding {
                start: 12,
                end: 15,
                category: Category::Ip,
            },
        ];
        let r = resolve_overlaps(f);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].category, Category::Password);
        assert_eq!((r[1].start, r[1].end), (12, 20));
    }
}
