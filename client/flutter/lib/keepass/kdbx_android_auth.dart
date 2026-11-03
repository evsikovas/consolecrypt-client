import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/local_auth_channel.dart';
import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/keepass/kdbx_client.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

typedef KdbxKeyCall = Future<Uint8List> Function(String action, Uint8List secret);
final kdbxAndroidAuthProvider = Provider<KdbxAndroidAuth>((ref) => KdbxAndroidAuth());

/// Android owns the per-use CryptoObject prompt; Rust seals/checks its material.
/// Injectable channel operations let tests assert wiping without authenticators.
class KdbxAndroidAuth {
  KdbxAndroidAuth({
    Future<LocalAuthKey> Function(String, String)? create,
    Future<Uint8List> Function(Uint8List, String)? release,
    Future<void> Function(String)? delete,
  }) : _create = create ?? LocalAuthChannel.instance.createKey,
       _release = release ?? LocalAuthChannel.instance.releaseKey,
       _delete = delete ?? LocalAuthChannel.instance.deleteKey;
  final Future<LocalAuthKey> Function(String, String) _create;
  final Future<Uint8List> Function(Uint8List, String) _release;
  final Future<void> Function(String) _delete;

  static String? alias(Uint8List handle) {
    if (handle.length < 2 || handle[0] != 1 || handle.length < 2 + handle[1]) return null;
    final name = utf8.decode(handle.sublist(2, 2 + handle[1]), allowMalformed: true);
    return RegExp(r'^cc-device-unlock-kdbx-[a-f0-9-]{36}$').hasMatch(name) ? name : null;
  }

  Future<void> _cleanup(Uint8List handle) async {
    final name = alias(handle);
    if (name != null) await _deleteSafe(name);
  }

  Future<void> _deleteSafe(String name) async {
    try {
      await _delete(name);
    } catch (_) {
      /* Best effort, like vault device keys. */
    }
  }

  Future<Uint8List> unlock(KdbxKeyCall call, String reason) async {
    final handle = await call('external_handle', Uint8List(0));
    final material = await _release(handle, reason);
    try {
      return await call('release_external', material);
    } finally {
      material.fillRange(0, material.length, 0);
    }
  }

  Future<void> protect(
    KdbxKeyCall call,
    Uint8List composite,
    String reason, {
    required bool enabled,
    bool forget = false,
  }) async {
    final old = enabled ? await call('external_handle', Uint8List(0)) : Uint8List(0);
    if (forget) {
      await call('forget', Uint8List(0));
      await _cleanup(old);
      return;
    }
    final name = 'cc-device-unlock-kdbx-${generateUuidV4()}';
    final key = await _create(name, reason);
    Uint8List? payload;
    bool committed = false;
    try {
      if (composite.length != 32 || key.material.length != 32 || key.handle.isEmpty || key.handle.length > 4096) {
        throw const KdbxFailure('key');
      }
      payload = Uint8List(64 + key.handle.length)
        ..setRange(0, 32, composite)
        ..setRange(32, 64, key.material)
        ..setRange(64, 64 + key.handle.length, key.handle);
      await call('enable_external', payload);
      committed = true;
    } finally {
      key.material.fillRange(0, key.material.length, 0);
      payload?.fillRange(0, payload.length, 0);
      if (!committed) await _deleteSafe(name);
    }
    await _cleanup(old);
  }
}
