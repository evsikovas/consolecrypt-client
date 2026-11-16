import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard, toAppException;
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_revocation.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_status.dart';

/// Actual service_revoke.rs methods, supplied only by a future generated
/// composition. No guessed generated import or current production activation.
abstract interface class AgentCollectorServiceRevocationNativeHandle {
  Future<String> details();
  Future<String> confirmRevocation();
  void cancel();
  Future<String> joined();
  void dispose();
}

final class RustAgentCollectorServiceWithRevocation
    implements AgentCollectorServicePort, AgentCollectorServiceStatusPort, AgentCollectorServiceRevocationPort {
  const RustAgentCollectorServiceWithRevocation({
    required this.original,
    required this.status,
    required this.prepareOriginal,
  });
  final AgentCollectorServicePort original;
  final AgentCollectorServiceStatusPort status;
  final Future<AgentCollectorServiceRevocationNativeHandle> Function(String requestJson) prepareOriginal;
  @override
  Future<AgentCollectorServiceCatalogView> readCatalog(RuntimeNoticeScope scope, AgentAssociationRecord record) =>
      original.readCatalog(scope, record);
  @override
  Future<AgentCollectorServiceHandle> prepare(RuntimeNoticeScope scope, AgentCollectorServiceIntent intent) =>
      original.prepare(scope, intent);
  @override
  Future<AgentCollectorServiceStatus> readSavedStatus(RuntimeNoticeScope scope, AgentAssociationRecord record) =>
      status.readSavedStatus(scope, record);
  @override
  Future<AgentCollectorServiceRevocationHandle> prepareServiceRevocation(AgentCollectorServiceStatus target) async {
    if (target.remoteRevoked) throw StateError('Service revocation already observed');
    final original = await guard(
      () => prepareOriginal(
        jsonEncode({
          'version': 1,
          'profileId': target.scope.profileId,
          'notificationScopeId': target.scope.nonce,
          'associationId': target.record.id,
        }),
      ),
    );
    return _Original(original, target);
  }
}

void _sync(void Function() call) {
  try {
    call();
  } catch (error) {
    final mapped = toAppException(error);
    reportVaultKeyMismatch(mapped);
    throw mapped;
  }
}

final class _Original implements AgentCollectorServiceRevocationHandle {
  _Original(this.original, this.target);
  final AgentCollectorServiceRevocationNativeHandle original;
  final AgentCollectorServiceStatus target;
  bool disposed = false;
  void _live() {
    if (disposed) throw StateError('Original revocation proxy disposed');
  }

  @override
  Future<AgentCollectorServiceRevocationView> details() async {
    _live();
    return parseAgentCollectorServiceRevocation(await guard(original.details), target);
  }

  @override
  Future<AgentCollectorServiceRevocationView> confirmRevocation() async {
    _live();
    return parseAgentCollectorServiceRevocation(await guard(original.confirmRevocation), target);
  }

  @override
  void cancel() {
    _live();
    _sync(original.cancel);
  }

  @override
  Future<AgentCollectorServiceRevocationCleanup> joined() async {
    _live();
    return parseAgentCollectorServiceRevocationCleanup(await guard(original.joined));
  }

  @override
  void dispose() {
    if (disposed) return;
    disposed = true;
    _sync(original.dispose);
  }
}
