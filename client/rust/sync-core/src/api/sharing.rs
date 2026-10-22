//! Capability-gated transport for selective-object sharing.
//!
//! Wire replies are structurally validated, but remain untrusted. The caller
//! must verify the owner and author signatures and durable checkpoints in
//! crypto-core before decrypting or showing any shared payload.

use super::{to_json, ApiClient, ApiError, SignerError};
use cc_protocol::sharing::{
    self, CreateShareRequest, DeletedShareListPage, PutSharedRevisionRequest,
    RotateShareAccessRequest, ShareHistoryPage, ShareListPage, SharedItemKind, SharedItemState,
    SharingCapabilities, SharingOperation, SharingRecipient,
};
use cc_protocol::sharing_notes::{
    self, RekeySharedNotesRequest, SharedNote, SharedNotesKeySet, SharedNotesPage,
};
use cc_protocol::{DeviceId, ObjectId, ShareId, UserId};
use reqwest::Method;
use std::collections::HashSet;
use url::Url;
use uuid::Uuid;

/// A sharing transport bound to a validated, pinned server instance. Its
/// constructor is private: absence of support never falls back to personal
/// vault endpoints or an unsigned request.
/// Extension kinds requested from `GET /v1/shares`; Host and Snippet are
/// always listed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShareKinds {
    pub groups: bool,
    pub secrets: bool,
    pub kube_contexts: bool,
    pub db_connections: bool,
}

/// `GET /v1/shares` as received: entries are decoded one by one, so one
/// malformed entry does not hide the rest of the page.
#[derive(serde::Deserialize)]
struct RawShareListPage {
    items: Vec<serde_json::Value>,
    next_after: Option<ShareId>,
    has_more: bool,
    #[serde(default)]
    departures: Vec<sharing::ShareDeparture>,
}

#[derive(Debug, Clone)]
pub struct SharingApi {
    client: ApiClient,
    capabilities: SharingCapabilities,
}

impl ApiClient {
    /// Authenticated discovery only; this does not grant access or trust any
    /// device key returned by the server directory.
    pub async fn sharing_capabilities(&self) -> Result<SharingCapabilities, ApiError> {
        require_signer(self)?;
        self.authed(Method::GET, self.endpoint("/v1/shares/capabilities"), None)
            .await
    }

    /// Enable sharing transport after comparing the server UUID with the
    /// locally pinned instance. A nil UUID, disabled feature, incompatible
    /// format, or replaced instance fails before any item operation.
    pub async fn sharing(&self, expected_instance_id: Uuid) -> Result<SharingApi, ApiError> {
        if expected_instance_id.is_nil() {
            return Err(config_error("a pinned sharing server instance is required"));
        }
        let capabilities = self.sharing_capabilities().await?;
        validate_capabilities(&capabilities, expected_instance_id)?;
        Ok(SharingApi {
            client: self.clone(),
            capabilities,
        })
    }
}

impl SharingApi {
    pub fn capabilities(&self) -> &SharingCapabilities {
        &self.capabilities
    }

    /// Recheck support after reconnecting. An instance replacement requires
    /// explicit new trust; it is never accepted as a routine server restart.
    pub async fn recheck_capabilities(&self) -> Result<(), ApiError> {
        let current = self.client.sharing_capabilities().await?;
        validate_capabilities(&current, self.capabilities.server_instance_id)?;
        if current.supports_groups != self.capabilities.supports_groups
            || current.supports_secrets != self.capabilities.supports_secrets
            || current.supports_kube_contexts != self.capabilities.supports_kube_contexts
            || current.supports_db_connections != self.capabilities.supports_db_connections
            || current.supports_owner_online_enrollment_v1
                != self.capabilities.supports_owner_online_enrollment_v1
        {
            return Err(config_error(
                "sharing capabilities changed; reconnect before continuing",
            ));
        }
        Ok(())
    }

    pub async fn list_shares(
        &self,
        after: Option<ShareId>,
        limit: u32,
    ) -> Result<ShareListPage, ApiError> {
        self.list_shares_with_kinds(after, limit, false, false)
            .await
    }

    /// Explicit support negotiation keeps older Host/Snippet callers intact.
    pub async fn list_shares_with_kinds(
        &self,
        after: Option<ShareId>,
        limit: u32,
        include_groups: bool,
        include_secrets: bool,
    ) -> Result<ShareListPage, ApiError> {
        self.list_shares_with_extensions(after, limit, include_groups, include_secrets, false)
            .await
    }

    pub async fn list_shares_with_extensions(
        &self,
        after: Option<ShareId>,
        limit: u32,
        include_groups: bool,
        include_secrets: bool,
        include_kube_contexts: bool,
    ) -> Result<ShareListPage, ApiError> {
        self.list_shares_including(
            after,
            limit,
            ShareKinds {
                groups: include_groups,
                secrets: include_secrets,
                kube_contexts: include_kube_contexts,
                db_connections: false,
            },
        )
        .await
    }

    /// Every extension kind is requested explicitly and only when advertised;
    /// the server must not return an unrequested kind.
    pub async fn list_shares_including(
        &self,
        after: Option<ShareId>,
        limit: u32,
        kinds: ShareKinds,
    ) -> Result<ShareListPage, ApiError> {
        self.list_shares_checked(after, limit, kinds)
            .await
            .map(|(page, _)| page)
    }

    /// [`SharingApi::list_shares_including`] plus how many entries were
    /// skipped because they could not be decoded or failed structural
    /// validation. Ordering, cursors and kinds of the readable entries are
    /// still enforced for the whole page.
    pub async fn list_shares_checked(
        &self,
        after: Option<ShareId>,
        limit: u32,
        kinds: ShareKinds,
    ) -> Result<(ShareListPage, u32), ApiError> {
        let ShareKinds {
            groups: include_groups,
            secrets: include_secrets,
            kube_contexts: include_kube_contexts,
            db_connections: include_db_connections,
        } = kinds;
        self.ready()?;
        if (include_groups && !self.capabilities.supports_groups)
            || (include_secrets && !self.capabilities.supports_secrets)
            || (include_kube_contexts && !self.capabilities.supports_kube_contexts)
            || (include_db_connections && !self.capabilities.supports_db_connections)
        {
            return Err(config_error("requested sharing kind is unavailable"));
        }
        page_limit(limit)?;
        if after == Some(ShareId::NIL) {
            return Err(config_error("invalid sharing list cursor"));
        }
        let mut url = self.client.endpoint(sharing::API_PATH);
        {
            let mut query = url.query_pairs_mut();
            if let Some(after) = after {
                query.append_pair("after", &after.to_string());
            }
            query.append_pair("limit", &limit.to_string());
            if include_groups {
                query.append_pair("include_groups", "true");
            }
            if include_kube_contexts {
                query.append_pair("include_kube_contexts", "true");
            }
            if include_db_connections {
                query.append_pair("include_db_connections", "true");
            }
            if include_secrets {
                query.append_pair("include_secrets", "true");
            }
        }
        let raw: RawShareListPage = self.client.authed(Method::GET, url, None).await?;
        let raw_count = raw.items.len();
        if raw.items.len() > limit as usize {
            return Err(response_error("sharing page exceeds limit"));
        }
        let mut skipped = 0u32;
        let mut items = Vec::with_capacity(raw.items.len());
        for value in raw.items {
            match serde_json::from_value::<SharedItemState>(value) {
                Ok(item) if self.state(&item, None).is_ok() => items.push(item),
                _ => skipped += 1,
            }
        }
        if skipped > 0 {
            tracing::warn!(skipped, "unreadable shared entries skipped");
        }
        let page = ShareListPage {
            items,
            next_after: raw.next_after,
            has_more: raw.has_more,
            departures: raw.departures,
        };
        let mut previous = after;
        for item in &page.items {
            if (item.access.manifest.kind == SharedItemKind::Group && !include_groups)
                || (item.access.manifest.kind == SharedItemKind::Secret && !include_secrets)
                || (item.access.manifest.kind == SharedItemKind::KubeContext
                    && !include_kube_contexts)
                || (item.access.manifest.kind == SharedItemKind::DbConnection
                    && !include_db_connections)
            {
                return Err(response_error("unrequested sharing kind"));
            }
            let id = item.access.manifest.share_id;
            if previous.is_some_and(|previous| id <= previous) {
                return Err(response_error("sharing page is not ordered"));
            }
            previous = Some(id);
        }
        if page.has_more {
            let next = page
                .next_after
                .ok_or_else(|| response_error("missing sharing pagination cursor"))?;
            if after.is_some_and(|after| next <= after)
                || previous.is_some_and(|previous| next < previous)
            {
                return Err(response_error("invalid sharing pagination cursor"));
            }
        } else if page.next_after.is_some() {
            return Err(response_error(
                "unexpected terminal sharing pagination cursor",
            ));
        }
        // Departures name items of this page; entries skipped above take
        // their departures with them instead of failing the whole page.
        if page.departures.len() > raw_count * sharing::MAX_MEMBERS {
            return Err(response_error("invalid sharing departures"));
        }
        let mut page = page;
        let readable = page
            .items
            .iter()
            .map(|item| item.access.manifest.share_id)
            .collect::<Vec<_>>();
        let before = page.departures.len();
        page.departures
            .retain(|departure| readable.contains(&departure.share_id));
        if skipped == 0 && page.departures.len() != before {
            return Err(response_error("invalid sharing departures"));
        }
        Ok((page, skipped))
    }

    /// Protocol 1.8 (`capabilities.share_leave`): this account leaves a
    /// shared item on all its devices. Nothing is signed or re-encrypted.
    pub async fn leave_share(&self, share_id: ShareId) -> Result<(), ApiError> {
        self.ready()?;
        self.client
            .authed_empty(Method::POST, self.item_url(share_id, "/leave")?, None)
            .await
    }

    /// Protocol 1.8 (`capabilities.share_restore`): owner-deleted items whose
    /// last content is still retained. Structural checks only; the caller
    /// verifies signatures against its accepted tombstone before opening.
    pub async fn deleted_shares(
        &self,
        after: Option<ShareId>,
        limit: u32,
    ) -> Result<DeletedShareListPage, ApiError> {
        self.ready()?;
        page_limit(limit)?;
        if after == Some(ShareId::NIL) {
            return Err(config_error("invalid sharing list cursor"));
        }
        let mut url = self.client.endpoint(sharing::DELETED_PATH);
        {
            let mut query = url.query_pairs_mut();
            if let Some(after) = after {
                query.append_pair("after", &after.to_string());
            }
            query.append_pair("limit", &limit.to_string());
        }
        let page: DeletedShareListPage = self.client.authed(Method::GET, url, None).await?;
        if page.items.len() > limit as usize {
            return Err(response_error("sharing page exceeds limit"));
        }
        let mut previous = after;
        for item in &page.items {
            let id = item.state.access.manifest.share_id;
            self.state(&item.state, None)?;
            self.revision(&item.last_revision, id, false)?;
            let tombstone = &item.state.revision.signed.mutation;
            let last = &item.last_revision.signed.mutation;
            if tombstone.operation != SharingOperation::Delete
                || last.operation != SharingOperation::Put
                || last.context.revision.checked_add(1) != Some(tombstone.context.revision)
                || last.manifest_revision != tombstone.manifest_revision
                || item.restorable_until <= item.deleted_at
            {
                return Err(response_error("invalid deleted sharing item"));
            }
            if previous.is_some_and(|previous| id <= previous) {
                return Err(response_error("sharing page is not ordered"));
            }
            previous = Some(id);
        }
        if page.has_more {
            let next = page
                .next_after
                .ok_or_else(|| response_error("missing sharing pagination cursor"))?;
            if after.is_some_and(|after| next <= after) {
                return Err(response_error("invalid sharing pagination cursor"));
            }
        } else if page.next_after.is_some() {
            return Err(response_error(
                "unexpected terminal sharing pagination cursor",
            ));
        }
        Ok(page)
    }

    pub async fn create_share(
        &self,
        request: &CreateShareRequest,
    ) -> Result<SharedItemState, ApiError> {
        self.ready()?;
        let expected = SharedItemState {
            access: request.access.clone(),
            revision: request.revision.clone(),
        };
        self.outgoing(&expected)?;
        let state: SharedItemState = self
            .client
            .authed(
                Method::POST,
                self.client.endpoint(sharing::API_PATH),
                Some(to_json(request)?),
            )
            .await?;
        self.state(&state, Some(expected.access.manifest.share_id))?;
        if state != expected {
            return Err(response_error(
                "created sharing state differs from signed request",
            ));
        }
        Ok(state)
    }

    pub async fn get_share(&self, share_id: ShareId) -> Result<SharedItemState, ApiError> {
        self.ready()?;
        let state: SharedItemState = self
            .client
            .authed(Method::GET, self.item_url(share_id, "")?, None)
            .await?;
        self.state(&state, Some(share_id))?;
        Ok(state)
    }

    pub async fn put_shared_revision(
        &self,
        share_id: ShareId,
        request: &PutSharedRevisionRequest,
    ) -> Result<SharedItemState, ApiError> {
        self.ready()?;
        self.revision(&request.revision, share_id, true)?;
        let state: SharedItemState = self
            .client
            .authed(
                Method::POST,
                self.item_url(share_id, "/revisions")?,
                Some(to_json(request)?),
            )
            .await?;
        self.state(&state, Some(share_id))?;
        if state.revision != request.revision {
            return Err(response_error(
                "accepted sharing revision differs from signed request",
            ));
        }
        Ok(state)
    }

    pub async fn rotate_shared_access(
        &self,
        share_id: ShareId,
        request: &RotateShareAccessRequest,
    ) -> Result<SharedItemState, ApiError> {
        self.ready()?;
        let expected = SharedItemState {
            access: request.access.clone(),
            revision: request.revision.clone(),
        };
        self.outgoing(&expected)?;
        if expected.access.manifest.share_id != share_id {
            return Err(config_error("sharing access request id mismatch"));
        }
        let state: SharedItemState = self
            .client
            .authed(
                Method::POST,
                self.item_url(share_id, "/access")?,
                Some(to_json(request)?),
            )
            .await?;
        self.state(&state, Some(share_id))?;
        if state != expected {
            return Err(response_error(
                "rotated sharing state differs from signed request",
            ));
        }
        Ok(state)
    }

    fn notes(&self) -> Result<(), ApiError> {
        self.ready()?;
        if !self.capabilities.supports_notes_v1 {
            return Err(config_error("shared notes are not offered by this server"));
        }
        Ok(())
    }

    /// Notes of a share and the key sets of their epochs (untrusted until the
    /// owner and author signatures are verified).
    pub async fn share_notes(&self, share_id: ShareId) -> Result<SharedNotesPage, ApiError> {
        self.notes()?;
        let page: SharedNotesPage = self
            .client
            .authed(Method::GET, self.item_url(share_id, "/notes")?, None)
            .await?;
        if page
            .key_sets
            .iter()
            .any(|k| sharing_notes::validate_key_set(k).is_err() || k.share_id != share_id)
            || page
                .notes
                .iter()
                .any(|n| sharing_notes::validate_note(n).is_err())
            || page.notes.len() > sharing_notes::MAX_NOTES_PER_SHARE
        {
            return Err(response_error("invalid shared notes page"));
        }
        Ok(page)
    }

    pub async fn put_share_notes_key(
        &self,
        share_id: ShareId,
        set: &SharedNotesKeySet,
    ) -> Result<(), ApiError> {
        self.notes()?;
        if set.share_id != share_id || sharing_notes::validate_key_set(set).is_err() {
            return Err(config_error("invalid shared notes key set"));
        }
        self.client
            .authed_empty(
                Method::PUT,
                self.item_url(share_id, "/notes/keys")?,
                Some(to_json(set)?),
            )
            .await
    }

    pub async fn put_share_note(
        &self,
        share_id: ShareId,
        note: &SharedNote,
    ) -> Result<(), ApiError> {
        self.notes()?;
        if sharing_notes::validate_note(note).is_err() {
            return Err(config_error("invalid shared note"));
        }
        self.client
            .authed_empty(
                Method::PUT,
                self.item_url(share_id, &format!("/notes/{}", note.note_id))?,
                Some(to_json(note)?),
            )
            .await
    }

    pub async fn delete_share_note(
        &self,
        share_id: ShareId,
        note_id: ObjectId,
    ) -> Result<(), ApiError> {
        self.notes()?;
        self.client
            .authed_empty(
                Method::DELETE,
                self.item_url(share_id, &format!("/notes/{note_id}"))?,
                None,
            )
            .await
    }

    pub async fn rekey_share_notes(
        &self,
        share_id: ShareId,
        request: &RekeySharedNotesRequest,
    ) -> Result<(), ApiError> {
        self.notes()?;
        if sharing_notes::validate_rekey(request).is_err() {
            return Err(config_error("invalid shared notes rekey"));
        }
        self.client
            .authed_empty(
                Method::POST,
                self.item_url(share_id, "/notes/rekey")?,
                Some(to_json(request)?),
            )
            .await
    }

    pub async fn share_history(
        &self,
        share_id: ShareId,
        after_manifest: u64,
        after_revision: i64,
        limit: u32,
    ) -> Result<ShareHistoryPage, ApiError> {
        self.ready()?;
        page_limit(limit)?;
        if after_manifest > i64::MAX as u64 || after_revision < 0 {
            return Err(config_error("invalid sharing history cursor"));
        }
        let mut url = self.item_url(share_id, "/history")?;
        url.query_pairs_mut()
            .append_pair("after_manifest", &after_manifest.to_string())
            .append_pair("after_revision", &after_revision.to_string())
            .append_pair("limit", &limit.to_string());
        let page: ShareHistoryPage = self.client.authed(Method::GET, url, None).await?;
        if page.manifests.len() > limit as usize
            || page.revisions.len() > limit as usize
            || page.latest_manifest_revision < after_manifest
            || page.latest_revision < after_revision
        {
            return Err(response_error("invalid sharing history bounds"));
        }
        let mut manifest_cursor = after_manifest;
        let mut revision_cursor = after_revision;
        let mut item_id = None;
        for access in &page.manifests {
            self.access(access, Some(share_id))?;
            manifest_cursor = manifest_cursor
                .checked_add(1)
                .ok_or_else(|| response_error("sharing history overflow"))?;
            if access.manifest.revision != manifest_cursor {
                return Err(response_error("sharing manifest history has a gap"));
            }
            check_item_id(&mut item_id, access.manifest.item_id)?;
        }
        for signed in &page.revisions {
            self.header(signed, share_id, false)?;
            revision_cursor = revision_cursor
                .checked_add(1)
                .ok_or_else(|| response_error("sharing history overflow"))?;
            if signed.mutation.context.revision != revision_cursor {
                return Err(response_error("sharing revision history has a gap"));
            }
            check_item_id(&mut item_id, signed.mutation.context.item_id)?;
        }
        if manifest_cursor > page.latest_manifest_revision
            || revision_cursor > page.latest_revision
            || page.has_more
                != (manifest_cursor < page.latest_manifest_revision
                    || revision_cursor < page.latest_revision)
        {
            return Err(response_error("inconsistent sharing history cursor"));
        }
        Ok(page)
    }

    /// Exact-email lookup, percent-encoded by url::Url. Returned public keys
    /// are discovery candidates only and must pass a human code comparison.
    /// Deprecated by protocol 1.8: prefer [`Self::sharing_recipient_lookup`].
    pub async fn sharing_recipient(&self, email: &str) -> Result<SharingRecipient, ApiError> {
        self.sharing_recipient_lookup(email, false).await
    }

    /// Exact-email lookup. With `post` (server `capabilities.
    /// recipient_lookup_post`, protocol 1.8) the address travels only in the
    /// JSON body of `POST /v1/shares/recipients/lookup`, never in a URL;
    /// otherwise the legacy `GET ?email=` is used for older servers.
    pub async fn sharing_recipient_lookup(
        &self,
        email: &str,
        post: bool,
    ) -> Result<SharingRecipient, ApiError> {
        self.ready()?;
        let email = email.trim();
        if email.is_empty() || email.len() > 320 || email.chars().any(char::is_control) {
            return Err(config_error("invalid sharing recipient email"));
        }
        let recipient: SharingRecipient = if post {
            let body = to_json(&sharing::RecipientLookupRequest {
                email: email.to_owned(),
            })?;
            self.client
                .authed(
                    Method::POST,
                    self.client.endpoint(sharing::RECIPIENT_LOOKUP_PATH),
                    Some(body),
                )
                .await?
        } else {
            let mut url = self.client.endpoint("/v1/shares/recipients");
            url.query_pairs_mut().append_pair("email", email);
            self.client.authed(Method::GET, url, None).await?
        };
        if recipient.user_id == UserId::NIL || recipient.devices.len() > sharing::MAX_MEMBERS {
            return Err(response_error("invalid sharing recipient"));
        }
        let mut devices = HashSet::new();
        for device in &recipient.devices {
            sharing::validate_member(device)
                .map_err(|_| response_error("invalid sharing recipient device"))?;
            if device.user_id != recipient.user_id || !devices.insert(device.device_id) {
                return Err(response_error("sharing recipient identity mismatch"));
            }
        }
        Ok(recipient)
    }

    fn ready(&self) -> Result<(), ApiError> {
        require_signer(&self.client)
    }

    fn item_url(&self, id: ShareId, suffix: &str) -> Result<Url, ApiError> {
        if id == ShareId::NIL {
            return Err(config_error("invalid sharing item id"));
        }
        Ok(self
            .client
            .endpoint(&format!("{}/{id}{suffix}", sharing::API_PATH)))
    }

    fn outgoing(&self, state: &SharedItemState) -> Result<(), ApiError> {
        self.outgoing_kind(state.access.manifest.kind)?;
        self.state(state, Some(state.access.manifest.share_id))
            .map_err(|_| config_error("invalid signed sharing request"))
    }

    fn outgoing_kind(&self, kind: SharedItemKind) -> Result<(), ApiError> {
        match kind {
            SharedItemKind::Group if !self.capabilities.supports_groups => {
                Err(config_error("group sharing is not enabled"))
            }
            SharedItemKind::KubeContext if !self.capabilities.supports_kube_contexts => {
                Err(config_error("Kubernetes sharing is not enabled"))
            }
            SharedItemKind::DbConnection if !self.capabilities.supports_db_connections => {
                Err(config_error("database connection sharing is not enabled"))
            }
            SharedItemKind::Secret if !self.capabilities.supports_secrets => {
                Err(config_error("secret sharing is not enabled"))
            }
            _ => Ok(()),
        }
    }

    fn access(
        &self,
        access: &cc_protocol::sharing::SignedAccessManifest,
        expected: Option<ShareId>,
    ) -> Result<(), ApiError> {
        sharing::validate_manifest(&access.manifest)
            .map_err(|_| response_error("malformed sharing access manifest"))?;
        let manifest = &access.manifest;
        self.outgoing_kind(manifest.kind)?;
        if access.signature.len() != 64
            || manifest.server_instance_id != self.capabilities.server_instance_id
            || manifest.revision != manifest.access_epoch
            || expected.is_some_and(|id| id != manifest.share_id)
            || manifest.members.len() > self.capabilities.max_members as usize
        {
            return Err(response_error("sharing access context mismatch"));
        }
        Ok(())
    }

    fn header(
        &self,
        header: &cc_protocol::sharing::SignedSharingMutation,
        expected: ShareId,
        outgoing: bool,
    ) -> Result<(), ApiError> {
        sharing::validate_mutation(&header.mutation).map_err(|_| {
            if outgoing {
                config_error("malformed sharing revision header")
            } else {
                response_error("malformed sharing revision header")
            }
        })?;
        if header.signature.len() != 64
            || header.mutation.context.server_instance_id != self.capabilities.server_instance_id
            || header.mutation.context.share_id != expected
        {
            return Err(response_error("sharing revision context mismatch"));
        }
        if outgoing {
            self.outgoing_kind(header.mutation.context.kind)?;
        }
        Ok(())
    }

    fn revision(
        &self,
        revision: &cc_protocol::sharing::SharedRevision,
        expected: ShareId,
        outgoing: bool,
    ) -> Result<(), ApiError> {
        self.header(&revision.signed, expected, outgoing)?;
        match (revision.signed.mutation.operation, revision.body.as_ref()) {
            (sharing::SharingOperation::Put, Some(body)) => {
                sharing::validate_body(body)
                    .map_err(|_| response_error("malformed shared encrypted body"))?;
                if body.ciphertext.len() > self.capabilities.max_ciphertext_bytes as usize {
                    return Err(response_error("shared encrypted body exceeds server limit"));
                }
            }
            (sharing::SharingOperation::Delete, None) => {}
            _ => return Err(response_error("sharing operation and body differ")),
        }
        Ok(())
    }

    fn state(&self, state: &SharedItemState, expected: Option<ShareId>) -> Result<(), ApiError> {
        self.access(&state.access, expected)?;
        let manifest = &state.access.manifest;
        self.revision(&state.revision, manifest.share_id, false)?;
        let header = &state.revision.signed.mutation;
        if manifest.item_id != header.context.item_id
            || manifest.kind != header.context.kind
            || manifest.access_epoch != header.context.access_epoch
            || manifest.revision != header.manifest_revision
        {
            return Err(response_error("shared item header and manifest differ"));
        }
        if let Some(body) = &state.revision.body {
            let members: HashSet<DeviceId> = manifest.members.iter().map(|m| m.device_id).collect();
            let envelopes: HashSet<DeviceId> = body
                .envelopes
                .iter()
                .map(|e| e.recipient_device_id)
                .collect();
            if members != envelopes {
                return Err(response_error(
                    "shared body recipients differ from manifest",
                ));
            }
        }
        Ok(())
    }
}

fn validate_capabilities(caps: &SharingCapabilities, expected: Uuid) -> Result<(), ApiError> {
    if !caps.enabled {
        return Err(config_error("server object sharing is disabled"));
    }
    if caps.format != sharing::FORMAT {
        return Err(config_error("unsupported server sharing format"));
    }
    if caps.server_instance_id != expected || caps.server_instance_id.is_nil() {
        return Err(config_error("sharing server instance changed"));
    }
    if caps.max_members == 0 || caps.max_ciphertext_bytes < 272 {
        return Err(response_error("invalid sharing capabilities limits"));
    }
    Ok(())
}

fn require_signer(client: &ApiClient) -> Result<(), ApiError> {
    if !client.has_request_signer() {
        return Err(ApiError::RequestSigning(SignerError::Unavailable));
    }
    Ok(())
}

fn page_limit(limit: u32) -> Result<(), ApiError> {
    if !(1..=sharing::MAX_PAGE_SIZE).contains(&limit) {
        return Err(config_error("sharing page limit must be between 1 and 100"));
    }
    Ok(())
}

fn check_item_id(
    previous: &mut Option<cc_protocol::ObjectId>,
    next: cc_protocol::ObjectId,
) -> Result<(), ApiError> {
    if previous.is_some_and(|id| id != next) {
        return Err(response_error("sharing history item identity changed"));
    }
    *previous = Some(next);
    Ok(())
}

fn config_error(reason: &'static str) -> ApiError {
    ApiError::InvalidConfig(reason.into())
}
fn response_error(reason: &'static str) -> ApiError {
    ApiError::InvalidResponse {
        status: 200,
        reason: reason.into(),
    }
}
