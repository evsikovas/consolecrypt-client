//! Explicit private custody provider; legacy worker admission is unchanged.
use cc_agent_identity_policy::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::path::PathBuf;
use zeroize::Zeroizing;

#[path = "custody.rs"]
mod custody;
use super::sqlite_worker::StrictJson;
use custody::{Custody, Handle};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
enum Command {
    #[serde(rename_all = "camelCase")]
    Begin {
        agent_id: AgentId,
        operation: Operation,
        lease: Box<Value>,
        proof: Box<Value>,
        freshness: Box<Value>,
    },
    #[serde(rename_all = "camelCase")]
    Check {
        handle: String,
        agent_id: AgentId,
        operation: Operation,
    },
    #[serde(rename_all = "camelCase")]
    Finalize {
        handle: String,
        agent_id: AgentId,
        operation: Operation,
    },
    #[serde(rename_all = "camelCase")]
    Retire {
        handle: String,
        agent_id: AgentId,
        operation: Operation,
    },
}

fn frame(value: Value) -> Result<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(serde_json::to_vec(&value).map_err(|_| Error::Storage)?);
    // The LF is part of the private output budget, including boundary cases.
    if bytes.len() >= 4096 {
        return Err(Error::Quota);
    }
    bytes.push(b'\n');
    Ok(bytes)
}
fn emit(value: Value) -> Result<()> {
    let bytes = frame(value)?;
    let mut output = std::io::stdout().lock();
    output
        .write_all(&bytes)
        .and_then(|_| output.flush())
        .map_err(|_| Error::Storage)
}
fn decode(line: &[u8]) -> std::result::Result<Command, (Error, bool)> {
    if line.len() > MAX_WIRE_BYTES {
        return Err((Error::Quota, true));
    }
    if !line.ends_with(b"\n") {
        return Err((Error::InvalidWire, true));
    }
    let StrictJson(value) = serde_json::from_slice(line).map_err(|error| {
        // Same bounded pathological-inner-data handling as the old worker.
        let fatal = !error.to_string().starts_with("recursion limit exceeded");
        (Error::InvalidWire, fatal)
    })?;
    serde_json::from_value(value).map_err(|_| (Error::InvalidWire, true))
}
fn execute<S: CompareAndSwap>(custody: &mut Custody<S>, command: Command) -> Result<Value> {
    match command {
        Command::Begin {
            agent_id,
            operation,
            lease,
            proof,
            freshness,
        } => {
            let begun = custody.begin(
                agent_id,
                operation,
                serde_json::from_value(*lease).map_err(|_| Error::InvalidWire)?,
                serde_json::from_value(*proof).map_err(|_| Error::InvalidWire)?,
                serde_json::from_value(*freshness).map_err(|_| Error::InvalidWire)?,
            )?;
            let encoded = begun.handle.encoded();
            Ok(json!({"handle":encoded.as_str(),"agentId":begun.agent_id,
                "operation":begun.operation,"expiresAt":begun.expires_at}))
        }
        Command::Check {
            handle,
            agent_id,
            operation,
        } => {
            let text = Zeroizing::new(handle);
            custody.check(&Handle::parse(&text)?, agent_id, &operation)?;
            Ok(json!({"phase":"checked"}))
        }
        Command::Finalize {
            handle,
            agent_id,
            operation,
        } => {
            let text = Zeroizing::new(handle);
            custody.finalize(&Handle::parse(&text)?, agent_id, &operation)?;
            Ok(json!({"phase":"finalized"}))
        }
        Command::Retire {
            handle,
            agent_id,
            operation,
        } => {
            let text = Zeroizing::new(handle);
            custody.retire(&Handle::parse(&text)?, agent_id, &operation)?;
            Ok(json!({"phase":"retired"}))
        }
    }
}
fn closes_worker<S: CompareAndSwap>(custody: &Custody<S>, error: Error) -> bool {
    matches!(
        error,
        Error::Storage | Error::UncertainCommit | Error::Entropy
    ) || custody.clock_failed()
}
pub(super) fn run(args: &[String]) -> Result<()> {
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
    let mut bytes = Zeroizing::new(Vec::new());
    std::fs::File::open(path)
        .map_err(|_| Error::Storage)?
        .take(MAX_WIRE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Storage)?;
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(Error::Quota);
    }
    let StrictJson(value) = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidWire)?;
    let pin: OwnerPin = serde_json::from_value(value).map_err(|_| Error::InvalidWire)?;
    let actor = Actor::new(SqliteJournalStore::open(
        &args[2],
        &args[4],
        pin,
        SqliteStorageBudget::default(),
    )?)?;
    let mut custody = Custody::new(actor);
    let mut input = std::io::stdin().lock();
    loop {
        let mut line = Zeroizing::new(Vec::new());
        let count = input
            .by_ref()
            .take(MAX_WIRE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(|_| Error::Storage)?;
        if count == 0 {
            return Ok(());
        }
        // Private protocol errors are fatal; Signed DTO errors inside a valid
        // begin command below are ordinary deny/continue responses.
        let command = match decode(&line) {
            Ok(command) => command,
            Err((error, fatal)) => {
                emit(json!({"ok":false,"code":error.to_string()}))?;
                if fatal {
                    return Err(error);
                }
                continue;
            }
        };
        match execute(&mut custody, command) {
            Ok(metadata) => emit(json!({"ok":true,"metadata":metadata}))?,
            Err(error) => {
                emit(json!({"ok":false,"code":error.to_string()}))?;
                if closes_worker(&custody, error) {
                    return Err(error);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_private_frames_bound_duplicates_and_unknown_actions() {
        for raw in [
            b"{\"action\":\"retire\",\"action\":\"retire\"}\n".as_slice(),
            b"{\"action\":\"admit\"}\n",
            b"{\"action\":\"status\"}\n",
            b"{\"action\":\"check\"}",
            b"{\"action\":\"begin\",\"now\":1000}\n",
        ] {
            assert!(decode(raw).is_err());
        }
        assert!(matches!(
            decode(&vec![b' '; MAX_WIRE_BYTES + 1]),
            Err((Error::Quota, true))
        ));
        let nested = format!("{}0{}\n", "[".repeat(150), "]".repeat(150));
        assert!(decode(nested.as_bytes()).is_err());
    }
    #[test]
    fn output_budget_includes_lf() {
        assert_eq!(frame(json!("a".repeat(4093))).unwrap().len(), 4096);
        assert!(matches!(frame(json!("a".repeat(4094))), Err(Error::Quota)));
    }
}
