import 'dart:io';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/settings.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/updates/update_controller.dart';
import 'package:consolecrypt/updates/update_service.dart';
import 'package:consolecrypt/updates/update_settings.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  testWidgets('iOS uses Apple updates without probing the desktop update server', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    final source = backend.services;
    final production = AppServices(
      profiles: source.profiles,
      auth: source.auth,
      vault: source.vault,
      inventory: source.inventory,
      terminal: source.terminal,
      sftp: source.sftp,
      tunnels: source.tunnels,
      snippets: source.snippets,
      ai: source.ai,
      devices: source.devices,
      sync: source.sync,
      settings: source.settings,
      backups: source.backups,
      files: source.files,
    );
    final fake = _Service();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [appServicesProvider.overrideWithValue(production), updateServiceProvider.overrideWithValue(fake)],
        child: const MaterialApp(
          localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(body: UpdateNoticeScope(child: UpdateSettingsSection())),
        ),
      ),
    );
    await settle(tester);
    expect(find.textContaining('App Store, TestFlight or Xcode'), findsOneWidget);
    expect(find.byKey(const ValueKey('updates-check')), findsNothing);
    expect(find.byKey(const ValueKey('updates-automatic')), findsNothing);
    expect(fake.checks, 0);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.iOS));
  for (final automatic in [true, false]) {
    testWidgets('startup respects persisted automatic=$automatic; manual check always works', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      await backend.services.settings.updateLocal(const LocalSettings(checkUpdatesAutomatically: false));
      if (automatic) await backend.services.settings.updateLocal(const LocalSettings());
      final source = backend.services;
      // Use the production flag with in-memory services: no Rust or network.
      final services = AppServices(
        profiles: source.profiles,
        auth: source.auth,
        vault: source.vault,
        inventory: source.inventory,
        terminal: source.terminal,
        sftp: source.sftp,
        tunnels: source.tunnels,
        snippets: source.snippets,
        ai: source.ai,
        devices: source.devices,
        sync: source.sync,
        settings: source.settings,
        backups: source.backups,
        files: source.files,
      );
      final fake = _Service();
      await tester.pumpWidget(
        ProviderScope(
          overrides: [appServicesProvider.overrideWithValue(services), updateServiceProvider.overrideWithValue(fake)],
          child: const MaterialApp(
            localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            supportedLocales: AppLocalizations.supportedLocales,
            home: Scaffold(
              body: UpdateNoticeScope(child: SingleChildScrollView(child: UpdateSettingsSection())),
            ),
          ),
        ),
      );
      await settle(tester);
      expect(fake.checks, automatic ? 1 : 0);
      await tester.tap(find.byKey(const ValueKey('updates-check')));
      await settle(tester);
      expect(fake.checks, automatic ? 2 : 1);
      await tester.tap(find.byKey(const ValueKey('updates-automatic')));
      await settle(tester);
      expect(source.settings.currentLocal.checkUpdatesAutomatically, !automatic);
      expect(tester.takeException(), isNull);
    });
  }
  testWidgets('installation needs confirmation; cancelling never downloads or starts an installer', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    final fake = _Service(offer: true);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          updateServiceProvider.overrideWithValue(fake),
        ],
        child: const MaterialApp(
          localizationsDelegates: [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(body: SingleChildScrollView(child: UpdateSettingsSection())),
        ),
      ),
    );
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('updates-check')));
    await settle(tester);
    expect(find.text('Version 9.0.0 is available'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('updates-install')));
    await settle(tester);
    expect(find.byType(AlertDialog), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await settle(tester);
    expect(fake.downloads, 0);
    expect(find.byType(AlertDialog), findsNothing);
  });
}

class _Service extends UpdateService {
  _Service({this.offer = false});
  final bool offer;
  int checks = 0;
  int downloads = 0;
  @override
  Future<File> download(UpdateRelease release, void Function(double) progress) async {
    downloads++;
    throw const UpdateException('test_download');
  }

  @override
  Future<UpdateRelease?> check() async {
    checks++;
    return offer
        ? UpdateRelease(
            version: '9.0.0',
            build: 9,
            platform: 'windows-x64',
            url: Uri.parse('https://git.evsikov.net/client.exe'),
            bytes: 1,
            sha256: '0' * 64,
            notes: const {},
          )
        : null;
  }
}
