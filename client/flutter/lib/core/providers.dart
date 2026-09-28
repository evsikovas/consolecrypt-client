import 'dart:async';

import 'package:consolecrypt/core/bridge/rust_app_services.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// `--dart-define=CC_MOCK=true`: run the UI on the in-memory mock backend
/// (demos, UI work without the Rust toolchain). Widget tests override
/// [appServicesProvider] with their own `MockBackend` instead.
const useMockBackend = bool.fromEnvironment('CC_MOCK');

/// THE swap point (ADR-0101 §8). Default: the real Rust core
/// ([RustAppServices], opened in `main.dart` before `runApp`); with
/// `CC_MOCK=true` the in-memory mock backend.
final appServicesProvider = Provider<AppServices>((ref) {
  if (useMockBackend) {
    final backend = MockBackend();
    ref.onDispose(() => unawaited(backend.dispose()));
    return backend.services;
  }
  final rust = RustAppServices.current;
  if (rust == null) {
    throw StateError('RustAppServices.open() must complete before the app starts (see main.dart)');
  }
  return rust.services;
});

// Service accessors --------------------------------------------------------

final profileServiceProvider = Provider<ProfileService>((ref) => ref.watch(appServicesProvider).profiles);
final authServiceProvider = Provider<AuthService>((ref) => ref.watch(appServicesProvider).auth);
final vaultServiceProvider = Provider<VaultService>((ref) => ref.watch(appServicesProvider).vault);
final inventoryServiceProvider = Provider<InventoryService>((ref) => ref.watch(appServicesProvider).inventory);
final terminalServiceProvider = Provider<TerminalService>((ref) => ref.watch(appServicesProvider).terminal);
final sftpServiceProvider = Provider<SftpService>((ref) => ref.watch(appServicesProvider).sftp);
final tunnelServiceProvider = Provider<TunnelService>((ref) => ref.watch(appServicesProvider).tunnels);
final snippetServiceProvider = Provider<SnippetService>((ref) => ref.watch(appServicesProvider).snippets);
final aiServiceProvider = Provider<AiService>((ref) => ref.watch(appServicesProvider).ai);
final devicesServiceProvider = Provider<DevicesService>((ref) => ref.watch(appServicesProvider).devices);
final syncServiceProvider = Provider<SyncService>((ref) => ref.watch(appServicesProvider).sync);
final settingsServiceProvider = Provider<SettingsService>((ref) => ref.watch(appServicesProvider).settings);
final backupServiceProvider = Provider<BackupService>((ref) => ref.watch(appServicesProvider).backups);
final fileDialogServiceProvider = Provider<FileDialogService>((ref) => ref.watch(appServicesProvider).files);
final developerControlsProvider = Provider<DeveloperControls?>((ref) => ref.watch(appServicesProvider).developer);

// State streams ------------------------------------------------------------

final profilesProvider = StreamProvider<ProfilesState>((ref) => ref.watch(profileServiceProvider).watchProfiles());
final activeProfileProvider = Provider<Profile?>((ref) => ref.watch(profilesProvider).value?.active);
final authStateProvider = StreamProvider<AuthState>((ref) => ref.watch(authServiceProvider).watchAuthState());
final vaultStatusProvider = StreamProvider<VaultStatus>((ref) => ref.watch(vaultServiceProvider).watchStatus());

final hostsProvider = StreamProvider<List<Host>>((ref) => ref.watch(inventoryServiceProvider).watchHosts());
final groupsProvider = StreamProvider<List<Group>>((ref) => ref.watch(inventoryServiceProvider).watchGroups());
final jumpProfilesProvider = StreamProvider<List<JumpProfile>>(
  (ref) => ref.watch(inventoryServiceProvider).watchJumpProfiles(),
);
final credentialsProvider = StreamProvider<List<Credential>>(
  (ref) => ref.watch(inventoryServiceProvider).watchCredentials(),
);
final knownHostsProvider = StreamProvider<List<KnownHost>>(
  (ref) => ref.watch(inventoryServiceProvider).watchKnownHosts(),
);
final snippetsProvider = StreamProvider<List<Snippet>>((ref) => ref.watch(snippetServiceProvider).watchSnippets());
final tunnelsProvider = StreamProvider<List<Tunnel>>((ref) => ref.watch(tunnelServiceProvider).watchTunnels());
final tunnelRuntimeProvider = StreamProvider<Map<ObjectId, TunnelRuntime>>(
  (ref) => ref.watch(tunnelServiceProvider).watchRuntime(),
);
final aiProvidersProvider = StreamProvider<List<AiProviderConfig>>(
  (ref) => ref.watch(aiServiceProvider).watchProviders(),
);
final devicesProvider = StreamProvider<DevicesSnapshot>((ref) => ref.watch(devicesServiceProvider).watchDevices());
final syncStatusProvider = StreamProvider<SyncStatus>((ref) => ref.watch(syncServiceProvider).watchStatus());
final localSettingsProvider = StreamProvider<LocalSettings>((ref) => ref.watch(settingsServiceProvider).watchLocal());
final vaultSettingsProvider = StreamProvider<VaultSettings?>((ref) => ref.watch(settingsServiceProvider).watchVault());
final transfersProvider = StreamProvider<List<TransferJob>>((ref) => ref.watch(sftpServiceProvider).watchTransfers());
final backupScheduleProvider = StreamProvider<BackupSchedule>(
  (ref) => ref.watch(backupServiceProvider).watchSchedule(),
);
final recentBackupsProvider = StreamProvider<List<BackupInfo>>(
  (ref) => ref.watch(backupServiceProvider).watchRecentBackups(),
);

/// Lookup helpers (id → object) used across screens.
final hostByIdProvider = Provider<Map<ObjectId, Host>>(
  (ref) => {for (final h in ref.watch(hostsProvider).value ?? const <Host>[]) h.id: h},
);
final credentialByIdProvider = Provider<Map<ObjectId, Credential>>(
  (ref) => {for (final c in ref.watch(credentialsProvider).value ?? const <Credential>[]) c.id: c},
);
final groupByIdProvider = Provider<Map<ObjectId, Group>>(
  (ref) => {for (final g in ref.watch(groupsProvider).value ?? const <Group>[]) g.id: g},
);

/// Clipboard with auto-clear duration from settings.
final secureClipboardProvider = Provider<SecureClipboard>((ref) {
  final seconds = ref.watch(localSettingsProvider).value?.clipboardClearSeconds ?? 30;
  final clipboard = SecureClipboard(clearAfter: Duration(seconds: seconds));
  ref.onDispose(clipboard.dispose);
  return clipboard;
});
