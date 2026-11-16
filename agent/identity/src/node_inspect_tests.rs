use std::os::unix::fs::MetadataExt;
fn inspection_snapshot(
    temp: &Temp,
) -> std::collections::BTreeMap<String, (Hash, u64, u64, u64, u64, i64, i64)> {
    fs::read_dir(temp.node())
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let m = fs::symlink_metadata(&path).unwrap();
            let bytes = zeroize::Zeroizing::new(fs::read(&path).unwrap());
            (
                path.file_name().unwrap().to_str().unwrap().to_owned(),
                (
                    file_hash(&bytes),
                    m.dev(),
                    m.ino(),
                    m.mode() as u64,
                    m.nlink(),
                    m.mtime(),
                    m.mtime_nsec(),
                ),
            )
        })
        .collect()
}
fn expected_inspection(
    temp: &Temp,
    layout: PrivateLayout,
    state: SelectionState,
    reader: u32,
    epoch: Option<u64>,
) {
    let before = inspection_snapshot(temp);
    let lock = fs::metadata(temp.node().join(".node-keys.lock")).unwrap();
    let observation = inspect_node_state(&temp.node(), id()).unwrap();
    assert_eq!(
        observation.facts(),
        &LocalNodeInspection {
            format: 1,
            agent_id: id(),
            private_layout: layout,
            required_reader_schema: reader,
            actual_selected_epoch: epoch,
            selection_state: state,
            guard: GuardIdentity {
                dev: lock.dev(),
                ino: lock.ino()
            }
        }
    );
    observation.recheck_guard_identity().unwrap();
    drop(observation);
    assert_eq!(inspection_snapshot(temp), before);
}
#[test]
fn inspector_exact_four_states_are_readonly_local_facts_without_sqlite_access() {
    let temp = Temp::new();
    let public = provision_node(&temp.node(), id()).unwrap();
    expected_inspection(
        &temp,
        PrivateLayout::LegacyV1,
        SelectionState::Legacy1,
        1,
        Some(1),
    );
    prepare_node_rotation(&temp.node(), id(), public.key_fingerprint).unwrap();
    expected_inspection(
        &temp,
        PrivateLayout::PreparedV2,
        SelectionState::Prepared1,
        1,
        Some(1),
    );
    let (temp, mut actor, p, input) = fixture();
    FAIL.with(|v| *v.borrow_mut() = Some(("after-readback", INTENT)));
    assert!(activate(
        &temp.node(),
        &mut actor,
        id(),
        &p.rotation_id,
        input.clone(),
        &mut now
    )
    .is_err());
    FAIL.with(|v| *v.borrow_mut() = None);
    drop(actor);
    // No authority exists at its configured path during inspection.
    fs::rename(temp.0.join("db"), temp.0.join("offline-db")).unwrap();
    expected_inspection(
        &temp,
        PrivateLayout::ActivationV3,
        SelectionState::ActivationPending,
        3,
        None,
    );
    assert!(!temp.node().join(ACTIVE).exists());
    fs::rename(temp.0.join("offline-db"), temp.0.join("db")).unwrap();
    let mut actor = open(&temp);
    activate(
        &temp.node(),
        &mut actor,
        id(),
        &p.rotation_id,
        input,
        &mut now,
    )
    .unwrap();
    drop(actor);
    fs::rename(temp.0.join("db"), temp.0.join("offline-db")).unwrap();
    expected_inspection(
        &temp,
        PrivateLayout::ActivationV3,
        SelectionState::Selected2,
        3,
        Some(2),
    );
}
#[test]
fn inspector_v2_preserves_supported_whitespace_and_order_while_v3_stays_canonical() {
    let temp = Temp::new();
    let public = provision_node(&temp.node(), id()).unwrap();
    prepare_node_rotation(&temp.node(), id(), public.key_fingerprint).unwrap();
    let path = temp.node().join("node-rotation.intent.v2");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let bytes = serde_json::to_vec_pretty(&value).unwrap();
    fs::write(path, &bytes).unwrap();
    let path = temp.node().join("node-rotation.state.v2");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["intentHash"] = serde_json::to_value(crate::wire::hash(
        b"consolecrypt.agent.node-prepare-file.v2",
        &bytes,
    ))
    .unwrap();
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    expected_inspection(
        &temp,
        PrivateLayout::PreparedV2,
        SelectionState::Prepared1,
        1,
        Some(1),
    );
    let (temp, mut actor, p, input) = fixture();
    activate(
        &temp.node(),
        &mut actor,
        id(),
        &p.rotation_id,
        input,
        &mut now,
    )
    .unwrap();
    let path = temp.node().join(INTENT);
    let bytes = fs::read(&path).unwrap();
    let mut padded = vec![b' '];
    padded.extend(bytes);
    fs::write(path, padded).unwrap();
    let before = inspection_snapshot(&temp);
    assert!(inspect_node_state(&temp.node(), id()).is_err());
    assert_eq!(inspection_snapshot(&temp), before);
}
#[test]
fn inspector_rejects_unknown_partial_missing_dependency_and_foreign_agent_without_mutation() {
    for case in [
        "missing-guard",
        "guard-mode",
        "directory-mode",
        "unknown",
        "partial-v2",
        "unknown-v2",
        "partial-v3",
        "orphan-active",
        "missing-pending",
        "foreign-agent",
        "guard-link",
    ] {
        let (temp, mut actor, p, input) = fixture();
        if case == "missing-pending" {
            activate(
                &temp.node(),
                &mut actor,
                id(),
                &p.rotation_id,
                input,
                &mut now,
            )
            .unwrap();
            fs::remove_file(temp.node().join("node-rotation.pending.v2")).unwrap();
        }
        match case {
            "directory-mode" => {
                fs::set_permissions(temp.node(), fs::Permissions::from_mode(0o500)).unwrap()
            }
            "missing-guard" => fs::remove_file(temp.node().join(".node-keys.lock")).unwrap(),
            "guard-mode" => fs::set_permissions(
                temp.node().join(".node-keys.lock"),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap(),
            "guard-link" => fs::hard_link(
                temp.node().join(".node-keys.lock"),
                temp.0.join("linked-lock"),
            )
            .unwrap(),
            "partial-v2" => fs::write(temp.node().join("node-rotation.state.v2"), b"{").unwrap(),
            "unknown" | "unknown-v2" | "partial-v3" | "orphan-active" => {
                let name = match case {
                    "unknown" => "unrecognized",
                    "unknown-v2" => "node-rotation.future.v9",
                    "partial-v3" => INTENT,
                    _ => ACTIVE,
                };
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(temp.node().join(name))
                    .unwrap();
            }
            _ => (),
        }
        let before = inspection_snapshot(&temp);
        let agent = if case == "foreign-agent" {
            AgentId::new("99999999-9999-4999-8999-999999999999").unwrap()
        } else {
            id()
        };
        assert!(inspect_node_state(&temp.node(), agent).is_err(), "{case}");
        assert_eq!(inspection_snapshot(&temp), before);
        if case == "directory-mode" {
            assert_eq!(
                fs::metadata(temp.node()).unwrap().permissions().mode() & 0o777,
                0o500
            );
            fs::set_permissions(temp.node(), fs::Permissions::from_mode(0o700)).unwrap();
            // owned fixture cleanup only
        }
    }
}
#[test]
fn inspector_opaque_shared_lifetime_blocks_writer_and_detects_named_lock_substitution() {
    let temp = Temp::new();
    provision_node(&temp.node(), id()).unwrap();
    let first = inspect_node_state(&temp.node(), id()).unwrap();
    let second = inspect_node_state(&temp.node(), id()).unwrap();
    fs::set_permissions(temp.node(), fs::Permissions::from_mode(0o500)).unwrap();
    assert!(matches!(
        first.recheck_guard_identity(),
        Err(Error::Storage)
    ));
    assert_eq!(
        fs::metadata(temp.node()).unwrap().permissions().mode() & 0o777,
        0o500
    );
    fs::set_permissions(temp.node(), fs::Permissions::from_mode(0o700)).unwrap();
    first.recheck_guard_identity().unwrap();
    assert!(matches!(
        crate::node_prepare::existing_lock(&temp.node()),
        Err(Error::Conflict)
    ));
    drop(second);
    assert!(matches!(
        crate::node_prepare::existing_lock(&temp.node()),
        Err(Error::Conflict)
    ));
    drop(first);
    let writer = crate::node_prepare::existing_lock(&temp.node()).unwrap();
    drop(writer);
    let observation = inspect_node_state(&temp.node(), id()).unwrap();
    fs::rename(temp.node().join(".node-keys.lock"), temp.0.join("old-lock")).unwrap();
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temp.node().join(".node-keys.lock"))
        .unwrap();
    assert!(matches!(
        observation.recheck_guard_identity(),
        Err(Error::Storage)
    ));
}
#[test]
fn inspector_guard_child() {
    if std::env::var_os("CC_INSPECTOR_ROOT").is_none() {
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("CC_INSPECTOR_ROOT").unwrap());
    validate_root(&root);
    let observation = inspect_node_state(&root.join("node"), id()).unwrap();
    fs::write(root.join("inspector.ready"), b"owned-readonly-inspector").unwrap();
    std::thread::sleep(Duration::from_secs(20));
    observation.recheck_guard_identity().unwrap();
}
struct InspectorChild(std::process::Child);
impl Drop for InspectorChild {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
#[test]
fn inspector_native_child_exit_releases_only_owned_shared_descriptor() {
    let temp = Temp::new();
    provision_node(&temp.node(), id()).unwrap();
    let before = inspection_snapshot(&temp);
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "node_activation::linux::tests::inspector_guard_child",
            "--nocapture",
        ])
        .env("CC_INSPECTOR_ROOT", &temp.0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut child = InspectorChild(child);
    let began = Instant::now();
    while !temp.0.join("inspector.ready").exists() {
        assert!(began.elapsed() < Duration::from_secs(5));
        assert!(child.0.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(
        crate::node_prepare::existing_lock(&temp.node()),
        Err(Error::Conflict)
    ));
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    drop(child);
    let writer = crate::node_prepare::existing_lock(&temp.node()).unwrap();
    drop(writer);
    assert_eq!(inspection_snapshot(&temp), before);
}
