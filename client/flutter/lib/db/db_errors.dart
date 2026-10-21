import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/db/db_models.dart';

/// The reason code a native database operation failed with (`tls_failed`,
/// `authentication_failed`…), or null for other failures.
String? dbFailureCode(Object error) =>
    error is AppException && error.args['field'] == 'database' ? error.args['rule'] : null; // l10n-ignore: wire field

/// A message that says what went wrong and what to try.
String dbFailureText(Object error, AppLocalizations l) => switch (dbFailureCode(error)) {
  'tls_failed' => l.dbErrorTls,
  'tls_unavailable' => l.dbErrorTlsUnavailable,
  'channel_binding_unsupported' => l.dbErrorChannelBinding,
  'cleartext_refused' => l.dbErrorCleartext,
  'authentication_failed' => l.dbErrorAuth,
  'connection_failed' => l.dbErrorConnection,
  'timeout' => l.dbErrorTimeout,
  'policy_denied' => l.dbQueryBlocked,
  'route_unavailable' || 'connection_unavailable' => l.dbErrorRoute,
  'kube_unreachable' => l.dbErrorKubeUnreachable,
  'kube_target_missing' => l.dbErrorKubeTarget,
  'kube_access_denied' => l.dbErrorKubeDenied,
  'kube_forward_blocked' => l.kubeForwardProductionBlocked,
  'kube_forward_failed' => l.dbErrorKubeForward,
  'tunnel_failed' => l.dbErrorTunnel,
  'tunnel_unsupported' => l.dbErrorTunnelKind,
  'docker_unreachable' => l.dbErrorDockerUnreachable,
  'docker_container_missing' => l.dbErrorDockerContainer,
  'docker_forward_failed' => l.dbErrorDockerForward,
  _ => l.dbFailed,
};

/// What protects a session, as shown in the workspace and on the card.
String dbTransportText(DbTransportSecurity security, AppLocalizations l) => switch (security) {
  DbTransportSecurity.verified => l.dbTransportVerified,
  DbTransportSecurity.unverified => l.dbTransportUnverified,
  DbTransportSecurity.tunnel => l.dbTransportTunnel,
  DbTransportSecurity.plaintext => l.dbTransportPlaintext,
};

/// Loopback addresses: port-forwards and local databases.
bool dbLoopbackHost(String host) {
  final value = host.trim().toLowerCase();
  return value == 'localhost' || value == '::1' || value.startsWith('127.');
}
