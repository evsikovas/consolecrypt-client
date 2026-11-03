# super_native_extensions

This plugin contains native functionality for [`super_clipboard`](https://pub.dev/packages/super_clipboard) and [`super_drag_and_drop`](https://pub.dev/packages/super_drag_and_drop).

Local security maintenance (2026-11-03): the Rust logger uses `simple_logger`
5 to remove `atty`, and `rust/Cargo.lock` pins patched `anyhow`, `bytes`, `rand`,
`time`, and `url`/`idna` dependencies. The Linux GTK3 stack uses a reviewed local
`glib` 0.17.10 backport of RUSTSEC-2024-0429; CI verifies its source hash,
lockfile wiring and optimized iterator regression. GTK/GLib/message-channel
macros use `proc-macro-error3` instead of the unmaintained first generation.
The root Rust workspace lockfile does not cover this plugin's native build.
