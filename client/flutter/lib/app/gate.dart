import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Where the user is in the access flow. Drives router redirects so no
/// screen behind the gate is reachable without an unlocked vault.
enum AppStage {
  loading,

  /// No active profile: "Use locally" / "Connect to a server" / restore.
  welcome,

  /// Active synced profile without a session.
  signedOut,

  /// Active profile has no vault yet → create passphrase.
  needsVault,

  /// Vault created, Recovery Kit not verified yet (mandatory).
  recoveryKitPending,
  locked,
  awaitingApproval,
  unlocked,
}

/// Pure stage computation (unit-tested).
AppStage computeStage(ProfilesState profiles, AuthState auth, VaultStatus vault) {
  final active = profiles.active;
  if (active == null) return AppStage.welcome;
  if (active.isSynced && !auth.isSignedIn) return AppStage.signedOut;
  switch (vault.phase) {
    case VaultPhase.none:
      return AppStage.needsVault;
    case VaultPhase.locked:
      return AppStage.locked;
    case VaultPhase.awaitingApproval:
      return AppStage.awaitingApproval;
    case VaultPhase.unlocked:
      return vault.recoveryKitConfirmed ? AppStage.unlocked : AppStage.recoveryKitPending;
  }
}

final appStageProvider = Provider<AppStage>((ref) {
  final profiles = ref.watch(profilesProvider);
  final auth = ref.watch(authStateProvider);
  final vault = ref.watch(vaultStatusProvider);
  if (!profiles.hasValue || !auth.hasValue || !vault.hasValue) return AppStage.loading;
  return computeStage(profiles.requireValue, auth.requireValue, vault.requireValue);
});

abstract final class AppRoutes {
  static const loading = '/loading';
  static const welcome = '/welcome';
  static const login = '/login';
  static const restore = '/restore';
  static const onboarding = '/onboarding';
  static const recoveryKit = '/onboarding/kit';
  static const verifyKit = '/onboarding/verify';
  static const localNotice = '/onboarding/local-notice';
  static const unlock = '/unlock';
  static const approval = '/unlock/approval';
  static const recovery = '/recovery';

  static const hosts = '/hosts';
  static const newHost = '/hosts/new';
  static String editHost(ObjectId id) => '/hosts/${id.value}';
  static const groups = '/groups';
  static const credentials = '/credentials';
  static const knownHosts = '/known-hosts';
  static const terminal = '/terminal';
  static const sftp = '/sftp';
  static const tunnels = '/tunnels';
  static const snippets = '/snippets';
  static const ai = '/ai';
  static const devices = '/devices';
  static const sync = '/sync';
  static const backups = '/backups';
  static const settings = '/settings';
  static const sharing = '/sharing';

  /// Profile-creation routes, reachable from every stage ("Add profile").
  static const profileCreation = {welcome, login, restore};

  static const shellPrefixes = [
    hosts,
    groups,
    credentials,
    knownHosts,
    terminal,
    sftp,
    tunnels,
    snippets,
    ai,
    devices,
    sync,
    backups,
    settings,
    sharing,
  ];
}

String stageHome(AppStage stage) => switch (stage) {
  AppStage.loading => AppRoutes.loading,
  AppStage.welcome => AppRoutes.welcome,
  AppStage.signedOut => AppRoutes.login,
  AppStage.needsVault => AppRoutes.onboarding,
  AppStage.recoveryKitPending => AppRoutes.recoveryKit,
  AppStage.locked => AppRoutes.unlock,
  AppStage.awaitingApproval => AppRoutes.approval,
  AppStage.unlocked => AppRoutes.hosts,
};

bool _isShellLocation(String location) =>
    AppRoutes.shellPrefixes.any((p) => location == p || location.startsWith('$p/'));

/// Pure redirect rule (unit-tested): `null` = stay.
String? redirectFor(AppStage stage, String location) {
  if (AppRoutes.profileCreation.contains(location)) return null;
  final allowed = switch (stage) {
    AppStage.loading => location == AppRoutes.loading,
    AppStage.welcome || AppStage.signedOut => false,
    AppStage.needsVault => location == AppRoutes.onboarding,
    AppStage.recoveryKitPending =>
      location == AppRoutes.recoveryKit || location == AppRoutes.verifyKit || location == AppRoutes.localNotice,
    AppStage.locked => location == AppRoutes.unlock || location == AppRoutes.recovery,
    AppStage.awaitingApproval => location == AppRoutes.approval,
    AppStage.unlocked => _isShellLocation(location),
  };
  return allowed ? null : stageHome(stage);
}
