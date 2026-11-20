use super::*;
use cc_app_core::{AppConfig, AppCore};
use serde_json::{json, Value};
use std::sync::Mutex;
static SERIAL: Mutex<()> = Mutex::new(());
struct Restore(Option<AppCore>);
impl Drop for Restore {
    fn drop(&mut self) {
        state::set_core(self.0.take());
    }
}
struct Fixture {
    directory: tempfile::TempDir,
    app: AppCore,
    project: String,
    revision: u64,
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let app = AppCore::new(AppConfig::for_tests(
            directory.path().to_string_lossy().into_owned(),
        ))
        .unwrap();
        app.create_local_profile(
            "Owned portable Bridge fixture".into(),
            "synthetic Bridge fixture password42".into(),
        )
        .await
        .unwrap();
        let project = app
            .automation_project_create("Owned portable Bridge project".into(), None)
            .await
            .unwrap()
            .id;
        app.automation_project_save(
            project.clone(),
            "selected.yml".into(),
            None,
            "---\n- hosts: all\n  tasks: []\n".into(),
        )
        .await
        .unwrap();
        let revision = app
            .automation_projects()
            .await
            .unwrap()
            .projects
            .into_iter()
            .find(|p| p.id == project)
            .unwrap()
            .revision
            .unwrap();
        Self {
            directory,
            app,
            project,
            revision,
        }
    }
}
#[test]
fn synchronous_bridge_constructor_bounds_and_redacts_invalid_password_bytes() {
    for bytes in [vec![], vec![0xff, 0xfe], vec![b'x'; 4097]] {
        let result = PortablePasswordTransfer::new(bytes);
        let Err(error) = result else {
            panic!("invalid password transfer must fail")
        };
        assert_eq!(error, unavailable());
    }
}
#[test]
fn issued_bridge_handle_cancel_and_join_do_not_touch_replacement_global_core() {
    let _serial = SERIAL.lock().unwrap();
    let _restore = Restore(state::core_opt());
    state::runtime().block_on(async {
        let original = Fixture::new().await;
        let replacement = Fixture::new().await;
        state::set_core(Some(original.app.clone()));
        let original_scope = automation_portable_scope().await.unwrap();
        let old = automation_portable_register_context(
            original_scope,
            original.project.clone(),
            original.revision,
        )
        .await
        .unwrap();
        state::set_core(Some(replacement.app.clone()));
        let replacement_scope = automation_portable_scope().await.unwrap();
        let fresh = automation_portable_register_context(
            replacement_scope,
            replacement.project.clone(),
            replacement.revision,
        )
        .await
        .unwrap();
        old.cancel();
        old.joined().await.unwrap();
        assert!(old.start().await.is_err());
        let value: Value = serde_json::from_str(&fresh.start().await.unwrap()).unwrap();
        assert_eq!(value["projectId"], replacement.project);
        fresh.joined().await.unwrap();
        assert!(replacement.app.is_unlocked().await);
        drop((old, fresh));
        state::set_core(None);
        original.app.shutdown().await.unwrap();
        replacement.app.shutdown().await.unwrap();
    });
}
#[test]
fn actual_bridge_context_export_inspect_uses_native_registration_ack_then_explicit_start() {
    let _serial = SERIAL.lock().unwrap();
    let _restore = Restore(state::core_opt());
    state::runtime().block_on(async {
        let f=Fixture::new().await;
        state::set_core(Some(f.app.clone()));
        let before=serde_json::to_value(f.app.automation_projects().await.unwrap()).unwrap();
        let scope=automation_portable_scope().await.unwrap();
        let context=automation_portable_register_context(scope.clone(),f.project.clone(),f.revision).await.unwrap();
        let value:Value=serde_json::from_str(&context.start().await.unwrap()).unwrap();
        context.joined().await.unwrap();
        let destination=f.directory.path().join("owned-bridge-export.ccproject");
        let request=json!({"scope":value["scope"],"projectId":f.project,"projectRevision":f.revision,"revisionDigest":value["revisionDigest"],"projectStamp":value["projectStamp"],"selectedPaths":["selected.yml"],"playbook":"selected.yml"}).to_string();
        let password=PortablePasswordTransfer::new(b"synthetic separate portable password".to_vec()).unwrap();
        let export=automation_portable_register_export(request,&password,destination.to_string_lossy().into_owned()).await.unwrap();
        assert!(!destination.exists(),"actual registration ACK never starts body/file creation");
        drop(password); // successful transfer Drop MUST NOT cancel handed-off body
        let exported:Value=serde_json::from_str(&export.start().await.unwrap()).unwrap();
        export.joined().await.unwrap();
        assert!(export.start().await.is_err());
        let password=PortablePasswordTransfer::new(b"synthetic separate portable password".to_vec()).unwrap();
        let inspect=automation_portable_register_inspect(scope,destination.to_string_lossy().into_owned(),&password).await.unwrap();
        drop(password);
        let inspected:Value=serde_json::from_str(&inspect.start().await.unwrap()).unwrap();
        inspect.joined().await.unwrap();
        assert_eq!(inspected["encryptedSha256"],exported["encryptedSha256"]);
        assert_eq!(inspected["summary"]["files"],1);
        assert_eq!(inspected["publicationSupported"],false);
        assert_eq!(serde_json::to_value(f.app.automation_projects().await.unwrap()).unwrap(),before);
        drop((context,export,inspect));
        state::set_core(None);
        f.app.shutdown().await.unwrap();
    });
}
