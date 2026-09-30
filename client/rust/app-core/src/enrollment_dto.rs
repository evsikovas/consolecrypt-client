//! Public-only UI values for owner-online own-device enrollment. These values
//! never contain challenge plaintext, object plaintext, DEKs or private keys.

use crate::sharing_dto::{SharingIdentityDto, SharingRoleDto};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnrollmentModeDto {
    Manual,
    Automatic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnrollmentGrantStatusDto {
    Active,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentGrantDto {
    pub share_id: String,
    pub grant_id: String,
    pub revision: u64,
    pub anchor: SharingIdentityDto,
    pub role_ceiling: SharingRoleDto,
    pub mode: EnrollmentModeDto,
    pub status: EnrollmentGrantStatusDto,
    /// Integral Unix seconds, matching the signed enrollment contract.
    pub not_before: i64,
    pub expires_at: i64,
    pub max_admissions: u32,
    pub admitted_count: u32,
    /// Local durable freeze; automatic acceptance may never override it.
    pub frozen: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnrollmentRequestStatusDto {
    Pending,
    Challenged,
    Responded,
    Accepted,
    Denied,
    Expired,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentRequestDto {
    pub share_id: String,
    pub grant_id: String,
    pub request_id: String,
    pub target: SharingIdentityDto,
    pub requested_role: SharingRoleDto,
    /// Full request hash, independently compared with the trusted anchor.
    pub comparison_code: String,
    pub status: EnrollmentRequestStatusDto,
    pub expires_at: i64,
    pub challenge_generation: Option<u64>,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentPairingDto {
    pub share_id: String,
    pub grant_id: String,
    pub request_id: Option<String>,
    /// Strict bounded public signed transcript; safe to exchange via file or
    /// clipboard. Import does not by itself establish trust in these keys.
    pub public_bundle_json: String,
    /// None for an anchor-source bundle; populated for a signed target request.
    pub comparison_code: Option<String>,
    pub expires_at: i64,
    pub target: Option<SharingIdentityDto>,
    pub requested_role: Option<SharingRoleDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentGrantCreateDto {
    pub anchor: SharingIdentityDto,
    pub confirmed_identity_code: String,
    pub role_ceiling: SharingRoleDto,
    pub mode: EnrollmentModeDto,
    pub expires_at: i64,
    pub max_admissions: u32,
}
