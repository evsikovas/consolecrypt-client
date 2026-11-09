# Reviewed WAL initialization backport

Upstream crate: libsqlite3-sys 0.38.2, unchanged package version/API/features.
Crates.io archive SHA-256:
`f1d20bef17f513b9b3004532233187769cd072d790971f4e4da0e346eb6401e8`.

Applied exactly the WAL reader change from SQLite commit
[e5bf119821f25be1d7f10ef0a2edbe175289166e](https://github.com/sqlite/sqlite/commit/e5bf119821f25be1d7f10ef0a2edbe175289166e)
to **both** `sqlite3/sqlite3.c` and `sqlcipher/sqlite3.c`.
When the requested page is larger than a WAL frame, initialize its unread tail
to zero before returning it to the caller (RUSTSEC-2026-0459).

Upstream licenses are preserved. No database schema, cipher settings, Rust API,
or toolchain baseline changes. The root Cargo patch is used on all platforms.

`client/security-backports.json` pins the complete reviewed native source digest,
including build.rs, bundled C and headers. The OSV exception requires this exact
local patch in the build graph and does not cover registry copies.

Run `python3 -m unittest discover -s client/scripts -p test_sqlite_wal_backport.py -v`.
This compiles both production readers, tests page size mismatches and buffer
boundaries, then reverse-patches each reader to prove the regression detects
the original bug. Also run cc-storage-core / cc-search-core tests against the
patched SQLCipher build for encryption, rekey, WAL and search compatibility.
