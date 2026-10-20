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
            let ranges = ranges(engine, text)?;
            let (start, end) = ranges
                .iter()
                .copied()
                .find(|(from, to)| start >= *from && start < *to)
                .or_else(|| ranges.last().copied().filter(|(_, to)| start == *to))
                .ok_or(DbError::InvalidRequest)?;
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

fn ranges(engine: DbEngine, text: &str) -> Result<Vec<(usize, usize)>, DbError> {
    let mut ends = Vec::new();
    match engine {
        DbEngine::Mongodb => {
            // Restricted JSON/collection expressions already enforce balanced
            // nesting and quote-aware semicolon boundaries.
            for part in super::mongo::split(text)? {
                let start = part.as_ptr() as usize - text.as_ptr() as usize;
                let end = start + part.len();
                let suffix = &text[end..];
                let whitespace = suffix.len() - suffix.trim_start().len();
                ends.push(if suffix.trim_start().starts_with(';') {
                    end + whitespace + 1
                } else {
                    end
                });
            }
        }
        DbEngine::Redis => {
            super::redis::tokenize(text)?;
            let mut quote = None;
            let mut escaped = false;
            let mut comment = false;
            let mut line_empty = true;
            for (offset, byte) in text.bytes().enumerate() {
                if comment {
                    if byte == b'\n' {
                        comment = false;
                        line_empty = true;
                        ends.push(offset + 1);
                    }
                    continue;
                }
                if line_empty && byte == b'#' {
                    comment = true;
                    continue;
                }
                if !byte.is_ascii_whitespace() {
                    line_empty = false;
                }
                if byte == b'\n' && quote.is_none() {
                    line_empty = true;
                }
                if escaped {
                    escaped = false;
                    continue;
                }
                if byte == b'\\' {
                    escaped = true;
                    continue;
                }
                match quote {
                    Some(current) if current == byte => quote = None,
                    Some(_) => {}
                    None if matches!(byte, b'\'' | b'"') => quote = Some(byte),
                    None if byte == b'\n' => ends.push(offset + 1),
                    _ => {}
                }
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
            for token in tokens {
                if token.token == Token::SemiColon {
                    ends.push(super::template::byte_offset(text, token.span.end)?);
                }
            }
        }
    }
    ends.push(text.len());
    ends.sort_unstable();
    ends.dedup();
    let mut start = 0;
    Ok(ends
        .into_iter()
        .map(|end| {
            let range = (start, end);
            start = end;
            range
        })
        .collect())
}
