//! The caller supplies an already verified, dedicated Runner SSH session. This
//! module has no resolver and cannot choose a different route or host key policy.
use crate::{AutomationError, HelperPeer, Result, TargetHandler};
use cc_ssh_core::SshSession;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteHelperConfig {
    pub python: String,
    pub module_dir: String,
    pub root: String,
}

fn quote_path(value: &str) -> Result<String> {
    if !value.starts_with('/') || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(AutomationError::Invalid(
            "remote helper paths must be absolute Unix paths",
        ));
    }
    Ok(format!("'{}'", value.replace('\'', "'\"'\"'")))
}

impl RemoteHelperConfig {
    pub(crate) fn private_validation_command(&self, operation: uuid::Uuid) -> Result<String> {
        crate::dependency_launch::DependencyExpectation::validation(&self.root, operation)?;
        Ok(format!("exec {} -I -c {} {} --validation-stdio --root {} --operation-id {}",quote_path(&self.python)?,
            "'import sys,runpy;sys.path.insert(0,sys.argv.pop(1));runpy.run_module(\"consolecrypt_runner\",run_name=\"__main__\")'",
            quote_path(&self.module_dir)?,quote_path(&self.root)?,operation))
    }
    pub(crate) fn dependency_command(
        &self,
        operation: uuid::Uuid,
    ) -> Result<(String, crate::dependency_launch::DependencyExpectation)> {
        let expected = crate::dependency_launch::DependencyExpectation::new(&self.root, operation)?;
        let command = format!("exec {} -I -c {} {} --dependency-stdio --root {} --operation-id {}",
            quote_path(&self.python)?,
            "'import sys,runpy;sys.path.insert(0,sys.argv.pop(1));runpy.run_module(\"consolecrypt_runner\",run_name=\"__main__\")'",
            quote_path(&self.module_dir)?, quote_path(&self.root)?, operation);
        Ok((command, expected))
    }

    /// Consumes a newly connected session, never an Arc borrowed from a Runner,
    /// terminal or shared pool. Its lifetime belongs to this dependency only.
    pub async fn connect_dependency(
        &self,
        session: SshSession,
        operation: uuid::Uuid,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<crate::DependencyHelperConnection> {
        let mut remote = crate::dependency_launch::OwnedDependencySsh::new(session);
        let opened = async {
            let (command, expected) = self.dependency_command(operation)?;
            let channel = tokio::select! {biased;
                _=cancel.cancelled()=>return Err(AutomationError::Closed),
                result=tokio::time::timeout(std::time::Duration::from_secs(10),remote.session().open_exec(&command))=>
                    result.map_err(|_|AutomationError::Timeout)?.map_err(|_|AutomationError::Remote("dependency_ssh_exec".into()))?,
            };
            Ok((channel,expected))
        }.await;
        let (channel, expected) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                remote.close().await;
                return Err(error);
            }
        };
        let (reader, writer) = tokio::io::split(channel.into_stream());
        // No target handler, scope registry, enrollment or job connection map.
        let peer = HelperPeer::connect(reader, writer);
        crate::DependencyHelperConnection::remote(peer, remote, expected, cancel).await
    }

    pub fn command(&self) -> Result<String> {
        // SSH exec carries one POSIX command string, unlike local argv. Quote
        // every configured path once; no project content is part of this command.
        Ok(format!("exec {} -I -c {} {} --stdio --root {}",
            quote_path(&self.python)?,
            "'import sys,runpy;sys.path.insert(0,sys.argv.pop(1));runpy.run_module(\"consolecrypt_runner\",run_name=\"__main__\")'",
            quote_path(&self.module_dir)?, quote_path(&self.root)?))
    }

    pub async fn connect(&self, session: &SshSession) -> Result<Arc<HelperPeer>> {
        self.connect_with_targets(session, None).await
    }

    pub async fn connect_with_targets(
        &self,
        session: &SshSession,
        targets: Option<Arc<dyn TargetHandler>>,
    ) -> Result<Arc<HelperPeer>> {
        let channel = session
            .open_exec(&self.command()?)
            .await
            .map_err(|_| AutomationError::Remote("runner_ssh_exec".into()))?;
        let (reader, writer) = tokio::io::split(channel.into_stream());
        Ok(HelperPeer::connect_with_targets(reader, writer, targets))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dependency_remote_command_has_fixed_mode_and_once_quoted_paths() {
        let config = RemoteHelperConfig {
            python: "/opt/python".into(),
            module_dir: "/home/a ' $(touch BAD)/src".into(),
            root: "/home/a ' $(touch BAD)/jobs".into(),
        };
        let id = uuid::Uuid::new_v4();
        let (command, expected) = config.dependency_command(id).unwrap();
        assert!(command.contains("--dependency-stdio --root "));
        assert!(command.contains(&quote_path(&config.root).unwrap()));
        assert!(command.contains(&quote_path(&config.module_dir).unwrap()));
        assert!(command.ends_with(&format!("--operation-id {id}")));
        assert_eq!(expected.operation, id);
        assert!(!config.command().unwrap().contains("--dependency-stdio"));
        assert!(config.dependency_command(uuid::Uuid::nil()).is_err());
    }

    #[test]
    fn remote_exec_quotes_paths_and_rejects_control_characters() {
        let mut config = RemoteHelperConfig {
            python: "/opt/python".into(),
            module_dir: "/home/runner/a ' $(touch BAD)".into(),
            root: "/home/runner/проекты".into(),
        };
        let command = config.command().unwrap();
        assert!(command.contains("'/home/runner/a '\"'\"' $(touch BAD)'"));
        config.root.push('\n');
        assert!(config.command().is_err());
        config.root = "-relative".into();
        assert!(config.command().is_err());
    }
}
