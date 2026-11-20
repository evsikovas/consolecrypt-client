import 'dart:convert';

import 'package:consolecrypt/automation/automation_project_credentials.dart';
import 'package:consolecrypt/core/bridge/rust_automation.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/project_environment_fixture.dart' as f;

// Actual generated signatures, synthetic response endpoint; no DLL/Native init.
class _Api extends Fake implements RustLibApi {
  final calls = <String>[];
  String? projectId;
  Map<String, dynamic>? saved;
  BridgeError? failure;
  @override
  Future<String> crateApiAutomationAutomationProjectCredentialsContext({required String id}) async {
    calls.add('context');
    projectId = id;
    if (failure case final error?) throw error;
    return jsonEncode(f.context());
  }

  @override
  Future<String> crateApiAutomationAutomationProjectCredentialsSave({required String requestJson}) async {
    calls.add('save');
    saved = jsonDecode(requestJson) as Map<String, dynamic>;
    if (failure case final error?) throw error;
    return jsonEncode({
      'version': 1,
      'project': {'id': f.projectId, 'revision': 2, 'read_only': false},
      'frame': f.frame(revision: 2),
      'policy': saved!['policy'],
    });
  }
}

void main() {
  late _Api api;
  setUp(() {
    api = _Api();
    RustLib.initMock(api: api);
    vaultKeyAlarm.value = null;
  });
  tearDown(() {
    RustLib.dispose();
    vaultKeyAlarm.value = null;
  });
  test(
    'actual production class exposes lazy optional context/save binding and encodes the original frame exactly once',
    () async {
      final service = RustAutomationService();
      expect(service, isA<AutomationProjectCredentialsService>());
      expect(api.calls, isEmpty);
      final AutomationProjectCredentialsService port = service;
      final raw = await port.projectCredentialsContext(f.projectId);
      expect(api.projectId, f.projectId);
      expect(api.calls, ['context']);
      final context = ProjectCredentialsContext.parse(raw, profileId: f.profileId, nonce: f.nonce, project: f.project);
      final request = context.saveRequest(context.policy), result = await port.saveProjectCredentials(request);
      expect(api.saved, request);
      expect(api.saved!['frame'], f.frame());
      expect(api.calls, ['context', 'save']);
      expect(context.saved(result).frame.revision, 2);
    },
  );
  test('actual generated error maps only the fixed Native field/rule and preserves vault alarm', () async {
    final AutomationProjectCredentialsService port = RustAutomationService();
    api.failure = const BridgeError(
      code: 'invalid_input',
      message: 'PRIVATE RAW must not render',
      details: {'field': 'automation_credentials', 'rule': 'project_environment_selection_changed'},
    );
    try {
      await port.projectCredentialsContext(f.projectId);
      fail('must reject');
    } on AppException catch (error) {
      expect(errorMessage(AppLocalizationsEn(), error), AppLocalizationsEn().automationDeploymentErrorChanged);
    }
    api.failure = const BridgeError(code: 'vault_key_mismatch', message: 'raw', details: {});
    await expectLater(
      port.projectCredentialsContext(f.projectId),
      throwsA(isA<AppException>().having((v) => v.code, 'code', AppErrorCode.vaultKeyMismatch)),
    );
    expect(vaultKeyAlarm.value?.code, AppErrorCode.vaultKeyMismatch);
  });
}
