import 'dart:convert';

import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/core/bridge/mapping.dart' show guard;
import 'package:consolecrypt/src/rust/api/project_settings_sync.dart' as native;

Future<ProjectSettingsReview> prepareRustProjectSettingsSync(String profileId, String nonce) async {
  final request = jsonEncode({'version': 1, 'profileId': profileId, 'notificationScopeId': nonce});
  final review = await guard(() => native.projectSettingsSyncPrepare(requestJson: request));
  return _RustProjectSettingsReview(review);
}

final class _RustProjectSettingsReview implements ProjectSettingsReview {
  const _RustProjectSettingsReview(this._review);

  final native.ProjectSettingsSyncReview _review;

  @override
  Future<String> details() => guard(_review.details);

  @override
  Future<String> confirm() => guard(_review.confirm);

  @override
  Future<String> recheckCheckpoint() => guard(_review.recheckCheckpoint);

  @override
  void cancel() => _review.cancel();
}
