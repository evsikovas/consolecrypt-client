//! The CLI reports the app version it ships with (client/flutter/pubspec.yaml,
//! e.g. `0.9.3+21063`), not the internal crate version.
use std::path::Path;

fn main() {
    let pubspec = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../flutter/pubspec.yaml");
    println!("cargo:rerun-if-changed={}", pubspec.display());
    let version = std::fs::read_to_string(&pubspec)
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("version:").map(|v| v.trim().to_owned()))
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
    println!("cargo:rustc-env=CC_APP_VERSION={version}");
}
