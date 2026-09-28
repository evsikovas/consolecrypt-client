import 'package:consolecrypt/core/services/ai_service.dart';
import 'package:consolecrypt/core/services/auth_service.dart';
import 'package:consolecrypt/core/services/backup_service.dart';
import 'package:consolecrypt/core/services/devices_service.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/core/services/inventory_service.dart';
import 'package:consolecrypt/core/services/profile_service.dart';
import 'package:consolecrypt/core/services/prompt_service.dart';
import 'package:consolecrypt/core/services/settings_service.dart';
import 'package:consolecrypt/core/services/sftp_browser_service.dart';
import 'package:consolecrypt/core/services/sftp_service.dart';
import 'package:consolecrypt/core/services/snippet_service.dart';
import 'package:consolecrypt/core/services/sync_service.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/services/tunnel_service.dart';
import 'package:consolecrypt/core/services/vault_service.dart';

export 'package:consolecrypt/core/services/ai_service.dart';
export 'package:consolecrypt/core/services/auth_service.dart';
export 'package:consolecrypt/core/services/backup_service.dart';
export 'package:consolecrypt/core/services/devices_service.dart';
export 'package:consolecrypt/core/services/errors.dart';
export 'package:consolecrypt/core/services/file_dialog_service.dart';
export 'package:consolecrypt/core/services/inventory_service.dart';
export 'package:consolecrypt/core/services/profile_service.dart';
export 'package:consolecrypt/core/services/prompt_service.dart';
export 'package:consolecrypt/core/services/settings_service.dart';
export 'package:consolecrypt/core/services/sftp_browser_service.dart';
export 'package:consolecrypt/core/services/sftp_service.dart';
export 'package:consolecrypt/core/services/snippet_service.dart';
export 'package:consolecrypt/core/services/sync_service.dart';
export 'package:consolecrypt/core/services/terminal_service.dart';
export 'package:consolecrypt/core/services/tunnel_service.dart';
export 'package:consolecrypt/core/services/vault_service.dart';

/// Hooks only the mock backend provides (demo/dev UI). The FRB backend
/// passes `null`, which hides every developer control.
abstract interface class DeveloperControls {
  /// Human-readable hints for demo credentials shown on gate screens.
  List<String> get demoHints;

  bool get simulatedOffline;

  void setSimulatedOffline(bool offline);

  /// Pretends a trusted device approved this device's pending request.
  Future<void> simulateApprovalFromOtherDevice();
}

/// The complete service surface the UI depends on — one swap point between
/// the in-memory mocks and the flutter_rust_bridge implementation
/// (`appServicesProvider` override in `main.dart`).
final class AppServices {
  const AppServices({
    required this.profiles,
    required this.auth,
    required this.vault,
    required this.inventory,
    required this.terminal,
    required this.sftp,
    required this.tunnels,
    required this.snippets,
    required this.ai,
    required this.devices,
    required this.sync,
    required this.settings,
    required this.backups,
    required this.files,
    this.sftpBrowser,
    this.prompts,
    this.developer,
    this.onDispose,
  });

  final ProfileService profiles;
  final AuthService auth;
  final VaultService vault;
  final InventoryService inventory;
  final TerminalService terminal;
  final SftpService sftp;
  final TunnelService tunnels;
  final SnippetService snippets;
  final AiService ai;
  final DevicesService devices;
  final SyncService sync;
  final SettingsService settings;
  final BackupService backups;
  final FileDialogService files;

  /// SFTP browser + edit sessions (SFTP_BROWSER_SPEC). Optional while a
  /// backend does not implement it: the UI then uses [SftpBrowserFallback]
  /// over [sftp] (`sftpBrowserServiceProvider` in `lib/sftp/`).
  final SftpBrowserService? sftpBrowser;

  /// Host-key / password / passphrase prompts outside terminal tabs
  /// (SFTP, tunnels, exec). `null`: such prompts are declined.
  final PromptService? prompts;
  final DeveloperControls? developer;
  final Future<void> Function()? onDispose;

  bool get isMock => developer != null;

  Future<void> dispose() async => onDispose?.call();
}
