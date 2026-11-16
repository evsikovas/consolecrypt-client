import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/panel_policy_fixture.dart' as f;

void main() {
  test('Native domains preserve exact catalog u64, nonnegative i64 cursor, Off revision0 and blocked floor', () {
    final review = PanelSyncConsentReview.parse(jsonEncode(f.policy()), f.syncScope());
    expect(review.policyRevision, '0');
    expect(review.catalog.revision, '18446744073709551615');
    expect(review.checkpoint.epoch, f.epoch);
    expect(review.catalog.objectRevision, '9223372036854775807');
    expect(review.checkpoint.sequence, '9223372036854775807');
    expect(review.canConfirm, false);
    expect(review.canRecheck, false);
    for (final bad in [
      f.policy()..['ready'] = true,
      f.policy()..['version'] = 2,
      f.policy()..['notificationScopeId'] = f.syncNextNonce,
      f.policy()..['canConfirm'] = true,
      f.policy()..['scope'] = 'currentVault',
      f.policy()..['policyRevision'] = '18446744073709551616',
    ]) {
      expect(() => PanelSyncConsentReview.parse(jsonEncode(bad), f.syncScope()), throwsA(anything));
    }
  });
  test(
    'construction is no RPC; explicit read carries original scope only and blocked floor sends zero actions',
    () async {
      final port = f.Port(),
          owner = PanelSyncConsentController(port: f.Port(), scope: f.syncScope(), isOriginal: () => true);
      owner.dispose();
      expect(port.requests, isEmpty);
      final current = PanelSyncConsentController(port: port, scope: f.syncScope(), isOriginal: () => true);
      await current.prepare();
      expect(port.requests, [
        {'version': 1, 'profileId': f.syncProfile, 'notificationScopeId': f.syncNonce},
      ]);
      await current.act(recheck: false);
      await current.act(recheck: true);
      expect(port.handle.confirms, 0);
      expect(port.handle.rechecks, 0);
      current.dispose();
    },
  );
  test('late prepare and asynchronous details cannot adopt replacement profile owner', () async {
    for (final details in [false, true]) {
      var original = true;
      final port = f.Port();
      if (details) {
        port.handle.pendingDetails = Completer<String>();
      } else {
        port.pending = Completer<MonitoringPanelSyncConsentHandle>();
      }
      final owner = PanelSyncConsentController(port: port, scope: f.syncScope(), isOriginal: () => original);
      final future = owner.prepare();
      await Future<void>.delayed(Duration.zero);
      original = false;
      if (details) {
        port.handle.pendingDetails!.complete(jsonEncode(f.policy()));
      } else {
        port.pending!.complete(port.handle);
      }
      await future;
      expect(owner.review, isNull);
      expect(port.handle.cancels, 1);
      expect(port.handle.confirms, 0);
      owner.dispose();
    }
  });
  test('confirmation and recapture are separate one-use Native actions, never a panel write grant', () async {
    final port = f.Port(value: f.policy(confirm: true)),
        owner = PanelSyncConsentController(port: f.Port(), scope: f.syncScope(), isOriginal: () => true);
    owner.dispose();
    final c = PanelSyncConsentController(port: port, scope: f.syncScope(), isOriginal: () => true);
    await c.prepare();
    await c.act(recheck: false);
    expect(c.result?.rechecked, false);
    expect(port.handle.confirms, 1);
    expect(port.handle.rechecks, 0);
    await c.act(recheck: false);
    await c.prepare();
    expect(port.requests, hasLength(1));
    expect(port.handle.confirms, 1);
    c.dispose();
    final recapture = f.Port(value: f.policy(enabled: true));
    final r = PanelSyncConsentController(port: recapture, scope: f.syncScope(), isOriginal: () => true);
    await r.prepare();
    expect(r.canConfirm, false);
    expect(r.canRecheck, true);
    await r.act(recheck: true);
    expect(r.result?.rechecked, true);
    expect(recapture.handle.confirms, 0);
    expect(recapture.handle.rechecks, 1);
    r.dispose();
  });
  test('post-action transport uncertainty is spent and stale results cannot claim success', () async {
    final port = f.Port(value: f.policy(confirm: true))..handle.lose = true;
    final owner = PanelSyncConsentController(port: port, scope: f.syncScope(), isOriginal: () => true);
    await owner.prepare();
    await owner.act(recheck: false);
    expect(owner.failure, PanelSyncConsentFailure.outcomeUnknown);
    await owner.act(recheck: false);
    await owner.prepare();
    expect(port.handle.confirms, 1);
    expect(port.requests, hasLength(1));
    owner.dispose();
    var original = true;
    final late = f.Port(value: f.policy(confirm: true))..handle.pendingAction = Completer<String>();
    final c = PanelSyncConsentController(port: late, scope: f.syncScope(), isOriginal: () => original);
    await c.prepare();
    final pending = c.act(recheck: false);
    original = false;
    late.handle.pendingAction!.complete(f.result());
    await pending;
    expect(c.result, isNull);
    expect(c.failure, PanelSyncConsentFailure.retired);
    expect(late.handle.cancels, 1);
    c.dispose();
  });
  testWidgets('original deadline expires blocked or actionable review without renewal or hidden reads', (tester) async {
    final start = tester.binding.clock.now(), port = f.Port(value: f.policy(confirm: true)..['remainingMs'] = 1000);
    final owner = PanelSyncConsentController(
      port: port,
      scope: f.syncScope(),
      isOriginal: () => true,
      elapsedForReview: () => tester.binding.clock.now().difference(start),
    );
    await owner.prepare();
    expect(owner.canConfirm, true);
    await tester.pump(const Duration(seconds: 1));
    expect(owner.canConfirm, false);
    expect(owner.failure, PanelSyncConsentFailure.expired);
    await owner.act(recheck: false);
    expect(port.handle.confirms, 0);
    expect(port.requests, hasLength(1));
    expect(port.handle.detailsCalls, 1);
    expect(port.handle.cancels, 1);
    owner.dispose();
  });
}
