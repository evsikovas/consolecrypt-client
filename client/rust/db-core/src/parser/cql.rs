//! CQL (Cassandra / ScyllaDB). A small lexer with the server's own rules for
//! strings (`'…'` with `''`, `$$…$$`), quoted names (`"…"` with `""`) and
//! comments (`--`, `//`, non-nesting `/* */`). The executed text is rebuilt
//! from the inspected tokens: comments are dropped and `{{name}}` templates
//! become `?` bind markers, so nothing the guard did not classify can reach
//! the server. Each statement is classified by its leading keywords; anything
//! not recognised is refused.
use super::{template, ClassifiedStatement, ParsedStatement, MAX_STATEMENTS};
use crate::guard::QueryRisk;
use crate::types::{DbError, DbParameter, DbQueryRequest};
use std::collections::BTreeSet;
use zeroize::Zeroize;

/// Reason codes of CQL statements (shown in reviews and read warnings).
pub const READ: &str = "cql_read";
pub const ALLOW_FILTERING: &str = "cql_allow_filtering";
pub const FULL_SCAN: &str = "cql_full_scan";
pub const USE: &str = "cql_use";
pub const WRITE: &str = "cql_write";
pub const BATCH: &str = "cql_batch";
pub const DDL: &str = "cql_ddl";
pub const TRUNCATE: &str = "cql_truncate";
pub const DROP: &str = "cql_drop";
pub const DROP_COLUMN: &str = "cql_drop_column";
pub const ALTER_KEYSPACE: &str = "cql_alter_keyspace";
pub const CODE: &str = "cql_code";
/// Roles, users and permissions: never allowed on production connections.
pub const ROLE_MANAGEMENT: &str = "cql_role_management";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Comment,
    Word,
    Quoted,
    Str,
    Number,
    Uuid,
    Blob,
    Punct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
}

const MAX_TOKENS: usize = 50_000;

fn ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}
fn ident_part(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn uuid_at(bytes: &[u8]) -> bool {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut index = 0;
    for (group, length) in GROUPS.iter().enumerate() {
        for _ in 0..*length {
            if !bytes.get(index).is_some_and(u8::is_ascii_hexdigit) {
                return false;
            }
            index += 1;
        }
        if group < 4 {
            if bytes.get(index) != Some(&b'-') {
                return false;
            }
            index += 1;
        }
    }
    !bytes.get(index).copied().is_some_and(ident_part)
}

/// Every token of `text`, comments included. Errors on unterminated strings,
/// names or comments and on characters CQL does not allow outside strings.
pub(crate) fn lex(text: &str) -> Result<Vec<Token>, DbError> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if tokens.len() > MAX_TOKENS {
            return Err(DbError::ResultLimit);
        }
        let start = index;
        let byte = bytes[index];
        let rest = &bytes[index..];
        let kind = if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        } else if rest.starts_with(b"--") || rest.starts_with(b"//") {
            index = text[index..].find('\n').map_or(bytes.len(), |end| index + end);
            Kind::Comment
        } else if rest.starts_with(b"/*") {
            let end = text[index + 2..]
                .find("*/")
                .ok_or(DbError::InvalidRequest)?;
            index += 2 + end + 2;
            Kind::Comment
        } else if byte == b'\'' {
            index += 1;
            loop {
                match bytes.get(index) {
                    None => return Err(DbError::InvalidRequest),
                    Some(b'\'') if bytes.get(index + 1) == Some(&b'\'') => index += 2,
                    Some(b'\'') => {
                        index += 1;
                        break;
                    }
                    Some(_) => index += 1,
                }
            }
            Kind::Str
        } else if rest.starts_with(b"$$") {
            let end = text[index + 2..]
                .find("$$")
                .ok_or(DbError::InvalidRequest)?;
            index += 2 + end + 2;
            Kind::Str
        } else if byte == b'"' {
            index += 1;
            loop {
                match bytes.get(index) {
                    None => return Err(DbError::InvalidRequest),
                    Some(b'"') if bytes.get(index + 1) == Some(&b'"') => index += 2,
                    Some(b'"') => {
                        index += 1;
                        break;
                    }
                    Some(_) => index += 1,
                }
            }
            if index - start == 2 {
                return Err(DbError::InvalidRequest);
            }
            Kind::Quoted
        } else if uuid_at(rest) {
            index += 36;
            Kind::Uuid
        } else if (byte == b'0')
            && matches!(bytes.get(index + 1), Some(b'x' | b'X'))
            && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            index += 2;
            while bytes.get(index).is_some_and(u8::is_ascii_hexdigit) {
                index += 1;
            }
            if bytes.get(index).copied().is_some_and(ident_part) {
                return Err(DbError::InvalidRequest);
            }
            Kind::Blob
        } else if byte.is_ascii_digit()
            || (byte == b'.' && bytes.get(index + 1).is_some_and(u8::is_ascii_digit))
        {
            // Numbers, exponents and duration literals (`1h30m`, `1.5e-3`).
            index += 1;
            while let Some(&next) = bytes.get(index) {
                if ident_part(next) || next == b'.' {
                    index += 1;
                } else if matches!(next, b'+' | b'-')
                    && matches!(bytes[index - 1], b'e' | b'E')
                    && bytes.get(index + 1).is_some_and(u8::is_ascii_digit)
                {
                    index += 2;
                } else {
                    break;
                }
            }
            Kind::Number
        } else if ident_start(byte) {
            index += 1;
            while bytes.get(index).copied().is_some_and(ident_part) {
                index += 1;
            }
            Kind::Word
        } else if rest.starts_with(b"<=")
            || rest.starts_with(b">=")
            || rest.starts_with(b"!=")
            || rest.starts_with(b"+=")
            || rest.starts_with(b"-=")
        {
            index += 2;
            Kind::Punct
        } else if b"()[]{},;.:=<>+-*/?%!".contains(&byte) {
            index += 1;
            Kind::Punct
        } else {
            return Err(DbError::InvalidRequest);
        };
        tokens.push(Token {
            kind,
            start,
            end: index,
        });
    }
    Ok(tokens)
}

/// One statement: its code tokens (no comments) as indexes into the lexer
/// output, and whether a `;` closed it.
struct Piece {
    tokens: Vec<Token>,
}

fn word(text: &str, token: &Token) -> Option<String> {
    (token.kind == Kind::Word).then(|| text[token.start..token.end].to_ascii_uppercase())
}

fn is_punct(text: &str, token: Option<&Token>, value: &str) -> bool {
    token.is_some_and(|token| token.kind == Kind::Punct && &text[token.start..token.end] == value)
}

/// Splits code tokens into statements at `;`, keeping a `BEGIN … BATCH …
/// APPLY BATCH` together with the `;` between its statements.
fn split(text: &str, tokens: &[Token]) -> Result<Vec<Piece>, DbError> {
    let mut pieces = Vec::new();
    let mut current: Vec<Token> = Vec::new();
    let mut in_batch = false;
    for token in tokens.iter().filter(|token| token.kind != Kind::Comment) {
        if is_punct(text, Some(token), ";") && !in_batch {
            if !current.is_empty() {
                pieces.push(Piece {
                    tokens: std::mem::take(&mut current),
                });
            }
            continue;
        }
        current.push(*token);
        if current.len() == 1 && word(text, token).as_deref() == Some("BEGIN") {
            in_batch = true;
        }
        if in_batch && current.len() >= 2 {
            let last = current.len() - 1;
            if word(text, &current[last]).as_deref() == Some("BATCH")
                && word(text, &current[last - 1]).as_deref() == Some("APPLY")
            {
                in_batch = false;
            }
        }
    }
    if in_batch {
        return Err(DbError::InvalidRequest);
    }
    if !current.is_empty() {
        pieces.push(Piece { tokens: current });
    }
    if pieces.len() > MAX_STATEMENTS {
        return Err(DbError::ResultLimit);
    }
    Ok(pieces)
}

/// The executed text: code tokens with one space where the editor text had
/// whitespace or a comment between them, templates as `?`.
fn render(
    text: &str,
    tokens: &[Token],
    parameters: &[DbParameter],
    used: &mut BTreeSet<String>,
) -> Result<(String, Vec<DbParameter>), DbError> {
    let mut output = String::with_capacity(text.len());
    let mut bound = Vec::new();
    let mut previous_end: Option<usize> = None;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if let Some(end) = previous_end {
            if end < token.start {
                output.push(' ');
            }
        }
        if is_punct(text, Some(token), "{") && is_punct(text, tokens.get(index + 1), "{") {
            let name = tokens
                .get(index + 2)
                .filter(|token| token.kind == Kind::Word)
                .map(|token| &text[token.start..token.end])
                .ok_or(DbError::InvalidRequest)?;
            if !template::valid_name(name)
                || !is_punct(text, tokens.get(index + 3), "}")
                || !is_punct(text, tokens.get(index + 4), "}")
            {
                return Err(DbError::InvalidRequest);
            }
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.name == name)
                .ok_or(DbError::InvalidRequest)?;
            used.insert(parameter.name.clone());
            bound.push(parameter.clone());
            output.push('?');
            previous_end = Some(tokens[index + 4].end);
            index += 5;
            continue;
        }
        // Raw bind markers would shift the template values.
        if is_punct(text, Some(token), "?") {
            return Err(DbError::InvalidRequest);
        }
        output.push_str(&text[token.start..token.end]);
        previous_end = Some(token.end);
        index += 1;
    }
    Ok((output, bound))
}

/// A (keyspace, name) reference: unquoted names fold to lower case.
fn name_at(text: &str, tokens: &[Token], index: usize) -> Option<(Option<String>, String, usize)> {
    let part = |token: &Token| match token.kind {
        Kind::Word => Some(text[token.start..token.end].to_ascii_lowercase()),
        Kind::Quoted => Some(text[token.start + 1..token.end - 1].replace("\"\"", "\"")),
        _ => None,
    };
    let first = part(tokens.get(index)?)?;
    if is_punct(text, tokens.get(index + 1), ".") {
        let second = part(tokens.get(index + 2)?)?;
        Some((Some(first), second, index + 3))
    } else {
        Some((None, first, index + 1))
    }
}

/// Words at parenthesis depth 0 (upper case), with their token index.
fn top_words(text: &str, tokens: &[Token]) -> Vec<(usize, String)> {
    let mut depth = 0i32;
    let mut words = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match &text[token.start..token.end] {
            "(" | "[" | "{" if token.kind == Kind::Punct => depth += 1,
            ")" | "]" | "}" if token.kind == Kind::Punct => depth -= 1,
            _ if depth == 0 => {
                if let Some(word) = word(text, token) {
                    words.push((index, word));
                }
            }
            _ => {}
        }
    }
    words
}

/// What a plain `SELECT` reads, for editing and scan warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectShape {
    pub keyspace: Option<String>,
    pub table: String,
    pub has_where: bool,
    pub has_limit: bool,
    pub allow_filtering: bool,
    /// No DISTINCT, JSON, GROUP BY or aggregate-only projection.
    pub plain: bool,
}

fn select_shape(text: &str, tokens: &[Token]) -> Option<SelectShape> {
    let words = top_words(text, tokens);
    if words.first().map(|(_, word)| word.as_str()) != Some("SELECT") {
        return None;
    }
    let from = words.iter().find(|(_, word)| word == "FROM")?.0;
    let (keyspace, table, _) = name_at(text, tokens, from + 1)?;
    let has = |wanted: &str| words.iter().any(|(_, word)| word == wanted);
    let allow_filtering = words
        .windows(2)
        .any(|pair| pair[0].1 == "ALLOW" && pair[1].1 == "FILTERING" && pair[1].0 == pair[0].0 + 1);
    let projection: Vec<&String> = words
        .iter()
        .take_while(|(index, _)| *index < from)
        .map(|(_, word)| word)
        .collect();
    Some(SelectShape {
        keyspace,
        table,
        has_where: has("WHERE"),
        has_limit: has("LIMIT"),
        allow_filtering,
        plain: !projection
            .iter()
            .any(|word| matches!(word.as_str(), "DISTINCT" | "JSON"))
            && !words
                .windows(2)
                .any(|pair| pair[0].1 == "GROUP" && pair[1].1 == "BY"),
    })
}

/// The source of a single-table `SELECT` (None for anything else).
pub fn select_source(text: &str) -> Option<SelectShape> {
    let tokens = lex(text).ok()?;
    let pieces = split(text, &tokens).ok()?;
    let [piece] = pieces.as_slice() else {
        return None;
    };
    select_shape(text, &piece.tokens)
}

fn classify(text: &str, tokens: &[Token]) -> Result<(QueryRisk, &'static str), DbError> {
    let words: Vec<String> = tokens
        .iter()
        .take(8)
        .map(|token| word(text, token).unwrap_or_default())
        .collect();
    let at = |index: usize| words.get(index).map(String::as_str).unwrap_or("");
    let role_object = |name: &str| matches!(name, "ROLE" | "USER");
    Ok(match at(0) {
        "SELECT" => {
            let shape = select_shape(text, tokens).ok_or(DbError::InvalidRequest)?;
            let system = shape
                .keyspace
                .as_deref()
                .is_some_and(|keyspace| keyspace.starts_with("system"));
            let reason = if shape.allow_filtering {
                ALLOW_FILTERING
            } else if !shape.has_where && !shape.has_limit && !system {
                FULL_SCAN
            } else {
                READ
            };
            (QueryRisk::ReadOnly, reason)
        }
        "LIST" if matches!(at(1), "ROLES" | "USERS" | "PERMISSIONS" | "ALL")
            || matches!(at(1), "SELECT" | "MODIFY" | "CREATE" | "ALTER" | "DROP" | "AUTHORIZE" | "DESCRIBE" | "EXECUTE") =>
        {
            (QueryRisk::ReadOnly, READ)
        }
        "DESCRIBE" | "DESC" => (QueryRisk::ReadOnly, READ),
        "USE" => (QueryRisk::ReadOnly, USE),
        "INSERT" | "UPDATE" | "DELETE" => (QueryRisk::Write, WRITE),
        "BEGIN" => {
            batch(text, tokens)?;
            (QueryRisk::Write, BATCH)
        }
        "TRUNCATE" => (QueryRisk::Destructive, TRUNCATE),
        "GRANT" | "REVOKE" => (QueryRisk::Destructive, ROLE_MANAGEMENT),
        "DROP" if role_object(at(1)) => (QueryRisk::Destructive, ROLE_MANAGEMENT),
        "DROP" => (QueryRisk::Destructive, DROP),
        "CREATE" | "ALTER" => {
            // CREATE OR REPLACE FUNCTION / AGGREGATE
            let object = if at(1) == "OR" && at(2) == "REPLACE" {
                at(3)
            } else {
                at(1)
            };
            match object {
                _ if role_object(object) => (QueryRisk::Destructive, ROLE_MANAGEMENT),
                "FUNCTION" | "AGGREGATE" | "TRIGGER" => (QueryRisk::Destructive, CODE),
                "KEYSPACE" | "SCHEMA" if at(0) == "ALTER" => {
                    (QueryRisk::Destructive, ALTER_KEYSPACE)
                }
                "TABLE" | "COLUMNFAMILY"
                    if at(0) == "ALTER"
                        && top_words(text, tokens)
                            .iter()
                            .any(|(_, word)| word == "DROP" || word == "RENAME") =>
                {
                    (QueryRisk::Destructive, DROP_COLUMN)
                }
                "KEYSPACE" | "SCHEMA" | "TABLE" | "COLUMNFAMILY" | "INDEX" | "CUSTOM"
                | "MATERIALIZED" | "TYPE" => (QueryRisk::Write, DDL),
                _ => return Err(DbError::InvalidRequest),
            }
        }
        _ => return Err(DbError::InvalidRequest),
    })
}

/// `BEGIN [UNLOGGED | COUNTER | LOGGED] BATCH [USING …] stmt; … APPLY BATCH`
/// holds only INSERT, UPDATE and DELETE statements.
fn batch(text: &str, tokens: &[Token]) -> Result<(), DbError> {
    let words: Vec<Option<String>> = tokens.iter().map(|token| word(text, token)).collect();
    let mut index = 1;
    if matches!(
        words.get(index).cloned().flatten().as_deref(),
        Some("UNLOGGED" | "COUNTER" | "LOGGED")
    ) {
        index += 1;
    }
    if words.get(index).cloned().flatten().as_deref() != Some("BATCH") {
        return Err(DbError::InvalidRequest);
    }
    index += 1;
    let length = tokens.len();
    if length < index + 2
        || words[length - 2].as_deref() != Some("APPLY")
        || words[length - 1].as_deref() != Some("BATCH")
    {
        return Err(DbError::InvalidRequest);
    }
    // USING TIMESTAMP n before the first statement.
    if words.get(index).cloned().flatten().as_deref() == Some("USING") {
        index += 1;
        while index < length - 2
            && !matches!(
                words[index].as_deref(),
                Some("INSERT" | "UPDATE" | "DELETE")
            )
        {
            index += 1;
        }
    }
    let body = &tokens[index..length - 2];
    let mut statements = 0;
    let mut expect_start = true;
    for (offset, token) in body.iter().enumerate() {
        if is_punct(text, Some(token), ";") {
            expect_start = true;
            continue;
        }
        if expect_start {
            match words[index + offset].as_deref() {
                Some("INSERT" | "UPDATE" | "DELETE") => statements += 1,
                _ => return Err(DbError::InvalidRequest),
            }
            expect_start = false;
        }
    }
    if statements == 0 {
        return Err(DbError::InvalidRequest);
    }
    Ok(())
}

/// The keyspace of `USE ks` (exact, case-sensitive when quoted).
fn use_keyspace(text: &str, tokens: &[Token]) -> Result<String, DbError> {
    match tokens {
        [_, name] => match name.kind {
            Kind::Word => Ok(text[name.start..name.end].to_ascii_lowercase()),
            Kind::Quoted => Ok(text[name.start + 1..name.end - 1].replace("\"\"", "\"")),
            _ => Err(DbError::InvalidRequest),
        },
        _ => Err(DbError::InvalidRequest),
    }
}

pub(super) fn parse(request: &DbQueryRequest) -> Result<Vec<ClassifiedStatement>, DbError> {
    let text = request.text.as_str();
    let tokens = lex(text)?;
    let pieces = split(text, &tokens)?;
    let mut used = BTreeSet::new();
    let mut result = Vec::with_capacity(pieces.len());
    for piece in pieces {
        let (risk, reason) = classify(text, &piece.tokens)?;
        let keyspace = if reason == USE {
            Some(use_keyspace(text, &piece.tokens)?)
        } else {
            None
        };
        let (rendered, parameters) = render(text, &piece.tokens, &request.parameters, &mut used)?;
        result.push(ClassifiedStatement {
            parsed: ParsedStatement::Cql {
                text: rendered,
                parameters,
                keyspace,
            },
            risk,
            reason,
        });
    }
    if used.len() != request.parameters.len() {
        for statement in &mut result {
            if let ParsedStatement::Cql { text, .. } = &mut statement.parsed {
                text.zeroize();
            }
        }
        return Err(DbError::InvalidRequest);
    }
    Ok(result)
}

/// Template names used in `text`.
pub(crate) fn parameter_names(text: &str) -> Result<BTreeSet<String>, DbError> {
    let tokens: Vec<Token> = lex(text)?
        .into_iter()
        .filter(|token| token.kind != Kind::Comment)
        .collect();
    let mut names = BTreeSet::new();
    let mut index = 0;
    while index < tokens.len() {
        if is_punct(text, tokens.get(index), "{") && is_punct(text, tokens.get(index + 1), "{") {
            if let Some(name) = tokens
                .get(index + 2)
                .filter(|token| token.kind == Kind::Word)
                .map(|token| &text[token.start..token.end])
            {
                if template::valid_name(name)
                    && is_punct(text, tokens.get(index + 3), "}")
                    && is_punct(text, tokens.get(index + 4), "}")
                {
                    names.insert(name.to_owned());
                    index += 5;
                    continue;
                }
            }
            return Err(DbError::InvalidRequest);
        }
        index += 1;
    }
    Ok(names)
}

/// Editor pieces: (comment, delimiter, start, end). A `;` inside a batch is
/// part of the batch, not a delimiter.
pub(crate) fn pieces(text: &str) -> Result<Vec<(bool, bool, usize, usize)>, DbError> {
    let tokens = lex(text)?;
    let mut result = Vec::with_capacity(tokens.len());
    let mut statement: Vec<Option<String>> = Vec::new();
    let mut in_batch = false;
    for token in &tokens {
        if token.kind == Kind::Comment {
            result.push((true, false, token.start, token.end));
            continue;
        }
        let delimiter = is_punct(text, Some(token), ";") && !in_batch;
        if delimiter {
            statement.clear();
        } else {
            statement.push(word(text, token));
            if statement.len() == 1 && statement[0].as_deref() == Some("BEGIN") {
                in_batch = true;
            }
            let length = statement.len();
            if in_batch
                && length >= 2
                && statement[length - 2].as_deref() == Some("APPLY")
                && statement[length - 1].as_deref() == Some("BATCH")
            {
                in_batch = false;
            }
        }
        result.push((false, delimiter, token.start, token.end));
    }
    Ok(result)
}

/// A CQL identifier, quoted unless it is a plain lower-case name.
pub fn quote_name(name: &str) -> String {
    let plain = !name.is_empty()
        && name.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !RESERVED.contains(&name.to_ascii_uppercase().as_str());
    if plain {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// Reserved CQL keywords that cannot be used as unquoted names.
const RESERVED: &[&str] = &[
    "ADD", "ALLOW", "ALTER", "AND", "APPLY", "ASC", "AUTHORIZE", "BATCH", "BEGIN", "BY",
    "COLUMNFAMILY", "CREATE", "DELETE", "DESC", "DESCRIBE", "DROP", "ENTRIES", "EXECUTE", "FROM",
    "FULL", "GRANT", "IF", "IN", "INDEX", "INFINITY", "INSERT", "INTO", "IS", "KEYSPACE", "LIMIT",
    "MODIFY", "NAN", "NORECURSIVE", "NOT", "NULL", "OF", "ON", "OR", "ORDER", "PRIMARY", "RENAME",
    "REPLACE", "REVOKE", "SCHEMA", "SELECT", "SET", "TABLE", "TO", "TOKEN", "TRUNCATE",
    "UNLOGGED", "UPDATE", "USE", "USING", "VIEW", "WHERE", "WITH",
];

/// `keyspace.name` with each part quoted when needed.
pub fn qualified(keyspace: &str, name: &str) -> String {
    if keyspace.is_empty() {
        quote_name(name)
    } else {
        format!("{}.{}", quote_name(keyspace), quote_name(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DbCell;

    fn request(text: &str) -> DbQueryRequest {
        DbQueryRequest {
            text: text.into(),
            parameters: vec![],
            approval_ticket: None,
        }
    }

    fn classified(text: &str) -> Vec<(QueryRisk, &'static str, String)> {
        parse(&request(text))
            .unwrap()
            .into_iter()
            .map(|statement| {
                let ParsedStatement::Cql { text, .. } = &statement.parsed else {
                    panic!("not CQL");
                };
                (statement.risk, statement.reason, text.clone())
            })
            .collect()
    }

    #[test]
    fn statements_are_classified_by_their_leading_keywords() {
        for (text, risk, reason) in [
            ("SELECT * FROM ks.t WHERE id = 1", QueryRisk::ReadOnly, READ),
            ("select * from t limit 10", QueryRisk::ReadOnly, READ),
            ("SELECT * FROM ks.t", QueryRisk::ReadOnly, FULL_SCAN),
            ("SELECT count(*) FROM t", QueryRisk::ReadOnly, FULL_SCAN),
            ("SELECT * FROM system.local", QueryRisk::ReadOnly, READ),
            (
                "SELECT * FROM t WHERE v = 3 ALLOW FILTERING",
                QueryRisk::ReadOnly,
                ALLOW_FILTERING,
            ),
            ("DESCRIBE KEYSPACES", QueryRisk::ReadOnly, READ),
            ("LIST ROLES", QueryRisk::ReadOnly, READ),
            ("USE shop", QueryRisk::ReadOnly, USE),
            ("INSERT INTO t (a) VALUES (1)", QueryRisk::Write, WRITE),
            ("UPDATE t SET v = 1 WHERE a = 1 IF EXISTS", QueryRisk::Write, WRITE),
            ("DELETE FROM t WHERE a = 1", QueryRisk::Write, WRITE),
            (
                "BEGIN UNLOGGED BATCH INSERT INTO t (a) VALUES (1); UPDATE t SET v = 2 WHERE a = 1; APPLY BATCH",
                QueryRisk::Write,
                BATCH,
            ),
            ("CREATE TABLE t (a int PRIMARY KEY)", QueryRisk::Write, DDL),
            ("CREATE INDEX ON t (v)", QueryRisk::Write, DDL),
            ("CREATE MATERIALIZED VIEW mv AS SELECT * FROM t WHERE a IS NOT NULL PRIMARY KEY (a)", QueryRisk::Write, DDL),
            ("CREATE TYPE address (street text)", QueryRisk::Write, DDL),
            ("ALTER TABLE t ADD c int", QueryRisk::Write, DDL),
            ("ALTER TABLE t WITH gc_grace_seconds = 10", QueryRisk::Write, DDL),
            ("ALTER TABLE t DROP c", QueryRisk::Destructive, DROP_COLUMN),
            ("ALTER KEYSPACE ks WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}", QueryRisk::Destructive, ALTER_KEYSPACE),
            ("TRUNCATE t", QueryRisk::Destructive, TRUNCATE),
            ("DROP TABLE t", QueryRisk::Destructive, DROP),
            ("DROP KEYSPACE ks", QueryRisk::Destructive, DROP),
            ("CREATE OR REPLACE FUNCTION f (a int) RETURNS NULL ON NULL INPUT RETURNS int LANGUAGE java AS 'return a;'", QueryRisk::Destructive, CODE),
            ("CREATE ROLE r WITH PASSWORD = 'x'", QueryRisk::Destructive, ROLE_MANAGEMENT),
            ("ALTER USER u WITH PASSWORD 'x'", QueryRisk::Destructive, ROLE_MANAGEMENT),
            ("DROP ROLE r", QueryRisk::Destructive, ROLE_MANAGEMENT),
            ("GRANT SELECT ON KEYSPACE ks TO r", QueryRisk::Destructive, ROLE_MANAGEMENT),
            ("REVOKE SELECT ON KEYSPACE ks FROM r", QueryRisk::Destructive, ROLE_MANAGEMENT),
        ] {
            let result = classified(text);
            assert_eq!(result.len(), 1, "{text}");
            assert_eq!((result[0].0, result[0].1), (risk, reason), "{text}");
        }
    }

    #[test]
    fn unknown_and_malformed_statements_are_refused() {
        for text in [
            "SOMETHING ELSE",
            "CREATE SERVICE LEVEL sl",
            "BEGIN BATCH SELECT * FROM t; APPLY BATCH",
            "BEGIN BATCH INSERT INTO t (a) VALUES (1);",
            "BEGIN BATCH APPLY BATCH",
            "SELECT * FROM t WHERE a = ?",
            "SELECT 'unterminated",
            "SELECT * FROM t /* open",
            "SELECT \"\" FROM t",
            "SELECT * FROM t WHERE a = {{1x}}",
            "USE a b",
            "SELECT * FROM t WHERE a = 1 # x",
        ] {
            assert!(parse(&request(text)).is_err(), "{text}");
        }
    }

    #[test]
    fn comments_never_reach_the_server_and_strings_keep_their_bytes() {
        let result = classified(
            "/* DROP TABLE t; */ SELECT a -- DROP\n, 'x''; DROP TABLE t; --' , $$ ; DROP $$ FROM ks.t // tail\n WHERE a = 1; DELETE FROM t WHERE a = 2",
        );
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].1, READ);
        assert_eq!(
            result[0].2,
            "SELECT a , 'x''; DROP TABLE t; --' , $$ ; DROP $$ FROM ks.t WHERE a = 1"
        );
        assert_eq!(result[1].0, QueryRisk::Write);
        // Literals that are not separated keep their spelling.
        let result = classified("SELECT * FROM t WHERE d = 1h30m AND u = 123e4567-e89b-12d3-a456-426614174000 AND b = 0xCAFE AND f = -1.5e-3");
        assert_eq!(
            result[0].2,
            "SELECT * FROM t WHERE d = 1h30m AND u = 123e4567-e89b-12d3-a456-426614174000 AND b = 0xCAFE AND f = -1.5e-3"
        );
    }

    #[test]
    fn a_batch_keeps_its_inner_semicolons() {
        let result = classified(
            "BEGIN BATCH\nINSERT INTO t (a) VALUES (1);\nDELETE FROM t WHERE a = 2;\nAPPLY BATCH;\nSELECT * FROM t WHERE a = 1;",
        );
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].1, BATCH);
        assert_eq!(
            result[0].2,
            "BEGIN BATCH INSERT INTO t (a) VALUES (1); DELETE FROM t WHERE a = 2; APPLY BATCH"
        );
    }

    #[test]
    fn templates_become_ordered_bind_markers() {
        let parsed = parse(&DbQueryRequest {
            text: "UPDATE t SET v = {{value}}, m = {{ map }} WHERE a = {{key}} AND b = {{value}}"
                .into(),
            parameters: vec![
                DbParameter {
                    name: "key".into(),
                    value: DbCell::Int("1".into()),
                    type_hint: None,
                },
                DbParameter {
                    name: "value".into(),
                    value: DbCell::Text("x".into()),
                    type_hint: None,
                },
                DbParameter {
                    name: "map".into(),
                    value: DbCell::Json("{}".into()),
                    type_hint: None,
                },
            ],
            approval_ticket: None,
        })
        .unwrap();
        let ParsedStatement::Cql {
            text, parameters, ..
        } = &parsed[0].parsed
        else {
            panic!("not CQL");
        };
        assert_eq!(text, "UPDATE t SET v = ?, m = ? WHERE a = ? AND b = ?");
        let names: Vec<_> = parameters.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["value", "map", "key", "value"]);
        // An unused parameter is refused.
        assert!(parse(&DbQueryRequest {
            text: "SELECT * FROM t WHERE a = 1".into(),
            parameters: vec![DbParameter {
                name: "x".into(),
                value: DbCell::Null,
                type_hint: None
            }],
            approval_ticket: None,
        })
        .is_err());
        assert_eq!(
            parameter_names("SELECT * FROM t WHERE a = {{a}} AND b = '{{not}}' -- {{no}}")
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            ["a"]
        );
    }

    #[test]
    fn select_sources_and_use() {
        let shape = select_source("SELECT a, b FROM \"Shop\".items WHERE a = 1").unwrap();
        assert_eq!(shape.keyspace.as_deref(), Some("Shop"));
        assert_eq!(shape.table, "items");
        assert!(shape.plain && shape.has_where);
        assert!(!select_source("SELECT DISTINCT a FROM t").unwrap().plain);
        assert!(!select_source("SELECT JSON * FROM t").unwrap().plain);
        assert!(select_source("INSERT INTO t (a) VALUES (1)").is_none());
        assert!(select_source("SELECT * FROM a; SELECT * FROM b").is_none());
        let parsed = parse(&request("USE \"MyKs\"")).unwrap();
        let ParsedStatement::Cql { keyspace, .. } = &parsed[0].parsed else {
            panic!("not CQL");
        };
        assert_eq!(keyspace.as_deref(), Some("MyKs"));
    }

    #[test]
    fn names_are_quoted_only_when_needed() {
        assert_eq!(quote_name("items"), "items");
        assert_eq!(quote_name("Items"), "\"Items\"");
        assert_eq!(quote_name("select"), "\"select\"");
        assert_eq!(quote_name("a\"b"), "\"a\"\"b\"");
        assert_eq!(qualified("shop", "order items"), "shop.\"order items\"");
    }
}
