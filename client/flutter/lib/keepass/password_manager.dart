import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/core/bridge/mapping.dart' show guard;
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/keepass/kdbx_android_auth.dart';
import 'package:consolecrypt/keepass/kdbx_client.dart';
import 'package:consolecrypt/keepass/webdav.dart';
import 'package:consolecrypt/src/rust/api/password_vault.dart' as native;
import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path/path.dart' as p;

final passwordManagerProvider = Provider<PasswordManager>((ref) {
  final profile = ref.watch(activeProfileProvider.select((p) => p?.id));
  final vault = ref.watch(vaultStatusProvider.select((v) => (v.value?.vaultId, v.value?.isUnlocked == true)));
  final manager = PasswordManager(ref, profile?.value ?? '', vault.$1?.value, vault.$2);
  ref.onDispose(manager.dispose);
  return manager;
});
final passwordManagerChangesProvider = StreamProvider<int>((ref) => ref.watch(passwordManagerProvider).changes);

class KdbxSource {
  KdbxSource.local(String path) : path = p.normalize(p.absolute(path)), connection = null;
  KdbxSource.dav(PasswordVaultConnection config) : path = config.url, connection = config;
  final String path;
  final PasswordVaultConnection? connection;
  bool get remote => connection != null;
  String get name => connection?.name ?? p.basename(path);
  String get id => sha256
      .convert(
        utf8.encode(remote ? 'dav:${connection!.id}:$path' : 'local:${Platform.isWindows ? path.toLowerCase() : path}'),
      )
      .toString();
}

class PasswordVaultSession {
  PasswordVaultSession(this.source, this.client, this.data, this.etag, [this.remoteFingerprint]);
  final KdbxSource source;
  final KdbxClient client;
  KdbxData data;
  String? etag;
  String? remoteFingerprint;
  int _pending = 0;
  Future<void> _tail = Future.value();
  bool get busy => _pending > 0;
  Object? saveError;
  final validity = ValueNotifier(true);
  List<KdbxData> rows(String key) => [for (final row in data[key] as List<Object?>? ?? []) row! as KdbxData];
}

/// Owns workers for the unlocked ConsoleCrypt profile, independently of UI.
/// Closing a page leaves databases available to Ctrl+K; locking kills them.
class PasswordManager {
  PasswordManager(this.ref, this.profile, String? vault, this.unlocked) {
    // A direct subscription also closes workers if UI providers are paused.
    _status = ref.read(vaultServiceProvider).watchStatus().listen((status) {
      if (!status.isUnlocked || status.vaultId?.value != vault) dispose();
    });
  }
  StreamSubscription<VaultStatus>? _status;
  final Ref ref;
  final String profile;
  final bool unlocked;
  final sessions = <String, PasswordVaultSession>{};
  final _changes = StreamController<int>.broadcast();
  final _opening = <String, KdbxClient>{};
  final _openingSources = <String>{};
  final _network = <KdbxWebDav, String>{};
  final _cancelledOpening = <String>{};
  bool get opening => _openingSources.isNotEmpty;
  void cancelOpening() {
    _cancelledOpening.addAll(_openingSources);
    for (final entry in _network.entries) {
      if (_openingSources.contains(entry.value)) entry.key.close();
    }
    for (final client in _opening.values) {
      client.close();
    }
  }

  var _revision = 0;
  var _disposed = false;
  final validity = ValueNotifier(true);
  String? selected;
  Stream<int> get changes async* {
    yield _revision;
    yield* _changes.stream;
  }

  bool get valid => !_disposed && unlocked;
  bool current(PasswordVaultSession session) => valid && identical(sessions[session.source.id], session);
  void notify() {
    if (!_disposed) _changes.add(++_revision);
  }

  void check() {
    if (!valid) throw const KdbxFailure('closed');
  }

  List<KdbxSource> get sources {
    if (!valid) return [];
    final all = <String, KdbxSource>{};
    for (final f in ref.read(settingsServiceProvider).currentLocal.passwordVaultFiles) {
      if (f['profile'] == profile) {
        final s = KdbxSource.local(f['path']!);
        all[s.id] = s;
      }
    }
    for (final c in ref.read(vaultSettingsProvider).value?.passwordVaults ?? <PasswordVaultConnection>[]) {
      final s = KdbxSource.dav(c);
      all[s.id] = s;
    }
    for (final s in sessions.values) {
      all[s.source.id] = s.source;
    }
    return all.values.toList();
  }

  Future<void> remember(KdbxSource source) async {
    check();
    if (source.remote) return;
    final settings = ref.read(settingsServiceProvider);
    final local = settings.currentLocal;
    final files = local.passwordVaultFiles
        .where((f) => f['profile'] != profile || KdbxSource.local(f['path']!).id != source.id)
        .toList();
    files.add({'profile': profile, 'path': source.path});
    await settings.updateLocal(local.copyWith(passwordVaultFiles: files));
    check();
    notify();
  }

  /// Forget the catalog entry/connection, never the user's KDBX file. Commit
  /// settings before closing a worker, so a failed settings write loses nothing.
  Future<void> forgetSource(KdbxSource source) async {
    check();
    final session = sessions[source.id];
    if (session?.busy == true) throw const KdbxFailure('busy');
    Future<void> commit() async {
      final settings = ref.read(settingsServiceProvider);
      if (source.remote) {
        final vault = ref.read(vaultSettingsProvider).value;
        if (vault == null) throw const KdbxFailure('closed');
        await settings.updateVault(
          vault.copyWith(passwordVaults: vault.passwordVaults.where((c) => KdbxSource.dav(c).id != source.id).toList()),
        );
      } else {
        final local = settings.currentLocal;
        await settings.updateLocal(
          local.copyWith(
            passwordVaultFiles: local.passwordVaultFiles
                .where((f) => f['profile'] != profile || KdbxSource.local(f['path']!).id != source.id)
                .toList(),
          ),
        );
      }
      check();
      if (session != null && !current(session)) throw const KdbxFailure('closed');
      lock(source);
    }

    if (session == null) {
      await commit();
    } else {
      await _enqueue(session, commit);
    }
  }

  Future<KdbxWebDav> _dav(KdbxSource source) async {
    final c = source.connection!;
    SecretText? secret;
    try {
      if (c.credentialId != null) {
        secret = await ref.read(inventoryServiceProvider).revealCredentialSecret(ObjectId(c.credentialId!));
      }
      check();
      return KdbxWebDav(Uri.parse(c.url), username: c.username, password: secret?.expose() ?? '');
    } finally {
      secret?.wipe();
    }
  }

  Future<T> _remote<T>(KdbxSource source, Future<T> Function(KdbxWebDav) action) async {
    final dav = await _dav(source);
    _network[dav] = source.id;
    try {
      return await action(dav);
    } finally {
      _network.remove(dav);
      dav.close();
    }
  }

  Future<PasswordVaultSession> open(
    KdbxSource source, {
    SecretText? password,
    String? keyFile,
    bool create = false,
    String? method,
    String? pin,
    String authReason =
        'Unlock your KDBX password database', // l10n-ignore: non-UI fallback; dialogs pass the localized reason.
  }) async {
    check();
    if (source.remote && !KdbxWebDav.isFileUri(Uri.parse(source.path))) {
      password?.wipe();
      throw const KdbxFailure('dav_url');
    }
    final existing = sessions[source.id];
    if (existing != null) {
      password?.wipe();
      selected = source.id;
      notify();
      return existing;
    }
    if (!_openingSources.add(source.id)) {
      password?.wipe();
      throw const KdbxFailure('busy');
    }
    notify();
    final KdbxClient client;
    try {
      client = await KdbxClient.start();
    } catch (_) {
      _openingSources.remove(source.id);
      password?.wipe();
      final cancelled = _cancelledOpening.remove(source.id);
      notify();
      if (cancelled) throw const KdbxFailure('cancelled');
      rethrow;
    }
    if (!valid || _cancelledOpening.contains(source.id)) {
      client.close();
      _openingSources.remove(source.id);
      password?.wipe();
      final cancelled = _cancelledOpening.remove(source.id);
      notify();
      throw KdbxFailure(cancelled ? 'cancelled' : 'closed');
    }
    _opening[source.id] = client;
    Uint8List? hash;
    try {
      if (method != null) {
        final info = await protection(source, method);
        hash = method == 'hello' && info['external'] == true
            ? await ref.read(kdbxAndroidAuthProvider).unlock((action, material) {
                check();
                return guard(
                  () => native.passwordVaultKey(
                    action: action,
                    id: source.id,
                    method: method,
                    secret: material,
                    pin: null,
                  ),
                );
              }, authReason)
            : await guard(
                () => native.passwordVaultKey(
                  action: 'release',
                  id: source.id,
                  method: method,
                  secret: Uint8List(0),
                  pin: pin,
                ),
              );
      }
      check();
      final dav = source.remote ? await _remote(source, (dav) => dav.read()) : null;
      check();
      final data = create
          ? await client.open(source.path, password!, create: true, keyFile: keyFile)
          : await client.openSource(
              source.path,
              password: password,
              keyFile: keyFile,
              encrypted: dav?.bytes,
              compositeKey: hash,
            );
      check();
      if (_cancelledOpening.contains(source.id)) throw const KdbxFailure('cancelled');
      final session = PasswordVaultSession(source, client, data, dav?.etag, dav?.fingerprint);
      sessions[source.id] = session;
      selected = source.id;
      await remember(source);
      notify();
      return session;
    } catch (_) {
      client.close();
      sessions.remove(source.id);
      if (_cancelledOpening.contains(source.id)) throw const KdbxFailure('cancelled');
      rethrow;
    } finally {
      hash?.fillRange(0, hash.length, 0);
      password?.wipe();
      _opening.remove(source.id);
      _openingSources.remove(source.id);
      _cancelledOpening.remove(source.id);
      notify();
    }
  }

  Future<T> _enqueue<T>(PasswordVaultSession session, Future<T> Function() action) async {
    if (!current(session)) throw const KdbxFailure('closed');
    session._pending++;
    notify();
    final operation = session._tail.then((_) async {
      if (!current(session)) throw const KdbxFailure('closed');
      return action();
    });
    session._tail = operation.then<void>((_) {}, onError: (Object _, StackTrace _) {});
    try {
      return await operation;
    } finally {
      session._pending--;
      notify();
    }
  }

  Future<KdbxData> request(PasswordVaultSession session, String action, [KdbxData args = const {}]) =>
      _enqueue(session, () async {
        await _snapshotRequest(session, action, args);
        if (const {
          'put',
          'group',
          'delete',
          'move',
          'renameGroup',
          'deleteGroup',
          'purgeGroup',
          'purgeEntry',
          'emptyTrash',
        }.contains(action)) {
          await _saveNow(session);
        }
        return session.data;
      });

  Future<KdbxData> _snapshotRequest(PasswordVaultSession session, String action, [KdbxData args = const {}]) async {
    if (!current(session)) throw const KdbxFailure('closed');
    final data = (await session.client.request(action, args))! as KdbxData;
    if (!current(session)) {
      data.clear();
      throw const KdbxFailure('closed');
    }
    session.data = data;
    notify();
    return data;
  }

  Future<String> field(PasswordVaultSession session, String id, String key) async {
    if (!current(session)) throw const KdbxFailure('closed');
    final value = (await session.client.request('field', {'id': id, 'key': key}))! as String;
    if (!current(session)) throw const KdbxFailure('closed');
    return value;
  }

  Future<void> save(PasswordVaultSession session) => _enqueue(session, () => _saveNow(session));

  Future<void> _saveNow(PasswordVaultSession session) async {
    if (!current(session)) throw const KdbxFailure('closed');
    session.saveError = null;
    try {
      if (!session.source.remote) {
        await _snapshotRequest(session, 'save');
        return;
      }
      final bytes = (await session.client.request('export'))! as Uint8List;
      if (!current(session)) throw const KdbxFailure('closed');
      final result = await _remote(
        session.source,
        (dav) => dav.write(bytes, session.etag, fingerprint: session.remoteFingerprint),
      );
      if (!current(session)) throw const KdbxFailure('closed');
      session.etag = result.etag;
      session.remoteFingerprint = result.fingerprint;
      await _snapshotRequest(session, 'saved');
    } catch (error) {
      session.saveError = error;
      rethrow;
    } finally {
      notify();
    }
  }

  Future<Map<String, Object?>> protection(KdbxSource source, String method) async {
    check();
    final bytes = await guard(
      () => native.passwordVaultKey(action: 'info', id: source.id, method: method, secret: Uint8List(0), pin: null),
    );
    check();
    return (jsonDecode(utf8.decode(bytes)) as Map).cast<String, Object?>();
  }

  Future<void> protect(
    PasswordVaultSession session,
    String method, {
    String? pin,
    bool forget = false,
    String authReason =
        'Unlock your KDBX password database', // l10n-ignore: non-UI fallback; dialogs pass the localized reason.
  }) async {
    if (!current(session)) throw const KdbxFailure('closed');
    final hash = forget ? Uint8List(0) : (await session.client.request('compositeKey'))! as Uint8List;
    try {
      if (!current(session)) throw const KdbxFailure('closed');
      final info = await protection(session.source, method);
      if (method == 'hello' && info['external'] == true) {
        await ref
            .read(kdbxAndroidAuthProvider)
            .protect(
              (action, material) {
                if (!current(session)) throw const KdbxFailure('closed');
                return guard(
                  () => native.passwordVaultKey(
                    action: action,
                    id: session.source.id,
                    method: method,
                    secret: material,
                    pin: null,
                  ),
                );
              },
              hash,
              authReason,
              enabled: info['enabled'] == true,
              forget: forget,
            );
        check();
        notify();
        return;
      }
      await guard(
        () => native.passwordVaultKey(
          action: forget ? 'forget' : 'enable',
          id: session.source.id,
          method: method,
          secret: hash,
          pin: pin,
        ),
      );
      check();
      notify();
    } finally {
      hash.fillRange(0, hash.length, 0);
    }
  }

  void lock(KdbxSource source) {
    final session = sessions.remove(source.id);
    session?.validity.value = false;
    session?.client.close();
    session?.data.clear();
    if (selected == source.id) selected = sessions.keys.firstOrNull;
    notify();
  }

  void dispose() {
    if (_disposed) return;
    _disposed = true;
    unawaited(_status?.cancel());
    validity.value = false;
    for (final dav in _network.keys) {
      dav.close();
    }
    _network.clear();
    for (final session in sessions.values) {
      session.validity.value = false;
      session.client.close();
      session.data.clear();
    }
    for (final client in _opening.values) {
      client.close();
    }
    sessions.clear();
    _opening.clear();
    _openingSources.clear();
    unawaited(_changes.close());
  }
}
