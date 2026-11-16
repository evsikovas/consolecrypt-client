# Monitoring: native boundary (work after R6)

The R6 ZIP is immutable and does not contain this integration. Source changes
below are not a completed release or a claim that the entire E2 scope passed.

Monitoring source configuration belongs to the existing encrypted VaultSettings
object. The source catalog has its own schema and monotonically increasing
catalog/source revisions. Unknown versions/fields remain encrypted and read-only;
an older generic settings form must preserve this catalog. Raw metrics are never
written to that object or local settings. The public sync server still stores
encrypted existing object types; this change does not introduce a server-side
metric database or new object kind.

This local preview does not certify mixed-version synchronization. Clients built
before the `monitoring` model field can discard that field when rewriting the
VaultSettings object. The new generic settings writer preserves the newest raw
catalog, including future schemas, but cannot retrofit preservation into an
already installed older client. Stable multi-device rollout therefore still
needs a compatible client baseline or a separately versioned storage migration.

The UI creates/edits a source in a separate CAS operation. Reads accept only:

```json
{
  "operationId": "de55b582-838c-46b7-822c-17e6f27c4204",
  "sourceId": "fcaa2ef3-b7f5-4992-b631-8f7d53232f63",
  "sourceRevision": 1,
  "action": {"kind": "query", "expression": "up", "time": null}
}
```

Other actions: `probe`, `queryRange` with `start/end/stepSeconds`,
`labelValues` with `label`, and `clearCache`. Responses identify the same source
and revision and contain `result`. Core transport failures retain typed safe
error codes; backend errors/warnings/partial data remain visible.

An endpoint, token, password or replacement source configuration in a read
request is rejected. Native fetches the saved source and checks the profile and
unlock lifetime. Direct transport is the first integrated route. An unsupported
SSH/Agent route fails before credential lookup, without falling back to direct.
No exporter or metrics listener is opened by viewing a graph.

Auth references name existing personal Password Credential objects. A Basic
reference uses its username and referenced Password Secret; a Bearer reference
uses its Password Secret as a token. Material is read only in native code after
configuration authorization, held in zeroizing Rust-only grant objects and never
bound through FRB. Grants bind the complete source configuration and expire in at
most 30 seconds. Bearer authentication requires HTTPS. Basic authentication
also supports explicit HTTP sources, with an inline disclosure that the
credentials have no transport encryption. HTTPS hostname and CA verification
remain enabled; redirects and ambient proxies are disabled. See
`monitoring-http-auth.md` for the exact policy.

Requests recheck source and credential revisions before HTTP and before returning
data. Blocking the vault cancels requests and clears client metadata caches.
Explicit cancellation is scoped to an operation UUID in this unlock; an early
cancel creates a bounded tombstone. There is no unbounded request queue. The
current native ceilings are 8 in-flight reads per unlock and 4 per source;
transport and response-size/series/point budgets are independently enforced.

Selectors are query filters, not access control. An unrestricted backend token
can still read that backend according to its server permissions. Tenant/gateway
enforcement and SSH/Agent routes require separate integration and acceptance.
Metric snapshots are processed within the metric backend's trust boundary and
do not acquire vault end-to-end encryption automatically.


## Explicit host bindings and typed panels (source after R8)

The first explicit binding creates catalog schema v2; a source-only v1 catalog
remains v1 until that action. A binding stores its Native-generated ID, saved
host/source IDs and revisions, display name and explicit label pairs. No binding
is inferred from an IP, hostname or the first matching exporter. Identical label
selectors for the same host/source are rejected; distinct named selectors are
permitted. Missing host/source references remain visible and deletable. A source
with referenced bindings cannot be removed until those bindings are removed.

`bindingDiscover` accepts a saved binding ID/revision and selected time. Native
queries a scoped `up` selector and reports resolved, missing, ambiguous or
unavailable identity. `up=0` may identify an exporter; it does not demonstrate
availability. Changing the source or host invalidates pending reads. Stale source
bindings require explicit re-review rather than silently accepting new authority.

`bindingPanel` accepts only the saved binding ID/revision, one of ten typed panel
names, and bounded start/end/step. The UI supplies no PromQL, endpoint, claimed
resolution or metric-name list for this action. Native independently checks
identity at the selected end, probes required metric names, then generates its
fixed query templates. Panels cover host overview, CPU, memory, filesystem,
disk I/O, network, Docker, PostgreSQL, Redis and Nginx. Units and semantics are
part of each query result. Missing samples and unavailable metrics are not
converted to zero; partial responses, warnings and non-finite values remain
explicit. Combined panel ceilings are 100 series, 100,000 points and 16 MiB,
inside the existing 35-second read deadline and per-unlock/source concurrency.

The separate generic query workspace still accepts user PromQL under the saved
source's authority. Typed panel selectors do not create backend tenant isolation.
SSH/Agent monitoring routes and a complete production Agent rollout remain
unimplemented. Passing local catalog, loopback HTTP and widget tests does not
certify the full platform matrix or mixed-version synchronization.


## R12 source: charts, saved definitions and explicit current-value observation

This section describes the delivered immutable R12 local Windows preview;
R11 and older archives remain unchanged. Headless tests do not certify full
macOS/Linux/mobile or production monitoring acceptance.

The common time selector uses UTC: 15 minutes, one hour, six hours, one day,
seven days or explicit custom bounds. Applying a range does not execute a
request. Chart results support multiple labelled series, legend visibility,
exact point inspection and presentation-only zoom/pan/reset. Viewport changes
neither fetch nor resample data. Gaps, NaN/Inf, warnings, partial data and unknown
units remain explicit; missing values never become zero. A user unit hint
labels a saved definition without converting or proving measurements.

Saved definitions introduce catalog v3 on explicit local creation. They contain
Native IDs, title, exact PromQL, source ID/revision and a user unit hint, inside
the existing encrypted settings object; measurements are not synchronized.
Reads take saved IDs/revisions, never replacement expressions/endpoints from UI.
Source edits require explicit re-review. Missing references remain visible;
referenced sources cannot be silently removed. Catalog/record CAS apply to
mutations; future schemas stay read-only and survive unrelated settings edits.

Synced-profile create/update currently fail with
`monitoring_panels / panel_sync_admission_required`. Local profiles remain
available. Reading and explicit CAS deletion of existing panels preserve v3.
The private roster diagnostic has no Ready state and grants no writes. Server
admission must also prevent later old-client admission; a compatible device
snapshot alone is insufficient. No mixed-version rollout is implied here.

Read-only alerts/rules use the selected saved source and existing Native
request/auth/TLS/size/lifecycle controls. They do not evaluate/edit rules,
create silences or install a notification service.

Explicit Pin starts one memory-only current-value observation with a separate
profile-owned service/request UUIDs. Cadence is 30 seconds, one minute or five
minutes, scheduled after completion without overlap or missed-tick replay.
Navigation cancels ordinary hidden monitoring reads; the pin continues, with
global Stop outside the hidden branch, including Terminal/minimal mode.
Opening the application or watching its provider creates no transport/query.

Each metric read checks the original source fingerprint before and after.
Source removal/revision, profile switch, lock, app pause/detach/exit stop it and
reject late results. Desktop focus loss alone does not stop it. Transient
failures retain explicitly stale previous values and last-success UTC, with
bounded backoff and stop after three consecutive failures. Permanent/source
errors clear state. Restart/unlock never resumes automatically or counts as
user activity. This is not rolling history or an Agent background service.

Workspace evidence: Tasks/monitoring-pin-accepted-combined-tests.log (269 scoped
UI tests), monitoring-r12-active-gated-catalog-sync-tests.log (48 AppCore tests),
and monitoring-saved-panels-active-http-tests.log (three actual HTTP cases).
Complete R12 suite/build/package results are recorded independently; these
focused checks do not certify the entire specification.

## R13 candidate: stale results and a shared notification centre

R13 source adds a scoped last-success snapshot for an explicitly repeated
identical monitoring read. A known transient transport/server failure marks
that snapshot stale with its actual last-success UTC; a successful empty result
replaces it. Source/revision, expression or time-range changes clear the old
snapshot. Invalid response/schema/query and unknown failure categories never
become transient merely because the HTTP status is 5xx. This adds no timer.

The shared notification centre observes the one existing Native event stream
and the existing pinned-observation owner. Committed job state changes include
only profile ID, job ID, revision and typed state. Ordinary hidden-section job
completion needs no Jobs polling. Session rows are bounded to64, remembered job
revisions to2048; lock/profile boundaries clear the projection. Stream gaps and
closure remain explicit; manual refresh reconciles one existing jobs() read.
An observed UTC is not advertised as a Runner completion timestamp.

Opening a job notice selects that exact job's existing read-only output panel.
Opening a pinned-observation notice selects only the matching saved source ID
and revision; stale/missing sources show an error. Neither action executes a
query, prepares/retries/confirms a job, creates a source, restarts a pin or
restores a navigation icon. The ordinary legacy banners remain intact; device
trust, synchronization, SSH, backups and other subsystem events have not all
been consolidated into this centre. NAV-09 remains partial.

Actual evidence includes321 default AppCore tests and a separate signed Ansible
job over two owned SSH fixtures: queued/running/succeeded events were received
without Jobs completion polling. UI widget/full-build/package results must be
checked separately before attributing this candidate to an archive. Private
Vault validation prerequisites and controller-retirement work do not expose a
public installer or Vault dispatcher.

## R14 source: literal API-error HTTP status and Grafana review

The opt-in Windows product-boundary fixture uses the verified immutable R13 DLL,
generated FRB, a new Memory-only local profile, a real owned loopback HTTP server
and production Dart chart/stale views. It found that QueryOutcome carries the
HTTP status on the envelope while its nested ApiError has only type/message.
The old Dart mapping lost that status and cleared a previous success on a known
503 server failure. QueryOutcome/LabelValues now pass that envelope fallback
only for server-type errors. Native-code/schema/unknown/auth failures retain the
strict failure policy. This correction is newer than immutable R13 AOT.

Seven service unit cases and one actual DLL/HTTP/widget flow passed, including
zero/NaN/partial/gap projection, exact POST range form,503 stale retention,
successful-empty replacement, and locked rejection before another HTTP request.
Evidence: Tasks/monitoring-packaged-ffi-status-fixed-tests.log. Formula evaluation
remains covered separately by compiled Native/promtool; the HTTP fixture does
not evaluate PromQL. A new R14 UI build/package is required before claiming the
corrected application shipped. Wider platforms and full MON06 are not certified.

Grafana import source is an offline one-panel review and existing-editor prefill,
not a datasource credential importer or dashboard runtime. Unsupported panels
stay explicit, source/revision is manually selected and freshly rechecked, and
manual Save still uses the existing local/CAS/admission checks. Opening Grafana
uses a separately entered, reviewed HTTP(S) address; dashboard links do not
supply that destination. Import does not execute metric queries or automatically
substitute variables. The manual-constant resolver and generalized15s observation
remain separately staged until their own integration and actual acceptance.

## Current source: selected monitoring data and AI review (12 November 2026)

This is a source/test integration, not another preview distribution. Existing
archives are unchanged. The user has deferred the next distribution until the
entire automation/monitoring specification is complete. Platform acceptance and
the mixed-version synchronization restrictions above remain separate gates.

After reading an observation, the user can choose specific samples or active
alerts, select an existing AI provider, and ask for an explanation or a playbook
proposal. Preparing the preview does not contact the model or repeat a metrics
query. Sending requires a separate action; a provider outside loopback also
requires explicit consent. The result is read-only text. It cannot save a file,
change a project, select targets, install an exporter or execute a command.

`monitoring_ai_prepare`, `monitoring_ai_send` and `monitoring_ai_discard` use the
same profile-owned service and existing provider construction/secret resolver.
Only the provider's API key is resolved for its sensitive authorization header.
Monitoring-source credentials, host routes, vault contents, terminal history and
project files are not supplied to the model. Prepare's profile/source/provider
references remain Native-local and are absent from model messages. The strict
selected-data projection excludes raw queries, URLs, annotations and raw errors;
remaining names/labels/question pass the provider's existing sanitizer policy.
Filtering is not a guarantee that arbitrary user-written text contains no secret;
the user reviews the exact sanitized message contents before sending.

The issuer stores an opaque, one-use ticket for the original unlocked vault,
source revision, exact provider/configuration and sanitized request. Tickets
expire after 120 seconds. There are at most four pending previews, two concurrent
preparations and two active generations per unlock, without an unbounded queue.
Profile locking, source/provider edits, discard and local-model replacement
cancel the relevant work. Final synchronous checks repeat original identity and
current configuration after asynchronous work, including equal-revision
readbacks. No unlocked/session guard is held during the model request. A local
failure never selects a remote provider or retries through generic chat.

Limits are eight series with at most 32 explicitly selected samples each, eight
active alerts, 32 KiB of selected-data JSON, 4 KiB for the question and 64 KiB for
the exact preview or response. The bundled local model additionally limits its
prepared preview to 8 KiB and reserves a 1024-token output budget. An oversized
local context is rejected rather than silently truncated. Responses have a
closed schema: explanation, optional unvalidated playbook and limitations. Tool
calls, extra authority fields and truncated/malformed responses are rejected.
Zero, NaN, infinities, gaps, partial/stale results, accepted time windows and
unknown units/clock skew retain their distinct meanings.

Current evidence includes 11 focused/67 complete AI-core tests, nine AppCore
issuer cases with an owned loopback provider, 161 integrated Monitoring/UI tests,
and clean full Flutter analysis. The generated Rust/Dart API pair has content
hash `652234751`; its Native check passed. The initial four bridge error-type
conversions and two independent exporter test-fixture mistakes were corrected
with original failure logs preserved. The broader Native regression set covered
401 nonignored cases; the two owned-remote-coordinator fixture guards remain
explicitly ignored. Tests do not claim production-provider, deployment or all-
platform certification. Current logs and source hashes are recorded in
`Tasks/cor-integration-current-checkpoint.json` in the parent workspace.


### Unsaved panel graph preview (source checkpoint)

The saved-panel editor exposes an explicit Show graph action with the existing
shared UTC period controller. It submits the draft expression through the same
profile/source-scoped bounded query service, displays the exact requested
expression beside its result, and never writes a panel or changes the separate
exploratory expression. Creating a synchronized saved panel still requires its
independent compatibility admission; preview does not bypass that writer gate.
No query occurs on import, typing, opening period controls or saving a draft.
A source change, cancellation, route hide or retired session invalidates late
results. Units chosen in the editor remain user hints, without converting values
or asserting a proven native metric unit. Advanced dates are collapsed to keep
the editor compact. Actual19 widget checks and RU narrow/wide visual checks are
recorded in Tasks/monitoring-draft-graph-compact-tests.log; targeted analysis is
clean. This source checkpoint has not been packaged as another interim build.


### Accepted unsaved graph for AI review (follow-up)

The explicit AI action beside an unsaved graph captures that graph's accepted
query result. It does not reuse a previous explorer result, attach unrelated
alerts, refresh metrics or save the panel. Original source/service/catalog and
foreground lifetime predicates still fence the captured result. Five related
test files passed38 cases; the final two-file analyzer is clean. Evidence:
Tasks/draft-graph-ai-context-evidence.json in the parent workspace.

An additional owned FFI test loaded the actual test-only Windows Native DLL
(content hash b2e98b207f1547b09955e9a21cf1dc3cecfdafca149366cac4b0ee3d4fffe414,
ABI652234751), created an ephemeral Memory-store profile and read metrics from
its own loopback endpoint. Explicit selected-data preview/send made exactly one
model request; a second send and a Native lock both denied reuse. This DLL
snapshot precedes the later private ACL integration and is not an application
distribution or all-platform/production-provider certification. The later
Native source separately passed409 AppCore cases (two existing remote guards)
and127 automation-core cases (three inherited guards).


### Explicit Grafana constant import

The Classic/V1 importer supports explicit manual constant values in whole quoted
PromQL label matchers only. Initial fields are blank/unacknowledged; saved
Grafana values and variable queries are never read or run. The existing single
cancellable isolate resolves and validates the literal query; changing values,
panel, source/revision or original profile invalidates review. The literal query
is displayed before an explicit editor prefill and independent Save. V2/query/
custom variables, macros and unsupported transformations/plugins remain reported
without modifying the original dashboard. Native source selection is independent
of Grafana datasource names/UIDs. No metric query occurs during import.

Current active gate:84 focused cases; fullFlutter2531PASS/22existing environment
guards; clean scoped analyzer; two actual-font Russian layouts plus two small
keyboard/enlarged-text cases. Evidence is in the parent workspace
Tasks/grafana-constants-active-evidence.json. No new distribution was produced.

### Job and metrics navigation

Graphs display the selected host's job history, original project revision and
known Runner process observations. The dashed admission marker uses the client
clock; an unknown target clock offset stays explicit. Switching UTC/local time
changes display only. Overlapping jobs remain separate, and missing legacy
metadata is not inferred from current project or host names.

The Jobs page and execution console can open a saved metrics context. The user
chooses an approved host, source and current binding. This transition performs
metadata reads only: it neither fetches metrics nor opens a terminal connection.
The Monitoring page consumes the original-session context once. Reading metrics
is a separate action that rechecks the original job, host, source and binding.
A changed profile, unlock, host revision or retired source rejects the old context.

Exporter templates remain inspect-first. Creating a template does not install a
service. After a separately approved successful job, the exporter workflow
verifies fresh data against the selected source and host. Saving its explicit
binding and opening the panel are separate actions.

## Agent telemetry and association: current client source

The existing **Linux Agent** dialog combines fixed public-file owner-export
reviews with separate Reader telemetry actions. It uses saved source/binding/
SSH-host and known-host references. Each review and read remains bound to the
original profile/unlock and foreground context. Opening the dialog never
enrolls an Agent, renews a grant or starts a metric read. Selecting public files
alone establishes no trust: Native validates the original owner and the exact
fixed purpose. Exporting a reviewed signed offer/certificate does not prove that
the Agent applied it. Transferred copies may remain outside the client; cancelling
a later action does not delete those copies. Reader telemetry permission is
separate from Node ownership/Tunnel admission and never grants job execution.

Current indicators and bounded local history are snapshots. The UI displays
observed UTC time and, for heartbeat data, the actual bounded Agent version.
Version text does not attest the installed binary, establish compatibility or
provide an update action. Collector running/disabled/stopped/not-running/
starting/degraded/blocked/unknown metadata is shown as recorded information.
Missing values stay Unknown; a legacy zero running gauge means only not-running,
with no invented cause. Local history is available only while the Agent can be
read and does not guarantee complete coverage of the selected period. Browsing
the already loaded history pages does not perform another request. An unavailable
read does not load a new dataset; retry requires a separate explicit review.

The additional **Agent association** flow is optional source integration. Its
production port is currently unbound while the trusted Native public factory,
generated adapter and corresponding paired acceptance are completed. The action
therefore remains hidden in the ordinary backend. UI contract tests are not
proof of a current installed Agent, a public Relay deployment or a live Tunnel.

When that real capability is supplied, the staged flow is: explicitly read local
status; select an actual saved personal Native SSH host and its Native revision;
then explicitly probe the fixed enrollment subsystem. Local status performs no
SSH probe. No local association record does not mean that the remote Agent is
unenrolled. A local Associated record is historical data, not a live permission
or reachability check.

A missing client TLS identity has a separate creation review and confirmation.
This creates only the Native-managed P256 client identity under its fixed
approximately 30-UTC-day certificate policy. Probe never creates or replaces it;
pending, unavailable and expired identities are not automatically reset or
rotated. Creating this identity does not enroll an Agent or start a probe.

The SSH transaction has two separate consumed approvals: the original offer,
then the observed claim before issuing its certificate. The review shows the
actual public keys/fingerprints, original owner identity and distinct Node
context, exact decimal epochs/checkpoints, client certificate digest and the
remaining original review time. The fixed public review budget starts at idle
registration and is not renewed by reading details or moving to the next stage.
Native also caps the requested certificate lifetime by the owner pin and existing
client certificate expiry; the final actual expiry is displayed. These UI values
never replace Native admission checks.

Lock, profile/service/trust changes, route retirement or expiry invalidate the
original view. Cancellation is a request. Only the issued operation's separate
cleanup observation can report observed or unknown cleanup; neither cancellation
nor cleanup proves rollback, remote drain or revocation. An ambiguous mutating
result stays Unknown, with no automatic retry or re-enrollment. The client accepts
no caller owner pin, PEM, command, URL, epoch or replacement trust key.

The independently provisioned fixed admin/enrollment SSH subsystem is a
prerequisite; the base Agent profile and these UI changes do not install or enable
it. See [the Agent source boundaries](../../agent/README.md). Ordinary Prometheus
source configuration and queries retain their existing path and credential policy.

