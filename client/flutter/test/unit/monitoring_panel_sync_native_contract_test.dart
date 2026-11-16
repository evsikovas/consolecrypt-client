import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/panel_policy_fixture.dart' as f;

void main() {
  test('Native UUID checkpoint epoch roundtrips while former decimal epoch is rejected', () {
    final value = f.policy();
    final review = PanelSyncConsentReview.parse(jsonEncode(value), f.syncScope());
    expect(review.checkpoint.epoch, f.epoch);
    expect(review.checkpoint.sequence, '9223372036854775807');
    for (final epoch in [
      '18446744073709551615',
      '00000000-0000-0000-0000-000000000000',
      f.epoch.toUpperCase(),
      '{${f.epoch}}',
    ]) {
      final bad = f.policy();
      (bad['checkpoint'] as Json)['vaultEpoch'] = epoch;
      expect(() => PanelSyncConsentReview.parse(jsonEncode(bad), f.syncScope()), throwsA(anything));
    }
  });
  test('actual server i64 and closed policy/state constraints reject invented synthetic domains', () {
    for (final kind in [
      'sequence',
      'offRevision',
      'enabledRevision',
      'zeroEnabled',
      'unknownState',
      'roster',
      'falseAction',
      'oversizeVersion',
    ]) {
      final value = f.policy(enabled: kind == 'enabledRevision' || kind == 'zeroEnabled');
      switch (kind) {
        case 'sequence':
          (value['checkpoint'] as Json)['vaultSequence'] = '9223372036854775808';
        case 'offRevision':
          value['policyRevision'] = '1';
        case 'enabledRevision':
          value['policyRevision'] = '9223372036854775808';
        case 'zeroEnabled':
          value['policyRevision'] = '0';
        case 'unknownState':
          value['accountState'] = 'ready';
        case 'roster':
          (value['readiness'] as Json)['checkedActiveDevices'] = 1;
        case 'falseAction':
          value['canRecheck'] = true;
          value['kind'] = 'checkpointRecapture';
        case 'oversizeVersion':
          value['offeredMinimumClientVersion'] = '18446744073709551616.0.0';
      }
      expect(() => PanelSyncConsentReview.parse(jsonEncode(value), f.syncScope()), throwsA(anything), reason: kind);
    }
    final enabled = PanelSyncConsentReview.parse(jsonEncode(f.policy(enabled: true)), f.syncScope());
    expect(enabled.policyRevision, '9223372036854775807');
    expect(enabled.canConfirm, false);
  });
  test(
    'result cursor epoch and policy revision match actual Native confirm and original-policy recapture producers',
    () {
      final review = PanelSyncConsentReview.parse(jsonEncode(f.policy(enabled: true)), f.syncScope());
      final good = PanelSyncConsentResult.parse(f.result(recheck: true), review, true);
      expect(good.checkpoint!.epoch, f.epoch);
      expect(good.checkpoint, review.checkpoint);
      for (final fault in ['epoch', 'sequence', 'revision']) {
        final value = object(jsonDecode(f.result(recheck: true)));
        if (fault == 'epoch') {
          (value['checkpoint'] as Json)['vaultEpoch'] = f.vault;
        }
        if (fault == 'sequence') {
          (value['checkpoint'] as Json)['vaultSequence'] = '0';
        }
        if (fault == 'revision') {
          value['policyRevision'] = '1';
        }
        expect(() => PanelSyncConsentResult.parse(jsonEncode(value), review, true), throwsA(anything));
      }
    },
  );
}
