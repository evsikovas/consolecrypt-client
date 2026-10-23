use crate::types::{DbError, DbParameter};
use sqlparser::dialect::Dialect;
use sqlparser::tokenizer::{Location, Token, Tokenizer};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Uses lexer spans to replace only unquoted template tokens. Original string
/// and comment bytes are never reconstructed from token Display output.
pub(super) fn sql_templates(
    dialect: &dyn Dialect,
    text: &str,
    parameters: &[DbParameter],
) -> Result<(String, BTreeMap<String, DbParameter>), DbError> {
    let tokens: Vec<_> = Tokenizer::new(dialect, text)
        .tokenize_with_location()
        .map_err(|_| DbError::InvalidRequest)?
        .into_iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_)))
        .collect();
    if tokens.len() > 50000 {
        return Err(DbError::ResultLimit);
    }
    let nonce = Uuid::new_v4().simple().to_string();
    let by_name: BTreeMap<_, _> = parameters
        .iter()
        .map(|value| (value.name.as_str(), value))
        .collect();
    let mut bindings = BTreeMap::new();
    let mut used = BTreeSet::new();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index].token == Token::LBrace
            && tokens
                .get(index + 1)
                .is_some_and(|token| token.token == Token::LBrace)
        {
            let Some(Token::Word(word)) = tokens.get(index + 2).map(|token| &token.token) else {
                return Err(DbError::InvalidRequest);
            };
            if word.quote_style.is_some()
                || !valid_name(&word.value)
                || !tokens
                    .get(index + 3)
                    .is_some_and(|token| token.token == Token::RBrace)
                || !tokens
                    .get(index + 4)
                    .is_some_and(|token| token.token == Token::RBrace)
            {
                return Err(DbError::InvalidRequest);
            }
            let parameter = by_name
                .get(word.value.as_str())
                .ok_or(DbError::InvalidRequest)?;
            let marker = format!("__cc_{nonce}_{}", bindings.len());
            let start = byte_offset(text, tokens[index].span.start)?;
            let end = byte_offset(text, tokens[index + 4].span.end)?;
            if start < cursor || end < start {
                return Err(DbError::InvalidRequest);
            }
            output.push_str(&text[cursor..start]);
            output.push(':');
            output.push_str(&marker);
            cursor = end;
            used.insert(word.value.clone());
            bindings.insert(format!(":{marker}"), (*parameter).clone());
            index += 5;
        } else {
            index += 1;
        }
    }
    if used.len() != parameters.len() {
        return Err(DbError::InvalidRequest);
    }
    output.push_str(&text[cursor..]);
    Ok((output, bindings))
}

pub(crate) fn byte_offset(text: &str, location: Location) -> Result<usize, DbError> {
    let mut line = 1;
    let mut column = 1;
    for (index, character) in text.char_indices() {
        if line == location.line && column == location.column {
            return Ok(index);
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    if line == location.line && column == location.column {
        Ok(text.len())
    } else {
        Err(DbError::InvalidRequest)
    }
}
