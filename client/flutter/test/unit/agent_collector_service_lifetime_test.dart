import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_controller.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_collector_service_credentials_fixture.dart' as c;
import '../support/agent_collector_service_flow_fixture.dart' as f;
import '../support/agent_collector_service_lifetime_fixture.dart' as x;

void main() {
  test('closed lifetime review requires exact purpose, original expiry, explicit refs and fixed safeguards', () {
    AgentCollectorServiceView parse(Map<String, dynamic> m) =>
        parseAgentCollectorServiceView(jsonEncode(m), f.scope, f.record);
    final v = parse(x.lifetimeWire());
    expect(v.lifetimeCredentials!.purpose, x.purpose);
    expect(v.lifetimeCredentials!.retentionUntilUtc, v.expiresAtUtc);
    expect(v.bootStamp, isNull);
    expect(v.runtimeObserved, isFalse);
    for (final change in <Map<String, dynamic>>[
      {'credentialPurpose': 'other'},
      {'requiresExplicitLifetimeConsent': false},
      {'automaticCredentialRefresh': true},
      {'encryptedCredentialRetentionUntilUtc': '2030-01-03T00:00:00Z'},
      {'credentialSelections': <Object>[]},
      {'reviewId': '00000000-0000-0000-0000-000000000000'},
      {'extraAuthority': true},
      {'kind': 'finiteCollectorServiceCredentials'},
    ]) {
      expect(() => parse({...x.lifetimeWire(), ...change}), throwsFormatException);
    }
    for (final key in ['reviewId', 'credentialPurpose', 'automaticCredentialRefresh']) {
      expect(() => parse(x.lifetimeWire()..remove(key)), throwsFormatException);
    }
    final plain = parse(c.selectedWire());
    expect(plain.lifetimeCredentials, isNull);
    expect(x.lifetimeIntent().matches(plain), isFalse);
    expect(AgentCollectorServiceIntent(f.record, f.catalog, credentials: x.refs()).matches(v), isFalse);
    expect(() => AgentCollectorServiceIntent(f.record, f.catalog,
        credentialUse: AgentCollectorServiceCredentialUse.serviceLifetime), throwsFormatException);
  });

  test('distinct callback gets only existing request refs; same issued handle confirms with no mode or replacement refs', () async {
    final calls = x.LifetimeCalls(), intent = x.lifetimeIntent();
    await expectLater(calls.base.prepare(f.scope, intent), throwsFormatException);
    expect(calls.singleRequests, isEmpty);
    final h = await calls.port.prepareLifetime(f.scope, intent);
    expect(calls.lifetimeRequests.single, intent.request(f.scope));
    expect(calls.lifetimeRequests.single.keys, unorderedEquals([
      'version', 'profileId', 'notificationScopeId', 'associationId', 'collectorId', 'manifestSha256', 'credentials',
    ]));
    expect(calls.lifetimeRequests.single.toString(), isNot(contains('serviceLifetime')));
    final v = await h.details();
    expect(intent.matches(v), isTrue);
    final done = await h.confirmConfiguration();
    expect(done.configurationRecorded, isTrue); // Synthetic metadata only.
    expect(done.runtimeObserved, isFalse);
    await h.joined();
    h.dispose();
    expect(calls.lifetime.calls, ['details', 'confirmConfiguration', 'joined', 'dispose']);
    expect(calls.single.calls, isEmpty);
  });

  test('controller freezes lifetime identity and does not renew original 60s on refreshed details', () async {
    final calls = x.LifetimeCalls();
    var elapsed = Duration.zero;
    final owner = AgentCollectorServiceController(
      port: calls.port, scope: f.scope, intent: x.lifetimeIntent(), original: () => true, elapsed: () => elapsed,
    );
    await owner.prepare();
    expect(owner.canConfirm, isTrue);
    elapsed = const Duration(seconds: 55);
    await owner.refreshReview();
    expect(owner.remainingMs, 5000);
    elapsed = const Duration(seconds: 61);
    expect(owner.canConfirm, isFalse);
    await owner.confirmConfiguration();
    expect(calls.lifetime.calls.where((c) => c == 'confirmConfiguration'), isEmpty);
    await owner.close();
    owner.dispose();
  });

  test('late lifetime issuance after cancel joins and disposes exactly the original proxy without confirmation', () async {
    final calls = x.LifetimeCalls();
    calls.pendingPrepare = Completer<AgentCollectorServiceNativeHandle>();
    final owner = AgentCollectorServiceController(
      port: calls.port, scope: f.scope, intent: x.lifetimeIntent(), original: () => true,
    );
    final pending = owner.prepare();
    owner.cancel();
    calls.pendingPrepare!.complete(calls.lifetime);
    await pending;
    await owner.close();
    expect(calls.lifetime.calls, ['cancel', 'joined', 'dispose']);
    expect(owner.canConfirm, isFalse);
    owner.dispose();
  });

  test('changed lifetime review ID is rejected before confirm instead of adopting a new review', () async {
    final calls = x.LifetimeCalls();
    final owner = AgentCollectorServiceController(
      port: calls.port, scope: f.scope, intent: x.lifetimeIntent(), original: () => true,
    );
    await owner.prepare();
    calls.lifetime.pendingDetails = Completer<String>()..complete(jsonEncode({
      ...x.lifetimeWire(), 'reviewId': 'b0000000-0000-4000-8000-00000000000b',
    }));
    await owner.refreshReview();
    expect(owner.canConfirm, isFalse);
    expect(owner.error, isNotNull);
    expect(calls.lifetime.calls.where((c) => c == 'confirmConfiguration'), isEmpty);
    await owner.close();
    owner.dispose();
  });
}
