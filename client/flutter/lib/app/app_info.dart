/// Product and author metadata (About dialog, window/bundle metadata).
///
/// The native copies live in `macos/Runner/Configs/AppInfo.xcconfig`
/// (`PRODUCT_COPYRIGHT`) and `windows/runner/Runner.rc`; keep them in sync.
library;

const kAppName = 'ConsoleCrypt';
const kAppAuthor = 'Alexander Evsikov';
const kAppAuthorEmail = 'i@evsikov.net';
const kAppCopyright = 'Copyright © 2026 $kAppAuthor';

/// Generated together with pubspec.yaml by client/scripts/bump-version.py.
/// Native bundles, About and the core all use this release/build identity.
const kAppVersion = '0.2.3';
const kAppBuildNumber = 66;
const kAppFullVersion = '$kAppVersion+$kAppBuildNumber';

/// SPDX licence expression of the client (ADR-0005).
const kAppLicense = 'AGPL-3.0-only';

/// SPDX licence of the self-hosted sync server (ADR-0005).
const kServerLicense = 'AGPL-3.0-only';

/// Readable licence labels; SPDX expressions above remain exact metadata.
const kAppLicenseDisplayName = 'AGPL-3.0';
const kServerLicenseDisplayName = kAppLicenseDisplayName;
