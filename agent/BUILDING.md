# Building the Agent

Use the repository Rust toolchain (`../rust-toolchain.toml`), Python 3.11 or newer, and the exact locked dependencies. Run `python3 -I -B agent/scripts/check_source.py` from the repository root, then `cargo test --locked --manifest-path agent/identity/Cargo.toml --all-targets --jobs 2`. Tests create only private temporary fixtures. No production configuration or existing node state is imported.

`DEVICE_BUILD_MANIFEST.json` is a source build plan, not an artifact receipt. The phase2 source merge and fixture path relocation are present; the canonical Rust/device build and enabled whole-startup gate remain separate validation steps. Root owns the canonical runtime/identity integration. A device build must use the patched SQLite path dependency and record its real target, full toolchain version, current source and binary hashes. Do not substitute an old debug/test ELF or old sealed package.

The separate native hard-mode collector source is an overlay on the exact official Prometheus source archive described in `third_party/prometheus-agent/SOURCE.json`. Validate the archive hash, apply its ordered patches and additive source files, preserve upstream LICENSE/NOTICE, and compile its ordinary `cmd/prometheus` binary with the declared Go toolchain. The repository intentionally does not contain a cloned upstream TSDB. Native hard mode remains an explicit, bounded profile, not a generic replacement TSDB or an automatic collector selector.

No source builder should create keys, discover/adopt a node, start a service or sign a release. A release package requires new binary/source/notice bindings and an explicitly authorized signature; the old sealed05 catalog is never rewritten.

`delivery/prototype_package.py` retains the bounded prototype package format. Its current source inventory must bind all 62 `SOURCES` files plus `launch.py` and the service unit, the actual identity binary, and a `bundled-python-wheel` component named `websockets`, version `17.4`, with `wheel_sha256` and `package_path` matching `delivery/package_wheel.py` and the full BSD notice. No current inventory is fabricated here. The exact pure wheel is packaged at `python/site-packages/websockets-17.4-py3-none-any.whl`. After verifying every signed package file, the launcher enables Python zipimport from that one release-owned path; it never calls pip or installs into ambient Python. Disabled TUN startup does not import the carrier.

The opt-in supervised V2 transport accepts only the root-controlled system `/usr/bin/python3.11` (Debian 12) or `/usr/bin/python3.12` (Ubuntu 24.04). The selected release must contain an explicitly reviewed `cc-tunnel-supervisor-runtime/v1` public pin with exactly `format`, `pythonExecutable` and the actual nonzero `pythonSha256`. The package binds these original bytes at `python/supervisor-runtime.json` in its source/notice inventory. Rust retains the selected static path and the original interpreter file identity/hash/mode; updates or mismatches deny, with no PATH lookup, interpreter discovery, fallback or grant renewal. Source support for these two paths is not by itself runtime acceptance on a new OS/architecture.

For a NAT/TUN-enabled candidate, pass `--supervisor-runtime-pin /explicit/reviewed-target-host-pin.json` to `delivery/release_pipeline.py package` (or the same option to the prototype packager). The pipeline makes an exact owned copy and binds its hash/path metadata in `build-public.json`; both notice generation and packaging recheck the same bytes. Absence remains supported for packages that do not enable V2, but V2 probe/bootstrap/activation must deny it. No CPython binary, stdlib or fingerprint is fabricated or bundled by this step.

The administrator first observes the fixed interpreter on the **actual target Host**, checks its root-owned physical 0755 file and SHA against the independently reviewed OS supply, then reviews that public pin as a build input before package signing. A CI runner's own interpreter or a fixture-global hash is not a target Host pin. Different interpreter builds require different reviewed package inputs. The real outbound role runs the fixed read-only `enrollment-role-check --tunnel-runtime-check`: it verifies the selected installed release, compares the actual target interpreter against that package pin, and repeats the selected-release identity checks. It does not create, rewrite or adopt a runtime pin. OS updates that change it require a newly reviewed package/pin; a missing or mismatched pin is an actionable prerequisite failure, not Ready.

The explicit volatile collection package additionally binds the fixed linux-amd64 vmagent v1.156.0 ELF at `collector/vmagent-prod` (SHA2039bc05...,23,209,584 bytes), its required notice component, and the three maintained collector runtime sources. The packager takes an explicit reviewed binary path; it does not search caches or download. Verified staging manages the fixed runtime at `/opt/consolecrypt-agent-install/opt/consolecrypt-agent/collector`; stable files must match the selected signed release before launch. Provisioning the dedicated user manager/delegation/tmpfs and signed operator catalog is a separate explicit prerequisite, not performed by package creation or collector probe.


## Delivery after a reviewed build

Follow [RELEASE_SIGNING.md](delivery/RELEASE_SIGNING.md) for the dedicated
Agent-release signer and explicit administrator trust bootstrap, then
[INSTALLING.md](delivery/INSTALLING.md) for operator commands. The manual CI
workflow's full signed package is Linux amd64 only; its ARM64 job only compiles
and tests. Installed volatile collection uses the separately pinned upstream
vmagent v1.156.0, not the unfinished Prometheus-overlay hard-mode contract.
Complete Go-module/stdlib notice inputs and final distribution review are still
required before a production package. No image, account, identity, grant or
service is created by a build or by reading these instructions.
