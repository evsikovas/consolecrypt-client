//! Ansible Vault 1.1/1.2 file compatibility, not ConsoleCrypt vault encryption.
//! No subprocess, remote connection, plaintext password file export or AI path.
use super::*;
use aes::cipher::{KeyIvInit, StreamCipher};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

const HEADER: &str = "$ANSIBLE_VAULT;1.1;AES256";
const MAX_CLEAR: usize = 60 * 1024;
type Cipher = ctr::Ctr128BE<aes::Aes256>;
fn invalid() -> AppError { AppError::invalid("automation_vault", "vault_file_or_password_invalid") }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    version: u8,
    profile_id: String,
    notification_scope_id: String,
    project_id: String,
    project_revision: u64,
    path: String,
    expected_sha256: String,
    action: Action,
    password_source: PasswordSource,
}
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
enum Action { Encrypt, Decrypt }
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
enum PasswordSource { Manual, ProjectFile }

fn keys(password: &[u8], salt: &[u8]) -> AppResult<Zeroizing<[u8; 80]>> {
    if password.is_empty() || password.len() > 4096 || salt.len() != 32 { return Err(invalid()); }
    let mut result = Zeroizing::new([0u8; 80]);
    pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, 10_000, &mut result[..]);
    Ok(result)
}
fn encrypt(clear: &[u8], password: &[u8], salt: &[u8; 32]) -> AppResult<String> {
    if clear.len() > MAX_CLEAR || clear.starts_with(b"$ANSIBLE_VAULT;") { return Err(invalid()); }
    let derived = keys(password, salt)?;
    let padding = 16 - clear.len() % 16;
    let mut encrypted = Zeroizing::new(Vec::with_capacity(clear.len() + padding));
    encrypted.extend_from_slice(clear);
    encrypted.resize(clear.len() + padding, padding as u8);
    let mut cipher = Cipher::new_from_slices(&derived[..32], &derived[64..]).map_err(|_| invalid())?;
    cipher.apply_keystream(&mut encrypted);
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&derived[32..64]).map_err(|_| invalid())?;
    mac.update(&encrypted);
    let body = format!("{}\n{}\n{}", hex::encode(salt), hex::encode(mac.finalize().into_bytes()), hex::encode(&*encrypted));
    let text = hex::encode(body);
    let mut result = String::from(HEADER);
    result.push('\n');
    for line in text.as_bytes().chunks(80) {
        result.push_str(std::str::from_utf8(line).map_err(|_| invalid())?);
        result.push('\n');
    }
    Ok(result)
}
fn decrypt(text: &str, password: &[u8]) -> AppResult<Zeroizing<String>> {
    if text.len() > 256 * 1024 { return Err(invalid()); }
    let mut lines = text.lines();
    let header = lines.next().ok_or_else(invalid)?;
    let fields: Vec<_> = header.split(';').collect();
    if !(fields.as_slice() == ["$ANSIBLE_VAULT", "1.1", "AES256"] ||
        fields.len() == 4 && fields[..3] == ["$ANSIBLE_VAULT", "1.2", "AES256"] &&
        !fields[3].is_empty() && fields[3].len() <= 128 && !fields[3].chars().any(char::is_control)) {
        return Err(invalid());
    }
    let outer: String = lines.map(str::trim).collect();
    let body = hex::decode(outer).map_err(|_| invalid())?;
    let pieces: Vec<_> = body.split(|c| *c == b'\n').collect();
    if pieces.len() != 3 { return Err(invalid()); }
    let salt = hex::decode(pieces[0]).map_err(|_| invalid())?;
    let tag = hex::decode(pieces[1]).map_err(|_| invalid())?;
    let mut clear = Zeroizing::new(hex::decode(pieces[2]).map_err(|_| invalid())?);
    if tag.len() != 32 || clear.is_empty() || clear.len() % 16 != 0 || clear.len() > MAX_CLEAR + 16 { return Err(invalid()); }
    let derived = keys(password, &salt)?;
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(&derived[32..64]).map_err(|_| invalid())?;
    mac.update(&clear);
    mac.verify_slice(&tag).map_err(|_| invalid())?;
    let mut cipher = Cipher::new_from_slices(&derived[..32], &derived[64..]).map_err(|_| invalid())?;
    cipher.apply_keystream(&mut clear);
    let padding = *clear.last().ok_or_else(invalid)? as usize;
    if !(1..=16).contains(&padding) || !clear[clear.len()-padding..].iter().all(|b| *b as usize == padding) { return Err(invalid()); }
    let plaintext_len = clear.len() - padding;
    clear.truncate(plaintext_len);
    if clear.len()>MAX_CLEAR {return Err(invalid());}
    Ok(Zeroizing::new(std::str::from_utf8(&clear).map_err(|_|invalid())?.to_owned()))
}

impl AppCore {
    /// Original-context launch input. The fixed local password is deliberately
    /// absent from revision manifests, editor JSON and Runner snapshot uploads.
    pub(crate) async fn automation_project_vault_file_password(
        &self, context:AutomationProjectContext, root:Arc<managed::OpenedProjectRoot>,
        revision:ProjectRevision,cancel:&tokio_util::sync::CancellationToken,
    )->AppResult<cc_models::secret::SecretValue> {
        context.check_after_io(self).await?;
        if cancel.is_cancelled() {return Err(invalid());}
        let retained=managed::admit_io(self,&context).await?;let slot=retained.clone();
        let directory=context.path.clone();let stopped=cancel.clone();
        let password=tokio::task::spawn_blocking(move||{
            let _slot=slot;
            if stopped.is_cancelled() {return Err(invalid());}
            root.recheck()?;root.reopen_same_directory(&directory).map_err(automation_error)?;
            if root.revision().map_err(automation_error)?!=revision {return Err(invalid());}
            let raw=Zeroizing::new(root.read_text(".vault/password").map_err(|_|invalid())?);
            let value=raw.trim_ascii();
            if value.is_empty() || value.len()>4096 || value.chars().any(|c|c.is_control() || matches!(c,'\u{2028}'|'\u{2029}')) {
                return Err(invalid());
            }
            root.recheck()?;root.reopen_same_directory(&directory).map_err(automation_error)?;
            if stopped.is_cancelled() {return Err(invalid());}
            Ok(cc_models::secret::SecretValue::new(value.to_owned()))
        }).await.map_err(|_|invalid())??;
        context.check_after_io(self).await?;
        if cancel.is_cancelled() {return Err(invalid());}
        Ok(password)
    }
    /// A user-authorized transform of one exact saved YAML file. Password bytes
    /// are a separate typed argument, never request JSON or run parameters.
    pub async fn automation_vault_transform(&self, request_json: String, password: Option<Vec<u8>>) -> AppResult<ProjectFile> {
        let password = password.map(Zeroizing::new);
        if request_json.len() > 4096 { return Err(invalid()); }
        let request: Request = serde_json::from_str(&request_json).map_err(|_| invalid())?;
        if request.version != 1 || request.project_revision == 0 ||
            !cc_models::automation::is_digest(&request.expected_sha256) ||
            !(request.path.ends_with(".yml") || request.path.ends_with(".yaml")) { return Err(invalid()); }
        let context = self.automation_project_context(&request.project_id).await?;
        if context.project.read_only || context.project.revision != Some(request.project_revision) { return Err(invalid()); }
        {
            let u = context.unlock.upgrade().ok_or(AppError::VaultLocked)?;
            let _profile = self.hold_session(&context.owner).await?;
            let _unlock = context.owner.hold_unlocked(&u).await?;
            if request.profile_id != context.owner.profile_id.to_string() ||
                u.notices.scope_id().as_deref() != Some(request.notification_scope_id.as_str()) { return Err(invalid()); }
        }
        let retained = managed::admit_io(self, &context).await?;
        let slot = retained.clone();
        let selection = context.root_selection.clone();
        let directory = context.path.clone();
        let result = tokio::task::spawn_blocking(move || -> AppResult<ProjectFile> {
            let _slot = slot;
            let root = selection.open()?;
            root.reopen_same_directory(&directory).map_err(automation_error)?;
            let original = root.read_file(&request.path).map_err(automation_error)?;
            if original.sha256 != request.expected_sha256 { return Err(AppError::invalid("automation_file", "changed")); }
            let file_password;
            let secret = match request.password_source {
                PasswordSource::Manual => password.as_deref().ok_or_else(invalid)?.as_slice(),
                PasswordSource::ProjectFile => {
                    if password.is_some() { return Err(invalid()); }
                    file_password = Zeroizing::new(root.read_text(".vault/password").map_err(|_| invalid())?);
                    file_password.as_bytes().trim_ascii()
                }
            };
            if secret.is_empty() || secret.len() > 4096 { return Err(invalid()); }
            let output = match request.action {
                Action::Encrypt => {
                    let mut salt = [0u8; 32];
                    getrandom::fill(&mut salt).map_err(|_| invalid())?;
                    Zeroizing::new(encrypt(original.text.as_bytes(), secret, &salt)?)
                }
                Action::Decrypt => decrypt(&original.text, secret)?,
            };
            root.recheck()?;
            root.reopen_same_directory(&directory).map_err(automation_error)?;
            let saved = root.write_file(&request.path, Some(&request.expected_sha256), &output).map_err(automation_error)?;
            root.recheck()?;
            Ok(saved)
        }).await.map_err(|_| invalid())??;
        context.check_after_io(self).await?;
        drop(retained);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_roundtrip_rejects_wrong_password_and_tampering() {
        let clear = b"---\npassword: synthetic\n";
        let value = encrypt(clear, b"synthetic-passphrase", &[7u8;32]).unwrap();
        assert!(value.starts_with(HEADER));
        assert_eq!(decrypt(&value,b"synthetic-passphrase").unwrap().as_bytes(),clear);
        assert!(decrypt(&value,b"different").is_err());
        let tampered = value.replacen("37", "38", 1);
        assert!(decrypt(&tampered,b"synthetic-passphrase").is_err());
    }
    #[test]
    fn supports_vault_id_header_but_rejects_double_encryption_and_unknown_format() {
        let value=encrypt(b"a: 1\n",b"secret",&[4;32]).unwrap();
        assert_eq!(&*decrypt(&value.replacen(HEADER,"$ANSIBLE_VAULT;1.2;AES256;prod",1),b"secret").unwrap(),"a: 1\n");
        assert!(encrypt(value.as_bytes(),b"secret",&[3;32]).is_err());
        assert!(decrypt(&value.replacen("AES256","AES",1),b"secret").is_err());
        assert!(encrypt(&vec![0;MAX_CLEAR+1],b"secret",&[3;32]).is_err());
    }

    #[test]
    fn matches_independent_openssl_ctr_and_python_pbkdf2_hmac_vector() {
        // Synthetic fixture produced independently by hashlib + OpenSSL
        // enc -aes-256-ctr, using Ansible's documented payload layout.
        let body="303030313032303330343035303630373038303930613062306330643065306631303131313231333134313531363137313831393161316231633164316531660a396631313932623837643864306234383038613436613436373061396231316365353534383332356562346136366230376566663132616365316232616535330a62383063363036316533623436616531653335396537616162343231393365333462376164313339373736653230653336366232373866616132396233326262";
        let mut expected=format!("{HEADER}\n");
        for chunk in body.as_bytes().chunks(80) { expected.push_str(std::str::from_utf8(chunk).unwrap());expected.push('\n'); }
        let salt: [u8;32]=std::array::from_fn(|i|i as u8);
        assert_eq!(encrypt(b"---\nsynthetic: openssl-vector\n",b"fixture-secret",&salt).unwrap(),expected);
        assert_eq!(&*decrypt(&expected,b"fixture-secret").unwrap(),"---\nsynthetic: openssl-vector\n");
    }

    #[tokio::test]
    async fn file_transform_is_scoped_cas_and_password_is_not_a_snapshot_file() {
        let temporary=tempfile::tempdir().unwrap();
        let app=AppCore::new(crate::AppConfig::for_tests(temporary.path().to_string_lossy().into_owned())).unwrap();
        app.create_local_profile("Vault file fixture".into(),"synthetic profile password 42".into()).await.unwrap();
        let project=app.automation_project_create("Vault files".into(),None).await.unwrap();
        let original=app.automation_project_save(project.id.clone(),"vars.yml".into(),None,"---\nvalue: synthetic\n".into()).await.unwrap();
        let context=app.automation_project_context(&project.id).await.unwrap();
        let u=context.unlock.upgrade().unwrap();
        let mut request=json!({"version":1,"profileId":context.owner.profile_id.to_string(),"notificationScopeId":u.notices.scope_id().unwrap(),
            "projectId":project.id,"projectRevision":project.revision,"path":"vars.yml","expectedSha256":original.sha256,
            "action":"encrypt","passwordSource":"manual"});
        drop(u);
        let mut foreign=request.clone();foreign["notificationScopeId"]=json!(Uuid::new_v4().to_string());
        assert!(app.automation_vault_transform(foreign.to_string(),Some(b"fixture-secret".to_vec())).await.is_err());
        let encrypted=app.automation_vault_transform(request.to_string(),Some(b"fixture-secret".to_vec())).await.unwrap();
        assert!(app.automation_vault_transform(request.to_string(),Some(b"fixture-secret".to_vec())).await.is_err());
        request["action"]=json!("decrypt");request["expectedSha256"]=json!(encrypted.sha256);
        assert!(app.automation_vault_transform(request.to_string(),Some(b"wrong".to_vec())).await.is_err());
        assert_eq!(app.automation_project_file(project.id.clone(),"vars.yml".into()).await.unwrap().text,encrypted.text);
        std::fs::create_dir(context.path.join(".vault")).unwrap();
        std::fs::write(context.path.join(".vault/password"),b"fixture-secret\n").unwrap();
        request["passwordSource"]=json!("projectFile");
        assert_eq!(app.automation_vault_transform(request.to_string(),None).await.unwrap().text,original.text);
        assert!(!app.automation_project_files(project.id.clone()).await.unwrap().entries.iter().any(|e|e.path()==".vault/password"));
        assert!(app.automation_project_file(project.id.clone(),".vault/password".into()).await.is_err());
        assert!(ProjectRoot::open(&context.path).unwrap().revision_for_publication().is_err());
    }
}
