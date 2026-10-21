//! Staged imports of database connections from a connection string or a
//! `.pgpass` file. Parsing happens natively; the bridge sees a review without
//! passwords and confirms chosen entries by index with a short-lived ticket.
use crate::{AppCore, AppError, AppResult};
use cc_db_core::import::{parse_pgpass, parse_uri, ImportedConnection};
use cc_models::{
    database::{DbEngine, DbRoute},
    secret::SecretValue,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const IMPORT_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_STAGED: usize = 8;

struct Staged {
    entries: Vec<ImportedConnection>,
    expires: Instant,
}

#[derive(Default)]
pub(crate) struct DbImports {
    staged: Mutex<HashMap<String, Staged>>,
}

impl DbImports {
    pub(crate) fn clear(&self) {
        self.staged
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

#[derive(Debug, Serialize)]
pub struct DbImportEntryDto {
    pub index: usize,
    pub name: String,
    pub engine: DbEngine,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub has_password: bool,
    pub warning_codes: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct DbImportReviewDto {
    pub ticket: String,
    pub expires_in_seconds: u64,
    pub entries: Vec<DbImportEntryDto>,
    pub skipped_wildcards: usize,
    pub skipped_local_sockets: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbImportChoice {
    pub index: usize,
    #[serde(default = "yes")]
    pub production: bool,
    #[serde(default = "yes")]
    pub save_password: bool,
    #[serde(default)]
    pub route: DbRoute,
}

fn yes() -> bool {
    true
}

fn invalid(reason: &'static str) -> AppError {
    AppError::invalid("database", reason)
}

impl AppCore {
    fn db_stage_import(
        &self,
        unlocked: &crate::session::Unlocked,
        entries: Vec<ImportedConnection>,
        skipped_wildcards: usize,
        skipped_local_sockets: usize,
    ) -> AppResult<DbImportReviewDto> {
        if entries.is_empty() {
            return Err(invalid("import_empty"));
        }
        let review = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let connection = entry.connection();
                let mut warning_codes = Vec::new();
                if connection.tls.mode == cc_models::database::DbTlsMode::Disable {
                    warning_codes.push("tls_disabled");
                }
                if entry.password().is_none() {
                    warning_codes.push("no_password");
                }
                DbImportEntryDto {
                    index,
                    name: connection.name.clone(),
                    engine: connection.engine,
                    host: connection.host.clone(),
                    port: connection.port,
                    database: connection.database.clone(),
                    username: connection.username.clone(),
                    has_password: entry.password().is_some(),
                    warning_codes,
                }
            })
            .collect();
        let ticket = uuid::Uuid::new_v4().to_string();
        let mut staged = unlocked
            .db_imports
            .staged
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        staged.retain(|_, item| item.expires > now);
        if staged.len() >= MAX_STAGED {
            return Err(invalid("too_many_imports"));
        }
        staged.insert(
            ticket.clone(),
            Staged {
                entries,
                expires: now + IMPORT_TTL,
            },
        );
        Ok(DbImportReviewDto {
            ticket,
            expires_in_seconds: IMPORT_TTL.as_secs(),
            entries: review,
            skipped_wildcards,
            skipped_local_sockets,
        })
    }

    /// Parses one connection string (`postgres://`, `mysql://`, `clickhouse://`,
    /// `redis://`, `mongodb[+srv]://`); the password moves into the staged entry.
    pub async fn db_import_uri(&self, uri: SecretString) -> AppResult<DbImportReviewDto> {
        let (_, unlocked) = self.unlocked().await?;
        let entry = parse_uri(&uri).map_err(|_| invalid("import_invalid"))?;
        self.db_stage_import(&unlocked, vec![entry], 0, 0)
    }

    /// Parses `.pgpass` contents; wildcard and local-socket lines are counted
    /// and skipped.
    pub async fn db_import_pgpass(&self, contents: SecretString) -> AppResult<DbImportReviewDto> {
        let (_, unlocked) = self.unlocked().await?;
        let parsed = parse_pgpass(&contents).map_err(|_| invalid("import_invalid"))?;
        self.db_stage_import(
            &unlocked,
            parsed.entries,
            parsed.skipped_wildcards,
            parsed.skipped_local_sockets,
        )
    }

    /// Reads `$PGPASSFILE` or `~/.pgpass` (desktop) and stages it. `None` when
    /// the file does not exist.
    pub async fn db_import_default_pgpass(&self) -> AppResult<Option<DbImportReviewDto>> {
        let path = std::env::var_os("PGPASSFILE")
            .map(std::path::PathBuf::from)
            .or_else(|| dirs_home().map(|home| home.join(".pgpass")));
        let Some(path) = path else {
            return Ok(None);
        };
        let contents = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(invalid("import_unreadable")),
        };
        let contents = zeroize::Zeroizing::new(contents);
        let text = std::str::from_utf8(&contents).map_err(|_| invalid("import_invalid"))?;
        self.db_import_pgpass(SecretString::from(text.to_owned()))
            .await
            .map(Some)
    }

    /// Saves the chosen staged entries; the ticket is consumed either way.
    pub async fn db_confirm_import(
        &self,
        ticket: String,
        choices: Vec<DbImportChoice>,
    ) -> AppResult<usize> {
        let (_, unlocked) = self.unlocked().await?;
        let staged = unlocked
            .db_imports
            .staged
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ticket)
            .ok_or_else(|| invalid("import_expired"))?;
        if staged.expires <= Instant::now() {
            return Err(invalid("import_expired"));
        }
        let mut entries: Vec<Option<ImportedConnection>> =
            staged.entries.into_iter().map(Some).collect();
        let mut saved = 0;
        for choice in choices {
            let entry = entries
                .get_mut(choice.index)
                .and_then(Option::take)
                .ok_or_else(|| invalid("import_choice_invalid"))?;
            let (mut connection, password) = entry.into_parts();
            connection.id = cc_models::ObjectId::NIL;
            connection.production = choice.production;
            connection.route = choice.route;
            let password = password
                .filter(|_| choice.save_password)
                .map(|password| SecretValue::new(password.expose_secret().to_owned()));
            self.db_save_connection(connection, password, None).await?;
            saved += 1;
        }
        Ok(saved)
    }

    pub async fn db_discard_import(&self, ticket: String) -> AppResult<()> {
        let (_, unlocked) = self.unlocked().await?;
        unlocked
            .db_imports
            .staged
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ticket);
        Ok(())
    }
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
}
