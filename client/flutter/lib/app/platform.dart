import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// Platform adaptation helpers. Uses `defaultTargetPlatform` (not
/// `dart:io`) so widget tests can switch platforms with
/// `debugDefaultTargetPlatformOverride` / `TargetPlatformVariant`.
abstract final class AppPlatform {
  static bool get isMacOS => defaultTargetPlatform == TargetPlatform.macOS;

  static bool get isIOS => defaultTargetPlatform == TargetPlatform.iOS;
  static bool get isAndroid => defaultTargetPlatform == TargetPlatform.android;

  /// iOS updates are installed through Apple's distribution mechanisms.
  static bool get supportsDirectUpdates => isMacOS || isWindows || defaultTargetPlatform == TargetPlatform.android;

  static bool get isMobile =>
      defaultTargetPlatform == TargetPlatform.android || defaultTargetPlatform == TargetPlatform.iOS;

  static bool get isWindows => defaultTargetPlatform == TargetPlatform.windows;

  static bool get isLinux => defaultTargetPlatform == TargetPlatform.linux;

  /// Cmd on Apple platforms, Ctrl elsewhere.
  static bool get usesMeta =>
      defaultTargetPlatform == TargetPlatform.macOS || defaultTargetPlatform == TargetPlatform.iOS;

  static SingleActivator primary(LogicalKeyboardKey key, {bool shift = false}) =>
      SingleActivator(key, meta: usesMeta, control: !usesMeta, shift: shift);

  /// "⌘K" on macOS, "Ctrl+K" on Windows ([primary] is ⌘ / Ctrl).
  static String shortcutLabel(LogicalKeyboardKey key, {bool shift = false, bool alt = false, bool primary = true}) {
    final name = switch (key) {
      LogicalKeyboardKey.comma => ',',
      LogicalKeyboardKey.period => '.',
      LogicalKeyboardKey.enter => usesMeta ? '↩' : 'Enter',
      _ => key.keyLabel.length == 1 ? key.keyLabel.toUpperCase() : key.keyLabel,
    };
    if (usesMeta) return '${alt ? '⌥' : ''}${shift ? '⇧' : ''}${primary ? '⌘' : ''}$name';
    return '${primary ? 'Ctrl+' : ''}${alt ? 'Alt+' : ''}${shift ? 'Shift+' : ''}$name';
  }

  /// Monospace family for terminals, keys and commands.
  static String get monospaceFamily => isMacOS ? 'Menlo' : (isWindows ? 'Consolas' : 'monospace');

  static const monospaceFallback = ['SF Mono', 'Cascadia Mono', 'Consolas', 'Menlo', 'DejaVu Sans Mono', 'monospace'];

  /// Common monospace families offered for terminals; only installed ones
  /// render, others fall back to [monospaceFallback].
  static List<String> get terminalFontChoices => [
    if (isMacOS) ..._macFonts,
    if (isWindows) ..._windowsFonts,
    if (!isMacOS && !isWindows) ..._linuxFonts,
    ..._commonFonts,
  ];

  static const _macFonts = [
    'Menlo', // l10n-ignore: font family name
    'Monaco', // l10n-ignore: font family name
    'SF Mono', // l10n-ignore: font family name
    'Courier New', // l10n-ignore: font family name
    'Andale Mono', // l10n-ignore: font family name
  ];
  static const _windowsFonts = [
    'Consolas', // l10n-ignore: font family name
    'Cascadia Mono', // l10n-ignore: font family name
    'Cascadia Code', // l10n-ignore: font family name
    'Lucida Console', // l10n-ignore: font family name
    'Courier New', // l10n-ignore: font family name
  ];
  static const _linuxFonts = [
    'DejaVu Sans Mono', // l10n-ignore: font family name
    'Ubuntu Mono', // l10n-ignore: font family name
    'Liberation Mono', // l10n-ignore: font family name
    'Noto Sans Mono', // l10n-ignore: font family name
  ];
  static const _commonFonts = [
    'JetBrains Mono', // l10n-ignore: font family name
    'Fira Code', // l10n-ignore: font family name
    'Source Code Pro', // l10n-ignore: font family name
    'Hack', // l10n-ignore: font family name
    'IBM Plex Mono', // l10n-ignore: font family name
    'Iosevka', // l10n-ignore: font family name
    'MesloLGS NF', // l10n-ignore: font family name
  ];

  /// Name of the OS authentication used for "unlock with this device"
  /// (product names are not translated).
  static String osAuthName(AppLocalizations l) =>
      isMacOS ? 'Touch ID' : (isWindows ? 'Windows Hello' : l.platformSystemAuthentication);

  /// Suggested device name for registration, in the UI language.
  static String defaultDeviceName(AppLocalizations l) =>
      isMacOS ? l.platformDeviceNameMac : (isWindows ? l.platformDeviceNamePc : l.platformDeviceNameOther);
}
