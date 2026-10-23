import 'package:consolecrypt/core/services/errors.dart';
import 'package:flutter/foundation.dart';

/// The last vault key mismatch (audit H2, TR-9) reported by the core —
/// through an error of any core call or the `vault_key_mismatch` event.
/// `VaultKeyAlarmScope` (lib/vault/vault_key_mismatch.dart) shows the
/// blocking screen for it.
final vaultKeyAlarm = ValueNotifier<AppException?>(null);

/// Report [error] if it is a vault key mismatch; returns whether it was.
bool reportVaultKeyMismatch(Object error) {
  if (error is AppException && error.code == AppErrorCode.vaultKeyMismatch) {
    vaultKeyAlarm.value = error;
    return true;
  }
  return false;
}
