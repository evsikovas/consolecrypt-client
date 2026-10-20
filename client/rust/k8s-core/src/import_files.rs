//! Explicitly approved, bounded external-file reads for staged kubeconfig
//! imports. Call on a blocking worker with a captured unlocked vault scope.
//! Nothing here executes a command, writes a file or persists an approval.
use crate::import::{
    canonical_certificates, CertificateSource, ImportContext, ImportedAuth, SecretSource,
    MAX_CONFIG_BYTES,
};
use secrecy::SecretString;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Read,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FileError {
    #[error("Confirm the exact referenced files before importing")]
    ApprovalRequired,
    #[error("A referenced file is unavailable or is not a regular file")]
    Unavailable,
    #[error("Referenced files exceed the import size limit")]
    Size,
    #[error("A referenced file has invalid certificate or credential data")]
    InvalidData,
}

/// Metadata for the confirmation UI. No file is opened, even for a size check.
pub fn pending_paths(context: &ImportContext) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    let mut cert = |source: &CertificateSource| {
        if let CertificateSource::File(path) = source {
            paths.insert(path.clone());
        }
    };
    if let Some(source) = &context.certificate_authority {
        cert(source);
    }
    match &context.auth {
        ImportedAuth::ClientCertificate { certificate, .. } => cert(certificate),
        ImportedAuth::Oidc {
            certificate_authority: Some(source),
            ..
        } => cert(source),
        _ => {}
    }
    let secret = match &context.auth {
        ImportedAuth::Token(source) => Some(source),
        ImportedAuth::ClientCertificate { key, .. } => Some(key),
        _ => None,
    };
    if let Some(SecretSource::File(path)) = secret {
        paths.insert(path.clone());
    }
    paths
}

/// Consumes the exact native preview the user approved. A stale/different set
/// of paths fails before any read; callers must also recheck their vault scope
/// after the blocking worker and before committing the prepared object graph.
pub fn resolve(
    mut context: ImportContext,
    approved: &BTreeSet<PathBuf>,
) -> Result<ImportContext, FileError> {
    let paths = pending_paths(&context);
    if &paths != approved {
        return Err(FileError::ApprovalRequired);
    }
    let mut bytes = 0;
    let mut contents = BTreeMap::new();
    for path in paths {
        let value = read_regular(&path)?;
        bytes += value.len();
        if bytes > MAX_TOTAL_BYTES {
            return Err(FileError::Size);
        }
        contents.insert(path, value);
    }
    let certificate = |source: &mut CertificateSource| -> Result<(), FileError> {
        if let CertificateSource::File(path) = source {
            let value = contents.get(path).ok_or(FileError::ApprovalRequired)?;
            *source = CertificateSource::Pem(
                canonical_certificates(value).map_err(|_| FileError::InvalidData)?,
            );
        }
        Ok(())
    };
    let secret = |source: &mut SecretSource, token: bool| -> Result<(), FileError> {
        if let SecretSource::File(path) = source {
            let value = contents.get(path).ok_or(FileError::ApprovalRequired)?;
            let value = if token { value.trim() } else { value.as_str() };
            if value.is_empty()
                || value.contains('\0')
                || (token && value.chars().any(char::is_whitespace))
            {
                return Err(FileError::InvalidData);
            }
            *source = SecretSource::Inline(SecretString::from(value));
        }
        Ok(())
    };
    if let Some(source) = &mut context.certificate_authority {
        certificate(source)?;
    }
    match &mut context.auth {
        ImportedAuth::Token(source) => secret(source, true)?,
        ImportedAuth::ClientCertificate {
            certificate: source,
            key,
        } => {
            certificate(source)?;
            secret(key, false)?;
        }
        ImportedAuth::Oidc {
            certificate_authority: Some(source),
            ..
        } => certificate(source)?,
        _ => {}
    }
    Ok(context)
}

fn read_regular(path: &Path) -> Result<Zeroizing<String>, FileError> {
    if !path.is_absolute() {
        return Err(FileError::Unavailable);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Do not block on a FIFO or follow a replaced final symlink.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path).map_err(|_| FileError::Unavailable)?;
    let metadata = file.metadata().map_err(|_| FileError::Unavailable)?;
    if !metadata.is_file() {
        return Err(FileError::Unavailable);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(FileError::Unavailable);
        }
    }
    if metadata.len() > MAX_CONFIG_BYTES as u64 {
        return Err(FileError::Size);
    }
    let mut value = Zeroizing::new(Vec::new());
    file.take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut value)
        .map_err(|_| FileError::Unavailable)?;
    if value.len() > MAX_CONFIG_BYTES {
        return Err(FileError::Size);
    }
    let text = std::str::from_utf8(&value).map_err(|_| FileError::InvalidData)?;
    Ok(Zeroizing::new(text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{import::parse, vault_import::prepare};
    use cc_models::kubernetes::KubeRoute;

    fn context(path: &Path, extra_cluster: &str) -> ImportContext {
        let yaml = format!(
            "apiVersion: v1\nkind: Config\nclusters:\n- name: c\n  cluster:\n    server: https://cluster.test\n{extra_cluster}contexts:\n- name: c\n  context:\n    cluster: c\n    user: u\nusers:\n- name: u\n  user:\n    tokenFile: token\n"
        );
        parse(&yaml, Some(&path.join("config")))
            .unwrap()
            .entries
            .remove(0)
            .context
            .unwrap()
    }

    #[test]
    fn approval_is_exact_and_checked_before_io() {
        let dir = tempfile::tempdir().unwrap();
        // Both paths are nonexistent. Permission must fail before IO.
        let value = context(dir.path(), "    certificate-authority: ca.pem\n");
        let mut approved = pending_paths(&value);
        approved.remove(&dir.path().join("ca.pem"));
        assert_eq!(
            resolve(value, &approved).unwrap_err(),
            FileError::ApprovalRequired
        );
        let value = context(dir.path(), "");
        let mut approved = pending_paths(&value);
        approved.insert(dir.path().join("other"));
        assert_eq!(
            resolve(value, &approved).unwrap_err(),
            FileError::ApprovalRequired
        );
    }

    #[test]
    fn approved_file_becomes_an_encrypted_secret_without_modifying_source() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("token"), "synthetic-file-token\n").unwrap();
        let value = context(dir.path(), "");
        let approved = pending_paths(&value);
        let value = resolve(value, &approved).unwrap();
        assert!(pending_paths(&value).is_empty());
        let prepared = prepare(value, true, KubeRoute::Direct).unwrap();
        assert_eq!(
            prepared.secrets[0].value.expose_secret(),
            "synthetic-file-token"
        );
        assert!(!serde_json::to_string(&prepared.context)
            .unwrap()
            .contains("synthetic-file-token"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("token")).unwrap(),
            "synthetic-file-token\n"
        );
    }

    #[test]
    fn approved_ca_is_validated_and_surrounding_text_is_not_imported() {
        let dir = tempfile::tempdir().unwrap();
        let cert = rcgen::generate_simple_self_signed(vec!["cluster.test".into()])
            .unwrap()
            .cert
            .pem();
        std::fs::write(dir.path().join("token"), "synthetic").unwrap();
        std::fs::write(
            dir.path().join("ca.pem"),
            format!("private-comment\n{cert}"),
        )
        .unwrap();
        let value = context(dir.path(), "    certificate-authority: ca.pem\n");
        let value = resolve(
            value,
            &pending_paths(&context(dir.path(), "    certificate-authority: ca.pem\n")),
        )
        .unwrap();
        let prepared = prepare(value, true, KubeRoute::Direct).unwrap();
        assert_eq!(
            prepared
                .cluster
                .certificate_authority_pem
                .unwrap()
                .replace("\r\n", "\n"),
            cert
        );
    }

    #[test]
    fn invalid_and_oversized_files_fail_without_leaking_their_contents() {
        let dir = tempfile::tempdir().unwrap();
        for (data, expected) in [
            (vec![b'x'; MAX_CONFIG_BYTES + 1], FileError::Size),
            (vec![255], FileError::InvalidData),
            (
                b"synthetic\r\nInjected: value".to_vec(),
                FileError::InvalidData,
            ),
        ] {
            std::fs::write(dir.path().join("token"), data).unwrap();
            let value = context(dir.path(), "");
            let approved = pending_paths(&value);
            let error = resolve(value, &approved).unwrap_err();
            assert_eq!(error, expected);
            assert!(!format!("{error:?}").contains("synthetic"));
        }
        assert_eq!(
            read_regular(dir.path()).unwrap_err(),
            FileError::Unavailable
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_substitution_does_not_read_the_new_target() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("target"), "unapproved").unwrap();
        std::os::unix::fs::symlink(dir.path().join("target"), dir.path().join("token")).unwrap();
        let value = context(dir.path(), "");
        let approved = pending_paths(&value);
        assert_eq!(
            resolve(value, &approved).unwrap_err(),
            FileError::Unavailable
        );
    }
}
