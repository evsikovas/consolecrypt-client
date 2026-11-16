//! Read-only acceptance probe. This example deliberately has no install mode.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = match std::env::args().nth(1) {
        Some(distribution) => cc_automation_core::SetupTarget::Wsl {
            distribution,
            user: None,
        },
        None => cc_automation_core::SetupTarget::Native,
    };
    let result = cc_automation_core::setup_environment(&target, false).await?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
