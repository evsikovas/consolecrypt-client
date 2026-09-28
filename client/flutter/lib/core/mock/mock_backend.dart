import 'package:consolecrypt/core/mock/mock_ai_service.dart';
import 'package:consolecrypt/core/mock/mock_auth_service.dart';
import 'package:consolecrypt/core/mock/mock_backup_service.dart';
import 'package:consolecrypt/core/mock/mock_cloud.dart';
import 'package:consolecrypt/core/mock/mock_config.dart';
import 'package:consolecrypt/core/mock/mock_devices_service.dart';
import 'package:consolecrypt/core/mock/mock_inventory_service.dart';
import 'package:consolecrypt/core/mock/mock_profile_service.dart';
import 'package:consolecrypt/core/mock/mock_prompt_service.dart';
import 'package:consolecrypt/core/mock/mock_settings_service.dart';
import 'package:consolecrypt/core/mock/mock_sftp_browser_service.dart';
import 'package:consolecrypt/core/mock/mock_sftp_service.dart';
import 'package:consolecrypt/core/mock/mock_snippet_service.dart';
import 'package:consolecrypt/core/mock/mock_sync_service.dart';
import 'package:consolecrypt/core/mock/mock_terminal_service.dart';
import 'package:consolecrypt/core/mock/mock_tunnel_service.dart';
import 'package:consolecrypt/core/mock/mock_vault_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';

export 'package:consolecrypt/core/mock/mock_config.dart';

/// In-memory implementation of every service (M3). Replaced by the
/// flutter_rust_bridge implementation by overriding `appServicesProvider`.
final class MockBackend implements DeveloperControls {
  MockBackend({this.config = const MockConfig.demo()}) : cloud = MockCloud(config) {
    profiles = MockProfileService(cloud);
    auth = MockAuthService(cloud);
    vault = MockVaultService(cloud);
    inventory = MockInventoryService(cloud);
    terminal = MockTerminalService(config, inventory);
    sftp = MockSftpService(config, inventory);
    sftpBrowser = MockSftpBrowserService(config, sftp, inventory);
    tunnels = MockTunnelService(cloud);
    snippets = MockSnippetService(cloud);
    ai = MockAiService(cloud);
    devices = MockDevicesService(cloud);
    sync = MockSyncService(cloud);
    settings = MockSettingsService(cloud);
    backups = MockBackupService(cloud);
    files = MockFileDialogService(cloud);
    prompts = MockPromptService();
  }

  final MockConfig config;
  final MockCloud cloud;
  late final MockProfileService profiles;
  late final MockAuthService auth;
  late final MockVaultService vault;
  late final MockInventoryService inventory;
  late final MockTerminalService terminal;
  late final MockSftpService sftp;
  late final MockSftpBrowserService sftpBrowser;
  late final MockTunnelService tunnels;
  late final MockSnippetService snippets;
  late final MockAiService ai;
  late final MockDevicesService devices;
  late final MockSyncService sync;
  late final MockSettingsService settings;
  late final MockBackupService backups;
  late final MockFileDialogService files;
  late final MockPromptService prompts;

  late final AppServices services = AppServices(
    profiles: profiles,
    auth: auth,
    vault: vault,
    inventory: inventory,
    terminal: terminal,
    sftp: sftp,
    tunnels: tunnels,
    snippets: snippets,
    ai: ai,
    devices: devices,
    sync: sync,
    settings: settings,
    backups: backups,
    files: files,
    sftpBrowser: sftpBrowser,
    prompts: prompts,
    developer: this,
    onDispose: dispose,
  );

  // DeveloperControls ---------------------------------------------------------

  @override
  List<String> get demoHints => [
    'Mock backend — nothing leaves this process.',
    'Demo account: ${MockCloud.demoEmail} / ${MockCloud.demoPassword} (trusted device).',
    'New-device account: ${MockCloud.newDeviceEmail} / ${MockCloud.demoPassword}.',
    'Demo vault passphrase: "${MockCloud.demoPassphrase}".',
    'Demo backup file: ${MockCloud.demoBackupPath}',
  ];

  @override
  bool get simulatedOffline => cloud.offline;

  @override
  void setSimulatedOffline(bool offline) => sync.setOffline(offline);

  @override
  Future<void> simulateApprovalFromOtherDevice() => vault.simulateApproval();

  // Test helpers ----------------------------------------------------------------

  /// Signs into the seeded demo account (trusted device) and unlocks.
  Future<void> debugSignInDemoAndUnlock() async {
    await profiles.createSyncedProfile(
      serverUrl: Uri.parse('https://sync.example.org'),
      email: MockCloud.demoEmail,
      password: SecretText(MockCloud.demoPassword),
      deviceName: 'Test Mac',
      createAccount: false,
    );
    await vault.unlockWithPassphrase(SecretText(MockCloud.demoPassphrase));
  }

  /// Creates an unlocked local profile with an empty vault.
  Future<void> debugCreateUnlockedLocalProfile({String name = 'Personal'}) async {
    await profiles.createLocalProfile(name: name);
    await vault.createVault(name: name, passphrase: SecretText('violet-anchor-muffin-glacier-42'));
    await vault.confirmRecoveryKitSaved();
  }

  Future<void> dispose() async {
    await terminal.dispose();
    await prompts.dispose();
    await sftpBrowser.dispose();
    await sftp.dispose();
    await tunnels.dispose();
    await snippets.dispose();
    await ai.dispose();
    await devices.dispose();
    await sync.dispose();
    await settings.dispose();
    await backups.dispose();
    await inventory.dispose();
    await vault.dispose();
    await auth.dispose();
    await cloud.dispose();
  }
}

/// Convenience for tests that need the words of the current Recovery Kit.
extension MockBackendDebug on MockBackend {
  List<String>? get debugActiveRecoveryWords => cloud.activeRecord?.vault?.words;

  VaultStatus get debugVaultStatus => vault.currentStatus;
}
