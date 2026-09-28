//! Validation of user text before it reaches SQLite, the server or the
//! file system: NUL / control characters, length limits, shapes.

use crate::error::{AppError, AppResult};
use std::path::PathBuf;

/// Maximum display-name length (profiles, vaults) in characters.
pub const MAX_DISPLAY_NAME_CHARS: usize = 128;
/// RFC 5321 path limit.
pub const MAX_EMAIL_LEN: usize = 254;
/// Maximum server URL length.
pub const MAX_URL_LEN: usize = 2048;
/// Maximum file path length accepted from the UI.
pub const MAX_PATH_LEN: usize = 4096;

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// Display name (profile, vault, device): trimmed, 1..=128 chars, no
/// control characters (incl. NUL).
pub fn display_name(field: &str, s: &str) -> AppResult<String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(AppError::invalid(field.to_owned(), "must not be empty"));
    }
    if has_control(t) {
        return Err(AppError::invalid(
            field.to_owned(),
            "must not contain control characters",
        ));
    }
    if t.chars().count() > MAX_DISPLAY_NAME_CHARS {
        return Err(AppError::invalid(
            field.to_owned(),
            format!("at most {MAX_DISPLAY_NAME_CHARS} characters"),
        ));
    }
    Ok(t.to_owned())
}

/// Account email: trimmed, basic `local@domain.tld` shape, no whitespace
/// or control characters, ≤ 254 bytes.
pub fn email(s: &str) -> AppResult<String> {
    let t = s.trim();
    let bad = |r: &str| Err(AppError::invalid("email", r.to_owned()));
    if t.is_empty() || t.len() > MAX_EMAIL_LEN {
        return bad("must be an email address of at most 254 characters");
    }
    if has_control(t) || t.chars().any(char::is_whitespace) {
        return bad("must not contain spaces or control characters");
    }
    let Some((local, domain)) = t.split_once('@') else {
        return bad("must be an email address");
    };
    if local.is_empty()
        || domain.is_empty()
        || domain.contains('@')
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return bad("must be an email address");
    }
    Ok(t.to_owned())
}

/// Server URL: http(s) only, parsable, no control characters. (sync-core
/// additionally requires https except for loopback.)
pub fn server_url(s: &str) -> AppResult<String> {
    let t = s.trim();
    if t.is_empty() || t.len() > MAX_URL_LEN || has_control(t) {
        return Err(AppError::invalid("server_url", "not a valid URL"));
    }
    let lower = t.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(AppError::invalid("server_url", "must start with https://"));
    }
    Ok(t.to_owned())
}

/// Local file path from the UI/CLI: non-empty, no NUL/control characters.
pub fn file_path(field: &str, s: &str) -> AppResult<PathBuf> {
    if s.trim().is_empty() || s.len() > MAX_PATH_LEN {
        return Err(AppError::invalid(field.to_owned(), "must be a file path"));
    }
    if has_control(s) {
        return Err(AppError::invalid(
            field.to_owned(),
            "must not contain control characters",
        ));
    }
    Ok(PathBuf::from(s))
}

/// Secrets typed by the user (passphrases, passwords): anything but NUL.
pub fn secret_text(field: &str, s: &str) -> AppResult<()> {
    if s.contains('\0') {
        return Err(AppError::invalid(field.to_owned(), "must not contain NUL"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NASTY: [&str; 5] = ["a\0b", "a\u{7}b", "line\nbreak", "tab\there", "\u{1b}[31m"];

    #[test]
    fn display_names() {
        assert_eq!(display_name("n", "  Work  ").unwrap(), "Work");
        assert!(display_name("n", "   ").is_err());
        assert!(display_name("n", &"x".repeat(129)).is_err());
        for s in NASTY {
            assert!(display_name("n", s).is_err(), "{s:?}");
        }
    }

    #[test]
    fn emails() {
        assert_eq!(email(" a@example.org ").unwrap(), "a@example.org");
        for s in [
            "",
            "no-at",
            "@example.org",
            "a@",
            "a@b",
            "a@@b.org",
            "a b@example.org",
            "a@example.org\0",
            "a\0@example.org",
            "a@exa\u{7}mple.org",
        ] {
            assert!(email(s).is_err(), "{s:?}");
        }
    }

    #[test]
    fn urls_and_paths() {
        assert!(server_url("https://sync.example.org").is_ok());
        assert!(server_url("http://localhost:8080").is_ok());
        for s in [
            "ftp://x",
            "sync.example.org",
            "https://a\0b",
            "https://a\nb",
            "",
        ] {
            assert!(server_url(s).is_err(), "{s:?}");
        }
        assert!(file_path("path", "/tmp/x.ccbackup").is_ok());
        for s in ["", "a\0b", "a\nb"] {
            assert!(file_path("path", s).is_err(), "{s:?}");
        }
        assert!(secret_text("p", "ok\tpass").is_ok());
        assert!(secret_text("p", "no\0pe").is_err());
    }
}
