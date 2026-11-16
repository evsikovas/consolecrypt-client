import 'package:consolecrypt/core/bridge/mapping.dart' show guard, toAppException;
import 'package:consolecrypt/core/security/vault_key_alarm.dart';
import 'package:consolecrypt/monitoring/monitoring_panel_sync_consent.dart';
import 'package:consolecrypt/src/rust/api/monitoring_consent.dart' as native;

/// Lazy actual generated entry point; construction cannot read or change policy.
final class RustMonitoringConsentPort implements MonitoringPanelSyncConsentPort {
  const RustMonitoringConsentPort({this.prepareNative = _prepare});
  final Future<native.MonitoringSettingsConsentHandle> Function(String) prepareNative;
  static Future<native.MonitoringSettingsConsentHandle> _prepare(String request) =>
      native.monitoringSettingsPrepareConsent(requestJson: request);
  @override
  Future<MonitoringPanelSyncConsentHandle> prepare(String requestJson) async =>
      _OriginalConsent(await guard(() => prepareNative(requestJson)));
}

/// All operations use the exact issued Native owner, never the global Core.
final class _OriginalConsent implements MonitoringPanelSyncConsentHandle {
  const _OriginalConsent(this.original);
  final native.MonitoringSettingsConsentHandle original;
  @override
  Future<String> details() => guard(original.details);
  @override
  Future<String> confirm() => guard(original.confirm);
  @override
  Future<String> recheckCheckpoint() => guard(original.recheckCheckpoint);
  @override
  void cancel() {
    try {
      original.cancel();
    } catch (error) {
      final mapped = toAppException(error);
      reportVaultKeyMismatch(mapped);
      throw mapped;
    }
  }
}
