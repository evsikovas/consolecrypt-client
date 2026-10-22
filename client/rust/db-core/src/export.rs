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
    match cell {
        DbCell::Null => "NULL".into(),
        DbCell::Bool(value) => if *value { "TRUE" } else { "FALSE" }.into(),
        DbCell::Int(v) | DbCell::Uint(v) | DbCell::Decimal(v) | DbCell::Float(v)
            if v.parse::<f64>().is_ok_and(f64::is_finite) =>
        {
            v.clone()
        }
        other => {
            let text = plain(other).unwrap_or_default();
            match engine {
                DbEngine::Mysql | DbEngine::Mariadb | DbEngine::Clickhouse => {
                    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
                }
                _ => format!("'{}'", text.replace('\'', "''")),
            }
        }
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
                    .map(|column| self.delimited(Some(&column.name)))
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
                let cells: Vec<String> =
                    row.iter().map(|cell| self.delimited(plain(cell))).collect();
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
            "INSERT INTO `shop`.`t` (`id`, `name`, `meta`) VALUES (1, 'it\\'s \\\\ ok', NULL);\n"
        );
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
