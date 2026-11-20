//! Private publication ACL subset. No existing descriptor is modified. This
//! proves a bounded observation, not an immutable ACL or an admin isolation.
use std::io;

mod kernel;
#[cfg(test)]
pub(super) mod tests;

pub(super) use kernel::{create, verify};

const MAX_DESCRIPTOR: usize = 64 * 1024;
const MAX_ACES: usize = 64;
const FILE_ALL: u32 = 0x001f_01ff;
const SYSTEM: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];
const ADMINISTRATORS: &[u8] = &[1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "publication private ACL is unsupported or unavailable",
    )
}
fn need(value: bool) -> io::Result<()> {
    if value { Ok(()) } else { Err(denied()) }
}
fn bytes(raw: &[u8], offset: usize, count: usize) -> io::Result<&[u8]> {
    offset
        .checked_add(count)
        .and_then(|end| raw.get(offset..end))
        .ok_or_else(denied)
}
fn u16_at(raw: &[u8], offset: usize) -> io::Result<u16> {
    Ok(u16::from_le_bytes(
        bytes(raw, offset, 2)?.try_into().map_err(|_| denied())?,
    ))
}
fn u32_at(raw: &[u8], offset: usize) -> io::Result<u32> {
    Ok(u32::from_le_bytes(
        bytes(raw, offset, 4)?.try_into().map_err(|_| denied())?,
    ))
}
/// Bounded binary SID, no pointer dereference or textual SID normalization.
fn sid_at(raw: &[u8], offset: usize) -> io::Result<&[u8]> {
    let header = bytes(raw, offset, 8)?;
    need(header[0] == 1 && header[1] <= 15)?;
    bytes(raw, offset, 8 + usize::from(header[1]) * 4)
}
fn descriptor(raw: &[u8], user: &[u8], protected: bool) -> io::Result<()> {
    need((20..=MAX_DESCRIPTOR).contains(&raw.len()))?;
    need(sid_at(user, 0)? == user && user != SYSTEM && user != ADMINISTRATORS)?;
    need(raw[0] == 1 && raw[1] == 0)?;
    let control = u16_at(raw, 2)?;
    // Self-relative, present NON-NULL DACL; protected is mandatory for a new
    // revision/staging family. Existing profile inheritance is never rewritten.
    need(control & 0x8004 == 0x8004 && (!protected || control & 0x1000 != 0))?;
    let owner = u32_at(raw, 4)? as usize;
    let acl = u32_at(raw, 16)? as usize;
    need(owner >= 20 && owner % 4 == 0 && acl >= 20 && acl % 4 == 0)?;
    let owner_sid = sid_at(raw, owner)?;
    need(owner_sid == user)?;
    let header = raw.get(acl..acl + 8).ok_or_else(denied)?;
    need(header[0] == 2 && header[1] == 0 && header[6..8] == [0, 0])?;
    let size = usize::from(u16_at(header, 2)?);
    let count = usize::from(u16_at(header, 4)?);
    need(size >= 8 && count > 0 && count <= MAX_ACES)?;
    let body = raw.get(acl..acl + size).ok_or_else(denied)?;
    need(owner + owner_sid.len() <= acl || acl + size <= owner)?;
    let mut cursor = 8;
    let mut owner_full = false;
    let mut owner_inherits = false;
    for _ in 0..count {
        let header = body.get(cursor..cursor + 8).ok_or_else(denied)?;
        // Only ordinary ALLOW ACEs. Deny/callback/object/conditional/unknown
        // forms are rejected, even when a general AccessCheck could allow them.
        need(header[0] == 0 && header[1] & !0x1f == 0)?;
        let length = usize::from(u16_at(header, 2)?);
        need(length >= 16 && length % 4 == 0)?;
        let ace = body.get(cursor..cursor + length).ok_or_else(denied)?;
        let sid = sid_at(ace, 8)?;
        need(sid.len() + 8 == length && (sid == user || sid == SYSTEM || sid == ADMINISTRATORS))?;
        let mask = u32_at(ace, 4)?;
        need(mask & !(FILE_ALL | 0xf000_0000) == 0)?;
        let full = mask & FILE_ALL == FILE_ALL || mask & 0x1000_0000 != 0;
        if sid == user && ace[1] & 8 == 0 && full {
            owner_full = true;
            // No NO_PROPAGATE/INHERIT_ONLY. Files AND child directories must
            // receive the owner grant before plaintext can be copied.
            owner_inherits |= ace[1] & 0x0f == 3;
        }
        cursor += length;
    }
    need(
        body.get(cursor..)
            .ok_or_else(denied)?
            .iter()
            .all(|v| *v == 0),
    )?;
    need(owner_full && (!protected || owner_inherits))
}
/// A fixed protected, self-relative descriptor supplied AT directory creation.
/// Trusted local admin/SYSTEM access is explicit, never a third-party grant.
fn new_descriptor(user: &[u8]) -> io::Result<Vec<u8>> {
    need(sid_at(user, 0)? == user && user != SYSTEM && user != ADMINISTRATORS)?;
    let acl_start = (20 + user.len()).next_multiple_of(4);
    let mut out = vec![0u8; acl_start + 8];
    out[0] = 1;
    out[2..4].copy_from_slice(&0x9004u16.to_le_bytes());
    out[4..8].copy_from_slice(&20u32.to_le_bytes());
    out[16..20].copy_from_slice(&(acl_start as u32).to_le_bytes());
    out[20..20 + user.len()].copy_from_slice(user);
    out[acl_start] = 2;
    out[acl_start + 4..acl_start + 6].copy_from_slice(&3u16.to_le_bytes());
    for sid in [user, SYSTEM, ADMINISTRATORS] {
        out.extend_from_slice(&[0, 3]);
        out.extend_from_slice(&((8 + sid.len()) as u16).to_le_bytes());
        out.extend_from_slice(&FILE_ALL.to_le_bytes());
        out.extend_from_slice(sid);
    }
    let acl_size = out.len() - acl_start;
    out[acl_start + 2..acl_start + 4].copy_from_slice(&(acl_size as u16).to_le_bytes());
    descriptor(&out, user, true)?;
    Ok(out)
}
