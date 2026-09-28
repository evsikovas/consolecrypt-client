import 'package:consolecrypt/core/models/models.dart';

/// Device-local preferences and vault-wide synced settings.
abstract interface class SettingsService {
  Stream<LocalSettings> watchLocal();

  LocalSettings get currentLocal;

  Future<void> updateLocal(LocalSettings settings);

  /// `null` while the vault is locked.
  Stream<VaultSettings?> watchVault();

  Future<void> updateVault(VaultSettings settings);
}
