//! Structural validation of server pages before anything is applied, so a
//! malformed or hostile response cannot corrupt local state or loop forever.

use crate::error::SyncError;
use cc_protocol::limits::{MAX_OBJECT_CIPHERTEXT_BYTES, NONCE_LEN, WRAPPED_DEK_LEN};
use cc_protocol::sync::{Change, ChangesResponse, SnapshotResponse, OBJECT_FORMAT_V1};
use std::collections::HashSet;

fn malformed(what: &str, reason: impl std::fmt::Display) -> SyncError {
    SyncError::MalformedResponse(format!("{what}: {reason}"))
}

fn check_change(c: &Change, live_only: bool) -> Result<(), String> {
    if c.revision < 1 {
        return Err("revision < 1".into());
    }
    if c.deleted == c.body.is_some() {
        return Err("body must be present iff the object is live".into());
    }
    if live_only && c.deleted {
        return Err("tombstone in snapshot".into());
    }
    if let Some(b) = &c.body {
        if b.ciphertext.is_empty() || b.ciphertext.len() > MAX_OBJECT_CIPHERTEXT_BYTES {
            return Err("ciphertext size out of bounds".into());
        }
        if b.format == OBJECT_FORMAT_V1
            && (b.nonce.len() != NONCE_LEN
                || b.wrapped_dek.len() != WRAPPED_DEK_LEN
                || b.wrapped_dek_nonce.len() != NONCE_LEN)
        {
            return Err("encrypted body field lengths".into());
        }
    }
    Ok(())
}

fn check_ordered(
    what: &str,
    items: &[Change],
    start: i64,
    latest: i64,
    live_only: bool,
) -> Result<(), SyncError> {
    let mut prev = start;
    let mut ids = HashSet::with_capacity(items.len());
    for c in items {
        if c.sequence <= prev {
            return Err(malformed(
                what,
                "sequences not strictly increasing above the cursor",
            ));
        }
        if c.sequence > latest {
            return Err(malformed(what, "sequence above latest_sequence"));
        }
        if !ids.insert(c.object_id) {
            return Err(malformed(what, "object listed twice"));
        }
        check_change(c, live_only).map_err(|r| malformed(what, r))?;
        prev = c.sequence;
    }
    Ok(())
}

/// Validate a `changes` page requested with `after` / `limit`.
pub(crate) fn changes_page(
    resp: &ChangesResponse,
    after: i64,
    limit: u32,
) -> Result<(), SyncError> {
    const W: &str = "changes";
    if resp.changes.len() > limit as usize {
        return Err(malformed(W, "more items than requested"));
    }
    if resp.latest_sequence < after {
        // A server whose sequence went backwards was restored from a backup:
        // callers screen pages with `SyncCursor::rollback_evidence` and start
        // a rollback recovery before validating (ADR-0103 addendum). A page
        // reaching this point unscreened is refused rather than applied.
        return Err(malformed(W, "latest_sequence below our cursor"));
    }
    check_ordered(W, &resp.changes, after, resp.latest_sequence, false)?;
    let expected = resp.changes.last().map_or(after, |c| c.sequence);
    if resp.next_after != expected {
        return Err(malformed(W, "next_after does not match the page"));
    }
    if resp.has_more && resp.changes.is_empty() {
        return Err(malformed(W, "has_more with an empty page"));
    }
    Ok(())
}

/// Validate a `snapshot` page requested with `cursor` / `limit`.
pub(crate) fn snapshot_page(
    resp: &SnapshotResponse,
    cursor: Option<i64>,
    limit: u32,
) -> Result<(), SyncError> {
    const W: &str = "snapshot";
    if resp.objects.len() > limit as usize {
        return Err(malformed(W, "more items than requested"));
    }
    let start = cursor.unwrap_or(0);
    check_ordered(W, &resp.objects, start, resp.latest_sequence, true)?;
    if let Some(next) = resp.next_cursor {
        match resp.objects.last() {
            None => return Err(malformed(W, "next_cursor with an empty page")),
            Some(last) if last.sequence != next => {
                return Err(malformed(W, "next_cursor does not match the page"))
            }
            _ => {}
        }
    }
    Ok(())
}
