use super::{ClassifiedStatement, ParsedStatement, MAX_STATEMENTS};
use crate::guard::QueryRisk;
use crate::types::{DbCell, DbError, DbParameter, DbQueryRequest};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fmt;
use zeroize::Zeroize;

/// The written key order of every object in a command. `serde_json` maps sort
/// their keys, but MongoDB reads sort specifications, index keys, embedded
/// document equality and inserted documents in the order they were written,
/// and expects the command name first. Parsed from the same text as the
/// classified command; duplicate keys are refused so that what the guard
/// classified is exactly what the driver sends.
#[derive(Clone, Default, PartialEq, Eq)]
pub enum KeyOrder {
    /// A scalar, or a position whose order is irrelevant.
    #[default]
    Leaf,
    Object(Vec<(String, KeyOrder)>),
    Array(Vec<KeyOrder>),
}

impl KeyOrder {
    fn object<const N: usize>(entries: [(&str, KeyOrder); N]) -> Self {
        Self::Object(
            entries
                .into_iter()
                .map(|(key, order)| (key.to_owned(), order))
                .collect(),
        )
    }

    fn set(&mut self, key: &str, order: KeyOrder) {
        if let Self::Object(entries) = self {
            match entries.iter_mut().find(|(name, _)| name == key) {
                Some(entry) => entry.1 = order,
                None => entries.push((key.to_owned(), order)),
            }
        }
    }

    /// Order of the child object or array stored under `key`.
    pub fn child(&self, key: &str) -> Option<&KeyOrder> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, order)| order),
            _ => None,
        }
    }

    /// Keys of this object in written order (empty for leaves and arrays).
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        let entries: &[(String, KeyOrder)] = match self {
            Self::Object(entries) => entries,
            _ => &[],
        };
        entries.iter().map(|(name, _)| name.as_str())
    }

    /// Order of the array element at `index`.
    pub fn element(&self, index: usize) -> Option<&KeyOrder> {
        match self {
            Self::Array(items) => items.get(index),
            _ => None,
        }
    }
}

impl Zeroize for KeyOrder {
    fn zeroize(&mut self) {
        match self {
            Self::Leaf => {}
            Self::Object(entries) => {
                for (key, order) in entries.iter_mut() {
                    key.zeroize();
                    order.zeroize();
                }
                entries.clear();
            }
            Self::Array(items) => {
                items.iter_mut().for_each(Zeroize::zeroize);
                items.clear();
            }
        }
    }
}

impl fmt::Debug for KeyOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeyOrder([REDACTED])")
    }
}

impl<'de> Deserialize<'de> for KeyOrder {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Order;
        impl<'de> Visitor<'de> for Order {
            type Value = KeyOrder;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_bool<E>(self, _: bool) -> Result<KeyOrder, E> {
                Ok(KeyOrder::Leaf)
            }
            fn visit_i64<E>(self, _: i64) -> Result<KeyOrder, E> {
                Ok(KeyOrder::Leaf)
            }
            fn visit_u64<E>(self, _: u64) -> Result<KeyOrder, E> {
                Ok(KeyOrder::Leaf)
            }
            fn visit_f64<E>(self, _: f64) -> Result<KeyOrder, E> {
                Ok(KeyOrder::Leaf)
            }
            fn visit_str<E>(self, _: &str) -> Result<KeyOrder, E> {
                Ok(KeyOrder::Leaf)
            }
            fn visit_unit<E>(self) -> Result<KeyOrder, E> {
                Ok(KeyOrder::Leaf)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<KeyOrder, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(KeyOrder::Array(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<KeyOrder, A::Error> {
                let mut seen = BTreeSet::new();
                let mut entries = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    // serde_json keeps the last duplicate; the guard must not
                    // classify a different value than the one that is sent.
                    if !seen.insert(key.clone()) {
                        return Err(de::Error::custom("duplicate key"));
                    }
                    entries.push((key, map.next_value()?));
                }
                Ok(KeyOrder::Object(entries))
            }
        }
        deserializer.deserialize_any(Order)
    }
}

pub(super) fn parse(request: &DbQueryRequest) -> Result<Vec<ClassifiedStatement>, DbError> {
    let parts = split(&request.text)?;
    let mut result = Vec::new();
    let mut used = BTreeSet::new();
    for text in parts {
        let (mut command, order) = if text.trim_start().starts_with('{') {
            // The key order comes from the same text; duplicate keys are
            // refused there, so the classified map is the executed one.
            let order = serde_json::from_str(text).map_err(|_| DbError::InvalidRequest)?;
            (
                serde_json::from_str(text).map_err(|_| DbError::InvalidRequest)?,
                order,
            )
        } else {
            ordered_expression(text)?
        };
        reject_identifier_parameters(&command, 0)?;
        classify(&command)?;
        bind(&mut command, &request.parameters, &mut used, 0)?;
        inspect_json(&command, 0)?;
        let (risk, reason) = classify(&command)?;
        result.push(ClassifiedStatement {
            parsed: ParsedStatement::Mongo { command, order },
            risk,
            reason,
        });
    }
    if used.len() != request.parameters.len() {
        return Err(DbError::InvalidRequest);
    }
    Ok(result)
}

fn bind(
    value: &mut Value,
    parameters: &[DbParameter],
    used: &mut BTreeSet<String>,
    depth: usize,
) -> Result<(), DbError> {
    if depth > 64 {
        return Err(DbError::ResultLimit);
    }
    match value {
        Value::Object(object) => {
            // Explicit JSON value placeholder. It cannot stand for a command,
            // field identifier or operator name.
            if object.len() == 1 && object.contains_key("$param") {
                let name = object["$param"].as_str().ok_or(DbError::InvalidRequest)?;
                let parameter = parameters
                    .iter()
                    .find(|parameter| parameter.name == name)
                    .ok_or(DbError::InvalidRequest)?;
                used.insert(name.into());
                *value = match &parameter.value {
                    DbCell::Null => Value::Null,
                    DbCell::Bool(value) => Value::Bool(*value),
                    DbCell::Text(value) => Value::String(value.clone()),
                    DbCell::Int(value) => {
                        value.parse::<i64>().map_err(|_| DbError::InvalidRequest)?;
                        json!({"$numberLong":value})
                    }
                    DbCell::Uint(value) => {
                        value.parse::<u64>().map_err(|_| DbError::InvalidRequest)?;
                        json!({"$numberDecimal":value})
                    }
                    DbCell::Decimal(value) => json!({"$numberDecimal":value}),
                    DbCell::Float(value) => {
                        let number = value.parse::<f64>().map_err(|_| DbError::InvalidRequest)?;
                        if !number.is_finite() {
                            return Err(DbError::InvalidRequest);
                        }
                        json!({"$numberDouble":value})
                    }
                    DbCell::Binary(value) => {
                        use base64::Engine;
                        base64::engine::general_purpose::STANDARD
                            .decode(value)
                            .map_err(|_| DbError::InvalidRequest)?;
                        json!({"$binary":{"base64":value,"subType":"00"}})
                    }
                    DbCell::Timestamp(value) | DbCell::Date(value) => json!({"$date":value}),
                    DbCell::Time(value) => Value::String(value.clone()),
                    // Structured JSON parameters are rejected: inserting an
                    // object into a predicate could inject Mongo operators.
                    DbCell::Json(_) => return Err(DbError::InvalidRequest),
                };
            } else {
                for (name, value) in object {
                    if name.contains("{{") || name.contains("}}") || name == "$param" {
                        return Err(DbError::InvalidRequest);
                    }
                    bind(value, parameters, used, depth + 1)?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                bind(value, parameters, used, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn reject_identifier_parameters(value: &Value, depth: usize) -> Result<(), DbError> {
    if depth > 64 {
        return Err(DbError::ResultLimit);
    }
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(
                    key.as_str(),
                    "find"
                        | "aggregate"
                        | "count"
                        | "distinct"
                        | "listIndexes"
                        | "collStats"
                        | "insert"
                        | "update"
                        | "delete"
                        | "findAndModify"
                        | "drop"
                        | "create"
                        | "createIndexes"
                        | "dropIndexes"
                        | "renameCollection"
                        | "to"
                        | "key"
                        | "from"
                        | "as"
                        | "localField"
                        | "foreignField"
                        | "connectFromField"
                        | "connectToField"
                        | "sort"
                        | "hint"
                        | "projection"
                        | "collation"
                        | "cursor"
                ) && contains_parameter(value)
                {
                    return Err(DbError::InvalidRequest);
                }
                reject_identifier_parameters(value, depth + 1)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_identifier_parameters(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn contains_parameter(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key("$param") || object.values().any(contains_parameter)
        }
        Value::Array(values) => values.iter().any(contains_parameter),
        _ => false,
    }
}

fn inspect_json(value: &Value, depth: usize) -> Result<(), DbError> {
    if depth > 64 {
        return Err(DbError::ResultLimit);
    }
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(
                    key.as_str(),
                    "$where"
                        | "$function"
                        | "$accumulator"
                        | "$out"
                        | "$merge"
                        | "$eval"
                        | "$code"
                        | "$codeWithScope"
                ) || key.contains('\0')
                {
                    return Err(DbError::PolicyDenied);
                }
                inspect_json(value, depth + 1)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                inspect_json(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn classify(command: &Value) -> Result<(QueryRisk, &'static str), DbError> {
    let object = command.as_object().ok_or(DbError::InvalidRequest)?;
    if object.keys().any(|key| {
        key.starts_with('$')
            || matches!(
                key.as_str(),
                "lsid"
                    | "txnNumber"
                    | "autocommit"
                    | "startTransaction"
                    | "readConcern"
                    | "writeConcern"
                    | "bypassDocumentValidation"
            )
    }) {
        return Err(DbError::PolicyDenied);
    }
    let operations = [
        "find",
        "aggregate",
        "count",
        "distinct",
        "listCollections",
        "listIndexes",
        "collStats",
        "dbStats",
        "explain",
        "insert",
        "update",
        "delete",
        "findAndModify",
        "drop",
        "dropDatabase",
        "create",
        "createIndexes",
        "dropIndexes",
        "renameCollection",
    ];
    let mut selected = operations.iter().filter(|name| object.contains_key(**name));
    let operation = *selected.next().ok_or(DbError::PolicyDenied)?;
    if selected.next().is_some() {
        return Err(DbError::InvalidRequest);
    }
    let allowed: &[&str] = match operation {
        "find" => &[
            "find",
            "filter",
            "projection",
            "sort",
            "skip",
            "limit",
            "batchSize",
            "singleBatch",
            "hint",
            "maxTimeMS",
            "collation",
            "comment",
            "let",
            "returnKey",
            "showRecordId",
        ],
        "aggregate" => &[
            "aggregate",
            "pipeline",
            "cursor",
            "maxTimeMS",
            "allowDiskUse",
            "hint",
            "collation",
            "comment",
            "let",
        ],
        "count" => &[
            "count",
            "query",
            "limit",
            "skip",
            "hint",
            "maxTimeMS",
            "collation",
            "comment",
        ],
        "distinct" => &[
            "distinct",
            "key",
            "query",
            "maxTimeMS",
            "collation",
            "comment",
        ],
        "listCollections" => &[
            "listCollections",
            "filter",
            "nameOnly",
            "authorizedCollections",
            "cursor",
            "maxTimeMS",
            "comment",
        ],
        "listIndexes" => &["listIndexes", "cursor", "maxTimeMS", "comment"],
        "collStats" => &["collStats", "scale", "maxTimeMS", "comment"],
        "dbStats" => &["dbStats", "scale", "freeStorage", "maxTimeMS", "comment"],
        "explain" => &["explain", "verbosity", "maxTimeMS", "comment"],
        "insert" => &["insert", "documents", "ordered", "maxTimeMS", "comment"],
        "update" => &[
            "update",
            "updates",
            "ordered",
            "maxTimeMS",
            "comment",
            "let",
        ],
        "delete" => &[
            "delete",
            "deletes",
            "ordered",
            "maxTimeMS",
            "comment",
            "let",
        ],
        "findAndModify" => &[
            "findAndModify",
            "query",
            "sort",
            "remove",
            "update",
            "new",
            "fields",
            "upsert",
            "maxTimeMS",
            "collation",
            "arrayFilters",
            "let",
            "comment",
        ],
        "drop" => &["drop", "maxTimeMS", "comment"],
        "dropDatabase" => &["dropDatabase", "maxTimeMS", "comment"],
        "create" => &[
            "create",
            "capped",
            "size",
            "max",
            "validator",
            "validationLevel",
            "validationAction",
            "collation",
            "maxTimeMS",
            "comment",
        ],
        "createIndexes" => &["createIndexes", "indexes", "maxTimeMS", "comment"],
        "dropIndexes" => &["dropIndexes", "index", "maxTimeMS", "comment"],
        "renameCollection" => &[
            "renameCollection",
            "to",
            "dropTarget",
            "maxTimeMS",
            "comment",
        ],
        _ => return Err(DbError::PolicyDenied),
    };
    if object.keys().any(|name| !allowed.contains(&name.as_str())) {
        return Err(DbError::PolicyDenied);
    }
    if !matches!(
        operation,
        "explain" | "listCollections" | "dbStats" | "dropDatabase"
    ) {
        let collection = object[operation].as_str().ok_or(DbError::InvalidRequest)?;
        if collection.is_empty() || collection.len() > 255 || collection.contains('\0') {
            return Err(DbError::InvalidRequest);
        }
    }
    if operation == "distinct" && object.get("key").and_then(Value::as_str).is_none() {
        return Err(DbError::InvalidRequest);
    }
    if let Some(value) = object.get("maxTimeMS") {
        let timeout = value.as_u64().ok_or(DbError::InvalidRequest)?;
        if !(1..=300000).contains(&timeout) {
            return Err(DbError::InvalidRequest);
        }
    }
    match operation {
        "drop" | "dropDatabase" | "create" | "createIndexes" | "dropIndexes"
        | "renameCollection" => return Ok((QueryRisk::Destructive, "mongo_ddl")),
        "explain" => {
            if object
                .get("verbosity")
                .and_then(Value::as_str)
                .is_some_and(|value| value != "queryPlanner")
            {
                return Err(DbError::PolicyDenied);
            }
            let (risk, _) = classify(&object["explain"])?;
            if risk != QueryRisk::ReadOnly {
                return Err(DbError::PolicyDenied);
            }
        }
        "aggregate" => {
            let pipeline = object
                .get("pipeline")
                .and_then(Value::as_array)
                .ok_or(DbError::InvalidRequest)?;
            inspect_pipeline(pipeline, 0)?;
        }
        "update" | "delete" => {
            let operations = object
                .get(if operation == "update" {
                    "updates"
                } else {
                    "deletes"
                })
                .and_then(Value::as_array)
                .filter(|values| !values.is_empty() && values.len() <= 1000)
                .ok_or(DbError::InvalidRequest)?;
            let unfiltered = operations.iter().any(|value| {
                value
                    .get("q")
                    .and_then(Value::as_object)
                    .is_none_or(Map::is_empty)
            });
            return Ok((
                if unfiltered {
                    QueryRisk::Destructive
                } else {
                    QueryRisk::Write
                },
                "mongo_write",
            ));
        }
        "findAndModify" => {
            let unfiltered = object
                .get("query")
                .and_then(Value::as_object)
                .is_none_or(Map::is_empty);
            return Ok((
                if unfiltered {
                    QueryRisk::Destructive
                } else {
                    QueryRisk::Write
                },
                "mongo_write",
            ));
        }
        "insert" => return Ok((QueryRisk::Write, "mongo_insert")),
        _ => {}
    }
    Ok((QueryRisk::ReadOnly, "mongo_read"))
}

fn inspect_pipeline(pipeline: &[Value], depth: usize) -> Result<(), DbError> {
    if depth > 32 || pipeline.len() > 256 {
        return Err(DbError::ResultLimit);
    }
    for stage in pipeline {
        let stage = stage
            .as_object()
            .filter(|value| value.len() == 1)
            .ok_or(DbError::InvalidRequest)?;
        let (name, body) = stage.iter().next().ok_or(DbError::InvalidRequest)?;
        match name.as_str() {
            "$match" | "$project" | "$set" | "$addFields" | "$unset" | "$group" | "$sort"
            | "$limit" | "$skip" | "$count" | "$unwind" | "$replaceRoot" | "$replaceWith"
            | "$sample" | "$bucket" | "$bucketAuto" | "$sortByCount" | "$redact"
            | "$graphLookup" | "$setWindowFields" | "$densify" | "$fill" | "$geoNear" => {}
            "$lookup" | "$unionWith" => {
                if let Some(nested) = body.get("pipeline") {
                    inspect_pipeline(nested.as_array().ok_or(DbError::InvalidRequest)?, depth + 1)?;
                }
            }
            "$facet" => {
                for nested in body.as_object().ok_or(DbError::InvalidRequest)?.values() {
                    inspect_pipeline(nested.as_array().ok_or(DbError::InvalidRequest)?, depth + 1)?;
                }
            }
            _ => return Err(DbError::PolicyDenied),
        }
    }
    Ok(())
}

pub(super) fn expression(text: &str) -> Result<Value, DbError> {
    ordered_expression(text).map(|(command, _)| command)
}

/// Builds the command and the source key order of every user-written object
/// it embeds (filters, projections, sort specs, pipelines, documents).
fn ordered_expression(text: &str) -> Result<(Value, KeyOrder), DbError> {
    let mut parser = Expression {
        text: text.trim(),
        offset: 0,
    };
    parser.literal("db")?;
    parser.literal(".")?;
    let mut collection = parser.identifier()?;
    if collection == "runCommand" {
        let (arguments, orders) = parser.arguments()?;
        parser.end()?;
        if arguments.len() != 1 {
            return Err(DbError::InvalidRequest);
        }
        return arguments
            .into_iter()
            .zip(orders)
            .next()
            .ok_or(DbError::InvalidRequest);
    }
    if collection == "getCollection" {
        let (arguments, _) = parser.arguments()?;
        if arguments.len() != 1 {
            return Err(DbError::InvalidRequest);
        }
        collection = arguments[0]
            .as_str()
            .ok_or(DbError::InvalidRequest)?
            .to_owned();
    }
    if collection.is_empty() || collection.len() > 255 || collection.contains('\0') {
        return Err(DbError::InvalidRequest);
    }
    parser.literal(".")?;
    let method = parser.identifier()?;
    let (arguments, orders) = parser.arguments()?;
    let empty = json!({});
    let first = arguments.first().cloned().unwrap_or_else(|| empty.clone());
    let order = |index: usize| orders.get(index).cloned().unwrap_or_default();
    let leaf = || KeyOrder::Leaf;
    let (mut command, mut order) = match method.as_str() {
        "find" | "findOne" if arguments.len() <= 2 => {
            let mut command = if method == "find" {
                json!({"find":collection,"filter":first})
            } else {
                json!({"find":collection,"filter":first,"limit":1,"singleBatch":true})
            };
            if let Some(projection) = arguments.get(1) {
                command["projection"] = projection.clone();
            }
            let order = KeyOrder::object([
                ("find", leaf()),
                ("filter", order(0)),
                ("projection", order(1)),
            ]);
            (command, order)
        }
        "aggregate" if arguments.len() == 1 => (
            json!({"aggregate":collection,"pipeline":first,"cursor":{}}),
            KeyOrder::object([("aggregate", leaf()), ("pipeline", order(0))]),
        ),
        "count" if arguments.len() <= 1 => (
            json!({"count":collection,"query":first}),
            KeyOrder::object([("count", leaf()), ("query", order(0))]),
        ),
        "countDocuments" if arguments.len() <= 1 => (
            json!({"aggregate":collection,"pipeline":[{"$match":first},{"$count":"count"}],"cursor":{}}),
            KeyOrder::object([
                ("aggregate", leaf()),
                (
                    "pipeline",
                    KeyOrder::Array(vec![KeyOrder::object([("$match", order(0))]), leaf()]),
                ),
            ]),
        ),
        "estimatedDocumentCount" if arguments.is_empty() => (
            json!({"count":collection}),
            KeyOrder::object([("count", leaf())]),
        ),
        "distinct" if (1..=2).contains(&arguments.len()) => (
            json!({"distinct":collection,"key":first,"query":arguments.get(1).unwrap_or(&empty)}),
            KeyOrder::object([("distinct", leaf()), ("key", leaf()), ("query", order(1))]),
        ),
        "getIndexes" if arguments.is_empty() => (
            json!({"listIndexes":collection}),
            KeyOrder::object([("listIndexes", leaf())]),
        ),
        "stats" if arguments.is_empty() => (
            json!({"collStats":collection}),
            KeyOrder::object([("collStats", leaf())]),
        ),
        "insertOne" if arguments.len() == 1 => (
            json!({"insert":collection,"documents":[first]}),
            KeyOrder::object([
                ("insert", leaf()),
                ("documents", KeyOrder::Array(vec![order(0)])),
            ]),
        ),
        "insertMany" if arguments.len() == 1 && first.is_array() => (
            json!({"insert":collection,"documents":first}),
            KeyOrder::object([("insert", leaf()), ("documents", order(0))]),
        ),
        "deleteOne" | "deleteMany" if arguments.len() == 1 => (
            json!({"delete":collection,"deletes":[{"q":first,"limit":if method=="deleteOne"{1}else{0}}]}),
            KeyOrder::object([
                ("delete", leaf()),
                (
                    "deletes",
                    KeyOrder::Array(vec![KeyOrder::object([("q", order(0)), ("limit", leaf())])]),
                ),
            ]),
        ),
        "updateOne" | "updateMany" | "replaceOne" if arguments.len() == 2 => (
            json!({"update":collection,"updates":[{"q":first,"u":arguments[1],"multi":method=="updateMany"}]}),
            KeyOrder::object([
                ("update", leaf()),
                (
                    "updates",
                    KeyOrder::Array(vec![KeyOrder::object([
                        ("q", order(0)),
                        ("u", order(1)),
                        ("multi", leaf()),
                    ])]),
                ),
            ]),
        ),
        "drop" if arguments.is_empty() => (
            json!({"drop":collection}),
            KeyOrder::object([("drop", leaf())]),
        ),
        "dropIndex" if arguments.len() == 1 => (
            json!({"dropIndexes":collection,"index":first}),
            KeyOrder::object([("dropIndexes", leaf()), ("index", order(0))]),
        ),
        _ => return Err(DbError::PolicyDenied),
    };
    while parser.remaining().starts_with('.') {
        parser.literal(".")?;
        let modifier = parser.identifier()?;
        let (arguments, mut orders) = parser.arguments()?;
        if command.get("find").is_none() || arguments.len() != 1 {
            return Err(DbError::PolicyDenied);
        }
        match modifier.as_str() {
            "sort" if arguments[0].is_object() => {
                command["sort"] = arguments[0].clone();
                order.set("sort", orders.pop().unwrap_or_default());
            }
            "limit" | "skip" => {
                let value = arguments[0]
                    .as_u64()
                    .filter(|value| *value <= 1_000_000)
                    .ok_or(DbError::InvalidRequest)?;
                command[modifier] = json!(value);
            }
            _ => return Err(DbError::PolicyDenied),
        }
    }
    parser.end()?;
    Ok((command, order))
}

struct Expression<'a> {
    text: &'a str,
    offset: usize,
}

impl Expression<'_> {
    fn remaining(&mut self) -> &str {
        let rest = &self.text[self.offset..];
        self.offset += rest.len() - rest.trim_start().len();
        &self.text[self.offset..]
    }

    fn literal(&mut self, text: &str) -> Result<(), DbError> {
        if !self.remaining().starts_with(text) {
            return Err(DbError::InvalidRequest);
        }
        self.offset += text.len();
        Ok(())
    }

    fn identifier(&mut self) -> Result<String, DbError> {
        let rest = self.remaining();
        let length = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'$'))
            .count();
        if length == 0 || length > 255 {
            return Err(DbError::InvalidRequest);
        }
        let value = rest[..length].to_owned();
        self.offset += length;
        Ok(value)
    }

    fn arguments(&mut self) -> Result<(Vec<Value>, Vec<KeyOrder>), DbError> {
        self.literal("(")?;
        let start = self.offset;
        let bytes = self.text.as_bytes();
        let mut string = false;
        let mut escaped = false;
        while self.offset < bytes.len() {
            let byte = bytes[self.offset];
            if string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    string = false;
                }
            } else if byte == b'"' {
                string = true;
            } else if byte == b')' {
                let text = format!("[{}]", &self.text[start..self.offset]);
                let arguments: Vec<Value> =
                    serde_json::from_str(&text).map_err(|_| DbError::InvalidRequest)?;
                let orders: Vec<KeyOrder> =
                    serde_json::from_str(&text).map_err(|_| DbError::InvalidRequest)?;
                self.offset += 1;
                return Ok((arguments, orders));
            } else if byte == b'(' {
                return Err(DbError::PolicyDenied);
            }
            self.offset += 1;
        }
        Err(DbError::InvalidRequest)
    }

    fn end(&mut self) -> Result<(), DbError> {
        if self.remaining().is_empty() {
            Ok(())
        } else {
            Err(DbError::InvalidRequest)
        }
    }
}

pub(super) fn split(text: &str) -> Result<Vec<&str>, DbError> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut string = false;
    let mut escaped = false;
    for (index, byte) in text.bytes().enumerate() {
        if string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
            }
        } else {
            match byte {
                b'"' => string = true,
                b'(' | b'{' | b'[' => depth += 1,
                b')' | b'}' | b']' => {
                    depth = depth.checked_sub(1).ok_or(DbError::InvalidRequest)?
                }
                b';' if depth == 0 => {
                    let part = text[start..index].trim();
                    if !part.is_empty() {
                        parts.push(part);
                    }
                    start = index + 1;
                }
                _ => {}
            }
        }
        if depth > 64 || parts.len() > MAX_STATEMENTS {
            return Err(DbError::ResultLimit);
        }
    }
    if string || depth != 0 {
        return Err(DbError::InvalidRequest);
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    Ok(parts)
}
