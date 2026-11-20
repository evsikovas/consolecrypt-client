import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_sessions.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/src/rust/api/host_metrics.dart' as native;

final class RustHostMetricsReader implements HostMetricsReader {
  const RustHostMetricsReader(this._terminal);

  final RustTerminalService _terminal;

  @override
  Stream<HostMetricsSample> watchHost(
    Host host,
    TerminalSessionId sessionId,
    RuntimeNoticeScope scope,
    String operationId,
  ) async* {
    final nativeId = _terminal.connectedSshRuntimeId(sessionId, host.id);
    if (nativeId == null) {
      throw const AppException(AppErrorCode.notFound, 'SSH session has closed', reason: AppErrorReason.sessionClosed);
    }
    final request = jsonEncode({
      'terminalId': nativeId,
      'operationId': operationId,
      'hostId': host.id.value,
      'hostUpdatedAtMs': host.updatedAt.millisecondsSinceEpoch,
      'profileId': scope.profileId,
      'notificationScopeId': scope.nonce,
    });
    try {
      await for (final item in native.hostMetricsWatch(requestJson: request)) {
        yield HostMetricsSample.fromJson(jsonDecode(item) as Json);
      }
    } catch (error) {
      throw toAppException(error);
    }
  }

  @override
  Future<void> cancelHostWatch(String operationId) => guard(() => native.hostMetricsCancel(operationId: operationId));
}
