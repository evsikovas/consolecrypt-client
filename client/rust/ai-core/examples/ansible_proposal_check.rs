//! Offline contract check of a synthetic/model response. No AppCore or execution.
use std::io::{self, Read};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("expected relative YAML path")?;
    let mut raw = String::new();
    io::stdin().take(256 * 1024 + 1).read_to_string(&mut raw)?;
    let proposal = cc_ai_core::automation::parse(&raw, &path)?;
    println!("{}", serde_json::json!({"valid_native_contract":true,
        "files":proposal.files.len(),"variables":proposal.variables.len(),
        "dependencies":proposal.dependencies.len(),"executed":false}));
    Ok(())
}
