//! Private local subprocess protocol. No startup greeting, raw SQL/signing,
//! caller clock, HTTP-selected paths, fallback or token-bearing diagnostics.
use cc_agent_identity_policy::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use zeroize::Zeroizing;
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
enum Command {
    Admit {
        lease: Box<Value>,
        proof: Box<Value>,
        freshness: Box<Value>,
        operation: Operation,
    },
    Mutate {
        mutation: Box<Value>,
    },
    Status,
}
fn observed() -> Result<Now> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::InvalidTime)?
        .as_secs();
    Ok(Now(Timestamp::new(seconds)?))
}
fn emit(value: Value) -> Result<()> {
    let bytes = serde_json::to_vec(&value).map_err(|_| Error::Storage)?;
    if bytes.len() > 4096 {
        return Err(Error::Quota);
    }
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|_| stdout.write_all(b"\n"))
        .and_then(|_| stdout.flush())
        .map_err(|_| Error::Storage)
}
pub(super) fn run(args: &[String], initialize: bool) -> Result<()> {
    if args.len() != 7
        || args[1] != "--db-dir"
        || args[3] != "--anchor-dir"
        || args[5] != "--owner-pin"
    {
        return Err(Error::InvalidWire);
    }
    let path = PathBuf::from(&args[6]);
    let meta = std::fs::symlink_metadata(&path).map_err(|_| Error::Storage)?;
    if !path.is_absolute()
        || !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() > MAX_WIRE_BYTES as u64
    {
        return Err(Error::Storage);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| Error::Storage)?
        .take(MAX_WIRE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Storage)?;
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(Error::Quota);
    }
    let pin: OwnerPin = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
    let store = if initialize {
        SqliteJournalStore::create(
            &args[2],
            &args[4],
            pin,
            observed()?,
            SqliteStorageBudget::default(),
        )?
    } else {
        SqliteJournalStore::open(&args[2], &args[4], pin, SqliteStorageBudget::default())?
    };
    if initialize {
        emit(json!({"initialized":true,"provider":"sqlite-journal-v1"}))?;
        return Ok(());
    }
    let mut actor = Actor::new(store)?;
    let mut stdin = std::io::stdin().lock();
    loop {
        let mut line = Zeroizing::new(Vec::new());
        let count = stdin
            .by_ref()
            .take(MAX_WIRE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(|_| Error::Storage)?;
        if count == 0 {
            return Ok(());
        }
        if count > MAX_WIRE_BYTES {
            emit(json!({"ok":false,"code":"Quota"}))?;
            return Err(Error::Quota);
        }
        // EOF without newline is an incomplete protocol frame, never a request.
        if !line.ends_with(b"\n") {
            emit(json!({"ok":false,"code":"InvalidWire"}))?;
            return Err(Error::InvalidWire);
        }
        let parsed = match serde_json::from_slice::<StrictJson>(&line) {
            Ok(value) => value.0,
            // A well-formed outer admit from HTTP can contain pathological inner
            // JSON; its depth error is a denied message, not daemon poisoning.
            Err(error) if error.to_string().starts_with("recursion limit exceeded") => {
                emit(json!({"ok":false,"code":"InvalidWire"}))?;
                continue;
            }
            Err(_) => {
                emit(json!({"ok":false,"code":"InvalidWire"}))?;
                return Err(Error::InvalidWire);
            }
        };
        let command: Command = match serde_json::from_value(parsed) {
            Ok(value) => value,
            Err(_) => {
                emit(json!({"ok":false,"code":"InvalidWire"}))?;
                return Err(Error::InvalidWire);
            }
        };
        let result = (|| -> Result<Value> {
            let now = observed()?;
            match command {
                Command::Admit {
                    lease,
                    proof,
                    freshness,
                    operation,
                } => {
                    if !matches!(operation, Operation::TelemetryRead { .. }) {
                        return Err(Error::ScopeDenied);
                    }
                    let permit = actor.admit(
                        serde_json::from_value(*lease).map_err(|_| Error::InvalidWire)?,
                        serde_json::from_value(*proof).map_err(|_| Error::InvalidWire)?,
                        Some(&serde_json::from_value(*freshness).map_err(|_| Error::InvalidWire)?),
                        &operation,
                        now,
                    )?;
                    actor.check_permit(&permit, observed()?)?;
                    Ok(
                        json!({"agentId":permit.agent_id(),"operation":permit.operation(),"expiresAt":permit.expires_at()}),
                    )
                }
                Command::Mutate { mutation } => {
                    super::apply_mutation(
                        &mut actor,
                        serde_json::from_value(*mutation).map_err(|_| Error::InvalidWire)?,
                        now,
                    )?;
                    Ok(json!({"revision":actor.snapshot()?.revision()}))
                }
                Command::Status => {
                    let state = actor.snapshot()?;
                    Ok(
                        json!({"provider":"sqlite-journal-v1","revision":state.revision(),"policy":state.capacity()}),
                    )
                }
            }
        })();
        match result {
            Ok(metadata) => emit(json!({"ok":true,"metadata":metadata}))?,
            Err(error) => {
                emit(json!({"ok":false,"code":error.to_string()}))?;
                if matches!(
                    error,
                    Error::Storage | Error::UncertainCommit | Error::Entropy
                ) {
                    return Err(error);
                }
            }
        }
    }
}

// Strict duplicate detection for the private framing channel, without echoing a
// supplied field name. Nested signed DTO validation remains a per-request denial.
pub(super) struct StrictJson(pub(super) Value);
impl<'de> serde::Deserialize<'de> for StrictJson {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = StrictJson;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("strict JSON frame")
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                v: bool,
            ) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::Number(v.into())))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::Number(v.into())))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| StrictJson(Value::Number(v)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::String(v.to_owned())))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::String(v)))
            }
            fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::Null))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut values: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some(value) = values.next_element::<StrictJson>()? {
                    out.push(value.0);
                }
                Ok(StrictJson(Value::Array(out)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut values: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut out = serde_json::Map::new();
                while let Some(key) = values.next_key::<String>()? {
                    if out.contains_key(&key) {
                        return Err(serde::de::Error::custom("cc-duplicate-json-key"));
                    }
                    let value: StrictJson = values.next_value()?;
                    out.insert(key, value.0);
                }
                Ok(StrictJson(Value::Object(out)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
