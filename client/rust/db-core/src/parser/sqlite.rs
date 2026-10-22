//! SQLite rules of the SQL guard: PRAGMA classification, statements that can
//! reach other files (ATTACH, VACUUM INTO) and functions that touch the file
//! system or load code. The native driver repeats the decisive checks in an
//! engine authorizer, and the remote `sqlite3` route runs `-safe`.
use crate::guard::QueryRisk;
use crate::types::DbError;
use sqlparser::ast::{ObjectName, Statement, Value};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::tokenizer::{Token, Tokenizer};

/// What a PRAGMA does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PragmaKind {
    /// Reports state (`PRAGMA table_info('t')`, `PRAGMA page_count`).
    Read,
    /// Changes the file or the connection (`PRAGMA user_version = 3`).
    Write,
    /// Never allowed: encryption keys, read-only switches, schema writes.
    Denied,
}

/// Pragmas that only report, without or with an argument.
const READ_PRAGMAS: &[&str] = &[
    "application_id",
    "auto_vacuum",
    "automatic_index",
    "busy_timeout",
    "cache_size",
    "cache_spill",
    "case_sensitive_like",
    "cell_size_check",
    "checkpoint_fullfsync",
    "collation_list",
    "compile_options",
    "data_version",
    "database_list",
    "defer_foreign_keys",
    "encoding",
    "foreign_key_check",
    "foreign_key_list",
    "foreign_keys",
    "freelist_count",
    "fullfsync",
    "function_list",
    "hard_heap_limit",
    "ignore_check_constraints",
    "index_info",
    "index_list",
    "index_xinfo",
    "integrity_check",
    "journal_mode",
    "journal_size_limit",
    "legacy_alter_table",
    "locking_mode",
    "max_page_count",
    "mmap_size",
    "module_list",
    "page_count",
    "page_size",
    "pragma_list",
    "query_only",
    "quick_check",
    "read_uncommitted",
    "recursive_triggers",
    "reverse_unordered_selects",
    "schema_version",
    "secure_delete",
    "soft_heap_limit",
    "stats",
    "synchronous",
    "table_info",
    "table_list",
    "table_xinfo",
    "temp_store",
    "threads",
    "user_version",
    "wal_autocheckpoint",
];

/// Pragmas whose argument names an object or bounds a check (a read).
const READ_WITH_ARGUMENT: &[&str] = &[
    "foreign_key_check",
    "foreign_key_list",
    "index_info",
    "index_list",
    "index_xinfo",
    "integrity_check",
    "quick_check",
    "table_info",
    "table_list",
    "table_xinfo",
];

/// Pragmas that are never run for a request: SQLCipher keys (they would
/// encrypt or re-key the user's file), the read-only switch the driver owns,
/// schema writes, trust and directory settings.
fn denied_pragma(name: &str) -> bool {
    matches!(
        name,
        "key"
            | "rekey"
            | "hexkey"
            | "hexrekey"
            | "textkey"
            | "textrekey"
            | "query_only"
            | "writable_schema"
            | "trusted_schema"
            | "temp_store_directory"
            | "data_store_directory"
            | "parser_trace"
            | "vdbe_trace"
            | "vdbe_listing"
            | "vdbe_debug"
            | "vdbe_addoptrace"
            | "sql_trace"
            | "default_cache_size"
    ) || name.starts_with("cipher")
        || name.starts_with("sqlcipher")
}

/// Classifies `PRAGMA [schema.]name [= value | (argument)]`. `argument` is
/// the parenthesized form, `assigned` the `=` form.
pub fn pragma_kind(name: &str, argument: bool, assigned: bool) -> PragmaKind {
    let name = name.to_ascii_lowercase();
    if denied_pragma(&name) && (assigned || argument || name != "query_only") {
        return PragmaKind::Denied;
    }
    if assigned {
        return PragmaKind::Write;
    }
    if argument {
        return if READ_WITH_ARGUMENT.contains(&name.as_str()) {
            PragmaKind::Read
        } else {
            PragmaKind::Write
        };
    }
    if READ_PRAGMAS.contains(&name.as_str()) {
        PragmaKind::Read
    } else {
        // Actions without a value: optimize, shrink_memory, wal_checkpoint,
        // incremental_vacuum, and anything unknown.
        PragmaKind::Write
    }
}

/// SQLite-specific statements; None leaves the statement to the shared rules.
pub(super) fn statement(
    statement: &Statement,
) -> Option<Result<(QueryRisk, &'static str), DbError>> {
    Some(match statement {
        Statement::Pragma { name, value, is_eq } => {
            if value
                .as_ref()
                .is_some_and(|value| matches!(value.value, Value::Placeholder(_)))
            {
                // PRAGMA cannot bind parameters.
                return Some(Err(DbError::InvalidRequest));
            }
            let Some(last) = name.0.last().and_then(|part| part.as_ident()) else {
                return Some(Err(DbError::PolicyDenied));
            };
            if name.0.len() > 2 {
                return Some(Err(DbError::PolicyDenied));
            }
            match pragma_kind(&last.value, value.is_some() && !is_eq, *is_eq) {
                PragmaKind::Read => Ok((QueryRisk::ReadOnly, "sqlite_pragma_read")),
                PragmaKind::Write => Ok((QueryRisk::Write, "sqlite_pragma_write")),
                PragmaKind::Denied => Err(DbError::PolicyDenied),
            }
        }
        // ATTACH opens (or creates) another file; DETACH is its pair.
        Statement::AttachDatabase { .. }
        | Statement::AttachDuckDBDatabase { .. }
        | Statement::DetachDuckDBDatabase { .. } => Err(DbError::PolicyDenied),
        // `VACUUM INTO` does not parse and stays refused; plain VACUUM
        // rewrites the whole file.
        Statement::Vacuum(vacuum)
            if !vacuum.full
                && !vacuum.sort_only
                && !vacuum.delete_only
                && !vacuum.reindex
                && !vacuum.recluster
                && vacuum.threshold.is_none()
                && !vacuum.boost =>
        {
            Ok((QueryRisk::Destructive, "sqlite_vacuum"))
        }
        Statement::Vacuum(_) => Err(DbError::PolicyDenied),
        Statement::Analyze(_) => Ok((QueryRisk::Write, "sqlite_analyze")),
        Statement::CreateTrigger(_)
        | Statement::DropTrigger(_)
        | Statement::CreateVirtualTable { .. } => Ok((QueryRisk::Destructive, "sql_ddl")),
        _ => return None,
    })
}

/// Functions that read or write files, load code or export encrypted
/// copies. Refused outright, even after an approval.
pub(crate) fn denied_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "load_extension"
            | "readfile"
            | "writefile"
            | "edit"
            | "lsdir"
            | "fsdir"
            | "zipfile"
            | "sqlar_compress"
            | "sqlar_uncompress"
            | "fts3_tokenizer"
            | "sqlcipher_export"
            | "sqlcipher_export_ex"
            | "sqlite_dbpage"
            | "sha3_query"
            | "shell_add_schema"
            | "shell_module_schema"
            | "shell_escape_crnl"
            | "shell_putsnl"
            | "shell_idquote"
            | "eval"
    )
}

pub(super) fn denied_function(name: &ObjectName) -> bool {
    name.0
        .last()
        .and_then(|part| part.as_ident())
        .is_some_and(|ident| denied_name(&ident.value))
}

/// Built-in scalar, aggregate, window, date and JSON functions.
pub(super) fn safe_function(name: &str) -> bool {
    matches!(
        name,
        "abs"
            | "char"
            | "coalesce"
            | "concat"
            | "concat_ws"
            | "format"
            | "glob"
            | "hex"
            | "ifnull"
            | "iif"
            | "if"
            | "instr"
            | "length"
            | "like"
            | "likelihood"
            | "likely"
            | "lower"
            | "ltrim"
            | "max"
            | "min"
            | "nullif"
            | "octet_length"
            | "printf"
            | "quote"
            | "random"
            | "randomblob"
            | "replace"
            | "round"
            | "rtrim"
            | "sign"
            | "soundex"
            | "sqlite_version"
            | "sqlite_source_id"
            | "substr"
            | "substring"
            | "trim"
            | "typeof"
            | "subtype"
            | "unhex"
            | "unicode"
            | "unistr"
            | "unistr_quote"
            | "unlikely"
            | "upper"
            | "zeroblob"
            | "changes"
            | "total_changes"
            | "last_insert_rowid"
            | "avg"
            | "count"
            | "group_concat"
            | "string_agg"
            | "sum"
            | "total"
            | "median"
            | "percentile"
            | "percentile_cont"
            | "percentile_disc"
            | "row_number"
            | "rank"
            | "dense_rank"
            | "percent_rank"
            | "cume_dist"
            | "ntile"
            | "lag"
            | "lead"
            | "first_value"
            | "last_value"
            | "nth_value"
            | "date"
            | "time"
            | "datetime"
            | "julianday"
            | "unixepoch"
            | "strftime"
            | "timediff"
            | "acos"
            | "acosh"
            | "asin"
            | "asinh"
            | "atan"
            | "atan2"
            | "atanh"
            | "ceil"
            | "ceiling"
            | "cos"
            | "cosh"
            | "degrees"
            | "exp"
            | "floor"
            | "ln"
            | "log"
            | "log10"
            | "log2"
            | "mod"
            | "pi"
            | "pow"
            | "power"
            | "radians"
            | "sin"
            | "sinh"
            | "sqrt"
            | "tan"
            | "tanh"
            | "trunc"
            | "json"
            | "jsonb"
            | "json_array"
            | "jsonb_array"
            | "json_array_length"
            | "json_error_position"
            | "json_extract"
            | "jsonb_extract"
            | "json_insert"
            | "jsonb_insert"
            | "json_object"
            | "jsonb_object"
            | "json_patch"
            | "jsonb_patch"
            | "json_pretty"
            | "json_remove"
            | "jsonb_remove"
            | "json_replace"
            | "jsonb_replace"
            | "json_set"
            | "jsonb_set"
            | "json_type"
            | "json_valid"
            | "json_quote"
            | "json_group_array"
            | "jsonb_group_array"
            | "json_group_object"
            | "jsonb_group_object"
            | "sqlite_compileoption_used"
            | "sqlite_compileoption_get"
    )
}

/// Table-valued functions that only read.
pub(super) fn safe_table_function(name: &str) -> bool {
    matches!(
        name,
        "json_each"
            | "json_tree"
            | "jsonb_each"
            | "jsonb_tree"
            | "generate_series"
            | "pragma_table_info"
            | "pragma_table_xinfo"
            | "pragma_table_list"
            | "pragma_index_list"
            | "pragma_index_info"
            | "pragma_index_xinfo"
            | "pragma_foreign_key_list"
            | "pragma_database_list"
            | "pragma_collation_list"
            | "pragma_function_list"
            | "pragma_module_list"
            | "pragma_pragma_list"
            | "pragma_compile_options"
            | "pragma_page_count"
            | "pragma_page_size"
            | "pragma_freelist_count"
    )
}

/// `PRAGMA table_info(users)` names its argument without quotes; the SQL
/// parser accepts only literals there, so a bare word inside the
/// parentheses of a PRAGMA becomes the equivalent string literal.
pub(super) fn pragma_arguments(text: &str) -> Result<String, DbError> {
    let tokens: Vec<_> = Tokenizer::new(&SQLiteDialect {}, text)
        .tokenize_with_location()
        .map_err(|_| DbError::InvalidRequest)?
        .into_iter()
        .filter(|token| !matches!(token.token, Token::Whitespace(_)))
        .collect();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut index = 0;
    while index < tokens.len() {
        let is_pragma = matches!(&tokens[index].token, Token::Word(word)
            if word.quote_style.is_none() && word.value.eq_ignore_ascii_case("pragma"));
        if !is_pragma {
            index += 1;
            continue;
        }
        // [schema .] name ( word )
        let mut at = index + 1;
        if matches!(tokens.get(at + 1).map(|t| &t.token), Some(Token::Period)) {
            at += 2;
        }
        let (Some(Token::Word(_)), Some(Token::LParen), Some(Token::Word(word)), Some(Token::RParen)) = (
            tokens.get(at).map(|t| &t.token),
            tokens.get(at + 1).map(|t| &t.token),
            tokens.get(at + 2).map(|t| &t.token),
            tokens.get(at + 3).map(|t| &t.token),
        ) else {
            index += 1;
            continue;
        };
        let start = super::template::byte_offset(text, tokens[at + 2].span.start)?;
        let end = super::template::byte_offset(text, tokens[at + 2].span.end)?;
        if start < cursor || end < start {
            return Err(DbError::InvalidRequest);
        }
        output.push_str(&text[cursor..start]);
        output.push('\'');
        output.push_str(&word.value.replace('\'', "''"));
        output.push('\'');
        cursor = end;
        index = at + 4;
    }
    output.push_str(&text[cursor..]);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pragmas_split_into_reads_writes_and_refusals() {
        assert_eq!(pragma_kind("table_info", true, false), PragmaKind::Read);
        assert_eq!(pragma_kind("PAGE_COUNT", false, false), PragmaKind::Read);
        assert_eq!(pragma_kind("query_only", false, false), PragmaKind::Read);
        assert_eq!(pragma_kind("user_version", false, true), PragmaKind::Write);
        assert_eq!(pragma_kind("journal_mode", false, true), PragmaKind::Write);
        assert_eq!(pragma_kind("optimize", false, false), PragmaKind::Write);
        assert_eq!(pragma_kind("wal_checkpoint", true, false), PragmaKind::Write);
        assert_eq!(pragma_kind("query_only", false, true), PragmaKind::Denied);
        assert_eq!(pragma_kind("writable_schema", false, true), PragmaKind::Denied);
        assert_eq!(pragma_kind("rekey", false, true), PragmaKind::Denied);
        assert_eq!(pragma_kind("key", false, false), PragmaKind::Denied);
        assert_eq!(pragma_kind("cipher_compatibility", false, true), PragmaKind::Denied);
    }

    #[test]
    fn bare_pragma_arguments_become_literals_outside_strings_only() {
        assert_eq!(
            pragma_arguments("PRAGMA table_info(users); PRAGMA main.index_list(\"t\")").unwrap(),
            "PRAGMA table_info('users'); PRAGMA main.index_list('t')"
        );
        assert_eq!(
            pragma_arguments("SELECT 'PRAGMA table_info(x)'").unwrap(),
            "SELECT 'PRAGMA table_info(x)'"
        );
        assert_eq!(
            pragma_arguments("PRAGMA table_info('a''b')").unwrap(),
            "PRAGMA table_info('a''b')"
        );
    }
}
