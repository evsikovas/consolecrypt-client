import 'dart:async';
import 'dart:io';
import 'dart:ui' show AppExitResponse;

import 'package:consolecrypt/app/app_info.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/bridge/rust_backup.dart';
import 'package:consolecrypt/core/bridge/rust_enrollment.dart';
import 'package:consolecrypt/core/bridge/rust_inventory.dart';
import 'package:consolecrypt/core/bridge/rust_prompts.dart';
import 'package:consolecrypt/core/bridge/rust_sessions.dart';
import 'package:consolecrypt/core/bridge/rust_sftp_browser.dart';
import 'package:consolecrypt/core/bridge/rust_sharing.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/src/rust/api/app.dart' as rs_app;
import 'package:consolecrypt/src/rust/frb_generated.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// Start-up options of the Rust core (production defaults).
final class RustCoreOptions {
  const RustCoreOptions({
    this.dataDir,
    this.keychainService,
    this.inMemorySecureStore = false,
    this.fastKdfForTests = false,
    this.backgroundSync = true,
    this.autoStartTunnels = true,
    this.clientVersion = appClientVersion,
    this.hookAppExit = true,
  });

  /// `null` = the ConsoleCrypt platform data directory.
  final String? dataDir;

  /// `null` = `io.consolecrypt.ConsoleCrypt`.
  final String? keychainService;

  /// Keys in process memory only (nothing survives a restart) — tests.
  final bool inMemorySecureStore;

  /// Argon2id server floor instead of calibration — tests only.
  final bool fastKdfForTests;

  final bool backgroundSync;
  final bool autoStartTunnels;
  final String clientVersion;

  /// Shut the core down (lock, close SQLCipher) when the app is asked to
  /// exit (Cmd+Q, last window closed, Windows close button).
  final bool hookAppExit;
}

/// Matches `version:` in pubspec.yaml (reported to servers and in backups).
const appClientVersion = kAppFullVersion;

/// The flutter_rust_bridge implementation of every service interface
/// (ADR-0101 §8): one global app-core instance behind `RustLib`.
final class RustAppServices {
  RustAppServices._(this.backend, this.services, this._exitListener);

  final RustBackend backend;
  final AppServices services;
  final AppLifecycleListener? _exitListener;

  static RustAppServices? _current;

  /// The instance opened by [open] (read by `appServicesProvider`).
  static RustAppServices? get current => _current;

  static Future<void>? _libInit;

  /// Loads the native library once (throws if it cannot be loaded).
  static Future<void> loadLibrary() => _libInit ??= RustLib.init();

  /// Loads the native core, initializes app-core and reopens the last
  /// profile (locked). Throws [AppException] / loader errors on failure.
  static Future<RustAppServices> open([RustCoreOptions options = const RustCoreOptions()]) async {
    final existing = _current;
    if (existing != null) return existing;
    await loadLibrary();
    final dataDir =
        options.dataDir ??
        (Platform.isAndroid || Platform.isIOS
            ? await MethodChannel(Platform.isIOS ? 'consolecrypt/ios' : 'consolecrypt/android')
                  .invokeMethod<String>('dataDirectory')
            : null);
    if ((Platform.isAndroid || Platform.isIOS) && (dataDir == null || dataDir.isEmpty)) {
      throw StateError('Mobile private storage unavailable');
    }
    final info = await guard(
      () => rs_app.coreInit(
        config: rs_app.CoreConfig(
          dataDir: dataDir,
          secureStore: options.inMemorySecureStore ? rs_app.SecureStoreChoice.memory : rs_app.SecureStoreChoice.os,
          keychainService: options.keychainService,
          kdf: options.fastKdfForTests ? rs_app.KdfChoice.floor : rs_app.KdfChoice.calibrate,
          clientVersion: options.clientVersion,
          backgroundSync: options.backgroundSync,
          autoStartTunnels: options.autoStartTunnels,
        ),
      ),
    );
    final hub = RustBackend(info);
    final terminal = RustTerminalService(hub);
    final sftp = RustSftpService(hub);
    final sftpBrowser = RustSftpBrowserService(hub, sftp);
    final prompts = RustPromptService(hub);
    final backups = RustBackupService(hub);
    final settings = RustSettingsService(hub);
    late final RustAppServices self;
    Future<void> dispose() async {
      self._exitListener?.dispose();
      await terminal.dispose();
      await sftpBrowser.dispose();
      await prompts.dispose();
      await sftp.dispose();
      await backups.dispose();
      await settings.flushLocalWrites();
      await hub.shutdown();
      await hub.dispose();
      if (identical(_current, self)) _current = null;
    }

    final services = AppServices(
      profiles: RustProfileService(hub),
      auth: RustAuthService(hub, options.clientVersion),
      vault: RustVaultService(hub),
      inventory: RustInventoryService(hub),
      terminal: terminal,
      sftp: sftp,
      tunnels: RustTunnelService(hub),
      snippets: RustSnippetService(hub),
      ai: RustAiService(hub),
      devices: RustDevicesService(hub),
      sync: RustSyncService(hub),
      settings: settings,
      backups: backups,
      files: const NativeFileDialogService(),
      sftpBrowser: sftpBrowser,
      prompts: prompts,
      sharing: RustSharingService(hub),
      enrollment: const RustEnrollmentService(),
      onDispose: dispose,
    );
    final exitListener = options.hookAppExit
        ? AppLifecycleListener(
            onExitRequested: () async {
              await settings.flushLocalWrites();
              await hub.shutdown();
              return AppExitResponse.exit;
            },
          )
        : null;
    self = RustAppServices._(hub, services, exitListener);
    await hub.start();
    return _current = self;
  }

  /// Locks and closes everything, then releases the Dart side.
  Future<void> close() => services.dispose();
}
