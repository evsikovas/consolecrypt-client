import 'dart:async';
import 'dart:convert';

import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Auto-clear choices in Settings, in seconds: 30 s … 1 day.
const clipboardClearChoices = [30, 60, 120, 300, 900, 3600, 28800, 86400];

/// A stored auto-clear duration as one of [clipboardClearChoices]: the
/// nearest choice not shorter than [seconds] (10 and 15 from older versions
/// become 30), the longest one above it.
int clipboardClearSeconds(int seconds) =>
    clipboardClearChoices.firstWhere((choice) => choice >= seconds, orElse: () => clipboardClearChoices.last);

/// Device-local slot for the pending clear of a copied secret, so a long
/// auto-clear deadline survives an app restart. Holds only the deadline and
/// a SHA-256 of the copied value — never the value.
abstract interface class ClipboardClearStore {
  Future<String?> read();
  Future<void> write(String? record);
}

/// Clipboard access with auto-clear for secrets.
///
/// After [copySecret], the clipboard is cleared once [clearAfter] elapses —
/// but only if it still holds the value we put there (the user may have
/// copied something else meanwhile). Values are never logged. With a
/// [store], the pending clear is also recorded so [clearExpired] can finish
/// it after a restart (on start and resume); the in-app timer alone does not
/// outlive the process.
class SecureClipboard {
  SecureClipboard({Duration clearAfter = const Duration(seconds: 30), this.store, DateTime Function()? now})
    : _clearAfter = _bounded(clearAfter),
      _now = now ?? DateTime.now;

  /// Where the pending clear is recorded (none: in-app timer only).
  final ClipboardClearStore? store;
  final DateTime Function() _now;

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
    await _record(null);
  });

  Future<void> _record(String? record) async {
    try {
      await store?.write(record);
    } catch (_) {
      // Best effort: the in-app timer still clears while the app runs.
    }
  }

  /// Finishes a clear recorded before a restart: once its deadline has
  /// passed, the clipboard is emptied only if it still holds the value we
  /// copied (same SHA-256) — foreign content is never touched. A deadline
  /// still ahead is re-armed as a timer.
  Future<void> clearExpired() => _enqueue(() async {
    final store = this.store;
    if (store == null || _disposed) return;
    String? raw;
    try {
      raw = await store.read();
    } catch (_) {
      return;
    }
    if (raw == null) return;
    final (deadline, marker) = _parseRecord(raw);
    if (deadline == null || marker == null) {
      await _record(null);
      return;
    }
    final left = deadline.difference(_now());
    if (left > Duration.zero) {
      if (_marker == null) {
        _marker = marker;
        _timer?.cancel();
        _timer = Timer(left, () => _enqueue(() => _clearIfUnchanged(marker)).ignore());
      }
      return;
    }
    _marker = marker;
    await _clearIfUnchanged(marker);
  });

  static (DateTime?, Digest?) _parseRecord(String raw) {
    try {
      final json = jsonDecode(raw) as Map<String, dynamic>;
      final hex = json['sha256'] as String;
      if (!RegExp(r'^[0-9a-f]{64}$').hasMatch(hex)) return (null, null);
      final bytes = [for (var i = 0; i < 64; i += 2) int.parse(hex.substring(i, i + 2), radix: 16)];
      return (DateTime.fromMillisecondsSinceEpoch(json['deadline_ms'] as int), Digest(bytes));
    } catch (_) {
      return (null, null);
    }
  }

  bool _operationCurrent(bool Function()? isCurrent) {
    try {
      return isCurrent?.call() != false;
    } catch (_) {
      return false;
    }
  }

  /// The deadline for a copy: [limit] (e.g. a team policy) may only shorten
  /// the app setting, never extend it.
  Duration effectiveClearAfter(Duration? limit) =>
      limit == null || _bounded(limit) >= clearAfter ? clearAfter : _bounded(limit);

  /// Copies a secret and schedules clearing it. [isCurrent] optionally guards
  /// delayed scoped operations. If an already-started OS write finishes after
  /// revocation, compare-and-clear removes only the value this call wrote.
  /// [limit] shortens the deadline (team clipboard policy).
  Future<void> copySecret(String value, {bool Function()? isCurrent, Duration? limit}) => _enqueue(() async {
    // A revoked operation queued behind another OS write must never start.
    if (!_operationCurrent(isCurrent)) return;
    if (_disposed) throw StateError('Clipboard owner disposed');
    final deadline = effectiveClearAfter(limit);
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
      await _record(
        jsonEncode({'deadline_ms': _now().add(deadline).millisecondsSinceEpoch, 'sha256': marker.toString()}),
      );
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
    await _record(null);
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
