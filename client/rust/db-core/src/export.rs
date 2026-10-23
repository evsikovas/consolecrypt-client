//! Copy and export of query results: CSV/TSV, JSON, NDJSON, Markdown and SQL
//! `INSERT`. Copies format rows already loaded for the grid; file exports run
//! the read-only query again and stream every row to disk without the grid limit.
use crate::{
    introspect::quote_identifier, session::DbSession, streaming::RowConsumer, DbCell, DbColumn,
    DbError, DbObjectRef, DbQueryRequest,
};
use async_trait::async_trait;
use cc_models::database::DbEngine;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Tsv,
    Json,
    Ndjson,
    Markdown,
    Insert,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportOptions {
    pub format: ExportFormat,
    #[serde(default = "yes")]
    pub header: bool,
    #[serde(default)]
    pub bom: bool,
    #[serde(default = "comma")]
    pub delimiter: String,
    #[serde(default = "double_quote")]
    pub quote: String,
    /// CSV/TSV: text cells a spreadsheet would run as a formula (`=`, `+`,
    /// `-`, `@`, TAB, CR first) get a leading `'`. Numeric cells never do.
    #[serde(default = "yes")]
    pub escape_formulas: bool,
}

fn yes() -> bool {
    true
}
fn comma() -> String {
    ",".into()
}
fn double_quote() -> String {
    "\"".into()
}

impl ExportOptions {
    fn validate(&self) -> Result<(char, char), DbError> {
        let single = |value: &str| {
            let mut chars = value.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c != '\n' && c != '\r' => Ok(c),
                _ => Err(DbError::InvalidRequest),
            }
        };
        let delimiter = if self.format == ExportFormat::Tsv {
            '\t'
        } else {
            single(&self.delimiter)?
        };
        let quote = single(&self.quote)?;
        if delimiter == quote {
            return Err(DbError::InvalidRequest);
        }
        Ok((delimiter, quote))
    }
}

/// Text of one cell as it appears in delimited and Markdown output.
fn plain(cell: &DbCell) -> Option<&str> {
    match cell {
        DbCell::Null => None,
        DbCell::Bool(true) => Some("true"),
        DbCell::Bool(false) => Some("false"),
        DbCell::Int(v)
        | DbCell::Uint(v)
        | DbCell::Decimal(v)
        | DbCell::Float(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v)
        | DbCell::Binary(v)
        | DbCell::Json(v) => Some(v),
    }
}

/// Would a spreadsheet treat `value` as a formula (OWASP CSV injection)?
fn formula_like(value: &str) -> bool {
    value.starts_with(['=', '+', '-', '@', '\t', '\r'])
}

/// Text cells only: a genuinely numeric `-5` stays a number.
fn textual(cell: &DbCell) -> bool {
    !matches!(
        cell,
        DbCell::Null | DbCell::Bool(_) | DbCell::Int(_) | DbCell::Uint(_) | DbCell::Decimal(_)
    ) && !matches!(cell, DbCell::Float(v) if v.parse::<f64>().is_ok())
}

fn json_value(cell: &DbCell) -> serde_json::Value {
    use serde_json::Value;
    match cell {
        DbCell::Null => Value::Null,
        DbCell::Bool(value) => Value::Bool(*value),
        DbCell::Int(v) | DbCell::Uint(v) => v
            .parse::<i64>()
            .map(Value::from)
            .or_else(|_| v.parse::<u64>().map(Value::from))
            .unwrap_or_else(|_| Value::String(v.clone())),
        DbCell::Float(v) => v
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| Value::String(v.clone()), Value::Number),
        DbCell::Json(v) => serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.clone())),
        DbCell::Decimal(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v)
        | DbCell::Binary(v) => Value::String(v.clone()),
    }
}

pub(crate) fn sql_literal(engine: DbEngine, cell: &DbCell) -> String {
    if engine == DbEngine::Sqlite {
        // INTEGER for booleans, X'..' for blobs, '' doubling; never backslashes.
        return crate::sqlite_cli::sqlite_literal(cell).unwrap_or_else(|_| match cell {
            DbCell::Int(v) | DbCell::Uint(v) | DbCell::Decimal(v) => {
                format!("'{}'", v.replace('\'', "''"))
            }
            _ => "NULL".into(),
        });
    }
    match cell {
        DbCell::Null => "NULL".into(),
        DbCell::Bool(value) => if *value { "TRUE" } else { "FALSE" }.into(),
        DbCell::Int(v) | DbCell::Uint(v) | DbCell::Decimal(v) | DbCell::Float(v)
            if v.parse::<f64>().is_ok_and(f64::is_finite) =>
        {
            v.clone()
        }
        other => string_literal(engine, plain(other).unwrap_or_default()),
    }
}

/// A string literal that reads back as `text` whatever the session's escape
/// mode. Quotes are always doubled (standard SQL, never `\'`):
///  * MySQL/MariaDB/ClickHouse: backslashes are escapes (unless MySQL's
///    `NO_BACKSLASH_ESCAPES`, where the doubled backslash only adds a
///    character and still cannot end the literal);
///  * PostgreSQL: a plain literal, or an `E'…'` literal when backslashes are
///    present, which reads the same with or without
///    `standard_conforming_strings`.
fn string_literal(engine: DbEngine, text: &str) -> String {
    let quoted = text.replace('\'', "''");
    match engine {
        DbEngine::Mysql | DbEngine::Mariadb | DbEngine::Clickhouse => {
            format!("'{}'", quoted.replace('\\', "\\\\"))
        }
        _ if text.contains('\\') => format!("E'{}'", quoted.replace('\\', "\\\\")),
        _ => format!("'{quoted}'"),
    }
}

/// Formats rows one by one so copies and streamed files share the rules.
struct Formatter {
    options: ExportOptions,
    delimiter: char,
    quote: char,
    engine: DbEngine,
    columns: Vec<DbColumn>,
    table: String,
    rows: u64,
}

impl Formatter {
    fn new(
        options: ExportOptions,
        engine: DbEngine,
        columns: Vec<DbColumn>,
        table: Option<&DbObjectRef>,
    ) -> Result<Self, DbError> {
        let (delimiter, quote) = options.validate()?;
        let table = match table {
            Some(table) => quote_identifier(engine, table),
            None => "exported_rows".into(),
        };
        Ok(Self {
            options,
            delimiter,
            quote,
            engine,
            columns,
            table,
            rows: 0,
        })
    }

    /// A delimited text field; `formula` marks text a spreadsheet could run.
    fn field(&self, value: &str, formula: bool) -> String {
        if formula && self.options.escape_formulas && formula_like(value) {
            self.delimited(Some(&format!("'{value}")))
        } else {
            self.delimited(Some(value))
        }
    }

    fn cell(&self, cell: &DbCell) -> String {
        match plain(cell) {
            Some(value) => self.field(value, textual(cell)),
            None => String::new(),
        }
    }

    fn delimited(&self, value: Option<&str>) -> String {
        let Some(value) = value else {
            return String::new();
        };
        let needs = value.contains(self.delimiter)
            || value.contains(self.quote)
            || value.contains(['\n', '\r'])
            || value.starts_with(' ')
            || value.ends_with(' ');
        if needs {
            let doubled = format!("{}{}", self.quote, self.quote);
            format!(
                "{q}{}{q}",
                value.replace(self.quote, &doubled),
                q = self.quote
            )
        } else {
            value.to_owned()
        }
    }

    fn markdown(value: Option<&str>) -> String {
        value
            .unwrap_or("NULL")
            .replace('\\', "\\\\")
            .replace('|', "\\|")
            .replace(['\n', '\r'], " ")
    }

    fn head(&self) -> String {
        let mut out = String::new();
        if self.options.bom && matches!(self.options.format, ExportFormat::Csv | ExportFormat::Tsv)
        {
            out.push('\u{feff}');
        }
        match self.options.format {
            ExportFormat::Csv | ExportFormat::Tsv if self.options.header => {
                let names: Vec<String> = self
                    .columns
                    .iter()
                    .map(|column| self.field(&column.name, true))
                    .collect();
                out.push_str(&names.join(&self.delimiter.to_string()));
                out.push_str("\r\n");
            }
            ExportFormat::Markdown => {
                let names: Vec<String> = self
                    .columns
                    .iter()
                    .map(|column| Self::markdown(Some(&column.name)))
                    .collect();
                out.push_str(&format!("| {} |\n", names.join(" | ")));
                out.push_str(&format!(
                    "|{}\n",
                    self.columns.iter().map(|_| " --- |").collect::<String>()
                ));
            }
            ExportFormat::Json => out.push('['),
            _ => {}
        }
        out
    }

    fn row(&mut self, row: &[DbCell]) -> String {
        let first = self.rows == 0;
        self.rows += 1;
        match self.options.format {
            ExportFormat::Csv | ExportFormat::Tsv => {
                let cells: Vec<String> = row.iter().map(|cell| self.cell(cell)).collect();
                format!("{}\r\n", cells.join(&self.delimiter.to_string()))
            }
            ExportFormat::Markdown => format!(
                "| {} |\n",
                row.iter()
                    .map(|cell| Self::markdown(plain(cell)))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
            ExportFormat::Json | ExportFormat::Ndjson => {
                // Keys keep the column order (a JSON map would sort them).
                let fields: Vec<String> = self
                    .columns
                    .iter()
                    .zip(row)
                    .map(|(column, cell)| {
                        format!(
                            "{}:{}",
                            serde_json::Value::String(column.name.clone()),
                            json_value(cell)
                        )
                    })
                    .collect();
                let text = format!("{{{}}}", fields.join(","));
                match (self.options.format, first) {
                    (ExportFormat::Ndjson, _) => format!("{text}\n"),
                    (_, true) => format!("\n  {text}"),
                    _ => format!(",\n  {text}"),
                }
            }
            ExportFormat::Insert => {
                let names: Vec<String> = self
                    .columns
                    .iter()
                    .map(|column| {
                        let reference = DbObjectRef {
                            database: String::new(),
                            schema: None,
                            kind: crate::DbObjectKind::Column,
                            name: column.name.clone(),
                            parent_name: None,
                        };
                        quote_identifier(self.engine, &reference)
                    })
                    .collect();
                format!(
                    "INSERT INTO {} ({}) VALUES ({});\n",
                    self.table,
                    names.join(", "),
                    row.iter()
                        .map(|cell| sql_literal(self.engine, cell))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }

    fn tail(&self) -> String {
        match self.options.format {
            ExportFormat::Json if self.rows == 0 => "]\n".into(),
            ExportFormat::Json => "\n]\n".into(),
            _ => String::new(),
        }
    }
}

/// Formats loaded rows for the clipboard.
pub fn format_rows(
    engine: DbEngine,
    columns: &[DbColumn],
    rows: &[Vec<DbCell>],
    table: Option<&DbObjectRef>,
    options: ExportOptions,
) -> Result<String, DbError> {
    let mut formatter = Formatter::new(options, engine, columns.to_vec(), table)?;
    let mut out = formatter.head();
    for row in rows {
        out.push_str(&formatter.row(row));
    }
    out.push_str(&formatter.tail());
    Ok(out)
}

/// Shared progress of one file export.
#[derive(Debug, Default)]
pub struct ExportProgress {
    pub rows: AtomicU64,
    pub bytes: AtomicU64,
    pub done: AtomicBool,
    pub cancelled: AtomicBool,
    pub error: Mutex<Option<&'static str>>,
}

impl ExportProgress {
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "rows": self.rows.load(Ordering::Relaxed),
            "bytes": self.bytes.load(Ordering::Relaxed),
            "done": self.done.load(Ordering::Acquire),
            "cancelled": self.cancelled.load(Ordering::Acquire),
            "error": *self.error.lock().unwrap_or_else(|e| e.into_inner()),
        })
    }
}

struct FileSink {
    writer: BufWriter<tokio::fs::File>,
    options: ExportOptions,
    engine: DbEngine,
    wanted: u32,
    index: u32,
    formatter: Option<Formatter>,
    progress: Arc<ExportProgress>,
    cancel: CancellationToken,
}

impl FileSink {
    async fn write(&mut self, text: String) -> Result<(), DbError> {
        if self.cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        self.writer
            .write_all(text.as_bytes())
            .await
            .map_err(|_| DbError::QueryFailed)?;
        self.progress
            .bytes
            .fetch_add(text.len() as u64, Ordering::Relaxed);
        Ok(())
    }
}

#[async_trait]
impl RowConsumer for FileSink {
    async fn start(
        &mut self,
        columns: Vec<DbColumn>,
        editable_table: Option<DbObjectRef>,
    ) -> Result<(), DbError> {
        if self.index == self.wanted {
            let formatter = Formatter::new(
                self.options.clone(),
                self.engine,
                columns,
                editable_table.as_ref(),
            )?;
            let head = formatter.head();
            self.formatter = Some(formatter);
            self.write(head).await?;
        }
        Ok(())
    }

    async fn row(&mut self, row: Vec<DbCell>) -> Result<bool, DbError> {
        let Some(formatter) = self
            .formatter
            .as_mut()
            .filter(|_| self.index == self.wanted)
        else {
            return Ok(true);
        };
        let text = formatter.row(&row);
        self.write(text).await?;
        self.progress.rows.fetch_add(1, Ordering::Relaxed);
        Ok(true)
    }

    async fn statement_done(&mut self, _: u64, _: Option<u64>) -> Result<(), DbError> {
        if self.index == self.wanted {
            if let Some(formatter) = &self.formatter {
                let tail = formatter.tail();
                self.write(tail).await?;
            }
        }
        self.index = self.index.saturating_add(1);
        Ok(())
    }
}

impl DbSession {
    /// Runs a read-only request again and streams result `result_index` into
    /// `path`. Writes are never re-executed for an export.
    pub async fn export(
        self: &Arc<Self>,
        request: &DbQueryRequest,
        result_index: u32,
        options: ExportOptions,
        path: PathBuf,
        progress: Arc<ExportProgress>,
        cancel: CancellationToken,
    ) -> Result<(), DbError> {
        options.validate()?;
        let statements = self.guard.prepare(request)?;
        if statements.iter().any(|statement| !statement.is_read_only()) {
            return Err(DbError::PolicyDenied);
        }
        let file = tokio::fs::File::create(&path)
            .await
            .map_err(|_| DbError::InvalidRequest)?;
        let mut sink = FileSink {
            writer: BufWriter::new(file),
            options,
            engine: self.info.engine,
            wanted: result_index,
            index: 0,
            formatter: None,
            progress,
            cancel: cancel.clone(),
        };
        let mut driver = self.driver.lock().await;
        for statement in &statements {
            self.guard.validate_permit(statement)?;
            driver
                .run(statement.parsed(), true, &mut sink, cancel.clone())
                .await?;
        }
        drop(driver);
        sink.writer
            .flush()
            .await
            .map_err(|_| DbError::QueryFailed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> Vec<DbColumn> {
        ["id", "name", "meta"]
            .into_iter()
            .map(|name| DbColumn {
                name: name.into(),
                type_name: String::new(),
                nullable: true,
                sensitive: false,
                primary_key: false,
            })
            .collect()
    }

    fn rows() -> Vec<Vec<DbCell>> {
        vec![
            vec![
                DbCell::Int("1".into()),
                DbCell::Text("a,\"b\"".into()),
                DbCell::Json("{\"k\":1}".into()),
            ],
            vec![
                DbCell::Int("2".into()),
                DbCell::Null,
                DbCell::Text("x|y".into()),
            ],
        ]
    }

    fn options(format: ExportFormat) -> ExportOptions {
        ExportOptions {
            format,
            header: true,
            bom: false,
            delimiter: ",".into(),
            quote: "\"".into(),
            escape_formulas: true,
        }
    }

    #[test]
    fn formats_every_kind() {
        let format = |kind| {
            format_rows(
                DbEngine::Postgresql,
                &columns(),
                &rows(),
                None,
                options(kind),
            )
            .unwrap()
        };
        assert_eq!(
            format(ExportFormat::Csv),
            "id,name,meta\r\n1,\"a,\"\"b\"\"\",\"{\"\"k\"\":1}\"\r\n2,,x|y\r\n"
        );
        assert_eq!(
            format(ExportFormat::Tsv),
            "id\tname\tmeta\r\n1\t\"a,\"\"b\"\"\"\t\"{\"\"k\"\":1}\"\r\n2\t\tx|y\r\n"
        );
        assert_eq!(
            format(ExportFormat::Json),
            "[\n  {\"id\":1,\"name\":\"a,\\\"b\\\"\",\"meta\":{\"k\":1}},\n  {\"id\":2,\"name\":null,\"meta\":\"x|y\"}\n]\n"
        );
        assert_eq!(format(ExportFormat::Ndjson).lines().count(), 2);
        assert_eq!(
            format(ExportFormat::Markdown),
            "| id | name | meta |\n| --- | --- | --- |\n| 1 | a,\"b\" | {\"k\":1} |\n| 2 | NULL | x\\|y |\n"
        );
        assert_eq!(
            format(ExportFormat::Insert),
            "INSERT INTO exported_rows (\"id\", \"name\", \"meta\") VALUES (1, 'a,\"b\"', '{\"k\":1}');\nINSERT INTO exported_rows (\"id\", \"name\", \"meta\") VALUES (2, NULL, 'x|y');\n"
        );
    }

    #[test]
    fn mysql_literals_escape_backslashes() {
        let rows = vec![vec![
            DbCell::Int("1".into()),
            DbCell::Text("it's \\ ok".into()),
            DbCell::Null,
        ]];
        let table = DbObjectRef {
            database: "shop".into(),
            schema: None,
            kind: crate::DbObjectKind::Table,
            name: "t".into(),
            parent_name: None,
        };
        assert_eq!(
            format_rows(
                DbEngine::Mysql,
                &columns(),
                &rows,
                Some(&table),
                options(ExportFormat::Insert)
            )
            .unwrap(),
            "INSERT INTO `shop`.`t` (`id`, `name`, `meta`) VALUES (1, 'it''s \\\\ ok', NULL);\n"
        );
    }

    #[test]
    fn string_literals_survive_every_escape_mode() {
        let tricky = [
            ("plain", "'plain'", "'plain'", "'plain'"),
            ("it's", "'it''s'", "'it''s'", "'it''s'"),
            (
                "\\'; DROP TABLE t; --",
                "'\\\\''; DROP TABLE t; --'",
                "E'\\\\''; DROP TABLE t; --'",
                "'\\\\''; DROP TABLE t; --'",
            ),
            ("end\\", "'end\\\\'", "E'end\\\\'", "'end\\\\'"),
            ("''", "''''''", "''''''", "''''''"),
            (
                "line\nbreak",
                "'line\nbreak'",
                "'line\nbreak'",
                "'line\nbreak'",
            ),
        ];
        for (text, mysql, postgres, clickhouse) in tricky {
            assert_eq!(string_literal(DbEngine::Mysql, text), mysql, "{text}");
            assert_eq!(string_literal(DbEngine::Mariadb, text), mysql, "{text}");
            assert_eq!(
                string_literal(DbEngine::Postgresql, text),
                postgres,
                "{text}"
            );
            assert_eq!(
                string_literal(DbEngine::Clickhouse, text),
                clickhouse,
                "{text}"
            );
        }
        // Never the backslash-quote form, which NO_BACKSLASH_ESCAPES reads as
        // the end of the literal.
        for engine in DbEngine::ALL {
            assert!(!string_literal(engine, "a'b\\'c").contains("\\'c"));
        }
        // Numbers stay bare; non-numeric "numbers" become literals.
        assert_eq!(
            sql_literal(DbEngine::Mysql, &DbCell::Int("-5".into())),
            "-5"
        );
        assert_eq!(
            sql_literal(DbEngine::Mysql, &DbCell::Decimal("1); DROP".into())),
            "'1); DROP'"
        );
        assert_eq!(
            sql_literal(DbEngine::Postgresql, &DbCell::Float("NaN".into())),
            "'NaN'"
        );
    }

    #[test]
    fn csv_escapes_formula_text_but_not_numbers() {
        let columns: Vec<DbColumn> = ["=HEADER", "n", "t"]
            .into_iter()
            .map(|name| DbColumn {
                name: name.into(),
                type_name: String::new(),
                nullable: true,
                sensitive: false,
                primary_key: false,
            })
            .collect();
        let rows = vec![
            vec![
                DbCell::Text("=1+1".into()),
                DbCell::Int("-5".into()),
                DbCell::Text("+7".into()),
            ],
            vec![
                DbCell::Text("@SUM(A1)".into()),
                DbCell::Float("-1.5".into()),
                DbCell::Text("-2".into()),
            ],
            vec![
                DbCell::Text("\tcmd".into()),
                DbCell::Decimal("-0.25".into()),
                DbCell::Text("\r=x".into()),
            ],
            vec![
                DbCell::Json("=\"a,b\"".into()),
                DbCell::Null,
                DbCell::Text("safe = text".into()),
            ],
        ];
        let csv = format_rows(
            DbEngine::Postgresql,
            &columns,
            &rows,
            None,
            options(ExportFormat::Csv),
        )
        .unwrap();
        assert_eq!(
            csv,
            "'=HEADER,n,t\r\n\
             '=1+1,-5,'+7\r\n\
             '@SUM(A1),-1.5,'-2\r\n\
             '\tcmd,-0.25,\"'\r=x\"\r\n\
             \"'=\"\"a,b\"\"\",,safe = text\r\n"
        );
        let tsv = format_rows(
            DbEngine::Postgresql,
            &columns,
            &rows[..1],
            None,
            options(ExportFormat::Tsv),
        )
        .unwrap();
        assert_eq!(tsv, "'=HEADER\tn\tt\r\n'=1+1\t-5\t'+7\r\n");
        let mut raw = options(ExportFormat::Csv);
        raw.escape_formulas = false;
        let csv = format_rows(DbEngine::Postgresql, &columns, &rows[..1], None, raw).unwrap();
        assert_eq!(csv, "=HEADER,n,t\r\n=1+1,-5,+7\r\n");
        // Older callers that do not send the option get escaping.
        let parsed: ExportOptions = serde_json::from_str(r#"{"format":"csv"}"#).unwrap();
        assert!(parsed.escape_formulas);
    }

    #[test]
    fn rejects_ambiguous_delimiters_and_adds_bom() {
        let mut bad = options(ExportFormat::Csv);
        bad.delimiter = "\"".into();
        assert!(format_rows(DbEngine::Postgresql, &columns(), &rows(), None, bad).is_err());
        let mut bom = options(ExportFormat::Csv);
        bom.bom = true;
        assert!(
            format_rows(DbEngine::Postgresql, &columns(), &[], None, bom)
                .unwrap()
                .starts_with('\u{feff}')
        );
    }
}
