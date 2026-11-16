# Isolated Prometheus core

This standalone crate starts MON-01/MON-02 without changing the frozen R6 workspace. The source is not wired to the production AppCore/FRB yet. It has no administrative API, background scheduler, vault access or general credential resolver.

`MonitoringClient::new(SourceConfig, Budgets)` supports anonymous direct connections. `query(expression, time)`, `query_range(expression, start, end, step_seconds)`, `probe()` and `label_values(label)` return typed DTOs from `src/dto.rs`. `__name__` requests metric names. Auth references and unsupported routes fail closed in the anonymous/JSON boundary. Rust-only AppCore integration may construct `SourceAuthorization::bearer` or `::basic` from an already authorized source and `Zeroizing<String>` material, then call `MonitoringClient::with_authorization`. This narrow grant binds the complete source config and auth-ref kind, has no Clone/Serde or resolver, and accepts at most 30 seconds TTL. The request deadline is capped by the remaining grant lifetime. AppCore must independently own source revision, native Credential lookup, unlocked lease, lock/profile cancellation and post-response revision checks. Never expose material constructors through FRB/JSON.

Defaults and hard ceilings: 15 seconds, 16 MiB response, 100 series, 1000 points per series, 100,000 total points, four concurrent requests per client, 16 KiB expression, 1000 metadata values, 64 labels per series. Smaller budgets are accepted. Range step is coarsened explicitly and returned in `effectiveStepSeconds`; exceeding a returned-result budget is an error, never silent truncation. The HTTP series limit requests one extra row to detect truncation; byte and series validation also protect against servers that ignore the limit. Metadata cache TTL is 60 seconds with at most 16 entries; `clear_cache()` removes it. The application must implement its global concurrency, cache clearing on profile/vault transitions, backoff and screen lifecycle.

TLS and hostname verification remain enabled, custom CA chains may be supplied, redirects and ambient proxies are disabled. Endpoint userinfo, query and fragments are rejected; credentials require HTTPS. Request paths are generated internally from fixed query/metadata API paths. Neither PromQL nor server responses are treated as URLs. Backend tenant/gateway permissions remain the actual access boundary, independent of selector filtering.

`QueryOutcome` retains API status, HTTP status, typed data, error type/message, warnings and infos. Warnings conservatively mark a response partial. Empty vectors remain empty; missing samples remain absent; NaN and infinities are tagged values. Native histogram results fail explicitly. Capability probing checks actual complete `vector(1)` vector and matrix results independently; it does not claim full backend compatibility. Exact known credential echoes are redacted before DTO parsing, including authorization header/token; transformed or unknown secrets cannot be guaranteed detectable.

Response timestamps must stay within the supported finite timestamp range; range responses must stay inside the requested inclusive interval and cannot exceed the number of requested points. Saturated concurrency returns `busy` immediately: there is no unbounded waiting queue. Cancelling a future releases its permit; grant expiry also stops delayed in-flight TLS responses and blocks expired metadata cache reads.

## Typed panels

`panels::PanelKind` provides Host overview, CPU, memory, filesystem, disk I/O, network, Docker, PostgreSQL, Redis and nginx templates. `client.prepare_panel(kind, &HostBinding, start, end, step)` performs one bounded source/selector-scoped metric presence probe and returns `Ready { plan, probe }` or `Unavailable { reason, missingMetrics, probe, error }`. Errors and warnings remain visible; missing metrics never become zero. PostgreSQL/Redis/nginx are gated by actual required metrics rather than backend/exporter product names. A Native-approved, unambiguous binding is a caller precondition. Use distinct explicit service/exporter bindings where instance labels differ; no IP/name matching is inferred.

The plan records source ID, native host UUID, binding revision, coarsened step, series/points/total-panel budgets and typed units. CPU uses average per-logical-CPU idle `rate` and preserves host identity labels; memory used percent is `100*(1-MemAvailable/MemTotal)`. Filesystem percentage means capacity unavailable to an unprivileged user, including reserved space, per mount. Counter rates use a five-minute window with reset handling in Prometheus, before any aggregation. Disk and network preserve device/interface series (including loopback), containers preserve identity and use CPU cores plus working-set bytes with a fixed nonempty-image filter. No cAdvisor installation or Kubernetes scrape deployment occurs. `PanelKind::query_templates` is preview only and does not assert availability. UI rendering/units, stale lifecycle and Native-approved execution remain integration work.

## Development adapter

Build/run `cc-monitoring-json` for isolated UI development. One bounded JSON request on stdin (close stdin after writing); one response on stdout. No raw secrets are accepted.

```json
{"source":{"id":"source-1","name":"Prometheus","endpoint":"http://127.0.0.1:9090","route":{"kind":"direct"},"tls":{"customCaPem":null},"auth":{"kind":"none"}},"operation":{"kind":"queryRange","expression":"up","start":0,"end":3600,"stepSeconds":15}}
```

Operations: `probe`; `query` with `expression` and optional `time`; `queryRange` with `expression/start/end/stepSeconds`; `labelValues` with `label`. Response is `{ "ok": true, "result": <DTO> }` or `{ "ok": false, "error": { "code": "...", "message": "...", "httpStatus": 401 } }` (HTTP status omitted when unavailable). Stdin is limited to 256 KiB. This single-request adapter does not retain caches between processes; long-lived AppCore service integration is still required.

## Verification

From the containing `monitoring-e2` directory (all builds use at most two jobs):

```powershell
cargo test --manifest-path client/rust/monitoring-core/Cargo.toml --target-dir target/monitoring-core -j 2 --offline
& ./client/rust/monitoring-core/test-real-prometheus.ps1
```

On this Windows sandbox, loopback socket connections require the tool's escalated execution permission; sandbox failure `WSAEACCES/10013` is not a core HTTP failure. The fixture script uses the official Prometheus v3.5.0 image pinned by digest, only a unique labelled container with loopback publish, CPU/RAM/PID limits, a read-only root and tmpfs. No host mounts or user credentials. Cleanup checks its exact ownership label.

Primary references checked for implementation: [Prometheus HTTP API](https://prometheus.io/docs/prometheus/latest/querying/api/), [official fixture release v3.5.0](https://github.com/prometheus/prometheus/releases/tag/v3.5.0). Pinned top-level dependency versions match the existing R6 lockfile; the isolated lockfile is independent.
