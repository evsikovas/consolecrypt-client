import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/keepass/external_vault.dart';
import 'package:crypto/crypto.dart' show sha256;
import 'package:flutter_riverpod/flutter_riverpod.dart';

final externalVaultRepositoryProvider = Provider<ExternalVaultRepository>((ref) {
  final profile = ref.watch(activeProfileProvider.select((p) => p?.id.value));
  final scope = ref.watch(vaultStatusProvider.select((s) => (s.value?.vaultId?.value, s.value?.isUnlocked)));
  return ExternalVaultRepository(ref, profile, scope.$2 == true ? scope.$1 : null);
});

final externalVaultConnectionsProvider = Provider<List<ExternalVaultConnection>>((ref) {
  final repository = ref.watch(externalVaultRepositoryProvider);
  final local = ref.watch(localSettingsProvider).value;
  final shared = ref.watch(vaultSettingsProvider).value;
  if (!repository.current || local == null) return const [];
  final records = repository.localConnections(local);
  if (shared != null && records.any((c) => !c.synced)) repository.scheduleMigration();
  final merged = {
    for (final c in records)
      if (!c.synced) c.id: c,
  };
  final byId = {for (final c in records) c.id: c};
  for (final row in shared?.externalVaults ?? const <SyncedExternalVault>[]) {
    try {
      final config = repository.resolve(row);
      final localConfig = byId[config.id];
      merged[config.id] = localConfig?.binding == config.binding
          ? repository.resolve(row, wrapped: localConfig?.wrapped)
          : config;
    } on ExternalVaultFailure {
      // Invalid settings from older or incompatible clients are not login targets.
    }
  }
  return merged.values.toList();
});

/// Shared connection metadata lives in the encrypted vault. Only this device
/// can use its own biometric wrapper. Legacy catalog entries are migrated once.
class ExternalVaultRepository {
  ExternalVaultRepository(this.ref, this.profile, this.vault);
  final Ref ref;
  final String? profile, vault;
  Future<void> _tail = Future.value();
  bool _migrating = false;

  bool get current {
    if (!ref.mounted || profile == null || vault == null) return false;
    final status = ref.read(vaultServiceProvider).currentStatus;
    return status.isUnlocked &&
        status.vaultId?.value == vault &&
        ref.read(profileServiceProvider).currentProfiles.active?.id.value == profile;
  }

  void _check([ExternalVaultConnection? connection]) {
    if (!current || connection != null && (connection.profile != profile || connection.vault != vault)) {
      throw const ExternalVaultFailure('closed');
    }
  }

  Future<void> _edit(Future<void> Function() operation) {
    final next = _tail.then((_) => operation());
    _tail = next.then<void>((_) {}, onError: (Object _, StackTrace _) {});
    return next;
  }

  List<ExternalVaultConnection> localConnections(LocalSettings local) {
    final out = <ExternalVaultConnection>[];
    for (final row in local.externalVaultConnections) {
      if (row['profile'] != profile || row['vault'] != vault) continue;
      try {
        out.add(ExternalVaultConnection.fromStorage(row));
      } catch (_) {
        // Ignore incomplete settings left by older clients.
      }
    }
    return out;
  }

  ExternalVaultConnection resolve(SyncedExternalVault row, {String? wrapped}) =>
      ExternalVaultConnection.fromPortable(row, profile: profile!, vault: vault!, wrapped: wrapped);

  Future<VaultSettings> _shared() async {
    _check();
    final data = await ref.read(settingsServiceProvider).watchVault().first;
    _check();
    if (data == null) throw const ExternalVaultFailure('closed');
    return data;
  }

  void scheduleMigration() {
    if (_migrating) return;
    _migrating = true;
    _edit(() async {
      final shared = await _shared();
      final settings = ref.read(settingsServiceProvider);
      final legacy = localConnections(settings.currentLocal).where((c) => !c.synced).toList();
      if (legacy.isEmpty) return;
      final byId = {for (final entry in shared.externalVaults) entry.id: entry};
      for (final c in legacy) {
        byId.putIfAbsent(c.id, () => c.portable());
      }
      if (byId.length > 256) throw const ExternalVaultFailure('store');
      if (byId.length != shared.externalVaults.length) {
        await settings.updateVault(shared.copyWith(externalVaults: byId.values.toList()));
      }
      _check();
      final local = settings.currentLocal;
      final migrated = legacy.map((c) => c.id).toSet();
      await settings.updateLocal(
        local.copyWith(
          externalVaultConnections: [
            for (final row in local.externalVaultConnections)
              if (row['profile'] == profile && row['vault'] == vault && migrated.contains(row['id']))
                {...row, 'synced': '1'}
              else
                row,
          ],
        ),
      );
    }).whenComplete(() => _migrating = false).ignore();
  }

  String _owner(ExternalVaultConnection config) =>
      'consolecrypt-vault:${sha256.convert(utf8.encode(config.portableBinding))}';

  Future<bool> _owned(ExternalVaultConnection config, String id) async {
    _check(config);
    final credentials = await ref.read(inventoryServiceProvider).watchCredentials().first;
    _check(config);
    return credentials.any(
      (c) => c.id.value == id && c.kind == CredentialKind.password && c.username == _owner(config),
    );
  }

  ExternalVaultConnection? _record(VaultSettings shared, String id) {
    final row = shared.externalVaults.where((item) => item.id == id).firstOrNull;
    return row == null ? null : resolve(row);
  }

  bool _matches(ExternalVaultConnection expected, ExternalVaultConnection? saved) =>
      expected.binding == saved?.binding && expected.credentialId == saved?.credentialId;

  Future<bool> contains(ExternalVaultConnection connection) async {
    _check(connection);
    final shared = _record(await _shared(), connection.id);
    if (shared != null) return connection.binding == shared.binding;
    return localConnections(ref.read(settingsServiceProvider).currentLocal)
        .any((c) => !c.synced && c.binding == connection.binding);
  }

  Future<SecretText> unlock(ExternalVaultConnection expected) async {
    _check(expected);
    final id = expected.credentialId;
    if (id == null || !_matches(expected, _record(await _shared(), expected.id)) || !await _owned(expected, id)) {
      throw const ExternalVaultFailure('authentication');
    }
    _check(expected);
    final secret = await ref.read(inventoryServiceProvider).revealCredentialSecret(ObjectId(id));
    try {
      if (!_matches(expected, _record(await _shared(), expected.id))) throw const ExternalVaultFailure('conflict');
      return secret;
    } catch (_) {
      secret.wipe();
      rethrow;
    }
  }

  Future<String> _create(ExternalVaultConnection config, SecretText login) async {
    _check(config);
    final bytes = login.exposeBytes();
    final copy = SecretText.fromBytes(bytes);
    bytes.fillRange(0, bytes.length, 0);
    try {
      final entry = await ref
          .read(inventoryServiceProvider)
          .createPasswordCredential(name: 'Vault / OpenBao · ${config.name}', username: _owner(config), password: copy);
      return entry.id.value;
    } finally {
      copy.wipe();
    }
  }

  Future<void> _clean(ExternalVaultConnection config, String id) async {
    try {
      if (!await _owned(config, id)) return;
      final shared = await _shared();
      if (shared.passwordVaults.any((c) => c.credentialId == id) ||
          shared.externalVaults.any((c) => c.credentialId == id)) {
        return;
      }
      // Core also refuses deletion of credentials in use outside this catalog.
      await ref.read(inventoryServiceProvider).deleteCredential(ObjectId(id));
    } catch (_) {
      // Retain the separate encrypted credential if ownership or references cannot be checked.
    }
  }

  Future<void> save(
    ExternalVaultConnection config, {
    ExternalVaultConnection? expected,
    SecretText? syncCredential,
    bool remove = false,
    bool revoke = false,
    LocalSettings Function(LocalSettings)? updateLocal,
  }) => _edit(() async {
    _check(config);
    final settings = ref.read(settingsServiceProvider);
    final beforeLocal = settings.currentLocal;
    // Validate device storage before creating a credential or committing a
    // portable edit. Saves sharing this device's catalog run on the same queue.
    final nextLocal = updateLocal?.call(beforeLocal);
    var shared = await _shared();
    final previous = _record(shared, config.id);
    if (expected != null &&
        (expected.synced || previous != null) &&
        (!_matches(expected, previous) || expected.name != previous?.name)) {
      throw const ExternalVaultFailure('conflict');
    }
    if (!remove && previous == null && shared.externalVaults.length >= 256) throw const ExternalVaultFailure('store');
    var id = !revoke && previous?.binding == config.binding ? previous?.credentialId : null;
    final oldId = previous?.credentialId;
    String? created;
    var committed = false;
    try {
      if (syncCredential != null) {
        var same = false;
        if (id != null && await _owned(config, id)) {
          final old = await ref.read(inventoryServiceProvider).revealCredentialSecret(ObjectId(id));
          try {
            _check(config);
            same = old.constantTimeEquals(syncCredential);
          } finally {
            old.wipe();
          }
        }
        if (!same) {
          created = await _create(config, syncCredential);
          id = created;
        }
      }
      shared = await _shared();
      final latest = _record(shared, config.id);
      if (previous == null ? latest != null : !_matches(previous, latest) || previous.name != latest?.name) {
        throw const ExternalVaultFailure('conflict');
      }
      final updated = remove ? null : config.portable(credentialId: id);
      final changed =
          previous?.binding != config.binding || previous?.name != config.name || previous?.credentialId != id;
      if (remove ? previous != null : changed) {
        await ref
            .read(settingsServiceProvider)
            .updateVault(
              shared.copyWith(
                externalVaults: [
                  for (final row in shared.externalVaults)
                    if (row.id != config.id) row,
                  ?updated,
                ],
              ),
            );
      }
      created = null;
      committed = true;
      _check(config);
      if (updateLocal != null && nextLocal != null) {
        final currentLocal = settings.currentLocal;
        await settings.updateLocal(identical(currentLocal, beforeLocal) ? nextLocal : updateLocal(currentLocal));
      }
    } finally {
      if (created != null && current) await _clean(config, created);
      if (committed && previous != null && oldId != null && (remove || oldId != id) && current) {
        await _clean(previous, oldId);
      }
    }
  });
}
