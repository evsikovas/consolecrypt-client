import 'dart:async';
import 'dart:convert';

import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Clipboard access with auto-clear for secrets.
///
/// After [copySecret], the clipboard is cleared once [clearAfter] elapses —
/// but only if it still holds the value we put there (the user may have
/// copied something else meanwhile). Values are never logged.
class SecureClipboard {
  SecureClipboard({Duration clearAfter = const Duration(seconds: 30)}) : _clearAfter = _bounded(clearAfter);

  static const _native = MethodChannel('consolecrypt/clipboard');
  static Duration _bounded(Duration duration) => Duration(milliseconds: duration.inMilliseconds.clamp(1000, 86400000));

  /// Delay before a copied secret is wiped from the clipboard.
  Duration get clearAfter => _clearAfter;
  Duration _clearAfter;

  Timer? _timer;
  Digest? _marker;
  Future<void> _pending = Future.value();
  bool _disposed = false;

  /// A setting change applies to future copies. It cannot extend or discard
  /// the deadline already promised for a secret in the clipboard.
  void updateClearAfter(Duration duration) => _clearAfter = _bounded(duration);

  Future<void> _enqueue(Future<void> Function() action) {
    final next = _pending.then((_) => action());
    _pending = next.then<void>((_) {}, onError: (Object _, StackTrace _) {});
    return next;
  }

  /// Copies non-secret text (public keys, fingerprints, commands).
  Future<void> copyPlain(String value) => _enqueue(() async {
    if (_disposed) throw StateError('Clipboard owner disposed');
    await Clipboard.setData(ClipboardData(text: value));
    _timer?.cancel();
    _marker = null;
  });

  bool _operationCurrent(bool Function()? isCurrent) {
    try {
      return isCurrent?.call() != false;
    } catch (_) {
      return false;
    }
  }

  /// Copies a secret and schedules clearing it. [isCurrent] optionally guards
  /// delayed scoped operations. If an already-started OS write finishes after
  /// revocation, compare-and-clear removes only the value this call wrote.
  Future<void> copySecret(String value, {bool Function()? isCurrent}) => _enqueue(() async {
    // A revoked operation queued behind another OS write must never start.
    if (!_operationCurrent(isCurrent)) return;
    if (_disposed) throw StateError('Clipboard owner disposed');
    final deadline = clearAfter;
    if (!kIsWeb && (defaultTargetPlatform == TargetPlatform.android || defaultTargetPlatform == TargetPlatform.iOS)) {
      // Fail closed if the native handler is unavailable: ordinary mobile
      // clipboard writes would lose sensitive/device-local/expiry options.
      await _native.invokeMethod<void>('copySecret', {
        'text': value,
        'clearAfterMilliseconds': deadline.inMilliseconds,
      });
    } else {
      await Clipboard.setData(ClipboardData(text: value));
    }
    _timer?.cancel();
    // Retain a digest, never the plaintext or a short String.hashCode.
    final marker = sha256.convert(utf8.encode(value));
    _marker = marker;
    if (_disposed || !_operationCurrent(isCurrent)) {
      await _clearIfUnchanged(marker);
    } else {
      _timer = Timer(deadline, () => _enqueue(() => _clearIfUnchanged(marker)).ignore());
    }
  });

  Future<void> _clearIfUnchanged(Digest marker) async {
    if (_marker != marker) return;
    final current = await Clipboard.getData(Clipboard.kTextPlain);
    if (current?.text != null && sha256.convert(utf8.encode(current!.text!)) == marker) {
      await Clipboard.setData(const ClipboardData(text: ''));
    }
    _marker = null;
  }

  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _timer?.cancel();
    _timer = null;
    // Best effort on graceful disposal; a force-quit cannot run Dart code.
    _enqueue(() async {
      final marker = _marker;
      if (marker != null) await _clearIfUnchanged(marker);
    }).ignore();
  }
}
