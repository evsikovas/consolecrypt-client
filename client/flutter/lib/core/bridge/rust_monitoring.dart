import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard;
import 'package:consolecrypt/core/bridge/rust_agent_association.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_generated.dart';
import 'package:consolecrypt/core/bridge/rust_agent_exporter_generated.dart';
import 'package:consolecrypt/core/bridge/rust_agent_metrics_collector.dart';
import 'package:consolecrypt/core/bridge/rust_agent_outbound_bootstrap.dart';
import 'package:consolecrypt/core/bridge/rust_agent_reader.dart';
import 'package:consolecrypt/core/bridge/rust_agent_setup.dart';
import 'package:consolecrypt/core/bridge/rust_agent_tunnel_saved_route_generated.dart';
import 'package:consolecrypt/core/bridge/rust_monitoring_consent.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/metrics_ai_native_transport.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/src/rust/api/monitoring.dart' as native;

/// Native owns saved configuration, credential resolution and transport.
final class RustMonitoringBackend implements MonitoringBackend {
  const RustMonitoringBackend({this.noticeEvents});
  final RuntimeNoticeEvents? noticeEvents;
  @override
  MonitoringService createService() => AppCoreMonitoringService(
    agentSetupPort: switch (noticeEvents?.currentScope.scope) {
      final RuntimeNoticeScope scope => RustAgentSetupPort(scope),
      _ => null,
    },
    agentAssociationPort: const RustAgentAssociationPort(),
    agentOutboundBootstrapPort: createGeneratedAgentOutboundBootstrapPort(),
    agentExporterPort: createGeneratedAgentExporterPort(),
    agentReaderPort: const RustAgentReaderPort(),
    agentTunnelRoutePort: createGeneratedSavedTunnelRoutePort(),
    agentMetricsCollectorPort: const RustAgentMetricsCollectorPort(),
    agentCollectorServicePort: createGeneratedAgentCollectorServicePort(),
    read: (request) async =>
        object(jsonDecode(await guard(() => native.monitoringRead(requestJson: jsonEncode(request))))),
    cancelOperation: (id) => guard(() => native.monitoringCancel(operationId: id)),
    hostContextRead: (request) async =>
        object(jsonDecode(await guard(() => native.monitoringHostContext(requestJson: jsonEncode(request))))),
    metricsAiRpc: (operation, request) async {
      final raw = jsonEncode(request);
      switch (operation) {
        case MetricsAiNativeOperation.prepare:
          return object(jsonDecode(await guard(() => native.monitoringAiPrepare(requestJson: raw))));
        case MetricsAiNativeOperation.send:
          return object(jsonDecode(await guard(() => native.monitoringAiSend(requestJson: raw))));
        case MetricsAiNativeOperation.discard:
          await guard(() => native.monitoringAiDiscard(requestJson: raw));
          return <String, Object?>{};
      }
    },
  );
  @override
  MonitoringSourceStore createSourceStore() => AppCoreMonitoringSourceStore(
    (method, arguments) async {
      final value = await guard(
        () => switch (method) {
          'monitoring_sources' => native.monitoringSources(),
          'monitoring_source_create' => native.monitoringSourceCreate(
            expectedCatalogRevision: arguments['expectedCatalogRevision'] as int,
            configurationJson: arguments['configurationJson'] as String,
          ),
          'monitoring_source_update' => native.monitoringSourceUpdate(
            id: arguments['id'] as String,
            expectedCatalogRevision: arguments['expectedCatalogRevision'] as int,
            expectedSourceRevision: arguments['expectedSourceRevision'] as int,
            configurationJson: arguments['configurationJson'] as String,
          ),
          'monitoring_source_delete' => native.monitoringSourceDelete(
            id: arguments['id'] as String,
            expectedCatalogRevision: arguments['expectedCatalogRevision'] as int,
            expectedSourceRevision: arguments['expectedSourceRevision'] as int,
          ),
          'monitoring_binding_create' => native.monitoringBindingCreate(
            requestJson: arguments['requestJson'] as String,
          ),
          'monitoring_binding_update' => native.monitoringBindingUpdate(
            requestJson: arguments['requestJson'] as String,
          ),
          'monitoring_binding_delete' => native.monitoringBindingDelete(
            requestJson: arguments['requestJson'] as String,
          ),
          'monitoring_panel_create' => native.monitoringPanelCreate(requestJson: arguments['requestJson'] as String),
          'monitoring_panel_update' => native.monitoringPanelUpdate(requestJson: arguments['requestJson'] as String),
          'monitoring_panel_delete' => native.monitoringPanelDelete(requestJson: arguments['requestJson'] as String),
          _ => throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse),
        },
      );
      return object(jsonDecode(value));
    },
    syncCheck: (request) async =>
        object(jsonDecode(await guard(() => native.monitoringSettingsSyncCheck(requestJson: jsonEncode(request))))),
    panelSyncConsentPort: const RustMonitoringConsentPort(),
  );
}
