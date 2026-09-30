//! Narrow sharing operations on an installation identity. Private device keys
//! stay inside vault-core; callers receive only ciphertext or signed documents.

use crate::DeviceIdentity;
use cc_crypto_core::sharing::{
    self, SharedPlaintext, SharingCryptoError, SharingManifestCheckpoint, SharingOwnerAnchor,
    SharingRevisionCheckpoint, SharingRevisionOpener, VerifiedSharingManifest,
    VerifiedSharingMutation,
};
use cc_crypto_core::sharing_enrollment::{
    self as enrollment, EnrollmentChallengeCheckpoint, EnrollmentCryptoError, GrantTransition,
    OwnerChallengeSecret, VerifiedAnchorEndorsement, VerifiedDevicePossession,
    VerifiedEnrollmentGrant, VerifiedEnrollmentRequest,
};
use cc_protocol::sharing::{
    AccessManifest, SharedEncryptedBody, SharingContext, SharingMutation, SignedAccessManifest,
    SignedSharingMutation,
};
use cc_protocol::sharing_enrollment::{
    AcceptOwnDeviceRequest, SharingOwnDeviceRequest, SharingOwnDevicesGrantState,
    SignedSharingAnchorEndorsement, SignedSharingDeviceChallenge,
    SignedSharingDeviceChallengeResponse, SignedSharingOwnDeviceRequest,
    SignedSharingOwnDevicesGrantState,
};
use cc_protocol::DeviceId;

type Result<T> = std::result::Result<T, SharingCryptoError>;

impl DeviceIdentity {
    pub fn sharing_sign_manifest(
        &self,
        manifest: AccessManifest,
        context: &SharingContext,
        owner: &SharingOwnerAnchor,
        previous: Option<&SharingManifestCheckpoint>,
    ) -> Result<SignedAccessManifest> {
        if owner.device_id != self.device_id() {
            return Err(SharingCryptoError::DeviceKeyMismatch);
        }
        sharing::sign_shared_manifest(self.secret_keys(), manifest, context, owner, previous)
    }

    pub fn sharing_seal_revision(
        &self,
        manifest: &VerifiedSharingManifest,
        context: &SharingContext,
        plaintext: &[u8],
    ) -> Result<SharedEncryptedBody> {
        sharing::seal_shared_revision(manifest, context, plaintext)
    }

    pub fn sharing_sign_mutation(
        &self,
        manifest: &VerifiedSharingManifest,
        mutation: SharingMutation,
        body: Option<&SharedEncryptedBody>,
        previous: Option<&SharingRevisionCheckpoint>,
    ) -> Result<SignedSharingMutation> {
        if mutation.writer_device_id != self.device_id() {
            return Err(SharingCryptoError::DeviceKeyMismatch);
        }
        sharing::sign_shared_mutation(manifest, self.secret_keys(), mutation, body, previous)
    }
}

impl SharingRevisionOpener for DeviceIdentity {
    fn open_shared_revision(
        &self,
        manifest: &VerifiedSharingManifest,
        revision: &VerifiedSharingMutation,
        body: Option<&SharedEncryptedBody>,
        recipient_device_id: DeviceId,
    ) -> Result<Option<SharedPlaintext>> {
        if recipient_device_id != self.device_id() {
            return Err(SharingCryptoError::DeviceKeyMismatch);
        }
        sharing::open_shared_revision(
            manifest,
            revision,
            body,
            recipient_device_id,
            self.secret_keys(),
        )
    }
}

impl DeviceIdentity {
    pub fn sharing_sign_enrollment_grant(
        &self,
        grant: SharingOwnDevicesGrantState,
        owner: &SharingOwnerAnchor,
        previous: Option<&VerifiedEnrollmentGrant>,
        transition: GrantTransition<'_>,
        now: i64,
    ) -> std::result::Result<SignedSharingOwnDevicesGrantState, EnrollmentCryptoError> {
        if owner.device_id != self.device_id() {
            return Err(EnrollmentCryptoError::Context);
        }
        enrollment::sign_grant_state(self.secret_keys(), grant, owner, previous, transition, now)
    }
    pub fn sharing_sign_enrollment_request(
        &self,
        request: SharingOwnDeviceRequest,
        grant: &VerifiedEnrollmentGrant,
        current: &VerifiedSharingManifest,
        now: i64,
    ) -> std::result::Result<SignedSharingOwnDeviceRequest, EnrollmentCryptoError> {
        if request.target.device_id != self.device_id() {
            return Err(EnrollmentCryptoError::Context);
        }
        enrollment::sign_device_request(self.secret_keys(), request, grant, current, now)
    }
    pub fn sharing_endorse_enrollment_request(
        &self,
        request: &VerifiedEnrollmentRequest,
        current: &VerifiedSharingManifest,
        confirmed_code: &str,
        now: i64,
    ) -> std::result::Result<SignedSharingAnchorEndorsement, EnrollmentCryptoError> {
        if request.grant().signed().grant.anchor.device_id != self.device_id() {
            return Err(EnrollmentCryptoError::Context);
        }
        enrollment::endorse_device_request(
            self.secret_keys(),
            request,
            current,
            confirmed_code,
            now,
        )
    }
    pub fn sharing_create_enrollment_challenge(
        &self,
        owner: &SharingOwnerAnchor,
        request: &VerifiedEnrollmentRequest,
        endorsement: &VerifiedAnchorEndorsement,
        current: &VerifiedSharingManifest,
        now: i64,
        previous: Option<&EnrollmentChallengeCheckpoint>,
    ) -> std::result::Result<
        (SignedSharingDeviceChallenge, OwnerChallengeSecret),
        EnrollmentCryptoError,
    > {
        if owner.device_id != self.device_id() {
            return Err(EnrollmentCryptoError::Context);
        }
        enrollment::create_device_challenge(
            self.secret_keys(),
            owner,
            request,
            endorsement,
            current,
            now,
            previous,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn sharing_answer_enrollment_challenge(
        &self,
        owner: &SharingOwnerAnchor,
        request: &VerifiedEnrollmentRequest,
        endorsement: &VerifiedAnchorEndorsement,
        current: &VerifiedSharingManifest,
        challenge: &SignedSharingDeviceChallenge,
        now: i64,
        previous: Option<&EnrollmentChallengeCheckpoint>,
    ) -> std::result::Result<SignedSharingDeviceChallengeResponse, EnrollmentCryptoError> {
        if request.signed().request.target.device_id != self.device_id() {
            return Err(EnrollmentCryptoError::Context);
        }
        enrollment::answer_device_challenge(
            self.secret_keys(),
            owner,
            request,
            endorsement,
            current,
            challenge,
            now,
            previous,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn sharing_accept_own_device(
        &self,
        owner: &SharingOwnerAnchor,
        before: &VerifiedSharingManifest,
        previous: &VerifiedSharingMutation,
        body: &SharedEncryptedBody,
        request: &VerifiedEnrollmentRequest,
        endorsement: &VerifiedAnchorEndorsement,
        possession: &VerifiedDevicePossession,
        latest: &EnrollmentChallengeCheckpoint,
        latest_grant: &VerifiedEnrollmentGrant,
        other_grants: &[VerifiedEnrollmentGrant],
        now: i64,
    ) -> std::result::Result<AcceptOwnDeviceRequest, EnrollmentCryptoError> {
        if owner.device_id != self.device_id() {
            return Err(EnrollmentCryptoError::Context);
        }
        enrollment::accept_own_device(
            self.secret_keys(),
            owner,
            before,
            previous,
            body,
            request,
            endorsement,
            possession,
            latest,
            latest_grant,
            other_grants,
            now,
        )
    }
}
