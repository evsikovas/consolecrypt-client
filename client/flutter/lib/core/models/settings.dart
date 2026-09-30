import 'package:consolecrypt/core/models/ai.dart';
import 'package:consolecrypt/core/models/glass_settings.dart';
import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/models/sftp_edit.dart';
import 'package:consolecrypt/core/models/terminal_colors.dart';

export 'package:consolecrypt/core/models/glass_settings.dart';

/// Mirrors `cc_models::settings::TerminalHistoryMode` (CLIENT_SPEC §12.4).
enum TerminalHistoryMode {
  /// History stays on this device (default).
  localOnly('local_only'),
  encryptedSync('encrypted_sync'),
  disabled('disabled');

  const TerminalHistoryMode(this.wireName);

  final String wireName;
}

/// Mirrors `cc_models::settings::VaultSettings` — synced, E2EE.
final class VaultSettings {
  const VaultSettings({
    required this.id,
    required this.vaultName,
    required this.createdAt,
    required this.updatedAt,
    this.terminalHistoryMode = TerminalHistoryMode.localOnly,
    this.defaultPrivacyProfile = PrivacyProfile.strict,
    this.syncAiConversations = false,
  });

  final ObjectId id;
  final String vaultName;
  final TerminalHistoryMode terminalHistoryMode;
  final PrivacyProfile defaultPrivacyProfile;
  final bool syncAiConversations;
  final DateTime createdAt;
  final DateTime updatedAt;

  VaultSettings copyWith({
    String? vaultName,
    TerminalHistoryMode? terminalHistoryMode,
    PrivacyProfile? defaultPrivacyProfile,
    bool? syncAiConversations,
  }) => VaultSettings(
    id: id,
    vaultName: vaultName ?? this.vaultName,
    terminalHistoryMode: terminalHistoryMode ?? this.terminalHistoryMode,
    defaultPrivacyProfile: defaultPrivacyProfile ?? this.defaultPrivacyProfile,
    syncAiConversations: syncAiConversations ?? this.syncAiConversations,
    createdAt: createdAt,
    updatedAt: DateTime.now().toUtc(),
  );
}

enum AppThemeMode { system, light, dark }

/// Presentation of the device-local workspace tools on the right.
enum WorkspacePanelStyle { floating, expanded }

/// Terminal colours are independent of the interface; system follows it.
enum TerminalColorScheme { system, dark, light, midnight, ocean, forest, amber, custom }

/// UI language (device-local). [system] follows the OS locale and falls
/// back to English when the OS language is not supported.
enum AppLocale {
  system('system'),
  en('en'),
  ru('ru');

  const AppLocale(this.wireName);

  /// Persisted value in `local_settings`.
  final String wireName;

  /// Language code for [en]/[ru]; `null` for [system].
  String? get languageCode => this == system ? null : wireName;

  static AppLocale fromWire(String? name) => values.where((l) => l.wireName == name).firstOrNull ?? system;
}

/// Device-local preferences (`local_settings` table; never synced).
final class LocalSettings {
  const LocalSettings({
    this.themeMode = AppThemeMode.system,
    this.uiFontScale = 1,
    this.uiAccentColor,
    this.uiBackgroundColor,
    this.terminalColorScheme = TerminalColorScheme.system,
    this.customTerminalColors,
    this.reopenLastProfile = false,
    this.terminalFontSize = 13,
    this.terminalScrollback = 5000,
    this.clipboardClearSeconds = 30,
    this.autoLockMinutes = 15,
    this.appLocale = AppLocale.system,
    this.glassMode = GlassMode.standard,
    this.sidebarStyle = SidebarStyle.floating,
    this.workspacePanelStyle = WorkspacePanelStyle.floating,
    this.sftpDefaultEditor,
    this.checkUpdatesAutomatically = true,
  });

  final AppThemeMode themeMode;

  /// 100% is the compact base (90% of the original type scale).
  static const uiFontBase = .9;
  static const uiFontScaleMin = .8;
  static const uiFontScaleMax = 1.4;
  final double uiFontScale;
  double get effectiveUiFontScale =>
      uiFontBase * (uiFontScale.isFinite ? uiFontScale.clamp(uiFontScaleMin, uiFontScaleMax) : 1);

  /// Optional opaque RGB colours (0x000000–0xFFFFFF), never synced.
  final int? uiAccentColor;
  final int? uiBackgroundColor;
  final TerminalColorScheme terminalColorScheme;
  final TerminalColors? customTerminalColors;

  /// Opening a profile can prompt for OS keychain access. Opt-in at startup.
  final bool reopenLastProfile;
  final double terminalFontSize;
  final int terminalScrollback;

  /// Copied secrets are wiped from the clipboard after this many seconds.
  final int clipboardClearSeconds;

  /// Lock the vault after this much inactivity (0 = never).
  final int autoLockMinutes;

  /// UI language; applied immediately (no restart).
  final AppLocale appLocale;

  /// Liquid Glass intensity (LIQUID_GLASS_SPEC §2.2); the OS Reduce
  /// Transparency setting still wins (see `resolveEffectiveGlass`).
  final GlassMode glassMode;

  /// Floating inset or edge-to-edge sidebar (LIQUID_GLASS_SPEC §2.5).
  final SidebarStyle sidebarStyle;

  final WorkspacePanelStyle workspacePanelStyle;

  /// Device-local editor for SFTP Open; null uses the OS file association.
  /// Application paths must never travel in vault sync.
  final AppRef? sftpDefaultEditor;

  /// Only release metadata is fetched. Installers require user confirmation.
  final bool checkUpdatesAutomatically;

  LocalSettings copyWith({
    AppThemeMode? themeMode,
    double? uiFontScale,
    int? uiAccentColor,
    int? uiBackgroundColor,
    bool resetUiColors = false,
    TerminalColorScheme? terminalColorScheme,
    TerminalColors? customTerminalColors,
    bool? reopenLastProfile,
    double? terminalFontSize,
    int? terminalScrollback,
    int? clipboardClearSeconds,
    int? autoLockMinutes,
    AppLocale? appLocale,
    GlassMode? glassMode,
    SidebarStyle? sidebarStyle,
    WorkspacePanelStyle? workspacePanelStyle,
    AppRef? sftpDefaultEditor,
    bool resetSftpDefaultEditor = false,
    bool? checkUpdatesAutomatically,
  }) => LocalSettings(
    themeMode: themeMode ?? this.themeMode,
    uiFontScale: uiFontScale ?? this.uiFontScale,
    uiAccentColor: resetUiColors ? null : (uiAccentColor ?? this.uiAccentColor),
    uiBackgroundColor: resetUiColors ? null : (uiBackgroundColor ?? this.uiBackgroundColor),
    terminalColorScheme: terminalColorScheme ?? this.terminalColorScheme,
    customTerminalColors: customTerminalColors ?? this.customTerminalColors,
    reopenLastProfile: reopenLastProfile ?? this.reopenLastProfile,
    terminalFontSize: terminalFontSize ?? this.terminalFontSize,
    terminalScrollback: terminalScrollback ?? this.terminalScrollback,
    clipboardClearSeconds: clipboardClearSeconds ?? this.clipboardClearSeconds,
    autoLockMinutes: autoLockMinutes ?? this.autoLockMinutes,
    appLocale: appLocale ?? this.appLocale,
    glassMode: glassMode ?? this.glassMode,
    sidebarStyle: sidebarStyle ?? this.sidebarStyle,
    workspacePanelStyle: workspacePanelStyle ?? this.workspacePanelStyle,
    sftpDefaultEditor: resetSftpDefaultEditor ? null : (sftpDefaultEditor ?? this.sftpDefaultEditor),
    checkUpdatesAutomatically: checkUpdatesAutomatically ?? this.checkUpdatesAutomatically,
  );
}
