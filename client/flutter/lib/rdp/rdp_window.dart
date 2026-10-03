import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

final rdpWindowProvider = Provider<RdpWindow>((_) => RdpWindow());

/// Native runners retain the previous window placement/mode until leave().
/// A failed or abandoned entry is always followed by leave by the caller.
class RdpWindow {
  static const channel = MethodChannel('consolecrypt/window');
  Object? _owner;

  /// Shared by every route/profile. A late entry may still need restoration;
  /// don't allow another screen to borrow the same native window until then.
  RdpWindowLease? acquire() {
    if (_owner != null) return null;
    final token = Object();
    _owner = token;
    return RdpWindowLease._(this, token);
  }

  bool get desktop =>
      !kIsWeb &&
      switch (defaultTargetPlatform) {
        TargetPlatform.macOS || TargetPlatform.windows || TargetPlatform.linux => true,
        _ => false,
      };

  Future<void> enter() async {
    if (!desktop) {
      await SystemChrome.setEnabledSystemUIMode(SystemUiMode.immersiveSticky);
      return;
    }
    if (await channel.invokeMethod<bool>('beginRdpFullscreen') != true) {
      throw PlatformException(code: 'fullscreen_unavailable');
    }
    // AppKit and window managers acknowledge fullscreen asynchronously. Do
    // not report success merely because a request was accepted by the runner.
    for (var attempt = 0; attempt < 50; attempt++) {
      if (await isFullscreen()) return;
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw PlatformException(code: 'fullscreen_timeout');
  }

  Future<bool> isFullscreen() async => !desktop || await channel.invokeMethod<bool>('isFullscreen') == true;
  Future<void> leave() async {
    if (desktop) {
      await channel.invokeMethod<void>('endRdpFullscreen');
      for (var attempt = 0; attempt < 150; attempt++) {
        if (await channel.invokeMethod<bool>('isRdpFullscreenRestored') == true) return;
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      throw PlatformException(code: 'fullscreen_restore_timeout');
    } else {
      await SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
    }
  }

  Future<void> minimize() async {
    await channel.invokeMethod<void>('minimize');
    // AppKit can first leave a preexisting fullscreen Space. An accepted
    // request is not completion: retain the lease through that transition.
    for (var attempt = 0; attempt < 150; attempt++) {
      if (await channel.invokeMethod<bool>('isRdpMinimized') == true) return;
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw PlatformException(code: 'fullscreen_minimize_timeout');
  }
}

final class RdpWindowLease {
  RdpWindowLease._(this.window, this._token);
  final RdpWindow window;
  final Object _token;
  bool _restoring = false;
  Future<void> enter() => window.enter();
  Future<void> restore({bool minimize = false}) async {
    if (_restoring || !identical(window._owner, _token)) return;
    _restoring = true;
    var restored = false;
    try {
      await window.leave();
      if (minimize) await window.minimize();
      restored = true;
    } finally {
      // A failed/late native restore or minimize must never affect a new owner.
      // Keep the lease on failure; ordinary OS window controls remain usable.
      if (restored && identical(window._owner, _token)) window._owner = null;
    }
  }
}
