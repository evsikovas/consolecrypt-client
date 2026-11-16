//! Single-request, bounded JSON adapter for isolated native UI development.
//! Production AppCore integration must own grants, lifecycle and source storage.
use cc_monitoring_core::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    source: SourceConfig,
    operation: Operation,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Operation {
    Probe,
    Query {
        expression: String,
        time: Option<f64>,
    },
    QueryRange {
        expression: String,
        start: f64,
        end: f64,
        #[serde(rename = "stepSeconds")]
        step_seconds: f64,
    },
    LabelValues {
        label: String,
    },
}
async fn execute(input: Input) -> Result<Value, Error> {
    let client = MonitoringClient::new(input.source, Budgets::default())?;
    // DTO serialization cannot fail for any result emitted by the core parser.
    Ok(match input.operation {
        Operation::Probe => json!(client.probe().await),
        Operation::Query { expression, time } => json!(client.query(&expression, time).await?),
        Operation::QueryRange {
            expression,
            start,
            end,
            step_seconds,
        } => json!(
            client
                .query_range(&expression, start, end, step_seconds)
                .await?
        ),
        Operation::LabelValues { label } => json!(client.label_values(&label).await?),
    })
}
#[tokio::main]
async fn main() {
    let mut bytes = Vec::new();
    let input = if std::io::stdin()
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .is_ok()
        && bytes.len() <= 256 * 1024
    {
        serde_json::from_slice::<Input>(&bytes).ok()
    } else {
        None
    };
    let output = match input {
        None => {
            json!({"ok":false,"error":{"code":"invalid_config","message":"Invalid or oversized JSON adapter request"}})
        }
        Some(input) => match execute(input).await {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(error) => json!({"ok":false,"error":error}),
        },
    };
    // Only validated, bounded DTOs are emitted; never print input or serde errors.
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, &output);
    let _ = stdout.write_all(b"\n");
}
