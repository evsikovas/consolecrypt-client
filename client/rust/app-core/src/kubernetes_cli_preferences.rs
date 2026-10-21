//! Device-local CLI preferences. These encrypted settings never become vault
//! objects, sync payloads, execution grants, or installation approvals.
use crate::kubernetes_cli::{self as cli, InstallMethod};
use crate::{AppCore, AppError, AppResult};
use cc_k8s_core::resources::ListQuery;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const KEY: &str = "kubernetes.flux.preferences.v1";

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FluxPreferences {
    pub manual_path: Option<String>,
    pub install_method: Option<InstallMethod>,
    pub dismiss_install_advice: bool,
    /// Context/namespace/Kustomization keys; paths stay on this device.
    pub diff_directories: BTreeMap<String, String>,
}
#[derive(Serialize)]
pub struct FluxPreferencesView {
    pub preferences: FluxPreferences,
    pub controller_version: Option<String>,
    pub path: FluxPathAdvice,
}
#[derive(Serialize)]
pub struct FluxPathAdvice {
    pub directory: String,
    pub present: bool,
    pub profile_file: Option<String>,
    pub line: Option<String>,
    pub can_add: bool,
}
impl std::fmt::Debug for FluxPreferences {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluxPreferences").finish_non_exhaustive()
    }
}
impl std::fmt::Debug for FluxPreferencesView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluxPreferencesView")
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for FluxPathAdvice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FluxPathAdvice").finish_non_exhaustive()
    }
}

impl FluxPreferences {
    /// Versions up to 0.8.5 stored the Flux binary chosen from discovery in
    /// the Windows verbatim form (`\\?\C:\…`) and showed it like that.
    /// Normalize on load and save; the path is canonicalized again (dunce)
    /// before every use, so both spellings resolve to the same binary.
    fn migrate_paths(mut self) -> Self {
        if let Some(path) = self.manual_path.as_mut() {
            *path = cc_platform_core::strip_verbatim_prefix(path);
        }
        for path in self.diff_directories.values_mut() {
            *path = cc_platform_core::strip_verbatim_prefix(path);
        }
        self
    }
}

fn validate(preferences: &FluxPreferences) -> AppResult<()> {
    fn valid_path(value: &str) -> bool {
        value.len() <= 8192
            && !value.chars().any(char::is_control)
            && Path::new(value).is_absolute()
    }
    if preferences
        .manual_path
        .as_deref()
        .is_some_and(|value| !valid_path(value))
        || preferences.diff_directories.len() > 200
        || preferences.diff_directories.iter().any(|(key, path)| {
            key.is_empty()
                || key.len() > 1024
                || key.chars().any(char::is_control)
                || !valid_path(path)
        })
    {
        return Err(AppError::invalid("kubernetes", "cli_invalid"));
    }
    Ok(())
}
fn path_advice() -> AppResult<FluxPathAdvice> {
    let destination = cli::destination().map_err(super::kubernetes_flux::error)?;
    let directory = destination
        .parent()
        .ok_or_else(|| AppError::invalid("kubernetes", "cli_invalid"))?;
    let present = std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|path| {
            if cfg!(windows) {
                path.to_string_lossy()
                    .eq_ignore_ascii_case(&directory.to_string_lossy())
            } else {
                path == directory
            }
        })
    });
    let (profile_file, line) = if cfg!(windows) {
        (None, None)
    } else {
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| AppError::invalid("kubernetes", "cli_invalid"))?;
        let quoted = directory.to_string_lossy().replace('\'', "'\\''");
        (
            Some(
                home.join(if cfg!(target_os = "macos") {
                    ".zprofile"
                } else {
                    ".profile"
                })
                .to_string_lossy()
                .into_owned(),
            ),
            Some(format!("export PATH='{quoted}':\"$PATH\"")),
        )
    };
    Ok(FluxPathAdvice {
        directory: directory.to_string_lossy().into_owned(),
        present,
        profile_file,
        line,
        can_add: cfg!(windows),
    })
}
fn newest_controller_version<'a>(
    labels: impl Iterator<Item = &'a serde_json::Value>,
) -> Option<String> {
    labels
        .filter_map(|value| value.as_str())
        .filter_map(|value| semver::Version::parse(value.trim_start_matches('v')).ok())
        // Controller component versions (1.x) are not Flux bundle versions.
        .filter(|version| version.major == 2 && version.pre.is_empty() && version.build.is_empty())
        .max()
        .map(|version| version.to_string())
}

impl AppCore {
    pub async fn kube_flux_preferences(
        &self,
        session_id: Option<String>,
    ) -> AppResult<FluxPreferencesView> {
        let (owner, unlocked) = self.unlocked().await?;
        let preferences = owner
            .storage()?
            .setting_get::<FluxPreferences>(KEY)
            .await?
            .unwrap_or_default()
            .migrate_paths();
        validate(&preferences)?;
        let controller_version =
            if let Some(session_id) = session_id {
                let live = unlocked.kube.get(&session_id)?;
                if let Ok(kind) = live.resources.resolve("apps", "v1", "deployments") {
                    let query = ListQuery {
                        namespace: Some("flux-system".into()),
                        label_selector: Some("app.kubernetes.io/part-of=flux".into()),
                        limit: Some(100),
                        ..ListQuery::default()
                    };
                    match live.resources.list(&kind, &query).await {
                        Ok(page) => newest_controller_version(page.items.iter().map(|row| {
                            &row.object["metadata"]["labels"]["app.kubernetes.io/version"]
                        })),
                        Err(_) => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        Ok(FluxPreferencesView {
            preferences,
            controller_version,
            path: path_advice()?,
        })
    }
    pub async fn kube_flux_preferences_save(&self, preferences: FluxPreferences) -> AppResult<()> {
        let preferences = preferences.migrate_paths();
        validate(&preferences)?;
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        owner.storage()?.setting_set(KEY, preferences).await?;
        Ok(())
    }
    /// The directory is fixed by the native installer. UI cannot supply an
    /// arbitrary PATH entry, registry key, or command.
    pub async fn kube_flux_path_add(&self) -> AppResult<FluxPathAdvice> {
        let (owner, unlocked) = self.unlocked().await?;
        let _profile = self.hold_session(&owner).await?;
        let _generation = owner.hold_unlocked(&unlocked).await?;
        #[cfg(windows)]
        windows::add_user_path()?;
        #[cfg(not(windows))]
        return Err(AppError::invalid("kubernetes", "desktop_only"));
        #[cfg(windows)]
        {
            let mut advice = path_advice()?;
            advice.present = true;
            Ok(advice)
        }
    }
}

#[cfg(windows)]
#[path = "kubernetes_cli_windows_path.rs"]
mod windows;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_never_accept_execution_data_or_relative_paths() {
        assert!(serde_json::from_str::<FluxPreferences>(r#"{"approval":true}"#).is_err());
        assert!(validate(&FluxPreferences {
            manual_path: Some("relative/flux".into()),
            ..Default::default()
        })
        .is_err());
        assert!(validate(&FluxPreferences {
            diff_directories: BTreeMap::from([(
                "ctx/ns/app".into(),
                "/tmp/project\nexport".into()
            )]),
            ..Default::default()
        })
        .is_err());
    }
    #[test]
    fn stored_windows_verbatim_paths_are_migrated_to_the_displayed_form() {
        let stored = FluxPreferences {
            manual_path: Some(r"\\?\C:\ProgramData\chocolatey\bin\flux.exe".into()),
            diff_directories: BTreeMap::from([
                ("ctx/ns/app".into(), r"\\?\UNC\server\share\gitops".into()),
                ("ctx/ns/other".into(), r"D:\work\gitops".into()),
            ]),
            ..Default::default()
        }
        .migrate_paths();
        assert_eq!(
            stored.manual_path.as_deref(),
            Some(r"C:\ProgramData\chocolatey\bin\flux.exe")
        );
        assert_eq!(
            stored.diff_directories["ctx/ns/app"],
            r"\\server\share\gitops"
        );
        assert_eq!(stored.diff_directories["ctx/ns/other"], r"D:\work\gitops");
        let unix = FluxPreferences {
            manual_path: Some("/usr/local/bin/flux".into()),
            ..Default::default()
        }
        .migrate_paths();
        assert_eq!(unix.manual_path.as_deref(), Some("/usr/local/bin/flux"));
    }
    #[test]
    fn controller_advice_ignores_component_versions_and_prereleases() {
        let labels = serde_json::json!(["v1.7.0", "v2.9.6", "v2.9.5", "v3.0.0", "v2.10.0-rc.1"]);
        assert_eq!(
            newest_controller_version(labels.as_array().unwrap().iter()),
            Some("2.9.6".into())
        );
    }
    #[tokio::test]
    async fn encrypted_preferences_survive_unlock_without_sync_objects_or_consent() {
        let temp = tempfile::tempdir().unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            temp.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile("CLI preferences".into(), "test profile password".into())
            .await
            .unwrap();
        let before = app.list_kube_contexts().await.unwrap().len();
        let prefs = FluxPreferences {
            manual_path: Some(
                if cfg!(windows) {
                    "C:\\fixture\\flux.exe"
                } else {
                    "/fixture/flux"
                }
                .into(),
            ),
            install_method: Some(InstallMethod::OfficialRelease),
            dismiss_install_advice: true,
            ..Default::default()
        };
        app.kube_flux_preferences_save(prefs).await.unwrap();
        app.lock().await.unwrap();
        assert!(app.kube_flux_preferences(None).await.is_err());
        app.unlock_with_passphrase("test profile password".into())
            .await
            .unwrap();
        let view = app.kube_flux_preferences(None).await.unwrap();
        assert!(view.preferences.dismiss_install_advice);
        assert_eq!(app.list_kube_contexts().await.unwrap().len(), before);
        assert!(app
            .kube_flux_approve("forged-from-preferences".into())
            .await
            .is_err());
    }
}
