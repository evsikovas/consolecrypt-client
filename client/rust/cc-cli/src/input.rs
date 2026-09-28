//! Secret and confirmation input: environment variables for scripts, a
//! no-echo prompt on a TTY otherwise. Secret values are never printed.

use crate::CliError;
use std::io::{BufRead, IsTerminal, Write};

/// Env var of the vault passphrase.
pub const ENV_PASSPHRASE: &str = "CC_PASSPHRASE";
/// Env var of a new vault passphrase (change / reset).
pub const ENV_NEW_PASSPHRASE: &str = "CC_NEW_PASSPHRASE";
/// Env var of the account password.
pub const ENV_PASSWORD: &str = "CC_PASSWORD";
/// Env var of a credential password (`cred add-password`).
pub const ENV_SECRET: &str = "CC_SECRET";
/// Env var of an SSH key passphrase.
pub const ENV_KEY_PASSPHRASE: &str = "CC_KEY_PASSPHRASE";
/// Env var answering "password at connect" prompts (host auth mode
/// `PasswordPrompt`).
pub const ENV_SSH_PASSWORD: &str = "CC_SSH_PASSWORD";
/// Env var of the Recovery Key (24 words or QR payload).
pub const ENV_RECOVERY_KEY: &str = "CC_RECOVERY_KEY";

pub fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

/// Read a secret from `env` or prompt (no echo). `confirm` asks twice.
pub fn secret(env: &str, prompt: &str, confirm: bool) -> Result<String, CliError> {
    if let Ok(v) = std::env::var(env) {
        if !v.is_empty() {
            return Ok(v);
        }
    }
    if !stdin_is_tty() {
        return Err(CliError::Usage(format!(
            "{prompt} required: set {env} or run interactively"
        )));
    }
    let first = rpassword::prompt_password(format!("{prompt}: "))
        .map_err(|e| CliError::Usage(format!("cannot read input: {e}")))?;
    if confirm {
        let second = rpassword::prompt_password(format!("Repeat {prompt}: "))
            .map_err(|e| CliError::Usage(format!("cannot read input: {e}")))?;
        if first != second {
            return Err(CliError::Usage("the two entries differ".into()));
        }
    }
    Ok(first)
}

/// Optional secret: env var or (on a TTY) a prompt where empty = none.
pub fn optional_secret(env: &str, prompt: &str) -> Result<Option<String>, CliError> {
    if let Ok(v) = std::env::var(env) {
        return Ok(Some(v).filter(|v| !v.is_empty()));
    }
    if !stdin_is_tty() {
        return Ok(None);
    }
    let v = rpassword::prompt_password(format!("{prompt} (empty = none): "))
        .map_err(|e| CliError::Usage(format!("cannot read input: {e}")))?;
    Ok(Some(v).filter(|v| !v.is_empty()))
}

/// Ask a question on the terminal (stderr) and read one line.
pub fn ask(question: &str) -> Result<String, CliError> {
    if !stdin_is_tty() {
        return Err(CliError::Usage(format!(
            "'{question}' needs an interactive terminal"
        )));
    }
    eprint!("{question} ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| CliError::Usage(format!("cannot read input: {e}")))?;
    Ok(line.trim().to_owned())
}

/// Yes/no question (default no).
pub fn confirm(question: &str) -> Result<bool, CliError> {
    let a = ask(&format!("{question} [y/N]"))?;
    Ok(matches!(a.to_ascii_lowercase().as_str(), "y" | "yes"))
}
