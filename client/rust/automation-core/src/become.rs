//! One approved job's Native-only sudo input. No serde, file or Runner material.
use crate::{AutomationError, Result, TargetFailure};
use base64::{Engine, engine::general_purpose::STANDARD};
use cc_models::secret::SecretValue;
use cc_ssh_core::{SshSession, russh::ChannelMsg};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use zeroize::Zeroizing;

pub struct ScopedBecomePassword(Mutex<Option<(SecretValue, u16)>>);
impl std::fmt::Debug for ScopedBecomePassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScopedBecomePassword([redacted])")
    }
}
impl ScopedBecomePassword {
    pub fn new(password: SecretValue) -> Result<Self> {
        if password.is_empty()
            || password.len() > 4096
            || password.expose_secret().contains(['\0', '\r', '\n'])
        {
            return Err(AutomationError::Invalid("invalid one-shot sudo password"));
        }
        Ok(Self(Mutex::new(Some((password, 0)))))
    }
    pub fn revoke(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
    pub(crate) fn remember(&self, mask: &cc_ssh_core::DiagnosticCapture) -> Result<()> {
        let state = self.0.lock().map_err(|_| AutomationError::Closed)?;
        let (secret, _) = state.as_ref().ok_or(AutomationError::Closed)?;
        mask.remember(secret.expose_secret())
            .map_err(|_| AutomationError::Closed)
    }
    fn line(&self) -> std::result::Result<Zeroizing<Vec<u8>>, TargetFailure> {
        let mut state = self.0.lock().map_err(|_| TargetFailure::Cancelled)?;
        let (secret, uses) = state.as_mut().ok_or(TargetFailure::Cancelled)?;
        if *uses >= 1024 {
            return Err(TargetFailure::OutputLimit);
        }
        *uses += 1;
        let mut bytes = Zeroizing::new(secret.expose_secret().as_bytes().to_vec());
        bytes.push(b'\n');
        Ok(bytes)
    }
    fn output_safe(&self, bytes: &[u8]) -> bool {
        let Ok(state) = self.0.lock() else {
            return false;
        };
        let Some((secret, _)) = state.as_ref() else {
            return false;
        };
        position(bytes, secret.expose_secret().as_bytes()).is_none()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    method: String,
    user: String,
    command: String,
    stdin: String,
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
fn wrapper(
    input: &Request,
    token: &str,
) -> std::result::Result<(String, String, String), TargetFailure> {
    if input.method != "sudo"
        || input.user.is_empty()
        || input.user.len() > 64
        || input.user.starts_with('-')
        || !input
            .user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || input.command.is_empty()
        || input.command.len() > 64 * 1024
        || input.command.contains('\0')
        || token.len() != 32
        || !token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(TargetFailure::UnsupportedOperation);
    }
    let prompt = format!("CC-SUDO-{token}:");
    let success = format!("CC-SUDO-OK-{token}");
    let body = format!("printf '%s\\n' {}; {}", quote(&success), input.command);
    Ok((
        format!(
            "/usr/bin/sudo -H -S -p {} -u {} -- /bin/sh -c {}",
            quote(&prompt),
            quote(&input.user),
            quote(&body)
        ),
        prompt,
        success,
    ))
}

pub(crate) async fn execute(
    session: &Arc<SshSession>,
    body: Value,
    password: &ScopedBecomePassword,
) -> std::result::Result<Value, TargetFailure> {
    let input: Request =
        serde_json::from_value(body).map_err(|_| TargetFailure::UnsupportedOperation)?;
    if input.stdin.len() > 350000 {
        return Err(TargetFailure::OutputLimit);
    }
    let data = STANDARD
        .decode(&input.stdin)
        .map_err(|_| TargetFailure::UnsupportedOperation)?;
    if data.len() > 256 * 1024 {
        return Err(TargetFailure::OutputLimit);
    }
    let (command, prompt, success) = wrapper(&input, &uuid::Uuid::new_v4().simple().to_string())?;
    let mut channel = tokio::time::timeout(Duration::from_secs(15), session.open_exec(&command))
        .await
        .map_err(|_| TargetFailure::ConnectionLost)?
        .map_err(|_| TargetFailure::ConnectionLost)?;
    let operation=async {
        let (mut stdout,mut stderr)=(Zeroizing::new(Vec::new()),Zeroizing::new(Vec::new()));let mut sent=false;let mut ready=false;let mut exit=None;
        let until=tokio::time::Instant::now()+Duration::from_secs(20);
        while let Some(message)=if ready{channel.wait().await}else{
            tokio::time::timeout_at(until,channel.wait()).await.map_err(|_|TargetFailure::OperationFailed)?
        } {
            match message {
                ChannelMsg::Data{data}=>stdout.extend_from_slice(&data),
                ChannelMsg::ExtendedData{data,..}=>stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus{exit_status}=>exit=Some(exit_status),
                ChannelMsg::Failure|ChannelMsg::ExitSignal{..}=>return Err(TargetFailure::OperationFailed),
                ChannelMsg::Close=>break,_=>{},
            }
            if stdout.len()>256*1024 || stderr.len()>256*1024{return Err(TargetFailure::OutputLimit)}
            if !ready {
                match observe_auth(&mut stdout,&mut stderr,&prompt,&success,sent)? {
                  AuthAction::Ready => {
                    ready=true;
                    if !data.is_empty(){channel.data(data.as_slice()).await.map_err(|_|TargetFailure::ConnectionLost)?;}
                    channel.eof().await.map_err(|_|TargetFailure::ConnectionLost)?;
                  }, AuthAction::Password => {
                    let line=password.line()?;channel.data(line.as_slice()).await.map_err(|_|TargetFailure::ConnectionLost)?;sent=true;
                  }, AuthAction::Wait=>{},
                }
                if stdout.len()+stderr.len()>32*1024{return Err(TargetFailure::OutputLimit)}
            }
        }
        if !ready{return Err(TargetFailure::OperationFailed)}
        // Fail closed rather than rewriting Ansible's JSON/module bytes: raw
        // echoed input never reaches the Runner, events or persisted artifacts.
        if !password.output_safe(&stdout) || !password.output_safe(&stderr){return Err(TargetFailure::OperationFailed)}
        let code=exit.filter(|n|*n<=255).ok_or(TargetFailure::ConnectionLost)?;
        Ok(json!({"exit_code":code,"stdout":STANDARD.encode(stdout.as_slice()),"stderr":STANDARD.encode(stderr.as_slice())}))
    }.await;
    if operation.is_err() {
        let _ = channel.close().await;
    }
    operation
}
#[derive(Debug, PartialEq, Eq)]
enum AuthAction {
    Wait,
    Password,
    Ready,
}
fn observe_auth(
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    prompt: &str,
    success: &str,
    sent: bool,
) -> std::result::Result<AuthAction, TargetFailure> {
    if let Some(at) = position(stdout, success.as_bytes()) {
        let end = at + success.len();
        let end = if stdout.get(end) == Some(&b'\r') {
            end + 1
        } else {
            end
        };
        let end = if stdout.get(end) == Some(&b'\n') {
            end + 1
        } else {
            end
        };
        stdout.drain(at..end);
        strip(stderr, prompt.as_bytes());
        strip(stdout, prompt.as_bytes());
        return Ok(AuthAction::Ready);
    }
    if stdout.len() + stderr.len() > 32 * 1024 {
        return Err(TargetFailure::OutputLimit);
    }
    if position(stderr, prompt.as_bytes()).is_some()
        || position(stdout, prompt.as_bytes()).is_some()
    {
        if sent {
            return Err(TargetFailure::OperationFailed);
        }
        strip(stdout, prompt.as_bytes());
        strip(stderr, prompt.as_bytes());
        return Ok(AuthAction::Password);
    }
    Ok(AuthAction::Wait)
}
fn position(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).position(|part| part == needle)
}
fn strip(bytes: &mut Vec<u8>, needle: &[u8]) {
    while let Some(at) = position(bytes, needle) {
        bytes.drain(at..at + needle.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sudo_command_is_fixed_and_quotes_untrusted_module_text() {
        let r = Request {
            method: "sudo".into(),
            user: "root".into(),
            command: "printf 'literal'; id".into(),
            stdin: String::new(),
        };
        let (cmd, prompt, ok) = wrapper(&r, &"a".repeat(32)).unwrap();
        assert!(cmd.starts_with("/usr/bin/sudo -H -S -p 'CC-SUDO-"));
        assert!(cmd.contains("-u 'root' -- /bin/sh -c '"));
        assert!(cmd.contains("'\"'\"'"));
        assert_ne!(prompt, ok);
    }
    #[test]
    fn unsupported_method_or_user_cannot_select_a_program() {
        for (method, user) in [("su", "root"), ("sudo", "root; id"), ("sudo", "-root")] {
            assert!(
                wrapper(
                    &Request {
                        method: method.into(),
                        user: user.into(),
                        command: "id".into(),
                        stdin: String::new()
                    },
                    &"a".repeat(32)
                )
                .is_err()
            );
        }
    }
    #[test]
    fn input_is_redacted_bounded_and_revocable() {
        let secret =
            ScopedBecomePassword::new(SecretValue::new("synthetic-private-input")).unwrap();
        assert!(!format!("{secret:?}").contains("synthetic-private-input"));
        let mask = cc_ssh_core::DiagnosticCapture::new();
        secret.remember(&mask).unwrap();
        assert_eq!(
            mask.redact("synthetic-private-input").unwrap(),
            "[REDACTED]"
        );
        assert_eq!(
            secret.line().unwrap().as_slice(),
            b"synthetic-private-input\n"
        );
        secret.revoke();
        assert!(secret.line().is_err());
        assert!(ScopedBecomePassword::new(SecretValue::new("bad\nline")).is_err());
    }
    #[test]
    fn chunked_prompt_requests_one_line_and_retry_is_denied() {
        let (mut out, mut err) = (vec![], b"native-".to_vec());
        assert_eq!(
            observe_auth(
                &mut out,
                &mut err,
                "native-prompt:",
                "native-success",
                false
            )
            .unwrap(),
            AuthAction::Wait
        );
        err.extend_from_slice(b"prompt:");
        assert_eq!(
            observe_auth(
                &mut out,
                &mut err,
                "native-prompt:",
                "native-success",
                false
            )
            .unwrap(),
            AuthAction::Password
        );
        assert!(err.is_empty());
        err.extend_from_slice(b"native-prompt:");
        assert!(matches!(
            observe_auth(&mut out, &mut err, "native-prompt:", "native-success", true),
            Err(TargetFailure::OperationFailed)
        ));
    }
    #[test]
    fn passwordless_success_keeps_module_bytes_without_requesting_secret() {
        let (mut out, mut err) = (b"native-success\r\n{\"changed\":false}".to_vec(), vec![]);
        assert_eq!(
            observe_auth(
                &mut out,
                &mut err,
                "native-prompt:",
                "native-success",
                false
            )
            .unwrap(),
            AuthAction::Ready
        );
        assert_eq!(out, b"{\"changed\":false}");
    }
    #[test]
    fn unbounded_preauth_output_fails_before_password_release() {
        assert!(matches!(
            observe_auth(
                &mut vec![b'x'; 32769],
                &mut vec![],
                "prompt",
                "success",
                false
            ),
            Err(TargetFailure::OutputLimit)
        ));
    }
    #[test]
    fn echoed_input_never_admitted_into_runner_result() {
        let secret =
            ScopedBecomePassword::new(SecretValue::new("synthetic-private-input")).unwrap();
        assert!(!secret.output_safe(b"prefix synthetic-private-input\n suffix"));
        assert!(secret.output_safe(b"ordinary module JSON"));
        secret.revoke();
        assert!(!secret.output_safe(b"ordinary module JSON"));
    }
}
