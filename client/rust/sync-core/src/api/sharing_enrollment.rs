//! Typed, sender-constrained owner-online enrollment transport. All transcripts
//! contain public identities, opaque challenges and signatures only. Structural
//! reply validation does not replace owner pins, pairing or possession checks.

use super::{to_json, ApiClient, ApiError};
use cc_protocol::sharing::{SharedItemKind, SharingCapabilities};
use cc_protocol::sharing_enrollment::*;
use reqwest::Method;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OwnDeviceEnrollmentApi {
    client: ApiClient,
    scope: EnrollmentScope,
    capabilities: SharingCapabilities,
}
fn config(reason: &'static str) -> ApiError {
    ApiError::InvalidConfig(reason.into())
}
fn invalid(reason: &'static str) -> ApiError {
    ApiError::InvalidResponse {
        status: 200,
        reason: reason.into(),
    }
}
fn outgoing<T>(result: Result<T, EnrollmentValidationError>) -> Result<T, ApiError> {
    result.map_err(|_| config("invalid enrollment request"))
}
fn incoming<T>(result: Result<T, EnrollmentValidationError>) -> Result<T, ApiError> {
    result.map_err(|_| invalid("invalid enrollment response"))
}
fn page_limit(limit: u32) -> Result<(), ApiError> {
    if !(1..=MAX_PAGE_SIZE as u32).contains(&limit) {
        Err(config("enrollment page limit must be between 1 and 100"))
    } else {
        Ok(())
    }
}

impl ApiClient {
    /// Scope must come from an independently verified item/pairing bundle;
    /// this constructor neither reads an item body nor discovers target keys.
    /// General sharing remains mandatory. Disabled kind/enrollment flags still
    /// permit narrow owner control-plane grant reads and terminal revocation.
    pub async fn sharing_enrollment(
        &self,
        scope: EnrollmentScope,
    ) -> Result<OwnDeviceEnrollmentApi, ApiError> {
        outgoing(validate_scope(&scope))?;
        let transport = self.sharing(scope.server_instance_id).await?;
        Ok(OwnDeviceEnrollmentApi {
            client: self.clone(),
            scope,
            capabilities: transport.capabilities().clone(),
        })
    }
}
impl OwnDeviceEnrollmentApi {
    pub fn scope(&self) -> &EnrollmentScope {
        &self.scope
    }
    pub fn capabilities(&self) -> &SharingCapabilities {
        &self.capabilities
    }
    fn active(&self) -> Result<(), ApiError> {
        let kind = match self.scope.kind {
            SharedItemKind::Host | SharedItemKind::Snippet => true,
            SharedItemKind::Group => self.capabilities.supports_groups,
            SharedItemKind::Secret => self.capabilities.supports_secrets,
        };
        if !self.capabilities.supports_owner_online_enrollment_v1 || !kind {
            return Err(config("owner-online enrollment is disabled"));
        }
        Ok(())
    }
    pub async fn recheck_capabilities(&self) -> Result<(), ApiError> {
        let current = self.client.sharing(self.scope.server_instance_id).await?;
        let caps = current.capabilities();
        if caps.supports_groups != self.capabilities.supports_groups
            || caps.supports_secrets != self.capabilities.supports_secrets
            || caps.supports_owner_online_enrollment_v1
                != self.capabilities.supports_owner_online_enrollment_v1
        {
            return Err(config(
                "enrollment capabilities changed; reconnect before continuing",
            ));
        }
        Ok(())
    }
    fn url(&self, suffix: &str) -> Url {
        self.client
            .endpoint(&format!("/v1/shares/{}/{}", self.scope.share_id, suffix))
    }
    fn child_url(&self, collection: &str, id: Uuid, suffix: &str) -> Result<Url, ApiError> {
        if id.is_nil() {
            return Err(config("invalid enrollment child id"));
        }
        Ok(self.url(&format!("{collection}/{id}{suffix}")))
    }
    fn grant(
        &self,
        signed: &SignedSharingOwnDevicesGrantState,
        id: Option<Uuid>,
    ) -> Result<(), ApiError> {
        incoming(validate_signed_grant(signed))?;
        if signed.grant.scope != self.scope || id.is_some_and(|id| id != signed.grant.grant_id) {
            return Err(invalid("enrollment grant context differs"));
        }
        Ok(())
    }
    fn request(&self, state: &OwnDeviceRequestState, id: Option<Uuid>) -> Result<(), ApiError> {
        incoming(validate_request_state(state))?;
        if state.request.request.scope != self.scope
            || id.is_some_and(|id| id != state.request.request.request_id)
        {
            return Err(invalid("enrollment request context differs"));
        }
        Ok(())
    }
    fn page_url(&self, collection: &str, after: Option<Uuid>, limit: u32) -> Result<Url, ApiError> {
        page_limit(limit)?;
        if after.is_some_and(|id| id.is_nil()) {
            return Err(config("invalid enrollment cursor"));
        }
        let mut url = self.url(collection);
        {
            let mut query = url.query_pairs_mut();
            if let Some(after) = after {
                query.append_pair("after", &after.to_string());
            }
            query.append_pair("limit", &limit.to_string());
        }
        Ok(url)
    }
    /// Server permits only the original owner to use this read when flags are
    /// off. The response has no item body, target feed or recipient discovery.
    pub async fn list_grants(
        &self,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<OwnDevicesGrantPage, ApiError> {
        let page: OwnDevicesGrantPage = self
            .client
            .authed(
                Method::GET,
                self.page_url("own-device-grants", after, limit)?,
                None,
            )
            .await?;
        incoming(validate_grant_page(&page, &self.scope))?;
        if page.items.len() > limit as usize
            || page
                .items
                .first()
                .is_some_and(|first| after.is_some_and(|after| first.grant.grant_id <= after))
        {
            return Err(invalid("invalid enrollment grant page"));
        }
        Ok(page)
    }
    pub async fn get_grant(&self, id: Uuid) -> Result<SignedSharingOwnDevicesGrantState, ApiError> {
        let grant = self
            .client
            .authed(
                Method::GET,
                self.child_url("own-device-grants", id, "")?,
                None,
            )
            .await?;
        self.grant(&grant, Some(id))?;
        Ok(grant)
    }
    pub async fn grant_history(
        &self,
        id: Uuid,
        after_revision: u64,
        limit: u32,
    ) -> Result<OwnDevicesGrantHistoryPage, ApiError> {
        page_limit(limit)?;
        if after_revision > i64::MAX as u64 {
            return Err(config("invalid enrollment history revision"));
        }
        let mut url = self.child_url("own-device-grants", id, "/history")?;
        url.query_pairs_mut()
            .append_pair("after_revision", &after_revision.to_string())
            .append_pair("limit", &limit.to_string());
        let page: OwnDevicesGrantHistoryPage = self.client.authed(Method::GET, url, None).await?;
        incoming(validate_grant_history_page(
            &page,
            &self.scope,
            id,
            after_revision,
        ))?;
        if page.states.len() > limit as usize {
            return Err(invalid("enrollment history exceeds page limit"));
        }
        Ok(page)
    }
    pub async fn publish_grant(
        &self,
        request: &PublishOwnDevicesGrantRequest,
    ) -> Result<SignedSharingOwnDevicesGrantState, ApiError> {
        outgoing(validate_signed_grant(&request.grant))?;
        if request.grant.grant.scope != self.scope {
            return Err(config("enrollment grant belongs to another context"));
        }
        if request.grant.grant.status != EnrollmentGrantStatus::Revoked {
            self.active()?;
        }
        let grant = self
            .client
            .authed(
                Method::POST,
                self.url("own-device-grants"),
                Some(to_json(request)?),
            )
            .await?;
        self.grant(&grant, Some(request.grant.grant.grant_id))?;
        if grant != request.grant {
            return Err(invalid(
                "published enrollment grant differs from signed request",
            ));
        }
        Ok(grant)
    }
    pub async fn list_requests(
        &self,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<OwnDeviceRequestPage, ApiError> {
        self.active()?;
        let page: OwnDeviceRequestPage = self
            .client
            .authed(
                Method::GET,
                self.page_url("own-device-requests", after, limit)?,
                None,
            )
            .await?;
        incoming(validate_request_page(&page, &self.scope))?;
        if page.items.len() > limit as usize
            || page.items.first().is_some_and(|first| {
                after.is_some_and(|after| first.request.request.request_id <= after)
            })
        {
            return Err(invalid("invalid enrollment request page"));
        }
        Ok(page)
    }
    pub async fn get_request(&self, id: Uuid) -> Result<OwnDeviceRequestState, ApiError> {
        self.active()?;
        let state = self
            .client
            .authed(
                Method::GET,
                self.child_url("own-device-requests", id, "")?,
                None,
            )
            .await?;
        self.request(&state, Some(id))?;
        Ok(state)
    }
    pub async fn submit_request(
        &self,
        request: &SubmitOwnDeviceRequest,
    ) -> Result<OwnDeviceRequestState, ApiError> {
        self.active()?;
        outgoing(validate_submission(request))?;
        if request.request.request.scope != self.scope {
            return Err(config("enrollment request belongs to another context"));
        }
        let state = self
            .client
            .authed(
                Method::POST,
                self.url("own-device-requests"),
                Some(to_json(request)?),
            )
            .await?;
        self.request(&state, Some(request.request.request.request_id))?;
        if state.grant_id != request.grant_id
            || state.request != request.request
            || state.endorsement != request.endorsement
        {
            return Err(invalid("submitted enrollment transcript differs"));
        }
        Ok(state)
    }
    pub async fn publish_challenge(
        &self,
        id: Uuid,
        request: &PublishOwnDeviceChallengeRequest,
    ) -> Result<OwnDeviceRequestState, ApiError> {
        self.active()?;
        outgoing(validate_signed_challenge(&request.challenge))?;
        let state = self
            .client
            .authed(
                Method::POST,
                self.child_url("own-device-requests", id, "/challenge")?,
                Some(to_json(request)?),
            )
            .await?;
        self.request(&state, Some(id))?;
        if state.challenge.as_ref() != Some(&request.challenge) {
            return Err(invalid("published enrollment challenge differs"));
        }
        Ok(state)
    }
    pub async fn submit_response(
        &self,
        id: Uuid,
        request: &SubmitOwnDeviceChallengeResponseRequest,
    ) -> Result<OwnDeviceRequestState, ApiError> {
        self.active()?;
        outgoing(validate_signed_response(&request.response))?;
        let state = self
            .client
            .authed(
                Method::POST,
                self.child_url("own-device-requests", id, "/response")?,
                Some(to_json(request)?),
            )
            .await?;
        self.request(&state, Some(id))?;
        if state.response.as_ref() != Some(&request.response) {
            return Err(invalid("submitted enrollment response differs"));
        }
        Ok(state)
    }
    pub async fn accept_request(
        &self,
        id: Uuid,
        request: &AcceptOwnDeviceRequest,
    ) -> Result<OwnDeviceAcceptanceResult, ApiError> {
        self.active()?;
        outgoing(validate_accept_request(request))?;
        let manifest = &request.rotation.access.manifest;
        if manifest.server_instance_id != self.scope.server_instance_id
            || manifest.share_id != self.scope.share_id
            || manifest.item_id != self.scope.item_id
            || manifest.kind != self.scope.kind
        {
            return Err(config("enrollment acceptance belongs to another context"));
        }
        let result: OwnDeviceAcceptanceResult = self
            .client
            .authed(
                Method::POST,
                self.child_url("own-device-requests", id, "/accept")?,
                Some(to_json(request)?),
            )
            .await?;
        incoming(validate_acceptance_result(&result))?;
        if result.state.access != request.rotation.access
            || result.state.revision != request.rotation.revision
            || result.acceptance != request.acceptance
            || result.consumed_grant_successor != request.consumed_grant_successor
            || result.other_grant_successors != request.other_grant_successors
        {
            return Err(invalid(
                "accepted enrollment result differs from signed request",
            ));
        }
        Ok(result)
    }
}
