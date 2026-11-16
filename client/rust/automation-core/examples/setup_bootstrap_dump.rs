//! Read-only fixture: emit the actual production WSL argument, never launch WSL.
use cc_automation_core::SetupTarget;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = SetupTarget::Wsl {
        distribution: "D".repeat(128),
        user: Some("u".repeat(64)),
    };
    let command = target.command()?;
    let args = command
        .get_args()
        .map(|value| value.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let script = args.last().ok_or("missing built-in argument")?;
    let units = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|value| value.to_string_lossy().encode_utf16().count() * 2 + 4)
        .sum::<usize>()
        + 1;
    assert!(units <= 32767);
    assert!(script.len() <= 15 * 1024);
    println!(
        "{}",
        serde_json::json!({"script":script,"worstCaseCommandUtf16Units":units,"argc":args.len(),
        "fixtureOnly":true,"launchedWsl":false})
    );
    Ok(())
}
