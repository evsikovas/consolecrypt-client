import 'package:consolecrypt/core/models/ai.dart';
import 'package:consolecrypt/core/models/glass_settings.dart';
import 'package:consolecrypt/core/models/ids.dart';
import 'package:consolecrypt/core/models/monitoring_preferences.dart';
import 'package:consolecrypt/core/models/navigation.dart';
import 'package:consolecrypt/core/models/rdp_mobile.dart';
import 'package:consolecrypt/core/models/saved_forward.dart';
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
final class PasswordVaultConnection {
  const PasswordVaultConnection({
    required this.id,
    required this.name,
    required this.url,
    this.username = '',
    this.credentialId,
  });
  final String id, name, url, username;
  final String? credentialId;
  factory PasswordVaultConnection.fromJson(Map<String, Object?> j) => PasswordVaultConnection(
    id: j['id']! as String,
    name: j['name']! as String,
    url: j['url']! as String,
    username: j['username'] as String? ?? '',
    credentialId: j['credential_id'] as String?,
  );
  Map<String, Object?> toJson() => {
    'id': id,
    'name': name,
    'url': url,
    'username': username,
    'credential_id': credentialId,
  };
}

/// Portable Vault/OpenBao connection. Authentication material lives in the
/// linked encrypted credential. Device biometric handles stay in local settings.
final class SyncedExternalVault {
  const SyncedExternalVault({
    required this.id,
    required this.name,
    required this.endpoint,
    required this.auth,
    required this.username,
    required this.authMount,
    required this.mount,
    required this.namespace,
    required this.versioned,
    this.credentialId,
  });

  final String id, name, endpoint, auth, username, authMount, mount, namespace;
  final bool versioned;
  final String? credentialId;

  factory SyncedExternalVault.fromJson(Map<String, Object?> json) => SyncedExternalVault(
    id: json['id']! as String,
    name: json['name']! as String,
    endpoint: json['endpoint']! as String,
    auth: json['auth']! as String,
    username: json['username']! as String,
    authMount: json['auth_mount']! as String,
    mount: json['mount']! as String,
    namespace: json['namespace']! as String,
    versioned: json['versioned']! as bool,
    credentialId: json['credential_id'] as String?,
  );

  Map<String, Object?> toJson() => {
    'id': id,
    'name': name,
    'endpoint': endpoint,
    'auth': auth,
    'username': username,
    'auth_mount': authMount,
    'mount': mount,
    'namespace': namespace,
    'versioned': versioned,
    'credential_id': credentialId,
  };
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
    this.passwordVaults = const [],
    this.externalVaults = const [],
    this.originalExternalVaults,
  });

  final ObjectId id;
  final String vaultName;
  final TerminalHistoryMode terminalHistoryMode;
  final PrivacyProfile defaultPrivacyProfile;
  final bool syncAiConversations;
  final List<PasswordVaultConnection> passwordVaults;
  final List<SyncedExternalVault> externalVaults;

  /// Snapshot of the catalog that an explicit edit was based on.
  final List<SyncedExternalVault>? originalExternalVaults;
  final DateTime createdAt;
  final DateTime updatedAt;

  VaultSettings copyWith({
    String? vaultName,
    TerminalHistoryMode? terminalHistoryMode,
    PrivacyProfile? defaultPrivacyProfile,
    bool? syncAiConversations,
    List<PasswordVaultConnection>? passwordVaults,
    List<SyncedExternalVault>? externalVaults,
  }) => VaultSettings(
    id: id,
    vaultName: vaultName ?? this.vaultName,
    terminalHistoryMode: terminalHistoryMode ?? this.terminalHistoryMode,
    defaultPrivacyProfile: defaultPrivacyProfile ?? this.defaultPrivacyProfile,
    syncAiConversations: syncAiConversations ?? this.syncAiConversations,
    passwordVaults: passwordVaults ?? this.passwordVaults,
    externalVaults: externalVaults ?? this.externalVaults,
    originalExternalVaults: externalVaults == null ? null : this.externalVaults,
    createdAt: createdAt,
    updatedAt: DateTime.now().toUtc(),
  );
}

enum AppThemeMode { system, light, dark }

/// Presentation of the device-local workspace tools on the right.
enum WorkspacePanelStyle { floating, expanded }

/// Collection screens (databases): cards grid or compact list rows.
enum CollectionView { cards, list }

/// Terminal tab second line: the host's list name ("This computer" for the
/// local shell), its network address (computer name locally), or nothing.
enum TerminalTabSubtitle { name, address, off }

/// Terminal tabs as a row above the terminal or as a column at its left
/// edge (desktop; phones always use the row).
enum TerminalTabPlacement { top, left }

enum TerminalMetric { cpu, ram, disk, received, sent, swap, load, latency }

/// Terminal colours are independent of the interface; system follows it.
enum TerminalColorScheme { system, dark, light, midnight, ocean, forest, amber, custom }

/// When the window is hidden from screenshots, recordings and screen
/// sharing (Windows: `WDA_EXCLUDEFROMCAPTURE`).
enum ScreenCaptureProtection {
  off('off'),

  /// Only while a secret is shown in plain text (default).
  whileSecretsVisible('secrets'),
  always('always');

  const ScreenCaptureProtection(this.wireName);

  /// Persisted value in `local_settings`.
  final String wireName;
}

/// UI language (device-local). [system] follows the OS locale and falls
/// back to English when the OS language is not supported.
enum AppLocale {
  system('system'),
  en('en'),
  ru('ru'),
  es('es'),
  zh('zh');

  const AppLocale(this.wireName);

  /// Persisted value in `local_settings`.
  final String wireName;

  /// Language code for an explicit preference, or `null` for [system].
  String? get languageCode => this == system ? null : wireName;

  static AppLocale fromWire(String? name) => values.where((l) => l.wireName == name).firstOrNull ?? system;
}

/// Device-local preferences (`local_settings` table; never synced).
final class LocalSettings {
  const LocalSettings({
    this.savedForwards = const [],
    this.passwordVaultFiles = const [],
    this.passwordVaultRecentByProfile = const {},
    this.externalVaultConnections = const [],
    this.themeMode = AppThemeMode.system,
    this.uiFontScale = 1,
    this.uiAccentColor,
    this.uiBackgroundColor,
    this.terminalColorScheme = TerminalColorScheme.system,
    this.customTerminalColors,
    this.reopenLastProfile = false,
    this.editorFontSize = 14,
    this.editorFontFamily,
    this.terminalFontSize = 13,
    this.terminalFontFamily,
    this.terminalTabSubtitle = TerminalTabSubtitle.name,
    this.terminalTabGrouped = false,
    this.terminalMetricsBar = true,
    this.allowMultilinePaste = false,
    this.hiddenTerminalMetrics = const {},
    this.terminalMetricColors = const {},
    this.terminalTabPlacement = TerminalTabPlacement.top,
    this.terminalTabColumnWidth = terminalTabColumnDefault,
    this.terminalTabColumnCollapsed = false,
    this.navigationByProfile = const {},
    this.monitoringByProfile = const {},
    this.sidebarCollapsed = false,
    this.sidebarCollapsedBesideTabs,
    this.shortcutOverrides = const {},
    this.terminalScrollback = 5000,
    this.clipboardClearSeconds = 30,
    this.autoLockMinutes = 15,
    this.appLocale = AppLocale.system,
    this.glassMode = GlassMode.standard,
    this.sidebarStyle = SidebarStyle.floating,
    this.workspacePanelStyle = WorkspacePanelStyle.floating,
    this.sftpDefaultEditor,
    this.checkUpdatesAutomatically = true,
    this.updateFeedIssuedAt,
    this.preferDeviceLocalAi = true,
    this.rdpMobile = const RdpMobileSettings(),
    this.databasesView = CollectionView.cards,
    this.dockerTerminalHintShown = false,
    this.dbCodeWrap = true,
    this.screenCaptureProtection = ScreenCaptureProtection.whileSecretsVisible,
    this.dbFetchSizes = const {},
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

  /// Local paths scoped by profile id. Never sent to sync.
  final List<Map<String, String>> passwordVaultFiles;

  /// Recently opened source IDs in newest-first order. No unlock material.
  static const passwordVaultRecentLimit = 5;
  final Map<String, List<String>> passwordVaultRecentByProfile;
  final List<Map<String, String>> externalVaultConnections;
  final List<SavedForward> savedForwards;
  static const editorMinFontSize = 8.0, editorMaxFontSize = 32.0;
  final double editorFontSize;
  final String? editorFontFamily;
  final double terminalFontSize;

  /// Installed monospace family for terminals; null uses the platform default.
  final String? terminalFontFamily;

  /// Second line of terminal tabs: list name, network address, or nothing.
  final TerminalTabSubtitle terminalTabSubtitle;

  /// Row above the terminal or column at its left edge (desktop only).
  final bool terminalTabGrouped;
  final bool terminalMetricsBar;
  final bool allowMultilinePaste;
  final Set<TerminalMetric> hiddenTerminalMetrics;
  final Map<TerminalMetric, int> terminalMetricColors;
  final TerminalTabPlacement terminalTabPlacement;

  /// Width of the expanded tab column, resized by dragging its edge.
  static const terminalTabColumnMin = 160.0;
  static const terminalTabColumnMax = 420.0;
  static const terminalTabColumnDefault = 176.0;
  final double terminalTabColumnWidth;

  /// The tab column is folded to icons.
  final bool terminalTabColumnCollapsed;

  /// The user folded the app sidebar to icons (desktop).
  final Map<String, NavigationPreferences> navigationByProfile;
  final Map<String, MonitoringPreferences> monitoringByProfile;

  final bool sidebarCollapsed;

  /// The user's sidebar choice next to the terminal tab column; `null`
  /// folds it there by default.
  final bool? sidebarCollapsedBesideTabs;

  /// App shortcuts changed by the user: command id → encoded binding, an
  /// empty value turns the shortcut off. Missing ids use the defaults.
  final Map<String, String> shortcutOverrides;
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

  /// Newest accepted update feed `issuedAt`; older signed feeds are refused.
  final DateTime? updateFeedIssuedAt;

  /// Effective only after installing the optional model on this computer.
  /// This preference never changes synced provider defaults.
  final bool preferDeviceLocalAi;
  final RdpMobileSettings rdpMobile;

  /// Databases screen layout: cards grid or compact list.
  final CollectionView databasesView;

  /// The one-time note that a container terminal gives full access
  /// (environment included) was shown in this profile.
  final bool dockerTerminalHintShown;

  /// Database code views (DDL, reviewed statements) wrap long lines.
  final bool dbCodeWrap;

  /// Hide the window from screen capture: never, while secrets are shown, or
  /// always (only where the OS supports it).
  final ScreenCaptureProtection screenCaptureProtection;

  /// Rows the result panel loads per "load more", by database connection id.
  final Map<String, int> dbFetchSizes;

  LocalSettings copyWith({
    List<SavedForward>? savedForwards,
    List<Map<String, String>>? passwordVaultFiles,
    Map<String, List<String>>? passwordVaultRecentByProfile,
    List<Map<String, String>>? externalVaultConnections,
    AppThemeMode? themeMode,
    double? uiFontScale,
    int? uiAccentColor,
    int? uiBackgroundColor,
    bool resetUiColors = false,
    TerminalColorScheme? terminalColorScheme,
    TerminalColors? customTerminalColors,
    bool? reopenLastProfile,
    double? editorFontSize,
    String? editorFontFamily,
    bool resetEditorFontFamily = false,
    double? terminalFontSize,
    String? terminalFontFamily,
    bool resetTerminalFontFamily = false,
    TerminalTabSubtitle? terminalTabSubtitle,
    bool? terminalTabGrouped,
    bool? terminalMetricsBar,
    bool? allowMultilinePaste,
    Set<TerminalMetric>? hiddenTerminalMetrics,
    Map<TerminalMetric, int>? terminalMetricColors,
    TerminalTabPlacement? terminalTabPlacement,
    double? terminalTabColumnWidth,
    bool? terminalTabColumnCollapsed,
    Map<String, NavigationPreferences>? navigationByProfile,
    Map<String, MonitoringPreferences>? monitoringByProfile,
    bool? sidebarCollapsed,
    bool? sidebarCollapsedBesideTabs,
    Map<String, String>? shortcutOverrides,
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
    DateTime? updateFeedIssuedAt,
    bool? preferDeviceLocalAi,
    RdpMobileSettings? rdpMobile,
    CollectionView? databasesView,
    bool? dockerTerminalHintShown,
    bool? dbCodeWrap,
    ScreenCaptureProtection? screenCaptureProtection,
    Map<String, int>? dbFetchSizes,
  }) => LocalSettings(
    savedForwards: savedForwards ?? this.savedForwards,
    passwordVaultFiles: passwordVaultFiles ?? this.passwordVaultFiles,
    passwordVaultRecentByProfile: passwordVaultRecentByProfile ?? this.passwordVaultRecentByProfile,
    externalVaultConnections: externalVaultConnections ?? this.externalVaultConnections,
    themeMode: themeMode ?? this.themeMode,
    uiFontScale: uiFontScale ?? this.uiFontScale,
    uiAccentColor: resetUiColors ? null : (uiAccentColor ?? this.uiAccentColor),
    uiBackgroundColor: resetUiColors ? null : (uiBackgroundColor ?? this.uiBackgroundColor),
    terminalColorScheme: terminalColorScheme ?? this.terminalColorScheme,
    customTerminalColors: customTerminalColors ?? this.customTerminalColors,
    reopenLastProfile: reopenLastProfile ?? this.reopenLastProfile,
    editorFontFamily: resetEditorFontFamily ? null : (editorFontFamily ?? this.editorFontFamily),
    editorFontSize: (editorFontSize ?? this.editorFontSize).clamp(editorMinFontSize, editorMaxFontSize),
    terminalFontSize: terminalFontSize ?? this.terminalFontSize,
    terminalFontFamily: resetTerminalFontFamily ? null : (terminalFontFamily ?? this.terminalFontFamily),
    terminalTabSubtitle: terminalTabSubtitle ?? this.terminalTabSubtitle,
    terminalTabGrouped: terminalTabGrouped ?? this.terminalTabGrouped,
    terminalMetricsBar: terminalMetricsBar ?? this.terminalMetricsBar,
    allowMultilinePaste: allowMultilinePaste ?? this.allowMultilinePaste,
    hiddenTerminalMetrics: hiddenTerminalMetrics ?? this.hiddenTerminalMetrics,
    terminalMetricColors: terminalMetricColors ?? this.terminalMetricColors,
    terminalTabPlacement: terminalTabPlacement ?? this.terminalTabPlacement,
    terminalTabColumnWidth: terminalTabColumnWidth ?? this.terminalTabColumnWidth,
    terminalTabColumnCollapsed: terminalTabColumnCollapsed ?? this.terminalTabColumnCollapsed,
    navigationByProfile: navigationByProfile ?? this.navigationByProfile,
    monitoringByProfile: monitoringByProfile ?? this.monitoringByProfile,
    sidebarCollapsed: sidebarCollapsed ?? this.sidebarCollapsed,
    sidebarCollapsedBesideTabs: sidebarCollapsedBesideTabs ?? this.sidebarCollapsedBesideTabs,
    shortcutOverrides: shortcutOverrides ?? this.shortcutOverrides,
    terminalScrollback: terminalScrollback ?? this.terminalScrollback,
    clipboardClearSeconds: clipboardClearSeconds ?? this.clipboardClearSeconds,
    autoLockMinutes: autoLockMinutes ?? this.autoLockMinutes,
    appLocale: appLocale ?? this.appLocale,
    glassMode: glassMode ?? this.glassMode,
    sidebarStyle: sidebarStyle ?? this.sidebarStyle,
    workspacePanelStyle: workspacePanelStyle ?? this.workspacePanelStyle,
    sftpDefaultEditor: resetSftpDefaultEditor ? null : (sftpDefaultEditor ?? this.sftpDefaultEditor),
    checkUpdatesAutomatically: checkUpdatesAutomatically ?? this.checkUpdatesAutomatically,
    updateFeedIssuedAt: updateFeedIssuedAt ?? this.updateFeedIssuedAt,
    preferDeviceLocalAi: preferDeviceLocalAi ?? this.preferDeviceLocalAi,
    rdpMobile: rdpMobile ?? this.rdpMobile,
    databasesView: databasesView ?? this.databasesView,
    dockerTerminalHintShown: dockerTerminalHintShown ?? this.dockerTerminalHintShown,
    dbCodeWrap: dbCodeWrap ?? this.dbCodeWrap,
    screenCaptureProtection: screenCaptureProtection ?? this.screenCaptureProtection,
    dbFetchSizes: dbFetchSizes ?? this.dbFetchSizes,
  );
}
