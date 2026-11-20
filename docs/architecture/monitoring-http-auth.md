# Explicit HTTP Basic source policy

Monitoring sources support HTTP with no authentication or Basic authentication
when the user explicitly saves that source configuration. Choosing HTTP+Basic
is the transport consent; there is no second approval or silent HTTPS rewrite.
UI should explain that HTTP sends its Basic credentials without encryption.
HTTP Bearer remains rejected. HTTPS Basic/Bearer certificate and custom-CA
verification are unchanged; no global insecure TLS switch exists.

The source stores only a Native credential UUID reference in encrypted
VaultSettings, never a password/token or URL userinfo. The existing Native
request resolves that credential under profile/unlock/source/secret revision
fences. Basic material stays request-scoped and zeroizing; the whole source
configuration binds its grant. Userinfo, query and fragments in source URLs
remain rejected. Fixed API paths append to a saved Grafana/Prometheus prefix.
Redirects and ambient proxies remain disabled, auth headers marked sensitive,
and known credential/header echoes redacted from untrusted response strings.
No new resolver, authentication DTO, tunnel fallback or endpoint discovery is
introduced by allowing this explicitly selected transport.
