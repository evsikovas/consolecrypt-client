import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// Platform adaptation helpers. Uses `defaultTargetPlatform` (not
/// `dart:io`) so widget tests can switch platforms with
/// `debugDefaultTargetPlatformOverride` / `TargetPlatformVariant`.
abstract final class AppPlatform {
  static bool get isMacOS => defaultTargetPlatform == TargetPlatform.macOS;

  static bool get isMobile =>
      defaultTargetPlatform == TargetPlatform.android || defaultTargetPlatform == TargetPlatform.iOS;

  static bool get isWindows => defaultTargetPlatform == TargetPlatform.windows;

  /// Cmd on Apple platforms, Ctrl elsewhere.
  static bool get usesMeta =>
      defaultTargetPlatform == TargetPlatform.macOS || defaultTargetPlatform == TargetPlatform.iOS;

  static SingleActivator primary(LogicalKeyboardKey key, {bool shift = false}) =>
      SingleActivator(key, meta: usesMeta, control: !usesMeta, shift: shift);

  /// "⌘K" on macOS, "Ctrl+K" on Windows.
  static String shortcutLabel(LogicalKeyboardKey key, {bool shift = false}) {
    final name = switch (key) {
      LogicalKeyboardKey.comma => ',',
      LogicalKeyboardKey.enter => usesMeta ? '↩' : 'Enter',
      _ => key.keyLabel.toUpperCase(),
    };
    if (usesMeta) return '${shift ? '⇧' : ''}⌘$name';
    return 'Ctrl+${shift ? 'Shift+' : ''}$name';
  }

  /// Monospace family for terminals, keys and commands.
  static String get monospaceFamily => isMacOS ? 'Menlo' : (isWindows ? 'Consolas' : 'monospace');

  static const monospaceFallback = ['SF Mono', 'Cascadia Mono', 'Consolas', 'Menlo', 'DejaVu Sans Mono', 'monospace'];

  /// Name of the OS authentication used for "unlock with this device"
  /// (product names are not translated).
  static String osAuthName(AppLocalizations l) =>
      isMacOS ? 'Touch ID' : (isWindows ? 'Windows Hello' : l.platformSystemAuthentication);

  /// Suggested device name for registration, in the UI language.
  static String defaultDeviceName(AppLocalizations l) =>
      isMacOS ? l.platformDeviceNameMac : (isWindows ? l.platformDeviceNamePc : l.platformDeviceNameOther);
}
