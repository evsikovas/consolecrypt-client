//! Opening the teams of an unlocked synced profile (TZ teams ТР-22) and
//! closing them again.
//!
//! For every `TeamMembership` in the personal vault: fetch the membership
//! log, the team detail and this member's key material (or, offline, the
//! copy cached in the sealed profile database), verify the log from
//! genesis, check it still extends the head verified before, open the VRK
//! from the user envelope (older epochs through the chain links), load the
//! team's objects into the working set, attest this device with a trust
//! marker and start the team's sync engine.

use super::{TeamHandle, TeamState, TeamWarning};
use crate::codec::{VaultCodec, MAX_OLDER_EPOCHS};
use crate::dto::AppEvent;
use crate::error::{AppError, AppResult};
use crate::session::{AppCtx, Session, Unlocked};
use crate::writer::VaultWriter;
use base64::Engine as _;
use cc_crypto_core::teams::{TeamIdentity, TeamLogView};
use cc_models::team::TeamMembership;
use cc_models::{ObjectId, ObjectKind, VaultObject};
use cc_protocol::teams::{
    SignedTeamLogEntry, TeamDetail, TeamKeyMaterial, TeamLogOp, TeamMemberKeys,
};
use cc_protocol::{UserId, VaultId};
use cc_storage_core::ProfileKind;
use cc_sync_core::{ApiError, ObjectStore, SyncEngine, SyncEvent};
use cc_vault_core::UnlockedVault;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

/// What opening a team needs from the session.
pub(crate) struct TeamOpenContext<'a> {
    pub session: &'a Arc<Session>,
    pub unlocked: &'a Arc<Unlocked>,
    pub user_id: UserId,
}

/// Local setting holding the cached log + key material of a team.
fn cache_key(vault_id: VaultId) -> String {
    format!("app_core.teams.cache.{vault_id}")
}

/// Offline copy of what the server showed last (in the sealed database).
#[derive(Serialize, Deserialize)]
struct TeamCache {
    log: Vec<SignedTeamLogEntry>,
    key: TeamKeyMaterial,
    detail: TeamDetail,
}

pub(crate) fn decode_identity(m: &TeamMembership) -> AppResult<TeamIdentity> {
    let bytes = zeroize::Zeroizing::new(
        base64::engine::general_purpose::STANDARD
            .decode(m.identity_secret.expose_secret())
            .map_err(|_| AppError::Crypto("team identity is malformed".into()))?,
    );
    TeamIdentity::from_secret_bytes(&bytes).map_err(|e| AppError::Crypto(e.to_string()))
}

pub(crate) fn encode_identity(identity: &TeamIdentity) -> cc_models::secret::SecretValue {
    let bytes = identity.to_secret_bytes();
    cc_models::secret::SecretValue::new(base64::engine::general_purpose::STANDARD.encode(&*bytes))
}

/// Verify the longest valid prefix of `log`. Returns the view and whether
/// every entry verified.
pub(crate) fn verify_prefix(
    vault_id: VaultId,
    log: &[SignedTeamLogEntry],
) -> AppResult<(TeamLogView, bool)> {
    let first = log
        .first()
        .ok_or_else(|| AppError::TeamUntrusted("invalid_log".into()))?;
    let mut view = TeamLogView::verify(vault_id, std::slice::from_ref(first))
        .map_err(|_| AppError::TeamUntrusted("invalid_log".into()))?;
    for e in &log[1..] {
        if view.apply(e).is_err() {
            return Ok((view, false));
        }
    }
    Ok((view, true))
}

/// Before the first verified head of a join, the latest entry adding `mine`
/// must be signed by the admin key the member code covered (SEC-02). Once a
/// head is recorded, the rollback check anchors later logs instead. Owners
/// and joins from before this check have no compared key to hold to.
fn joined_by_compared_admin(
    m: &TeamMembership,
    log: &[SignedTeamLogEntry],
    mine: &TeamMemberKeys,
) -> bool {
    let first = m.verified_log_seq == 0 && m.verified_log_head.is_empty();
    let Some(admin) = m.join_admin_key.as_deref().filter(|_| first) else {
        return true;
    };
    let Some(admin) = unhex32(admin) else {
        return false;
    };
    log.iter()
        .rev()
        .find(|e| matches!(&e.entry.op, TeamLogOp::AddMember { member, .. } if member == mine))
        .is_some_and(|e| e.entry.actor_signing_key.as_slice() == admin.as_slice())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Whether the server's answer means "you are not a member (any more)".
fn not_member(e: &ApiError) -> bool {
    matches!(e.status(), Some(403 | 404))
}

/// Open (or re-open) every team of the profile. Best effort per team;
/// teams whose membership object disappeared are closed.
pub(crate) async fn open_teams(cx: &TeamOpenContext<'_>) -> AppResult<()> {
    let writer = &cx.unlocked.writer;
    let _gate = writer.teams.gate.lock().await;
    let memberships: Vec<TeamMembership> = writer
        .working
        .objects()
        .into_iter()
        .filter_map(|o| match o {
            VaultObject::TeamMembership(m) => Some(m),
            _ => None,
        })
        .collect();
    for open in writer.teams.all() {
        if !memberships.iter().any(|m| m.team_vault_id == open.vault_id) {
            close_locked(cx.session, cx.unlocked, open.vault_id, false).await;
        }
    }
    for m in memberships {
        if let Err(e) = open_one(cx, &m).await {
            tracing::warn!(team = %m.team_vault_id, error = %e, "team not opened");
        }
    }
    cx.session.ctx.emit(AppEvent::TeamsChanged);
    Ok(())
}

/// Re-read one team from the server (membership changed, key rotated).
pub(crate) async fn refresh_team(cx: &TeamOpenContext<'_>, vault_id: VaultId) -> AppResult<()> {
    let _gate = cx.unlocked.writer.teams.gate.lock().await;
    let m = membership_of(&cx.unlocked.writer, vault_id)
        .ok_or_else(|| AppError::not_found("team", vault_id))?;
    let r = open_one(cx, &m).await;
    cx.session.ctx.emit(AppEvent::TeamsChanged);
    r
}

pub(crate) fn membership_of(writer: &VaultWriter, vault_id: VaultId) -> Option<TeamMembership> {
    writer.working.objects().into_iter().find_map(|o| match o {
        VaultObject::TeamMembership(m) if m.team_vault_id == vault_id => Some(m),
        _ => None,
    })
}

async fn fetch(
    cx: &TeamOpenContext<'_>,
    vault_id: VaultId,
) -> AppResult<Option<(TeamCache, bool)>> {
    let storage = cx.session.storage()?;
    let api = match cx.session.kind().await? {
        ProfileKind::Synced => cx.session.api_opt(),
        ProfileKind::Local => None,
    };
    if let Some(api) = api {
        let online = async {
            let detail = api.get_team(vault_id).await?;
            let log = api.team_log(vault_id).await?;
            let key = api.team_key(vault_id).await?;
            Ok::<_, ApiError>(TeamCache { log, key, detail })
        }
        .await;
        match online {
            Ok(c) => return Ok(Some((c, true))),
            Err(e) if not_member(&e) => return Ok(None),
            Err(e) if e.is_offline() || e.is_retryable() => {
                tracing::info!(team = %vault_id, "team server unreachable; using the cache");
            }
            Err(e) => return Err(e.into()),
        }
    }
    let cached: Option<TeamCache> = storage.setting_get(cache_key(vault_id)).await?;
    match cached {
        Some(c) => Ok(Some((c, false))),
        None => Err(AppError::Offline("team not cached yet".into())),
    }
}

async fn open_one(cx: &TeamOpenContext<'_>, m: &TeamMembership) -> AppResult<()> {
    let vault_id = m.team_vault_id;
    let writer = &cx.unlocked.writer;
    let Some((cache, online)) = fetch(cx, vault_id).await? else {
        // The server refuses: close, but keep the membership (only a
        // verified log without us, or the explicit event, removes it).
        tracing::warn!(team = %vault_id, "the server refuses the team");
        close_locked(cx.session, cx.unlocked, vault_id, false).await;
        return Ok(());
    };
    let identity = decode_identity(m)?;
    let (view, complete) = verify_prefix(vault_id, &cache.log)?;
    let mut warning = (!complete).then_some(TeamWarning::InvalidLog);
    if m.verified_log_seq > 0 || !m.verified_log_head.is_empty() {
        let head = unhex32(&m.verified_log_head);
        let extends = head.is_some_and(|h| view.extends(&cache.log, m.verified_log_seq, &h));
        if !extends {
            warning = Some(TeamWarning::LogRollback);
        }
    }
    let mine = identity.member_keys(cx.user_id);
    let me = view.members.get(&cx.user_id);
    if me.map(|x| &x.keys) != Some(&mine) {
        if m.invitation_id.is_some() {
            // Join not confirmed yet: the admin still compares the code.
            return Ok(());
        }
        if warning.is_none() {
            // A complete, consistent log without us: we were removed.
            close_locked(cx.session, cx.unlocked, vault_id, true).await;
            return Ok(());
        }
        return Err(AppError::TeamUntrusted(
            warning.map(TeamWarning::code).unwrap_or_default().into(),
        ));
    }
    if !joined_by_compared_admin(m, &cache.log, &mine) {
        // The first log after joining must add us with an entry signed by
        // the admin whose code we compared; anything else is a team the
        // server made up (SEC-02).
        tracing::warn!(team = %vault_id, "the team log does not come from the compared admin");
        return Err(AppError::TeamUntrusted(
            TeamWarning::InvalidLog.code().into(),
        ));
    }
    // Already open with the same key: refresh the membership view in place.
    // Rebuilding the store and engine here would race the running sync.
    if let Some(open) = writer.teams.get(vault_id) {
        let st = open.state();
        if online
            && warning.is_none()
            && st.warning.is_none()
            && st.key_epoch == cache.key.key_epoch
        {
            open.update(|s| {
                s.view = view.clone();
                s.log = cache.log.clone();
                s.listed = cache.detail.members.clone();
                s.rotation_required = cache.detail.team.rotation_required;
                s.offline = false;
            });
            cx.session
                .storage()?
                .setting_set(cache_key(vault_id), cache)
                .await?;
            record_verified_head(writer, m, &view, None).await?;
            if open.engine().is_none() {
                start_team_engine(cx, &open).await?;
            }
            return Ok(());
        }
    }
    let current = UnlockedVault::open_team(&identity, &cache.key.envelope, vault_id, cx.user_id)
        .map_err(AppError::from)?;
    if let Err(why) = check_pinned_key(m, &current, &cache.key, complete.then_some(view.key_epoch))
    {
        // A key the server picked for an epoch we already know, or a newer
        // one that does not chain back to it: refuse (SEC-03).
        tracing::warn!(team = %vault_id, why, "the team key does not match the pinned one");
        return Err(AppError::TeamUntrusted("key_mismatch".into()));
    }
    let pin = (cache.key.key_epoch, current.key_commitment());
    let mut older = Vec::new();
    let mut epoch = cache.key.key_epoch;
    let mut newest = None::<UnlockedVault>;
    while epoch > 0 && older.len() < MAX_OLDER_EPOCHS {
        let Some(link) = cache.key.chain.iter().find(|l| l.key_epoch == epoch) else {
            break;
        };
        let opener = newest.as_ref().unwrap_or(&current);
        match opener.open_previous_epoch(link) {
            Ok(prev) => {
                if let Some(n) = newest.take() {
                    older.push(n);
                }
                newest = Some(prev);
            }
            Err(e) => {
                tracing::warn!(team = %vault_id, epoch, error = %e, "key chain link rejected");
                break;
            }
        }
        epoch -= 1;
    }
    older.extend(newest);
    let attest = (online && !cache.detail.team.caller_trusted)
        .then(|| current.attest_device_request(&cx.session.identity()))
        .transpose()
        .map_err(AppError::from)?;
    let codec = VaultCodec::with_epochs(current, older);
    let store = ObjectStore::new(vault_id, cx.session.storage()?, codec.clone());
    let rx = store.subscribe();
    let handle = TeamHandle::new(
        vault_id,
        cx.user_id,
        codec,
        store.clone(),
        identity,
        TeamState {
            view: view.clone(),
            log: cache.log.clone(),
            listed: cache.detail.members.clone(),
            key_epoch: cache.key.key_epoch,
            rotation_required: cache.detail.team.rotation_required,
            settings: None,
            warning,
            offline: !online,
        },
    );
    let (objects, failed) = store.list().await?;
    for (id, e) in &failed {
        tracing::warn!(team = %vault_id, object_id = %id, error = %e, "team object failed to decrypt");
    }
    handle.set_settings(&writer.working, newest_settings(&objects));
    if let Some(old) = writer.teams.insert(handle.clone()) {
        old.shutdown().await;
    }
    writer.working.replace_vault(Some(vault_id), objects);
    handle.push_task(tokio::spawn(team_object_event_task(
        writer.clone(),
        vault_id,
        rx,
        cx.session.ctx.clone(),
    )));
    if online {
        cx.session
            .storage()?
            .setting_set(cache_key(vault_id), cache)
            .await?;
        if let (Some(req), Some(api)) = (attest, cx.session.api_opt()) {
            if let Err(e) = api.attest_device(cx.session.device_id(), &req).await {
                tracing::warn!(team = %vault_id, error = %e, "team attestation failed");
            }
        }
    }
    if warning.is_none() {
        record_verified_head(writer, m, &view, Some(pin)).await?;
        if online {
            start_team_engine(cx, &handle).await?;
        }
    } else {
        cx.session.ctx.emit(AppEvent::TeamWarning {
            vault_id: vault_id.to_string(),
            warning: warning.map(TeamWarning::code).unwrap_or_default().into(),
        });
    }
    Ok(())
}

/// Remember the verified log head (rollback detection) and finish a
/// pending join in the personal-vault membership object.
/// Record the verified log head and, after a full open, the accepted team
/// key (`pin`: epoch and commitment).
async fn record_verified_head(
    writer: &VaultWriter,
    m: &TeamMembership,
    view: &TeamLogView,
    pin: Option<(u32, [u8; 32])>,
) -> AppResult<()> {
    let head = hex(&view.head);
    let pin = pin.map(|(epoch, commitment)| (epoch, hex(&commitment)));
    let pin_changed = pin.as_ref().is_some_and(|(epoch, commitment)| {
        m.pinned_key_epoch != Some(*epoch)
            || m.pinned_key_commitment.as_deref() != Some(commitment.as_str())
    });
    if view.seq != m.verified_log_seq
        || head != m.verified_log_head
        || m.invitation_id.is_some()
        || pin_changed
    {
        let mut updated = m.clone();
        updated.invitation_id = None;
        updated.verified_log_seq = view.seq;
        updated.verified_log_head = head;
        if let Some((epoch, commitment)) = pin {
            updated.pinned_key_epoch = Some(epoch);
            updated.pinned_key_commitment = Some(commitment);
        }
        updated.updated_at = chrono::Utc::now();
        writer.put(VaultObject::TeamMembership(updated)).await?;
    }
    Ok(())
}

/// Longest rotation chain walked back to a pinned key.
const MAX_PIN_CHAIN: u32 = 4096;

/// SEC-03: the key the server hands out must be the pinned key of its epoch,
/// or a newer one whose chain links lead back to the pinned key; and, with a
/// complete log, its epoch must be the one the verified log announces. No
/// pin yet (first open, or a membership from before this check) → accepted
/// and pinned.
fn check_pinned_key(
    m: &TeamMembership,
    current: &UnlockedVault,
    key: &TeamKeyMaterial,
    log_epoch: Option<u32>,
) -> Result<(), &'static str> {
    if log_epoch.is_some_and(|e| e != key.key_epoch) {
        return Err("the key epoch differs from the verified log");
    }
    let Some(pinned_epoch) = m.pinned_key_epoch else {
        return Ok(());
    };
    let pinned = m
        .pinned_key_commitment
        .as_deref()
        .and_then(unhex32)
        .ok_or("malformed pinned key")?;
    if key.key_epoch < pinned_epoch {
        return Err("an older key epoch than the pinned one");
    }
    if key.key_epoch - pinned_epoch > MAX_PIN_CHAIN {
        return Err("too many rotations since the pinned key");
    }
    let mut older: Option<UnlockedVault> = None;
    for epoch in (pinned_epoch + 1..=key.key_epoch).rev() {
        let link = key
            .chain
            .iter()
            .find(|l| l.key_epoch == epoch)
            .ok_or("a chain link back to the pinned key is missing")?;
        let opener = older.as_ref().unwrap_or(current);
        older = Some(
            opener
                .open_previous_epoch(link)
                .map_err(|_| "a chain link back to the pinned key is rejected")?,
        );
    }
    let reached = older.as_ref().unwrap_or(current).key_commitment();
    if reached == pinned {
        Ok(())
    } else {
        Err("the key does not lead back to the pinned key")
    }
}

fn newest_settings(
    objects: &[cc_sync_core::DecryptedObject],
) -> Option<cc_models::settings::VaultSettings> {
    objects
        .iter()
        .filter_map(|d| match &d.payload.object {
            VaultObject::VaultSettings(s) => Some(s.clone()),
            _ => None,
        })
        .max_by_key(|s| s.updated_at)
}

async fn start_team_engine(cx: &TeamOpenContext<'_>, team: &Arc<TeamHandle>) -> AppResult<()> {
    let Some(api) = cx.session.api_opt() else {
        return Ok(());
    };
    let engine = SyncEngine::new(team.store.clone(), api, cx.session.engine_config()).await?;
    *team.engine.write().unwrap_or_else(|p| p.into_inner()) = Some(engine.clone());
    if let Some(rx) = cx.unlocked.subscribe_ws().await {
        team.push_task(engine.attach_events(rx));
        engine.start();
    }
    Ok(())
}

/// Close a team locally. With `purge` (removed, left) its local objects,
/// cache and membership object are deleted too.
pub(crate) async fn close_team(
    session: &Arc<Session>,
    unlocked: &Arc<Unlocked>,
    vault_id: VaultId,
    purge: bool,
) {
    let _gate = unlocked.writer.teams.gate.lock().await;
    close_locked(session, unlocked, vault_id, purge).await;
}

async fn close_locked(
    session: &Arc<Session>,
    unlocked: &Arc<Unlocked>,
    vault_id: VaultId,
    purge: bool,
) {
    let writer = &unlocked.writer;
    if let Some(t) = writer.teams.remove(vault_id) {
        t.shutdown().await;
    }
    writer.working.drop_vault(vault_id);
    if purge {
        if let Ok(st) = session.storage() {
            let _ = st.write(move |tx| tx.purge_vault(vault_id)).await;
            let key = cache_key(vault_id);
            let _ = st.write(move |tx| tx.setting_delete(&key)).await;
        }
        if let Some(m) = membership_of(writer, vault_id) {
            if let Err(e) = writer.delete(m.id).await {
                tracing::warn!(team = %vault_id, error = %e, "membership not removed");
            }
        }
        session.ctx.emit(AppEvent::TeamRemoved {
            vault_id: vault_id.to_string(),
        });
    }
    session.ctx.emit(AppEvent::ObjectsChanged {
        kind: ObjectKind::Host,
        ids: Vec::new(),
        origin: crate::dto::ChangeOriginDto::Remote,
    });
}

/// Keeps the working set consistent with one team store's events.
async fn team_object_event_task(
    writer: VaultWriter,
    vault_id: VaultId,
    mut rx: broadcast::Receiver<SyncEvent>,
    ctx: Arc<AppCtx>,
) {
    use crate::dto::ChangeOriginDto;
    let emit = |kind: ObjectKind, id: ObjectId, origin: ChangeOriginDto| {
        ctx.emit(AppEvent::ObjectsChanged {
            kind,
            ids: vec![id.to_string()],
            origin,
        })
    };
    loop {
        match rx.recv().await {
            Ok(SyncEvent::ObjectChanged {
                object_id, origin, ..
            }) => {
                let origin = match origin {
                    cc_sync_core::ChangeOrigin::Local => ChangeOriginDto::Local,
                    cc_sync_core::ChangeOrigin::Remote => ChangeOriginDto::Remote,
                    cc_sync_core::ChangeOrigin::ConflictResolution => {
                        ChangeOriginDto::ConflictResolution
                    }
                };
                match writer.refresh_in(Some(vault_id), object_id).await {
                    Ok(Some(kind)) => emit(kind, object_id, origin),
                    Ok(None) => {}
                    Err(AppError::VaultLocked | AppError::NotFound { .. }) => break,
                    Err(e) => {
                        tracing::warn!(object_id = %object_id, error = %e, "team object refresh failed")
                    }
                }
            }
            Ok(SyncEvent::ObjectRemoved { object_id, .. }) => {
                if let Some(kind) = writer.working.apply_in(
                    Some(vault_id),
                    object_id,
                    crate::working_set::Readback::Missing,
                ) {
                    emit(kind, object_id, ChangeOriginDto::Remote);
                }
            }
            Ok(SyncEvent::ConflictResolved {
                object_id,
                conflict_copy,
                ..
            }) => {
                for id in std::iter::once(object_id).chain(conflict_copy) {
                    if let Ok(Some(kind)) = writer.refresh_in(Some(vault_id), id).await {
                        emit(kind, id, ChangeOriginDto::ConflictResolution);
                    }
                }
            }
            Ok(SyncEvent::SnapshotCompleted { .. })
            | Err(broadcast::error::RecvError::Lagged(_)) => {
                let Some(team) = writer.teams.get(vault_id) else {
                    break;
                };
                match team.store.list().await {
                    Ok((objects, _)) => {
                        team.set_settings(&writer.working, newest_settings(&objects));
                        writer.working.replace_vault(Some(vault_id), objects);
                        ctx.emit(AppEvent::TeamsChanged);
                    }
                    Err(e) => tracing::warn!(team = %vault_id, error = %e, "team reload failed"),
                }
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// One sync cycle for every open team (read-after-sync for `sync_now`).
pub(crate) async fn sync_teams(writer: &VaultWriter) {
    for team in writer.teams.all() {
        let Some(engine) = team.engine() else {
            continue;
        };
        match engine.sync_now().await {
            Ok(r) if r.pulled > 0 || r.resolved > 0 || r.snapshot_pages > 0 => {
                if let Ok((objects, _)) = team.store.list().await {
                    team.set_settings(&writer.working, newest_settings(&objects));
                    writer.working.replace_vault(Some(team.vault_id), objects);
                }
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(team = %team.vault_id, error = %e, "team sync failed"),
        }
    }
}
