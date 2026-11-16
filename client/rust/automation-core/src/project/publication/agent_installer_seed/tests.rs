use super::*;
use serde_json::Value;
use std::fs;

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let owner = tempfile::Builder::new().prefix("owned-publication-agent-installer-").tempdir().unwrap();
    let profile = owner.path().join("profile");
    fs::create_dir(&profile).unwrap();
    #[cfg(windows)]
    super::super::protect_owned_publication_fixture(&profile);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&profile, fs::Permissions::from_mode(0o700)).unwrap();
    }
    (owner, profile)
}
#[test]
fn exact_binary_source_new_only_and_source_is_never_release_trust() {
    let (_owner, profile) = fixture();
    let artifact = profile.parent().unwrap().join("release.zip");
    let manifest = profile.parent().unwrap().join("release.json");
    let bytes = include_bytes!("fixtures/parse-fixture.zip");
    fs::write(&artifact, bytes).unwrap();
    fs::write(
        &manifest,
        include_bytes!("fixtures/signed/release-manifest.json"),
    )
    .unwrap();
    // Ordinary existing managed-project parents historically use0755. Source
    // factory must not repair them; only its exclusive UUID child is private.
    fs::create_dir_all(profile.join("automation/projects")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            profile.join("automation"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        fs::set_permissions(
            profile.join("automation/projects"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let project = Uuid::new_v4();
    let seed = AgentInstallerSeed::create(
        &profile,
        project,
        AgentInstallOperation::Install,
        false,
        Some((&artifact, &manifest)),
        |_, _, _| Ok(()),
        || Ok(()),
    )
    .unwrap();
    seed.recheck().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(profile.join("automation/projects"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert_eq!(
            fs::metadata(seed.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let mut actual = Vec::new();
    seed.root
        .open_file("release/candidate.zip")
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual.as_slice(), &bytes[..]);
    assert_eq!(fs::read(&artifact).unwrap().as_slice(), &bytes[..]);
    assert!(!seed.claimed_release().unwrap().signature_verified);
    let play = seed.root.read_file("playbook.yml").unwrap().text;
    let v: Value = serde_json::from_str(&play).unwrap();
    assert_eq!(v[0]["vars"]["cc_agent_install_start"], false);
    assert_eq!(v[0]["vars"]["cc_agent_install_operation"], "install");
    assert!(v[0].get("capabilities").is_none());
    // The installed role's ../../ adapters genuinely resolve to this root.
    for file in [
        "install_linux.py",
        "installed_runtime_policy.py",
        "release_platforms.py",
        "collector_provision.py",
        "collector_namespace_probe.py",
        "collector_disk_namespace_probe.py",
        "stage.py",
        "release_verifier.py",
        "notice_inventory.py",
        "prototype_package.py",
        "package_wheel.py",
    ] {
        assert!(seed.revision.entries.iter().any(|e| e.path() == file));
    }
    assert!(
        AgentInstallerSeed::create(
            &profile,
            project,
            AgentInstallOperation::Status,
            false,
            None,
            |_, _, _| Ok(()),
            || Ok(())
        )
        .is_err()
    );
    seed.recheck().unwrap();
}
#[test]
fn oversized_selected_file_is_rejected_before_managed_directory_allocation() {
    let (_owner, profile) = fixture();
    let a = profile.parent().unwrap().join("oversize.zip");
    let m = profile.parent().unwrap().join("release.json");
    fs::File::create(&a)
        .unwrap()
        .set_len(MAX_ARTIFACT + 1)
        .unwrap();
    fs::write(&m, b"{}").unwrap();
    assert!(
        AgentInstallerSeed::create(
            &profile,
            Uuid::new_v4(),
            AgentInstallOperation::Update,
            false,
            Some((&a, &m)),
            |_, _, _| Ok(()),
            || Ok(())
        )
        .is_err()
    );
    assert!(!profile.join("automation").exists());
}
#[test]
fn cancelled_source_snapshot_cannot_publish_or_resume_a_partial_root() {
    let (_owner, profile) = fixture();
    let project = Uuid::new_v4();
    let mut calls = 0usize;
    let result = AgentInstallerSeed::create(
        &profile,
        project,
        AgentInstallOperation::Remove,
        false,
        None,
        |_, _, _| Ok(()),
        || {
            calls += 1;
            if calls > 12 {
                Err(AutomationError::Closed)
            } else {
                Ok(())
            }
        },
    );
    assert!(result.is_err());
    assert!(
        profile
            .join("automation/projects")
            .join(project.to_string())
            .exists()
    );
    assert!(
        AgentInstallerSeed::create(
            &profile,
            project,
            AgentInstallOperation::Status,
            false,
            None,
            |_, _, _| Ok(()),
            || Ok(())
        )
        .is_err()
    );
}
#[test]
fn artifact_hardlink_and_release_on_nonrelease_operation_are_refused_before_create() {
    let (_owner, profile) = fixture();
    let a = profile.parent().unwrap().join("release.zip");
    let m = profile.parent().unwrap().join("release.json");
    fs::write(&a, b"public").unwrap();
    fs::write(&m, b"{}").unwrap();
    fs::hard_link(&a, profile.parent().unwrap().join("other.zip")).unwrap();
    assert!(
        AgentInstallerSeed::create(
            &profile,
            Uuid::new_v4(),
            AgentInstallOperation::Install,
            false,
            Some((&a, &m)),
            |_, _, _| Ok(()),
            || Ok(())
        )
        .is_err()
    );
    assert!(
        AgentInstallerSeed::create(
            &profile,
            Uuid::new_v4(),
            AgentInstallOperation::Remove,
            false,
            Some((&a, &m)),
            |_, _, _| Ok(()),
            || Ok(())
        )
        .is_err()
    );
    assert!(!profile.join("automation").exists());
}
