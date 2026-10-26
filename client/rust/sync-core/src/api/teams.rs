//! Teams transport (protocol 1.9). Replies are untrusted: callers verify
//! the membership log in crypto-core (`TeamLogView`) before trusting any
//! member, role or key, and open the team key only from their own user
//! envelope.

use super::{to_json, ApiClient, ApiError};
use cc_protocol::paths;
use cc_protocol::teams::{
    AcceptTeamInvitationRequest, CreateTeamRequest, InviteTeamMemberRequest,
    ListTeamInvitationsResponse, ListTeamsResponse, SignedTeamLogEntry, SubmitTeamLogRequest,
    SubmitTeamLogResponse, TeamDetail, TeamInfo, TeamInvitation, TeamKeyMaterial, TeamLogPage,
};
use cc_protocol::{TeamInvitationId, VaultId};
use reqwest::Method;
use url::Url;

impl ApiClient {
    fn team_url(&self, template: &str, vault_id: VaultId) -> Url {
        self.endpoint(&paths::fill(
            template,
            &[("vault_id", &vault_id.to_string())],
        ))
    }

    fn invitation_url(&self, template: &str, id: TeamInvitationId) -> Url {
        self.endpoint(&paths::fill(
            template,
            &[("invitation_id", &id.to_string())],
        ))
    }

    /// `POST /v1/teams`.
    pub async fn create_team(&self, req: &CreateTeamRequest) -> Result<TeamInfo, ApiError> {
        self.authed(
            Method::POST,
            self.endpoint(paths::TEAMS),
            Some(to_json(req)?),
        )
        .await
    }

    /// `GET /v1/teams`.
    pub async fn list_teams(&self) -> Result<ListTeamsResponse, ApiError> {
        self.authed(Method::GET, self.endpoint(paths::TEAMS), None)
            .await
    }

    /// `GET /v1/teams/{vault_id}`.
    pub async fn get_team(&self, vault_id: VaultId) -> Result<TeamDetail, ApiError> {
        self.authed(Method::GET, self.team_url(paths::TEAM, vault_id), None)
            .await
    }

    /// The whole membership log (`GET /v1/teams/{vault_id}/log`, all pages).
    pub async fn team_log(&self, vault_id: VaultId) -> Result<Vec<SignedTeamLogEntry>, ApiError> {
        let mut out: Vec<SignedTeamLogEntry> = Vec::new();
        loop {
            let mut url = self.team_url(paths::TEAM_LOG, vault_id);
            if let Some(last) = out.last() {
                url.query_pairs_mut()
                    .append_pair("after", &last.entry.seq.to_string());
            }
            let page: TeamLogPage = self.authed(Method::GET, url, None).await?;
            let more = page.has_more && !page.entries.is_empty();
            out.extend(page.entries);
            if !more {
                return Ok(out);
            }
        }
    }

    /// `POST /v1/teams/{vault_id}/log`.
    pub async fn submit_team_log(
        &self,
        vault_id: VaultId,
        req: &SubmitTeamLogRequest,
    ) -> Result<SubmitTeamLogResponse, ApiError> {
        self.authed(
            Method::POST,
            self.team_url(paths::TEAM_LOG, vault_id),
            Some(to_json(req)?),
        )
        .await
    }

    /// `GET /v1/teams/{vault_id}/key`.
    pub async fn team_key(&self, vault_id: VaultId) -> Result<TeamKeyMaterial, ApiError> {
        self.authed(Method::GET, self.team_url(paths::TEAM_KEY, vault_id), None)
            .await
    }

    /// `PUT /v1/teams/{vault_id}/key-commitment` (protocol 1.12).
    pub async fn put_team_key_commitment(
        &self,
        vault_id: VaultId,
        req: &cc_protocol::teams::PutTeamKeyCommitmentRequest,
    ) -> Result<(), ApiError> {
        self.authed_empty(
            Method::PUT,
            self.team_url(paths::TEAM_KEY_COMMITMENT, vault_id),
            Some(to_json(req)?),
        )
        .await
    }

    /// `POST /v1/teams/{vault_id}/invitations` (always `202`).
    pub async fn invite_team_member(
        &self,
        vault_id: VaultId,
        req: &InviteTeamMemberRequest,
    ) -> Result<(), ApiError> {
        self.authed_empty(
            Method::POST,
            self.team_url(paths::TEAM_INVITATIONS, vault_id),
            Some(to_json(req)?),
        )
        .await
    }

    /// `GET /v1/teams/{vault_id}/invitations` (admins).
    pub async fn team_invitations(
        &self,
        vault_id: VaultId,
    ) -> Result<ListTeamInvitationsResponse, ApiError> {
        self.authed(
            Method::GET,
            self.team_url(paths::TEAM_INVITATIONS, vault_id),
            None,
        )
        .await
    }

    /// `GET /v1/teams/invitations` (incoming).
    pub async fn incoming_team_invitations(&self) -> Result<ListTeamInvitationsResponse, ApiError> {
        self.authed(
            Method::GET,
            self.endpoint(paths::TEAM_INVITATIONS_INBOX),
            None,
        )
        .await
    }

    /// `POST /v1/teams/invitations/{id}/accept`.
    pub async fn accept_team_invitation(
        &self,
        id: TeamInvitationId,
        req: &AcceptTeamInvitationRequest,
    ) -> Result<TeamInvitation, ApiError> {
        self.authed(
            Method::POST,
            self.invitation_url(paths::TEAM_INVITATION_ACCEPT, id),
            Some(to_json(req)?),
        )
        .await
    }

    /// `DELETE /v1/teams/invitations/{id}` (decline / cancel).
    pub async fn close_team_invitation(&self, id: TeamInvitationId) -> Result<(), ApiError> {
        self.authed_empty(
            Method::DELETE,
            self.invitation_url(paths::TEAM_INVITATION, id),
            None,
        )
        .await
    }
}
