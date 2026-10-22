//! Notes travelling with a share (`share_notes_v1`), a child of the sharing
//! facade so it reuses its verified state. The owner device issues the notes
//! key of every access epoch and moves older notes into the current one; any
//! member reads every note and writes its own. Plaintext stays in this
//! process: the server only sees owner-signed key sets and signed ciphertext.
use super::*;
use cc_crypto_core::sharing_notes::{open_shared_note, rekey_shared_note, NotesKey};
use cc_protocol::sharing_notes::{
    RekeySharedNotesRequest, RekeyedSharedNote, SharedNote, SharedNotesPage, MAX_REKEY_BATCH,
};
use std::collections::HashMap;

/// Local object → share links for notes (profile settings, never synced).
const LINKS_KEY: &str = "cc.sharing.notes-links.v1";
const MAX_LINKS: usize = 4096;
/// The JSON `scope` narrows a note to a part of the shared object (a
/// container on a shared Docker host, a resource of a shared context).
const MAX_SCOPE_BYTES: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareNoteLinkDto {
    /// `host:<id>`, `rdp_host:<id>`, `db_connection:<id>` or `kube_context:<id>`.
    pub object: String,
    pub share_id: String,
}

/// What a note says; never carries its share or author (signed separately).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    scope: Option<serde_json::Value>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedNoteDto {
    pub id: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub scope: Option<serde_json::Value>,
    pub author_user_id: String,
    pub mine: bool,
    pub can_delete: bool,
    pub revision: u64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedNotesDto {
    pub share_id: String,
    /// The server offers shared notes.
    pub enabled: bool,
    pub owner: bool,
    pub owner_user_id: String,
    /// The owner has not issued the key of the current epoch yet.
    pub waiting_for_owner: bool,
    pub notes: Vec<SharedNoteDto>,
    /// Notes this device cannot open yet (earlier epochs not moved by the owner).
    pub pending: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedNoteInputDto {
    /// Empty creates a note.
    #[serde(default)]
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub scope: Option<serde_json::Value>,
}

fn valid_link_object(object: &str) -> bool {
    let Some((kind, id)) = object.split_once(':') else {
        return false;
    };
    matches!(kind, "host" | "rdp_host" | "db_connection" | "kube_context")
        && ObjectId::from_str(id).is_ok()
}

fn validate_input(input: &SharedNoteInputDto) -> AppResult<()> {
    let title = input.title.trim();
    if title.is_empty()
        || title.chars().count() > cc_models::note::MAX_TITLE_CHARS
        || title.chars().any(char::is_control)
        || input.body.len() > cc_models::note::MAX_BODY_BYTES
        || input.tags.len() > cc_models::note::MAX_TAGS
        || input
            .tags
            .iter()
            .any(|t| t.trim().is_empty() || t.chars().count() > cc_models::note::MAX_TAG_CHARS)
        || input
            .scope
            .as_ref()
            .is_some_and(|scope| !scope.is_object() || scope.to_string().len() > MAX_SCOPE_BYTES)
    {
        return Err(invalid("invalid shared note"));
    }
    Ok(())
}

/// Everything needed to read or write the notes of one verified share.
struct NotesContext {
    env: Environment,
    snapshot: SharingSnapshot,
    manifest: VerifiedSharingManifest,
    share: ShareId,
    instance: Uuid,
}

impl NotesContext {
    fn owner(&self) -> bool {
        let m = self.manifest.manifest();
        m.owner_device_id == self.env.pin.device && m.owner_user_id == self.env.pin.user
    }

    fn api(&self) -> AppResult<&SharingApi> {
        self.env.transport()
    }

    fn enabled(&self) -> bool {
        self.env
            .transport
            .as_ref()
            .is_some_and(|api| api.capabilities().supports_notes_v1)
    }
}

impl AppCore {
    pub async fn share_note_links(&self) -> AppResult<Vec<ShareNoteLinkDto>> {
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        let stored: Option<String> = wait_generation(
            &unlocked,
            session
                .storage()?
                .read(|tx| tx.setting_get::<String>(LINKS_KEY)),
        )
        .await?;
        Ok(stored
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    }

    /// Links a local object to a share (after publishing or accepting it);
    /// `share_id` empty removes the link.
    pub async fn share_note_link(&self, object: String, share_id: String) -> AppResult<()> {
        if !valid_link_object(&object)
            || (!share_id.is_empty() && ShareId::from_str(&share_id).is_err())
        {
            return Err(invalid("invalid shared note link"));
        }
        let session = self.session().await?;
        let unlocked = session.unlocked().await?;
        wait_generation(
            &unlocked,
            session
                .storage()?
                .write(move |tx| -> Result<(), cc_storage_core::StorageError> {
                    let stored: Option<String> = tx.setting_get(LINKS_KEY)?;
                    let mut links: Vec<ShareNoteLinkDto> = stored
                        .and_then(|json| serde_json::from_str(&json).ok())
                        .unwrap_or_default();
                    links.retain(|link| link.object != object);
                    if !share_id.is_empty() && links.len() < MAX_LINKS {
                        links.push(ShareNoteLinkDto { object, share_id });
                    }
                    let json = serde_json::to_string(&links).unwrap_or_else(|_| "[]".into());
                    tx.setting_set(LINKS_KEY, &json)
                }),
        )
        .await
    }

    async fn share_notes_context(&self, share_id: &str) -> AppResult<NotesContext> {
        let session = self.session().await?;
        let env = self.sharing_environment(session.clone(), true).await?;
        let share = ShareId::from_str(share_id).map_err(|_| invalid("invalid shared item id"))?;
        let binding = env.binding(share)?;
        let store = env.store()?;
        let current = env.wait(env.transport()?.get_share(share)).await?;
        self.sharing_check(&env).await?;
        let snapshot = self
            .sharing_refresh_state(&env, &store, &binding, current)
            .await?;
        let manifest = trusted_manifest(&snapshot)?;
        Ok(NotesContext {
            instance: binding.server_instance_id,
            env,
            snapshot,
            manifest,
            share,
        })
    }

    /// Signing keys of current and former member devices; the history
    /// manifests are accepted only with the pinned owner's signature.
    async fn share_note_authors(
        &self,
        cx: &NotesContext,
    ) -> AppResult<HashMap<DeviceId, [u8; 32]>> {
        let mut keys: HashMap<DeviceId, [u8; 32]> = HashMap::new();
        let mut add = |members: &[SharingMember]| {
            for member in members {
                if let Some(key) = member.signing_public_key.to_array::<32>() {
                    keys.insert(member.device_id, key);
                }
            }
        };
        add(&cx.manifest.manifest().members);
        let anchor = cx.snapshot.owner_anchor();
        let mut after = 0;
        for _ in 0..MAX_HISTORY_PAGES {
            let page = cx
                .env
                .wait(cx.api()?.share_history(cx.share, after, 0, 100))
                .await?;
            for signed in &page.manifests {
                let m = &signed.manifest;
                let context = SharingContext {
                    server_instance_id: m.server_instance_id,
                    share_id: m.share_id,
                    item_id: m.item_id,
                    revision: 1,
                    access_epoch: m.access_epoch,
                    kind: m.kind,
                };
                if m.share_id == cx.share
                    && verify_shared_manifest(signed, &context, &anchor, None).is_ok()
                {
                    add(&m.members);
                }
                after = after.max(m.revision);
            }
            if !page.has_more || page.manifests.is_empty() {
                break;
            }
        }
        Ok(keys)
    }

    /// Opens every key set this device has an envelope in, by epoch.
    fn share_note_keys(cx: &NotesContext, page: &SharedNotesPage) -> HashMap<u64, NotesKey> {
        page.key_sets
            .iter()
            .filter_map(|set| {
                cx.env
                    .identity
                    .sharing_open_notes_key_set(set, &cx.manifest)
                    .ok()
                    .map(|key| (set.access_epoch, key))
            })
            .collect()
    }

    /// Owner duties: issue the current epoch's key, then move every older
    /// note into it. Returns the page to read from.
    async fn share_notes_maintain(
        &self,
        cx: &NotesContext,
        mut page: SharedNotesPage,
    ) -> AppResult<SharedNotesPage> {
        if !cx.owner() {
            return Ok(page);
        }
        let epoch = cx.manifest.manifest().access_epoch;
        if page.access_epoch != epoch {
            return Ok(page);
        }
        let mut keys = Self::share_note_keys(cx, &page);
        if let std::collections::hash_map::Entry::Vacant(slot) = keys.entry(epoch) {
            let key = NotesKey::generate().map_err(integrity)?;
            let set = cx
                .env
                .identity
                .sharing_seal_notes_key_set(&cx.manifest, &key)
                .map_err(integrity)?;
            cx.env
                .wait(cx.api()?.put_share_notes_key(cx.share, &set))
                .await?;
            page.key_sets.push(set);
            slot.insert(key);
        }
        let stale: Vec<&SharedNote> = page
            .notes
            .iter()
            .filter(|n| n.access_epoch < epoch)
            .collect();
        if stale.is_empty() {
            return Ok(page);
        }
        let authors = self.share_note_authors(cx).await?;
        let current = &keys[&epoch];
        let mut moved = Vec::new();
        for note in stale {
            let (Some(old), Some(author)) = (
                keys.get(&note.access_epoch),
                authors.get(&note.author_device_id),
            ) else {
                continue;
            };
            if let Ok(next) =
                rekey_shared_note(note, cx.instance, cx.share, old, author, epoch, current)
            {
                moved.push(RekeyedSharedNote {
                    note_id: next.note_id,
                    revision: next.revision,
                    nonce: next.nonce,
                    ciphertext: next.ciphertext,
                });
            }
        }
        for batch in moved.chunks(MAX_REKEY_BATCH) {
            let request = RekeySharedNotesRequest {
                access_epoch: epoch,
                notes: batch.to_vec(),
            };
            cx.env
                .wait(cx.api()?.rekey_share_notes(cx.share, &request))
                .await?;
        }
        cx.env.wait(cx.api()?.share_notes(cx.share)).await
    }

    pub async fn share_notes(&self, share_id: String) -> AppResult<SharedNotesDto> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let cx = self.share_notes_context(&share_id).await?;
        let mut result = SharedNotesDto {
            share_id: share_id.clone(),
            enabled: cx.enabled(),
            owner: cx.owner(),
            owner_user_id: cx.manifest.manifest().owner_user_id.to_string(),
            waiting_for_owner: false,
            notes: Vec::new(),
            pending: 0,
        };
        if !result.enabled {
            return Ok(result);
        }
        let page = cx.env.wait(cx.api()?.share_notes(cx.share)).await?;
        let page = self.share_notes_maintain(&cx, page).await?;
        self.sharing_check(&cx.env).await?;
        let epoch = cx.manifest.manifest().access_epoch;
        let keys = Self::share_note_keys(&cx, &page);
        result.waiting_for_owner = !keys.contains_key(&epoch);
        let authors = if page
            .notes
            .iter()
            .all(|n| cx.manifest.member(n.author_device_id).is_some())
        {
            cx.manifest
                .manifest()
                .members
                .iter()
                .filter_map(|m| {
                    m.signing_public_key
                        .to_array::<32>()
                        .map(|k| (m.device_id, k))
                })
                .collect()
        } else {
            self.share_note_authors(&cx).await?
        };
        for note in &page.notes {
            let opened = keys
                .get(&note.access_epoch)
                .zip(authors.get(&note.author_device_id))
                .and_then(|(key, author)| {
                    open_shared_note(note, cx.instance, cx.share, key, author).ok()
                })
                .and_then(|plain| serde_json::from_slice::<Payload>(plain.as_slice()).ok());
            let Some(payload) = opened else {
                result.pending += 1;
                continue;
            };
            let mine = note.author_user_id == cx.env.pin.user;
            result.notes.push(SharedNoteDto {
                id: note.note_id.to_string(),
                title: payload.title,
                body: payload.body,
                tags: payload.tags,
                scope: payload.scope,
                author_user_id: note.author_user_id.to_string(),
                mine,
                can_delete: mine || cx.owner(),
                revision: note.revision,
                created_at_ms: payload.created_at_ms,
                updated_at_ms: payload.updated_at_ms,
            });
        }
        Ok(result)
    }

    pub async fn share_note_save(
        &self,
        share_id: String,
        input: SharedNoteInputDto,
    ) -> AppResult<SharedNoteDto> {
        validate_input(&input)?;
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let cx = self.share_notes_context(&share_id).await?;
        if !cx.enabled() {
            return Err(AppError::Unsupported(
                "shared notes are not offered by this server".into(),
            ));
        }
        let page = cx.env.wait(cx.api()?.share_notes(cx.share)).await?;
        let page = self.share_notes_maintain(&cx, page).await?;
        let epoch = cx.manifest.manifest().access_epoch;
        let keys = Self::share_note_keys(&cx, &page);
        let key = keys
            .get(&epoch)
            .ok_or_else(|| invalid("the owner has not enabled shared notes for this item yet"))?;
        let now = chrono::Utc::now().timestamp_millis();
        let (note_id, revision, created) = if input.id.is_empty() {
            (ObjectId::new(), 1, now)
        } else {
            let id = ObjectId::from_str(&input.id).map_err(|_| invalid("invalid note id"))?;
            let existing = page
                .notes
                .iter()
                .find(|n| n.note_id == id)
                .ok_or_else(|| AppError::not_found("shared note", id))?;
            if existing.author_user_id != cx.env.pin.user {
                return Err(invalid("shared notes are edited by their author"));
            }
            let created = keys
                .get(&existing.access_epoch)
                .and_then(|key| {
                    let author = cx.manifest.member(existing.author_device_id)?;
                    let signing = author.signing_public_key.to_array::<32>()?;
                    open_shared_note(existing, cx.instance, cx.share, key, &signing).ok()
                })
                .and_then(|plain| serde_json::from_slice::<Payload>(plain.as_slice()).ok())
                .map_or(now, |p| p.created_at_ms);
            (id, existing.revision + 1, created)
        };
        let payload = Payload {
            title: input.title.trim().to_owned(),
            body: input.body,
            tags: input
                .tags
                .iter()
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
                .collect(),
            scope: input.scope,
            created_at_ms: created,
            updated_at_ms: now,
        };
        let bytes = zeroize::Zeroizing::new(
            serde_json::to_vec(&payload).map_err(|_| invalid("cannot encode shared note"))?,
        );
        let note = cx
            .env
            .identity
            .sharing_seal_note(
                cx.env.pin.user,
                cx.instance,
                cx.share,
                note_id,
                revision,
                epoch,
                key,
                &bytes,
            )
            .map_err(integrity)?;
        cx.env
            .wait(cx.api()?.put_share_note(cx.share, &note))
            .await?;
        Ok(SharedNoteDto {
            id: note_id.to_string(),
            title: payload.title,
            body: payload.body,
            tags: payload.tags,
            scope: payload.scope,
            author_user_id: cx.env.pin.user.to_string(),
            mine: true,
            can_delete: true,
            revision,
            created_at_ms: created,
            updated_at_ms: now,
        })
    }

    pub async fn share_note_delete(&self, share_id: String, note_id: String) -> AppResult<()> {
        let session = self.session().await?;
        let _gate = self.sharing_gate(&session).await?;
        let cx = self.share_notes_context(&share_id).await?;
        let id = ObjectId::from_str(&note_id).map_err(|_| invalid("invalid note id"))?;
        cx.env.wait(cx.api()?.delete_share_note(cx.share, id)).await
    }
}
