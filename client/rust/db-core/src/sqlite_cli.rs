//! SQLite files on another machine: the `sqlite3` program on an SSH host, in
//! a Docker container or a Kubernetes Pod runs each guarded statement. No file
//! is copied. The statement travels on standard input (never on the command
//! line, where other users could see it); bound values are written into it as
//! SQLite literals after the guard approved the statement.
//!
//! Reads run `-readonly` against `file:…?mode=ro` and under
//! `PRAGMA query_only`; writes only when the guard permitted them. `-safe`
//! (3.37+) disables ATTACH, file functions, extensions and file dot-commands.
//! Output is `-json` (3.33+) or CSV with a header and a NULL sentinel.
use crate::{streaming::RowConsumer, DbCell, DbColumn, DbError, DbParameter};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use cc_models::database::{DbConnection, DbOptions};
use serde::de::{Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;
use sqlparser::dialect::SQLiteDialect;
use sqlparser::tokenizer::{Token, Tokenizer};
use std::{fmt, sync::Arc, time::Duration, time::Instant};
use tokio_util::sync::CancellationToken;

/// The oldest `sqlite3` with the table-valued PRAGMA functions the structure
/// browser reads.
pub const MIN_VERSION: (u32, u32, u32) = (3, 16, 0);
const JSON_VERSION: (u32, u32, u32) = (3, 33, 0);
const SAFE_VERSION: (u32, u32, u32) = (3, 37, 0);
/// Busy timeout in milliseconds (`.timeout`).
const BUSY_MILLISECONDS: u32 = 5000;

/// What one `sqlite3` run produced. `stdout` stops at the caller's limit.
#[derive(Default, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Exit status; None when the transport reported none.
    pub status: Option<i32>,
    /// `stdout` was cut at the limit.
    pub truncated: bool,
}

impl fmt::Debug for CliOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CliOutput")
            .field("stdout_bytes", &self.stdout.len())
            .field("stderr_bytes", &self.stderr.len())
            .field("status", &self.status)
            .field("truncated", &self.truncated)
            .finish()
    }
}

/// Runs a program on the target with the given argument vector and
/// standard input. Implementations: SSH exec, Docker exec, Kubernetes exec
/// (and a local process in tests). Cancellation stops the program.
#[async_trait]
pub trait SqliteCliRunner: Send + Sync + fmt::Debug {
    async fn run(
        &self,
        argv: Vec<String>,
        stdin: Vec<u8>,
        stdout_limit: usize,
        cancel: CancellationToken,
    ) -> Result<CliOutput, DbError>;
}

/// Quotes an argument vector for a POSIX shell (the SSH exec channel).
pub fn shell_command(argv: &[String]) -> String {
    argv.iter()
        .map(|argument| {
            if !argument.is_empty()
                && argument
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_./:=".contains(&b))
            {
                argument.clone()
            } else {
                format!("'{}'", argument.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `file:` URI of a path with `mode=ro` or `mode=rw` (neither creates).
pub fn file_uri(path: &str, writable: bool) -> String {
    let mut uri = String::from(if path.starts_with('/') {
        "file://"
    } else {
        "file:"
    });
    for byte in path.bytes() {
        match byte {
            b'%' | b'?' | b'#' => uri.push_str(&format!("%{byte:02x}")),
            0x21..=0x7e => uri.push(byte as char),
            _ => uri.push_str(&format!("%{byte:02x}")),
        }
    }
    uri.push_str(if writable { "?mode=rw" } else { "?mode=ro" });
    uri
}

/// `3.45.1 2024-01-30 …` → (3, 45, 1).
pub fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let word = text.split_whitespace().next()?;
    let mut parts = word.split('.').map(|part| part.parse::<u32>().ok());
    Some((
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    ))
}

/// Why `sqlite3` failed, from its exit status and message.
pub fn classify_failure(output: &CliOutput) -> DbError {
    let message = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    let missing_program = output.status == Some(127)
        || message.contains("executable file not found")
        || (message.contains("sqlite3")
            && (message.contains("not found") || message.contains("no such file or directory")));
    if missing_program {
        DbError::SqliteCliMissing
    } else if message.contains("file is not a database") || message.contains("encrypted") {
        DbError::NotSqlite
    } else if message.contains("unable to open database") {
        DbError::FileMissing
    } else if message.contains("readonly database")
        || message.contains("not authorized")
        || message.contains("safe mode")
    {
        DbError::PolicyDenied
    } else if message.contains("database is locked") || message.contains("database table is locked")
    {
        DbError::Timeout
    } else if message.contains("interrupted") {
        DbError::Cancelled
    } else {
        DbError::QueryFailed
    }
}

/// An SQLite literal for a bound value (`NULL`, `42`, `1.5`, `'it''s'`,
/// `X'00FF'`). Text with NUL travels as a cast blob.
pub fn sqlite_literal(cell: &DbCell) -> Result<String, DbError> {
    let text = |value: &str| {
        if value.contains('\0') {
            format!("CAST(X'{}' AS TEXT)", hex(value.as_bytes()))
        } else {
            format!("'{}'", value.replace('\'', "''"))
        }
    };
    Ok(match cell {
        DbCell::Null => "NULL".into(),
        DbCell::Bool(value) => if *value { "1" } else { "0" }.into(),
        DbCell::Int(value) => value
            .trim()
            .parse::<i64>()
            .map_err(|_| DbError::InvalidRequest)?
            .to_string(),
        DbCell::Uint(value) => {
            let value: u64 = value.trim().parse().map_err(|_| DbError::InvalidRequest)?;
            i64::try_from(value)
                .map_err(|_| DbError::InvalidRequest)?
                .to_string()
        }
        DbCell::Float(value) => {
            let value: f64 = value.trim().parse().map_err(|_| DbError::InvalidRequest)?;
            if value.is_nan() {
                "NULL".into()
            } else if value.is_infinite() {
                if value > 0.0 { "9e999" } else { "-9e999" }.into()
            } else {
                crate::sqlite::real_text(value)
            }
        }
        DbCell::Binary(value) => format!(
            "X'{}'",
            hex(&STANDARD
                .decode(value)
                .map_err(|_| DbError::InvalidRequest)?)
        ),
        DbCell::Decimal(value)
        | DbCell::Text(value)
        | DbCell::Date(value)
        | DbCell::Time(value)
        | DbCell::Timestamp(value)
        | DbCell::Json(value) => text(value),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// Replaces the `?N` placeholders of a guarded statement with literals.
pub fn inline_parameters(sql: &str, parameters: &[DbParameter]) -> Result<String, DbError> {
    let tokens = Tokenizer::new(&SQLiteDialect {}, sql)
        .tokenize_with_location()
        .map_err(|_| DbError::InvalidRequest)?;
    let mut output = String::with_capacity(sql.len());
    let mut cursor = 0;
    let mut used = 0;
    for token in &tokens {
        let Token::Placeholder(marker) = &token.token else {
            continue;
        };
        let index: usize = marker
            .strip_prefix('?')
            .and_then(|number| number.parse().ok())
            .filter(|index| (1..=parameters.len()).contains(index))
            .ok_or(DbError::InvalidRequest)?;
        let start = crate::parser::byte_offset(sql, token.span.start)?;
        let end = crate::parser::byte_offset(sql, token.span.end)?;
        if start < cursor || end < start {
            return Err(DbError::InvalidRequest);
        }
        output.push_str(&sql[cursor..start]);
        output.push_str(&sqlite_literal(&parameters[index - 1].value)?);
        cursor = end;
        used += 1;
    }
    if used == 0 && !parameters.is_empty() {
        return Err(DbError::InvalidRequest);
    }
    output.push_str(&sql[cursor..]);
    Ok(output)
}

/// One result of a statement: column names and typed rows.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CliResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<DbCell>>,
}

/// One JSON row with its keys in output order (duplicate names kept).
struct OrderedRow(Vec<(String, Box<RawValue>)>);

impl<'de> serde::Deserialize<'de> for OrderedRow {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RowVisitor;
        impl<'de> Visitor<'de> for RowVisitor {
            type Value = OrderedRow;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<OrderedRow, A::Error> {
                let mut fields = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    fields.push((key, value));
                }
                Ok(OrderedRow(fields))
            }
        }
        deserializer.deserialize_map(RowVisitor)
    }
}

/// A JSON value of `sqlite3 -json` as a cell. Numbers keep their spelling
/// (INTEGER without a point or exponent, REAL otherwise). The program writes
/// BLOB bytes as `\u00XX` characters; a string made only of such characters
/// with a control byte reads as binary.
fn json_cell(raw: &RawValue) -> Result<DbCell, DbError> {
    let text = raw.get().trim();
    if text == "null" {
        return Ok(DbCell::Null);
    }
    if text.starts_with('"') {
        let value: String = serde_json::from_str(text).map_err(|_| DbError::QueryFailed)?;
        let latin = value.chars().all(|c| (c as u32) <= 0xff);
        let control = value
            .chars()
            .any(|c| matches!(c as u32, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f..=0x9f));
        return Ok(if latin && control {
            DbCell::Binary(STANDARD.encode(value.chars().map(|c| c as u8).collect::<Vec<_>>()))
        } else {
            DbCell::Text(value)
        });
    }
    if text.starts_with(|c: char| c == '-' || c.is_ascii_digit()) {
        return Ok(if text.contains(['.', 'e', 'E']) {
            DbCell::Float(text.to_owned())
        } else {
            DbCell::Int(text.to_owned())
        });
    }
    match text {
        "true" => Ok(DbCell::Int("1".into())),
        "false" => Ok(DbCell::Int("0".into())),
        _ => Ok(DbCell::Json(text.to_owned())),
    }
}

/// Parses `sqlite3 -json` output: each result is an array with one row per
/// line (`[{…},` … `{…}]`). A result cut short by the output limit keeps its
/// complete lines.
pub fn parse_json(output: &[u8]) -> Result<Vec<CliResult>, DbError> {
    let text = std::str::from_utf8(output).map_err(|_| DbError::QueryFailed)?;
    let mut results: Vec<CliResult> = Vec::new();
    for line in text.lines() {
        let mut line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            results.push(CliResult::default());
            line = rest.trim_start();
            if line == "]" || line.is_empty() {
                continue;
            }
        }
        let body = line
            .strip_suffix(',')
            .or_else(|| line.strip_suffix(']'))
            .unwrap_or(line)
            .trim_end();
        let Ok(row) = serde_json::from_str::<OrderedRow>(body) else {
            // A line cut by the output limit ends the parse.
            break;
        };
        let result = results.last_mut().ok_or(DbError::QueryFailed)?;
        if result.columns.is_empty() {
            result.columns = row.0.iter().map(|(name, _)| name.clone()).collect();
        }
        if row.0.len() != result.columns.len() {
            return Err(DbError::QueryFailed);
        }
        result.rows.push(
            row.0
                .iter()
                .map(|(_, value)| json_cell(value))
                .collect::<Result<_, _>>()?,
        );
    }
    Ok(results)
}

/// Parses `sqlite3 -csv -header -nullvalue <null>` output of one statement:
/// the header, then rows. Values are text (CSV keeps no types) except the
/// NULL sentinel. `changes_header` starts a second result (the row count of
/// a write).
pub fn parse_csv(
    output: &[u8],
    null: &str,
    changes_header: Option<&str>,
) -> Result<Vec<CliResult>, DbError> {
    let text = std::str::from_utf8(output).map_err(|_| DbError::QueryFailed)?;
    let mut records: Vec<Vec<(String, bool)>> = Vec::new();
    let mut record: Vec<(String, bool)> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    let mut pending = false;
    while let Some(c) = chars.next() {
        pending = true;
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                in_quotes = true;
                quoted = true;
            }
            ',' => record.push((std::mem::take(&mut field), std::mem::take(&mut quoted))),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                record.push((std::mem::take(&mut field), std::mem::take(&mut quoted)));
                records.push(std::mem::take(&mut record));
                pending = false;
            }
            _ => field.push(c),
        }
    }
    if pending && !in_quotes {
        record.push((field, quoted));
        records.push(record);
    }
    let mut results: Vec<CliResult> = Vec::new();
    for record in records {
        let is_changes = changes_header
            .is_some_and(|header| record.len() == 1 && record[0].0 == header && !record[0].1);
        match results.last_mut() {
            Some(result) if !is_changes && !result.columns.is_empty() => {
                if record.len() != result.columns.len() {
                    return Err(DbError::QueryFailed);
                }
                result.rows.push(
                    record
                        .into_iter()
                        .map(|(value, quoted)| {
                            if !quoted && value == null {
                                DbCell::Null
                            } else {
                                DbCell::Text(value)
                            }
                        })
                        .collect(),
                );
            }
            _ => results.push(CliResult {
                columns: record.into_iter().map(|(name, _)| name).collect(),
                rows: Vec::new(),
            }),
        }
    }
    Ok(results)
}

pub(crate) struct SqliteCli {
    runner: Arc<dyn SqliteCliRunner>,
    path: String,
    /// The connection setting: every statement runs read-only.
    read_only: bool,
    json: bool,
    safe: bool,
    /// Reads open `mode=rw` under `query_only`: a WAL file whose shared
    /// memory file a read-only open cannot create (older programs).
    rw_reads: bool,
    timeout: Duration,
    output_limit: usize,
    /// Distinct per session: NULL in CSV output, the header of `changes()`.
    nonce: String,
    pub version: String,
    pub file_name: String,
}

impl fmt::Debug for SqliteCli {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SqliteCli([SCOPED])")
    }
}

impl SqliteCli {
    /// Checks the program and its version, then that the file is a database.
    pub async fn connect(
        connection: &DbConnection,
        runner: Arc<dyn SqliteCliRunner>,
        output_limit: usize,
        scope: CancellationToken,
    ) -> Result<Self, DbError> {
        let DbOptions::Sqlite { read_only } = connection.options else {
            return Err(DbError::InvalidRequest);
        };
        let probe = runner
            .run(
                vec!["sqlite3".into(), "-version".into()],
                Vec::new(),
                4096,
                scope.child_token(),
            )
            .await?;
        if probe.status != Some(0) {
            return Err(match classify_failure(&probe) {
                DbError::QueryFailed => DbError::SqliteCliMissing,
                other => other,
            });
        }
        let version_text = String::from_utf8_lossy(&probe.stdout).into_owned();
        let version = parse_version(&version_text).ok_or(DbError::SqliteCliMissing)?;
        if version < MIN_VERSION {
            return Err(DbError::SqliteCliMissing);
        }
        let path = connection.database.clone();
        let file_name = path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(&path)
            .to_owned();
        let mut driver = Self {
            runner,
            path,
            read_only,
            json: version >= JSON_VERSION,
            safe: version >= SAFE_VERSION,
            rw_reads: false,
            timeout: Duration::from_secs(u64::from(connection.limits.statement_timeout_seconds)),
            output_limit: output_limit.max(1 << 20),
            nonce: uuid::Uuid::new_v4().simple().to_string(),
            version: format!("{}.{}.{}", version.0, version.1, version.2),
            file_name,
        };
        // A missing file or one that is not a database fails here.
        let probe = "SELECT count(*) FROM sqlite_master";
        let mut sink = crate::session::Collect::default();
        match driver
            .run(probe, &[], true, &mut sink, scope.child_token())
            .await
        {
            Err(DbError::FileMissing) => {
                // A read-only open of a WAL file without its -shm file fails;
                // `mode=rw` never creates the database itself.
                driver.rw_reads = true;
                driver
                    .run(probe, &[], true, &mut sink, scope.child_token())
                    .await?;
            }
            other => other?,
        }
        Ok(driver)
    }

    fn null_value(&self) -> String {
        format!("__cc_null_{}", self.nonce)
    }

    fn changes_header(&self) -> String {
        format!("__cc_changes_{}", self.nonce)
    }

    /// The `sqlite3` argument vector for a read or a write.
    pub(crate) fn argv(&self, writable: bool) -> Vec<String> {
        let mut argv: Vec<String> = vec!["sqlite3".into(), "-batch".into(), "-bail".into()];
        if self.safe {
            argv.push("-safe".into());
        }
        let open_writable = writable || self.rw_reads;
        if !open_writable {
            argv.push("-readonly".into());
        }
        if self.json {
            argv.push("-json".into());
        } else {
            argv.extend([
                "-csv".into(),
                "-header".into(),
                "-nullvalue".into(),
                self.null_value(),
            ]);
        }
        argv.extend([
            "-cmd".into(),
            format!(".timeout {BUSY_MILLISECONDS}"),
            file_uri(&self.path, open_writable),
        ]);
        argv
    }

    /// Standard input for one statement: the read-only switch, the statement
    /// with its values, and for a write the number of changed rows.
    pub(crate) fn script(
        &self,
        sql: &str,
        parameters: &[DbParameter],
        writable: bool,
    ) -> Result<String, DbError> {
        let statement = inline_parameters(sql, parameters)?;
        // Without -safe, a line starting with '.' must never reach the
        // program's command reader (it is SQL here, but refuse to be sure).
        if !self.safe
            && statement
                .lines()
                .skip(1)
                .any(|line| line.starts_with('.') || line.starts_with('#'))
        {
            return Err(DbError::PolicyDenied);
        }
        let mut script = String::new();
        if !writable {
            script.push_str("PRAGMA query_only = 1;\n");
        }
        script.push_str(statement.trim_end().trim_end_matches(';'));
        script.push_str(";\n");
        if writable {
            script.push_str(&format!(
                "SELECT changes() AS \"{}\";\n",
                self.changes_header()
            ));
        }
        Ok(script)
    }

    pub fn query<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [DbParameter],
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move { self.run(sql, bindings, read_only, sink, cancel).await })
    }

    /// Runs statements one after another (the program commits each).
    pub fn query_all<'a>(
        &'a mut self,
        statements: &'a [(&'a str, &'a [DbParameter])],
        read_only: bool,
        sink: &'a mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> futures::future::BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            for (sql, bindings) in statements {
                self.run(sql, bindings, read_only, sink, cancel.clone())
                    .await?;
            }
            Ok(())
        })
    }

    /// Runs one statement and returns the program's output.
    async fn execute(
        &self,
        sql: &str,
        bindings: &[DbParameter],
        writable: bool,
        cancel: CancellationToken,
    ) -> Result<CliOutput, DbError> {
        let script = self.script(sql, bindings, writable)?;
        let run = self.runner.run(
            self.argv(writable),
            script.into_bytes(),
            self.output_limit,
            cancel.clone(),
        );
        let output = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(DbError::Cancelled),
            result = tokio::time::timeout(self.timeout, run) => result.map_err(|_| DbError::Timeout)??,
        };
        if output.status != Some(0) {
            return Err(classify_failure(&output));
        }
        Ok(output)
    }

    /// The raw output of a read (the plan drawing of EXPLAIN QUERY PLAN).
    pub(crate) async fn raw(
        &self,
        sql: &str,
        bindings: &[DbParameter],
        cancel: CancellationToken,
    ) -> Result<Vec<u8>, DbError> {
        Ok(self.execute(sql, bindings, false, cancel).await?.stdout)
    }

    async fn run(
        &self,
        sql: &str,
        bindings: &[DbParameter],
        read_only: bool,
        sink: &mut (dyn RowConsumer + Send),
        cancel: CancellationToken,
    ) -> Result<(), DbError> {
        let writable = !read_only && !self.read_only;
        let started = Instant::now();
        let output = self.execute(sql, bindings, writable, cancel).await?;
        let changes_header = self.changes_header();
        let mut results = if self.json {
            parse_json(&output.stdout)?
        } else {
            parse_csv(&output.stdout, &self.null_value(), Some(&changes_header))?
        };
        let affected = if writable {
            let changes = results.pop().ok_or(DbError::QueryFailed)?;
            if changes.columns != [changes_header.clone()] {
                return Err(DbError::QueryFailed);
            }
            match changes.rows.first().and_then(|row| row.first()) {
                Some(DbCell::Int(value) | DbCell::Text(value)) => value.parse().ok(),
                _ => None,
            }
        } else {
            None
        };
        if results.len() > 1 {
            return Err(DbError::QueryFailed);
        }
        let result = results.pop().unwrap_or_default();
        sink.start(
            result
                .columns
                .iter()
                .map(|name| DbColumn {
                    name: name.clone(),
                    type_name: String::new(),
                    nullable: true,
                    sensitive: crate::sensitive_column(name),
                    primary_key: false,
                })
                .collect(),
            None,
        )
        .await?;
        for row in result.rows {
            if !sink.row(row).await? {
                return Err(DbError::ResultLimit);
            }
        }
        if output.truncated {
            return Err(DbError::ResultLimit);
        }
        sink.statement_done(started.elapsed().as_millis() as u64, affected)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_output_keeps_column_order_duplicates_types_and_blobs() {
        let output = br#"[{"a":1,"b":"x'y\"z\nnl","c":"\u0000\u00ff\u0041","d":1.5,"a":1},
{"a":9223372036854775807,"b":null,"c":null,"d":1.0e+300,"a":1}]
[{"__cc_changes_x":2}]
"#;
        let results = parse_json(output).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].columns, ["a", "b", "c", "d", "a"]);
        assert_eq!(
            results[0].rows[0],
            vec![
                DbCell::Int("1".into()),
                DbCell::Text("x'y\"z\nnl".into()),
                DbCell::Binary(STANDARD.encode([0u8, 0xff, 0x41])),
                DbCell::Float("1.5".into()),
                DbCell::Int("1".into()),
            ]
        );
        assert_eq!(
            results[0].rows[1][0],
            DbCell::Int("9223372036854775807".into())
        );
        assert_eq!(results[0].rows[1][1], DbCell::Null);
        assert_eq!(results[0].rows[1][3], DbCell::Float("1.0e+300".into()));
        assert_eq!(results[1].rows[0][0], DbCell::Int("2".into()));
        // Text with non-Latin characters stays text.
        let text = parse_json("[{\"t\":\"é\\u0001 ж\"}]\n".as_bytes()).unwrap();
        assert_eq!(text[0].rows[0][0], DbCell::Text("é\u{1} ж".into()));
        // A row cut by the output limit is dropped, complete ones stay.
        let cut = parse_json(b"[{\"a\":1},\n{\"a\":2},\n{\"a\":").unwrap();
        assert_eq!(cut[0].rows.len(), 2);
        assert!(parse_json(b"").unwrap().is_empty());
    }

    #[test]
    fn csv_output_keeps_quotes_newlines_and_the_null_sentinel() {
        let output = b"a,b,c\r\n1,\"x\"\"y\r\nz\",NULLX\r\n2,\"NULLX\",\r\n__cc_changes\r\n3\r\n";
        let results = parse_csv(output, "NULLX", Some("__cc_changes")).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].columns, ["a", "b", "c"]);
        assert_eq!(
            results[0].rows[0],
            vec![
                DbCell::Text("1".into()),
                DbCell::Text("x\"y\r\nz".into()),
                DbCell::Null
            ]
        );
        // A quoted sentinel is the text, an empty field is empty text.
        assert_eq!(results[0].rows[1][1], DbCell::Text("NULLX".into()));
        assert_eq!(results[0].rows[1][2], DbCell::Text(String::new()));
        assert_eq!(results[1].columns, ["__cc_changes"]);
        assert_eq!(results[1].rows[0][0], DbCell::Text("3".into()));
    }

    #[test]
    fn literals_follow_sqlite_rules() {
        assert_eq!(sqlite_literal(&DbCell::Null).unwrap(), "NULL");
        assert_eq!(sqlite_literal(&DbCell::Bool(true)).unwrap(), "1");
        assert_eq!(sqlite_literal(&DbCell::Int(" -5 ".into())).unwrap(), "-5");
        assert!(sqlite_literal(&DbCell::Int("1; DROP".into())).is_err());
        assert!(sqlite_literal(&DbCell::Uint("18446744073709551615".into())).is_err());
        assert_eq!(
            sqlite_literal(&DbCell::Float("1e300".into())).unwrap(),
            "1e300"
        );
        assert_eq!(
            sqlite_literal(&DbCell::Float("inf".into())).unwrap(),
            "9e999"
        );
        assert_eq!(
            sqlite_literal(&DbCell::Text("it's \\ fine".into())).unwrap(),
            "'it''s \\ fine'"
        );
        assert_eq!(
            sqlite_literal(&DbCell::Text("a\0b".into())).unwrap(),
            "CAST(X'610062' AS TEXT)"
        );
        assert_eq!(
            sqlite_literal(&DbCell::Binary(STANDARD.encode([0u8, 255]))).unwrap(),
            "X'00FF'"
        );
    }

    #[test]
    fn placeholders_become_literals_and_strings_stay_untouched() {
        let parameters = vec![
            DbParameter {
                name: String::new(),
                value: DbCell::Text("'; DROP TABLE t; --".into()),
                type_hint: None,
            },
            DbParameter {
                name: String::new(),
                value: DbCell::Int("7".into()),
                type_hint: None,
            },
        ];
        assert_eq!(
            inline_parameters("SELECT '?1', ?2 WHERE x = ?1", &parameters).unwrap(),
            "SELECT '?1', 7 WHERE x = '''; DROP TABLE t; --'"
        );
        assert!(inline_parameters("SELECT ?3", &parameters).is_err());
    }

    #[test]
    fn commands_quote_for_the_shell_and_uris_never_create() {
        assert_eq!(
            shell_command(&[
                "sqlite3".into(),
                "-cmd".into(),
                ".timeout 5000".into(),
                "it's".into()
            ]),
            "sqlite3 -cmd '.timeout 5000' 'it'\\''s'"
        );
        assert_eq!(
            file_uri("/data/my db?.sqlite", false),
            "file:///data/my%20db%3f.sqlite?mode=ro"
        );
        assert_eq!(file_uri("app.db", true), "file:app.db?mode=rw");
        assert_eq!(
            parse_version("3.31.1 2020-01-27 19:55:54 abc"),
            Some((3, 31, 1))
        );
        assert_eq!(parse_version("3.8 x"), Some((3, 8, 0)));
        assert_eq!(parse_version("sh: sqlite3: not found"), None);
        let missing = CliOutput {
            stderr: b"sh: 1: sqlite3: not found".to_vec(),
            status: Some(127),
            ..Default::default()
        };
        assert_eq!(classify_failure(&missing), DbError::SqliteCliMissing);
        let docker = CliOutput {
            stderr:
                b"OCI runtime exec failed: exec: \"sqlite3\": executable file not found in $PATH"
                    .to_vec(),
            status: Some(126),
            ..Default::default()
        };
        assert_eq!(classify_failure(&docker), DbError::SqliteCliMissing);
        let not_db = CliOutput {
            stderr: b"Error: file is not a database".to_vec(),
            status: Some(1),
            ..Default::default()
        };
        assert_eq!(classify_failure(&not_db), DbError::NotSqlite);
    }
}
