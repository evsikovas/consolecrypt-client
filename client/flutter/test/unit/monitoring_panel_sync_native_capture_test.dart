import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('actual owned Native details serializer is compatible with the production Dart parser', () {
    // Untouched output from the real owned-loopback Native test, not a Dart DTO fixture.
    // This asserts wire compatibility only; it cannot establish a live profile scope.
    final bytes = File('test/fixtures/monitoring_consent_native_details.json')
        .readAsBytesSync();
    expect(
      sha256.convert(bytes).toString(),
      '811546122ed0ea017f2721226322652c11621dc0af49a15b3e11a12dcb60e91e',
    );
    final raw = utf8.decode(bytes);
    expect(utf8.encode(raw).length, lessThanOrEqualTo(16384));
    final wire = object(jsonDecode(raw));
    final capturedFrame = RuntimeNoticeScope.fromProfile({
      'id': wire['profileId'],
      'active': true,
      'vault_state': 'unlocked',
      'notification_scope_id': wire['notificationScopeId'],
    });
    expect(capturedFrame, isNotNull);
    final parsed = PanelSyncConsentReview.parse(raw, capturedFrame!);
    expect(parsed.checkpoint.epoch, object(wire['checkpoint'])['vaultEpoch']);
    expect(
      parsed.checkpoint.epoch,
      matches(RegExp(r'^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$')),
    );
    expect(parsed.policyRevision, '0');
    expect(parsed.checkpoint.sequence, '9007199254740993');
    expect(parsed.snapshot.account, PanelSyncAccount.off);
    expect(parsed.snapshot.releasedFloor, isNull);
    expect(parsed.snapshot.readiness, PanelSyncReadiness.floorUndeclared);
    expect(parsed.canConfirm, isFalse);
    expect(parsed.canRecheck, isFalse);
    expect(parsed.kind, PanelSyncConsentKind.blocked);
    // Causal former mismatch: an otherwise actual body must reject a numeric epoch.
    wire['checkpoint'] = object(wire['checkpoint'])
      ..['vaultEpoch'] = '18446744073709551615';
    expect(
      () => PanelSyncConsentReview.parse(jsonEncode(wire), capturedFrame),
      throwsFormatException,
    );
  });
}
