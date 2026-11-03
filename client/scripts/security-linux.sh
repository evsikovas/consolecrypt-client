#!/usr/bin/env bash
set -euo pipefail
# Run the GLib backport with optimizer settings that exposed the upstream UB,
# then verify filesystem capability/atomic-download regressions on real Unix.
cargo test --locked --release \
  --manifest-path client/flutter/vendor/super_native_extensions/rust/Cargo.toml \
  --test glib_security
cargo test --locked --release --manifest-path client/rust/Cargo.toml \
  -p cc-sftp-core --lib
cargo test --locked --release --manifest-path client/rust/Cargo.toml \
  -p cc-platform-core --all-targets
