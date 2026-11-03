// File copies/deletes are asynchronous to keep the picker and UI responsive.
// ignore_for_file: avoid_slow_async_io
import 'dart:io';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/keepass/kdbx_client.dart';
import 'package:consolecrypt/keepass/password_manager.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path/path.dart' as p;

final kdbxDocumentsProvider = Provider<KdbxDocuments>((ref) => KdbxDocuments());

/// Mobile pickers may return disposable copies. Keep the editable encrypted
/// database in app-private storage, and export copies through the OS picker.
class KdbxDocuments {
  KdbxDocuments({bool? mobile, Future<String> Function()? directory})
    : mobile = mobile ?? AppPlatform.isMobile,
      _directory = directory ?? _nativeDirectory;
  final bool mobile;
  final Future<String> Function() _directory;
  final _pending = <String>{};
  static const limit = 128 * 1024 * 1024;

  static Future<String> _nativeDirectory() async {
    final path = await MethodChannel(Platform.isIOS ? 'consolecrypt/ios' : 'consolecrypt/android')
        .invokeMethod<String>('dataDirectory');
    if (path == null || path.isEmpty) throw const KdbxFailure('io');
    return path;
  }

  Future<KdbxSource?> choose(FileDialogService dialogs, {bool create = false}) async {
    if (!mobile) {
      final path = create
          ? await dialogs.chooseSaveFile(suggestedName: 'Passwords.kdbx', extensions: const ['kdbx'])
          : await dialogs.chooseOpenFile(extensions: const ['kdbx']);
      return path == null ? null : KdbxSource.local(path);
    }
    final input = create ? null : await dialogs.chooseOpenFile(extensions: const ['kdbx']);
    if (!create && input == null) return null;
    if (input != null && await File(input).length() > limit) throw const KdbxFailure('limits');
    final folder = Directory(p.join(await _directory(), 'password-vaults', generateUuidV4()));
    await folder.create(recursive: true);
    final name = input == null ? 'Passwords.kdbx' : p.basename(input);
    final source = KdbxSource.local(p.join(folder.path, name));
    _pending.add(source.path);
    try {
      if (input != null) {
        await File(input).copy(source.path);
        if (await File(source.path).length() > limit) throw const KdbxFailure('limits');
      }
      return source;
    } catch (_) {
      await discard(source);
      rethrow;
    }
  }

  void keep(KdbxSource source) => _pending.remove(source.path);

  Future<void> discard(KdbxSource source) async {
    // Only paths created by this instance may be removed. Never the original.
    if (!_pending.remove(source.path)) return;
    try {
      final file = File(source.path);
      if (await file.exists()) await file.delete();
      await file.parent.delete(); // Non-recursive: an unexpected sibling stays.
    } on FileSystemException {
      /* Private orphan can be retried after restart. */
    }
  }
}
