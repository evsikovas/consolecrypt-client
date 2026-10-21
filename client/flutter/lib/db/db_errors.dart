import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';

/// The reason code a native database operation failed with (`tls_failed`,
/// `authentication_failed`…), or null for other failures.
String? dbFailureCode(Object error) =>
    error is AppException && error.args['field'] == 'database' ? error.args['rule'] : null; // l10n-ignore: wire field

/// A message that says what went wrong and what to try.
String dbFailureText(Object error, AppLocalizations l) => switch (dbFailureCode(error)) {
  'tls_failed' => l.dbErrorTls,
  'authentication_failed' => l.dbErrorAuth,
  'connection_failed' => l.dbErrorConnection,
  'timeout' => l.dbErrorTimeout,
  'policy_denied' => l.dbQueryBlocked,
  'route_unavailable' || 'connection_unavailable' => l.dbErrorRoute,
  _ => l.dbFailed,
};

/// Loopback addresses: port-forwards and local databases.
bool dbLoopbackHost(String host) {
  final value = host.trim().toLowerCase();
  return value == 'localhost' || value == '::1' || value.startsWith('127.');
}
