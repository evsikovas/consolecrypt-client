import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// E2EE sync engine of the active profile (sync-core outbox worker +
/// WebSocket events). Local profiles report [SyncState.localOnly].
abstract interface class SyncService {
  Stream<SyncStatus> watchStatus();

  SyncStatus get currentStatus;

  /// Push the outbox and pull changes now (otherwise automatic).
  Future<void> syncNow();

  /// Local → Synced (ADR-0106): sign in / register, `POST /v1/vaults` with
  /// the existing vault id + envelopes, upload every object as a create.
  /// Falls back to the reconnect/merge path if the vault already exists.
  /// Interrupted uploads resume idempotently. The stream ends with
  /// [EnableSyncStep.done] or [EnableSyncStep.failed].
  Stream<EnableSyncProgress> enableSync({
    required Uri serverUrl,
    required String email,
    required SecretText password,
    required String deviceName,
    required bool createAccount,
  });

  /// Synced → Local: stops sync and keeps all local data. Optionally revokes
  /// this device on the server; the server copy is left untouched.
  Future<void> disconnect({required bool revokeThisDevice});
}
