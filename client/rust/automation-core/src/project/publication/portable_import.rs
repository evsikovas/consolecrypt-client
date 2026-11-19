//! Fresh-project purpose over the SAME namespace, ACL, file journal, walker and
//! no-replace OS adapter. No SQL, imported catalog/inventory or approval flags.
use super::*;
use crate::portable_package::PortableImport;
use std::path::PathBuf;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportFacts {
    project: Uuid,
    operation: Uuid,
    reservation_row_sha256: String,
    encrypted_sha256: String,
    portable_digest: String,
    source_digest: String,
    files: usize,
    bytes: u64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportIntent {
    version: u32,
    purpose: ImportPurpose,
    transaction: Uuid,
    parent: DirectoryIdentity,
    facts: ImportFacts,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ImportPurpose {
    PortableNewProjectV1,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
enum CatalogBefore {
    Absent,
}
#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportCatalogAudit {
    version: u32,
    purpose: ImportPurpose,
    before: CatalogBefore,
    intent_sha256: String,
    project: Uuid,
    transaction: Uuid,
    selected_revision: u64,
    selected_row_sha256: String,
    selected_digest: String,
    selected: DirectoryIdentity,
}
impl ImportIntent {
    fn stage(&self) -> String {
        format!("stage-{}", self.transaction)
    }
    fn target(&self) -> String {
        format!("generation-{}", self.transaction)
    }
    fn marker(&self, id: DirectoryIdentity) -> Result<Marker> {
        Ok(Marker {
            version: 1,
            intent_sha256: hash(&encode(self)?),
            stage: id,
        })
    }
}
/// No authority constructor from journal/JSON. Only a genuine authenticated
/// package is consumed by prepare(), and Native must separately own acceptance.
pub(crate) struct PreparedPortableImport {
    namespace: namespace::ManagedNamespace,
    profile: PathBuf,
    intent: ImportIntent,
    marker: Marker,
    expected: ProjectRevision,
    _permit: OwnedSemaphorePermit,
}
pub(crate) struct PublishedPortableImport {
    namespace: namespace::ManagedNamespace,
    profile: PathBuf,
    intent: ImportIntent,
    marker: Marker,
    expected: ProjectRevision,
    target: ProjectRoot,
    _permit: OwnedSemaphorePermit,
}
/// Read-only pin for an ACTUALLY selected SQL generation. Does not recreate a
/// publisher, package or approval. Current content may legitimately be edited.
pub(crate) struct PinnedPortableImport {
    namespace: namespace::ManagedNamespace,
    intent: ImportIntent,
    marker: Marker,
    target: ProjectRoot,
}
fn fingerprint(intent: &ImportIntent, marker: &Marker) -> Result<String> {
    Ok(hash(&encode(&(
        hash(&encode(intent)?),
        hash(&encode(marker)?),
    ))?))
}
fn identity_check(
    namespace: &namespace::ManagedNamespace,
    intent: &ImportIntent,
    marker: &Marker,
    target: &ProjectRoot,
) -> Result<()> {
    namespace.check()?;
    check_entries(namespace, intent)?;
    require(parsed::<ImportIntent>(&read_small(&namespace.root, INTENT)?)? == *intent)?;
    for name in [CREATED, READY, PUBLISHED] {
        require(parsed::<Marker>(&read_small(&namespace.root, name)?)? == *marker)?;
    }
    require(!exists(&namespace.root, &intent.stage())?)?;
    let named = namespace::open_child(&namespace.root, &intent.target())?;
    require(identity(&named)? == marker.stage && identity(&target.root)? == marker.stage)?;
    namespace::verify_private_child(&named)?;
    let mut prior = None;
    for name in [CATALOG_COMMITTED, COMPLETE] {
        if exists(&namespace.root, name)? {
            let audit: ImportCatalogAudit = parsed(&read_small(&namespace.root, name)?)?;
            require(
                audit.version == 1
                    && audit.purpose == ImportPurpose::PortableNewProjectV1
                    && audit.before == CatalogBefore::Absent
                    && audit.intent_sha256 == hash(&encode(intent)?)
                    && audit.project == intent.facts.project
                    && audit.transaction == intent.transaction
                    && audit.selected_revision == 3
                    && valid_hash(&audit.selected_row_sha256)
                    && audit.selected_digest == intent.facts.portable_digest
                    && audit.selected == marker.stage,
            )?;
            if name == COMPLETE {
                require(prior.as_ref() == Some(&audit))?;
            } else {
                prior = Some(audit);
            }
        }
    }
    Ok(())
}
impl PinnedPortableImport {
    pub(crate) fn open(
        profile: &Path,
        project: Uuid,
        transaction: Uuid,
        expected_fingerprint: &str,
    ) -> Result<Self> {
        require(!project.is_nil() && !transaction.is_nil() && valid_hash(expected_fingerprint))?;
        let namespace = namespace::ManagedNamespace::existing_fresh_project(profile, project)?;
        let intent: ImportIntent = parsed(&read_small(&namespace.root, INTENT)?)?;
        require(
            intent.version == 1
                && intent.purpose == ImportPurpose::PortableNewProjectV1
                && intent.transaction == transaction
                && intent.facts.project == project
                && !intent.facts.operation.is_nil()
                && intent.parent == identity(&namespace.root)?
                && valid_hash(&intent.facts.encrypted_sha256)
                && valid_hash(&intent.facts.portable_digest)
                && valid_hash(&intent.facts.source_digest)
                && valid_hash(&intent.facts.reservation_row_sha256)
                && intent.facts.files <= 10_000
                && intent.facts.bytes <= cc_crypto_core::MAX_PORTABLE_PROJECT_PLAIN as u64,
        )?;
        let marker: Marker = parsed(&read_small(&namespace.root, READY)?)?;
        require(
            marker.version == 1
                && marker.intent_sha256 == hash(&encode(&intent)?)
                && fingerprint(&intent, &marker)? == expected_fingerprint,
        )?;
        let target = ProjectRoot {
            root: namespace::open_child(&namespace.root, &intent.target())?,
            #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
            publication_boundary: true,
        };
        identity_check(&namespace, &intent, &marker, &target)?;
        Ok(Self {
            namespace,
            intent,
            marker,
            target,
        })
    }
    pub(crate) fn root(&self) -> &ProjectRoot {
        &self.target
    }
    pub(crate) fn recheck(&self) -> Result<()> {
        identity_check(&self.namespace, &self.intent, &self.marker, &self.target)
    }
}
/// Observation only: absent family stops without any mkdir/copy/repair.
pub(crate) fn admit(profile: &Path, project: Uuid) -> Result<()> {
    let _observed = namespace::ManagedNamespace::fresh_project(profile, project, false)?;
    Ok(())
}
fn check_entries(namespace: &namespace::ManagedNamespace, intent: &ImportIntent) -> Result<()> {
    let stage = intent.stage();
    let target = intent.target();
    let allowed = [
        INTENT,
        CREATED,
        READY,
        PUBLISHED,
        CATALOG_COMMITTED,
        COMPLETE,
        stage.as_str(),
        target.as_str(),
    ];
    let mut count = 0usize;
    for entry in namespace.root.entries()? {
        let entry = entry?;
        count += 1;
        require(count <= allowed.len())?;
        let name = entry.file_name();
        let text = name.to_str().ok_or_else(recovery)?;
        require(allowed.contains(&text))?;
    }
    Ok(())
}
pub(crate) fn supported_package(package: &PortableImport) -> Result<()> {
    require(package.encrypted_sha256().is_some_and(valid_hash))?;
    supported_tree(package.revision())
}

/// A fresh genuine package and CURRENT Native SQL fingerprint are mandatory.
/// Existing intent/markers alone never reconstruct a publication grant. This
/// can finish an already staged/renamed generation, never re-copy or overwrite.
pub(crate) fn observe_recovery(
    profile: &Path,
    project: Uuid,
    operation: Uuid,
    transaction: Uuid,
    reservation_sha: &str,
    fp: &str,
    package: &PortableImport,
    ack: &crate::dependency_publication::CatalogPublicationAck,
    cancel: &CancellationToken,
) -> Result<()> {
    require(
        recovery_impl(
            profile,
            project,
            operation,
            transaction,
            reservation_sha,
            fp,
            package,
            ack,
            cancel,
            false,
        )?
        .is_none(),
    )
}
pub(crate) fn recover(
    profile: &Path,
    project: Uuid,
    operation: Uuid,
    transaction: Uuid,
    reservation_sha: &str,
    fp: &str,
    package: PortableImport,
    ack: &crate::dependency_publication::CatalogPublicationAck,
    cancel: &CancellationToken,
) -> Result<PublishedPortableImport> {
    recovery_impl(
        profile,
        project,
        operation,
        transaction,
        reservation_sha,
        fp,
        &package,
        ack,
        cancel,
        true,
    )?
    .ok_or_else(recovery)
}
fn recovery_impl(
    profile: &Path,
    project: Uuid,
    operation: Uuid,
    transaction: Uuid,
    reservation_row_sha256: &str,
    expected_fingerprint: &str,
    package: &PortableImport,
    current_sql_ack: &crate::dependency_publication::CatalogPublicationAck,
    cancel: &CancellationToken,
    finish: bool,
) -> Result<Option<PublishedPortableImport>> {
    require(
        !project.is_nil()
            && !operation.is_nil()
            && !transaction.is_nil()
            && valid_hash(reservation_row_sha256)
            && valid_hash(expected_fingerprint),
    )?;
    let (selected_project, selected_transaction, revision, row_sha) =
        current_sql_ack.selected_facts();
    require(
        selected_project == project
            && selected_transaction == transaction
            && matches!(revision, 2 | 3)
            && valid_hash(row_sha),
    )?;
    let held = permit()?;
    check_cancel(cancel)?;
    let encrypted_sha256 = package
        .encrypted_sha256()
        .filter(|s| valid_hash(s))
        .ok_or_else(recovery)?;
    let summary = package.summary();
    let expected = package.revision().clone();
    supported_tree(&expected)?;
    let namespace = namespace::ManagedNamespace::existing_fresh_project(profile, project)?;
    let intent: ImportIntent = parsed(&read_small(&namespace.root, INTENT)?)?;
    require(
        intent.version == 1
            && intent.purpose == ImportPurpose::PortableNewProjectV1
            && intent.transaction == transaction
            && intent.parent == identity(&namespace.root)?
            && intent.facts
                == ImportFacts {
                    project,
                    operation,
                    reservation_row_sha256: reservation_row_sha256.into(),
                    encrypted_sha256: encrypted_sha256.into(),
                    portable_digest: summary.portable_digest,
                    source_digest: summary.source_digest,
                    files: summary.files,
                    bytes: summary.bytes,
                },
    )?;
    check_entries(&namespace, &intent)?;
    let marker: Marker = parsed(&read_small(&namespace.root, READY)?)?;
    require(
        marker.version == 1
            && marker.intent_sha256 == hash(&encode(&intent)?)
            && parsed::<Marker>(&read_small(&namespace.root, CREATED)?)? == marker
            && fingerprint(&intent, &marker)? == expected_fingerprint,
    )?;
    let stage_exists = exists(&namespace.root, &intent.stage())?;
    let target_exists = exists(&namespace.root, &intent.target())?;
    require(stage_exists != target_exists)?;
    if stage_exists {
        // Rev3 is an actually selected ready SQL row: it must NEVER rename.
        require(
            revision == 2
                && !exists(&namespace.root, CATALOG_COMMITTED)?
                && !exists(&namespace.root, COMPLETE)?,
        )?;
        let prepared = PreparedPortableImport {
            namespace,
            profile: profile.to_owned(),
            intent,
            marker,
            expected,
            _permit: held,
        };
        require(prepared.fingerprint()? == expected_fingerprint)?;
        if !finish {
            return Ok(None);
        }
        return prepared.publish(current_sql_ack, cancel).map(Some);
    }
    let target = ProjectRoot {
        root: namespace::open_child(&namespace.root, &intent.target())?,
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        publication_boundary: true,
    };
    require(
        identity(&target.root)? == marker.stage && target.revision_for_publication()? == expected,
    )?;
    namespace::verify_private_child(&target.root)?;
    namespace.check()?;
    check_cancel(cancel)?;
    let observed = exists(&namespace.root, PUBLISHED)?;
    if observed {
        identity_check(&namespace, &intent, &marker, &target)?;
    } else {
        require(
            !exists(&namespace.root, CATALOG_COMMITTED)? && !exists(&namespace.root, COMPLETE)?,
        )?;
    }
    if !finish {
        return Ok(None);
    }
    if !observed {
        // The independent SQL identity fence and actual authenticated target
        // prove the rename outcome. Only a fixed observation is repaired.
        sync_directory(&namespace.root)?;
        write_new(&namespace.root, PUBLISHED, &marker)?;
    }
    let published = PublishedPortableImport {
        namespace,
        profile: profile.to_owned(),
        intent,
        marker,
        expected,
        target,
        _permit: held,
    };
    published.recheck()?;
    check_cancel(cancel)?;
    Ok(Some(published))
}
impl PreparedPortableImport {
    pub(crate) fn prepare(
        profile: &Path,
        project: Uuid,
        operation: Uuid,
        transaction: Uuid,
        reservation_row_sha256: &str,
        package: PortableImport,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        require(
            !project.is_nil()
                && !operation.is_nil()
                && !transaction.is_nil()
                && valid_hash(reservation_row_sha256),
        )?;
        let held = permit()?;
        check_cancel(cancel)?;
        let package = package.into_verified();
        let encrypted_sha256 = package
            .encrypted_sha256()
            .filter(|s| valid_hash(s))
            .ok_or_else(recovery)?
            .to_owned();
        let summary = package.summary();
        let expected = package.revision().clone();
        supported_tree(&expected)?;
        // Fresh purpose has NO fictitious old row, original root or dependency
        // preview. Native's later same-transaction SQL CAS requires ID ABSENT.
        let namespace = namespace::ManagedNamespace::fresh_project(profile, project, true)?;
        namespace.check()?;
        require(namespace.root.entries()?.next().transpose()?.is_none())?;
        let intent = ImportIntent {
            version: 1,
            purpose: ImportPurpose::PortableNewProjectV1,
            transaction,
            parent: identity(&namespace.root)?,
            facts: ImportFacts {
                project,
                operation,
                reservation_row_sha256: reservation_row_sha256.into(),
                encrypted_sha256,
                portable_digest: summary.portable_digest,
                source_digest: summary.source_digest,
                files: summary.files,
                bytes: summary.bytes,
            },
        };
        write_new(&namespace.root, INTENT, &intent)?;
        check_cancel(cancel)?;
        namespace.check()?;
        let staging = ProjectRoot {
            root: namespace::mkdir(&namespace.root, &intent.stage())?,
            #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
            publication_boundary: true,
        };
        namespace::verify_private_child(&staging.root)?;
        let marker = intent.marker(identity(&staging.root)?)?;
        write_new(&namespace.root, CREATED, &marker)?;
        // Existing authenticated copier is the ONLY content writer. No second
        // extractor/walker, source-kind conversion or imported ID resolution.
        let written = package.write_uncommitted(&staging, cancel)?;
        require(written == expected && staging.revision_for_publication()? == expected)?;
        namespace.check()?;
        sync_tree(&staging, &expected)?;
        check_cancel(cancel)?;
        write_new(&namespace.root, READY, &marker)?;
        drop(staging); // preserve existing Windows no-DELETE-share semantics
        Ok(Self {
            namespace,
            profile: profile.to_owned(),
            intent,
            marker,
            expected,
            _permit: held,
        })
    }
    pub(crate) fn fingerprint(&self) -> Result<String> {
        self.recheck_staged()?;
        Ok(hash(&encode(&(
            hash(&encode(&self.intent)?),
            hash(&encode(&self.marker)?),
        ))?))
    }
    fn recheck_staged(&self) -> Result<()> {
        self.namespace.check()?;
        check_entries(&self.namespace, &self.intent)?;
        require(
            parsed::<ImportIntent>(&read_small(&self.namespace.root, INTENT)?)? == self.intent,
        )?;
        for name in [CREATED, READY] {
            require(parsed::<Marker>(&read_small(&self.namespace.root, name)?)? == self.marker)?;
        }
        require(
            !exists(&self.namespace.root, &self.intent.target())?
                && !exists(&self.namespace.root, PUBLISHED)?,
        )?;
        let stage = ProjectRoot {
            root: namespace::open_child(&self.namespace.root, &self.intent.stage())?,
            #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
            publication_boundary: true,
        };
        require(
            identity(&stage.root)? == self.marker.stage
                && stage.revision_for_publication()? == self.expected,
        )?;
        namespace::verify_private_child(&stage.root)
    }
    pub(crate) fn publish(
        self,
        prepared_ack: &crate::dependency_publication::CatalogPublicationAck,
        cancel: &CancellationToken,
    ) -> Result<PublishedPortableImport> {
        self.recheck_staged()?;
        let (project, transaction, revision, row) = prepared_ack.selected_facts();
        require(
            project == self.intent.facts.project
                && transaction == self.intent.transaction
                && revision == 2
                && valid_hash(row),
        )?;
        self.namespace.check()?;
        check_entries(&self.namespace, &self.intent)?;
        let raw = read_small(&self.namespace.root, INTENT)?;
        require(parsed::<ImportIntent>(&raw)? == self.intent)?;
        require(
            parsed::<Marker>(&read_small(&self.namespace.root, CREATED)?)? == self.marker
                && parsed::<Marker>(&read_small(&self.namespace.root, READY)?)? == self.marker,
        )?;
        require(!exists(&self.namespace.root, &self.intent.target())?)?;
        let stage = ProjectRoot {
            root: namespace::open_child(&self.namespace.root, &self.intent.stage())?,
            #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
            publication_boundary: true,
        };
        require(
            identity(&stage.root)? == self.marker.stage
                && stage.revision_for_publication()? == self.expected,
        )?;
        namespace::verify_private_child(&stage.root)?;
        drop(stage);
        self.namespace.check()?;
        check_cancel(cancel)?;
        rename_new(
            &self.namespace.root,
            &self.intent.stage(),
            &self.intent.target(),
            &self.marker.stage,
        )?;
        // Rename has happened: no cancellation-return before observation.
        sync_directory(&self.namespace.root)?;
        let target = ProjectRoot {
            root: namespace::open_child(&self.namespace.root, &self.intent.target())?,
            #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
            publication_boundary: true,
        };
        require(
            identity(&target.root)? == self.marker.stage
                && target.revision_for_publication()? == self.expected,
        )?;
        write_new(&self.namespace.root, PUBLISHED, &self.marker)?;
        self.namespace.check()?;
        // After this commit point errors mean retained generation/recovery,
        // never proof of rollback, retry permission or catalog installation.
        check_cancel(cancel)?;
        Ok(PublishedPortableImport {
            namespace: self.namespace,
            profile: self.profile,
            intent: self.intent,
            marker: self.marker,
            expected: self.expected,
            target,
            _permit: self._permit,
        })
    }
}
impl PublishedPortableImport {
    pub(crate) fn recheck(&self) -> Result<()> {
        self.namespace.check()?;
        check_entries(&self.namespace, &self.intent)?;
        require(
            parsed::<ImportIntent>(&read_small(&self.namespace.root, INTENT)?)? == self.intent,
        )?;
        for name in [CREATED, READY, PUBLISHED] {
            require(parsed::<Marker>(&read_small(&self.namespace.root, name)?)? == self.marker)?;
        }
        require(!exists(&self.namespace.root, &self.intent.stage())?)?;
        let named = namespace::open_child(&self.namespace.root, &self.intent.target())?;
        require(
            identity(&named)? == self.marker.stage
                && identity(&self.target.root)? == self.marker.stage,
        )?;
        namespace::verify_private_child(&named)?;
        require(self.target.revision_for_publication()? == self.expected)
    }
    pub(crate) fn project(&self) -> Uuid {
        self.intent.facts.project
    }
    pub(crate) fn transaction(&self) -> Uuid {
        self.intent.transaction
    }
    pub(crate) fn digest(&self) -> &str {
        &self.expected.digest
    }
    pub(crate) fn target_path(&self) -> PathBuf {
        self.profile
            .join("automation")
            .join("project-revisions")
            .join(self.project().to_string())
            .join(self.intent.target())
    }
    pub(crate) fn fingerprint(&self) -> Result<String> {
        self.recheck()?;
        Ok(hash(&encode(&(
            hash(&encode(&self.intent)?),
            hash(&encode(&self.marker)?),
        ))?))
    }
    pub(crate) fn record_catalog_ack(
        &self,
        ack: &crate::dependency_publication::CatalogPublicationAck,
    ) -> Result<()> {
        self.record(ack, CATALOG_COMMITTED)
    }
    pub(crate) fn record_complete(
        &self,
        ack: &crate::dependency_publication::CatalogPublicationAck,
    ) -> Result<()> {
        self.recheck()?;
        let expected = self.audit(ack)?;
        require(
            exists(&self.namespace.root, CATALOG_COMMITTED)?
                && parsed::<ImportCatalogAudit>(&read_small(
                    &self.namespace.root,
                    CATALOG_COMMITTED,
                )?)? == expected,
        )?;
        self.record(ack, COMPLETE)
    }
    fn audit(
        &self,
        ack: &crate::dependency_publication::CatalogPublicationAck,
    ) -> Result<ImportCatalogAudit> {
        let (project, transaction, revision, row) = ack.selected_facts();
        require(
            project == self.project()
                && transaction == self.transaction()
                && revision == 3
                && valid_hash(row),
        )?;
        Ok(ImportCatalogAudit {
            version: 1,
            purpose: ImportPurpose::PortableNewProjectV1,
            before: CatalogBefore::Absent,
            intent_sha256: hash(&encode(&self.intent)?),
            project,
            transaction,
            selected_revision: revision,
            selected_row_sha256: row.into(),
            selected_digest: self.expected.digest.clone(),
            selected: self.marker.stage.clone(),
        })
    }
    fn record(
        &self,
        ack: &crate::dependency_publication::CatalogPublicationAck,
        name: &str,
    ) -> Result<()> {
        self.recheck()?;
        let expected = self.audit(ack)?;
        if exists(&self.namespace.root, name)? {
            require(
                parsed::<ImportCatalogAudit>(&read_small(&self.namespace.root, name)?)? == expected,
            )?;
        } else {
            write_new(&self.namespace.root, name, &expected)?;
        }
        self.recheck()
    }
}

#[cfg(test)]
mod tests;
