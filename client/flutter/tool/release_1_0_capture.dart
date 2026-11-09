// Real production widgets with synthetic data only. No native core, accounts,
// credentials or user files are opened. Run explicitly; not a normal unit test.
// flutter test tool/release_1_0_capture.dart --dart-define=CC_GUIDE_LOCALE=ru
import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/ai/local/local_ai_hardware.dart';
import 'package:consolecrypt/ai/local/local_ai_runtime.dart';
import 'package:consolecrypt/ai/local_ai_providers.dart';
import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/mock/mock_docker.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/saved_forward.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/docker/docker_forwards.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/keepass/kdbx_documents.dart';
import 'package:consolecrypt/keepass/password_manager.dart';
import 'package:consolecrypt/tunnels/saved_forwards.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../test/helpers/test_app.dart';

const language = String.fromEnvironment('CC_GUIDE_LOCALE', defaultValue: 'ru');
const output = String.fromEnvironment('CC_GUIDE_OUTPUT', defaultValue: '../../.ci-cache/release-1.0-shots');
const captureCase = String.fromEnvironment('CC_GUIDE_CASE');

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    for (final family in [
      'Segoe UI',
      'Segoe UI Variable Text',
      'Segoe UI Variable Display',
      '.SF Pro Text',
      '.SF Pro Display',
      '.AppleSystemUIFont',
      'CupertinoSystemText',
      'CupertinoSystemDisplay',
      'Roboto',
      'Consolas',
      'Menlo',
      'monospace',
    ]) {
      await (FontLoader(family)..addFont(rootBundle.load('assets/fonts/Roboto-Regular.ttf'))).load();
    }
    await (FontLoader('MaterialIcons')..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'))).load();
  });
  testWidgets('capture the 1.0 password manager and saved forwards', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    const ru = language == 'ru';
    await backend.services.settings.updateLocal(
      backend.services.settings.currentLocal.copyWith(
        appLocale: ru ? AppLocale.ru : AppLocale.en,
        themeMode: AppThemeMode.dark,
      ),
    );
    final temp = (await tester.runAsync(() => Directory.systemTemp.createTemp('cc-guide-1.0-')))!;
    addTearDown(() => temp.delete(recursive: true));
    final docker = MockDockerService();
    final runtime = LocalAiRuntime(
      platform: LocalAiPlatform.linuxX64,
      supportDirectory: Directory('${temp.path}/local-ai'),
    );
    await tester.runAsync(runtime.initialize);
    addTearDown(runtime.dispose);
    final capture = GlobalKey();
    tester.view.physicalSize = const Size(1440, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      RepaintBoundary(
        key: capture,
        child: ColoredBox(
          color: const Color(0xff101a2d),
          child: ProviderScope(
            overrides: [
              appServicesProvider.overrideWithValue(backend.services),
              dockerServiceProvider.overrideWithValue(docker),
              deviceLocalAiAvailableProvider.overrideWithValue(true),
              localAiRuntimeProvider.overrideWithValue(runtime),
              localAiHardwareProvider.overrideWith(
                (ref) async =>
                    const LocalAiHardware(platform: LocalAiPlatform.linuxX64, totalMemoryBytes: 8 * 1024 * 1024 * 1024),
              ),
              aiServiceProvider.overrideWithValue(backend.services.ai),
              kdbxDocumentsProvider.overrideWithValue(KdbxDocuments(mobile: false)),
            ],
            child: const ConsoleCryptApp(),
          ),
        ),
      ),
    );
    await settle(tester);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final router = container.read(routerProvider);
    final manager = container.read(passwordManagerProvider);
    Future<void> shot(String name) async {
      await settle(tester);
      expect(tester.takeException(), isNull);
      if (captureCase.isNotEmpty && captureCase != name) return;
      final boundary = capture.currentContext!.findRenderObject()! as RenderRepaintBoundary;
      await tester.runAsync(() async {
        final image = await boundary.toImage();
        final data = await image.toByteData(format: ui.ImageByteFormat.png);
        await Directory(output).create(recursive: true);
        await File('$output/$name-$language.png').writeAsBytes(data!.buffer.asUint8List());
        image.dispose();
      });
    }

    Future<void> wait(Finder finder) async {
      for (var i = 0; i < 400 && finder.evaluate().isEmpty; i++) {
        await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 25)));
        await tester.pump(const Duration(milliseconds: 25));
      }
      expect(finder, findsOneWidget);
    }

    Future<void> escape() async {
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await settle(tester);
    }

    router.go(AppRoutes.passwords);
    await settle(tester);
    await shot('passwords-welcome');
    final session = (await tester.runAsync(
      () => manager.open(
        KdbxSource.local('${temp.path}/Team.kdbx'),
        password: SecretText('synthetic-guide-only'),
        create: true,
      ),
    ))!;
    const title = ru ? 'API сервиса' : 'Service API';
    const folderName = ru ? 'Работа' : 'Work';
    await tester.runAsync(() async {
      await manager.request(session, 'group', {'name': folderName});
      final group = session.rows('groups').firstWhere((g) => g['label'] == folderName)['id'];
      await manager.request(session, 'put', {
        'Title': title,
        'UserName': 'operator@example.test',
        'Password': 'synthetic-password-only',
        'URL': 'https://api.example.test',
        'Notes': ru ? 'Только демонстрационные данные' : 'Demonstration data only',
        'custom': {'API token': 'synthetic-token-only'},
        'group': group,
      });
      await manager.request(session, 'put', {
        'Title': ru ? 'Почта команды' : 'Team email',
        'UserName': 'team@example.test',
        'Password': 'synthetic-email-only',
        'URL': 'https://mail.example.test',
        'Notes': '',
        'group': session.data['root'],
      });
    });
    final id = session.rows('entries').firstWhere((e) => e['title'] == title)['id'];
    final entry = find.byKey(ValueKey('keepass-entry-$id'));
    await settle(tester);
    await tester.tap(entry);
    await shot('passwords-workspace');
    if (captureCase == 'passwords-open-only' || captureCase == 'passwords-locked-picker') {
      final closed = KdbxSource.local('${temp.path}/Archive.kdbx');
      await tester.runAsync(() async {
        await File(session.source.path).copy(closed.path);
        await manager.remember(closed);
      });
      await settle(tester);
      await shot('passwords-open-only');
      await tapKey(tester, 'keepass-unlock-others');
      await shot('passwords-locked-picker');
      expect(tester.takeException(), isNull);
      return;
    }
    final folder = session.rows('groups').firstWhere((g) => g['label'] == folderName)['id'];
    await tapKey(tester, 'keepass-group-menu-$folder');
    await shot('passwords-group-menu');
    await tapKey(tester, 'keepass-group-rename');
    await shot('passwords-group-rename');
    await tester.tap(find.text(ru ? 'Отмена' : 'Cancel').last);
    await settle(tester);
    await tapKey(tester, 'keepass-group-menu-$folder');
    await tapKey(tester, 'keepass-group-delete');
    await shot('passwords-group-delete');
    await tapKey(tester, 'confirm-cancel');

    await tester.tap(entry, buttons: kSecondaryMouseButton, kind: ui.PointerDeviceKind.mouse);
    await shot('passwords-entry-menu');
    await tapKey(tester, 'keepass-entry-move');
    await shot('passwords-move');
    await escape();
    await tapKey(tester, 'keepass-edit-entry');
    await wait(find.byKey(const ValueKey('keepass-add-field')));
    await shot('passwords-editor');
    await escape();
    await tapKey(tester, 'keepass-generator-open');
    await shot('passwords-generator');
    await escape();
    await tapKey(tester, 'keepass-add-database');
    await tapKey(tester, 'keepass-add-dav');
    await enterKey(tester, 'keepass-webdav-name', ru ? 'Общая база' : 'Shared database');
    await enterKey(tester, 'keepass-webdav-url', 'https://dav.example.test/passwords.kdbx');
    await enterKey(tester, 'keepass-webdav-username', 'demo@example.test');
    await shot('passwords-webdav');
    await tester.tap(find.text(ru ? 'Отмена' : 'Cancel').last);
    await settle(tester);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('palette-input')), '${ru ? 'пароли' : 'passwords'}: API');
    await shot('passwords-palette');
    tester.widget<TextField>(find.byKey(const ValueKey('palette-input'))).onSubmitted!(
      '${ru ? 'пароли' : 'passwords'}: API',
    );
    await wait(find.byKey(ValueKey('palette-password-${session.source.id}-$id-Password')));
    await shot('passwords-palette-fields');
    for (var i = 0; i < 4 && find.byKey(const ValueKey('palette-input')).evaluate().isNotEmpty; i++) {
      await escape();
    }
    final dockerSession = await docker.connect('local');
    final target = (await docker.containers(dockerSession.id)).firstWhere((c) => c.name == 'shop-postgres-1');
    final context = tester.element(find.byKey(const ValueKey('keepass-manager')));
    unawaited(showDockerForwardDialog(context, service: docker, session: dockerSession, container: target));
    await settle(tester);
    await tapKey(tester, 'docker-forward-save');
    await shot('tunnels-save-forward');
    await tester.tap(find.text(ru ? 'Отмена' : 'Cancel').last);
    await settle(tester);
    await container
        .read(savedForwardManagerProvider)
        .save(
          SavedForward(
            id: 'demo-forward',
            profile: manager.profile,
            kind: SavedForwardKind.docker,
            connectionId: 'local',
            target: 'shop-postgres-1',
            remotePort: 5432,
            localPort: 15432,
          ),
        );
    router.go(AppRoutes.tunnels);
    await shot('tunnels-saved');
    router.go(AppRoutes.credentials);
    await shot('credentials');
    debugDefaultTargetPlatformOverride = TargetPlatform.linux;
    router.go(AppRoutes.settings);
    await settle(tester);
    await tester.ensureVisible(find.byKey(const ValueKey('local-ai-settings')));
    await shot('local-ai-linux');
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    router.go(AppRoutes.passwords);
    await settle(tester);
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    tester.view.physicalSize = const Size(390, 844);
    await settle(tester);
    // The selected card is retained across navigation. Show the list first.
    final back = find.byKey(const ValueKey('keepass-back-entries'));
    if (back.evaluate().isNotEmpty) await tester.tap(back);
    await shot('passwords-phone');
    await tester.tap(find.byKey(ValueKey('keepass-entry-$id')));
    await shot('passwords-phone-card');
    await tapKey(tester, 'keepass-back-entries');
    await tapKey(tester, 'keepass-compact-groups');
    await tapKey(tester, 'keepass-compact-group-$folder');
    await tapKey(tester, 'keepass-compact-group-actions');
    await shot('passwords-phone-group-menu');
    await escape();
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    tester.view.physicalSize = const Size(1440, 900);
    await settle(tester);
    await tester.runAsync(() async {
      await manager.request(session, 'group', {'name': ru ? 'Удалённые сервисы' : 'Deleted services'});
    });
    final discarded = session.rows('groups').last['id'];
    await tester.runAsync(() async {
      await manager.request(session, 'deleteGroup', {'id': discarded});
      await manager.request(session, 'put', {
        'Title': ru ? 'Старый API-ключ' : 'Old API key',
        'UserName': 'demo-service',
        'Password': 'synthetic-retired-token',
        'URL': '',
        'Notes': '',
        'group': session.data['recycleBin'],
      });
    });
    final bin = session.data['recycleBin'];
    await tapKey(tester, 'keepass-group-$bin');
    await shot('passwords-trash');
    await tapKey(tester, 'keepass-empty-trash');
    await shot('passwords-empty-trash');
    await tapKey(tester, 'confirm-cancel');
    await tapKey(tester, 'keepass-group-menu-$discarded');
    await shot('passwords-trash-group-menu');
    await escape();
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    tester.view.physicalSize = const Size(390, 844);
    await settle(tester);
    await tapKey(tester, 'keepass-compact-groups');
    await tapKey(tester, 'keepass-compact-group-$bin');
    await shot('passwords-phone-trash');
    debugDefaultTargetPlatformOverride = TargetPlatform.windows;
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
