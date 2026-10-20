import 'dart:async';
import 'dart:io';
import 'dart:ui' show PointerDeviceKind;

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/updates/update_controller.dart';
import 'package:consolecrypt/updates/update_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  for (final language in [AppLocale.en, AppLocale.ru]) {
    testWidgets('startup notice can hover, confirm and download above the real app router ($language)', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      setTestLocale(backend, language);
      final service = _Service();
      addTearDown(service.dispose);
      tester.view.physicalSize = AppPlatform.isMobile ? const Size(412, 915) : const Size(1200, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            appServicesProvider.overrideWithValue(backend.services),
            updateServiceProvider.overrideWithValue(service),
          ],
          child: const ConsoleCryptApp(),
        ),
      );
      await settle(tester);
      final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
      await container.read(updateControllerProvider.notifier).check();
      await settle(tester);
      final label = language == AppLocale.ru ? 'Скачать и установить' : 'Download and install';
      final download = find.byTooltip(label);
      expect(download, findsOneWidget);

      final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
      await mouse.addPointer(location: Offset.zero);
      await mouse.moveTo(tester.getCenter(download));
      await settle(tester);
      expect(tester.takeException(), isNull, reason: 'hover must have an Overlay above the notice');
      expect(service.downloads, 0);
      await mouse.removePointer();
      await tester.tap(download);
      await settle(tester);
      expect(tester.takeException(), isNull, reason: 'confirmation must use the app Navigator');
      expect(find.byType(AlertDialog), findsOneWidget);
      await tester.tap(find.text(language == AppLocale.ru ? 'Отмена' : 'Cancel'));
      await settle(tester);
      expect(service.downloads, 0);

      await tester.tap(download);
      await settle(tester);
      await tester.tap(find.widgetWithText(FilledButton, label));
      await settle(tester);
      expect(service.downloads, 1);
      expect(container.read(updateControllerProvider).phase, UpdatePhase.downloading);
      expect(find.textContaining('25%'), findsOneWidget);
      service.completeDownload();
      await settle(tester);
      expect(service.installs, 1);
      expect(container.read(updateControllerProvider).phase, UpdatePhase.opened);
      expect(download, findsNothing);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await settle(tester);
      expect(tester.takeException(), isNull, reason: 'the notice must not leave a detached overlay entry');
    }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.macOS, TargetPlatform.android}));
  }

  testWidgets('dismissing and unmounting an active notice removes its overlay', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    final service = _Service();
    addTearDown(service.dispose);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          updateServiceProvider.overrideWithValue(service),
        ],
        child: const ConsoleCryptApp(),
      ),
    );
    await settle(tester);
    final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
    final controller = container.read(updateControllerProvider.notifier);
    await controller.check();
    await settle(tester);
    expect(find.byKey(const ValueKey('update-notice')), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('update-notice-dismiss')));
    await settle(tester);
    expect(find.byKey(const ValueKey('update-notice')), findsNothing);
    await controller.check();
    await settle(tester);
    expect(find.byKey(const ValueKey('update-notice')), findsNothing);
    service.version = '9.0.1';
    await controller.check();
    await settle(tester);
    expect(find.byKey(const ValueKey('update-notice')), findsOneWidget);
    await tester.pumpWidget(const SizedBox.shrink());
    await settle(tester);
    expect(tester.takeException(), isNull);
    expect(service.downloads, 0);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('confirmation cannot silently install a replacement release', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    setTestLocale(backend, AppLocale.en);
    final service = _Service();
    addTearDown(service.dispose);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          updateServiceProvider.overrideWithValue(service),
        ],
        child: const ConsoleCryptApp(),
      ),
    );
    await settle(tester);
    final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
    final controller = container.read(updateControllerProvider.notifier);
    await controller.check();
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('update-notice-install')));
    await settle(tester);
    service.version = '9.0.1';
    await controller.check();
    await settle(tester);
    await tester.tap(find.widgetWithText(FilledButton, 'Download and install'));
    await settle(tester);
    expect(service.downloads, 0);
    expect(container.read(updateControllerProvider).release!.version, '9.0.1');
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await settle(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}

class _Service extends UpdateService {
  final _download = Completer<File>();
  Directory? _folder;
  int downloads = 0;
  int installs = 0;
  String version = '9.0.0';

  @override
  Future<UpdateRelease?> check() async => UpdateRelease(
    version: version,
    build: 9,
    platform: 'windows-x64',
    url: Uri.parse('https://updates.consolecrypt.dev/test-installer.exe'),
    bytes: 4,
    sha256: '0' * 64,
    notes: const {},
  );

  @override
  Future<File> download(UpdateRelease release, void Function(double) progress) async {
    downloads++;
    progress(.25);
    return _download.future;
  }

  void completeDownload() {
    _folder = Directory.systemTemp.createTempSync('consolecrypt-notice-test-');
    final file = File('${_folder!.path}/installer')..writeAsBytesSync([0, 1, 2, 3]);
    _download.complete(file);
  }

  @override
  Future<UpdateInstallResult> install(
    File file,
    UpdateRelease release, {
    String? macosSaveTitle,
    String? macosSavePrompt,
  }) async {
    installs++;
    return UpdateInstallResult.opened;
  }

  void dispose() => _folder?.deleteSync(recursive: true);
}
