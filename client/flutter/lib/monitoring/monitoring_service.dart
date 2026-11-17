import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/agent_association_models.dart' show checkedAssociationId;
import 'package:consolecrypt/monitoring/agent_association_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_exporter.dart';
import 'package:consolecrypt/monitoring/agent_heartbeat_observation.dart';
import 'package:consolecrypt/monitoring/agent_metrics_collector_port.dart';
import 'package:consolecrypt/monitoring/agent_outbound_bootstrap.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:consolecrypt/monitoring/agent_setup_port.dart';
import 'package:consolecrypt/monitoring/agent_tunnel_route_port.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/metrics_ai_native_transport.dart';
import 'package:consolecrypt/monitoring/metrics_ai_review_flow.dart';
import 'package:consolecrypt/monitoring/monitoring_alert_models.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_exporter_review.dart';
import 'package:consolecrypt/monitoring/monitoring_host_context.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';

abstract interface class MonitoringService {
  Future<SourceCapabilities> probe(MonitoringSource source);
  Future<QueryOutcome> query(MonitoringSource source, MonitoringQuery query);
  Future<LabelValues> labelValues(MonitoringSource source, String label);
  void cancel();
}

/// Explicit one-read transport override; the saved source is never mutated.
abstract interface class MonitoringAgentRouteReader {
  Future<QueryOutcome> queryViaAgentRoute(MonitoringSource source, MonitoringQuery query, {required String routeId});
}

/// Optional local label-value cache controls on the original profile service.
abstract interface class MonitoringMetadataCache {
  Duration get labelCacheTtl;
  Future<void> clearCache(MonitoringSource source);
}

/// Optional read-only backend-status capability on the same profile service.
abstract interface class MonitoringAlertReader {
  Future<MonitoringAlertSnapshot> readAlerts(MonitoringSource source, MonitoringAlertKind kind);
}

abstract interface class MonitoringSavedPanelReader {
  Future<SavedPanelRead> readSavedPanel(MonitoringSource source, SavedMonitoringPanel panel, {SavedPanelRange? window});
}

abstract interface class MonitoringSavedPanelStore {
  List<SavedMonitoringPanel> get panels;
  Future<SavedMonitoringPanel> savePanel(SavedPanelDefinition definition, {SavedMonitoringPanel? previous});
  Future<void> deletePanel(SavedMonitoringPanel panel);
}

abstract interface class MonitoringBindingReader {
  Future<BindingReadResult> discover(MonitoringSource source, MonitoringBinding binding, double time);
  Future<BindingReadResult> readPanel(
    MonitoringSource source,
    MonitoringBinding binding,
    MonitoringPanel panel, {
    required double start,
    required double end,
    required double stepSeconds,
  });
  void cancelBinding();
}

String monitoringOperationId() {
  final random = Random.secure();
  final bytes = List.generate(16, (_) => random.nextInt(256));
  bytes[6] = (bytes[6] & 15) | 64;
  bytes[8] = (bytes[8] & 63) | 128;
  final hex = bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}';
}

typedef MonitoringCatalogRpc = Future<Json> Function(String method, Json arguments);

/// Production seam: reads contain only a saved ID/revision and operation UUID.
/// Native storage, authorization, credential resolution and transport stay in AppCore.
class AppCoreMonitoringService
    implements
        MonitoringService,
        MonitoringAgentRouteReader,
        MonitoringMetadataCache,
        AgentReaderCapability,
        AgentHeartbeatObservationCapability,
        AgentAssociationCapability,
        AgentExporterCapability,
        AgentCollectorServiceCapability,
        AgentTunnelRouteCapability,
        AgentOutboundBootstrapCapability,
        AgentMetricsCollectorCapability,
        MonitoringBindingReader,
        MonitoringAlertReader,
        MonitoringSavedPanelReader,
        AgentSetupCapability,
        MonitoringHostContextReader,
        MetricsAiNativeCapability,
        MonitoringExporterReader {
  AppCoreMonitoringService({
    required this.read,
    required this.cancelOperation,
    this.hostContextRead,
    this.metricsAiRpc,
    this.agentReaderPort,
    this.agentAssociationPort,
    this.agentExporterPort,
    this.agentCollectorServicePort,
    this.agentTunnelRoutePort,
    this.agentOutboundBootstrapPort,
    this.agentMetricsCollectorPort,
    this.agentSetupPort,
  });
  @override
  final AgentSetupPort? agentSetupPort;
  @override
  final AgentReaderNativePort? agentReaderPort;
  @override
  final agentHeartbeatObservations = AgentHeartbeatObservations();
  @override
  final AgentAssociationPort? agentAssociationPort;
  @override
  final AgentExporterPort? agentExporterPort;
  @override
  final AgentCollectorServicePort? agentCollectorServicePort;
  @override
  final AgentTunnelRoutePort? agentTunnelRoutePort;
  @override
  final AgentOutboundBootstrapPort? agentOutboundBootstrapPort;
  @override
  final AgentMetricsCollectorPort? agentMetricsCollectorPort;
  final MetricsAiNativeRpc? metricsAiRpc;
  @override
  bool get metricsAiSupported => metricsAiRpc != null;
  @override
  Future<Json> metricsAiRequest(MetricsAiNativeOperation operation, Json request) {
    final rpc = metricsAiRpc;
    if (rpc == null) throw const MetricsAiFlowException(MetricsAiFlowIssue.transport);
    return rpc(operation, request);
  }

  final Future<Json> Function(Json request)? hostContextRead;
  final Future<Json> Function(Json request) read;
  final Future<void> Function(String operationId) cancelOperation;
  @override
  bool get hostContextSupported => hostContextRead != null;
  Future<Json> _hostMetadata(Json request) async {
    final rpc = hostContextRead;
    if (rpc == null) throw const MonitoringHostContextException(MonitoringHostContextIssue.unsupported);
    return rpc(request);
  }

  @override
  Future<JobContextMetadata> captureHostContext({
    required String profileId,
    required MonitoringSource source,
    required MonitoringBinding binding,
  }) async {
    final request = monitoringHostCaptureRequest(profileId, source, binding);
    return monitoringHostCaptureResponse(await _hostMetadata(request), request);
  }

  @override
  Future<JobContextMetadata> recheckHostContext(JobContextMetadata original) async {
    final request = monitoringHostRecheckRequest(original);
    final current = monitoringHostCaptureResponse(await _hostMetadata(request), request);
    if (!sameJobContextMetadata(original, current)) {
      throw const MonitoringHostContextException(MonitoringHostContextIssue.changed);
    }
    return current;
  }

  final _pending = <String>{};
  final _bindingPending = <String>{};
  final _exporterPending = <String>{};
  Future<Json> _read(
    MonitoringSource source,
    Json action, {
    bool binding = false,
    bool exporter = false,
    String? selectedAgentRouteId,
  }) async {
    final operationId = monitoringOperationId();
    _pending.add(operationId);
    if (binding) _bindingPending.add(operationId);
    if (exporter) _exporterPending.add(operationId);
    try {
      final response = await read({
        'operationId': operationId,
        'sourceId': source.id,
        'sourceRevision': source.revision,
        'selectedAgentRouteId': ?selectedAgentRouteId,
        'action': action,
      });
      if (response['sourceId'] != source.id || response['sourceRevision'] != source.revision) {
        throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
      }
      final result = object(response['result']);
      if (result['status'] == null && result['error'] is Map && object(result['error'])['code'] != null) {
        throw MonitoringFailure.fromJson(object(result['error']));
      }
      return result;
    } finally {
      _pending.remove(operationId);
      _bindingPending.remove(operationId);
      _exporterPending.remove(operationId);
    }
  }

  @override
  Future<MonitoringExporterContext> exporterContext(
    MonitoringSource source, {
    required String profileId,
    required String hostId,
    required String jobId,
    required int jobRevision,
  }) async {
    final context = MonitoringExporterContext.fromJson(
      await _read(source, {
        'kind': 'exporterContext',
        'version': 1,
        'profileId': profileId,
        'hostId': hostId,
        'jobId': jobId,
        'jobRevision': jobRevision,
      }, exporter: true),
    );
    if (context.profileId != profileId ||
        context.hostId != hostId ||
        context.jobId != jobId ||
        context.jobRevision != jobRevision ||
        context.sourceId != source.id ||
        context.sourceRevision != source.revision) {
      throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
    }
    return context;
  }

  @override
  Future<MonitoringExporterObservation> verifyExporter(
    MonitoringSource source,
    MonitoringExporterContext original,
    MonitoringExporterKind exporter,
    Map<String, String> selector, {
    int maxAgeSeconds = 120,
  }) async {
    if (original.sourceId != source.id || original.sourceRevision != source.revision) {
      throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
    }
    return MonitoringExporterObservation.fromJson(
      await _read(source, original.verification(exporter, selector, maxAgeSeconds), exporter: true),
      original,
      exporter,
      maxAgeSeconds,
    );
  }

  @override
  void cancelExporter() {
    for (final id in _exporterPending.toList()) {
      unawaited(cancelOperation(id).catchError((Object _) {}));
    }
  }

  @override
  Future<MonitoringAlertSnapshot> readAlerts(MonitoringSource source, MonitoringAlertKind kind) async =>
      MonitoringAlertSnapshot.fromJson(kind, await _read(source, {'kind': kind.name}));

  @override
  Future<SavedPanelRead> readSavedPanel(
    MonitoringSource source,
    SavedMonitoringPanel panel, {
    SavedPanelRange? window,
  }) async {
    if (!panel.currentFor(source)) throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
    return SavedPanelRead.fromJson(
      await _read(source, {
        'kind': 'savedPanel',
        'panelId': panel.id,
        'panelRevision': panel.revision,
        if (window != null) 'range': window.toJson(),
      }),
      panel,
    );
  }

  @override
  Future<SourceCapabilities> probe(MonitoringSource source) async =>
      SourceCapabilities.fromJson(await _read(source, {'kind': 'probe'}));
  @override
  Future<QueryOutcome> query(MonitoringSource source, MonitoringQuery query) async {
    query.validate();
    return QueryOutcome.fromJson(await _read(source, query.toOperation()));
  }

  @override
  Future<QueryOutcome> queryViaAgentRoute(
    MonitoringSource source,
    MonitoringQuery query, {
    required String routeId,
  }) async {
    query.validate();
    checkedAssociationId(routeId);
    return QueryOutcome.fromJson(await _read(source, query.toOperation(), selectedAgentRouteId: routeId));
  }

  @override
  Future<LabelValues> labelValues(MonitoringSource source, String label) async =>
      LabelValues.fromJson(await _read(source, {'kind': 'labelValues', 'label': label}));
  @override
  Duration get labelCacheTtl => const Duration(seconds: 60);
  @override
  Future<void> clearCache(MonitoringSource source) async {
    final result = await _read(source, {'kind': 'clearCache'});
    if (result.length != 1 || result['cleared'] != true) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
    }
  }

  void _bindingContext(MonitoringSource source, MonitoringBinding binding, double end) {
    binding.validate();
    if (source.id != binding.sourceId ||
        source.revision != binding.sourceRevision ||
        binding.state != MonitoringBindingState.current) {
      throw const MonitoringFailure.local(MonitoringErrorCode.bindingChanged);
    }
    if (!end.isFinite || end.abs() > 253402300799) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidQuery);
    }
  }

  BindingReadResult _bindingResult(Json json, MonitoringBinding binding, {MonitoringPanel? panel}) {
    final result = BindingReadResult.fromJson(json);
    if (result.bindingId != binding.id ||
        result.bindingRevision != binding.revision ||
        result.panel != panel ||
        result.kind != (panel == null ? 'bindingDiscovery' : 'bindingPanel')) {
      throw const MonitoringFailure.local(MonitoringErrorCode.bindingChanged);
    }
    return result;
  }

  @override
  Future<BindingReadResult> discover(MonitoringSource source, MonitoringBinding binding, double time) async {
    _bindingContext(source, binding, time);
    return _bindingResult(
      await _read(source, {
        'kind': 'bindingDiscover',
        'bindingId': binding.id,
        'bindingRevision': binding.revision,
        'time': time,
      }, binding: true),
      binding,
    );
  }

  @override
  Future<BindingReadResult> readPanel(
    MonitoringSource source,
    MonitoringBinding binding,
    MonitoringPanel panel, {
    required double start,
    required double end,
    required double stepSeconds,
  }) async {
    _bindingContext(source, binding, end);
    if (!start.isFinite || start.abs() > 253402300799 || end < start || !stepSeconds.isFinite || stepSeconds <= 0) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidQuery);
    }
    return _bindingResult(
      await _read(source, {
        'kind': 'bindingPanel',
        'bindingId': binding.id,
        'bindingRevision': binding.revision,
        'panel': panel.name,
        'start': start,
        'end': end,
        'stepSeconds': stepSeconds,
      }, binding: true),
      binding,
      panel: panel,
    );
  }

  @override
  void cancelBinding() {
    for (final id in _bindingPending.toList()) {
      unawaited(cancelOperation(id).catchError((Object _) {}));
    }
  }

  @override
  void cancel() {
    agentHeartbeatObservations.clear();
    for (final id in _pending.toList()) {
      unawaited(cancelOperation(id).catchError((Object _) {}));
    }
  }
}

abstract interface class MonitoringSourceStore {
  bool get readOnly;
  Future<List<MonitoringSource>> load();
  Future<MonitoringSource> upsert(MonitoringSource source, {bool create = false});
  Future<void> delete(MonitoringSource source);
}

abstract interface class MonitoringBindingStore {
  List<MonitoringBinding> get bindings;
  Future<MonitoringBinding> saveBinding(MonitoringBinding binding, {bool create = false});
  Future<void> deleteBinding(MonitoringBinding binding);
}

abstract interface class MonitoringExporterBindingStore {
  Future<MonitoringBinding> saveExporterBinding(MonitoringBinding binding, MonitoringExporterContext original);
}

typedef MonitoringPanelSyncCheckRpc = Future<Json> Function(Json request);

abstract interface class MonitoringPanelSyncDiagnostics {
  MonitoringPanelSyncSnapshot? get panelSync;
  bool get canCheckPanelSync;
  Future<MonitoringPanelSyncSnapshot> checkPanelSync({
    required RuntimeNoticeScope original,
    required bool Function() isCurrent,
  });
}

class AppCoreMonitoringSourceStore
    implements
        MonitoringSourceStore,
        MonitoringBindingStore,
        MonitoringSavedPanelStore,
        MonitoringExporterBindingStore,
        MonitoringPanelSyncDiagnostics,
        MonitoringPanelSyncConsentCapability {
  AppCoreMonitoringSourceStore(this.rpc, {this.syncCheck, this.panelSyncConsentPort});
  @override
  final MonitoringPanelSyncConsentPort? panelSyncConsentPort;
  final MonitoringCatalogRpc rpc;
  final MonitoringPanelSyncCheckRpc? syncCheck;
  int _syncGeneration = 0;
  @override
  MonitoringPanelSyncSnapshot? panelSync;
  @override
  bool get canCheckPanelSync => syncCheck != null;
  @override
  Future<MonitoringPanelSyncSnapshot> checkPanelSync({
    required RuntimeNoticeScope original,
    required bool Function() isCurrent,
  }) async {
    final check = syncCheck;
    if (check == null) throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedResult);
    if (!isCurrent()) throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
    final token = ++_syncGeneration;
    final profile = original.profileId, nonce = original.nonce;
    final raw = await check({'version': 1, 'profileId': profile, 'notificationScopeId': nonce});
    if (!isCurrent() || token != _syncGeneration) {
      throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
    }
    final result = MonitoringPanelSyncSnapshot.fromJson(raw);
    if (!result.matches(profile, nonce)) throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
    panelSync = result;
    return result;
  }

  int? _revision;
  Set<String> _knownIds = {};
  List<MonitoringBinding> _bindings = const [];
  List<SavedMonitoringPanel> _panels = const [];
  @override
  List<SavedMonitoringPanel> get panels => _panels;
  @override
  List<MonitoringBinding> get bindings => _bindings;
  @override
  bool readOnly = true;
  List<MonitoringSource> _accept(Json catalog) {
    try {
      if (![1, 2, 3].contains(catalog['schemaVersion']) ||
          catalog.keys.toSet().difference({
            'schemaVersion',
            'revision',
            'readOnly',
            'sources',
            'bindings',
            'panelSync',
            if (catalog['schemaVersion'] == 3) 'panels',
          }).isNotEmpty ||
          catalog.containsKey('bindings') && catalog['bindings'] is! List) {
        throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedCatalogVersion);
      }
      _syncGeneration++;
      panelSync = null;
      if (catalog['panelSync'] != null) {
        try {
          panelSync = MonitoringPanelSyncSnapshot.fromJson(object(catalog['panelSync']));
        } catch (_) {
          /* Optional diagnostic data cannot invalidate runnable source metadata. */
        }
      }
      final version = catalog['schemaVersion'] as int;
      final rows = catalog['bindings'] as List? ?? (version == 1 ? [] : null);
      if (rows == null || rows.length > 256 || version == 1 && rows.isNotEmpty) {
        throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedCatalogVersion);
      }
      final panelRows = catalog['panels'];
      if (version == 3 && (panelRows is! List || panelRows.length > 64)) {
        throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedCatalogVersion);
      }
      final panels = (panelRows as List? ?? []).map((row) => SavedMonitoringPanel.fromJson(object(row))).toList();
      if (panels.map((p) => p.id).toSet().length != panels.length) {
        throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedCatalogVersion);
      }
      final bindings = rows.map((r) => MonitoringBinding.fromRecord(object(r))).toList();
      final selectors = bindings.map((binding) {
        final keys = binding.selector.keys.toList()..sort();
        return jsonEncode([
          binding.hostId,
          binding.sourceId,
          {for (final k in keys) k: binding.selector[k]},
        ]);
      }).toSet();
      if (bindings.map((b) => b.id).toSet().length != bindings.length || selectors.length != bindings.length) {
        throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedCatalogVersion);
      }
      final sources = (catalog['sources'] as List).map((s) => MonitoringSource.fromRecord(object(s))).toList();
      final revision = catalog['revision'] as int, readonly = catalog['readOnly'] as bool;
      _revision = revision;
      readOnly = readonly;
      _bindings = List.unmodifiable(bindings);
      _panels = List.unmodifiable(panels);
      _knownIds = sources.map((s) => s.id).toSet();
      return sources;
    } catch (_) {
      readOnly = true;
      panelSync = null;
      _syncGeneration++;
      _revision = null;
      _bindings = const [];
      _panels = const [];
      _knownIds = {};
      throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedCatalogVersion);
    }
  }

  @override
  Future<List<MonitoringSource>> load() async => _accept(await rpc('monitoring_sources', {}));
  @override
  Future<MonitoringSource> upsert(MonitoringSource source, {bool create = false}) async {
    if (readOnly || _revision == null || source.readOnly) {
      throw const MonitoringFailure.local(MonitoringErrorCode.readOnly);
    }
    source.validate(allowEmptyId: create);
    final previousIds = Set<String>.of(_knownIds);
    final config = source.toJson();
    if (create) config['id'] = '';
    final sources = _accept(
      await rpc(create ? 'monitoring_source_create' : 'monitoring_source_update', {
        'expectedCatalogRevision': _revision,
        'configurationJson': jsonEncode(config),
        if (!create) 'id': source.id,
        if (!create) 'expectedSourceRevision': source.revision,
      }),
    );
    if (!create) return sources.singleWhere((s) => s.id == source.id);
    // Native assigns the identity. The catalog diff is authoritative even if
    // another source has identical user-facing configuration.
    final created = sources.where((s) => !previousIds.contains(s.id)).toList();
    if (created.length != 1) {
      throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
    }
    return created.single;
  }

  @override
  Future<void> delete(MonitoringSource source) async {
    if (readOnly || _revision == null || source.readOnly) {
      throw const MonitoringFailure.local(MonitoringErrorCode.readOnly);
    }
    if (_bindings.any((binding) => binding.sourceId == source.id) ||
        _panels.any((p) => p.definition.sourceId == source.id)) {
      throw const MonitoringFailure.local(MonitoringErrorCode.sourceInUse);
    }
    _accept(
      await rpc('monitoring_source_delete', {
        'id': source.id,
        'expectedCatalogRevision': _revision,
        'expectedSourceRevision': source.revision,
      }),
    );
  }

  @override
  Future<SavedMonitoringPanel> savePanel(SavedPanelDefinition definition, {SavedMonitoringPanel? previous}) async {
    if (readOnly || _revision == null) throw const MonitoringFailure.local(MonitoringErrorCode.readOnly);
    definition.validate();
    final ids = _panels.map((p) => p.id).toSet();
    _accept(
      await rpc(previous == null ? 'monitoring_panel_create' : 'monitoring_panel_update', {
        'requestJson': jsonEncode({
          'expectedCatalogRevision': _revision,
          'definition': definition.toJson(),
          if (previous != null) 'id': previous.id,
          if (previous != null) 'expectedPanelRevision': previous.revision,
        }),
      }),
    );
    final saved = _panels.where((p) => previous == null ? !ids.contains(p.id) : p.id == previous.id).toList();
    if (saved.length != 1) throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
    return saved.single;
  }

  @override
  Future<void> deletePanel(SavedMonitoringPanel panel) async {
    if (readOnly || _revision == null) throw const MonitoringFailure.local(MonitoringErrorCode.readOnly);
    _accept(
      await rpc('monitoring_panel_delete', {
        'requestJson': jsonEncode({
          'id': panel.id,
          'expectedCatalogRevision': _revision,
          'expectedPanelRevision': panel.revision,
        }),
      }),
    );
  }

  @override
  Future<MonitoringBinding> saveBinding(MonitoringBinding binding, {bool create = false}) =>
      _saveBinding(binding, create: create);
  @override
  Future<MonitoringBinding> saveExporterBinding(MonitoringBinding binding, MonitoringExporterContext original) {
    if (binding.hostId != original.hostId ||
        binding.sourceId != original.sourceId ||
        binding.sourceRevision != original.sourceRevision) {
      throw const MonitoringFailure.local(MonitoringErrorCode.bindingChanged);
    }
    return _saveBinding(binding, create: true, exporterOrigin: original);
  }

  Future<MonitoringBinding> _saveBinding(
    MonitoringBinding binding, {
    bool create = false,
    MonitoringExporterContext? exporterOrigin,
  }) async {
    if (readOnly || _revision == null) throw const MonitoringFailure.local(MonitoringErrorCode.readOnly);
    binding.validate(create: create);
    final previousIds = _bindings.map((b) => b.id).toSet();
    _accept(
      await rpc(create ? 'monitoring_binding_create' : 'monitoring_binding_update', {
        'requestJson': jsonEncode({
          'expectedCatalogRevision': _revision,
          if (exporterOrigin != null) 'exporterOrigin': exporterOrigin.bindingOrigin,
          if (!create) 'id': binding.id,
          if (!create) 'expectedBindingRevision': binding.revision,
          'name': binding.name,
          'hostId': binding.hostId,
          'sourceId': binding.sourceId,
          'expectedSourceRevision': binding.sourceRevision,
          'selector': binding.selector,
        }),
      }),
    );
    final saved = create
        ? _bindings.where((b) => !previousIds.contains(b.id)).toList()
        : _bindings.where((b) => b.id == binding.id).toList();
    if (saved.length != 1) throw const MonitoringFailure.local(MonitoringErrorCode.bindingChanged);
    return saved.single;
  }

  @override
  Future<void> deleteBinding(MonitoringBinding binding) async {
    if (readOnly || _revision == null) throw const MonitoringFailure.local(MonitoringErrorCode.readOnly);
    _accept(
      await rpc('monitoring_binding_delete', {
        'requestJson': jsonEncode({
          'id': binding.id,
          'expectedCatalogRevision': _revision,
          'expectedBindingRevision': binding.revision,
        }),
      }),
    );
  }
}

abstract interface class MonitoringBackend {
  MonitoringService createService();
  MonitoringSourceStore createSourceStore();
}
