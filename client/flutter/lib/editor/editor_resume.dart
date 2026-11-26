import 'dart:convert';
import 'dart:math';
import 'dart:typed_data';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:cryptography/cryptography.dart';
import 'package:cryptography/dart.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Only ciphertext survives a lock. Closing the app or leaving the profile drops it.
final editorResumeProvider = Provider<EditorResume>((ref) {
  final owner = ref.watch(activeProfileProvider.select((p) => p?.id));
  final cache = EditorResume(owner);
  ref.onDispose(cache.clear);
  ref.listen(appStageProvider, (_, stage) {
    if (stage != AppStage.locked && stage != AppStage.unlocked) cache.clear();
  });
  return cache;
});

class EditorResume {
  EditorResume(this.owner);
  final ProfileId? owner;
  final nonce = Uint8List.fromList(List.generate(32, (_) => _random.nextInt(256)));
  static final _random = Random.secure();
  final _cipher = DartAesGcm.with256bits();
  SecretBox? _sealed;

  void clear() => _sealed = null;

  void save(SecretKeyData key, Map<String, Object?> state) {
    final bytes = utf8.encode(jsonEncode(state));
    try {
      _sealed = _cipher.encryptSync(bytes, secretKeyData: key);
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }

  Map<String, dynamic>? open(SecretKeyData key) {
    final box = _sealed;
    if (box == null) return null;
    final bytes = _cipher.decryptSync(box, secretKeyData: key);
    try {
      return jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>;
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }
}
