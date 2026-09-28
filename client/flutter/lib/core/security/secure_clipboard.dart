import 'dart:async';

import 'package:flutter/services.dart';

/// Clipboard access with auto-clear for secrets.
///
/// After [copySecret], the clipboard is cleared once [clearAfter] elapses —
/// but only if it still holds the value we put there (the user may have
/// copied something else meanwhile). Values are never logged.
class SecureClipboard {
  SecureClipboard({this.clearAfter = const Duration(seconds: 30)});

  /// Delay before a copied secret is wiped from the clipboard.
  final Duration clearAfter;

  Timer? _timer;

  /// Copies non-secret text (public keys, fingerprints, commands).
  Future<void> copyPlain(String value) => Clipboard.setData(ClipboardData(text: value));

  /// Copies a secret and schedules clearing it.
  Future<void> copySecret(String value) async {
    await Clipboard.setData(ClipboardData(text: value));
    _timer?.cancel();
    final marker = value.hashCode;
    _timer = Timer(clearAfter, () => unawaited(_clearIfUnchanged(marker)));
  }

  Future<void> _clearIfUnchanged(int marker) async {
    final current = await Clipboard.getData(Clipboard.kTextPlain);
    if (current?.text != null && current!.text.hashCode == marker) {
      await Clipboard.setData(const ClipboardData(text: ''));
    }
  }

  void dispose() {
    _timer?.cancel();
    _timer = null;
  }
}
