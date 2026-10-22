//! Protocol 1.8 membership actions: a recipient leaves a share; the owner
//! lists and restores items it deleted within the server's restore window.
//! A tombstone is never resurrected: a restore republishes the verified last
//! content as a new item, and recipients accept it again.
use super::*;
use cc_protocol::meta::ServerCapabilities;
use cc_protocol::sharing::DeletedSharedItem;

/// Bounded like `sharing_list`: at most 100 pages of 100 items.
const MAX_DELETED_PAGES: usize = 100;

struct OpenedDeleted {
    snapshot: SharingSnapshot,
    projection: SharedProjection,
    item: DeletedSharedItem,
}

impl OpenedDeleted {
    /// Previous recipients except this device and accounts that left.
    fn recipients(&self, own: DeviceId) -> Vec<SharingMember> {
        let departed: BTreeSet<UserId> = self.item.departed_user_ids.iter().copied().collect();
        self.snapshot
            .state()
            .access
            .manifest
            .members
            .iter()
            .filter(|member| member.device_id != own && !departed.contains(&member.user_id))
            .cloned()
            .collect()
    }
}

impl AppCore {
    async fn sharing_require_server(
        &self,
        env: &Environment,
        enabled: fn(&ServerCapabilities) -> bool,
    ) -> AppResult<()> {
        if !enabled(&self.sharing_server_capabilities(env).await?) {
            return Err(AppError::Unsupported(
                "this sharing action is not supported by the server".into(),
            ));
        }
        Ok(())
    }

    /// Leave a received item on all devices of this account. The local
    /// cached record and its index entry are removed; personal copies stay.
    pub async fn sharing_leave(&self, share_id: String) -> AppResult<()> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), true).await?;
        let id = ShareId::from_str(&share_id).map_err(|_| invalid("invalid shared item id"))?;
        self.sharing_require_server(&env, |caps| caps.share_leave)
            .await?;
        let store = env.store()?;
        let bindings: Vec<SharingBinding> = env
            .workspace
            .items
            .iter()
            .filter(|item| item.binding.share_id == id)
            .map(|item| item.binding.clone())
            .collect();
        for binding in &bindings {
            let cached = env
                .wait(async { store.load(binding).await.map_err(integrity) })
                .await;
            if let Ok(Some(snapshot)) = cached {
                if snapshot.owner_anchor().user_id == env.pin.user {
                    return Err(invalid(
                        "the owner deletes a shared item instead of leaving it",
                    ));
                }
            }
        }
        self.sharing_check(&env).await?;
        match env.raw(env.transport()?.leave_share(id)).await? {
            Ok(()) => {}
            // Already left (or removed by the owner): finish the local part.
            Err(error) if error.status() == Some(404) => {}
            Err(error) => return Err(error.into()),
        }
        self.sharing_check(&env).await?;
        env.workspace
            .items
            .retain(|item| item.binding.share_id != id);
        env.workspace
            .outbox
            .retain(|entry| entry.binding.share_id != id);
        self.sharing_save(&mut env).await?;
        for binding in &bindings {
            env.wait(async { store.forget(binding).await.map_err(integrity) })
                .await?;
        }
        self.sharing_check(&env).await?;
        Ok(())
    }

    /// Restorable items this owner device deleted; unverifiable entries are
    /// skipped (fail closed) rather than shown.
    pub async fn sharing_deleted(&self) -> AppResult<Vec<SharingDeletedItemDto>> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        self.sharing_require_server(&env, |caps| caps.share_restore)
            .await?;
        let store = env.store()?;
        let mut items = Vec::new();
        for item in self.sharing_deleted_items(&env).await? {
            let opened = match self.sharing_open_deleted(&env, &store, item).await {
                Ok(Some(opened)) => opened,
                Ok(None) => continue,
                Err(AppError::VaultLocked) => return Err(AppError::VaultLocked),
                Err(_) => continue,
            };
            items.push(SharingDeletedItemDto {
                share_id: opened.snapshot.binding().share_id.to_string(),
                kind: match opened.snapshot.binding().kind {
                    SharedItemKind::Host => SharingKindDto::Host,
                    SharedItemKind::Snippet => SharingKindDto::Snippet,
                    SharedItemKind::Group => SharingKindDto::Group,
                    SharedItemKind::Secret => SharingKindDto::Secret,
                    SharedItemKind::KubeContext => SharingKindDto::KubeContext,
                    SharedItemKind::DbConnection => SharingKindDto::DbConnection,
                },
                preview_json: Some(projection_preview_json(&opened.projection)?),
                deleted_at_ms: opened.item.deleted_at.timestamp_millis(),
                restorable_until_ms: opened.item.restorable_until.timestamp_millis(),
                recipients: opened.recipients(env.pin.device).len() as u32,
            });
        }
        self.sharing_check(&env).await?;
        Ok(items)
    }

    /// Republish a deleted item's verified last content as a new share with
    /// the same recipients (except those who left). Call `sharing_flush`.
    pub async fn sharing_restore(&self, share_id: String) -> AppResult<SharingItemDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let mut env = self.sharing_environment(session.clone(), true).await?;
        let id = ShareId::from_str(&share_id).map_err(|_| invalid("invalid shared item id"))?;
        self.sharing_require_server(&env, |caps| caps.share_restore)
            .await?;
        if env
            .workspace
            .outbox
            .iter()
            .any(|entry| entry.restored_from == Some(id))
        {
            return Err(invalid("this item is already being restored"));
        }
        let store = env.store()?;
        let item = self
            .sharing_deleted_items(&env)
            .await?
            .into_iter()
            .find(|item| item.state.access.manifest.share_id == id)
            .ok_or_else(|| AppError::not_found("deleted shared item", id))?;
        let opened = self
            .sharing_open_deleted(&env, &store, item)
            .await?
            .ok_or_else(|| AppError::not_found("deleted shared item", id))?;
        let mut members = vec![env.own_member()];
        members.extend(opened.recipients(env.pin.device));
        let OpenedDeleted { projection, .. } = opened;
        self.sharing_publish_members(&mut env, projection, members, Some(id))
            .await
    }

    async fn sharing_deleted_items(&self, env: &Environment) -> AppResult<Vec<DeletedSharedItem>> {
        let mut items = Vec::new();
        let mut after = None;
        for _ in 0..MAX_DELETED_PAGES {
            let page = env
                .wait(env.transport()?.deleted_shares(after, 100))
                .await?;
            self.sharing_check(env).await?;
            items.extend(page.items);
            if !page.has_more {
                return Ok(items);
            }
            after = page.next_after;
            if after.is_none() {
                return Err(invalid("invalid shared item cursor"));
            }
        }
        Err(invalid(
            "deleted shared item listing exceeds its page bound",
        ))
    }

    /// Bring the accepted local record to the signed tombstone, then accept
    /// the retained revision only if that tombstone's chain hash commits to
    /// it, and open it on this owner device.
    async fn sharing_open_deleted(
        &self,
        env: &Environment,
        store: &SharingStateStore,
        item: DeletedSharedItem,
    ) -> AppResult<Option<OpenedDeleted>> {
        let binding = binding_of(&item.state);
        if binding.server_instance_id != env.pin.instance
            || !env.supports(binding.kind)
            || !env
                .workspace
                .items
                .iter()
                .any(|indexed| indexed.binding == binding)
        {
            return Ok(None);
        }
        let snapshot = self
            .sharing_refresh_state(env, store, &binding, item.state.clone())
            .await?;
        if snapshot.owner_anchor() != env.own_anchor() {
            return Ok(None);
        }
        let tombstone = &snapshot.state().revision.signed.mutation;
        if snapshot.state() != &item.state || tombstone.operation != SharingOperation::Delete {
            return Err(invalid("deleted shared item differs from its tombstone"));
        }
        let manifest = trusted_manifest(&snapshot)?;
        let verified = verify_shared_mutation(&manifest, &item.last_revision.signed, None)
            .map_err(integrity)?;
        if verified.checkpoint().hash.as_slice() != tombstone.previous_revision_hash.as_slice()
            || verified.mutation().context.revision.checked_add(1)
                != Some(tombstone.context.revision)
        {
            return Err(integrity(
                "retained revision is not the tombstone's predecessor",
            ));
        }
        use cc_crypto_core::sharing::SharingRevisionOpener as _;
        let plain = env
            .identity
            .open_shared_revision(
                &manifest,
                &verified,
                item.last_revision.body.as_ref(),
                env.pin.device,
            )
            .map_err(integrity)?
            .ok_or_else(|| invalid("retained shared revision has no content"))?;
        let projection = SharedProjection::decode(binding.kind, plain.as_slice())?;
        drop(plain);
        self.sharing_check(env).await?;
        Ok(Some(OpenedDeleted {
            snapshot,
            projection,
            item,
        }))
    }
}
