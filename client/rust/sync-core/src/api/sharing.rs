//! Capability-gated transport for selective-object sharing.
//!
//! Wire replies are structurally validated, but remain untrusted. The caller
//! must verify the owner and author signatures and durable checkpoints in
//! crypto-core before decrypting or showing any shared payload.

use super::{to_json, ApiClient, ApiError, SignerError};
use cc_protocol::sharing::{
    self, CreateShareRequest, PutSharedRevisionRequest, RotateShareAccessRequest, ShareHistoryPage,
    ShareListPage, SharedItemKind, SharedItemState, SharingCapabilities, SharingRecipient,
};
use cc_protocol::{DeviceId, ShareId, UserId};
use reqwest::Method;
use std::collections::HashSet;
use url::Url;
use uuid::Uuid;

/// A sharing transport bound to a validated, pinned server instance. Its
/// constructor is private: absence of support never falls back to personal
/// vault endpoints or an unsigned request.
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
        self.ready()?;
        if (include_groups && !self.capabilities.supports_groups)
            || (include_secrets && !self.capabilities.supports_secrets)
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
            if include_secrets {
                query.append_pair("include_secrets", "true");
            }
        }
        let page: ShareListPage = self.client.authed(Method::GET, url, None).await?;
        if page.items.len() > limit as usize {
            return Err(response_error("sharing page exceeds limit"));
        }
        let mut previous = after;
        for item in &page.items {
            self.state(item, None)?;
            if (item.access.manifest.kind == SharedItemKind::Group && !include_groups)
                || (item.access.manifest.kind == SharedItemKind::Secret && !include_secrets)
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
    pub async fn sharing_recipient(&self, email: &str) -> Result<SharingRecipient, ApiError> {
        self.ready()?;
        let email = email.trim();
        if email.is_empty() || email.len() > 320 || email.chars().any(char::is_control) {
            return Err(config_error("invalid sharing recipient email"));
        }
        let mut url = self.client.endpoint("/v1/shares/recipients");
        url.query_pairs_mut().append_pair("email", email);
        let recipient: SharingRecipient = self.client.authed(Method::GET, url, None).await?;
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
