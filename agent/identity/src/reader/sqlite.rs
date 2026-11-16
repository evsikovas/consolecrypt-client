//! Independent reader ledger; adapted bounded SQLite backend, no Node schema migration.
use super::policy::{replay, ReaderGenesis as Genesis};
use super::{
    ReaderCapacity, ReaderCompareAndSwap, ReaderError as Error, ReaderIssuerPin,
    ReaderResult as Result, ReaderState,
};
use crate::{Hash, Now, MAX_WIRE_BYTES};
use rusqlite::{
    config::DbConfig, limits::Limit, params, Connection, ErrorCode, OpenFlags, TransactionBehavior,
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DB_NAME: &str = "reader.sqlite3";
const ANCHOR_NAME: &str = "reader-head.json";
const FORMAT: &str = "cc-agent-reader-anchor-v1";
const PAGE_BYTES: u64 = 4096;
const MAX_RECORD_BYTES: usize = 64 * 1024;
const CREATE_META:&str="CREATE TABLE reader_meta (singleton INTEGER PRIMARY KEY CHECK(singleton=1), schema_version INTEGER NOT NULL CHECK(schema_version=1), ledger_revision INTEGER NOT NULL CHECK(ledger_revision>=0), ledger_digest BLOB NOT NULL CHECK(length(ledger_digest)=32), logical_bytes INTEGER NOT NULL CHECK(logical_bytes>=0), pending_anchor INTEGER NOT NULL CHECK(pending_anchor IN (0,1)))";
const CREATE_EVENTS:&str="CREATE TABLE reader_events (ledger_revision INTEGER PRIMARY KEY CHECK(ledger_revision>=0), previous_ledger_digest BLOB NOT NULL CHECK(length(previous_ledger_digest)=32), entry BLOB NOT NULL CHECK(length(entry)<=65536), ledger_digest BLOB NOT NULL CHECK(length(ledger_digest)=32))";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderStorageBudget {
    pub max_records: u64,
    pub max_logical_bytes: u64,
    pub max_database_bytes: u64,
    pub max_wal_bytes: u64,
    pub max_shm_bytes: u64,
    pub free_reserve_bytes: u64,
}
impl Default for ReaderStorageBudget {
    fn default() -> Self {
        Self {
            max_records: 10_000,
            max_logical_bytes: 8 * 1024 * 1024,
            max_database_bytes: 16 * 1024 * 1024,
            max_wal_bytes: 20 * 1024 * 1024,
            max_shm_bytes: 1024 * 1024,
            free_reserve_bytes: 8 * 1024 * 1024,
        }
    }
}
impl ReaderStorageBudget {
    fn validate(&self) -> Result<()> {
        if !(2..=10_000).contains(&self.max_records)
            || !(512..=8 * 1024 * 1024).contains(&self.max_logical_bytes)
            || !(1024 * 1024..=16 * 1024 * 1024).contains(&self.max_database_bytes)
            || self.max_database_bytes % PAGE_BYTES != 0
            || self.max_wal_bytes < self.worst_write_bytes()?
            || self.max_wal_bytes > 20 * 1024 * 1024
            || !(64 * 1024..=1024 * 1024).contains(&self.max_shm_bytes)
            || self.free_reserve_bytes > 8 * 1024 * 1024
        {
            return Err(Error::Quota);
        }
        Ok(())
    }
    fn worst_write_bytes(&self) -> Result<u64> {
        // cache_spill=OFF and an exact no-trigger schema: a transaction cannot
        // dirty more database pages than max_page_count. Include WAL frame/header
        // overhead and a conservative extra allowance for the pending-clear txn.
        (self.max_database_bytes / PAGE_BYTES)
            .checked_mul(PAGE_BYTES + 24)
            .and_then(|v| v.checked_add(32 + 64 * 1024))
            .ok_or(Error::Quota)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Head {
    revision: u64,
    digest: Hash,
    logical_bytes: u64,
    pending: bool,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Anchor {
    format: String,
    #[serde(rename = "ledgerRevision")]
    revision: u64,
    #[serde(rename = "ledgerDigest")]
    digest: Hash,
}
struct Cache {
    state: ReaderState,
    head: Head,
}
struct Inner {
    conn: Connection,
    cache: Option<Cache>,
    startup_verified: u64,
    suffix_verified: u64,
    cas_verified: u64,
    poisoned: bool,
}
/// Live cache contains only a state verified in this process. Its persistent
/// representation is signed transition history, never Deserialize<ReaderState>.
pub struct ReaderSqliteLedger {
    data: PathBuf,
    anchor: PathBuf,
    pin: ReaderIssuerPin,
    budget: ReaderStorageBudget,
    database_identity: (u64, u64),
    inner: RefCell<Inner>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderStorageStatus {
    pub provider: &'static str,
    pub revision: u64,
    pub policy: ReaderCapacity,
    pub logical_bytes: u64,
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub shm_bytes: u64,
    pub startup_records_verified: u64,
    pub suffix_records_verified: u64,
    pub cas_records_verified: u64,
    pub budget: ReaderStorageBudget,
}

impl ReaderSqliteLedger {
    pub fn create(
        data: impl AsRef<Path>,
        anchor: impl AsRef<Path>,
        pin: ReaderIssuerPin,
        now: Now,
        budget: ReaderStorageBudget,
    ) -> Result<Self> {
        ReaderState::bootstrap(pin.clone(), now)?;
        budget.validate()?;
        let data = crate::durable::absolute(data.as_ref())?;
        let anchor = crate::durable::absolute(anchor.as_ref())?;
        separate(&data, &anchor)?;
        private_directory(data.parent().ok_or(Error::Storage)?)?;
        private_directory(anchor.parent().ok_or(Error::Storage)?)?;
        crate::durable::secure_new_dir(&data)?;
        crate::durable::secure_new_dir(&anchor)?;
        private_directory(&data)?;
        private_directory(&anchor)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(data.join(DB_NAME))
            .map_err(|_| Error::Storage)?
            .sync_all()
            .map_err(|_| Error::Storage)?;
        let mut conn = connect(&data, &budget)?;
        conn.execute_batch(&format!("{CREATE_META};{CREATE_EVENTS};"))
            .map_err(sql_error)?;
        let bytes = serde_json::to_vec(&Genesis {
            version: 1,
            pin: pin.clone(),
            at: now.0,
        })
        .map_err(|_| Error::Storage)?;
        let digest = chained(Hash::from_bytes([0; 32]), &bytes);
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO reader_events VALUES (0,?1,?2,?3)",
            params![&[0u8; 32][..], &bytes, digest.as_bytes().as_slice()],
        )
        .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO reader_meta VALUES (1,1,0,?1,?2,1)",
            params![digest.as_bytes().as_slice(), bytes.len() as i64],
        )
        .map_err(sql_error)?;
        tx.commit().map_err(sql_error)?;
        sync_dir(&data)?;
        let head = Head {
            revision: 0,
            digest,
            logical_bytes: bytes.len() as u64,
            pending: false,
        };
        write_anchor(&anchor, &head)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        tx.execute(
            "UPDATE reader_meta SET pending_anchor=0 WHERE singleton=1",
            [],
        )
        .map_err(sql_error)?;
        tx.commit().map_err(sql_error)?;
        sync_dir(data.parent().ok_or(Error::Storage)?)?;
        sync_dir(anchor.parent().ok_or(Error::Storage)?)?;
        let identity = db_identity(&data)?;
        let store = Self {
            data,
            anchor,
            pin,
            budget,
            database_identity: identity,
            inner: RefCell::new(Inner {
                conn,
                cache: None,
                startup_verified: 0,
                suffix_verified: 0,
                cas_verified: 0,
                poisoned: false,
            }),
        };
        store.load()?;
        Ok(store)
    }
    pub fn open(
        data: impl AsRef<Path>,
        anchor: impl AsRef<Path>,
        pin: ReaderIssuerPin,
        budget: ReaderStorageBudget,
    ) -> Result<Self> {
        budget.validate()?;
        let data = crate::durable::absolute(data.as_ref())?;
        let anchor = crate::durable::absolute(anchor.as_ref())?;
        separate(&data, &anchor)?;
        private_directory(&data)?;
        private_directory(&anchor)?;
        let conn = connect(&data, &budget)?;
        let identity = db_identity(&data)?;
        let store = Self {
            data,
            anchor,
            pin,
            budget,
            database_identity: identity,
            inner: RefCell::new(Inner {
                conn,
                cache: None,
                startup_verified: 0,
                suffix_verified: 0,
                cas_verified: 0,
                poisoned: false,
            }),
        };
        store.load()?;
        Ok(store)
    }
    pub fn status(&self) -> Result<ReaderStorageStatus> {
        let state = self.load()?;
        let inner = self.inner.borrow();
        let head = &inner.cache.as_ref().ok_or(Error::Storage)?.head;
        let sizes = file_sizes(&self.data, &self.budget)?;
        Ok(ReaderStorageStatus {
            provider: "reader-sqlite-journal-v1",
            revision: state.revision(),
            policy: state.capacity(),
            logical_bytes: head.logical_bytes,
            database_bytes: sizes.0,
            wal_bytes: sizes.1,
            shm_bytes: sizes.2,
            startup_records_verified: inner.startup_verified,
            suffix_records_verified: inner.suffix_verified,
            cas_records_verified: inner.cas_verified,
            budget: self.budget.clone(),
        })
    }
}
impl ReaderCompareAndSwap for ReaderSqliteLedger {
    fn load(&self) -> Result<ReaderState> {
        private_directory(&self.data)?;
        private_directory(&self.anchor)?;
        if db_identity(&self.data)? != self.database_identity {
            return Err(Error::Storage);
        }
        file_sizes(&self.data, &self.budget)?;
        let mut inner = self.inner.borrow_mut();
        if inner.poisoned {
            return Err(Error::UncertainCommit);
        }
        let Inner {
            conn,
            cache,
            startup_verified,
            suffix_verified,
            ..
        } = &mut *inner;
        refresh(
            conn,
            cache,
            startup_verified,
            suffix_verified,
            &self.anchor,
            &self.pin,
            &self.budget,
        )
    }
    fn compare_and_swap(&mut self, expected: u64, next: ReaderState) -> Result<()> {
        private_directory(&self.data)?;
        private_directory(&self.anchor)?;
        if db_identity(&self.data)? != self.database_identity {
            return Err(Error::Storage);
        }
        let inner = self.inner.get_mut();
        if inner.poisoned {
            return Err(Error::UncertainCommit);
        }
        // Physical SQLite WAL checkpoint only. No logical event/tombstone rows
        // are deleted. A reader preventing truncation causes bounded Conflict.
        let (busy, _, _): (i64, i64, i64) = inner
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(sql_error)?;
        if busy != 0 {
            return Err(Error::Conflict);
        }
        let Inner {
            conn,
            cache,
            startup_verified,
            suffix_verified,
            cas_verified,
            poisoned,
        } = inner;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let current = refresh(
            &tx,
            cache,
            startup_verified,
            suffix_verified,
            &self.anchor,
            &self.pin,
            &self.budget,
        )?;
        if current.revision() != expected {
            return Err(Error::Conflict);
        }
        if next.revision() != expected.checked_add(1).ok_or(Error::Quota)?
            || next.owner().context != self.pin.context
        {
            return Err(Error::Storage);
        }
        if next.revision() + 1 > self.budget.max_records {
            return Err(Error::Quota);
        }
        let entry = next.latest.clone().ok_or(Error::Storage)?;
        let verified = replay(current, entry.clone()).map_err(|_| Error::Storage)?;
        *cas_verified = cas_verified.checked_add(1).ok_or(Error::Quota)?;
        let bytes = serde_json::to_vec(&entry).map_err(|_| Error::Storage)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(Error::Quota);
        }
        let old_head = &cache.as_ref().ok_or(Error::Storage)?.head;
        let logical = old_head
            .logical_bytes
            .checked_add(bytes.len() as u64)
            .ok_or(Error::Quota)?;
        if logical > self.budget.max_logical_bytes {
            return Err(Error::Quota);
        }
        let sizes = file_sizes(&self.data, &self.budget)?;
        let reserve = self.budget.worst_write_bytes()?;
        if sizes.1.checked_add(reserve).ok_or(Error::Quota)? > self.budget.max_wal_bytes {
            return Err(Error::Quota);
        }
        if fs4::available_space(&self.data).map_err(|_| Error::Storage)?
            < reserve
                .checked_add(self.budget.free_reserve_bytes)
                .ok_or(Error::Quota)?
        {
            return Err(Error::Quota);
        }
        let digest = chained(old_head.digest, &bytes);
        let new_head = Head {
            revision: verified.revision(),
            digest,
            logical_bytes: logical,
            pending: false,
        };
        tx.execute(
            "INSERT INTO reader_events VALUES (?1,?2,?3,?4)",
            params![
                new_head.revision as i64,
                old_head.digest.as_bytes().as_slice(),
                &bytes,
                digest.as_bytes().as_slice()
            ],
        )
        .map_err(sql_error)?;
        let changed=tx.execute("UPDATE reader_meta SET ledger_revision=?1,ledger_digest=?2,logical_bytes=?3,pending_anchor=1 WHERE singleton=1 AND ledger_revision=?4 AND pending_anchor=0",params![new_head.revision as i64,digest.as_bytes().as_slice(),logical as i64,expected as i64]).map_err(sql_error)?;
        if changed != 1 {
            return Err(Error::Conflict);
        }
        #[cfg(test)]
        fault_hook("before-sql-commit");
        tx.commit().map_err(sql_error)?;
        // The pending barrier is durable before dropping SQLite's writer lock.
        // Other writers cannot acknowledge anything until this exact anchor is
        // synced and pending is cleared by another durable transaction.
        *poisoned = true;
        #[cfg(test)]
        fault_hook("after-sql-commit");
        sync_dir(&self.data)?;
        write_anchor(&self.anchor, &new_head)?;
        #[cfg(test)]
        fault_hook("after-anchor-ack");
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let pending = read_head(&tx)?;
        if pending.revision != new_head.revision
            || pending.digest != new_head.digest
            || pending.logical_bytes != new_head.logical_bytes
            || !pending.pending
            || !anchor_matches(&self.anchor, &new_head)?
        {
            return Err(Error::UncertainCommit);
        }
        tx.execute(
            "UPDATE reader_meta SET pending_anchor=0 WHERE singleton=1",
            [],
        )
        .map_err(sql_error)?;
        tx.commit().map_err(sql_error)?;
        *cache = Some(Cache {
            state: verified,
            head: new_head,
        });
        *poisoned = false;
        #[cfg(test)]
        fault_hook("after-pending-clear");
        Ok(())
    }
}
fn connect(data: &Path, budget: &ReaderStorageBudget) -> Result<Connection> {
    file_sizes(data, budget)?;
    let conn = Connection::open_with_flags(
        data.join(DB_NAME),
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(sql_error)?;
    conn.busy_timeout(Duration::from_millis(100))
        .map_err(sql_error)?;
    conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, (MAX_RECORD_BYTES + 4096) as i32)
        .map_err(sql_error)?;
    conn.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 8192)
        .map_err(sql_error)?;
    conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)
        .map_err(sql_error)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(sql_error)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)
        .map_err(sql_error)?;
    conn.pragma_update(None, "page_size", PAGE_BYTES as i64)
        .map_err(sql_error)?;
    conn.pragma_update(
        None,
        "max_page_count",
        (budget.max_database_bytes / PAGE_BYTES) as i64,
    )
    .map_err(sql_error)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(sql_error)?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(sql_error)?;
    conn.pragma_update(None, "cache_spill", "OFF")
        .map_err(sql_error)?;
    conn.pragma_update(None, "cache_size", -1024)
        .map_err(sql_error)?;
    conn.pragma_update(None, "temp_store", "MEMORY")
        .map_err(sql_error)?;
    conn.pragma_update(None, "wal_autocheckpoint", 32)
        .map_err(sql_error)?;
    conn.pragma_update(None, "journal_size_limit", budget.max_wal_bytes as i64)
        .map_err(sql_error)?;
    let mode: String = conn
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .map_err(sql_error)?;
    let sync: i64 = conn
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .map_err(sql_error)?;
    let pages: i64 = conn
        .pragma_query_value(None, "max_page_count", |r| r.get(0))
        .map_err(sql_error)?;
    let size: i64 = conn
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .map_err(sql_error)?;
    let spill: i64 = conn
        .pragma_query_value(None, "cache_spill", |r| r.get(0))
        .map_err(sql_error)?;
    let (version, source, codec): (String, String, i64) = conn
        .query_row(
            "SELECT sqlite_version(),sqlite_source_id(),sqlite_compileoption_used('HAS_CODEC')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(sql_error)?;
    if mode != "wal"
        || sync != 2
        || size != PAGE_BYTES as i64
        || pages as u64 > budget.max_database_bytes / PAGE_BYTES
        || spill != 0
        || version != "3.53.2"
        || !source.contains("d6e03d8c777cfa2d35e3b60d8ec3e0187f3e9f99d8e2ee9cac695fd6fcdf1a24")
        || codec != 0
    {
        return Err(Error::Storage);
    }
    Ok(conn)
}
fn schema(conn: &Connection) -> Result<()> {
    let mut stmt=conn.prepare("SELECT type,name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name LIMIT 3").map_err(sql_error)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(sql_error)?;
    let actual = rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    let expected = vec![
        (
            "table".to_string(),
            "reader_events".to_string(),
            CREATE_EVENTS.to_string(),
        ),
        (
            "table".to_string(),
            "reader_meta".to_string(),
            CREATE_META.to_string(),
        ),
    ];
    if actual != expected {
        return Err(Error::Storage);
    }
    Ok(())
}
fn read_head(conn: &Connection) -> Result<Head> {
    let (version,revision,digest,bytes,pending):(i64,i64,Vec<u8>,i64,i64)=conn.query_row("SELECT schema_version,ledger_revision,ledger_digest,logical_bytes,pending_anchor FROM reader_meta WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(sql_error)?;
    if version != 1 || revision < 0 || bytes < 0 || !(0..=1).contains(&pending) {
        return Err(Error::Storage);
    }
    Ok(Head {
        revision: revision as u64,
        digest: blob_hash(digest)?,
        logical_bytes: bytes as u64,
        pending: pending == 1,
    })
}
fn refresh(
    conn: &Connection,
    cache: &mut Option<Cache>,
    startup: &mut u64,
    suffix: &mut u64,
    anchor: &Path,
    pin: &ReaderIssuerPin,
    budget: &ReaderStorageBudget,
) -> Result<ReaderState> {
    schema(conn)?;
    let head = read_head(conn)?;
    if head.pending {
        return Err(Error::UncertainCommit);
    }
    if head.revision + 1 > budget.max_records || head.logical_bytes > budget.max_logical_bytes {
        return Err(Error::Quota);
    }
    if !anchor_matches(anchor, &head)? {
        return Err(Error::UncertainCommit);
    }
    if let Some(known) = cache.as_ref() {
        if head.revision < known.head.revision
            || (head.revision == known.head.revision
                && (head.digest != known.head.digest
                    || head.logical_bytes != known.head.logical_bytes))
        {
            return Err(Error::UncertainCommit);
        }
        if head == known.head {
            return Ok(known.state.clone());
        }
    }
    let start = cache.as_ref().map(|v| v.head.revision as i64).unwrap_or(-1);
    let mut state = cache.as_ref().map(|v| v.state.clone());
    let mut digest = cache
        .as_ref()
        .map(|v| v.head.digest)
        .unwrap_or(Hash::from_bytes([0; 32]));
    let mut total = cache.as_ref().map(|v| v.head.logical_bytes).unwrap_or(0);
    let mut count = 0u64;
    let mut statement=conn.prepare("SELECT ledger_revision,previous_ledger_digest,entry,ledger_digest FROM reader_events WHERE ledger_revision>?1 ORDER BY ledger_revision").map_err(sql_error)?;
    let mut rows = statement.query(params![start]).map_err(sql_error)?;
    while let Some(row) = rows.next().map_err(sql_error)? {
        let revision: i64 = row.get(0).map_err(sql_error)?;
        let previous: Vec<u8> = row.get(1).map_err(sql_error)?;
        let bytes: Vec<u8> = row.get(2).map_err(sql_error)?;
        let claimed: Vec<u8> = row.get(3).map_err(sql_error)?;
        let expected = state.as_ref().map(|v| v.revision() + 1).unwrap_or(0);
        if revision < 0
            || revision as u64 != expected
            || bytes.len() > MAX_RECORD_BYTES
            || blob_hash(previous)? != digest
        {
            return Err(Error::Storage);
        }
        let computed = chained(digest, &bytes);
        if computed != blob_hash(claimed)? {
            return Err(Error::UncertainCommit);
        }
        total = total.checked_add(bytes.len() as u64).ok_or(Error::Quota)?;
        count += 1;
        if total > budget.max_logical_bytes || count > budget.max_records {
            return Err(Error::Quota);
        }
        if revision == 0 {
            let genesis: Genesis =
                super::strict_json(&bytes, MAX_RECORD_BYTES).map_err(|_| Error::Storage)?;
            if genesis.version != 1 || genesis.pin != *pin {
                return Err(Error::Storage);
            }
            state = Some(
                ReaderState::bootstrap(genesis.pin, Now(genesis.at)).map_err(|_| Error::Storage)?,
            );
        } else {
            let entry = super::strict_json(&bytes, MAX_RECORD_BYTES).map_err(|_| Error::Storage)?;
            state = Some(
                replay(state.take().ok_or(Error::Storage)?, entry).map_err(|_| Error::Storage)?,
            );
        }
        digest = computed;
    }
    drop(rows);
    drop(statement);
    let state = state.ok_or(Error::Storage)?;
    if state.revision() != head.revision || digest != head.digest || total != head.logical_bytes {
        return Err(Error::UncertainCommit);
    }
    if read_head(conn)? != head || !anchor_matches(anchor, &head)? {
        return Err(Error::Conflict);
    }
    if cache.is_none() {
        *startup = startup.checked_add(count).ok_or(Error::Quota)?;
    } else {
        *suffix = suffix.checked_add(count).ok_or(Error::Quota)?;
    }
    *cache = Some(Cache {
        state: state.clone(),
        head,
    });
    Ok(state)
}
fn blob_hash(bytes: Vec<u8>) -> Result<Hash> {
    Ok(Hash::from_bytes(
        bytes.try_into().map_err(|_| Error::Storage)?,
    ))
}
fn anchor_matches(directory: &Path, head: &Head) -> Result<bool> {
    for entry in fs::read_dir(directory).map_err(|_| Error::Storage)? {
        if entry.map_err(|_| Error::Storage)?.file_name() != ANCHOR_NAME {
            return Err(Error::UncertainCommit);
        }
    }
    let path = directory.join(ANCHOR_NAME);
    private_file(&path, MAX_WIRE_BYTES as u64)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| Error::Storage)?
        .take(MAX_WIRE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Storage)?;
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(Error::Storage);
    }
    let anchor: Anchor =
        super::strict_json(&bytes, crate::MAX_WIRE_BYTES).map_err(|_| Error::Storage)?;
    if bytes != super::wire::canonical(&anchor)? {
        return Err(Error::Storage);
    }
    if anchor.format != FORMAT {
        return Err(Error::Storage);
    }
    Ok(anchor.revision == head.revision && anchor.digest == head.digest)
}
fn write_anchor(directory: &Path, head: &Head) -> Result<()> {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| Error::Entropy)?;
    if nonce == [0; 16] {
        return Err(Error::Entropy);
    }
    let temporary = directory.join(format!("reader-head-{}.tmp", hex::encode(nonce)));
    let bytes = super::wire::canonical(&Anchor {
        format: FORMAT.to_string(),
        revision: head.revision,
        digest: head.digest,
    })
    .map_err(|_| Error::Storage)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| Error::Storage)?;
        file.write_all(&bytes).map_err(|_| Error::Storage)?;
        file.sync_all().map_err(|_| Error::Storage)?;
        fs::rename(&temporary, directory.join(ANCHOR_NAME)).map_err(|_| Error::Storage)?;
        sync_dir(directory)
    })();
    if temporary.exists() {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn separate(a: &Path, b: &Path) -> Result<()> {
    if a.starts_with(b) || b.starts_with(a) {
        Err(Error::Storage)
    } else {
        Ok(())
    }
}
fn db_identity(data: &Path) -> Result<(u64, u64)> {
    let meta = fs::symlink_metadata(data.join(DB_NAME)).map_err(|_| Error::Storage)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Error::Storage);
    }
    Ok((meta.dev(), meta.ino()))
}
fn private_file(path: &Path, max: u64) -> Result<u64> {
    let meta = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.uid() != current_uid()?
        || meta.nlink() != 1
        || meta.permissions().mode() & 0o777 != 0o600
        || meta.len() > max
    {
        return Err(Error::Storage);
    }
    Ok(meta.len())
}
fn file_sizes(data: &Path, budget: &ReaderStorageBudget) -> Result<(u64, u64, u64)> {
    for entry in fs::read_dir(data).map_err(|_| Error::Storage)? {
        let name = entry.map_err(|_| Error::Storage)?.file_name();
        if name != DB_NAME
            && name != format!("{DB_NAME}-wal").as_str()
            && name != format!("{DB_NAME}-shm").as_str()
        {
            return Err(Error::Storage);
        }
    }
    let db = private_file(&data.join(DB_NAME), budget.max_database_bytes)?;
    let mut extra = [0u64; 2];
    for (i, (name, limit)) in [
        (format!("{DB_NAME}-wal"), budget.max_wal_bytes),
        (format!("{DB_NAME}-shm"), budget.max_shm_bytes),
    ]
    .iter()
    .enumerate()
    {
        let path = data.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => extra[i] = private_file(&path, *limit)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(Error::Storage),
        }
    }
    Ok((db, extra[0], extra[1]))
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|v| v.sync_all())
        .map_err(|_| Error::Storage)
}
fn sql_error(error: rusqlite::Error) -> Error {
    match error {
        rusqlite::Error::SqliteFailure(info, _) => match info.code {
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => Error::Conflict,
            ErrorCode::DiskFull => Error::Quota,
            _ => Error::Storage,
        },
        _ => Error::Storage,
    }
}

fn chained(previous: Hash, bytes: &[u8]) -> Hash {
    let mut payload = previous.as_bytes().to_vec();
    payload.extend(bytes);
    super::wire::hash(b"consolecrypt.agent.reader.ledger-chain.v1", &payload)
}
fn current_uid() -> Result<u32> {
    // Fixed Linux procfs process identity, never caller-supplied configuration.
    Ok(fs::metadata("/proc/self")
        .map_err(|_| Error::Storage)?
        .uid())
}
fn private_directory(path: &Path) -> Result<()> {
    crate::durable::secure_path(path)?;
    let meta = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
    if !meta.is_dir() || meta.uid() != current_uid()? || meta.permissions().mode() & 0o777 != 0o700
    {
        return Err(Error::Storage);
    }
    Ok(())
}
#[cfg(test)]
fn fault_hook(point: &str) {
    if std::env::var("CC_READER_SQLITE_TEST_FAULT").ok().as_deref() != Some(point) {
        return;
    }
    let root =
        PathBuf::from(std::env::var("CC_READER_SQLITE_TEST_ROOT").expect("owned reader test root"));
    assert!(
        root.is_absolute()
            && root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("cc-reader-sqlite-unit-")
    );
    assert!(
        fs::read(root.join("owned.marker")).unwrap() == b"cc-reader-sqlite-unit-v1",
        "owned test marker must match"
    );
    fs::write(root.join("fault.ready"), point.as_bytes()).unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(100));
    }
}
