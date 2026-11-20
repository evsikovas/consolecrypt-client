use super::*;
use cc_automation_core::{
    VaultValidationBinding, VaultValidationMarker, VaultValidationPasswordLease,
    VaultValidationPasswordRequest, VaultValidationPhase, VaultValidationPurpose,
    VaultValidationRuntime, VerifiedValidation,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn phase_binding(root:&ProjectRoot,revision:&ProjectRevision,selected:Uuid,scope:Uuid,
    phase:VaultValidationPhase,nonce:Uuid,playbook:&str,runtime:&VaultValidationRuntime,call_index:u8,
)->AppResult<VaultValidationBinding> {
    if selected.is_nil() {
        root.bind_ephemeral_vault_validation(revision,scope,phase,nonce,playbook,runtime,call_index)
    } else {
        root.bind_vault_validation(revision,selected,scope,phase,nonce,playbook,runtime,call_index)
    }.map_err(|_|invalid())
}

/// Native routing identity is not authority. The private owner retains the
/// original explicit selection/project/session and both one-use phase budgets.
pub(crate) struct NativeVaultValidation {
    review: Arc<VaultCredentialReview>,
    scope: Uuid,
    nonces: [[Uuid; 2]; 2],
    issued: [[AtomicBool; 2]; 2],
    privacy: VaultValidationBinding,
    marker: VaultValidationMarker,
}
impl std::fmt::Debug for NativeVaultValidation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeVaultValidation")
            .finish_non_exhaustive()
    }
}
pub(crate) struct NativeVaultValidationPassword {
    // Retain capture/admission/lifecycle while the private pipe owner exists.
    scope: Arc<NativeVaultValidation>,
    lease: VaultValidationPasswordLease,
}
impl std::fmt::Debug for NativeVaultValidationPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeVaultValidationPassword")
            .finish_non_exhaustive()
    }
}
impl VaultCredentialReview {
    /// Caller has selected the exact Native ref; scope/phase identities are
    /// generated HERE, never supplied as an 'approved' JSON flag by a caller.
    pub(crate) async fn begin_validation(
        self,
        app: &AppCore,
        playbook: String,
        runtime: VaultValidationRuntime,
    ) -> AppResult<Arc<NativeVaultValidation>> {
        let u = self.project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        {
            let _profile = app.hold_session(&self.project.owner).await?;
            let _unlock = self.project.owner.hold_unlocked(&u).await?;
            self.check_held(&u).await?;
        }
        drop(u);
        let scope = Uuid::new_v4();
        runtime.require_fresh().map_err(|_| invalid())?;
        let nonces: [[Uuid; 2]; 2] =
            std::array::from_fn(|_| std::array::from_fn(|_| Uuid::new_v4()));
        let syntax_nonce = nonces[0][0];
        let io = automation_io_slot(&self.project.owner)?;
        let root = self.root.clone();
        let current_path = self.project.path.clone();
        let revision = self.revision.clone();
        let selected = self.binding.credential_id();
        let privacy = tokio::task::spawn_blocking(move || {
            let _io = io;
            root.recheck().map_err(|_| invalid())?;
            let current = root
                .reopen_same_directory(&current_path)
                .map_err(|_| invalid())?;
            let binding = phase_binding(&current,
                    &revision,
                    selected,
                    scope,
                    VaultValidationPhase::Syntax,
                    syntax_nonce,
                    &playbook,
                    &runtime,
                    0,
                )?;
            // Name still points to captured directory after the full walk.
            // Defined check point; no universal lock against later OS renames.
            root.reopen_same_directory(&current_path)
                .map_err(|_| invalid())?;
            root.recheck().map_err(|_| invalid())?;
            Ok::<_, AppError>(binding)
        })
        .await
        .map_err(|_| invalid())??;
        let u = self.project.unlock.upgrade().ok_or(AppError::VaultLocked)?;
        let _profile = app.hold_session(&self.project.owner).await?;
        let _unlock = self.project.owner.hold_unlocked(&u).await?;
        self.check_held(&u).await?;
        drop(_unlock);
        drop(_profile);
        // Mark privacy BEFORE material lookup: diagnostics may contain decrypted
        // YAML values even if a later password request fails/is never made.
        Ok(Arc::new(NativeVaultValidation {
            review: Arc::new(self),
            scope,
            nonces,
            issued: std::array::from_fn(|_| std::array::from_fn(|_| AtomicBool::new(false))),
            marker: privacy.diagnostics_marker(),
            privacy,
        }))
    }
}
impl NativeVaultValidation {
    fn phase_index(phase: VaultValidationPhase) -> usize {
        match phase {
            VaultValidationPhase::Syntax => 0,
            VaultValidationPhase::Lint => 1,
        }
    }
    pub(crate) fn request(&self, phase: VaultValidationPhase) -> VaultValidationPasswordRequest {
        // Compatibility helper for the first child only, never a phase replay.
        self.request_call(phase, 0)
            .expect("fixed first child index")
    }
    pub(crate) fn request_call(
        &self,
        phase: VaultValidationPhase,
        call_index: u8,
    ) -> AppResult<VaultValidationPasswordRequest> {
        if call_index >= 2 {
            return Err(invalid());
        }
        let mut request = self.privacy.request();
        request.phase = phase;
        request.call_index = call_index;
        request.phase_nonce = self.nonces[Self::phase_index(phase)][usize::from(call_index)];
        Ok(request)
    }
    /// Original live context and exact request, not nonce possession, issue a
    /// distinct fresh lease per loader call (two per phase, four total). No broad ID lookup or shared plaintext.
    pub(crate) async fn phase(
        self: &Arc<Self>,
        app: &AppCore,
        request: &VaultValidationPasswordRequest,
        observed_runtime: VaultValidationRuntime,
    ) -> AppResult<NativeVaultValidationPassword> {
        // The dedicated Native owner must freshly probe its SAME peer for
        // every call; this non-Clone short-lived token is consumed by its FS
        // worker. Helper-reported compatibility is not credential authority.
        observed_runtime.require_fresh().map_err(|_| invalid())?;
        let expected = self.request_call(request.phase, request.call_index)?;
        if request.purpose != expected.purpose
            || request.environment_digest != expected.environment_digest
            || request.helper_digest != expected.helper_digest
            || observed_runtime.environment_digest() != expected.environment_digest
            || observed_runtime.helper_digest() != expected.helper_digest
            || request.version != 1
            || request.scope_id != expected.scope_id
            || request.phase_nonce != expected.phase_nonce
            || request.revision_digest != expected.revision_digest
            || request.reference_sha256 != expected.reference_sha256
        {
            return Err(invalid());
        }
        let issued =
            &self.issued[Self::phase_index(request.phase)][usize::from(request.call_index)];
        if issued
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(invalid());
        }
        let u = self
            .review
            .project
            .unlock
            .upgrade()
            .ok_or(AppError::VaultLocked)?;
        {
            let _profile = app.hold_session(&self.review.project.owner).await?;
            let _unlock = self.review.project.owner.hold_unlocked(&u).await?;
            self.review.check_held(&u).await?;
        }
        drop(u); // No strong unlock/keys retained across blocking filesystem IO.
        let io = automation_io_slot(&self.review.project.owner)?;
        let root = self.review.root.clone();
        let current_path = self.review.project.path.clone();
        let revision = self.review.revision.clone();
        let selected = self.review.binding.credential_id();
        let playbook = self.privacy.playbook().to_owned();
        let scope = self.scope;
        let phase = request.phase;
        let nonce = request.phase_nonce;
        let call_index = request.call_index;
        let binding = tokio::task::spawn_blocking(move || {
            let _io = io;
            root.recheck().map_err(|_| invalid())?;
            let current = root
                .reopen_same_directory(&current_path)
                .map_err(|_| invalid())?;
            let binding = phase_binding(&current,
                    &revision,
                    selected,
                    scope,
                    phase,
                    nonce,
                    &playbook,
                    &observed_runtime,
                    call_index,
                )?;
            // Name still points to captured directory after the full walk.
            // Defined check point; no universal lock against later OS renames.
            root.reopen_same_directory(&current_path)
                .map_err(|_| invalid())?;
            root.recheck().map_err(|_| invalid())?;
            Ok::<_, AppError>(binding)
        })
        .await
        .map_err(|_| invalid())??;
        let u = self
            .review
            .project
            .unlock
            .upgrade()
            .ok_or(AppError::VaultLocked)?;
        {
            let _profile = app.hold_session(&self.review.project.owner).await?;
            let _unlock = self.review.project.owner.hold_unlocked(&u).await?;
            self.review.check_held(&u).await?;
            if !u
                .working()
                .watch_revisions(self.review.fences.clone(), self.review.revoked.clone())
            {
                return Err(invalid());
            }
        }
        binding.require_live().map_err(|_| invalid())?;
        let secret = tokio::select! {
            _=self.review.revoked.cancelled()=>return Err(invalid()),
            secret=self.review.password_value(&u)=>secret?,
        };
        let _profile = app.hold_session(&self.review.project.owner).await?;
        let _unlock = self.review.project.owner.hold_unlocked(&u).await?;
        self.review.check_held(&u).await?;
        let lease = VaultValidationPasswordLease::issue(
            &binding,
            secret,
            self.review.revoked.child_token(),
        )
        .map_err(|_| invalid())?;
        Ok(NativeVaultValidationPassword {
            scope: self.clone(),
            lease,
        })
    }
    /// The only automatic result/AI boundary: consume every potentially Vault
    /// backed proof through this trusted marker, never a guest used_vault flag.
    pub(crate) fn seal_result(
        &self,
        validation: VerifiedValidation,
    ) -> AppResult<VerifiedValidation> {
        validation
            .with_vault_context(&self.marker)
            .map_err(|_| invalid())
    }
    pub(crate) fn revoke(&self) {
        self.review.revoked.cancel();
    }
}

#[cfg(test)]
mod tests;

pub(super) mod owner;

pub(super) mod job;
