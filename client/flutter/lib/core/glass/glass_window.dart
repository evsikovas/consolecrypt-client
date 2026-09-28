import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:macos_window_utils/macos_window_utils.dart';

/// Native window chrome (LIQUID_GLASS_SPEC §4.1, §6.7).
///
/// * macOS (`macos_window_utils`, MIT): transparent, full-size-content,
///   unified title bar (52-pt band) with the traffic lights over the floating
///   sidebar. The window itself stays opaque: glass samples the in-app
///   ambient backdrop, and AppKit keeps honouring Reduce Transparency for
///   its own window material.
/// * Windows (runner, `consolecrypt/window` channel): caption and caption
///   text colours follow the theme (Windows 11 22000+), rounded corners are
///   requested by the runner.
///
/// Every call is best-effort: without the native side (tests, other
/// platforms) it returns `false` and nothing changes.
abstract final class GlassWindowChrome {
  static const windowChannel = MethodChannel('consolecrypt/window');

  static Future<bool>? _macInit;
  static bool _unifiedTitlebar = false;

  static bool get _isMacOS => !kIsWeb && defaultTargetPlatform == TargetPlatform.macOS;
  static bool get _isWindows => !kIsWeb && defaultTargetPlatform == TargetPlatform.windows;

  /// Whether the unified title bar is currently applied (then interactive
  /// widgets in the top 52 pt need `MacosToolbarPassthrough`, which
  /// `GlassToolbar` adds automatically).
  static bool get unifiedTitlebar => _unifiedTitlebar;

  /// Initialises `macos_window_utils` once (lazily: nothing native is
  /// touched until chrome is requested).
  static Future<bool> _ensureMacInitialized() => _macInit ??= () async {
    try {
      await WindowManipulator.initialize();
      return true;
    } on MissingPluginException {
      return false;
    } on PlatformException {
      return false;
    }
  }();

  /// Applies or removes the unified transparent title bar on macOS.
  static Future<bool> setUnifiedTitlebar({required bool enabled}) async {
    if (!_isMacOS || !await _ensureMacInitialized()) return false;
    try {
      if (enabled) {
        await WindowManipulator.makeTitlebarTransparent();
        await WindowManipulator.enableFullSizeContentView();
        await WindowManipulator.hideTitle();
        await WindowManipulator.addToolbar();
        await WindowManipulator.setToolbarStyle(toolbarStyle: NSWindowToolbarStyle.unified);
      } else {
        await WindowManipulator.removeToolbar();
        await WindowManipulator.showTitle();
        await WindowManipulator.disableFullSizeContentView();
        await WindowManipulator.makeTitlebarOpaque();
      }
      _unifiedTitlebar = enabled;
      return true;
    } on PlatformException {
      return false;
    }
  }

  /// Windows: caption bar colour = ambient base, caption text = label.
  static Future<bool> setCaptionColors({required Color caption, required Color text}) async {
    if (!_isWindows) return false;
    try {
      await windowChannel.invokeMethod<void>('setCaptionColors', {
        'caption': caption.toARGB32(),
        'text': text.toARGB32(),
      });
      return true;
    } on MissingPluginException {
      return false;
    } on PlatformException {
      return false;
    }
  }
}
