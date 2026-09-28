import 'dart:async';

import 'package:consolecrypt/core/glass/glass_mode_resolver.dart';
import 'package:consolecrypt/core/models/glass_settings.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Dart side of the `consolecrypt/accessibility` MethodChannel implemented
/// in `macos/Runner/MainFlutterWindow.swift` and
/// `windows/runner/glass_window_bridge.cpp`.
///
/// * Dart → native `getSignals` returns a map of booleans
///   (see [OsAccessibilitySignals.toMap]).
/// * native → Dart `signalsChanged` carries the same map whenever the OS
///   setting changes (macOS `accessibilityDisplayOptionsDidChange` / power
///   state, Windows `WM_SETTINGCHANGE` / `WM_THEMECHANGED` / power).
///
/// Platforms without an implementation (tests, Linux) report
/// [OsAccessibilitySignals.none]; `MediaQuery` flags still apply.
class AccessibilityBridge {
  AccessibilityBridge({MethodChannel? channel}) : _channel = channel ?? const MethodChannel(channelName);

  static const channelName = 'consolecrypt/accessibility';

  final MethodChannel _channel;
  final StreamController<OsAccessibilitySignals> _changes = StreamController.broadcast();
  bool _handlerInstalled = false;

  /// Current OS state, or [OsAccessibilitySignals.none] if unavailable.
  Future<OsAccessibilitySignals> read() async {
    try {
      final map = await _channel.invokeMapMethod<Object?, Object?>('getSignals');
      return OsAccessibilitySignals.fromMap(map);
    } on MissingPluginException {
      return OsAccessibilitySignals.none;
    } on PlatformException {
      return OsAccessibilitySignals.none;
    }
  }

  /// Changes pushed by the runner.
  Stream<OsAccessibilitySignals> get changes {
    if (!_handlerInstalled) {
      _handlerInstalled = true;
      _channel.setMethodCallHandler((call) async {
        if (call.method == 'signalsChanged' && !_changes.isClosed) {
          _changes.add(OsAccessibilitySignals.fromMap(call.arguments as Map<Object?, Object?>?));
        }
        return null;
      });
    }
    return _changes.stream;
  }

  Future<void> dispose() async {
    if (_handlerInstalled) _channel.setMethodCallHandler(null);
    await _changes.close();
  }
}

/// The bridge instance; override in tests to inject a mocked channel.
final accessibilityBridgeProvider = Provider<AccessibilityBridge>((ref) {
  final bridge = AccessibilityBridge();
  ref.onDispose(() => unawaited(bridge.dispose()));
  return bridge;
});

/// Live OS accessibility / display signals (initially [OsAccessibilitySignals.none]
/// until the runner answers).
final osAccessibilityProvider = NotifierProvider<OsAccessibilityNotifier, OsAccessibilitySignals>(
  OsAccessibilityNotifier.new,
);

class OsAccessibilityNotifier extends Notifier<OsAccessibilitySignals> {
  StreamSubscription<OsAccessibilitySignals>? _sub;

  @override
  OsAccessibilitySignals build() {
    final bridge = ref.watch(accessibilityBridgeProvider);
    _sub = bridge.changes.listen((signals) => state = signals);
    ref.onDispose(() => unawaited(_sub?.cancel()));
    unawaited(
      bridge.read().then((signals) {
        if (ref.mounted) state = signals;
      }),
    );
    return OsAccessibilitySignals.none;
  }
}

/// The user's in-app Glass setting (device-local, persisted by the
/// SettingsService).
final glassModeProvider = Provider<GlassMode>(
  (ref) => ref.watch(localSettingsProvider.select((s) => s.value?.glassMode ?? GlassMode.standard)),
);

/// Whether the OS asks for increased contrast (native signal only; the
/// Windows high-contrast theme also arrives via `MediaQuery.highContrast`).
final osIncreaseContrastProvider = Provider<bool>(
  (ref) => ref.watch(osAccessibilityProvider.select((s) => s.increaseContrast)),
);
