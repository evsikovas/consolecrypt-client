import 'dart:async';

import 'package:consolecrypt/ai/device_local_ai_service.dart';
import 'package:consolecrypt/core/services/ai_service.dart';
import 'package:consolecrypt/monitoring/metrics_ai_context.dart';
import 'package:consolecrypt/monitoring/metrics_ai_leased_transport.dart';
import 'package:consolecrypt/monitoring/metrics_ai_review_flow.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/fake_local_ai_runtime.dart';
import '../helpers/test_app.dart';
import 'metrics_ai_context_test.dart' as fixture;

class _Bridge implements DeviceLocalAiBridge {
  bool registered = false;
  int registrations = 0;
  @override
  Future<void> registerDeviceLocalProvider({
    required String profileId,
    required String vaultId,
    required String registrationId,
    required int port,
    required String apiKey,
    required String model,
  }) async {
    registered = true;
    registrations++;
  }

  @override
  Future<void> clearDeviceLocalProvider(String registrationId) async {
    registered = false;
  }
}

class _Native implements MetricsAiReviewTransport {
  _Native(this.registered);
  final bool Function() registered;
  int prepares = 0, sends = 0, discards = 0;
  @override
  Future<MetricsAiPreparedReview> prepare(MetricsAiFence original, MetricsAiRequest request) async {
    expect(registered(), isTrue, reason: 'Native must see the registered selected endpoint');
    prepares++;
    return MetricsAiPreparedReview(
      ticket: 'own-$prepares',
      previewText: 'exact frozen local context',
      external: false,
      expiresUtc: DateTime.utc(2030),
    );
  }

  @override
  Future<MetricsAiReadonlyProposal> send(String ticket, {required bool approveExternal}) async {
    expect(registered(), isTrue, reason: 'the exact endpoint must survive human review');
    sends++;
    return MetricsAiReadonlyProposal.fromWire({
      'explanation': 'readonly',
      'proposedPlaybook': null,
      'limitations': <String>[],
    });
  }

  @override
  Future<void> discard(String ticket) async {
    discards++;
  }
}

void main() {
  MetricsAiFence fence() => MetricsAiFence(
    profileLease: Object(),
    metricsLease: Object(),
    providerLease: Object(),
    sourceFingerprint: fixture.source.fingerprint,
  );
  MetricsAiRequest request(MetricsAiFence f) => MetricsAiRequest(
    action: MetricsAiAction.explain,
    question: 'Explain selected values',
    projection: MetricsAiProjection.selected(
      f,
      metrics: [],
      alerts: [MetricsAiAlertSelection.fromAccepted(fixture.alerts(), f, alertIndex: 0)],
      createdUtc: DateTime.utc(2026),
    ),
  );

  test('actual DeviceLocalAiService registration precedes Native preview and holds runtime lease', () async {
    final backend = testBackend();
    await backend.debugSignInDemoAndUnlock();
    final runtime = FakeLocalAiRuntime(), bridge = _Bridge();
    final local = DeviceLocalAiService(
      delegate: backend.services.ai,
      bridge: bridge,
      runtime: runtime,
      settings: backend.services.settings,
      profiles: backend.services.profiles,
      vault: backend.services.vault,
    );
    final native = _Native(() => bridge.registered);
    Future<void>? holder;
    final transport = MetricsAiLeasedTransport(
      delegate: native,
      isCurrent: () => true,
      withProvider: (body) => holder = local.withDeviceLocalProvider(body),
    );
    final original = fence();
    final preview = await transport.prepare(original, request(original));
    expect(bridge.registrations, 1);
    expect(runtime.leases, 1);
    expect(native.sends, 0);
    await transport.send(preview.ticket, approveExternal: false);
    expect(runtime.leases, 1);
    transport.dispose();
    await holder;
    expect(runtime.leases, 0);
    expect(bridge.registrations, 1);
    await local.dispose();
    await runtime.dispose();
    await backend.dispose();
  });

  test('one selected endpoint remains registered across prepare human review send and close', () async {
    var registered = false, registrations = 0;
    final ended = Completer<void>();
    final native = _Native(() => registered);
    final transport = MetricsAiLeasedTransport(
      delegate: native,
      isCurrent: () => true,
      withProvider: (body) async {
        registrations++;
        registered = true;
        try {
          await body();
        } finally {
          registered = false;
          ended.complete();
        }
      },
    );
    final original = fence();
    final preview = await transport.prepare(original, request(original));
    expect(registrations, 1);
    expect(registered, isTrue);
    expect(native.sends, 0);
    await transport.discard(preview.ticket);
    final next = await transport.prepare(original, request(original));
    expect(registrations, 1, reason: 'an explicit new preview reuses the still-held endpoint');
    final result = await transport.send(next.ticket, approveExternal: false);
    expect(result.executable, isFalse);
    expect(registered, isTrue);
    transport.dispose();
    await ended.future;
    expect(registered, isFalse);
    expect(native.prepares, 2);
    expect(native.sends, 1);
    expect(native.discards, 1);
    await expectLater(transport.send(next.ticket, approveExternal: false), throwsA(isA<MetricsAiFlowException>()));
    expect(registrations, 1, reason: 'closed review must not restart a model or use a fallback');
  });

  test('closing while local startup is pending settles caller and rejects the late endpoint', () async {
    final started = Completer<void>(), start = Completer<void>(), ended = Completer<void>();
    var registered = false;
    final native = _Native(() => registered);
    final transport = MetricsAiLeasedTransport(
      delegate: native,
      isCurrent: () => true,
      withProvider: (body) async {
        started.complete();
        await start.future;
        registered = true;
        try {
          await body();
        } finally {
          registered = false;
          ended.complete();
        }
      },
    );
    final original = fence();
    final preparing = transport.prepare(original, request(original));
    final denied = expectLater(preparing, throwsA(isA<MetricsAiFlowException>()));
    await started.future;
    transport.dispose();
    await denied;
    start.complete();
    await ended.future;
    expect(native.prepares, 0);
    expect(native.sends, 0);
    expect(registered, isFalse);
  });

  test('startup failure is sanitized and never reaches Native or another provider', () async {
    final native = _Native(() => false);
    final transport = MetricsAiLeasedTransport(
      delegate: native,
      isCurrent: () => true,
      withProvider: (_) async {
        throw StateError('private path and local endpoint diagnostics must stay hidden');
      },
    );
    final original = fence();
    await expectLater(transport.prepare(original, request(original)), throwsA(isA<MetricsAiFlowException>()));
    expect(native.prepares, 0);
    expect(native.sends, 0);
    transport.dispose();
  });

  test('changed original scope after preview refuses send without reacquiring an endpoint', () async {
    var current = true, registered = false, registrations = 0;
    final ended = Completer<void>();
    final native = _Native(() => registered);
    final transport = MetricsAiLeasedTransport(
      delegate: native,
      isCurrent: () => current,
      withProvider: (body) async {
        registrations++;
        registered = true;
        try {
          await body();
        } finally {
          registered = false;
          ended.complete();
        }
      },
    );
    final original = fence();
    final preview = await transport.prepare(original, request(original));
    current = false;
    await expectLater(transport.send(preview.ticket, approveExternal: false), throwsA(isA<MetricsAiFlowException>()));
    transport.dispose();
    await ended.future;
    expect(native.sends, 0);
    expect(registrations, 1);
  });
}
