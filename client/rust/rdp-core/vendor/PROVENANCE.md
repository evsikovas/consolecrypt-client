# IronRDP connector compatibility patch

Source: published `ironrdp-connector` 0.10.0 archive from crates.io, corresponding to upstream IronRDP 0.17.0. Upstream license: MIT OR Apache-2.0. Upstream: https://github.com/Devolutions/IronRDP/tree/ironrdp-v0.17.0/crates/ironrdp-connector

Only normalized Cargo.toml dependency requirements are changed: picky 7.0.0-rc.25 → exactly 7.0.0-rc.26; SSPI 0.21 → exactly 0.23.0. The original release pins prerelease Dalek versions incompatible with ConsoleCrypt's stable Dalek versions. The replacements use stable Dalek. No cryptographic algorithm, protocol state machine, certificate verification, or credential handling is patched.

Cargo.toml.orig is the unmodified published source manifest. One compiler-required API adaptation in src/credssp.rs propagates SSPI's newly fallible TsRequest::buffer_len result instead of assuming an infallible length. Other vendored source files are retained from the official published archive. Remove the patch when a published connector supports stable dependencies directly.

The upstream changelog has normalized line endings and trailing whitespace; its content is unchanged.
