use super::{template, ClassifiedStatement, ParsedStatement};
use crate::guard::QueryRisk;
use crate::types::{DbCell, DbError, DbParameter, DbQueryRequest};
use cc_models::database::DbEngine;
use sqlparser::ast::{
    AlterColumnOperation, AlterTableOperation, ArrayElemTypeDef, BinaryOperator, DataType,
    EnumMember, Expr, Ident, ObjectName, ObjectNamePart, Query, Select, Statement, TableFactor,
    Value, ValueWithSpan, VisitMut, VisitorMut,
};
use sqlparser::dialect::{
    ClickHouseDialect, Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect,
};
use sqlparser::parser::Parser;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;
use zeroize::Zeroizing;

pub(super) fn parse(
    engine: DbEngine,
    default_schema: &str,
    request: &DbQueryRequest,
) -> Result<Vec<ClassifiedStatement>, DbError> {
    if default_schema.is_empty() || default_schema.len() > 256 || default_schema.contains('\0') {
        return Err(DbError::InvalidRequest);
    }
    let dialect: Box<dyn Dialect> = match engine {
        DbEngine::Postgresql => Box::new(PostgreSqlDialect {}),
        DbEngine::Mysql | DbEngine::Mariadb => Box::new(MySqlDialect {}),
        DbEngine::Clickhouse => Box::new(ClickHouseDialect {}),
        DbEngine::Sqlite => Box::new(SQLiteDialect {}),
        _ => return Err(DbError::InvalidRequest),
    };
    let (text, bindings) =
        template::sql_templates(dialect.as_ref(), &request.text, &request.parameters)?;
    let text = if engine == DbEngine::Sqlite {
        super::sqlite::pragma_arguments(&text)?
    } else {
        text
    };
    let text = Zeroizing::new(text);
    let mut statements = Parser::new(dialect.as_ref())
        .with_recursion_limit(64)
        .try_with_sql(&text)
        .map_err(|_| DbError::InvalidRequest)?
        .parse_statements()
        .map_err(|_| DbError::InvalidRequest)?;
    if statements.len() > super::MAX_STATEMENTS {
        return Err(DbError::ResultLimit);
    }
    let mut result = Vec::with_capacity(statements.len());
    let mut all_used = BTreeSet::new();
    for statement in &mut statements {
        verify_rendering(engine, dialect.as_ref(), statement)?;
        let mut visitor = GuardVisitor {
            engine,
            default_schema,
            risk: QueryRisk::ReadOnly,
            reason: "sql_read",
            bindings: &bindings,
            parameters: Vec::new(),
            used: BTreeSet::new(),
            depth: 0,
            nodes: 0,
            scopes: Vec::new(),
            cte_scopes: BTreeMap::new(),
        };
        if let ControlFlow::Break(error) = statement.visit(&mut visitor) {
            return Err(error);
        }
        all_used.extend(visitor.used);
        spell(engine, statement);
        // Execute the inspected AST, never original text with conditional
        // comments or dialect-dependent uninspected trailing fragments.
        result.push(ClassifiedStatement {
            parsed: ParsedStatement::Sql {
                text: statement.to_string(),
                parameters: visitor.parameters,
            },
            risk: visitor.risk,
            reason: visitor.reason,
        });
    }
    if all_used.len() != bindings.len() {
        return Err(DbError::InvalidRequest);
    }
    Ok(result)
}

/// The executed text is the rendered AST. It must re-parse to exactly the
/// statement the guard inspected: sqlparser unescapes backslashes in MySQL and
/// ClickHouse literals but renders them back verbatim, so `'x\\', ' FROM t'`
/// would otherwise reach the server as `'x\', ' FROM t'` with a hidden clause.
fn verify_rendering(
    engine: DbEngine,
    dialect: &dyn Dialect,
    statement: &Statement,
) -> Result<(), DbError> {
    let mut probe = statement.clone();
    spell(engine, &mut probe);
    let text = Zeroizing::new(probe.to_string());
    let reparsed = Parser::new(dialect)
        .with_recursion_limit(64)
        .try_with_sql(&text)
        .map_err(|_| DbError::InvalidRequest)?
        .parse_statements()
        .map_err(|_| DbError::InvalidRequest)?;
    if reparsed.len() != 1 || reparsed[0] != *statement {
        return Err(DbError::InvalidRequest);
    }
    Ok(())
}

/// Renders literals and type names in the server's own spelling.
fn spell(engine: DbEngine, statement: &mut Statement) {
    if matches!(
        engine,
        DbEngine::Mysql | DbEngine::Mariadb | DbEngine::Clickhouse
    ) {
        let _ = statement.visit(&mut Spelling { engine });
    }
}

struct Spelling {
    engine: DbEngine,
}

impl VisitorMut for Spelling {
    type Break = ();

    fn pre_visit_value(&mut self, value: &mut ValueWithSpan) -> ControlFlow<()> {
        // Both servers treat a backslash in a quoted literal as an escape.
        if let Value::SingleQuotedString(text)
        | Value::DoubleQuotedString(text)
        | Value::NationalStringLiteral(text)
        | Value::SingleQuotedByteStringLiteral(text)
        | Value::DoubleQuotedByteStringLiteral(text) = &mut value.value
        {
            if text.contains('\\') {
                *text = text.replace('\\', "\\\\");
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<()> {
        if self.engine == DbEngine::Clickhouse {
            match expr {
                Expr::Cast { data_type, .. }
                | Expr::Convert {
                    data_type: Some(data_type),
                    ..
                } => clickhouse_type_spelling(data_type),
                Expr::TypedString(value) => clickhouse_type_spelling(&mut value.data_type),
                _ => {}
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_statement(&mut self, statement: &mut Statement) -> ControlFlow<()> {
        if self.engine != DbEngine::Clickhouse {
            return ControlFlow::Continue(());
        }
        match statement {
            Statement::CreateTable(create) => {
                for column in &mut create.columns {
                    clickhouse_type_spelling(&mut column.data_type);
                }
            }
            Statement::CreateView(view) => {
                for column in &mut view.columns {
                    if let Some(data_type) = &mut column.data_type {
                        clickhouse_type_spelling(data_type);
                    }
                }
            }
            Statement::AlterTable(alter) => {
                for operation in &mut alter.operations {
                    match operation {
                        AlterTableOperation::AddColumn { column_def, .. } => {
                            clickhouse_type_spelling(&mut column_def.data_type)
                        }
                        AlterTableOperation::ModifyColumn { data_type, .. }
                        | AlterTableOperation::ChangeColumn { data_type, .. }
                        | AlterTableOperation::AlterColumn {
                            op: AlterColumnOperation::SetDataType { data_type, .. },
                            ..
                        } => clickhouse_type_spelling(data_type),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

/// sqlparser renders `String`, `Float64`, `Int8`, `Int64` and `Enum8/16` in
/// upper case; ClickHouse type families are case-sensitive.
fn clickhouse_type_spelling(data_type: &mut DataType) {
    let name = match data_type {
        DataType::String(None) => "String",
        DataType::Float64 => "Float64",
        DataType::Int8(None) => "Int8",
        DataType::Int64 => "Int64",
        DataType::Enum(members, Some(bits @ (8 | 16))) => {
            let members = members
                .iter()
                .map(|member| match member {
                    EnumMember::Name(name) => clickhouse_string(name),
                    EnumMember::NamedValue(name, value) => {
                        format!("{} = {value}", clickhouse_string(name))
                    }
                })
                .collect();
            *data_type = DataType::Custom(
                ObjectName(vec![ObjectNamePart::Identifier(Ident::new(format!(
                    "Enum{bits}"
                )))]),
                members,
            );
            return;
        }
        DataType::Array(
            ArrayElemTypeDef::AngleBracket(inner)
            | ArrayElemTypeDef::SquareBracket(inner, _)
            | ArrayElemTypeDef::Parenthesis(inner),
        )
        | DataType::Nullable(inner)
        | DataType::LowCardinality(inner) => return clickhouse_type_spelling(inner),
        DataType::Map(key, value) => {
            clickhouse_type_spelling(key);
            return clickhouse_type_spelling(value);
        }
        DataType::Tuple(fields) => {
            for field in fields {
                clickhouse_type_spelling(&mut field.field_type);
            }
            return;
        }
        DataType::Nested(columns) => {
            for column in columns {
                clickhouse_type_spelling(&mut column.data_type);
            }
            return;
        }
        _ => return,
    };
    *data_type = DataType::Custom(
        ObjectName(vec![ObjectNamePart::Identifier(Ident::new(name))]),
        Vec::new(),
    );
}

fn clickhouse_string(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

struct GuardVisitor<'a> {
    engine: DbEngine,
    default_schema: &'a str,
    risk: QueryRisk,
    reason: &'static str,
    bindings: &'a BTreeMap<String, DbParameter>,
    parameters: Vec<DbParameter>,
    used: BTreeSet<String>,
    depth: usize,
    nodes: usize,
    scopes: Vec<BTreeSet<String>>,
    cte_scopes: BTreeMap<usize, BTreeSet<String>>,
}

impl GuardVisitor<'_> {
    fn risk(&mut self, risk: QueryRisk, reason: &'static str) {
        if risk > self.risk {
            self.risk = risk;
            self.reason = reason;
        }
    }

    fn function(&mut self, name: &mut ObjectName, table: bool) {
        let Some(parts) = name
            .0
            .iter()
            .map(|part| part.as_ident())
            .collect::<Option<Vec<_>>>()
        else {
            self.risk(QueryRisk::Destructive, "sql_dynamic_function");
            return;
        };
        let last = parts
            .last()
            .map(|ident| ident.value.as_str())
            .unwrap_or_default();
        let lower = last.to_ascii_lowercase();
        let canonical_quote = parts.iter().all(|ident| {
            ident.quote_style.is_none() || ident.value == ident.value.to_ascii_lowercase()
        });
        let qualified_builtin = self.engine == DbEngine::Postgresql
            && parts.len() == 2
            && parts[0].value == "pg_catalog";
        let safe = canonical_quote
            && (parts.len() == 1 || qualified_builtin)
            && safe_function(self.engine, &lower, table);
        if !safe {
            self.risk(
                QueryRisk::Destructive,
                "sql_unknown_or_side_effecting_function",
            );
            return;
        }
        if self.engine == DbEngine::Postgresql && !special_pg_function(&lower) {
            *name = ObjectName(vec![
                ObjectNamePart::Identifier(Ident::new("pg_catalog")),
                ObjectNamePart::Identifier(Ident::new(lower)),
            ]);
        }
    }
}

impl VisitorMut for GuardVisitor<'_> {
    type Break = DbError;

    fn pre_visit_statement(&mut self, statement: &mut Statement) -> ControlFlow<DbError> {
        if self.engine == DbEngine::Sqlite {
            match super::sqlite::statement(statement) {
                Some(Ok((risk, reason))) => {
                    self.risk(risk, reason);
                    return ControlFlow::Continue(());
                }
                Some(Err(error)) => return ControlFlow::Break(error),
                None => {}
            }
        }
        match statement {
            Statement::Query(_)
            | Statement::ExplainTable { .. }
            | Statement::ShowVariable { .. }
            | Statement::ShowVariables { .. }
            | Statement::ShowStatus { .. }
            | Statement::ShowCreate { .. }
            | Statement::ShowColumns { .. }
            | Statement::ShowDatabases { .. }
            | Statement::ShowSchemas { .. }
            | Statement::ShowTables { .. }
            | Statement::ShowViews { .. }
            | Statement::ShowFunctions { .. }
            | Statement::ShowCharset(_)
            | Statement::ShowCollation { .. } => {}
            Statement::Explain {
                analyze, options, ..
            } => {
                // Options can encode ANALYZE separately from the boolean field.
                if *analyze
                    || options.as_ref().is_some_and(|options| {
                        options.iter().any(|option| {
                            !matches!(
                                option.name.value.to_ascii_lowercase().as_str(),
                                "format" | "verbose" | "costs" | "settings" | "summary"
                            )
                        })
                    })
                {
                    self.risk(
                        QueryRisk::Destructive,
                        "sql_explain_executes_or_unknown_option",
                    );
                }
            }
            Statement::Insert(_) => self.risk(QueryRisk::Write, "sql_insert"),
            Statement::Update(update) => self.risk(
                if update.selection.is_none() {
                    QueryRisk::Destructive
                } else {
                    QueryRisk::Write
                },
                "sql_update",
            ),
            Statement::Delete(delete) => self.risk(
                if delete.selection.is_none() {
                    QueryRisk::Destructive
                } else {
                    QueryRisk::Write
                },
                "sql_delete",
            ),
            Statement::CreateTable(_)
            | Statement::CreateView { .. }
            | Statement::CreateIndex(_)
            | Statement::CreateSchema { .. }
            | Statement::CreateDatabase { .. }
            | Statement::CreateSequence { .. }
            | Statement::CreateFunction(_)
            | Statement::Drop { .. }
            | Statement::Truncate(_)
            | Statement::AlterTable(_)
            | Statement::AlterIndex { .. }
            | Statement::AlterView { .. } => self.risk(QueryRisk::Destructive, "sql_ddl"),
            // Session, role, transaction and wire-protocol controls must remain
            // driver-owned even outside production. Unknown variants fail closed.
            _ => return ControlFlow::Break(DbError::PolicyDenied),
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_query(&mut self, query: &mut Query) -> ControlFlow<DbError> {
        if !query.locks.is_empty() {
            self.risk(QueryRisk::Write, "sql_row_lock");
        }
        if query.settings.is_some()
            || query.format_clause.is_some()
            || query.for_clause.is_some()
            || !query.pipe_operators.is_empty()
        {
            return ControlFlow::Break(DbError::PolicyDenied);
        }
        let address = query as *const Query as usize;
        let inherited = self
            .cte_scopes
            .remove(&address)
            .unwrap_or_else(|| self.scopes.last().cloned().unwrap_or_default());
        let mut visible = inherited.clone();
        if let Some(with) = &query.with {
            let all: BTreeSet<_> = with
                .cte_tables
                .iter()
                .map(|cte| canonical_ident(&cte.alias.name))
                .collect();
            for cte in &with.cte_tables {
                let mut scope = visible.clone();
                if with.recursive {
                    scope.extend(all.iter().cloned());
                }
                self.cte_scopes
                    .insert(cte.query.as_ref() as *const Query as usize, scope);
                visible.insert(canonical_ident(&cte.alias.name));
            }
        }
        self.scopes.push(visible);
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &mut Query) -> ControlFlow<DbError> {
        self.scopes.pop();
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &mut Select) -> ControlFlow<DbError> {
        if select.into.is_some() {
            self.risk(QueryRisk::Destructive, "sql_select_into");
        }
        if !select.optimizer_hints.is_empty() {
            // Hints can change session/query limits, for example SET_VAR.
            return ControlFlow::Break(DbError::PolicyDenied);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, relation: &mut ObjectName) -> ControlFlow<DbError> {
        if relation.0.iter().any(|part| part.as_ident().is_none()) {
            return ControlFlow::Break(DbError::PolicyDenied);
        }
        if self.engine == DbEngine::Postgresql && relation.0.len() == 1 {
            let ident = relation.0[0].as_ident().expect("ident checked");
            let name = if ident.quote_style.is_some() {
                ident.value.clone()
            } else {
                ident.value.to_ascii_lowercase()
            };
            if !self
                .scopes
                .last()
                .is_some_and(|scope| scope.contains(&name))
            {
                relation.0.insert(
                    0,
                    ObjectNamePart::Identifier(Ident::with_quote('"', self.default_schema)),
                );
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, table: &mut TableFactor) -> ControlFlow<DbError> {
        if self.engine == DbEngine::Sqlite {
            if let TableFactor::Table {
                name,
                args: Some(_),
                ..
            }
            | TableFactor::Function { name, .. } = table
            {
                if super::sqlite::denied_function(name) {
                    return ControlFlow::Break(DbError::PolicyDenied);
                }
            }
        }
        match table {
            TableFactor::Table {
                name,
                args: Some(_),
                ..
            }
            | TableFactor::Function { name, .. } => self.function(name, true),
            TableFactor::Table { with_hints, .. } if !with_hints.is_empty() => {
                return ControlFlow::Break(DbError::PolicyDenied);
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<DbError> {
        self.depth += 1;
        self.nodes += 1;
        if self.depth > 128 || self.nodes > 50000 {
            return ControlFlow::Break(DbError::ResultLimit);
        }
        match expr {
            Expr::Function(function)
                if self.engine == DbEngine::Sqlite
                    && super::sqlite::denied_function(&function.name) =>
            {
                return ControlFlow::Break(DbError::PolicyDenied)
            }
            Expr::Function(function) => self.function(&mut function.name, false),
            Expr::BinaryOp {
                op: BinaryOperator::Assignment,
                ..
            } => return ControlFlow::Break(DbError::PolicyDenied),
            Expr::BinaryOp {
                op: BinaryOperator::Custom(_) | BinaryOperator::PGCustomBinaryOperator(_),
                ..
            } => {
                self.risk(QueryRisk::Destructive, "sql_custom_operator");
            }
            Expr::Cast { data_type, .. } if !safe_type(data_type) => {
                self.risk(QueryRisk::Destructive, "sql_custom_cast")
            }
            Expr::Convert {
                data_type: Some(data_type),
                ..
            } if !safe_type(data_type) => self.risk(QueryRisk::Destructive, "sql_custom_cast"),
            Expr::TypedString(value) if !safe_type(&value.data_type) => {
                self.risk(QueryRisk::Destructive, "sql_custom_cast")
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn post_visit_expr(&mut self, _expr: &mut Expr) -> ControlFlow<DbError> {
        self.depth -= 1;
        ControlFlow::Continue(())
    }

    fn pre_visit_value(&mut self, value: &mut ValueWithSpan) -> ControlFlow<DbError> {
        let Value::Placeholder(marker) = &mut value.value else {
            return ControlFlow::Continue(());
        };
        let Some(parameter) = self.bindings.get(marker) else {
            return ControlFlow::Break(DbError::InvalidRequest);
        };
        self.used.insert(marker.clone());
        let mut parameter = parameter.clone();
        let index = self.parameters.len() + 1;
        *marker = match self.engine {
            DbEngine::Postgresql => format!("${index}"),
            DbEngine::Mysql | DbEngine::Mariadb => "?".into(),
            DbEngine::Sqlite => format!("?{index}"),
            DbEngine::Clickhouse => {
                let Some(kind) = clickhouse_type(&parameter) else {
                    return ControlFlow::Break(DbError::InvalidRequest);
                };
                parameter.name = format!("ccp{index}");
                parameter.type_hint = Some(kind.into());
                format!("{{ccp{index}:{kind}}}")
            }
            _ => return ControlFlow::Break(DbError::InvalidRequest),
        };
        self.parameters.push(parameter);
        ControlFlow::Continue(())
    }
}

fn safe_type(data_type: &DataType) -> bool {
    match data_type {
        DataType::Custom(..) | DataType::Struct(..) | DataType::Union(..) => false,
        DataType::Array(ArrayElemTypeDef::AngleBracket(inner))
        | DataType::Array(ArrayElemTypeDef::SquareBracket(inner, _))
        | DataType::Array(ArrayElemTypeDef::Parenthesis(inner))
        | DataType::Nullable(inner)
        | DataType::LowCardinality(inner) => safe_type(inner),
        _ => true,
    }
}

fn clickhouse_type(parameter: &DbParameter) -> Option<&'static str> {
    match &parameter.value {
        DbCell::Null => Some("Nullable(String)"),
        DbCell::Bool(_) => Some("Bool"),
        DbCell::Int(_) => Some("Int64"),
        DbCell::Uint(_) => Some("UInt64"),
        DbCell::Float(_) => Some("Float64"),
        DbCell::Text(_) | DbCell::Binary(_) | DbCell::Json(_) => Some("String"),
        DbCell::Date(_) => Some("Date"),
        DbCell::Timestamp(_) => Some("DateTime64(6)"),
        DbCell::Time(_) => Some("String"),
        DbCell::Decimal(_) => match parameter.type_hint.as_deref() {
            Some("Decimal128(18)") => Some("Decimal128(18)"),
            Some("Decimal256(38)") => Some("Decimal256(38)"),
            _ => None,
        },
    }
}

fn special_pg_function(name: &str) -> bool {
    matches!(
        name,
        "coalesce"
            | "greatest"
            | "least"
            | "nullif"
            | "current_date"
            | "current_time"
            | "current_timestamp"
            | "localtime"
            | "localtimestamp"
            | "current_user"
            | "session_user"
            | "current_role"
    )
}

fn safe_function(engine: DbEngine, name: &str, table: bool) -> bool {
    if table {
        return match engine {
            DbEngine::Postgresql => matches!(
                name,
                "generate_series"
                    | "generate_subscripts"
                    | "unnest"
                    | "json_each"
                    | "jsonb_each"
                    | "json_each_text"
                    | "jsonb_each_text"
                    | "json_array_elements"
                    | "jsonb_array_elements"
                    | "json_array_elements_text"
                    | "jsonb_array_elements_text"
                    | "json_object_keys"
                    | "jsonb_object_keys"
                    | "regexp_split_to_table"
            ),
            DbEngine::Clickhouse => matches!(
                name,
                "numbers" | "numbers_mt" | "zeros" | "zeros_mt" | "values"
            ),
            DbEngine::Sqlite => super::sqlite::safe_table_function(name),
            _ => false,
        };
    }
    if engine == DbEngine::Sqlite {
        return super::sqlite::safe_function(name);
    }
    if matches!(
        name,
        "count"
            | "sum"
            | "avg"
            | "min"
            | "max"
            | "abs"
            | "ceil"
            | "ceiling"
            | "floor"
            | "round"
            | "sqrt"
            | "power"
            | "exp"
            | "ln"
            | "log"
            | "log10"
            | "sign"
            | "mod"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "atan2"
            | "pi"
            | "lower"
            | "upper"
            | "length"
            | "char_length"
            | "character_length"
            | "octet_length"
            | "concat"
            | "concat_ws"
            | "trim"
            | "ltrim"
            | "rtrim"
            | "replace"
            | "reverse"
            | "substring"
            | "substr"
            | "left"
            | "right"
            | "lpad"
            | "rpad"
            | "repeat"
            | "ascii"
            | "coalesce"
            | "greatest"
            | "least"
            | "nullif"
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
    ) {
        return true;
    }
    match engine {
        DbEngine::Postgresql => {
            special_pg_function(name)
                || matches!(
                    name,
                    "array_agg"
                        | "string_agg"
                        | "bool_and"
                        | "bool_or"
                        | "every"
                        | "bit_and"
                        | "bit_or"
                        | "stddev"
                        | "stddev_pop"
                        | "stddev_samp"
                        | "variance"
                        | "var_pop"
                        | "var_samp"
                        | "date_part"
                        | "date_trunc"
                        | "date_bin"
                        | "age"
                        | "now"
                        | "transaction_timestamp"
                        | "statement_timestamp"
                        | "clock_timestamp"
                        | "to_char"
                        | "to_date"
                        | "to_timestamp"
                        | "make_date"
                        | "make_time"
                        | "make_timestamp"
                        | "make_timestamptz"
                        | "make_interval"
                        | "json_build_object"
                        | "jsonb_build_object"
                        | "json_build_array"
                        | "jsonb_build_array"
                        | "json_agg"
                        | "jsonb_agg"
                        | "json_object_agg"
                        | "jsonb_object_agg"
                        | "to_json"
                        | "to_jsonb"
                        | "json_typeof"
                        | "jsonb_typeof"
                        | "json_array_length"
                        | "jsonb_array_length"
                        | "json_extract_path"
                        | "jsonb_extract_path"
                        | "json_extract_path_text"
                        | "jsonb_extract_path_text"
                        | "array_length"
                        | "array_ndims"
                        | "array_dims"
                        | "array_lower"
                        | "array_upper"
                        | "array_position"
                        | "array_positions"
                        | "array_remove"
                        | "array_replace"
                        | "array_append"
                        | "array_prepend"
                        | "array_cat"
                        | "array_to_string"
                        | "string_to_array"
                        | "cardinality"
                        | "regexp_match"
                        | "regexp_matches"
                        | "regexp_replace"
                        | "regexp_split_to_array"
                        | "split_part"
                        | "strpos"
                        | "btrim"
                        | "initcap"
                        | "translate"
                        | "quote_ident"
                        | "quote_literal"
                        | "quote_nullable"
                        | "encode"
                        | "decode"
                        | "md5"
                        | "sha224"
                        | "sha256"
                        | "sha384"
                        | "sha512"
                        | "current_database"
                        | "pg_typeof"
                        | "format_type"
                        | "pg_column_size"
                        | "pg_size_pretty"
                        | "pg_table_size"
                        | "pg_indexes_size"
                        | "pg_total_relation_size"
                        | "pg_relation_size"
                        // Read-only server diagnostics: session and server
                        // identity, WAL positions, catalog decompilers and
                        // sizes. None writes, signals, sleeps, locks for the
                        // session or touches files.
                        | "format"
                        | "version"
                        | "pg_backend_pid"
                        | "pg_blocking_pids"
                        | "pg_postmaster_start_time"
                        | "inet_server_addr"
                        | "inet_server_port"
                        | "pg_database_size"
                        | "pg_is_in_recovery"
                        | "pg_current_wal_lsn"
                        | "pg_last_wal_receive_lsn"
                        | "pg_last_wal_replay_lsn"
                        | "pg_last_xact_replay_timestamp"
                        | "pg_wal_lsn_diff"
                        | "col_description"
                        | "obj_description"
                        | "pg_get_constraintdef"
                        | "pg_get_indexdef"
                )
        }
        DbEngine::Mysql | DbEngine::Mariadb => matches!(
            name,
            "if" | "ifnull"
                | "isnull"
                | "group_concat"
                | "json_object"
                | "json_array"
                | "json_extract"
                | "json_unquote"
                | "json_quote"
                | "json_valid"
                | "json_type"
                | "json_length"
                | "json_keys"
                | "json_contains"
                | "json_contains_path"
                | "json_search"
                | "json_arrayagg"
                | "json_objectagg"
                | "date"
                | "time"
                | "year"
                | "month"
                | "day"
                | "dayofmonth"
                | "dayofweek"
                | "dayofyear"
                | "hour"
                | "minute"
                | "second"
                | "microsecond"
                | "week"
                | "weekday"
                | "quarter"
                | "date_format"
                | "str_to_date"
                | "datediff"
                | "timediff"
                | "timestampdiff"
                | "timestampadd"
                | "date_add"
                | "date_sub"
                | "adddate"
                | "subdate"
                | "last_day"
                | "curdate"
                | "curtime"
                | "now"
                | "current_date"
                | "current_time"
                | "current_timestamp"
                | "utc_date"
                | "utc_time"
                | "utc_timestamp"
                | "from_unixtime"
                | "unix_timestamp"
                | "locate"
                | "instr"
                | "substring_index"
                | "find_in_set"
                | "format"
                | "hex"
                | "unhex"
                | "to_base64"
                | "from_base64"
                | "md5"
                | "sha1"
                | "sha2"
                | "crc32"
                | "database"
                | "version"
                | "connection_id"
                | "json_value"
                | "regexp_like"
                | "regexp_replace"
                | "regexp_substr"
        ),
        DbEngine::Clickhouse => matches!(
            name,
            "countif"
                | "sumif"
                | "avgif"
                | "minif"
                | "maxif"
                | "uniq"
                | "uniqexact"
                | "uniqcombined"
                | "uniqhll12"
                | "quantile"
                | "quantiles"
                | "median"
                | "grouparray"
                | "groupuniqarray"
                | "argmin"
                | "argmax"
                | "any"
                | "anylast"
                | "anyheavy"
                | "countdistinct"
                | "tostring"
                | "toint8"
                | "toint16"
                | "toint32"
                | "toint64"
                | "touint8"
                | "touint16"
                | "touint32"
                | "touint64"
                | "tofloat32"
                | "tofloat64"
                | "todate"
                | "todatetime"
                | "todatetime64"
                | "todate32"
                | "todecimal32"
                | "todecimal64"
                | "todecimal128"
                | "toyear"
                | "tomonth"
                | "todayofmonth"
                | "todayofweek"
                | "tohour"
                | "tominute"
                | "tosecond"
                | "tostartofday"
                | "tostartofmonth"
                | "tostartofyear"
                | "tostartofhour"
                | "formatdatetime"
                | "parsedatetimebesteffort"
                | "datediff"
                | "dateadd"
                | "today"
                | "yesterday"
                | "now"
                | "now64"
                | "arrayjoin"
                | "arraymap"
                | "arrayfilter"
                | "arrayexists"
                | "arrayall"
                | "arraycount"
                | "arrayfirst"
                | "arrayelement"
                | "arraylength"
                | "arraydistinct"
                | "arraysort"
                | "arrayreversesort"
                | "arrayreduce"
                | "arrayzip"
                | "arrayenumerate"
                | "arrayconcat"
                | "arraypushback"
                | "arraypushfront"
                | "arraypopback"
                | "arraypopfront"
                | "arrayslice"
                | "arrayflatten"
                | "arrayintersect"
                | "arraydifference"
                | "arraycumsum"
                | "has"
                | "hasall"
                | "hasany"
                | "indexof"
                | "empty"
                | "notempty"
                | "splitbychar"
                | "splitbystring"
                | "arraystringconcat"
                | "position"
                | "positioncaseinsensitive"
                | "match"
                | "extract"
                | "extractall"
                | "replaceregexpone"
                | "replaceregexpall"
                | "replaceall"
                | "replaceone"
                | "jsonextract"
                | "jsonextractstring"
                | "jsonextractint"
                | "jsonextractfloat"
                | "jsonextractbool"
                | "jsonextractraw"
                | "jsonhas"
                | "isvalidjson"
                | "map"
                | "mapkeys"
                | "mapvalues"
                | "mapcontains"
                | "tuple"
                | "tupleelement"
                | "if"
                | "multiif"
                | "isnull"
                | "ifnull"
                | "assumenotnull"
                | "tonullable"
                | "siphash64"
                | "cityhash64"
                | "xxhash64"
                | "hex"
                | "unhex"
                | "base64encode"
                | "base64decode"
                | "isfinite"
                | "isnan"
                // Pure formatting and server identity (no I/O).
                | "formatreadablesize"
                | "formatreadablequantity"
                | "formatreadabletimedelta"
                | "uptime"
                | "version"
                | "hostname"
                | "tostartofminute"
                | "tostartoffiveminutes"
                | "tostartoffifteenminutes"
                | "tostartofweek"
                | "tostartofquarter"
        ),
        _ => false,
    }
}

fn canonical_ident(ident: &Ident) -> String {
    if ident.quote_style.is_some() {
        ident.value.clone()
    } else {
        ident.value.to_ascii_lowercase()
    }
}
