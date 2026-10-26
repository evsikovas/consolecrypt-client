//! CSV import into a table. The file is read as a stream (never whole),
//! decoded (UTF-8 with or without BOM, Windows-1251, Latin-1), split with the
//! `csv` crate, converted to typed cells per target column and written with
//! parameterized multi-row `INSERT` statements in batches, inside one
//! transaction or one per batch. Every batch goes through the session guard
//! like any other write. File content only ever travels to the target
//! database; values that look like spreadsheet formulas are imported as they
//! are (escaping them is an export concern).
use crate::{
    introspect::quote_identifier, row_changes::DbEditColumn, streaming::RowConsumer, DbCell,
    DbColumn, DbError, DbObjectKind, DbObjectRef, DbParameter, DbQueryRequest, DbSession,
};
use async_trait::async_trait;
use cc_models::database::DbEngine;
use encoding_rs::{Decoder, Encoding, UTF_8, WINDOWS_1251, WINDOWS_1252};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tokio_util::sync::CancellationToken;

/// Largest file the import reads.
pub const MAX_IMPORT_BYTES: u64 = 200 * 1024 * 1024;
/// Largest field and record, in bytes after decoding.
pub const MAX_FIELD_BYTES: usize = 1024 * 1024;
pub const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
/// Most columns a file may have.
pub const MAX_COLUMNS: usize = 1024;
/// Rows shown in the preview.
pub const PREVIEW_ROWS: usize = 100;
/// Rows checked against the target types before the import.
pub const CHECK_ROWS: usize = 1000;
/// Bound values per statement (both engines allow 65,535).
const MAX_PARAMETERS: usize = 60_000;
/// Rows per batch the user may choose.
pub const MAX_BATCH_ROWS: u32 = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CsvEncoding {
    /// UTF-8 (a BOM is skipped); Windows-1251 when the start is not UTF-8.
    #[default]
    Auto,
    Utf8,
    Windows1251,
    Latin1,
}

/// What a field holds when it means NULL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CsvNull {
    /// An empty field.
    #[default]
    Empty,
    /// The word `NULL` (any case).
    Word,
    /// `\N`.
    Backslash,
    /// Nothing is NULL.
    Never,
}

/// Order of day, month and year in dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CsvDateOrder {
    /// `2026-10-22` (ISO 8601).
    #[default]
    Iso,
    /// `22.10.2026`, `22/10/2026`.
    Dmy,
    /// `10/22/2026`.
    Mdy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CsvOptions {
    pub encoding: CsvEncoding,
    /// `None` detects `,`, `;`, TAB or `|`.
    pub delimiter: Option<char>,
    pub quote: char,
    pub header: bool,
    /// Lines skipped before the header (or the first row).
    pub skip_rows: u32,
    pub null: CsvNull,
    /// Numbers use `,` as the decimal separator (`1,5`).
    pub decimal_comma: bool,
    pub date_order: CsvDateOrder,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            encoding: CsvEncoding::Auto,
            delimiter: None,
            quote: '"',
            header: true,
            skip_rows: 0,
            null: CsvNull::Empty,
            decimal_comma: false,
            date_order: CsvDateOrder::Iso,
        }
    }
}

impl CsvOptions {
    fn validate(&self) -> Result<(), DbError> {
        let ok = |c: char| c.is_ascii() && !c.is_ascii_alphanumeric() && c != '\n' && c != '\r';
        if !ok(self.quote)
            || self.delimiter.is_some_and(|d| !ok(d) || d == self.quote)
            || self.skip_rows > 1000
        {
            return Err(DbError::InvalidRequest);
        }
        Ok(())
    }
}

/// The first rows of a file as the options split them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CsvPreview {
    pub delimiter: char,
    pub encoding: CsvEncoding,
    /// Header names, or `column1`, `column2`… without a header row.
    pub columns: Vec<String>,
    /// Up to [`PREVIEW_ROWS`] rows; `None` is NULL.
    pub rows: Vec<Vec<Option<String>>>,
    pub file_bytes: u64,
}

/// Where a file column goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvMapping {
    /// Column index in the file.
    pub source: usize,
    /// Target column name.
    pub target: String,
}

/// Values of a target column that do not convert, from the checked rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CsvColumnCheck {
    pub target: String,
    pub kind: String,
    pub bad: u64,
    /// Up to three bad values, shortened.
    pub samples: Vec<String>,
    /// A required column gets NULL (or is not mapped at all).
    pub missing_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CsvInsertMode {
    #[default]
    Insert,
    /// Rows whose key exists are skipped.
    SkipExisting,
    /// Rows whose key exists are updated.
    Upsert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CsvTransaction {
    /// All or nothing.
    #[default]
    Whole,
    /// Each batch commits on its own.
    PerBatch,
}

/// Everything the import needs; reviewed before it runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvImportPlan {
    pub table: DbObjectRef,
    pub options: CsvOptions,
    pub mapping: Vec<CsvMapping>,
    #[serde(default)]
    pub mode: CsvInsertMode,
    #[serde(default = "default_batch")]
    pub batch_rows: u32,
    /// Bad rows are skipped (and reported) instead of stopping the import.
    #[serde(default)]
    pub skip_bad_rows: bool,
    #[serde(default)]
    pub transaction: CsvTransaction,
    /// Delete every row of the table first (in the import transaction).
    #[serde(default)]
    pub truncate: bool,
}

fn default_batch() -> u32 {
    500
}

/// Live counters of a running import.
#[derive(Debug, Default)]
pub struct CsvImportProgress {
    pub bytes_read: AtomicU64,
    pub total_bytes: AtomicU64,
    pub rows_read: AtomicU64,
    pub imported: AtomicU64,
    pub skipped: AtomicU64,
    pub failed: AtomicU64,
    pub done: AtomicBool,
    pub cancelled: AtomicBool,
    pub error: Mutex<Option<String>>,
    pub report: Mutex<Option<String>>,
}

impl CsvImportProgress {
    pub fn snapshot(&self) -> serde_json::Value {
        let n = |value: &AtomicU64| value.load(Ordering::Acquire);
        serde_json::json!({
            "bytes_read": n(&self.bytes_read),
            "total_bytes": n(&self.total_bytes),
            "rows_read": n(&self.rows_read),
            "imported": n(&self.imported),
            "skipped": n(&self.skipped),
            "failed": n(&self.failed),
            "done": self.done.load(Ordering::Acquire),
            "cancelled": self.cancelled.load(Ordering::Acquire),
            "error": self.error.lock().map(|e| e.clone()).unwrap_or_default(),
            "report": self.report.lock().map(|r| r.clone()).unwrap_or_default(),
        })
    }
}

// ---- reading -------------------------------------------------------------

/// Decodes a byte stream to UTF-8 as it is read and counts the input bytes.
struct Decoding<R> {
    inner: R,
    decoder: Option<Decoder>,
    input: Vec<u8>,
    output: Vec<u8>,
    position: usize,
    finished: bool,
    read: std::sync::Arc<AtomicU64>,
}

impl<R: Read> Read for Decoding<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.position < self.output.len() {
                let n = buffer.len().min(self.output.len() - self.position);
                buffer[..n].copy_from_slice(&self.output[self.position..self.position + n]);
                self.position += n;
                return Ok(n);
            }
            if self.finished {
                return Ok(0);
            }
            self.input.resize(64 * 1024, 0);
            let n = self.inner.read(&mut self.input)?;
            self.read.fetch_add(n as u64, Ordering::Relaxed);
            let last = n == 0;
            match &mut self.decoder {
                None => {
                    // UTF-8 passes through; the csv reader checks each field.
                    self.output.clear();
                    self.output.extend_from_slice(&self.input[..n]);
                }
                Some(decoder) => {
                    self.output.clear();
                    let capacity = decoder
                        .max_utf8_buffer_length(n)
                        .ok_or_else(|| io::Error::other("decode"))?;
                    let mut text = String::with_capacity(capacity);
                    let (_, _, _) = decoder.decode_to_string(&self.input[..n], &mut text, last);
                    self.output.extend_from_slice(text.as_bytes());
                }
            }
            self.position = 0;
            self.finished = last;
        }
    }
}

/// Opens `path`, checks its size and picks the encoding (a UTF-8 BOM is
/// skipped). Returns the decoding reader and the effective encoding.
fn open(
    path: &Path,
    encoding: CsvEncoding,
    read: std::sync::Arc<AtomicU64>,
) -> Result<(Decoding<BufReader<File>>, CsvEncoding, u64), DbError> {
    let file = File::open(path).map_err(|_| DbError::InvalidRequest)?;
    let size = file.metadata().map_err(|_| DbError::InvalidRequest)?.len();
    if size > MAX_IMPORT_BYTES {
        return Err(DbError::ResultLimit);
    }
    let mut reader = BufReader::new(file);
    let mut head = vec![0u8; 64 * 1024];
    let mut filled = 0;
    while filled < head.len() {
        let n = reader
            .read(&mut head[filled..])
            .map_err(|_| DbError::InvalidRequest)?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    head.truncate(filled);
    let bom = head.starts_with(&[0xEF, 0xBB, 0xBF]);
    let effective = match encoding {
        CsvEncoding::Auto if bom => CsvEncoding::Utf8,
        CsvEncoding::Auto => {
            // A cut multi-byte sequence at the end of the sample is fine.
            match std::str::from_utf8(&head) {
                Ok(_) => CsvEncoding::Utf8,
                Err(error) if error.error_len().is_none() => CsvEncoding::Utf8,
                Err(_) => CsvEncoding::Windows1251,
            }
        }
        other => other,
    };
    let encoding: &'static Encoding = match effective {
        CsvEncoding::Windows1251 => WINDOWS_1251,
        CsvEncoding::Latin1 => WINDOWS_1252,
        _ => UTF_8,
    };
    let skip = if bom && effective == CsvEncoding::Utf8 {
        3
    } else {
        0
    };
    read.fetch_add(filled as u64, Ordering::Relaxed);
    let decoder = (encoding != UTF_8).then(|| encoding.new_decoder_without_bom_handling());
    let mut decoding = Decoding {
        inner: reader,
        decoder,
        input: Vec::new(),
        output: Vec::new(),
        position: 0,
        finished: false,
        read,
    };
    // The sample is decoded like the rest of the stream.
    match &mut decoding.decoder {
        None => decoding.output = head[skip..].to_vec(),
        Some(decoder) => {
            let mut text = String::with_capacity(
                decoder
                    .max_utf8_buffer_length(head.len())
                    .ok_or(DbError::InvalidRequest)?,
            );
            let _ = decoder.decode_to_string(&head, &mut text, filled < 64 * 1024);
            decoding.output = text.into_bytes();
            decoding.finished = filled < 64 * 1024;
        }
    }
    if decoding.decoder.is_none() {
        decoding.finished = filled < 64 * 1024;
    }
    Ok((decoding, effective, size))
}

/// The delimiter of the first lines: the candidate found the same number of
/// times (outside quotes) on most lines, preferring more columns.
pub fn detect_delimiter(sample: &str, quote: char) -> char {
    let lines: Vec<&str> = sample.lines().filter(|l| !l.is_empty()).take(20).collect();
    let mut best = (',', 0usize, 0usize);
    for candidate in [',', ';', '\t', '|'] {
        let counts: Vec<usize> = lines
            .iter()
            .map(|line| {
                let mut quoted = false;
                line.chars()
                    .filter(|&c| {
                        if c == quote {
                            quoted = !quoted;
                        }
                        c == candidate && !quoted
                    })
                    .count()
            })
            .collect();
        let Some(&first) = counts.first() else {
            continue;
        };
        if first == 0 {
            continue;
        }
        let agreeing = counts.iter().filter(|&&n| n == first).count();
        if (agreeing, first) > (best.1, best.2) {
            best = (candidate, agreeing, first);
        }
    }
    best.0
}

/// A CSV reader over the decoded file with the options applied: the
/// skipped lines are gone and the header (if any) is read.
struct Source {
    reader: csv::Reader<Decoding<BufReader<File>>>,
    columns: Vec<String>,
    delimiter: char,
    encoding: CsvEncoding,
    size: u64,
    line: u64,
}

fn source(
    path: &Path,
    options: &CsvOptions,
    read: std::sync::Arc<AtomicU64>,
) -> Result<Source, DbError> {
    options.validate()?;
    let (mut decoding, encoding, size) = open(path, options.encoding, read)?;
    // Skip lines before the table (titles, notes) on the decoded text.
    let mut skipped = 0;
    let mut byte = [0u8; 1];
    while skipped < options.skip_rows {
        match decoding.read(&mut byte) {
            Ok(0) => break,
            Ok(_) if byte[0] == b'\n' => skipped += 1,
            Ok(_) => {}
            Err(_) => return Err(DbError::InvalidRequest),
        }
    }
    let delimiter = match options.delimiter {
        Some(delimiter) => delimiter,
        None => {
            let available = decoding.output.len() - decoding.position;
            let sample = String::from_utf8_lossy(
                &decoding.output[decoding.position..decoding.position + available.min(64 * 1024)],
            )
            .into_owned();
            detect_delimiter(&sample, options.quote)
        }
    };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter as u8)
        .quote(options.quote as u8)
        .has_headers(false)
        .flexible(true)
        .double_quote(true)
        .from_reader(decoding);
    let mut line = u64::from(skipped);
    let mut columns = Vec::new();
    if options.header {
        let mut record = csv::StringRecord::new();
        if reader
            .read_record(&mut record)
            .map_err(|_| DbError::InvalidRequest)?
        {
            line += 1;
            columns = record.iter().map(|name| name.trim().to_owned()).collect();
        }
    }
    if columns.len() > MAX_COLUMNS {
        return Err(DbError::ResultLimit);
    }
    Ok(Source {
        reader,
        columns,
        delimiter,
        encoding,
        size,
        line,
    })
}

/// One data record, its line number and the fields (`None` = NULL).
type Record = (u64, Vec<Option<String>>);

impl Source {
    /// The next record; `Err` carries the line of a record that could not
    /// be read (bad encoding, too large).
    fn next(&mut self, options: &CsvOptions) -> Option<Result<Record, u64>> {
        let mut record = csv::ByteRecord::new();
        match self.reader.read_byte_record(&mut record) {
            Ok(false) => None,
            Err(_) => {
                self.line += 1;
                Some(Err(self.line))
            }
            Ok(true) => {
                self.line = record.position().map(|p| p.line()).unwrap_or(self.line + 1)
                    + u64::from(options.skip_rows);
                if record.as_slice().len() > MAX_RECORD_BYTES || record.len() > MAX_COLUMNS {
                    return Some(Err(self.line));
                }
                let mut fields = Vec::with_capacity(record.len());
                for field in record.iter() {
                    if field.len() > MAX_FIELD_BYTES {
                        return Some(Err(self.line));
                    }
                    let Ok(text) = std::str::from_utf8(field) else {
                        return Some(Err(self.line));
                    };
                    fields.push(null_or(text, options.null));
                }
                Some(Ok((self.line, fields)))
            }
        }
    }
}

fn null_or(text: &str, null: CsvNull) -> Option<String> {
    let is_null = match null {
        CsvNull::Empty => text.is_empty(),
        CsvNull::Word => text.eq_ignore_ascii_case("null"),
        CsvNull::Backslash => text == "\\N",
        CsvNull::Never => false,
    };
    (!is_null).then(|| text.to_owned())
}

/// The first rows of the file with the options applied.
pub fn preview(path: &Path, options: &CsvOptions) -> Result<CsvPreview, DbError> {
    let mut source = source(path, options, Default::default())?;
    let mut rows = Vec::new();
    while rows.len() < PREVIEW_ROWS {
        match source.next(options) {
            None => break,
            Some(Ok((_, fields))) => rows.push(fields),
            Some(Err(_)) => return Err(DbError::InvalidRequest),
        }
    }
    let width = rows
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(source.columns.len());
    let mut columns = source.columns.clone();
    for i in columns.len()..width {
        columns.push(format!("column{}", i + 1));
    }
    for (i, name) in columns.iter_mut().enumerate() {
        if name.is_empty() {
            *name = format!("column{}", i + 1);
        }
    }
    Ok(CsvPreview {
        delimiter: source.delimiter,
        encoding: source.encoding,
        columns,
        rows,
        file_bytes: source.size,
    })
}

// ---- conversion ----------------------------------------------------------

/// The value kind a target column takes, from its catalogue type name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CsvKind {
    Int,
    Uint,
    Decimal,
    Float,
    Bool,
    Date,
    Time,
    Timestamp,
    Json,
    Binary,
    Text,
    /// A new MongoDB field: each value as it reads (whole number, number,
    /// true/false, ISO date or timestamp, else text).
    Auto,
}

impl CsvKind {
    pub fn of(type_name: &str) -> Self {
        // ClickHouse wrappers: Nullable(T), LowCardinality(T).
        let mut inner = type_name.trim();
        for wrapper in ["Nullable(", "LowCardinality("] {
            if let Some(rest) = inner
                .strip_prefix(wrapper)
                .and_then(|r| r.strip_suffix(')'))
            {
                inner = rest.trim();
            }
        }
        let lower = inner.to_ascii_lowercase();
        if lower == "auto" {
            return Self::Auto;
        }
        let base = lower.split(['(', ' ']).next().unwrap_or("").trim();
        let unsigned = lower.contains("unsigned");
        if lower.ends_with("[]") || base.starts_with('_') {
            return Self::Text;
        }
        if lower == "tinyint(1)" || base == "boolean" || base == "bool" {
            return Self::Bool;
        }
        match base {
            "uint8" | "uint16" | "uint32" | "uint64" | "uint128" | "uint256" => Self::Uint,
            // MongoDB BSON type names.
            "long" => Self::Int,
            "int16" | "int32" | "int64" | "int128" | "int256" => Self::Int,
            "float32" | "float64" => Self::Float,
            "decimal32" | "decimal64" | "decimal128" | "decimal256" => Self::Decimal,
            "date32" => Self::Date,
            "datetime64" => Self::Timestamp,
            "smallint" | "integer" | "int" | "int2" | "int4" | "int8" | "bigint" | "tinyint"
            | "mediumint" | "serial" | "bigserial" | "smallserial" | "year" | "varint" => {
                if unsigned {
                    Self::Uint
                } else {
                    Self::Int
                }
            }
            "numeric" | "decimal" | "money" => Self::Decimal,
            "real" | "double" | "float" | "float4" | "float8" => Self::Float,
            "date" => Self::Date,
            "time" | "timetz" => Self::Time,
            "timestamp" | "timestamptz" | "datetime" => Self::Timestamp,
            "json" | "jsonb" => Self::Json,
            "bytea" | "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary" => {
                Self::Binary
            }
            _ => Self::Text,
        }
    }
}

fn number(text: &str, decimal_comma: bool) -> Option<String> {
    let mut value: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{a0}' && *c != '\u{202f}')
        .collect();
    if decimal_comma {
        if value.contains('.') && value.contains(',') {
            value = value.replace('.', "");
        }
        value = value.replace(',', ".");
    }
    let digits = value.strip_prefix(['+', '-']).unwrap_or(&value);
    let (mantissa, exponent) = match digits.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (digits, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let ok = !(whole.is_empty() && fraction.is_empty())
        && whole.chars().all(|c| c.is_ascii_digit())
        && fraction.chars().all(|c| c.is_ascii_digit())
        && exponent.is_none_or(|e| {
            let e = e.strip_prefix(['+', '-']).unwrap_or(e);
            !e.is_empty() && e.chars().all(|c| c.is_ascii_digit())
        });
    ok.then_some(value)
}

fn date(text: &str, order: CsvDateOrder) -> Option<chrono::NaiveDate> {
    let parts: Vec<&str> = text.trim().split(['-', '.', '/']).collect();
    let [a, b, c] = parts.as_slice() else {
        return None;
    };
    let (year, month, day) = match order {
        CsvDateOrder::Iso => (a, b, c),
        CsvDateOrder::Dmy => (c, b, a),
        CsvDateOrder::Mdy => (c, a, b),
    };
    if year.len() != 4 {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
}

fn time(text: &str) -> Option<chrono::NaiveTime> {
    let text = text.trim();
    ["%H:%M:%S%.f", "%H:%M:%S", "%H:%M"]
        .iter()
        .find_map(|format| chrono::NaiveTime::parse_from_str(text, format).ok())
}

/// A typed cell for `text` in a column of `kind`, or `None` when it does
/// not convert.
pub fn convert(text: Option<&str>, kind: CsvKind, options: &CsvOptions) -> Option<DbCell> {
    let Some(text) = text else {
        return Some(DbCell::Null);
    };
    let trimmed = text.trim();
    Some(match kind {
        CsvKind::Int | CsvKind::Uint => {
            let value = number(trimmed, false)?;
            let digits = value.strip_prefix('+').unwrap_or(&value);
            if digits.contains(['.', 'e', 'E']) || digits.parse::<i128>().is_err() {
                return None;
            }
            if kind == CsvKind::Uint {
                if digits.starts_with('-') {
                    return None;
                }
                DbCell::Uint(digits.into())
            } else {
                DbCell::Int(digits.into())
            }
        }
        CsvKind::Decimal => DbCell::Decimal(number(trimmed, options.decimal_comma)?),
        CsvKind::Float => {
            let lower = trimmed.to_ascii_lowercase();
            if ["nan", "infinity", "-infinity", "inf", "-inf"].contains(&lower.as_str()) {
                DbCell::Float(lower)
            } else {
                DbCell::Float(number(trimmed, options.decimal_comma)?)
            }
        }
        CsvKind::Bool => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "t" | "1" | "yes" | "y" | "on" => DbCell::Bool(true),
            "false" | "f" | "0" | "no" | "n" | "off" => DbCell::Bool(false),
            _ => return None,
        },
        CsvKind::Date => DbCell::Date(date(trimmed, options.date_order)?.to_string()),
        CsvKind::Time => DbCell::Time(time(trimmed)?.to_string()),
        CsvKind::Timestamp => {
            let (day, clock) = match trimmed.split_once(['T', ' ']) {
                Some((day, clock)) => (day, Some(clock.trim())),
                None => (trimmed, None),
            };
            let day = date(day, options.date_order)?;
            // An explicit offset stays with the value (`+03:00`, `Z`).
            let (clock, zone) = match clock {
                Some(clock) => {
                    let split = clock
                        .rfind(['+', '-', 'Z'])
                        .filter(|&i| i >= 5)
                        .unwrap_or(clock.len());
                    (Some(&clock[..split]), &clock[split..])
                }
                None => (None, ""),
            };
            let clock = match clock {
                Some(clock) => time(clock)?,
                None => chrono::NaiveTime::MIN,
            };
            let zone = zone.trim();
            if !zone.is_empty()
                && zone != "Z"
                && !zone.chars().skip(1).all(|c| c.is_ascii_digit() || c == ':')
            {
                return None;
            }
            DbCell::Timestamp(format!("{}T{}{}", day, clock, zone))
        }
        CsvKind::Json => {
            serde_json::from_str::<serde_json::Value>(trimmed).ok()?;
            DbCell::Json(trimmed.into())
        }
        CsvKind::Binary => {
            use base64::Engine;
            let hex = trimmed
                .strip_prefix("\\x")
                .or_else(|| trimmed.strip_prefix("0x"));
            let bytes = match hex {
                Some(hex) => hex::decode(hex).ok()?,
                None => base64::engine::general_purpose::STANDARD
                    .decode(trimmed)
                    .ok()?,
            };
            DbCell::Binary(base64::engine::general_purpose::STANDARD.encode(bytes))
        }
        CsvKind::Text => DbCell::Text(text.into()),
        CsvKind::Auto => {
            let iso = CsvOptions {
                date_order: CsvDateOrder::Iso,
                ..options.clone()
            };
            [
                CsvKind::Int,
                CsvKind::Float,
                CsvKind::Date,
                CsvKind::Timestamp,
            ]
            .into_iter()
            .find_map(|kind| {
                // Dates need their separators; numbers keep the dot.
                let shaped = match kind {
                    CsvKind::Date | CsvKind::Timestamp => {
                        trimmed.len() >= 10 && trimmed.contains('-')
                    }
                    CsvKind::Float => !options.decimal_comma || !trimmed.contains(','),
                    _ => true,
                };
                shaped.then(|| convert(Some(text), kind, &iso)).flatten()
            })
            .or(match trimmed {
                "true" => Some(DbCell::Bool(true)),
                "false" => Some(DbCell::Bool(false)),
                _ => None,
            })
            .unwrap_or_else(|| DbCell::Text(text.into()))
        }
    })
}

/// Checks the first [`CHECK_ROWS`] rows against the target columns.
pub fn check(
    path: &Path,
    options: &CsvOptions,
    mapping: &[CsvMapping],
    columns: &[DbEditColumn],
) -> Result<Vec<CsvColumnCheck>, DbError> {
    let targets = targets(mapping, columns)?;
    let mut checks: Vec<CsvColumnCheck> = targets
        .iter()
        .map(|(_, column, kind)| CsvColumnCheck {
            target: column.name.clone(),
            kind: format!("{kind:?}").to_ascii_lowercase(),
            bad: 0,
            samples: Vec::new(),
            missing_required: false,
        })
        .collect();
    for column in columns.iter().filter(|c| c.required()) {
        if !mapping.iter().any(|m| m.target == column.name) {
            checks.push(CsvColumnCheck {
                target: column.name.clone(),
                kind: format!("{:?}", CsvKind::of(&column.type_name)).to_ascii_lowercase(),
                bad: 0,
                samples: Vec::new(),
                missing_required: true,
            });
        }
    }
    let mut reader = source(path, options, Default::default())?;
    let mut rows = 0;
    while rows < CHECK_ROWS {
        let Some(Ok((_, fields))) = reader.next(options) else {
            break;
        };
        rows += 1;
        for (check, (source, column, kind)) in checks.iter_mut().zip(&targets) {
            let field = fields.get(*source).cloned().flatten();
            let converted = convert(field.as_deref(), *kind, options);
            let bad = match &converted {
                None => true,
                Some(DbCell::Null) if column.required() => {
                    check.missing_required = true;
                    false
                }
                _ => false,
            };
            if bad {
                check.bad += 1;
                if check.samples.len() < 3 {
                    let text = field.unwrap_or_default();
                    let mut short: String = text.chars().take(60).collect();
                    if text.chars().count() > 60 {
                        short.push('…');
                    }
                    check.samples.push(short);
                }
            }
        }
    }
    Ok(checks)
}

/// Source index, target column and kind per mapped column.
fn targets<'a>(
    mapping: &[CsvMapping],
    columns: &'a [DbEditColumn],
) -> Result<Vec<(usize, &'a DbEditColumn, CsvKind)>, DbError> {
    if mapping.is_empty() || mapping.len() > MAX_COLUMNS {
        return Err(DbError::InvalidRequest);
    }
    let mut seen = std::collections::BTreeSet::new();
    mapping
        .iter()
        .map(|m| {
            let column = columns
                .iter()
                .find(|c| c.name == m.target)
                .ok_or(DbError::InvalidRequest)?;
            if column.generated || !seen.insert(&m.target) || m.source >= MAX_COLUMNS {
                return Err(DbError::InvalidRequest);
            }
            Ok((m.source, column, CsvKind::of(&column.type_name)))
        })
        .collect()
}

/// A simple `CREATE TABLE` for a file: one nullable column per file column,
/// typed from the first [`CHECK_ROWS`] rows (whole numbers, decimals, dates,
/// timestamps, true/false, else text). A starting point for the table
/// designer; it is reviewed and run like any other statement.
pub fn create_table_sql(
    path: &Path,
    options: &CsvOptions,
    engine: DbEngine,
    table: &DbObjectRef,
) -> Result<String, DbError> {
    if !matches!(
        engine,
        DbEngine::Postgresql
            | DbEngine::Mysql
            | DbEngine::Mariadb
            | DbEngine::Sqlite
            | DbEngine::Clickhouse
    ) || table.name.is_empty()
    {
        return Err(DbError::InvalidRequest);
    }
    let shape = preview(path, options)?;
    let candidates = [
        CsvKind::Int,
        CsvKind::Decimal,
        CsvKind::Date,
        CsvKind::Timestamp,
        CsvKind::Bool,
    ];
    let mut kinds: Vec<Vec<CsvKind>> = vec![candidates.to_vec(); shape.columns.len()];
    let mut seen = vec![false; shape.columns.len()];
    let mut reader = source(path, options, Default::default())?;
    let mut rows = 0;
    while rows < CHECK_ROWS {
        let Some(Ok((_, fields))) = reader.next(options) else {
            break;
        };
        rows += 1;
        for (column, field) in fields.iter().enumerate().take(kinds.len()) {
            let Some(text) = field else {
                continue;
            };
            seen[column] = true;
            kinds[column].retain(|kind| {
                // 0/1 read as numbers, not as true/false.
                (*kind != CsvKind::Bool || !matches!(text.trim(), "0" | "1"))
                    && convert(Some(text), *kind, options).is_some()
            });
        }
    }
    let type_name = |kind: Option<&CsvKind>| match (engine, kind) {
        (DbEngine::Postgresql, Some(CsvKind::Int)) => "bigint",
        (DbEngine::Postgresql, Some(CsvKind::Decimal)) => "numeric",
        (DbEngine::Postgresql, Some(CsvKind::Timestamp)) => "timestamp",
        (DbEngine::Postgresql, Some(CsvKind::Bool)) => "boolean",
        (DbEngine::Postgresql, _) => "text",
        // SQLite type affinity: dates and timestamps are ISO-8601 text.
        (DbEngine::Sqlite, Some(CsvKind::Int | CsvKind::Bool)) => "INTEGER",
        (DbEngine::Sqlite, Some(CsvKind::Decimal)) => "NUMERIC",
        (DbEngine::Sqlite, _) => "TEXT",
        (DbEngine::Clickhouse, Some(CsvKind::Int)) => "Nullable(Int64)",
        (DbEngine::Clickhouse, Some(CsvKind::Decimal)) => "Nullable(Decimal(38, 10))",
        (DbEngine::Clickhouse, Some(CsvKind::Timestamp)) => "Nullable(DateTime64(3))",
        (DbEngine::Clickhouse, Some(CsvKind::Bool)) => "Nullable(Bool)",
        (DbEngine::Clickhouse, Some(CsvKind::Date)) => "Nullable(Date32)",
        (DbEngine::Clickhouse, _) => "Nullable(String)",
        (_, Some(CsvKind::Int)) => "bigint",
        (_, Some(CsvKind::Decimal)) => "decimal(38,10)",
        (_, Some(CsvKind::Timestamp)) => "datetime",
        (_, Some(CsvKind::Bool)) => "boolean",
        (_, _) => "text",
    };
    let definitions: Vec<String> = shape
        .columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let kind = if seen[i] { kinds[i].first() } else { None };
            let kind = match kind {
                Some(CsvKind::Date) if engine != DbEngine::Clickhouse => "date",
                other => type_name(other),
            };
            format!("    {} {kind}", quote_column(engine, name))
        })
        .collect();
    // A MergeTree needs a sorting key; the designer can change it later.
    let engine_clause = if engine == DbEngine::Clickhouse {
        "\nENGINE = MergeTree\nORDER BY tuple()"
    } else {
        ""
    };
    Ok(format!(
        "CREATE TABLE {} (\n{}\n){engine_clause};",
        quote_identifier(engine, table),
        definitions.join(",\n")
    ))
}

// ---- statements ----------------------------------------------------------

fn quote_column(engine: DbEngine, name: &str) -> String {
    quote_identifier(
        engine,
        &DbObjectRef {
            database: String::new(),
            schema: None,
            kind: DbObjectKind::Column,
            name: name.into(),
            parent_name: None,
        },
    )
}

/// `INSERT … VALUES (…), (…)` for `rows` rows of `columns`, with the
/// conflict clause of `mode` (keyed by `key`). Placeholders are `{{pN}}`
/// from `first` on.
pub fn insert_sql(
    engine: DbEngine,
    table: &DbObjectRef,
    columns: &[String],
    key: &[String],
    mode: CsvInsertMode,
    rows: usize,
) -> Result<String, DbError> {
    if columns.is_empty() || rows == 0 {
        return Err(DbError::InvalidRequest);
    }
    let names: Vec<String> = columns.iter().map(|c| quote_column(engine, c)).collect();
    let mut values = Vec::with_capacity(rows);
    let mut index = 0;
    for _ in 0..rows {
        let row: Vec<String> = (0..columns.len())
            .map(|_| {
                index += 1;
                format!("{{{{p{}}}}}", index - 1)
            })
            .collect();
        values.push(format!("({})", row.join(", ")));
    }
    let target = quote_identifier(engine, table);
    if engine == DbEngine::Cassandra {
        // One row per statement (batches have a small size limit). CQL
        // INSERT replaces a row with the same key, so only upsert may do
        // that; insert and skip-existing write new keys only.
        if rows != 1 || key.is_empty() {
            return Err(DbError::InvalidRequest);
        }
        let condition = match mode {
            CsvInsertMode::Upsert => "",
            CsvInsertMode::Insert | CsvInsertMode::SkipExisting => " IF NOT EXISTS",
        };
        return Ok(format!(
            "INSERT INTO {target} ({}) VALUES {}{condition}",
            names.join(", "),
            values.join(", ")
        ));
    }
    let updates: Vec<&String> = columns.iter().filter(|c| !key.contains(c)).collect();
    let conflict = match (engine, mode) {
        // MergeTree keys are not unique: every row is a new row.
        (DbEngine::Clickhouse, CsvInsertMode::Insert) => String::new(),
        (DbEngine::Clickhouse, _) => return Err(DbError::InvalidRequest),
        (_, CsvInsertMode::Insert) => String::new(),
        (_, _) if key.is_empty() => return Err(DbError::InvalidRequest),
        (DbEngine::Postgresql, CsvInsertMode::SkipExisting) => {
            format!(" ON CONFLICT ({}) DO NOTHING", quoted(engine, key))
        }
        (DbEngine::Postgresql, CsvInsertMode::Upsert) if updates.is_empty() => {
            format!(" ON CONFLICT ({}) DO NOTHING", quoted(engine, key))
        }
        (DbEngine::Postgresql, CsvInsertMode::Upsert) => format!(
            " ON CONFLICT ({}) DO UPDATE SET {}",
            quoted(engine, key),
            updates
                .iter()
                .map(|c| {
                    let c = quote_column(engine, c);
                    format!("{c} = EXCLUDED.{c}")
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        // A rowid table without a declared key has no conflict target.
        (DbEngine::Sqlite, _) if key == ["rowid"] => return Err(DbError::InvalidRequest),
        // SQLite 3.24+: the same upsert syntax as PostgreSQL.
        (DbEngine::Sqlite, CsvInsertMode::SkipExisting) => {
            format!(" ON CONFLICT ({}) DO NOTHING", quoted(engine, key))
        }
        (DbEngine::Sqlite, CsvInsertMode::Upsert) if updates.is_empty() => {
            format!(" ON CONFLICT ({}) DO NOTHING", quoted(engine, key))
        }
        (DbEngine::Sqlite, CsvInsertMode::Upsert) => format!(
            " ON CONFLICT ({}) DO UPDATE SET {}",
            quoted(engine, key),
            updates
                .iter()
                .map(|c| {
                    let c = quote_column(engine, c);
                    format!("{c} = excluded.{c}")
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        (DbEngine::Mysql | DbEngine::Mariadb, CsvInsertMode::SkipExisting) => {
            // A no-op update keeps the row; unlike INSERT IGNORE it does not
            // hide other errors (truncation, bad values).
            let c = quote_column(engine, &key[0]);
            format!(" ON DUPLICATE KEY UPDATE {c} = {c}")
        }
        (DbEngine::Mysql | DbEngine::Mariadb, CsvInsertMode::Upsert) if updates.is_empty() => {
            let c = quote_column(engine, &key[0]);
            format!(" ON DUPLICATE KEY UPDATE {c} = {c}")
        }
        (DbEngine::Mysql | DbEngine::Mariadb, CsvInsertMode::Upsert) => format!(
            " ON DUPLICATE KEY UPDATE {}",
            updates
                .iter()
                .map(|c| {
                    let c = quote_column(engine, c);
                    format!("{c} = VALUES({c})")
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => return Err(DbError::InvalidRequest),
    };
    Ok(format!(
        "INSERT INTO {target} ({}) VALUES {}{conflict}",
        names.join(", "),
        values.join(", ")
    ))
}

fn quoted(engine: DbEngine, names: &[String]) -> String {
    names
        .iter()
        .map(|n| quote_column(engine, n))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Rows per statement for `columns` columns within the bound-value limit.
pub fn batch_rows(requested: u32, columns: usize) -> usize {
    let cap = (MAX_PARAMETERS / columns.max(1)).max(1);
    (requested.clamp(1, MAX_BATCH_ROWS) as usize).min(cap)
}

/// The statements the review shows: the optional `DELETE FROM` and one
/// `INSERT` row of placeholders. Its parameters are NULL; the digest binds
/// the approval to exactly this template.
pub fn template(
    engine: DbEngine,
    plan: &CsvImportPlan,
    key: &[String],
) -> Result<DbQueryRequest, DbError> {
    if !matches!(
        engine,
        DbEngine::Postgresql
            | DbEngine::Mysql
            | DbEngine::Mariadb
            | DbEngine::Sqlite
            | DbEngine::Cassandra
            | DbEngine::Clickhouse
            | DbEngine::Mongodb
            | DbEngine::Redis
    ) || plan.table.kind
        != match engine {
            DbEngine::Mongodb => DbObjectKind::Collection,
            DbEngine::Redis => DbObjectKind::RedisDatabase,
            _ => DbObjectKind::Table,
        }
    {
        return Err(DbError::InvalidRequest);
    }
    match engine {
        DbEngine::Mongodb => return mongo_template(plan),
        DbEngine::Redis => return redis_template(plan),
        _ => {}
    }
    let columns: Vec<String> = plan.mapping.iter().map(|m| m.target.clone()).collect();
    let insert = insert_sql(engine, &plan.table, &columns, key, plan.mode, 1)?;
    let text = if plan.truncate {
        // CQL has no unconditional DELETE, and a ClickHouse DELETE needs a
        // WHERE: TRUNCATE (reviewed as dangerous).
        let clear = match engine {
            DbEngine::Cassandra => "TRUNCATE",
            DbEngine::Clickhouse => "TRUNCATE TABLE",
            _ => "DELETE FROM",
        };
        format!(
            "{clear} {};\n{insert};",
            quote_identifier(engine, &plan.table)
        )
    } else {
        format!("{insert};")
    };
    Ok(DbQueryRequest {
        text,
        parameters: (0..columns.len())
            .map(|i| DbParameter {
                name: format!("p{i}"),
                value: DbCell::Null,
                type_hint: None,
            })
            .collect(),
        approval_ticket: None,
    })
}

/// Swallows result rows of write statements and counts affected rows.
#[derive(Default)]
pub(crate) struct Affected {
    pub rows: u64,
}

#[async_trait]
impl RowConsumer for Affected {
    async fn start(&mut self, _: Vec<DbColumn>, _: Option<DbObjectRef>) -> Result<(), DbError> {
        Ok(())
    }

    async fn row(&mut self, _: Vec<DbCell>) -> Result<bool, DbError> {
        Ok(true)
    }

    async fn statement_done(&mut self, _: u64, affected: Option<u64>) -> Result<(), DbError> {
        self.rows += affected.unwrap_or(0);
        Ok(())
    }
}

pub(crate) struct Reader {
    source: Source,
    pub(crate) targets: Vec<(usize, CsvKind, bool)>,
    pub(crate) columns: Vec<String>,
}

impl Reader {
    pub(crate) fn open(
        path: &Path,
        plan: &CsvImportPlan,
        columns: &[DbEditColumn],
        read: std::sync::Arc<AtomicU64>,
    ) -> Result<(Self, u64), DbError> {
        let resolved = targets(&plan.mapping, columns)?;
        let source = source(path, &plan.options, read)?;
        let size = source.size;
        Ok((
            Self {
                targets: resolved
                    .iter()
                    .map(|(s, c, k)| (*s, *k, c.required()))
                    .collect(),
                columns: resolved.iter().map(|(_, c, _)| c.name.clone()).collect(),
                source,
            },
            size,
        ))
    }

    /// The next row as typed cells: `Ok(None)` at the end, `Err((line,
    /// fields, reason))` for a row that does not convert.
    #[allow(clippy::type_complexity)]
    pub(crate) fn next(
        &mut self,
        options: &CsvOptions,
    ) -> Option<Result<(u64, Vec<DbCell>), (u64, Vec<Option<String>>, &'static str)>> {
        match self.source.next(options)? {
            Err(line) => Some(Err((line, Vec::new(), "unreadable"))),
            Ok((line, fields)) => {
                let mut cells = Vec::with_capacity(self.targets.len());
                for (source, kind, required) in &self.targets {
                    let field = fields.get(*source).cloned().flatten();
                    match convert(field.as_deref(), *kind, options) {
                        Some(DbCell::Null) if *required => {
                            return Some(Err((line, fields, "required")))
                        }
                        Some(cell) => cells.push(cell),
                        None => return Some(Err((line, fields, "type"))),
                    }
                }
                Some(Ok((line, cells)))
            }
        }
    }
}

/// CSV text of one rejected row for the error report: line, reason, fields.
pub(crate) fn report_line(line: u64, reason: &str, fields: &[Option<String>]) -> String {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    let mut record = vec![line.to_string(), reason.to_owned()];
    record.extend(fields.iter().map(|f| f.clone().unwrap_or_default()));
    let _ = writer.write_record(&record);
    String::from_utf8(writer.into_inner().unwrap_or_default()).unwrap_or_default()
}

// ---- running ----------------------------------------------------------------

/// The open write transaction of a PostgreSQL or MySQL driver.
enum Tx<'a> {
    Postgres(&'a mut crate::postgres::Postgres),
    Mysql(&'a mut crate::mysql::Mysql),
    Sqlite(&'a mut crate::sqlite::Sqlite),
    /// `sqlite3` on a host: every statement runs and commits on its own, so
    /// there is no transaction to open and a failed statement changed
    /// nothing (savepoints are not needed).
    SqliteCli(&'a mut crate::sqlite_cli::SqliteCli),
    /// CQL has no transactions: every row is its own write.
    Cassandra(&'a mut crate::cassandra::Cassandra),
    /// No transactions either; each INSERT block is written whole or not.
    Clickhouse(&'a mut crate::clickhouse::ClickHouse),
    /// No transactions; each document is written or refused on its own.
    Mongo(&'a mut crate::mongo::Mongo),
    /// No transactions; one key per command.
    Redis(&'a mut crate::redis_driver::Redis),
}

impl Tx<'_> {
    /// Whether one transaction can span several statements.
    fn transactional(&self) -> bool {
        !matches!(
            self,
            Self::SqliteCli(_)
                | Self::Cassandra(_)
                | Self::Clickhouse(_)
                | Self::Mongo(_)
                | Self::Redis(_)
        )
    }

    async fn begin(&mut self) -> Result<(), DbError> {
        match self {
            Self::Postgres(driver) => driver.begin_write().await,
            Self::Mysql(driver) => driver.begin_write().await,
            Self::Sqlite(driver) => driver.begin_write().await,
            Self::SqliteCli(_)
            | Self::Cassandra(_)
            | Self::Clickhouse(_)
            | Self::Mongo(_)
            | Self::Redis(_) => Ok(()),
        }
    }

    async fn control(&mut self, sql: &'static str) -> Result<(), DbError> {
        match self {
            Self::Postgres(driver) => driver.control(sql).await,
            Self::Mysql(driver) => driver.control(sql).await,
            Self::Sqlite(driver) => driver.control(sql).await,
            Self::SqliteCli(_)
            | Self::Cassandra(_)
            | Self::Clickhouse(_)
            | Self::Mongo(_)
            | Self::Redis(_) => Ok(()),
        }
    }

    async fn step(
        &mut self,
        sql: &str,
        bindings: &[DbParameter],
        cancel: CancellationToken,
    ) -> Result<u64, DbError> {
        match self {
            Self::Postgres(driver) => driver.step(sql, bindings, cancel).await,
            Self::Mysql(driver) => driver.step(sql, bindings, cancel).await,
            Self::Sqlite(driver) => driver.step(sql, bindings, cancel).await,
            Self::SqliteCli(driver) => driver.step(sql, bindings, cancel).await,
            Self::Cassandra(driver) => driver.import_step(sql, bindings, cancel).await,
            Self::Clickhouse(driver) => {
                let mut written = Affected::default();
                driver
                    .query(sql, bindings, false, &mut written, cancel)
                    .await?;
                Ok(written.rows)
            }
            // Written through `import_mongo_batch` and `import_redis_batch`.
            Self::Mongo(_) | Self::Redis(_) => Err(DbError::InvalidRequest),
        }
    }

    async fn end(&mut self, commit: bool) -> Result<(), DbError> {
        match self {
            Self::Postgres(driver) => driver.end_write(commit).await,
            Self::Mysql(driver) => driver.end_write(commit).await,
            Self::Sqlite(driver) => driver.end_write(commit).await,
            Self::SqliteCli(_)
            | Self::Cassandra(_)
            | Self::Clickhouse(_)
            | Self::Mongo(_)
            | Self::Redis(_) => Ok(()),
        }
    }
}

/// Rejected rows written to the error report (created on the first one).
struct Report {
    path: Option<PathBuf>,
    file: Option<File>,
    progress: Arc<CsvImportProgress>,
}

impl Report {
    fn add(&mut self, line: u64, reason: &str, fields: &[Option<String>]) {
        use std::io::Write;
        self.progress.failed.fetch_add(1, Ordering::AcqRel);
        let Some(path) = &self.path else {
            return;
        };
        if self.file.is_none() {
            match File::create(path) {
                Ok(mut file) => {
                    let _ = file.write_all(b"line,reason,fields\n");
                    if let Ok(mut report) = self.progress.report.lock() {
                        *report = Some(path.to_string_lossy().into_owned());
                    }
                    self.file = Some(file);
                }
                Err(_) => {
                    self.path = None;
                    return;
                }
            }
        }
        if let Some(file) = &mut self.file {
            let _ = file.write_all(report_line(line, reason, fields).as_bytes());
        }
    }
}

/// A statement the guard permitted, as the driver runs it.
fn permitted(
    session: &DbSession,
    request: &DbQueryRequest,
) -> Result<Vec<(String, Vec<DbParameter>)>, DbError> {
    let permits = session.guard.prepare(request)?;
    let mut statements = Vec::with_capacity(permits.len());
    for permit in &permits {
        session.guard.validate_permit(permit)?;
        match permit.parsed() {
            crate::parser::ParsedStatement::Sql { text, parameters }
            | crate::parser::ParsedStatement::Cql {
                text,
                parameters,
                keyspace: None,
            } => statements.push((text.clone(), parameters.clone())),
            _ => return Err(DbError::InvalidRequest),
        }
    }
    Ok(statements)
}

fn batch_request(
    engine: DbEngine,
    plan: &CsvImportPlan,
    columns: &[String],
    key: &[String],
    rows: &[(u64, Vec<DbCell>)],
) -> Result<DbQueryRequest, DbError> {
    let text = insert_sql(engine, &plan.table, columns, key, plan.mode, rows.len())?;
    let parameters = rows
        .iter()
        .flat_map(|(_, cells)| cells.iter())
        .enumerate()
        .map(|(i, cell)| DbParameter {
            name: format!("p{i}"),
            value: cell.clone(),
            type_hint: match cell {
                // ClickHouse binds a decimal with an explicit scale.
                DbCell::Decimal(value) if engine == DbEngine::Clickhouse => {
                    Some(clickhouse_decimal(value).into())
                }
                _ => None,
            },
        })
        .collect();
    Ok(DbQueryRequest {
        text,
        parameters,
        approval_ticket: None,
    })
}

/// A MongoDB field the import may write: not empty, no `$` in front, no
/// `.` (a path) and no NUL.
fn mongo_field(name: &str) -> bool {
    !name.is_empty() && name.len() <= 256 && !name.starts_with('$') && !name.contains(['.', '\0'])
}

/// The effective MongoDB mode: an upsert that sets nothing but `_id` only
/// adds missing documents.
fn mongo_mode(mode: CsvInsertMode, columns: &[String]) -> CsvInsertMode {
    if mode == CsvInsertMode::Upsert && columns.iter().all(|c| c == "_id") {
        CsvInsertMode::SkipExisting
    } else {
        mode
    }
}

/// An `insert` (or, for upsert, an `update … upsert: true` by `_id`) of
/// `rows` with `ordered: false`, values as `$param` placeholders. NULL
/// fields are left out; a 24-digit hex text into an objectId field is an
/// ObjectId; a date or timestamp without an offset is UTC.
fn mongo_request(
    collection: &str,
    columns: &[String],
    object_ids: &[bool],
    mode: CsvInsertMode,
    rows: &[(u64, Vec<DbCell>)],
    keep_nulls: bool,
) -> Result<DbQueryRequest, DbError> {
    if columns.is_empty() || rows.is_empty() || !columns.iter().all(|c| mongo_field(c)) {
        return Err(DbError::InvalidRequest);
    }
    let quote = |text: &str| serde_json::to_string(text).unwrap_or_default();
    let mut parameters = Vec::new();
    let mut value = |cell: &DbCell, object_id: bool| -> String {
        if let (true, DbCell::Text(text)) = (object_id, cell) {
            if text.len() == 24 && text.chars().all(|c| c.is_ascii_hexdigit()) {
                return format!(r#"{{"$oid":"{text}"}}"#);
            }
        }
        let cell = match cell {
            DbCell::Date(day) => DbCell::Timestamp(format!("{day}T00:00:00Z")),
            DbCell::Timestamp(at)
                if !at.ends_with('Z')
                    && at.get(10..).is_some_and(|tail| !tail.contains(['+', '-'])) =>
            {
                DbCell::Timestamp(format!("{at}Z"))
            }
            other => other.clone(),
        };
        let name = format!("p{}", parameters.len());
        parameters.push(DbParameter {
            name: name.clone(),
            value: cell,
            type_hint: None,
        });
        format!(r#"{{"$param":"{name}"}}"#)
    };
    let mode = mongo_mode(mode, columns);
    let key = columns.iter().position(|c| c == "_id");
    let mut entries = Vec::with_capacity(rows.len());
    for (_, cells) in rows {
        let mut fields = Vec::new();
        let mut id = None;
        for (index, (column, cell)) in columns.iter().zip(cells).enumerate() {
            if matches!(cell, DbCell::Null) && !keep_nulls {
                continue;
            }
            let rendered = value(cell, object_ids.get(index).copied().unwrap_or(false));
            if mode == CsvInsertMode::Upsert && Some(index) == key {
                id = Some(rendered);
            } else {
                fields.push(format!("{}:{rendered}", quote(column)));
            }
        }
        entries.push(match mode {
            CsvInsertMode::Upsert => format!(
                r#"{{"q":{{"_id":{}}},"u":{{"$set":{{{}}}}},"upsert":true}}"#,
                id.ok_or(DbError::InvalidRequest)?,
                fields.join(",")
            ),
            _ => format!("{{{}}}", fields.join(",")),
        });
    }
    let text = match mode {
        CsvInsertMode::Upsert => format!(
            r#"{{"update":{},"updates":[{}],"ordered":false}}"#,
            quote(collection),
            entries.join(",")
        ),
        _ => format!(
            r#"{{"insert":{},"documents":[{}],"ordered":false}}"#,
            quote(collection),
            entries.join(",")
        ),
    };
    Ok(DbQueryRequest {
        text,
        parameters,
        approval_ticket: None,
    })
}

/// The reviewed MongoDB template: the optional `delete` of every document
/// and one document of placeholders.
fn mongo_template(plan: &CsvImportPlan) -> Result<DbQueryRequest, DbError> {
    let columns: Vec<String> = plan.mapping.iter().map(|m| m.target.clone()).collect();
    if mongo_mode(plan.mode, &columns) == CsvInsertMode::Upsert
        && !columns.iter().any(|c| c == "_id")
    {
        return Err(DbError::InvalidRequest);
    }
    let row = (0, vec![DbCell::Null; columns.len()]);
    let mut request = mongo_request(&plan.table.name, &columns, &[], plan.mode, &[row], true)?;
    if plan.truncate {
        request.text = format!(
            r#"{{"delete":{},"deletes":[{{"q":{{}},"limit":0}}]}};{}"#,
            serde_json::to_string(&plan.table.name).unwrap_or_default(),
            request.text
        );
    }
    Ok(request)
}

/// A Redis hash field the import may name in a command: printable, no
/// spaces, quotes, backslashes or placeholder braces.
fn redis_field(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && !matches!(name, "key" | "value")
        && !name.contains("{{")
        && !name.contains("}}")
        && name
            .chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && !matches!(c, '"' | '\'' | '\\'))
}

/// What a Redis import writes per row: a string (`key` and `value`) or a
/// hash (`key` and named fields).
enum RedisShape {
    String,
    Hash(Vec<String>),
}

fn redis_shape(columns: &[String]) -> Result<RedisShape, DbError> {
    let fields: Vec<&String> = columns.iter().filter(|c| *c != "key").collect();
    if !columns.iter().any(|c| c == "key") || fields.is_empty() {
        return Err(DbError::InvalidRequest);
    }
    if fields.iter().any(|c| *c == "value") {
        return if fields.len() == 1 {
            Ok(RedisShape::String)
        } else {
            Err(DbError::InvalidRequest)
        };
    }
    if !fields.iter().all(|c| redis_field(c)) {
        return Err(DbError::InvalidRequest);
    }
    Ok(RedisShape::Hash(fields.into_iter().cloned().collect()))
}

/// The commands for one row: the `EXISTS` check (a hash that must not be
/// overwritten) and the write. A string is `SET` (`NX` unless upserting),
/// a hash `HSET` with its non-NULL fields. Keys get the folder `prefix`.
fn redis_row(
    columns: &[String],
    shape: &RedisShape,
    prefix: &str,
    mode: CsvInsertMode,
    cells: &[DbCell],
) -> Result<(Option<DbQueryRequest>, DbQueryRequest), DbError> {
    let cell = |name: &str| {
        columns
            .iter()
            .position(|c| c == name)
            .and_then(|i| cells.get(i))
    };
    let key = match cell("key") {
        Some(DbCell::Text(key)) if !key.is_empty() => DbCell::Text(format!("{prefix}{key}")),
        _ => return Err(DbError::InvalidRequest),
    };
    let parameter = |name: &str, value: DbCell| DbParameter {
        name: name.into(),
        value,
        type_hint: None,
    };
    let keep = mode == CsvInsertMode::Upsert;
    match shape {
        RedisShape::String => {
            let value = cell("value").cloned().unwrap_or(DbCell::Null);
            if matches!(value, DbCell::Null) {
                return Err(DbError::InvalidRequest);
            }
            Ok((
                None,
                DbQueryRequest {
                    text: format!("SET {{{{k}}}} {{{{v}}}}{}", if keep { "" } else { " NX" }),
                    parameters: vec![parameter("k", key), parameter("v", value)],
                    approval_ticket: None,
                },
            ))
        }
        RedisShape::Hash(fields) => {
            let mut text = String::from("HSET {{k}}");
            let mut parameters = vec![parameter("k", key.clone())];
            for field in fields {
                if let Some(value) = cell(field).filter(|v| !matches!(v, DbCell::Null)) {
                    let name = format!("f{}", parameters.len());
                    text.push_str(&format!(" {field} {{{{{name}}}}}"));
                    parameters.push(parameter(&name, value.clone()));
                }
            }
            if parameters.len() == 1 {
                return Err(DbError::InvalidRequest);
            }
            let check = (!keep).then(|| DbQueryRequest {
                text: "EXISTS {{k}}".into(),
                parameters: vec![parameter("k", key)],
                approval_ticket: None,
            });
            Ok((
                check,
                DbQueryRequest {
                    text,
                    parameters,
                    approval_ticket: None,
                },
            ))
        }
    }
}

/// The key prefix of a Redis import target: a key folder's, else none.
fn redis_prefix(target: &DbObjectRef) -> &str {
    if target.parent_name.is_some() {
        &target.name
    } else {
        ""
    }
}

/// The reviewed Redis template: the commands of one row (no clearing:
/// a whole-database FLUSHDB is never offered).
fn redis_template(plan: &CsvImportPlan) -> Result<DbQueryRequest, DbError> {
    if plan.truncate {
        return Err(DbError::InvalidRequest);
    }
    let columns: Vec<String> = plan.mapping.iter().map(|m| m.target.clone()).collect();
    let shape = redis_shape(&columns)?;
    let cells = vec![DbCell::Text("…".into()); columns.len()];
    let (check, write) = redis_row(
        &columns,
        &shape,
        redis_prefix(&plan.table),
        plan.mode,
        &cells,
    )?;
    // The check and the write share the key placeholder.
    let parameters = write.parameters.clone();
    let text = match check {
        Some(check) => format!("{}\n{}", check.text, write.text),
        None => write.text.clone(),
    };
    Ok(DbQueryRequest {
        text,
        parameters,
        approval_ticket: None,
    })
}

/// The ClickHouse decimal type a value fits: 20 integer and 18 fraction
/// digits, else 38 and 38.
fn clickhouse_decimal(value: &str) -> &'static str {
    let digits = value.trim_start_matches(['+', '-']);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if !digits.contains(['e', 'E']) && whole.len() <= 20 && fraction.len() <= 18 {
        "Decimal128(18)"
    } else {
        "Decimal256(38)"
    }
}

impl DbSession {
    /// The reviewable template of `plan` (see [`template`]), checked against
    /// the table's columns and key. Writes must be allowed.
    pub async fn import_template(&self, plan: &CsvImportPlan) -> Result<DbQueryRequest, DbError> {
        if self.guard.is_read_only() || self.guard.policy().mobile {
            return Err(DbError::PolicyDenied);
        }
        let columns = self.import_columns(&plan.table, &plan.mapping).await?;
        targets(&plan.mapping, &columns)?;
        let key = self.import_key(&plan.table).await?;
        template(self.info.engine, plan, &key)
    }

    /// The columns an import may write: the table's, and for a MongoDB
    /// collection also new fields named in `mapping` (each value typed as
    /// it reads).
    pub async fn import_columns(
        &self,
        table: &DbObjectRef,
        mapping: &[CsvMapping],
    ) -> Result<Vec<DbEditColumn>, DbError> {
        let mut columns = self.edit_columns(table).await?;
        if self.info.engine == DbEngine::Redis {
            // Other targets are hash fields.
            for target in mapping.iter().map(|m| &m.target) {
                if columns.iter().any(|c| &c.name == target) {
                    continue;
                }
                if !redis_field(target) || columns.len() >= MAX_COLUMNS {
                    return Err(DbError::InvalidRequest);
                }
                columns.push(DbEditColumn {
                    name: target.clone(),
                    type_name: "hash field".into(),
                    nullable: true,
                    has_default: true,
                    generated: false,
                    key: false,
                });
            }
            return Ok(columns);
        }
        if self.info.engine == DbEngine::Mongodb {
            for target in mapping.iter().map(|m| &m.target) {
                if columns.iter().any(|c| &c.name == target) {
                    continue;
                }
                if !mongo_field(target) || columns.len() >= MAX_COLUMNS {
                    return Err(DbError::InvalidRequest);
                }
                columns.push(DbEditColumn {
                    name: target.clone(),
                    type_name: "auto".into(),
                    nullable: true,
                    has_default: true,
                    generated: false,
                    key: false,
                });
            }
        }
        Ok(columns)
    }

    /// The key skip-existing and upsert match on: `_id` in MongoDB.
    async fn import_key(&self, table: &DbObjectRef) -> Result<Vec<String>, DbError> {
        match self.info.engine {
            DbEngine::Mongodb => return Ok(vec!["_id".into()]),
            DbEngine::Redis => return Ok(vec!["key".into()]),
            _ => {}
        }
        self.primary_key(table).await
    }

    /// Imports a CSV file as `plan` says. `approved` is the reviewed
    /// [`template`] with its approved ticket: it permits the optional
    /// `DELETE FROM`. Every batch is a guarded write of the same shape.
    /// Rows that do not convert (or that the server refuses, with
    /// `skip_bad_rows`) go to `report`; cancelling rolls back the open
    /// transaction (earlier batches stay with per-batch transactions).
    pub async fn import_csv(
        &self,
        path: &Path,
        plan: &CsvImportPlan,
        approved: &DbQueryRequest,
        progress: Arc<CsvImportProgress>,
        cancel: CancellationToken,
        report: Option<PathBuf>,
    ) -> Result<(), DbError> {
        let engine = self.info.engine;
        let key = self.import_key(&plan.table).await?;
        let columns = self.import_columns(&plan.table, &plan.mapping).await?;
        if template(engine, plan, &key)?.text != approved.text {
            return Err(DbError::ApprovalRequired);
        }
        let object_ids: Vec<bool> = plan
            .mapping
            .iter()
            .map(|m| {
                columns
                    .iter()
                    .any(|c| c.name == m.target && c.type_name == "objectId")
            })
            .collect();
        let read = Arc::new(AtomicU64::new(0));
        let (mut reader, size) = Reader::open(path, plan, &columns, read.clone())?;
        progress.total_bytes.store(size, Ordering::Release);
        let names = reader.columns.clone();
        let rows_per_batch = if engine == DbEngine::Cassandra {
            1
        } else {
            batch_rows(plan.batch_rows, names.len())
        };
        // The approval is checked (and a destructive one consumed) up front.
        let (truncate, mongo_truncate) = if matches!(engine, DbEngine::Mongodb | DbEngine::Redis) {
            let permits = self.guard.prepare(approved)?;
            let mut clear = None;
            for permit in &permits {
                self.guard.validate_permit(permit)?;
            }
            if plan.truncate {
                match permits.first().map(|permit| permit.parsed()) {
                    Some(crate::parser::ParsedStatement::Mongo { command, order }) => {
                        clear = Some((command.clone(), order.clone()));
                    }
                    _ => return Err(DbError::InvalidRequest),
                }
            }
            (None, clear)
        } else {
            let approved_statements = permitted(self, approved)?;
            let truncate = plan
                .truncate
                .then(|| approved_statements.first().cloned())
                .flatten();
            (truncate, None)
        };
        let mut report = Report {
            path: report,
            file: None,
            progress: progress.clone(),
        };
        let mut driver = self.driver.lock().await;
        let mut tx = match &mut *driver {
            crate::session::Driver::Postgres(driver) => Tx::Postgres(driver),
            crate::session::Driver::Mysql(driver) => Tx::Mysql(driver),
            crate::session::Driver::Sqlite(driver) => Tx::Sqlite(driver),
            crate::session::Driver::SqliteCli(driver) => Tx::SqliteCli(driver),
            crate::session::Driver::Cassandra(driver) => Tx::Cassandra(driver),
            crate::session::Driver::ClickHouse(driver) => Tx::Clickhouse(driver),
            crate::session::Driver::Mongo(driver) => Tx::Mongo(driver),
            crate::session::Driver::Redis(driver) => Tx::Redis(driver),
        };
        let whole = plan.transaction == CsvTransaction::Whole;
        if whole && !tx.transactional() {
            // `sqlite3` on a host commits every statement: a whole-file
            // transaction cannot be promised there.
            return Err(DbError::InvalidRequest);
        }
        let mut open = false;
        let result: Result<(), DbError> = async {
            if whole {
                tx.begin().await?;
                open = true;
            }
            let mut first = true;
            loop {
                if cancel.is_cancelled() {
                    return Err(DbError::Cancelled);
                }
                let mut rows = Vec::with_capacity(rows_per_batch);
                while rows.len() < rows_per_batch {
                    match reader.next(&plan.options) {
                        None => break,
                        Some(Ok(row)) => rows.push(row),
                        Some(Err((line, fields, reason))) => {
                            if !plan.skip_bad_rows {
                                if let Ok(mut error) = progress.error.lock() {
                                    *error = Some(format!("row_{reason}:{line}"));
                                }
                                return Err(DbError::InvalidRequest);
                            }
                            report.add(line, reason, &fields);
                        }
                    }
                    progress.rows_read.fetch_add(1, Ordering::AcqRel);
                }
                progress
                    .bytes_read
                    .store(read.load(Ordering::Acquire).min(size), Ordering::Release);
                if rows.is_empty() && !(first && (truncate.is_some() || mongo_truncate.is_some())) {
                    break;
                }
                if !whole {
                    tx.begin().await?;
                    open = true;
                }
                if first {
                    if let Some((sql, parameters)) = &truncate {
                        tx.step(sql, parameters, cancel.clone()).await?;
                    }
                    if let (Some((command, order)), Tx::Mongo(driver)) = (&mongo_truncate, &mut tx)
                    {
                        driver
                            .import_write(&plan.table.database, command, order, &cancel)
                            .await?;
                    }
                    first = false;
                }
                if let (false, Tx::Redis(driver)) = (rows.is_empty(), &mut tx) {
                    self.import_redis_batch(
                        driver,
                        plan,
                        &names,
                        &rows,
                        &progress,
                        &mut report,
                        &cancel,
                    )
                    .await?;
                } else if let (false, Tx::Mongo(driver)) = (rows.is_empty(), &mut tx) {
                    self.import_mongo_batch(
                        driver,
                        plan,
                        &names,
                        &object_ids,
                        &rows,
                        &progress,
                        &mut report,
                        &cancel,
                    )
                    .await?;
                } else if !rows.is_empty() {
                    self.import_batch(
                        &mut tx,
                        plan,
                        &names,
                        &key,
                        &rows,
                        &progress,
                        &mut report,
                        &cancel,
                    )
                    .await?;
                }
                if !whole {
                    tx.end(true).await?;
                    open = false;
                }
            }
            if whole {
                tx.end(true).await?;
                open = false;
            }
            Ok(())
        }
        .await;
        if result.is_err() && open {
            let _ = tx.end(false).await;
            if whole {
                // Nothing of the whole-file transaction is kept.
                progress.imported.store(0, Ordering::Release);
                progress.skipped.store(0, Ordering::Release);
            }
        }
        progress
            .bytes_read
            .store(read.load(Ordering::Acquire).min(size), Ordering::Release);
        result
    }

    /// Redis rows, one key at a time. An existing key is skipped in
    /// skip-existing mode and refused in insert mode; a hash is checked with
    /// `EXISTS` first (not atomic with the write).
    #[allow(clippy::too_many_arguments)]
    async fn import_redis_batch(
        &self,
        driver: &mut crate::redis_driver::Redis,
        plan: &CsvImportPlan,
        names: &[String],
        rows: &[(u64, Vec<DbCell>)],
        progress: &CsvImportProgress,
        report: &mut Report,
        cancel: &CancellationToken,
    ) -> Result<(), DbError> {
        let shape = redis_shape(names)?;
        let prefix = redis_prefix(&plan.table);
        let database = &plan.table.database;
        for (line, cells) in rows {
            let run = |request: DbQueryRequest| -> Result<Vec<Vec<u8>>, DbError> {
                let permits = self.guard.prepare(&request)?;
                let [permit] = permits.as_slice() else {
                    return Err(DbError::InvalidRequest);
                };
                self.guard.validate_permit(permit)?;
                match permit.parsed() {
                    crate::parser::ParsedStatement::Redis { argv } => Ok(argv.clone()),
                    _ => Err(DbError::InvalidRequest),
                }
            };
            let outcome: Result<bool, DbError> = async {
                let (check, write) = redis_row(names, &shape, prefix, plan.mode, cells)?;
                if let Some(check) = check {
                    if driver.import_write(database, &run(check)?, cancel).await? != Some(0) {
                        return Ok(false);
                    }
                }
                Ok(driver
                    .import_write(database, &run(write)?, cancel)
                    .await?
                    .is_some())
            }
            .await;
            match outcome {
                Ok(true) => {
                    progress.imported.fetch_add(1, Ordering::AcqRel);
                }
                Ok(false) if plan.mode == CsvInsertMode::SkipExisting => {
                    progress.skipped.fetch_add(1, Ordering::AcqRel);
                }
                Err(
                    error @ (DbError::Cancelled
                    | DbError::Timeout
                    | DbError::SessionClosed
                    | DbError::ConnectionFailed
                    | DbError::PolicyDenied),
                ) => return Err(error),
                // An existing key in insert mode, or a refused write.
                _ if plan.skip_bad_rows => {
                    let fields: Vec<Option<String>> = cells.iter().map(cell_text).collect();
                    report.add(*line, "refused", &fields);
                }
                _ => {
                    if let Ok(mut error) = progress.error.lock() {
                        *error = Some(format!("row_refused:{line}"));
                    }
                    return Err(DbError::QueryFailed);
                }
            }
        }
        Ok(())
    }

    /// One MongoDB batch, `ordered: false`: every document is written or
    /// refused on its own. A duplicate `_id` is skipped in skip-existing
    /// mode; other refused documents are reported with `skip_bad_rows`,
    /// else they stop the import (the others of the batch stay written).
    #[allow(clippy::too_many_arguments)]
    async fn import_mongo_batch(
        &self,
        driver: &mut crate::mongo::Mongo,
        plan: &CsvImportPlan,
        names: &[String],
        object_ids: &[bool],
        rows: &[(u64, Vec<DbCell>)],
        progress: &CsvImportProgress,
        report: &mut Report,
        cancel: &CancellationToken,
    ) -> Result<(), DbError> {
        let mode = mongo_mode(plan.mode, names);
        let request = mongo_request(&plan.table.name, names, object_ids, plan.mode, rows, false)?;
        let permits = self.guard.prepare(&request)?;
        let [permit] = permits.as_slice() else {
            return Err(DbError::InvalidRequest);
        };
        self.guard.validate_permit(permit)?;
        let crate::parser::ParsedStatement::Mongo { command, order } = permit.parsed() else {
            return Err(DbError::InvalidRequest);
        };
        let (written, refused) = driver
            .import_write(&plan.table.database, command, order, cancel)
            .await?;
        let mut skipped = 0u64;
        let mut failed = Vec::new();
        for (index, code) in refused {
            // 11000: duplicate key.
            if code == 11000 && mode == CsvInsertMode::SkipExisting {
                skipped += 1;
            } else if let Some(row) = rows.get(index) {
                failed.push(row);
            }
        }
        let written = if mode == CsvInsertMode::Upsert {
            rows.len() as u64 - skipped - failed.len() as u64
        } else {
            written
        };
        progress.imported.fetch_add(written, Ordering::AcqRel);
        progress.skipped.fetch_add(skipped, Ordering::AcqRel);
        if !failed.is_empty() && !plan.skip_bad_rows {
            if let Ok(mut error) = progress.error.lock() {
                *error = Some(format!("row_refused:{}", failed[0].0));
            }
            return Err(DbError::QueryFailed);
        }
        for row in failed {
            let fields: Vec<Option<String>> = row.1.iter().map(cell_text).collect();
            report.add(row.0, "refused", &fields);
        }
        Ok(())
    }

    /// One batch; with `skip_bad_rows` a refused batch is retried row by
    /// row inside savepoints and the refused rows are reported.
    #[allow(clippy::too_many_arguments)]
    async fn import_batch(
        &self,
        tx: &mut Tx<'_>,
        plan: &CsvImportPlan,
        names: &[String],
        key: &[String],
        rows: &[(u64, Vec<DbCell>)],
        progress: &CsvImportProgress,
        report: &mut Report,
        cancel: &CancellationToken,
    ) -> Result<(), DbError> {
        let engine = self.info.engine;
        let count = |n: usize, affected: u64| {
            let n = n as u64;
            let written = match plan.mode {
                CsvInsertMode::Upsert => n,
                _ => affected.min(n),
            };
            progress.imported.fetch_add(written, Ordering::AcqRel);
            progress.skipped.fetch_add(n - written, Ordering::AcqRel);
        };
        let request = batch_request(engine, plan, names, key, rows)?;
        let statements = permitted(self, &request)?;
        let [(sql, parameters)] = statements.as_slice() else {
            return Err(DbError::InvalidRequest);
        };
        if !plan.skip_bad_rows {
            let affected = tx.step(sql, parameters, cancel.clone()).await?;
            count(rows.len(), affected);
            return Ok(());
        }
        tx.control("SAVEPOINT cc_import_batch").await?;
        match tx.step(sql, parameters, cancel.clone()).await {
            Ok(affected) => {
                tx.control("RELEASE SAVEPOINT cc_import_batch").await?;
                count(rows.len(), affected);
                return Ok(());
            }
            Err(
                error @ (DbError::Cancelled
                | DbError::Timeout
                | DbError::SessionClosed
                | DbError::ConnectionFailed
                | DbError::PolicyDenied),
            ) => return Err(error),
            Err(_) => tx.control("ROLLBACK TO SAVEPOINT cc_import_batch").await?,
        }
        for row in rows {
            let request = batch_request(engine, plan, names, key, std::slice::from_ref(row))?;
            let statements = permitted(self, &request)?;
            let [(sql, parameters)] = statements.as_slice() else {
                return Err(DbError::InvalidRequest);
            };
            tx.control("SAVEPOINT cc_import_row").await?;
            match tx.step(sql, parameters, cancel.clone()).await {
                Ok(affected) => {
                    tx.control("RELEASE SAVEPOINT cc_import_row").await?;
                    count(1, affected);
                }
                Err(
                    error @ (DbError::Cancelled
                    | DbError::Timeout
                    | DbError::SessionClosed
                    | DbError::ConnectionFailed
                    | DbError::PolicyDenied),
                ) => return Err(error),
                Err(_) => {
                    tx.control("ROLLBACK TO SAVEPOINT cc_import_row").await?;
                    let fields: Vec<Option<String>> = row.1.iter().map(cell_text).collect();
                    report.add(row.0, "refused", &fields);
                }
            }
        }
        tx.control("RELEASE SAVEPOINT cc_import_batch").await?;
        Ok(())
    }
}

fn cell_text(cell: &DbCell) -> Option<String> {
    match cell {
        DbCell::Null => None,
        DbCell::Bool(value) => Some(value.to_string()),
        DbCell::Int(v)
        | DbCell::Uint(v)
        | DbCell::Decimal(v)
        | DbCell::Float(v)
        | DbCell::Text(v)
        | DbCell::Date(v)
        | DbCell::Time(v)
        | DbCell::Timestamp(v)
        | DbCell::Binary(v)
        | DbCell::Json(v) => Some(v.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clickhouse_and_bson_type_names_map_to_kinds() {
        for (name, kind) in [
            ("Nullable(Int64)", CsvKind::Int),
            ("UInt8", CsvKind::Uint),
            ("LowCardinality(Nullable(String))", CsvKind::Text),
            ("Decimal(10, 2)", CsvKind::Decimal),
            ("DateTime64(3)", CsvKind::Timestamp),
            ("Date32", CsvKind::Date),
            ("Float32", CsvKind::Float),
            ("Bool", CsvKind::Bool),
            ("long", CsvKind::Int),
            ("datetime", CsvKind::Timestamp),
            ("objectId", CsvKind::Text),
            ("auto", CsvKind::Auto),
        ] {
            assert_eq!(CsvKind::of(name), kind, "{name}");
        }
        assert_eq!(clickhouse_decimal("-12.50"), "Decimal128(18)");
        assert_eq!(
            clickhouse_decimal("123456789012345678901.5"),
            "Decimal256(38)"
        );
    }

    #[test]
    fn auto_values_type_themselves() {
        let options = CsvOptions::default();
        let auto = |text: &str| convert(Some(text), CsvKind::Auto, &options);
        assert_eq!(auto("12"), Some(DbCell::Int("12".into())));
        assert_eq!(auto("1.5"), Some(DbCell::Float("1.5".into())));
        assert_eq!(auto("true"), Some(DbCell::Bool(true)));
        assert_eq!(auto("2024-01-02"), Some(DbCell::Date("2024-01-02".into())));
        assert!(matches!(
            auto("2024-01-02 03:04:05"),
            Some(DbCell::Timestamp(_))
        ));
        assert_eq!(auto("yes"), Some(DbCell::Text("yes".into())));
        assert_eq!(auto("007x"), Some(DbCell::Text("007x".into())));
        assert_eq!(convert(None, CsvKind::Auto, &options), Some(DbCell::Null));
    }

    #[test]
    fn mongo_requests_insert_or_upsert_by_id() {
        let rows = vec![
            (
                2,
                vec![
                    DbCell::Text("0123456789abcdef01234567".into()),
                    DbCell::Null,
                    DbCell::Timestamp("2024-01-02T03:04:05".into()),
                ],
            ),
            (
                3,
                vec![
                    DbCell::Text("plain".into()),
                    DbCell::Int("7".into()),
                    DbCell::Date("2024-01-02".into()),
                ],
            ),
        ];
        let columns = ["_id".to_string(), "qty".into(), "seen".into()];
        let insert = mongo_request(
            "items",
            &columns,
            &[true, false, false],
            CsvInsertMode::Insert,
            &rows,
            false,
        )
        .unwrap();
        assert_eq!(
            insert.text,
            r#"{"insert":"items","documents":[{"_id":{"$oid":"0123456789abcdef01234567"},"seen":{"$param":"p0"}},{"_id":{"$param":"p1"},"qty":{"$param":"p2"},"seen":{"$param":"p3"}}],"ordered":false}"#
        );
        assert_eq!(
            insert.parameters[0].value,
            DbCell::Timestamp("2024-01-02T03:04:05Z".into())
        );
        assert_eq!(
            insert.parameters[3].value,
            DbCell::Timestamp("2024-01-02T00:00:00Z".into())
        );
        let upsert = mongo_request(
            "items",
            &columns,
            &[true, false, false],
            CsvInsertMode::Upsert,
            &rows[..1],
            false,
        )
        .unwrap();
        assert_eq!(
            upsert.text,
            r#"{"update":"items","updates":[{"q":{"_id":{"$oid":"0123456789abcdef01234567"}},"u":{"$set":{"seen":{"$param":"p0"}}},"upsert":true}],"ordered":false}"#
        );
        // Only _id: an upsert adds missing documents.
        assert_eq!(
            mongo_mode(CsvInsertMode::Upsert, &["_id".to_string()]),
            CsvInsertMode::SkipExisting
        );
        for bad in ["$where", "a.b", ""] {
            assert!(mongo_request(
                "items",
                &[bad.to_string()],
                &[],
                CsvInsertMode::Insert,
                &rows[..1],
                false
            )
            .is_err());
        }
    }
    use std::io::Write;

    fn file(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        file
    }

    fn column(name: &str, type_name: &str, nullable: bool) -> DbEditColumn {
        DbEditColumn {
            name: name.into(),
            type_name: type_name.into(),
            nullable,
            has_default: false,
            generated: false,
            key: name == "id",
        }
    }

    #[test]
    fn detects_delimiters_outside_quotes() {
        assert_eq!(detect_delimiter("a;b;c\n1;2;3\n", '"'), ';');
        assert_eq!(detect_delimiter("a\tb\n1\t2\n", '"'), '\t');
        assert_eq!(detect_delimiter("a|b|c\n1|\"x|y\"|3\n", '"'), '|');
        assert_eq!(
            detect_delimiter("name,note\n\"a;b;c\",d\n\"e;f\",g\n", '"'),
            ','
        );
        assert_eq!(detect_delimiter("single\nvalue\n", '"'), ',');
    }

    #[test]
    fn previews_utf8_with_bom_quotes_and_multiline_fields() {
        let csv =
            file("\u{feff}id;name;note\n1;\"say \"\"hi\"\"\";\n2;\"two\nlines\";x\n".as_bytes());
        let preview = preview(csv.path(), &CsvOptions::default()).unwrap();
        assert_eq!(preview.delimiter, ';');
        assert_eq!(preview.encoding, CsvEncoding::Utf8);
        assert_eq!(preview.columns, ["id", "name", "note"]);
        assert_eq!(
            preview.rows,
            vec![
                vec![Some("1".into()), Some("say \"hi\"".into()), None],
                vec![
                    Some("2".into()),
                    Some("two\nlines".into()),
                    Some("x".into())
                ],
            ]
        );
    }

    #[test]
    fn decodes_windows_1251_and_latin1() {
        // "Привет" in Windows-1251.
        let cp1251 = [
            b'a', b',', b'b', b'\n', 0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2, b',', b'1', b'\n',
        ];
        let auto = preview(file(&cp1251).path(), &CsvOptions::default()).unwrap();
        assert_eq!(auto.encoding, CsvEncoding::Windows1251);
        assert_eq!(auto.rows[0][0].as_deref(), Some("Привет"));
        let latin = b"a\ncaf\xE9\n";
        let options = CsvOptions {
            encoding: CsvEncoding::Latin1,
            ..CsvOptions::default()
        };
        assert_eq!(
            preview(file(latin).path(), &options).unwrap().rows[0][0].as_deref(),
            Some("café")
        );
    }

    #[test]
    fn header_skip_rows_and_null_markers() {
        let csv = file(b"exported by tool\n\na,b,c\nNULL,\\N,\n");
        let mut options = CsvOptions {
            skip_rows: 2,
            null: CsvNull::Word,
            ..CsvOptions::default()
        };
        let p = preview(csv.path(), &options).unwrap();
        assert_eq!(p.columns, ["a", "b", "c"]);
        assert_eq!(
            p.rows[0],
            vec![None, Some("\\N".into()), Some(String::new())]
        );
        options.null = CsvNull::Backslash;
        assert_eq!(
            preview(csv.path(), &options).unwrap().rows[0],
            vec![Some("NULL".into()), None, Some(String::new())]
        );
        options.null = CsvNull::Empty;
        options.header = false;
        options.skip_rows = 3;
        let p = preview(csv.path(), &options).unwrap();
        assert_eq!(p.columns, ["column1", "column2", "column3"]);
        assert_eq!(p.rows[0][2], None);
    }

    #[test]
    fn refuses_files_over_the_limit_and_bad_options() {
        let csv = file(b"a\n1\n");
        let bad = CsvOptions {
            quote: 'x',
            ..CsvOptions::default()
        };
        assert!(preview(csv.path(), &bad).is_err());
        let same = CsvOptions {
            delimiter: Some('"'),
            ..CsvOptions::default()
        };
        assert!(preview(csv.path(), &same).is_err());
        let big = tempfile::NamedTempFile::new().unwrap();
        big.as_file().set_len(MAX_IMPORT_BYTES + 1).unwrap();
        assert!(matches!(
            preview(big.path(), &CsvOptions::default()),
            Err(DbError::ResultLimit)
        ));
    }

    #[test]
    fn converts_values_by_column_type() {
        let o = CsvOptions::default();
        let comma = CsvOptions {
            decimal_comma: true,
            date_order: CsvDateOrder::Dmy,
            ..CsvOptions::default()
        };
        assert_eq!(CsvKind::of("integer"), CsvKind::Int);
        assert_eq!(CsvKind::of("int unsigned"), CsvKind::Uint);
        assert_eq!(CsvKind::of("numeric(10,2)"), CsvKind::Decimal);
        assert_eq!(CsvKind::of("character varying(40)"), CsvKind::Text);
        assert_eq!(CsvKind::of("timestamp with time zone"), CsvKind::Timestamp);
        assert_eq!(CsvKind::of("tinyint(1)"), CsvKind::Bool);
        assert_eq!(CsvKind::of("integer[]"), CsvKind::Text);
        assert_eq!(
            convert(Some(" 42 "), CsvKind::Int, &o),
            Some(DbCell::Int("42".into()))
        );
        assert_eq!(convert(Some("4.2"), CsvKind::Int, &o), None);
        assert_eq!(convert(Some("-1"), CsvKind::Uint, &o), None);
        assert_eq!(
            convert(Some("1 234,50"), CsvKind::Decimal, &comma),
            Some(DbCell::Decimal("1234.50".into()))
        );
        assert_eq!(
            convert(Some("1.234,5"), CsvKind::Decimal, &comma),
            Some(DbCell::Decimal("1234.5".into()))
        );
        assert_eq!(convert(Some("1,5"), CsvKind::Decimal, &o), None);
        assert_eq!(
            convert(Some("2e3"), CsvKind::Float, &o),
            Some(DbCell::Float("2e3".into()))
        );
        assert_eq!(
            convert(Some("Yes"), CsvKind::Bool, &o),
            Some(DbCell::Bool(true))
        );
        assert_eq!(convert(Some("maybe"), CsvKind::Bool, &o), None);
        assert_eq!(
            convert(Some("22.10.2026"), CsvKind::Date, &comma),
            Some(DbCell::Date("2026-10-22".into()))
        );
        assert_eq!(convert(Some("31.02.2026"), CsvKind::Date, &comma), None);
        assert_eq!(
            convert(Some("2026-10-22 09:51:18"), CsvKind::Timestamp, &o),
            Some(DbCell::Timestamp("2026-10-22T09:51:18".into()))
        );
        assert_eq!(
            convert(Some("2026-10-22T09:51:18.5+03:00"), CsvKind::Timestamp, &o),
            Some(DbCell::Timestamp("2026-10-22T09:51:18.500+03:00".into()))
        );
        assert_eq!(
            convert(
                Some("10/22/2026 9:51"),
                CsvKind::Timestamp,
                &CsvOptions {
                    date_order: CsvDateOrder::Mdy,
                    ..CsvOptions::default()
                }
            ),
            Some(DbCell::Timestamp("2026-10-22T09:51:00".into()))
        );
        assert_eq!(
            convert(Some("{\"a\": 1}"), CsvKind::Json, &o),
            Some(DbCell::Json("{\"a\": 1}".into()))
        );
        assert_eq!(convert(Some("{a: 1}"), CsvKind::Json, &o), None);
        assert_eq!(
            convert(Some("\\x00ff"), CsvKind::Binary, &o),
            Some(DbCell::Binary("AP8=".into()))
        );
        // Text keeps everything, formulas included.
        assert_eq!(
            convert(Some("=SUM(A1)"), CsvKind::Text, &o),
            Some(DbCell::Text("=SUM(A1)".into()))
        );
        assert_eq!(convert(None, CsvKind::Int, &o), Some(DbCell::Null));
    }

    #[test]
    fn check_reports_bad_values_and_missing_required_columns() {
        let csv = file(b"id,amount,when\n1,10,2026-01-01\n2,ten,2026-13-01\nx,,\n");
        let columns = [
            column("id", "integer", false),
            column("amount", "integer", true),
            column("when", "date", true),
            column("name", "text", false),
        ];
        let mapping = [
            CsvMapping {
                source: 0,
                target: "id".into(),
            },
            CsvMapping {
                source: 1,
                target: "amount".into(),
            },
            CsvMapping {
                source: 2,
                target: "when".into(),
            },
        ];
        let checks = check(csv.path(), &CsvOptions::default(), &mapping, &columns).unwrap();
        assert_eq!(checks[0].bad, 1);
        assert_eq!(checks[0].samples, ["x"]);
        assert_eq!(checks[1].samples, ["ten"]);
        assert_eq!(checks[2].samples, ["2026-13-01"]);
        assert!(checks[3].missing_required && checks[3].target == "name");
        // Generated, unknown and repeated targets are refused.
        let twice = [mapping[0].clone(), mapping[0].clone()];
        assert!(check(csv.path(), &CsvOptions::default(), &twice, &columns).is_err());
        let unknown = [CsvMapping {
            source: 0,
            target: "nope".into(),
        }];
        assert!(check(csv.path(), &CsvOptions::default(), &unknown, &columns).is_err());
    }

    #[test]
    fn insert_statements_per_engine_and_mode() {
        let pg = DbObjectRef {
            database: String::new(),
            schema: Some("public".into()),
            kind: DbObjectKind::Table,
            name: "items".into(),
            parent_name: None,
        };
        let my = DbObjectRef {
            database: "shop".into(),
            schema: None,
            ..pg.clone()
        };
        let columns = ["id".to_string(), "na\"me".to_string()];
        let key = ["id".to_string()];
        assert_eq!(
            insert_sql(DbEngine::Postgresql, &pg, &columns, &key, CsvInsertMode::Insert, 2).unwrap(),
            "INSERT INTO \"public\".\"items\" (\"id\", \"na\"\"me\") VALUES ({{p0}}, {{p1}}), ({{p2}}, {{p3}})"
        );
        assert_eq!(
            insert_sql(DbEngine::Postgresql, &pg, &columns, &key, CsvInsertMode::SkipExisting, 1)
                .unwrap(),
            "INSERT INTO \"public\".\"items\" (\"id\", \"na\"\"me\") VALUES ({{p0}}, {{p1}}) ON CONFLICT (\"id\") DO NOTHING"
        );
        assert_eq!(
            insert_sql(
                DbEngine::Postgresql,
                &pg,
                &columns,
                &key,
                CsvInsertMode::Upsert,
                1
            )
            .unwrap(),
            "INSERT INTO \"public\".\"items\" (\"id\", \"na\"\"me\") VALUES ({{p0}}, {{p1}}) \
             ON CONFLICT (\"id\") DO UPDATE SET \"na\"\"me\" = EXCLUDED.\"na\"\"me\""
        );
        assert_eq!(
            insert_sql(DbEngine::Mysql, &my, &columns, &key, CsvInsertMode::SkipExisting, 1).unwrap(),
            "INSERT INTO `shop`.`items` (`id`, `na\"me`) VALUES ({{p0}}, {{p1}}) ON DUPLICATE KEY UPDATE `id` = `id`"
        );
        assert_eq!(
            insert_sql(
                DbEngine::Mariadb,
                &my,
                &columns,
                &key,
                CsvInsertMode::Upsert,
                1
            )
            .unwrap(),
            "INSERT INTO `shop`.`items` (`id`, `na\"me`) VALUES ({{p0}}, {{p1}}) \
             ON DUPLICATE KEY UPDATE `na\"me` = VALUES(`na\"me`)"
        );
        assert!(insert_sql(
            DbEngine::Postgresql,
            &pg,
            &columns,
            &[],
            CsvInsertMode::Upsert,
            1
        )
        .is_err());
        assert!(insert_sql(
            DbEngine::Clickhouse,
            &my,
            &columns,
            &key,
            CsvInsertMode::Upsert,
            1
        )
        .is_err());
        assert_eq!(batch_rows(500, 10), 500);
        assert_eq!(batch_rows(5000, 100), 600);
        assert_eq!(batch_rows(0, 3), 1);
    }

    #[test]
    fn the_template_binds_the_review_to_the_plan() {
        let plan = CsvImportPlan {
            table: DbObjectRef {
                database: String::new(),
                schema: Some("public".into()),
                kind: DbObjectKind::Table,
                name: "items".into(),
                parent_name: None,
            },
            options: CsvOptions::default(),
            mapping: vec![CsvMapping {
                source: 0,
                target: "id".into(),
            }],
            mode: CsvInsertMode::Insert,
            batch_rows: 100,
            skip_bad_rows: false,
            transaction: CsvTransaction::Whole,
            truncate: true,
        };
        let request = template(DbEngine::Postgresql, &plan, &["id".into()]).unwrap();
        assert_eq!(
            request.text,
            "DELETE FROM \"public\".\"items\";\nINSERT INTO \"public\".\"items\" (\"id\") VALUES ({{p0}});"
        );
        assert_eq!(request.parameters.len(), 1);
        let plan: CsvImportPlan = serde_json::from_value(serde_json::json!({
            "table": {"database": "", "schema": "public", "kind": "table", "name": "items"},
            "options": {"delimiter": ";", "null": "backslash"},
            "mapping": [{"source": 1, "target": "id"}],
        }))
        .unwrap();
        assert_eq!(plan.batch_rows, 500);
        assert_eq!(plan.options.delimiter, Some(';'));
        assert!(!plan.truncate);
        assert!(serde_json::from_value::<CsvImportPlan>(serde_json::json!({
            "table": {"database": "", "kind": "table", "name": "items"},
            "options": {},
            "mapping": [],
            "sql": "DROP TABLE items"
        }))
        .is_err());
    }

    #[test]
    fn infers_a_create_table_from_the_first_rows() {
        let csv = file(b"id,price,day,at,flag,name,empty\n1,1.5,2026-01-02,2026-01-02 10:00,yes,a,\n2,2,2026-01-03,2026-01-03 11:00,no,b,\n");
        let table = DbObjectRef {
            database: String::new(),
            schema: Some("public".into()),
            kind: DbObjectKind::Table,
            name: "new \"one\"".into(),
            parent_name: None,
        };
        assert_eq!(
            create_table_sql(csv.path(), &CsvOptions::default(), DbEngine::Postgresql, &table).unwrap(),
            "CREATE TABLE \"public\".\"new \"\"one\"\"\" (\n    \"id\" bigint,\n    \"price\" numeric,\n    \"day\" date,\n    \
             \"at\" timestamp,\n    \"flag\" boolean,\n    \"name\" text,\n    \"empty\" text\n);"
        );
        let my = DbObjectRef {
            database: "shop".into(),
            schema: None,
            ..table
        };
        assert!(
            create_table_sql(csv.path(), &CsvOptions::default(), DbEngine::Mysql, &my)
                .unwrap()
                .contains("`price` decimal(38,10)")
        );
        assert!(
            create_table_sql(csv.path(), &CsvOptions::default(), DbEngine::Redis, &my).is_err()
        );
    }

    #[test]
    fn report_lines_are_csv() {
        assert_eq!(
            report_line(7, "type", &[Some("a,b".into()), None]),
            "7,type,\"a,b\",\n"
        );
    }
}
