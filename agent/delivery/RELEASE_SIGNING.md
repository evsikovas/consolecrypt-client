# Agent release candidates and administrator bootstrap

`cc-agent-release-sign` is an offline developer/CI tool. It signs only the
existing typed `ReleaseManifest` under `agent-release-manifest`, using the
existing Rust canonical statement bytes and Ed25519 implementation. It is a
separate binary and is never included in the normal Agent package. It cannot
sign node, owner, enrollment, reader, tunnel or arbitrary raw statements.

Select a dedicated Agent release key through your controlled key-management
process. This source does not create keys. Do not reuse an application, RuStore,
enrollment, owner, Agent identity or vault key. The tool also requires the exact
expected public key, obtained independently of a candidate package.

For a developer invocation, supply a raw 32-byte seed file with mode0600 in an
operator-owned0700 directory. Only Linux is supported. Input links, hardlinks,
special files, changed bytes and permissive secret-file modes are refused.
The output directory must be new and have an existing operator-owned parent.
It receives only `release-manifest.json` and `public-key.json`; incomplete public
output is preserved and must be inspected rather than overwritten.

```
cc-agent-release-sign --manifest /owned/release/unsigned-release.json \
  --output-dir /owned/release/signed --expected-public-key APPROVED_64_HEX \
  --seed-file /owned/private/agent-release-seed
```

The alternate `--seed-env CC_AGENT_RELEASE_ED25519_SEED_HEX` accepts exactly that
dedicated variable containing64 lowercase hex characters. It has no other key
name, provider, seed-generation or default-key fallback. The variable is removed
from the child environment after capture, and owned seed buffers/signing state
are zeroized on drop. The invoking shell/CI still owns its environment; scope the
secret to the signing step and use the CI platform's secret handling.

The staged `agent-release-candidate.yml` workflow is manual, main-branch-only and
uses the protected `agent-release-signing` environment. Configure its dedicated
secret `CC_AGENT_RELEASE_ED25519_SEED_HEX`, public variable
`CC_AGENT_RELEASE_EXPECTED_PUBLIC_KEY`, and reviewed nonnegative monotonic floor
`CC_AGENT_RELEASE_APPROVED_FLOOR`. Missing configuration hard-fails. No workflow,
secret or environment is created by adding these files. Native builds use
Ubuntu24.04 amd64 with Rust1.98.1, the locked crates and patched vendor source.
The full signed candidate includes the explicit official vmagent v1.156.0
Linux-amd64 binary from the maintained supply lock. CI's separate opt-in supply
step fetches and checks that archive/binary; packaging requires the exact
`--vmagent-binary` input and copies the original pinned bytes. Its complete
VictoriaMetrics Apache-2.0 license is pinned in `licenses/VMAGENT-LICENSE.txt`
and bound into the notice inventory. This is the upstream vmagent implementation;
it does not claim the separate Prometheus-overlay hard-mode capability.
This project license alone does not prove complete transitive Go-module or Go
standard-library notices. Before packaging, the fixed `collector_materials.py`
step reads actual `go version -m -json` output without executing vmagent. It uses
a fresh owned cache, `GOTOOLCHAIN=local`, public `proxy.golang.org` and
`sum.golang.org`, and `GOVCS=*:off`; no module builds, hooks, private proxies,
ambient Go configuration or automatic toolchain downloads are used. Exact dependency versions come from the actual binary. For this exact pinned
official binary, Main is `(devel)`, VCS data is absent and all19 dependency sums
are absent. Its separately pinned official v1.156.0 source go.mod/go.sum supplies
only the missing h1 sums for those exact observed dependency path/version pairs.
The source record contains the actual observed tag commit and original source
hashes; it does NOT establish the binary's VCS commit, an independently
authenticated signer, or reproducible source/build correspondence. These limits
are retained as `sourceNoticeProvenance` with `sourceBuildCorrespondence=false`.
Unknown versions, local replacements, missing exact source checksum rows or full
license materials still refuse the candidate.

Every module ZIP must match the official Go downloader's verified sum and exact
embedded identity. Full LICENSE/NOTICE/COPYING/AUTHORS/CONTRIBUTORS/COPYRIGHT/PATENTS
materials are collected, including nested component notices. The matching Go
source archive is selected from the official Go download metadata and rehashed;
its full standard-library materials are collected too. The original raw buildinfo,
module/archive/material hashes and receipt are bound to the original collector
SHA. Missing or oversized materials fail the existing package/notice gate; text
is never shortened to fit. Packaging requires this output explicitly via
`--collector-materials`. Collection is not a legal compatibility certification.

The main project's complete pinned license is included separately; no synthetic
main-module checksum is invented. Each dependency ZIP must match the official Go
tool's downloaded h1 and the exact separately pinned source go.sum row. Actual
`CGO_ENABLED=1` remains visible in original buildinfo: collection of project,
module and Go-source notices does not certify native linkage/license completeness.
The candidate remains explicitly not production-distribution-ready.
The Go tool flags and download receipt are described in the official
[Go command reference](https://pkg.go.dev/cmd/go) and
[module download reference](https://go.dev/ref/mod#go-mod-download).

ARM runs in a separate unprivileged compile/test job without release secrets,
signing, full packaging, deployment or an installed metrics capability claim.
The fixed installed collector currently supports Linux amd64 only. The runner labels are documented in
[GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

Every run captures source before/after the actual Cargo build, selects exactly
the normal Agent and separate signer outputs, checks their ELF architecture and
copies them without shared hardlinks. Notices are regenerated from the actual
current build descriptor, exact public crate archives, vendored SQLite, pinned
websockets wheel and full license texts. Old prototype/fixture inventories are
never used as the new build's inventory. The existing package builder and notice
validator remain the package implementation. The candidate is signed, then the
actual normal Rust verifier must accept the exact original artifact, public key,
manifest, platform, protocol and floor. No signer executable or secret is uploaded.

The workflow retains a CI candidate and exact committed public Agent source.
It does not create a GitHub release, push a tag, install a service or establish
production trust. Notice validation proves file integrity, not legal compatibility
or a validated SBOM. Exact Rust standard-library/native-linkage notice review,
corresponding-source distribution review and final supported-platform release
approval remain explicit gates before public distribution. No all-Linux or
actual-running-version claim follows from a package display version.

## Initial administrator trust

The installer intentionally refuses a missing bootstrap. Its trust root is the
fixed root-controlled verifier, authenticated release public key and `trust.json`
at the paths in `release-bootstrap.template.json`. A CI `trust.example.json` is
only public input: its presence or a self-consistent package signature does not
authenticate a new signer. Obtain the verifier SHA256 and release-key fingerprint
through an independently authenticated administrator channel.

The optional `consolecrypt_agent_release_trust` role accepts explicit approval,
those exact independently approved hashes/fingerprint, and public controller
files. Its read-only controller checker validates actual bytes, architecture and
the closed `ReleaseSignerPin` schema; it does not claim cryptographic verification
of a release. It creates only the fixed public trust files. An existing matching
bootstrap is observed unchanged; foreign, partial, linked or writable trust is
refused. There is no implicit key rotation, trust repair, service start/stop,
enrollment, profile setup, private identity material or data deletion.

Bootstrap is a separate explicit administrator operation. The ordinary Agent
installer project factory does not include or run this role. Subsequent release
installation still verifies signatures with the original pinned Rust verifier
and retains its existing high-water rollback floor. Multi-file trust publication
is not a power-failure transaction; a partial failure stays fail-closed and needs
explicit administrator investigation.
