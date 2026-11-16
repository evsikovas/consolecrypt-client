import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard, toAppException;
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';

/// Typed callbacks for actual Core operation.rsbc785401 methods. The generated
/// Bridge import/factory is intentionally absent until Root's real FRB receipt.
abstract interface class AgentCollectorServiceNativeHandle {
  Future<String> details();
  Future<String> confirmConfiguration();
  void cancel();
  Future<String> joined();
  void dispose();
}

final class RustAgentCollectorServicePort implements AgentCollectorServicePort {
  const RustAgentCollectorServicePort({required this.prepareOriginal, required this.readCatalogJson});
  final Future<AgentCollectorServiceNativeHandle> Function(String requestJson) prepareOriginal;
  final Future<String> Function(String requestJson) readCatalogJson;
  @override
  Future<AgentCollectorServiceCatalogView> readCatalog(RuntimeNoticeScope scope, AgentAssociationRecord record) async =>
      parseAgentCollectorServiceCatalogView(
        await guard(
          () => readCatalogJson(
            jsonEncode({
              'version': 1,
              'profileId': scope.profileId,
              'notificationScopeId': scope.nonce,
              'associationId': record.id,
            }),
          ),
        ),
        scope,
        record,
      );
  @override
  Future<AgentCollectorServiceHandle> prepare(RuntimeNoticeScope scope, AgentCollectorServiceIntent intent) async {
    if (intent.credentialUse != AgentCollectorServiceCredentialUse.singleStart) {
      throw const FormatException('SingleStart prepare cannot delegate service-lifetime credentials');
    }
    final original = await guard(() => prepareOriginal(jsonEncode(intent.request(scope))));
    return RustAgentCollectorServiceIssuedHandle(original, scope, intent);
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

/// Shared typed proxy for the SAME issued opaque handle in either prepare path.
/// No current Core lookup and no protocol/credential parameters on confirmation.
final class RustAgentCollectorServiceIssuedHandle implements AgentCollectorServiceHandle {
  RustAgentCollectorServiceIssuedHandle(this.original, this.scope, this.intent);
  final AgentCollectorServiceNativeHandle original;
  final RuntimeNoticeScope scope;
  final AgentCollectorServiceIntent intent;
  bool disposed = false;
  void _live() {
    if (disposed) throw StateError('Original service proxy disposed');
  }

  AgentCollectorServiceView _parse(String raw) {
    final view = parseAgentCollectorServiceView(raw, scope, intent.record);
    if (!intent.matches(view)) {
      throw const FormatException('Original service catalog changed');
    }
    return view;
  }

  @override
  Future<AgentCollectorServiceView> details() async {
    _live();
    return _parse(await guard(original.details));
  }

  @override
  Future<AgentCollectorServiceView> confirmConfiguration() async {
    _live();
    return _parse(await guard(original.confirmConfiguration));
  }

  @override
  void cancel() {
    _live();
    _sync(original.cancel);
  }

  @override
  Future<AgentCollectorServiceCleanup> joined() async {
    _live();
    return parseAgentCollectorServiceCleanup(await guard(original.joined));
  }

  @override
  void dispose() {
    if (disposed) return;
    disposed = true;
    _sync(original.dispose);
  }
}
