use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::sync::atomic::{AtomicUsize, Ordering};
#[path = "dependency_preview_live.rs"]
mod live;
use std::{fs, path::PathBuf};
use tokio::io::{AsyncWriteExt, BufReader};

fn fixture() -> Value {
    crate::json::parse(include_bytes!("fixtures/dependency-preview.json")).unwrap()
}
fn selected(value: &Value) -> SourceSelection {
    SourceSelection::new(
        crate::DependencyKind::Role,
        "demo".into(),
        "1.2.3".into(),
        serde_json::from_value(value["selection"]["source"].clone()).unwrap(),
        0,
    )
    .unwrap()
}
fn archive(value: &Value) -> VerifiedArchive {
    let mut raw = Vec::new();
    for chunk in value["chunks"].as_array().unwrap() {
        raw.extend(
            STANDARD
                .decode(chunk["response"]["data"].as_str().unwrap())
                .unwrap(),
        );
    }
    crate::verify_dependency_archive(selected(value), &raw).unwrap()
}
struct Trees {
    base_path: PathBuf,
    profile_path: PathBuf,
    project: uuid::Uuid,
    candidate_path: PathBuf,
    base: ProjectRoot,
    candidate: ProjectRoot,
    _own: tempfile::TempDir, // Last: owned handles close before TempDir cleanup.
}
fn make_trees(value: &Value) -> Trees {
    let own = tempfile::Builder::new()
        .prefix("owned-publication-preview-")
        .tempdir()
        .unwrap();
    let project = uuid::Uuid::new_v4();
    let profile_path = own.path().join("profile");
    let base_path = profile_path
        .join("automation/projects")
        .join(project.to_string());
    let candidate_path = own.path().join("candidate");
    for path in [
        &profile_path,
        &profile_path.join("automation"),
        &profile_path.join("automation/projects"),
        &base_path,
        &candidate_path,
    ] {
        private_fixture_directory(path);
    }
    fs::write(base_path.join("play.yml"), b"---\n").unwrap();
    for file in value["candidate_files"].as_array().unwrap() {
        let path = file["path"].as_str().unwrap();
        crate::validate_project_path(path).unwrap();
        let destination = candidate_path.join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(
            destination,
            STANDARD.decode(file["data"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
    }
    Trees {
        base: ProjectRoot::open(&base_path).unwrap(),
        candidate: ProjectRoot::open(&candidate_path).unwrap(),
        _own: own,
        base_path,
        profile_path,
        project,
        candidate_path,
    }
}

#[test]
fn actual_helper_candidate_is_independently_bound_to_archive_base_lock_and_receipt() {
    let value = fixture();
    let trees = make_trees(&value);
    let archive = archive(&value);
    let before = trees.base.revision().unwrap();
    let after = trees.candidate.revision().unwrap();
    assert_eq!(before.digest, value["base_digest"]);
    assert_eq!(after.digest, value["info"]["review_digest"]);
    let (_, old) = native_base(&trees.base, &before, &selected(&value)).unwrap();
    let diff = verify_candidate(
        &trees.base,
        &before,
        &trees.candidate,
        &after,
        &selected(&value),
        &archive,
        &old,
    )
    .unwrap();
    assert_eq!(diff.baseline_digest, before.digest);
    assert_eq!(diff.current_digest, after.digest);
    assert!(
        diff.changes
            .iter()
            .any(|change| change.path == "roles/demo/lookup_plugins/evil.py"
                && change.kind == "added")
    );
    assert!(!diff.changes.iter().any(|change| change.path == "play.yml"));
    assert_eq!(before.entries.len(), 1);
}

#[test]
fn self_consistent_helper_tree_cannot_replace_archive_payload_base_file_lock_source_or_receipt() {
    let value = fixture();
    let archive = archive(&value);
    for attack in [
        "archive_payload",
        "base_file",
        "extra_file",
        "lock_source",
        "provenance",
    ] {
        let trees = make_trees(&value);
        let before = trees.base.revision().unwrap();
        match attack {
            "archive_payload" => fs::write(
                trees.candidate_path.join("roles/demo/tasks/main.yml"),
                b"different reviewed code",
            )
            .unwrap(),
            "base_file" => {
                fs::write(trees.candidate_path.join("play.yml"), b"mutated base").unwrap()
            }
            "extra_file" => fs::write(
                trees.candidate_path.join("unreviewed.py"),
                b"raise RuntimeError('never run')",
            )
            .unwrap(),
            "lock_source" => {
                let path = trees.candidate_path.join(LOCK);
                let mut lock: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                lock["dependencies"][0]["source"]["url"] =
                    json!("https://different.example.test/archive.tar.gz");
                fs::write(path, serde_json::to_vec(&lock).unwrap()).unwrap();
            }
            _ => {
                let path = trees.candidate_path.join(RECEIPTS);
                let mut receipt: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                receipt["receipts"][0]["origin"] = json!("https://fabricated.example.test");
                fs::write(path, serde_json::to_vec(&receipt).unwrap()).unwrap();
            }
        }
        // Recompute the whole claimed candidate digest: rejecting only a bad
        // helper checksum would miss these independent-origin/base attacks.
        let after = trees.candidate.revision().unwrap();
        assert!(
            verify_candidate(
                &trees.base,
                &before,
                &trees.candidate,
                &after,
                &selected(&value),
                &archive,
                &[]
            )
            .is_err(),
            "{attack}"
        );
        assert_eq!(
            fs::read(trees.base_path.join("play.yml")).unwrap(),
            b"---\n"
        );
    }
}

#[test]
fn future_receipt_case_collision_and_stale_native_base_fail_before_preparation() {
    let value = fixture();
    let trees = make_trees(&value);
    let future = b"{\"version\":99,\"opaque\":\"retain exactly\"}";
    fs::write(trees.base_path.join(RECEIPTS), future).unwrap();
    let before = trees.base.revision().unwrap();
    assert!(matches!(
        native_base(&trees.base, &before, &selected(&value)),
        Err(AutomationError::Invalid(_))
    ));
    assert_eq!(fs::read(trees.base_path.join(RECEIPTS)).unwrap(), future);
    let trees = make_trees(&value);
    let old = trees.base.revision().unwrap();
    fs::write(trees.base_path.join("play.yml"), b"changed").unwrap();
    assert!(native_base(&trees.base, &old, &selected(&value)).is_err());
    let trees = make_trees(&value);
    fs::create_dir(trees.base_path.join("ROLES")).unwrap();
    let before = trees.base.revision().unwrap();
    assert!(native_base(&trees.base, &before, &selected(&value)).is_err());
}

#[test]
fn existing_pinned_lock_record_is_preserved_exactly_when_no_receipt_merge_is_needed() {
    let value = fixture();
    let trees = make_trees(&value);
    let selected = selected(&value);
    let archive = archive(&value);
    for root in [&trees.base_path, &trees.candidate_path] {
        fs::create_dir_all(root.join("roles/old/tasks")).unwrap();
        fs::write(root.join("roles/old/tasks/main.yml"), b"---\n").unwrap();
    }
    let old_tree = subtree(&trees.base.revision().unwrap(), "roles/old").unwrap();
    let old = Record {
        kind: crate::DependencyKind::Role,
        name: "old".into(),
        version: "1.0.0".into(),
        source: crate::DependencyArtifactSource {
            url: "https://old.example.test/exact.tar.gz".into(),
            sha256: "1".repeat(64),
        },
        tree_digest: old_tree.digest,
    };
    fs::write(
        trees.base_path.join(LOCK),
        serde_json::to_vec(&json!({"version":1,"dependencies":[old]})).unwrap(),
    )
    .unwrap();
    let before = trees.base.revision().unwrap();
    let (_, records) = native_base(&trees.base, &before, &selected).unwrap();
    assert_eq!(records.len(), 1);
    let path = trees.candidate_path.join(LOCK);
    let mut lock: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    lock["dependencies"]
        .as_array_mut()
        .unwrap()
        .insert(0, serde_json::to_value(&old).unwrap());
    fs::write(&path, serde_json::to_vec(&lock).unwrap()).unwrap();
    let after = trees.candidate.revision().unwrap();
    assert!(verify_candidate(
        &trees.base,
        &before,
        &trees.candidate,
        &after,
        &selected,
        &archive,
        &records
    )
    .is_ok());
    lock["dependencies"][0]["version"] = json!("2.0.0");
    fs::write(path, serde_json::to_vec(&lock).unwrap()).unwrap();
    let tampered = trees.candidate.revision().unwrap();
    assert!(verify_candidate(
        &trees.base,
        &before,
        &trees.candidate,
        &tampered,
        &selected,
        &archive,
        &records
    )
    .is_err());
}

#[tokio::test]
async fn actual_receiver_downloads_uncommitted_candidate_only_and_checks_all_bytes() {
    for corrupt in [false, true] {
        let value = fixture();
        let trees = make_trees(&value);
        let expected = trees.candidate.revision().unwrap();
        let capture = UploadedRevision {
            revision_id: uuid::Uuid::parse_str(value["info"]["preparation_id"].as_str().unwrap())
                .unwrap(),
            digest: expected.digest.clone(),
            file_count: expected
                .entries
                .iter()
                .filter(|entry| matches!(entry, ProjectEntry::File { .. }))
                .count(),
            entry_count: expected.entries.len(),
            bytes: expected.bytes,
        };
        let own = tempfile::tempdir().unwrap();
        let destination = ProjectRoot::open(own.path()).unwrap();
        let (client, server) = tokio::io::duplex(4096);
        let (reader, writer) = tokio::io::split(client);
        let peer = HelperPeer::connect(reader, writer);
        let files = value["candidate_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| {
                (
                    file["path"].as_str().unwrap().to_string(),
                    STANDARD.decode(file["data"].as_str().unwrap()).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let served = expected.clone();
        let mut damaged = false;
        let worker = tokio::spawn(async move {
            let (reader, mut writer) = tokio::io::split(server);
            let mut reader = BufReader::new(reader);
            while let Some(raw) = crate::read_frame(&mut reader).await.unwrap() {
                let request: Value = serde_json::from_slice(&raw).unwrap();
                let body = &request["body"];
                let result = match request["operation"].as_str().unwrap() {
                    "dependency_files" => {
                        let offset = body["offset"].as_u64().unwrap() as usize;
                        json!({"entries":served.entries.iter().skip(offset).take(100).collect::<Vec<_>>(),"total":served.entries.len()})
                    }
                    "dependency_chunk" => {
                        let path = body["path"].as_str().unwrap();
                        let offset = body["offset"].as_u64().unwrap() as usize;
                        let limit = body["limit"].as_u64().unwrap() as usize;
                        let file = &files[path];
                        let mut data = file[offset..(offset + limit).min(file.len())].to_vec();
                        if corrupt && !damaged && !data.is_empty() {
                            data[0] ^= 1;
                            damaged = true;
                        }
                        json!({"review_digest":served.digest,"path":path,"offset":offset,"data":STANDARD.encode(data),"eof":offset+limit==file.len()})
                    }
                    other => panic!("No commit/capture/revision fallback permitted: {other}"),
                };
                let mut response=serde_json::to_vec(&json!({"version":1,"type":"response","request_id":request["request_id"],"result":result,"error":null})).unwrap();
                response.push(b'\n');
                if writer.write_all(&response).await.is_err() {
                    break;
                }
            }
        });
        let result = destination
            .download_dependency_candidate(&capture, &peer, &CancellationToken::new())
            .await;
        if corrupt {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap(), expected);
        }
        peer.close().await;
        worker.await.unwrap();
    }
}

// The optional check receives ONLY an actual framed/independently verified
// preview while its dedicated helper owner is alive. It cannot fabricate one.
type PublicationCheck = fn(&Trees, &ProjectRoot, &VerifiedDependencyPreview, &Value, uuid::Uuid);
async fn run_fixed_prepare_fixture(mode: &'static str, check: Option<PublicationCheck>) {
    let _serial = crate::dependency_transfer::DEPENDENCY_TEST_SERIAL
        .lock()
        .await;
    let value = fixture();
    let trees = make_trees(&value);
    let expected = trees.candidate.revision().unwrap();
    if mode == "existing_receipt" {
        fs::write(
            trees.base_path.join(RECEIPTS),
            b"{\"version\":99,\"opaque\":true}",
        )
        .unwrap();
    }
    let before = trees.base.revision().unwrap();
    let own = tempfile::tempdir().unwrap();
    let stage = ProjectRoot::open(own.path()).unwrap();
    let files = value["candidate_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            (
                file["path"].as_str().unwrap().to_string(),
                STANDARD.decode(file["data"].as_str().unwrap()).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let record: Value = serde_json::from_slice(&files[LOCK]).unwrap();
    let provenance: Value = serde_json::from_slice(&files[RECEIPTS]).unwrap();
    let operation = uuid::Uuid::new_v4();
    let uploaded_id = uuid::Uuid::new_v4();
    let fetches = Arc::new(AtomicUsize::new(0));
    let uploads = Arc::new(AtomicUsize::new(0));
    let candidates = Arc::new(AtomicUsize::new(0));
    let counts = (fetches.clone(), uploads.clone(), candidates.clone());
    let served = expected.clone();
    let base = before.clone();
    let fixture = value.clone();
    let (client, server) = tokio::io::duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    let peer = HelperPeer::connect(reader, writer);
    let worker = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(server);
        let mut reader = BufReader::new(reader);
        let mut chunk_index = 0;
        while let Some(raw) = crate::read_frame(&mut reader).await.unwrap() {
            let request: Value = serde_json::from_slice(&raw).unwrap();
            let body = &request["body"];
            let result = match request["operation"].as_str().unwrap() {
                "probe" => {
                    json!({"version":1,"mode":"dependency_prepare","mode_version":1,"helper":"0.1.0","protocol":1,
                    "root_namespace":".consolecrypt-dependency-operations","root_family":sha(b"jobs"),"operation_id":operation,
                    "execution_enabled":false,"target_dispatch_enabled":false,"network_policy":"reviewed_exact_https_worker"})
                }
                "upload_begin" => {
                    counts.1.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(body["digest"], base.digest);
                    json!({"upload_id":uploaded_id})
                }
                "upload_entry" => {
                    assert_eq!(
                        body["entry"],
                        serde_json::to_value(&base.entries[0]).unwrap()
                    );
                    json!({"accepted":true})
                }
                "upload_chunk" => {
                    assert_eq!(
                        STANDARD.decode(body["data"].as_str().unwrap()).unwrap(),
                        b"---\n"
                    );
                    json!({"offset":4})
                }
                "upload_commit" => {
                    json!({"revision_id":uploaded_id,"digest":base.digest,"file_count":1,"entry_count":1,"bytes":4})
                }
                "dependency_prepare" => {
                    counts.0.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(body["selection"], fixture["selection"]);
                    assert_eq!(body["revision_digest"], base.digest);
                    assert_eq!(body["lock_sha256"], Value::Null);
                    assert_eq!(body["strip_components"], 0);
                    assert_eq!(body["network_origin"], "https://source.example.test");
                    let mut provenance = provenance["receipts"][0].clone();
                    if mode == "lying_provenance" {
                        provenance["origin"] = json!("https://unreviewed.example.test");
                    }
                    json!({"preparation_id":fixture["info"]["preparation_id"],"review_digest":served.digest,
                        "file_count":served.entries.iter().filter(|entry|matches!(entry,ProjectEntry::File{..})).count(),
                        "entry_count":served.entries.len(),"bytes":served.bytes,"record":record["dependencies"][0],"provenance":provenance})
                }
                "dependency_archive_info" => fixture["info"].clone(),
                "dependency_archive_chunk" => {
                    assert_eq!(body, &fixture["chunks"][chunk_index]["request"]);
                    let result = fixture["chunks"][chunk_index]["response"].clone();
                    chunk_index += 1;
                    result
                }
                "dependency_files" => {
                    counts.2.fetch_add(1, Ordering::SeqCst);
                    let offset = body["offset"].as_u64().unwrap() as usize;
                    json!({"entries":served.entries.iter().skip(offset).take(100).collect::<Vec<_>>(),"total":served.entries.len()})
                }
                "dependency_chunk" => {
                    let path = body["path"].as_str().unwrap();
                    let offset = body["offset"].as_u64().unwrap() as usize;
                    let limit = body["limit"].as_u64().unwrap() as usize;
                    let data = &files[path];
                    json!({"review_digest":served.digest,"path":path,"offset":offset,
                        "data":STANDARD.encode(&data[offset..offset+limit]),"eof":offset+limit==data.len()})
                }
                other => panic!("Forbidden implicit commit/capture/source execution: {other}"),
            };
            let mut response=serde_json::to_vec(&json!({"version":1,"type":"response","request_id":request["request_id"],"result":result,"error":null})).unwrap();
            response.push(b'\n');
            if writer.write_all(&response).await.is_err() {
                break;
            }
        }
    });
    let owner =
        DependencyHelperConnection::for_protocol_fixture(peer.clone(), operation, "/fixture/jobs")
            .await
            .unwrap();
    let result = owner
        .prepare_preview(
            &trees.base,
            &before,
            &selected(&value),
            &stage,
            &CancellationToken::new(),
        )
        .await;
    match mode {
        "valid" => {
            let preview = result.unwrap();
            if let Some(check) = check {
                check(&trees, &stage, &preview, &value, operation);
            }
            assert_eq!(preview.candidate(), &expected);
            assert!(preview
                .archive()
                .is_from_current_peer(owner.private_control_peer()));
            assert!(cc_models::automation::is_digest(preview.review_digest()));
            preview
                .revalidate(&trees.base, &before, &selected(&value), &stage)
                .unwrap();
            let mut wrong = value.clone();
            wrong["selection"]["source"]["url"] =
                json!("https://different.example.invalid/role.tar.gz");
            assert!(preview
                .revalidate(&trees.base, &before, &selected(&wrong), &stage)
                .is_err());
            for (root, path) in [
                (trees.base_path.as_path(), "play.yml"),
                (trees.base_path.as_path(), LOCK),
                (trees.base_path.as_path(), RECEIPTS),
                (own.path(), "roles/demo/files/payload.bin"),
                (own.path(), LOCK),
                (own.path(), RECEIPTS),
            ] {
                let path = root.join(path);
                let old = fs::read(&path).ok();
                fs::write(&path, b"changed after review").unwrap();
                assert!(preview
                    .revalidate(&trees.base, &before, &selected(&value), &stage)
                    .is_err());
                if let Some(old) = old {
                    fs::write(path, old).unwrap()
                } else {
                    fs::remove_file(path).unwrap()
                }
                preview
                    .revalidate(&trees.base, &before, &selected(&value), &stage)
                    .unwrap();
            }
            assert_eq!(fetches.load(Ordering::SeqCst), 1);
            assert_eq!(uploads.load(Ordering::SeqCst), 1);
            assert!(candidates.load(Ordering::SeqCst) > 0);
        }
        "existing_receipt" => {
            assert!(matches!(result, Err(AutomationError::Invalid(_))));
            assert_eq!(fetches.load(Ordering::SeqCst), 0);
            assert_eq!(uploads.load(Ordering::SeqCst), 0);
            assert_eq!(
                fs::read(trees.base_path.join(RECEIPTS)).unwrap(),
                b"{\"version\":99,\"opaque\":true}"
            );
        }
        _ => {
            assert!(result.is_err());
            assert_eq!(fetches.load(Ordering::SeqCst), 1);
            assert_eq!(candidates.load(Ordering::SeqCst), 0);
        }
    }
    assert_eq!(trees.base.revision().unwrap(), before);
    owner.close().await;
    worker.await.unwrap();
}
#[tokio::test]
async fn fixed_prepare_adapter_crosses_real_framing_and_receipt_rejection_precedes_upload_or_fetch()
{
    for mode in ["valid", "existing_receipt", "lying_provenance"] {
        run_fixed_prepare_fixture(mode, None).await;
    }
}
fn private_fixture_directory(path: &std::path::Path) {
    fs::create_dir_all(path).unwrap();
    #[cfg(windows)]
    crate::project::publication::protect_owned_publication_fixture(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
#[path = "dependency_publication_tests.rs"]
mod publication_facade_tests;

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires owned same-filesystem file-bind parent moved after initial genuine preview walks"]
async fn initial_publication_revalidation_uses_strict_roots_after_same_byte_bind_injection() {
    run_fixed_prepare_fixture("valid", Some(|trees, stage, preview, value, _operation| {
        let root = std::env::var("CC_PUBLICATION_WHOLE_BOUNDARY_ROOT").expect("owned fixture required");
        assert!(root.starts_with("/tmp/owned-publication-whole-"));
        let injected = PathBuf::from(root).join("initial-candidate-files");
        let before = trees.base.revision().unwrap();
        let (actual, plain_control_passed) = crate::project::publication::initial_bind_revalidation_fixture(
            &trees.base, &before, &selected(value), stage, preview, &injected, trees._own.path()).unwrap();
        assert!(plain_control_passed, "OLD plain-root real revalidation must accept the same bytes, proving the strict rejection is not a digest mismatch");
        assert!(matches!(actual, Err(AutomationError::Io(ref error)) if error.raw_os_error()==Some(18)));
        preview.revalidate(&trees.base, &before, &selected(value), stage).unwrap();
    })).await;
}
