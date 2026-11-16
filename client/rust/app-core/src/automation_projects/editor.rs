//! Explicit desktop editor action. It edits ordinary source files, not a Runner
//! snapshot, and cannot confirm/run a job or modify protected Git provenance.
use super::*;
use crate::edit::{AppRefDto, OpenWithDto};
use cc_sftp_core::edit::OpenWith;

impl AppCore {
    pub async fn automation_project_open_editor(
        &self,
        id: String,
        path: String,
        expected_sha256: String,
        editor: Option<AppRefDto>,
    ) -> AppResult<()> {
        if !cfg!(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux"
        )) {
            return Err(AppError::invalid(
                "automation_editor",
                "desktop editor required",
            ));
        }
        cc_automation_core::validate_project_path(&path).map_err(automation_error)?;
        if path == cc_automation_core::GIT_SOURCE_FILE
            || !cc_models::automation::is_digest(&expected_sha256)
        {
            return Err(AppError::invalid(
                "automation_editor",
                "invalid editable file",
            ));
        }
        let application = editor
            .map(|app| OpenWithDto::App { app }.to_open_with())
            .transpose()?;
        let context = self.automation_project_context(&id).await?;
        let generation = context.owner.automation.unlocked_generation()?;
        let retained_io = managed::admit_io(self, &context).await?;
        let slot = retained_io.clone();
        let opener = self.inner.ctx.file_opener();
        let weak = context.unlock.clone();
        let owner = context.owner.clone();
        let root_path = context.path.clone();
        let source = context.root_selection.clone();
        tokio::task::spawn_blocking(move || -> AppResult<()> {
            let _slot = slot;
            let root = source.open()?;
            let file = root.read_file(&path).map_err(automation_error)?;
            if file.sha256 != expected_sha256 {
                return Err(automation_error(
                    cc_automation_core::AutomationError::FileChanged,
                ));
            }
            // Queued work is fenced before admitting the OS action. Never hold
            // a vault key/transition guard while waiting for an external app.
            if weak.upgrade().is_none()
                || owner.automation.stopped.is_cancelled()
                || owner.automation.unlocked_generation()? != generation
            {
                return Err(AppError::VaultLocked);
            }
            root.recheck()?;
            let file_path = root_path.join(&path);
            match application {
                Some(OpenWith::App(app)) => opener.open_with(&file_path, &app),
                None => opener.open_path(&file_path),
                _ => {
                    return Err(AppError::invalid(
                        "automation_editor",
                        "explicit application or system text editor required",
                    ));
                }
            }
            .map_err(|error| match error {
                cc_platform_core::OpenError::NoTextEditor => AppError::NoTextEditor,
                _ => AppError::EditorLaunch("cannot open the selected project file".into()),
            })
            ?;
            root.recheck()?;
            Ok(())
        })
        .await
        .map_err(|_| AppError::internal("editor worker stopped"))??;
        context.check_after_io(self).await?;
        drop(retained_io);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_platform_core::open::{LaunchOutput, LaunchSpec};
    use cc_platform_core::{FileOpener, Launcher, OpenError, OsFamily};
    #[derive(Debug, Default)]
    struct Recording(std::sync::Mutex<Vec<LaunchSpec>>);
    impl Launcher for Recording {
        fn launch(&self, spec: &LaunchSpec) -> Result<LaunchOutput, OpenError> {
            self.0.lock().unwrap().push(spec.clone());
            Ok(LaunchOutput::default())
        }
    }
    #[tokio::test]
    async fn editor_is_explicit_hash_checked_and_does_not_admit_jobs_or_change_files() {
        let profile = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(
            source.path().join("файл с пробелом.yml"),
            "---\n# exact original\n",
        )
        .unwrap();
        let app = AppCore::new(crate::AppConfig::for_tests(
            profile.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Editor fixture".into(),
            "correct horse editor fixture passphrase".into(),
        )
        .await
        .unwrap();
        let recorder = Arc::new(Recording::default());
        app.set_file_opener(FileOpener::new(OsFamily::Windows, recorder.clone()));
        let project = app
            .automation_project_create(
                "Owned editor source".into(),
                Some(source.path().to_string_lossy().into_owned()),
            )
            .await
            .unwrap();
        let file = app
            .automation_project_file(project.id.clone(), "файл с пробелом.yml".into())
            .await
            .unwrap();
        let editor = AppRefDto {
            kind: "path".into(),
            value: source
                .path()
                .join("test editor.exe")
                .to_string_lossy()
                .into_owned(),
        };
        app.automation_project_open_editor(
            project.id.clone(),
            file.path.clone(),
            file.sha256.clone(),
            Some(editor),
        )
        .await
        .unwrap();
        let launches = recorder.0.lock().unwrap().clone();
        assert_eq!(launches.len(), 1);
        let LaunchSpec::Exec(spec) = &launches[0] else {
            panic!("explicit editor was not used")
        };
        assert_eq!(
            spec.args.last().unwrap(),
            source.path().join(&file.path).as_os_str()
        );
        assert_eq!(
            std::fs::read_to_string(source.path().join(&file.path)).unwrap(),
            file.text
        );
        assert!(
            app.automation_jobs().await.unwrap()["jobs"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        for path in ["../outside", cc_automation_core::GIT_SOURCE_FILE] {
            assert!(
                app.automation_project_open_editor(
                    project.id.clone(),
                    path.into(),
                    file.sha256.clone(),
                    None
                )
                .await
                .is_err()
            );
        }
        std::fs::write(source.path().join(&file.path), "# external edit\n").unwrap();
        assert!(
            app.automation_project_open_editor(
                project.id.clone(),
                file.path.clone(),
                file.sha256.clone(),
                None
            )
            .await
            .is_err()
        );
        app.lock().await.unwrap();
        assert!(
            app.automation_project_open_editor(project.id, file.path, file.sha256, None)
                .await
                .is_err()
        );
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
        app.shutdown().await.unwrap();
    }
}
