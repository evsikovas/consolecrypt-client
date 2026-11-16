import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';

import 'agent_association_fixture.dart' as a;
import 'agent_collector_service_fixture.dart' as v;
import 'agent_metrics_fixture.dart' as m;

final scope = a.scope;
final record = m.metricRecord();
final catalog = parseAgentCollectorServiceView(jsonEncode(v.serviceWire()), scope, record).catalog;
AgentCollectorServiceIntent intent() => AgentCollectorServiceIntent(record, catalog);
Never throwFixtureError(Object error) {
  if (error is Exception) throw error;
  if (error is Error) throw error;
  throw StateError('Unsupported owned fixture error');
}

/// SYNTHETIC closed DTOs from actual operation44724 +view5356. No genuine Node
/// signatures, service configuration, process, FFI or generated API acceptance.
String catalogReply({String cleanup = 'closedObserved'}) {
  final source = v.serviceWire();
  return jsonEncode({
    'version': 1,
    'profileId': scope.profileId,
    'notificationScopeId': scope.nonce,
    'kind': 'finiteCollectorServiceCatalog',
    'associationId': record.id,
    'hostId': record.hostId,
    'hostRevision': record.hostRevision,
    'node': source['node'],
    'collectorId': catalog.collectorId,
    'manifestSha256': catalog.manifestSha256,
    'manifest': source['manifest'],
    'cleanupState': cleanup,
  });
}

String joinedReply({String phase = 'cancelled', String state = 'closedObserved', bool mayRemain = false}) =>
    jsonEncode({
      'version': 1,
      'phase': phase,
      'cleanupState': state,
      'serviceMayRemainActive': mayRemain,
      'runtimeState': 'notObserved',
      'deliveryGuaranteed': false,
      'lossUnknown': true,
    });

final class NativeHandle implements AgentCollectorServiceNativeHandle {
  final calls = <String>[];
  Completer<String>? pendingDetails, pendingConfirm, pendingJoin;
  Object? confirmError;
  int remainingMs = 60000;
  String phase = 'serviceReview';
  bool mutationMay = false;
  @override
  Future<String> details() async {
    calls.add('details');
    return pendingDetails?.future ?? jsonEncode({...v.serviceWire(), 'remainingMs': remainingMs});
  }

  @override
  Future<String> confirmConfiguration() async {
    calls.add('confirmConfiguration');
    mutationMay = true;
    phase = 'configuring';
    if (confirmError != null) {
      phase = 'unknown';
      throwFixtureError(confirmError!);
    }
    if (pendingConfirm != null) return pendingConfirm!.future;
    phase = 'configurationRecorded';
    return jsonEncode(v.serviceWire(phase: phase));
  }

  @override
  void cancel() {
    calls.add('cancel');
    if (!mutationMay) phase = 'cancelled';
  }

  @override
  Future<String> joined() async {
    calls.add('joined');
    return pendingJoin?.future ??
        joinedReply(phase: phase, state: phase == 'configuring' ? 'unknown' : 'closedObserved', mayRemain: mutationMay);
  }

  @override
  void dispose() {
    calls.add('dispose');
  }
}

final class Calls {
  Calls(this.handle);
  final NativeHandle handle;
  final catalogRequests = <Map<String, dynamic>>[], prepareRequests = <Map<String, dynamic>>[];
  Object? catalogError;
  String? catalogOverride;
  Completer<String>? pendingCatalog;
  Completer<AgentCollectorServiceNativeHandle>? pendingPrepare;
  Future<String> read(String raw) async {
    catalogRequests.add(jsonDecode(raw) as Map<String, dynamic>);
    if (catalogError != null) throwFixtureError(catalogError!);
    return pendingCatalog?.future ?? catalogOverride ?? catalogReply();
  }

  Future<AgentCollectorServiceNativeHandle> prepare(String raw) async {
    prepareRequests.add(jsonDecode(raw) as Map<String, dynamic>);
    return pendingPrepare?.future ?? handle;
  }

  AgentCollectorServicePort get port => RustAgentCollectorServicePort(prepareOriginal: prepare, readCatalogJson: read);
}
