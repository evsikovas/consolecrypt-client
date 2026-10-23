import 'dart:async';
import 'dart:convert';

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
/// the native ownership token — never the value or a hash of it.
abstract interface class ClipboardClearStore {
  Future<String?> read();
  Future<void> write(String? record);
}

/// What the native `clearIfOwned` did with a token.
enum ClipboardClearResult {
  /// Our clip was still there and is gone now.
  cleared,

  /// Someone (the user, another app or a newer copy) replaced it: untouched.
  foreign,

  /// Nothing to clear: the clipboard is empty or the token is unknown.
  gone,

  /// Android only: the clipboard cannot be read in the background and
  /// clearing without a check is off; the native side keeps the pending
  /// clear and finishes it when the app regains focus.
  deferred,
}

/// The single clipboard API of the app (S8/S9).
///
/// * [copySecret] — passwords, keys, revealed values: kept out of clipboard
///   history and cloud/other devices, marked as a secret, auto-cleared;
/// * [copyPrivate] — terminal/log selection, query results: kept out of
///   history and other devices, not cleared;
/// * [copyPlain] — paths, ids, public keys, commands.
///
/// Secrets and private text go through the native `consolecrypt/clipboard`
/// channel on every platform; with no native handler the copy fails (it is
/// never downgraded to an ordinary clipboard write). Dart never reads the
/// clipboard: clearing is the native `clearIfOwned` by the ownership token
/// the copy returned (clipboard change count / sequence number / clip id).
/// With a [store], the pending clear is recorded so [clearExpired] can
/// finish it after a restart.
class SecureClipboard {
  SecureClipboard({Duration clearAfter = const Duration(seconds: 30), this.store, DateTime Function()? now})
    : _clearAfter = _bounded(clearAfter),
      _now = now ?? DateTime.now;

  /// Where the pending clear is recorded (none: in-app timer only).
  final ClipboardClearStore? store;
  final DateTime Function() _now;

  static const channel = MethodChannel('consolecrypt/clipboard');
  static Duration _bounded(Duration duration) => Duration(milliseconds: duration.inMilliseconds.clamp(1000, 86400000));

  /// Delay before a copied secret is wiped from the clipboard.
  Duration get clearAfter => _clearAfter;
  Duration _clearAfter;

  Timer? _timer;
  String? _token;
  int _retries = 0;
  Future<void> _pending = Future.value();
  bool _disposed = false;

  /// A setting change applies to future copies. It cannot extend or discard
  /// the deadline already promised for a secret in the clipboard.
  void updateClearAfter(Duration duration) => _clearAfter = _bounded(duration);

  Future<T> _enqueue<T>(Future<T> Function() action) {
    final next = _pending.then((_) => action());
    _pending = next.then<void>((_) {}, onError: (Object _, StackTrace _) {});
    return next;
  }

  /// Drops the in-app pending clear after the clipboard got new content.
  Future<void> _forgetPending() async {
    _timer?.cancel();
    _timer = null;
    _token = null;
    _retries = 0;
    await _record(null);
  }

  /// Copies non-secret text (public keys, fingerprints, commands).
  Future<void> copyPlain(String value) => _enqueue(() async {
    if (_disposed) throw StateError('Clipboard owner disposed');
    // A pending native clear (Android alarm) must not wipe this public copy.
    // Sent before the write (platform messages keep their order); not awaited:
    // without a native side (an unsupported platform) nothing is scheduled.
    channel.invokeMethod<void>('forget').ignore();
    await Clipboard.setData(ClipboardData(text: value));
    await _forgetPending();
  });

  /// Copies text that should stay on this device and out of clipboard
  /// history (terminal and log selection, query results). Not auto-cleared.
  /// Fails if the native handler is unavailable.
  Future<void> copyPrivate(String value) => _enqueue(() async {
    if (_disposed) throw StateError('Clipboard owner disposed');
    await channel.invokeMethod<void>('copyPrivate', {'text': value});
    await _forgetPending();
  });

  Future<void> _record(String? record) async {
    try {
      await store?.write(record);
    } catch (_) {
      // Best effort: the in-app timer still clears while the app runs.
    }
  }

  /// Finishes a clear recorded before a restart: once its deadline has
  /// passed, the native side clears the clipboard only if it still holds
  /// our clip — foreign content is never touched. A deadline still ahead is
  /// re-armed as a timer. Records of older versions (a value hash, no token)
  /// cannot be checked without reading the clipboard and are dropped.
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
    final (deadline, token) = _parseRecord(raw);
    if (deadline == null || token == null) {
      await _record(null);
      return;
    }
    final left = deadline.difference(_now());
    if (left > Duration.zero) {
      if (_token == null) {
        _token = token;
        _arm(token, left);
      }
      return;
    }
    _token = token;
    await _clear(token);
  });

  static (DateTime?, String?) _parseRecord(String raw) {
    try {
      final json = jsonDecode(raw) as Map<String, dynamic>;
      final token = json['token'];
      if (token is! String || token.isEmpty || token.length > 128) return (null, null);
      return (DateTime.fromMillisecondsSinceEpoch(json['deadline_ms'] as int), token);
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
  /// revocation, the clear by token removes only the clip this call wrote.
  /// [limit] shortens the deadline (team clipboard policy). [autoGenerated]
  /// marks content the app put there without a user action (RDP clipboard).
  Future<void> copySecret(String value, {bool Function()? isCurrent, Duration? limit, bool autoGenerated = false}) =>
      _enqueue(() async {
        // A revoked operation queued behind another OS write must never start.
        if (!_operationCurrent(isCurrent)) return;
        if (_disposed) throw StateError('Clipboard owner disposed');
        final deadline = effectiveClearAfter(limit);
        // Fail closed: an ordinary clipboard write would lose the history,
        // cloud and expiry protections.
        final token = await channel.invokeMethod<String>('copySecret', {
          'text': value,
          'clearAfterMilliseconds': deadline.inMilliseconds,
          'autoGenerated': autoGenerated,
        });
        if (token == null || token.isEmpty) {
          throw PlatformException(code: 'clipboard', message: 'No clipboard ownership token');
        }
        _timer?.cancel();
        _token = token;
        _retries = 0;
        if (_disposed || !_operationCurrent(isCurrent)) {
          await _clear(token);
        } else {
          await _record(jsonEncode({'deadline_ms': _now().add(deadline).millisecondsSinceEpoch, 'token': token}));
          _arm(token, deadline);
        }
      });

  void _arm(String token, Duration after) {
    _timer?.cancel();
    _timer = Timer(after, () => _enqueue(() => _clear(token)).ignore());
  }

  /// Clears the pending secret now if the clipboard still holds it (a
  /// dialog that revealed it closes or the app goes to the background).
  /// `null`: nothing pending, or the native clear failed (kept for retry).
  Future<ClipboardClearResult?> clearNow() => _enqueue(() async {
    final token = _token;
    return token == null ? null : _clear(token);
  });

  Future<ClipboardClearResult?> _clear(String token) async {
    if (_token != token) return null;
    final String? result;
    try {
      result = await channel.invokeMethod<String>('clearIfOwned', {'token': token});
    } catch (_) {
      // The clipboard may be held by another app for a moment: retry a few
      // times, then leave the record for the next start or resume.
      if (!_disposed && _retries++ < 3) _arm(token, const Duration(seconds: 10));
      return null;
    }
    if (_token != token) return null;
    await _forgetPending();
    return ClipboardClearResult.values.asNameMap()[result] ?? ClipboardClearResult.gone;
  }

  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _timer?.cancel();
    _timer = null;
    // Best effort on graceful disposal; a force-quit cannot run Dart code
    // (the native side clears on window close/exit where it can).
    _enqueue(() async {
      final token = _token;
      if (token != null) await _clear(token);
    }).ignore();
  }
}
