import 'dart:async';

import 'package:consolecrypt/monitoring/metrics_ai_context.dart';
import 'package:consolecrypt/monitoring/metrics_ai_native_transport.dart';
import 'package:consolecrypt/monitoring/metrics_ai_review_flow.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

import 'metrics_ai_context_test.dart' as fixture;

String id(int value) => '00000000-0000-4000-8000-${value.toRadixString(16).padLeft(12, '0')}';

void main() {
  final source = MonitoringSource(id: id(2), revision: 4, name: 'Owned', endpoint: 'https://private.invalid');
  MetricsAiFence fence() => MetricsAiFence(
    profileLease: Object(),
    metricsLease: Object(),
    providerLease: Object(),
    sourceFingerprint: source.fingerprint,
  );
  MetricsAiRequest request(MetricsAiFence original) => MetricsAiRequest(
    action: MetricsAiAction.explain,
    question: 'Explain explicitly selected observations',
    projection: MetricsAiProjection.selected(
      original,
      metrics: [],
      alerts: [MetricsAiAlertSelection.fromAccepted(fixture.alerts(), original, alertIndex: 0)],
      createdUtc: DateTime.utc(2026, 11, 12),
    ),
  );
  Json preview() => {
    'version': 1,
    'ticket': 'opaque',
    'previewText': 'Native sanitized exact preview',
    'external': true,
    'expiresUtc': '2026-11-12T15:00:00Z',
  };
  test('missing native issuer never falls back to read or generic chat', () async {
    var reads = 0;
    final service = AppCoreMonitoringService(
      read: (_) async {
        reads++;
        return {};
      },
      cancelOperation: (_) async {},
    );
    final original = fence();
    final transport = MetricsAiNativeTransport(
      capability: service,
      original: original,
      profileId: id(1),
      source: source,
      providerId: id(3),
      isCurrent: () => true,
    );
    expect(service.metricsAiSupported, isFalse);
    await expectLater(transport.prepare(original, request(original)), throwsA(isA<MetricsAiFlowException>()));
    expect(reads, 0);
  });
  test('prepare local refs are outside exact selected data, send contains only receipt and consent', () async {
    final calls = <(MetricsAiNativeOperation, Json)>[];
    final service = AppCoreMonitoringService(
      read: (_) async => throw StateError('No metrics HTTP'),
      cancelOperation: (_) async {},
      metricsAiRpc: (op, wire) async {
        calls.add((op, wire));
        return op == MetricsAiNativeOperation.prepare
            ? preview()
            : op == MetricsAiNativeOperation.send
            ? {
                'version': 1,
                'explanation': 'Read-only explanation',
                'proposedPlaybook': null,
                'limitations': <String>[],
              }
            : {};
      },
    );
    final original = fence();
    final transport = MetricsAiNativeTransport(
      capability: service,
      original: original,
      profileId: id(1),
      source: source,
      providerId: id(3),
      isCurrent: () => true,
    );
    final review = await transport.prepare(original, request(original));
    expect(calls.single.$2.keys.toSet(), {
      'version',
      'profileId',
      'sourceId',
      'sourceRevision',
      'providerId',
      'action',
      'question',
      'selectedData',
    });
    expect(calls.single.$2['selectedData'].toString(), isNot(contains('private.invalid')));
    final proposal = await transport.send(review.ticket, approveExternal: true);
    expect(proposal.executable, isFalse);
    expect(calls.last.$2, {'version': 1, 'profileId': id(1), 'ticket': 'opaque', 'approveExternal': true});
    await transport.discard(review.ticket);
    expect(calls.last.$2, {'version': 1, 'profileId': id(1), 'ticket': 'opaque'});
  });
  test('late original-session completion denied without adopting replacement scope', () async {
    var current = true;
    final pending = Completer<Json>();
    var calls = 0;
    final operations = <MetricsAiNativeOperation>[];
    final service = AppCoreMonitoringService(
      read: (_) async => {},
      cancelOperation: (_) async {},
      metricsAiRpc: (operation, wire) {
        calls++;
        operations.add(operation);
        if (operation == MetricsAiNativeOperation.discard) {
          expect(wire, {'version': 1, 'profileId': id(1), 'ticket': 'opaque'});
          return Future.value(<String, Object?>{});
        }
        return pending.future;
      },
    );
    final original = fence();
    final transport = MetricsAiNativeTransport(
      capability: service,
      original: original,
      profileId: id(1),
      source: source,
      providerId: id(3),
      isCurrent: () => current,
    );
    final preparing = transport.prepare(original, request(original));
    current = false;
    pending.complete(preview());
    await expectLater(preparing, throwsA(isA<MetricsAiFlowException>()));
    await expectLater(transport.send('opaque', approveExternal: true), throwsA(isA<MetricsAiFlowException>()));
    expect(calls, 2);
    expect(operations, [MetricsAiNativeOperation.prepare, MetricsAiNativeOperation.discard]);
  });
  test('malformed issuer preview and tool-bearing result are refused', () async {
    final original = fence();
    var wire = preview()..['extra'] = 'untrusted';
    final service = AppCoreMonitoringService(
      read: (_) async => {},
      cancelOperation: (_) async {},
      metricsAiRpc: (_, _) async => wire,
    );
    final transport = MetricsAiNativeTransport(
      capability: service,
      original: original,
      profileId: id(1),
      source: source,
      providerId: id(3),
      isCurrent: () => true,
    );
    await expectLater(transport.prepare(original, request(original)), throwsA(isA<MetricsAiFlowException>()));
    wire = {
      'version': 1,
      'explanation': 'owned',
      'proposedPlaybook': null,
      'limitations': <String>[],
      'tools': <Object>[],
    };
    await expectLater(transport.send('opaque', approveExternal: true), throwsA(isA<MetricsAiFlowException>()));
  });
  test('foreign projection/provider fence denied before issuer request', () async {
    var calls = 0;
    final original = fence();
    final service = AppCoreMonitoringService(
      read: (_) async => {},
      cancelOperation: (_) async {},
      metricsAiRpc: (_, _) async {
        calls++;
        return preview();
      },
    );
    final transport = MetricsAiNativeTransport(
      capability: service,
      original: original,
      profileId: id(1),
      source: source,
      providerId: id(3),
      isCurrent: () => true,
    );
    await expectLater(transport.prepare(original, request(fence())), throwsA(isA<MetricsAiFlowException>()));
    expect(calls, 0);
    transport.dispose();
    await expectLater(transport.discard('opaque'), throwsA(isA<MetricsAiFlowException>()));
    expect(calls, 0);
  });
}
