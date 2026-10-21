//! Editor scopes use UTF-16 offsets, matching Flutter TextSelection. Selection
//! is never itself an execution permit: the resulting request still enters the
//! native guard before any driver can execute it.
use crate::types::{DbCell, DbError, DbParameter, MAX_QUERY_BYTES};
use cc_models::database::DbEngine;
use serde::Serialize;
use sqlparser::{
    dialect::{ClickHouseDialect, Dialect, MySqlDialect, PostgreSqlDialect},
    parser::Parser,
    tokenizer::{Token, Tokenizer},
};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryScope {
    Current,
    Selection,
    Script,
}

#[derive(Debug, Serialize)]
pub struct PreparedQuery {
    pub text: String,
    pub parameter_names: Vec<String>,
}

pub fn prepare_query(
    engine: DbEngine,
    text: &str,
    scope: QueryScope,
    selection_start: usize,
    selection_end: usize,
) -> Result<PreparedQuery, DbError> {
    validate_length(text)?;
    let start = utf16_offset(text, selection_start)?;
    let end = utf16_offset(text, selection_end)?;
    if start > end {
        return Err(DbError::InvalidRequest);
    }
    let selected = match scope {
        QueryScope::Script => text,
        QueryScope::Selection if start < end => &text[start..end],
        QueryScope::Selection => return Err(DbError::InvalidRequest),
        QueryScope::Current => {
            let (start, end) = current_statement(engine, text, start)?;
            &text[start..end]
        }
    };
    let selected = if engine == DbEngine::Redis {
        selected.trim()
    } else {
        selected.trim().trim_end_matches(';').trim_end()
    };
    if selected.is_empty() {
        return Err(DbError::InvalidRequest);
    }
    Ok(PreparedQuery {
        text: selected.into(),
        parameter_names: parameter_names(engine, selected)?,
    })
}

pub fn format_query(engine: DbEngine, text: &str) -> Result<String, DbError> {
    validate_length(text)?;
    match engine {
        DbEngine::Redis => {
            let lines = super::redis::tokenize(text)?;
            Ok(lines
                .iter()
                .map(|line| {
                    line.iter()
                        .map(|argument| {
                            let mut result = String::from("\"");
                            for byte in argument {
                                match *byte {
                                    b'"' => result.push_str("\\\""),
                                    b'\\' => result.push_str("\\\\"),
                                    b'\n' => result.push_str("\\n"),
                                    b'\r' => result.push_str("\\r"),
                                    b'\t' => result.push_str("\\t"),
                                    32..=126 => result.push(*byte as char),
                                    value => result.push_str(&format!("\\x{value:02x}")),
                                }
                            }
                            result.push('"');
                            result
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        DbEngine::Mongodb => {
            let commands = super::mongo::split(text)?
                .into_iter()
                .map(|part| {
                    let command: serde_json::Value = if part.trim_start().starts_with('{') {
                        serde_json::from_str(part).map_err(|_| DbError::InvalidRequest)?
                    } else {
                        super::mongo::expression(part)?
                    };
                    serde_json::to_string_pretty(&command).map_err(|_| DbError::InvalidRequest)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(commands.join(";\n"))
        }
        _ => {
            let dialect = dialect(engine)?;
            let parameters = parameter_names(engine, text)?
                .into_iter()
                .map(|name| DbParameter {
                    name,
                    value: DbCell::Null,
                    type_hint: None,
                })
                .collect::<Vec<_>>();
            let (text, markers) =
                super::template::sql_templates(dialect.as_ref(), text, &parameters)?;
            let statements = Parser::new(dialect.as_ref())
                .with_recursion_limit(64)
                .try_with_sql(&text)
                .map_err(|_| DbError::InvalidRequest)?
                .parse_statements()
                .map_err(|_| DbError::InvalidRequest)?;
            if statements.len() > super::MAX_STATEMENTS {
                return Err(DbError::ResultLimit);
            }
            let mut formatted = statements
                .iter()
                .map(|statement| format!("{statement:#}"))
                .collect::<Vec<_>>()
                .join(";\n");
            for (marker, parameter) in markers {
                formatted = formatted.replace(&marker, &format!("{{{{{}}}}}", parameter.name));
            }
            Ok(formatted)
        }
    }
}

pub fn parameter_names(engine: DbEngine, text: &str) -> Result<Vec<String>, DbError> {
    validate_length(text)?;
    let mut names = BTreeSet::new();
    match engine {
        DbEngine::Redis => {
            for line in super::redis::tokenize(text)? {
                for (index, argument) in line.iter().enumerate() {
                    if argument.starts_with(b"{{") && argument.ends_with(b"}}") {
                        let name = std::str::from_utf8(&argument[2..argument.len() - 2])
                            .map_err(|_| DbError::InvalidRequest)?;
                        if index == 0 || !super::template::valid_name(name) {
                            return Err(DbError::InvalidRequest);
                        }
                        names.insert(name.to_owned());
                    } else if argument
                        .windows(2)
                        .any(|value| value == b"{{" || value == b"}}")
                    {
                        return Err(DbError::InvalidRequest);
                    }
                }
            }
        }
        DbEngine::Mongodb => {
            for part in super::mongo::split(text)? {
                let command = if part.trim_start().starts_with('{') {
                    serde_json::from_str(part).map_err(|_| DbError::InvalidRequest)?
                } else {
                    super::mongo::expression(part)?
                };
                mongo_parameters(&command, &mut names, 0)?;
            }
        }
        _ => {
            let dialect = dialect(engine)?;
            let tokens = Tokenizer::new(dialect.as_ref(), text)
                .tokenize()
                .map_err(|_| DbError::InvalidRequest)?
                .into_iter()
                .filter(|token| !matches!(token, Token::Whitespace(_)))
                .collect::<Vec<_>>();
            if tokens.len() > 50000 {
                return Err(DbError::ResultLimit);
            }
            for window in tokens.windows(5) {
                if window[0] == Token::LBrace
                    && window[1] == Token::LBrace
                    && window[3] == Token::RBrace
                    && window[4] == Token::RBrace
                {
                    let Token::Word(word) = &window[2] else {
                        return Err(DbError::InvalidRequest);
                    };
                    if word.quote_style.is_some() || !super::template::valid_name(&word.value) {
                        return Err(DbError::InvalidRequest);
                    }
                    names.insert(word.value.clone());
                }
            }
        }
    }
    if names.len() > crate::types::MAX_PARAMETER_COUNT {
        return Err(DbError::ResultLimit);
    }
    Ok(names.into_iter().collect())
}

fn mongo_parameters(
    value: &serde_json::Value,
    names: &mut BTreeSet<String>,
    depth: usize,
) -> Result<(), DbError> {
    if depth > 64 {
        return Err(DbError::ResultLimit);
    }
    match value {
        serde_json::Value::Object(object) => {
            if let Some(name) = object.get("$param") {
                let name = name
                    .as_str()
                    .filter(|name| super::template::valid_name(name))
                    .ok_or(DbError::InvalidRequest)?;
                if object.len() != 1 {
                    return Err(DbError::InvalidRequest);
                }
                names.insert(name.into());
            } else {
                for value in object.values() {
                    mongo_parameters(value, names, depth + 1)?;
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                mongo_parameters(value, names, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_length(text: &str) -> Result<(), DbError> {
    if text.is_empty() || text.len() > MAX_QUERY_BYTES || text.contains('\0') {
        Err(DbError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn utf16_offset(text: &str, wanted: usize) -> Result<usize, DbError> {
    let mut units = 0;
    for (offset, character) in text.char_indices() {
        if units == wanted {
            return Ok(offset);
        }
        units += character.len_utf16();
        if units > wanted {
            return Err(DbError::InvalidRequest);
        }
    }
    if units == wanted {
        Ok(text.len())
    } else {
        Err(DbError::InvalidRequest)
    }
}

fn dialect(engine: DbEngine) -> Result<Box<dyn Dialect>, DbError> {
    match engine {
        DbEngine::Postgresql => Ok(Box::new(PostgreSqlDialect {})),
        DbEngine::Mysql | DbEngine::Mariadb => Ok(Box::new(MySqlDialect {})),
        DbEngine::Clickhouse => Ok(Box::new(ClickHouseDialect {})),
        _ => Err(DbError::InvalidRequest),
    }
}

/// One runnable statement of the editor text: `start..end` is what runs (its
/// first token through its `;`), `first_line..=last_line` the editor lines it
/// occupies, including comments written directly above or after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Statement {
    start: usize,
    end: usize,
    first_line: usize,
    last_line: usize,
}

/// Byte offsets of line starts, for line numbers of byte offsets.
struct Lines(Vec<usize>);

impl Lines {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(offset, _)| offset + 1),
        );
        Self(starts)
    }

    /// Zero-based line of a byte offset.
    fn line(&self, offset: usize) -> usize {
        self.0
            .partition_point(|start| *start <= offset)
            .saturating_sub(1)
    }

    /// Line of the last byte of `start..end` (a newline belongs to the line
    /// it ends).
    fn last_line(&self, start: usize, end: usize) -> usize {
        self.line(end.saturating_sub(1).max(start))
    }

    /// Byte offset of a 1-based tokenizer location (columns count characters).
    fn offset(
        &self,
        text: &str,
        location: sqlparser::tokenizer::Location,
    ) -> Result<usize, DbError> {
        let line = usize::try_from(location.line).map_err(|_| DbError::InvalidRequest)?;
        let column = usize::try_from(location.column).map_err(|_| DbError::InvalidRequest)?;
        let start = *self
            .0
            .get(line.checked_sub(1).ok_or(DbError::InvalidRequest)?)
            .ok_or(DbError::InvalidRequest)?;
        let mut offset = start;
        for (index, character) in text[start..].chars().enumerate() {
            if index + 1 == column {
                return Ok(offset);
            }
            offset += character.len_utf8();
        }
        if text[start..].chars().count() + 1 == column {
            Ok(text.len())
        } else {
            Err(DbError::InvalidRequest)
        }
    }
}

/// Statements being collected: comments and code between two delimiters.
#[derive(Default)]
struct Group {
    first_line: Option<usize>,
    start: Option<usize>,
    end: usize,
    last_line: usize,
}

impl Group {
    fn comment(&mut self, first: usize, last: usize) {
        self.first_line.get_or_insert(first);
        self.last_line = self.last_line.max(last);
    }

    fn code(&mut self, start: usize, end: usize, first: usize, last: usize) {
        self.first_line.get_or_insert(first);
        self.start.get_or_insert(start);
        self.end = end;
        self.last_line = self.last_line.max(last);
    }

    /// The finished statement; a group of comments only is not one.
    fn finish(&mut self, statements: &mut Vec<Statement>) {
        let group = std::mem::take(self);
        if let (Some(start), Some(first_line)) = (group.start, group.first_line) {
            statements.push(Statement {
                start,
                end: group.end,
                first_line,
                last_line: group.last_line,
            });
        }
    }
}

/// Statements of the editor text, delimited like the classic SQL clients: by
/// `;` outside strings and comments, and by blank lines. Redis runs one
/// command per line.
fn statements(engine: DbEngine, text: &str) -> Result<Vec<Statement>, DbError> {
    let lines = Lines::new(text);
    let mut statements = Vec::new();
    let mut group = Group::default();
    // Line of the previous token (blank lines split) and of the `;` that
    // closed the previous statement (a comment after it stays with it).
    let mut previous: Option<usize> = None;
    let mut closed: Option<usize> = None;
    let mut token =
        |kind: Piece, start: usize, end: usize, group: &mut Group, out: &mut Vec<Statement>| {
            let (first, last) = (lines.line(start), lines.last_line(start, end));
            if previous.is_some_and(|line| first > line + 1) {
                group.finish(out);
                closed = None;
            }
            previous = Some(last);
            match kind {
                Piece::Comment if group.first_line.is_none() && closed == Some(first) => {
                    if let Some(statement) = out.last_mut() {
                        statement.last_line = statement.last_line.max(last);
                    }
                }
                Piece::Comment => group.comment(first, last),
                Piece::Code => group.code(start, end, first, last),
                Piece::Delimiter => {
                    if group.start.is_some() {
                        group.code(start, end, first, last);
                    }
                    group.finish(out);
                    closed = Some(last);
                }
            }
        };
    match engine {
        DbEngine::Redis => {
            super::redis::tokenize(text)?;
            for (start, end, comment) in redis_lines(text) {
                let kind = if comment { Piece::Comment } else { Piece::Code };
                token(kind, start, end, &mut group, &mut statements);
                // Every command line is its own statement.
                if !comment {
                    group.finish(&mut statements);
                }
            }
        }
        DbEngine::Mongodb => {
            // Validates balanced nesting and quotes like the script parser.
            super::mongo::split(text)?;
            for (kind, start, end) in mongo_pieces(text) {
                token(kind, start, end, &mut group, &mut statements);
            }
        }
        _ => {
            let dialect = dialect(engine)?;
            let tokens = Tokenizer::new(dialect.as_ref(), text)
                .tokenize_with_location()
                .map_err(|_| DbError::InvalidRequest)?;
            if tokens.len() > 50000 {
                return Err(DbError::ResultLimit);
            }
            for item in tokens {
                use sqlparser::tokenizer::Whitespace;
                let kind = match &item.token {
                    Token::EOF
                    | Token::Whitespace(
                        Whitespace::Space | Whitespace::Newline | Whitespace::Tab,
                    ) => continue,
                    Token::Whitespace(_) => Piece::Comment,
                    Token::SemiColon => Piece::Delimiter,
                    _ => Piece::Code,
                };
                let start = lines.offset(text, item.span.start)?;
                let end = lines.offset(text, item.span.end)?;
                if end <= start {
                    continue;
                }
                token(kind, start, end, &mut group, &mut statements);
            }
        }
    }
    group.finish(&mut statements);
    Ok(statements)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Piece {
    Comment,
    Code,
    Delimiter,
}

/// Redis commands (trimmed, quote-aware across lines) and `#` comment lines
/// as (start, end, comment).
fn redis_lines(text: &str) -> Vec<(usize, usize, bool)> {
    let mut result = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut start: Option<usize> = None;
    let mut end = 0;
    let mut comment = false;
    for (offset, byte) in text.bytes().enumerate() {
        if comment {
            if byte == b'\n' {
                comment = false;
                result.push((start.take().unwrap_or(offset), offset, true));
            }
            continue;
        }
        let Some(from) = start else {
            if !byte.is_ascii_whitespace() {
                start = Some(offset);
                end = offset + 1;
                comment = byte == b'#';
                if !comment {
                    quote = matches!(byte, b'\'' | b'"').then_some(byte);
                    escaped = byte == b'\\';
                }
            }
            continue;
        };
        if escaped {
            escaped = false;
            end = offset + 1;
        } else if byte == b'\\' {
            escaped = true;
            end = offset + 1;
        } else if let Some(current) = quote {
            if current == byte {
                quote = None;
            }
            end = offset + 1;
        } else if byte == b'\n' {
            result.push((from, end, false));
            start = None;
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
            end = offset + 1;
        } else if !byte.is_ascii_whitespace() {
            end = offset + 1;
        }
    }
    if let Some(from) = start {
        result.push((from, if comment { text.len() } else { end }, comment));
    }
    result
}

/// MongoDB expressions: code runs between top-level `;`, `//` comment lines
/// are kept apart; strings and nesting are respected.
fn mongo_pieces(text: &str) -> Vec<(Piece, usize, usize)> {
    let bytes = text.as_bytes();
    let mut pieces = Vec::new();
    let mut depth = 0usize;
    let mut string = false;
    let mut escaped = false;
    let mut index = 0;
    let mut code: Option<(usize, usize)> = None;
    let push_code = |code: &mut Option<(usize, usize)>, pieces: &mut Vec<(Piece, usize, usize)>| {
        if let Some((start, end)) = code.take() {
            pieces.push((Piece::Code, start, end));
        }
    };
    while index < bytes.len() {
        let byte = bytes[index];
        if string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
            }
        } else if depth == 0 && byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            push_code(&mut code, &mut pieces);
            let end = text[index..]
                .find('\n')
                .map_or(text.len(), |at| index + at + 1);
            pieces.push((Piece::Comment, index, end));
            index = end;
            continue;
        } else {
            match byte {
                b'"' => string = true,
                b'(' | b'{' | b'[' => depth += 1,
                b')' | b'}' | b']' => depth = depth.saturating_sub(1),
                b';' if depth == 0 => {
                    push_code(&mut code, &mut pieces);
                    pieces.push((Piece::Delimiter, index, index + 1));
                    index += 1;
                    continue;
                }
                _ => {}
            }
        }
        if byte.is_ascii_whitespace() && !string {
            // Each line of code is reported separately so blank lines between
            // two expressions are seen.
            if byte == b'\n' && depth == 0 {
                push_code(&mut code, &mut pieces);
            }
        } else {
            let start = code.map_or(index, |(start, _)| start);
            code = Some((start, index + 1));
        }
        index += 1;
    }
    push_code(&mut code, &mut pieces);
    pieces
}

/// The statement for a caret at byte `caret`: the one on the caret's line
/// (the one under the caret when a line holds several), else the nearest one
/// above, else the nearest one below.
fn current_statement(
    engine: DbEngine,
    text: &str,
    caret: usize,
) -> Result<(usize, usize), DbError> {
    let statements = statements(engine, text)?;
    let line = Lines::new(text).line(caret);
    let on_line: Vec<&Statement> = statements
        .iter()
        .filter(|statement| statement.first_line <= line && line <= statement.last_line)
        .collect();
    let chosen = if on_line.is_empty() {
        statements
            .iter()
            .rev()
            .find(|statement| statement.last_line < line)
            .or_else(|| {
                statements
                    .iter()
                    .find(|statement| statement.first_line > line)
            })
    } else {
        on_line
            .iter()
            .find(|statement| statement.start <= caret && caret <= statement.end)
            .or_else(|| {
                on_line
                    .iter()
                    .rev()
                    .find(|statement| statement.start <= caret)
            })
            .or_else(|| on_line.first())
            .copied()
    };
    chosen
        .map(|statement| (statement.start, statement.end))
        .ok_or(DbError::InvalidRequest)
}
