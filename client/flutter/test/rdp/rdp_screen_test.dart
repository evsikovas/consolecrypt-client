import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secure_clipboard.dart';
import 'package:consolecrypt/core/util/value_stream.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/rdp/rdp_launcher.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_screen.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import 'fake_rdp_service.dart';

class _RecordedClipboard extends SecureClipboard {
  int writes = 0, byteCount = 0;
  @override
  Future<void> copySecret(String value, {bool Function()? isCurrent}) async {
    if (isCurrent?.call() == false) return;
    writes++;
    byteCount = utf8.encode(value).length;
  }
}

class _Fixture {
  _Fixture() {
    profile = Profile(
      id: ProfileId.generate(),
      name: 'Synthetic workspace',
      kind: ProfileKind.local,
      createdAt: DateTime.now().toUtc(),
    );
    profiles = ValueStreamController(ProfilesState(profiles: [profile], activeId: profile.id));
  }
  late final Profile profile;
  late final ValueStreamController<ProfilesState> profiles;
  final status = ValueStreamController(const VaultStatus(phase: VaultPhase.unlocked));
  final service = FakeRdpService();
  final budget = GlassBackdropBudget();
  final clipboard = _RecordedClipboard();
  Host? savedHost;

  Future<void> pump(WidgetTester tester, {String language = 'en', Size size = const Size(1200, 900)}) async {
    tester.view.physicalSize = size;
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    addTearDown(profiles.close);
    addTearDown(status.close);
    addTearDown(budget.dispose);
    await tester.pumpWidget(
      ProviderScope(
        retry: (_, _) => null,
        overrides: [
          profilesProvider.overrideWith((_) => profiles.stream),
          vaultStatusProvider.overrideWith((_) => status.stream),
          rdpServiceProvider.overrideWithValue(service),
          secureClipboardProvider.overrideWithValue(clipboard),
        ],
        child: MaterialApp(
          locale: Locale(language),
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          theme: AppTheme.build(Brightness.dark, platform: TargetPlatform.windows),
          themeAnimationDuration: Duration.zero,
          builder: (_, child) => GlassScope(
            data: GlassScopeData(appearance: GlassAppearance.fallback, budget: budget),
            child: RepaintBoundary(key: const ValueKey('rdp-demo-surface'), child: child!),
          ),
          home: Scaffold(
            body: Column(
              children: [
                if (savedHost != null)
                  Consumer(
                    builder: (context, ref, _) => GlassButton(
                      key: const ValueKey('launch-saved'),
                      label: 'Saved test connection',
                      onPressed: () => RdpLauncher.openSavedHost(context, ref, savedHost!).ignore(),
                    ),
                  ),
                const Expanded(child: RdpScreen()),
              ],
            ),
          ),
        ),
      ),
    );
    await settle(tester);
  }

  Future<void> dispose(WidgetTester tester) async {
    await tester.pumpWidget(const SizedBox.shrink());
    await settle(tester);
  }
}

Future<void> _fill(WidgetTester tester) async {
  await tapKey(tester, 'rdp-new-connection');
  await enterKey(tester, 'rdp-address', 'example.test');
  await enterKey(tester, 'rdp-username', 'demo');
  final runtimeValue = List.generate(24, (_) => Random.secure().nextInt(10)).join();
  await enterKey(tester, 'rdp-password', runtimeValue);
  await tapKey(tester, 'rdp-connect-submit');
}

Future<void> _approve(WidgetTester tester) async {
  await tapKey(tester, 'rdp-certificate-checked');
  await tapKey(tester, 'rdp-certificate-accept');
}

Future<void> _pasteShortcut(WidgetTester tester) async {
  await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
  await tester.pump();
  final modifier = defaultTargetPlatform == TargetPlatform.macOS
      ? LogicalKeyboardKey.metaLeft
      : LogicalKeyboardKey.controlLeft;
  await tester.sendKeyDownEvent(modifier);
  await tester.sendKeyEvent(LogicalKeyboardKey.keyV);
  await tester.sendKeyUpEvent(modifier);
}

void _mockLocalClipboard(Future<Object?> Function() read) {
  final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  messenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
    if (call.method == 'Clipboard.getData') return read();
    return null;
  });
  addTearDown(() => messenger.setMockMethodCallHandler(SystemChannels.platform, null));
}

Future<void> _enableClipboard(WidgetTester tester) async {
  await tapKey(tester, 'rdp-permissions');
  await tapKey(tester, 'rdp-allow-clipboard');
  await tapKey(tester, 'rdp-permissions-apply');
}

Future<void> _connectedWithClipboard(WidgetTester tester, _Fixture fixture) async {
  await fixture.pump(tester);
  await _fill(tester);
  await _approve(tester);
  await _enableClipboard(tester);
}

const _guideCapture = bool.fromEnvironment('CC_RDP_GUIDE_CAPTURE');
const _guideOutput = String.fromEnvironment('CC_RDP_GUIDE_OUTPUT', defaultValue: '../../.local/verification/rdp-guide');
Future<void> _loadGuideFonts(WidgetTester tester) async {
  await tester.runAsync(() async {
    for (final family in [
      '.SF Pro Text',
      '.SF Pro Display',
      '.AppleSystemUIFont',
      'Roboto',
      'Segoe UI Variable Text',
      'Segoe UI Variable Display',
      'Segoe UI',
    ]) {
      final loader = FontLoader(family)
        ..addFont(File('/System/Library/Fonts/SFNS.ttf').readAsBytes().then(ByteData.sublistView));
      await loader.load();
    }
    final icons = FontLoader('MaterialIcons')..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'));
    await icons.load();
  });
}

Future<void> _captureGuide(WidgetTester tester, String name) async {
  final boundary = tester.renderObject<RenderRepaintBoundary>(find.byKey(const ValueKey('rdp-demo-surface')));
  await tester.runAsync(() async {
    final image = await boundary.toImage();
    try {
      final png = await image.toByteData(format: ui.ImageByteFormat.png);
      if (png == null) throw StateError('demo_capture_unavailable');
      await Directory(_guideOutput).create(recursive: true);
      await File('$_guideOutput/$name.png').writeAsBytes(png.buffer.asUint8List());
    } finally {
      image.dispose();
    }
  });
}

void main() {
  testWidgets('allowed RDP paste shortcut reads fresh local text without first clicking Send', (tester) async {
    final fixture = _Fixture();
    var reads = 0;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.getData') {
          reads++;
          return {'text': 'Fresh demo / Привет'};
        }
        return null;
      },
    );
    addTearDown(
      () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-allow-clipboard');
    await tapKey(tester, 'rdp-permissions-apply');
    await _pasteShortcut(tester);
    await settle(tester);
    expect(reads, 1);
    expect(fixture.service.clipboardOffers, ['Fresh demo / Привет']);
    expect(fixture.service.pasteCommits, [('rdp-1', 'opaque-confirmed-1')]);
    expect(fixture.service.clipboardRequests, 0);
    await fixture.dispose(tester);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.macOS}));

  testWidgets('disabled clipboard exchange keeps remote Ctrl V and never reads local clipboard', (tester) async {
    final fixture = _Fixture();
    var reads = 0;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.getData') reads++;
        return null;
      },
    );
    addTearDown(
      () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    fixture.service.inputs.clear();
    await _pasteShortcut(tester);
    await settle(tester);
    expect(reads, 0);
    expect(fixture.service.clipboardOffers, isEmpty);
    expect(
      fixture.service.inputs
          .expand((batch) => batch)
          .whereType<RdpScancodeInput>()
          .map((key) => (key.code, key.down))
          .toList(),
      [(0x1d, true), (0x2f, true), (0x2f, false), (0x1d, false)],
    );
    await fixture.dispose(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('RDP paste waits for exact ACK and commits once before later typing', (tester) async {
    final fixture = _Fixture();
    var reads = 0;
    _mockLocalClipboard(() async {
      reads++;
      return {'text': reads == 1 ? 'First demo' : 'Second demo'};
    });
    await _connectedWithClipboard(tester, fixture);
    fixture.service.confirmedOfferGate = Completer<String>();
    await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
    await tester.pump();
    fixture.service.inputs.clear();
    fixture.service.callOrder.clear();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    expect(await tester.sendKeyDownEvent(LogicalKeyboardKey.keyV), isTrue);
    expect(await tester.sendKeyRepeatEvent(LogicalKeyboardKey.keyV), isTrue);
    expect(await tester.sendKeyRepeatEvent(LogicalKeyboardKey.keyV), isTrue);
    expect(await tester.sendKeyUpEvent(LogicalKeyboardKey.keyV), isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(reads, 1);
    expect(fixture.service.pasteCommits, isEmpty);
    expect(fixture.service.inputs.expand((batch) => batch).whereType<RdpScancodeInput>().map((k) => (k.code, k.down)), [
      (0x1d, true),
      (0x1d, false),
    ]);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyX, character: 'x');
    await tester.pump();
    expect(fixture.service.inputs.expand((batch) => batch).whereType<RdpUnicodeInput>(), isEmpty);
    fixture.service.confirmedOfferGate!.complete('exact-ack');
    await settle(tester);
    expect(fixture.service.pasteCommits, [('rdp-1', 'exact-ack')]);
    expect(fixture.service.callOrder, ['input', 'input', 'input', 'confirmed-offer', 'commit', 'input']);
    expect(fixture.service.inputs.expand((batch) => batch).whereType<RdpUnicodeInput>().single.text, 'x');
    // A subsequent explicit shortcut reads the new clipboard rather than
    // silently reusing the first offer or its single-use ticket.
    fixture.service.confirmedOfferGate = null;
    await _pasteShortcut(tester);
    await settle(tester);
    expect(reads, 2);
    expect(fixture.service.clipboardOffers, ['First demo', 'Second demo']);
    expect(fixture.service.pasteCommits, [('rdp-1', 'exact-ack'), ('rdp-1', 'opaque-confirmed-2')]);
    await fixture.dispose(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('RDP clipboard read waits behind already queued modifier writes', (tester) async {
    final fixture = _Fixture();
    var reads = 0;
    _mockLocalClipboard(() async {
      reads++;
      return {'text': 'Demo'};
    });
    await _connectedWithClipboard(tester, fixture);
    await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
    await settle(tester);
    fixture.service.inputGate = Completer<void>();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.keyV);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(reads, 0);
    expect(fixture.service.pasteCommits, isEmpty);
    fixture.service.inputGate!.complete();
    await settle(tester);
    expect(reads, 1);
    expect(fixture.service.pasteCommits, hasLength(1));
    await fixture.dispose(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final cancellation in ['focus', 'tab']) {
    testWidgets('priority release cancels a confirmed paste before held commit dispatch on $cancellation', (
      tester,
    ) async {
      final fixture = _Fixture();
      _mockLocalClipboard(() async => {'text': 'Demo'});
      await _connectedWithClipboard(tester, fixture);
      if (cancellation == 'tab') {
        await _fill(tester);
        await _approve(tester);
        await _enableClipboard(tester);
      }
      final commit = Completer<void>();
      fixture.service.pasteCommitGate = commit;
      try {
        await _pasteShortcut(tester);
        await settle(tester);
        expect(fixture.service.pasteCommits, hasLength(1));
        final releases = fixture.service.inputs.expand((batch) => batch).whereType<RdpReleaseAllInput>().length;
        await tester.sendKeyEvent(LogicalKeyboardKey.keyX, character: 'x');
        await tester.pump();
        if (cancellation == 'focus') {
          FocusManager.instance.primaryFocus!.unfocus();
        } else {
          // Controller transition must invalidate synchronously, even before
          // Flutter disposes the previous view on the following frame.
          final container = ProviderScope.containerOf(tester.element(find.byType(RdpScreen)));
          final controller = container.read(rdpWorkspaceProvider);
          controller.activate(controller.tabs.first);
        }
        await tester.pump();
        expect(
          fixture.service.inputs.expand((batch) => batch).whereType<RdpReleaseAllInput>().length,
          greaterThan(releases),
        );
        expect(commit.isCompleted, isFalse);
        commit.complete();
        await settle(tester);
        expect(fixture.service.inputs.expand((batch) => batch).whereType<RdpUnicodeInput>(), isEmpty);
        if (cancellation == 'focus') {
          await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
          await tester.pump();
          await tester.sendKeyEvent(LogicalKeyboardKey.keyF, character: 'f');
          await settle(tester);
          expect(fixture.service.inputs.expand((batch) => batch).whereType<RdpUnicodeInput>().single.text, 'f');
        }
      } finally {
        if (!commit.isCompleted) commit.complete();
        await fixture.dispose(tester);
      }
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }

  testWidgets('a local read cannot resume after switching away and back without an intervening frame', (tester) async {
    final fixture = _Fixture();
    final read = Completer<Object?>();
    _mockLocalClipboard(() => read.future);
    await _connectedWithClipboard(tester, fixture);
    await _fill(tester);
    await _approve(tester);
    await _enableClipboard(tester);
    final controller = ProviderScope.containerOf(tester.element(find.byType(RdpScreen))).read(rdpWorkspaceProvider);
    final original = controller.active!;
    try {
      await _pasteShortcut(tester);
      await settle(tester);
      controller.activate(controller.tabs.first);
      controller.activate(original);
      // No old-view dispose or focus listener has run between transitions.
      read.complete({'text': 'Demo'});
      await settle(tester);
      expect(fixture.service.clipboardOffers, isEmpty);
      expect(fixture.service.pasteCommits, isEmpty);
      expect(original.clipboardPastePending, isFalse);
    } finally {
      if (!read.isCompleted) read.complete({'text': 'Demo'});
      await fixture.dispose(tester);
    }
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('a second distinct shortcut while ACK is pending does not cancel the first paste', (tester) async {
    final fixture = _Fixture();
    var reads = 0;
    _mockLocalClipboard(() async {
      reads++;
      return {'text': 'Demo'};
    });
    await _connectedWithClipboard(tester, fixture);
    final ack = Completer<String>();
    fixture.service.confirmedOfferGate = ack;
    fixture.service.inputs.clear();
    try {
      await _pasteShortcut(tester);
      await settle(tester);
      await _pasteShortcut(tester);
      await settle(tester);
      expect(reads, 1);
      ack.complete('one-ack');
      await settle(tester);
      expect(fixture.service.pasteCommits, hasLength(1));
      expect(fixture.service.inputs.expand((batch) => batch).whereType<RdpReleaseAllInput>(), hasLength(1));
    } finally {
      if (!ack.isCompleted) ack.complete('one-ack');
      await fixture.dispose(tester);
    }
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final cancellation in ['tab', 'lock', 'profile', 'focus', 'permission']) {
    testWidgets('late clipboard ACK cannot paste after $cancellation change', (tester) async {
      final fixture = _Fixture();
      _mockLocalClipboard(() async => {'text': 'Demo'});
      await _connectedWithClipboard(tester, fixture);
      if (cancellation == 'tab') {
        await _fill(tester);
        await _approve(tester);
        await _enableClipboard(tester);
      }
      final container = ProviderScope.containerOf(tester.element(find.byType(RdpScreen)));
      final controller = container.read(rdpWorkspaceProvider);
      final tab = controller.active!;
      fixture.service.confirmedOfferGate = Completer<String>();
      await _pasteShortcut(tester);
      await settle(tester);
      expect(fixture.service.clipboardOffers, ['Demo']);
      switch (cancellation) {
        case 'tab':
          await tester.tap(find.byKey(const ValueKey('rdp-tab-rdp-1')));
          await settle(tester);
        case 'lock':
          fixture.status.value = const VaultStatus(phase: VaultPhase.locked);
          await tester.pump();
          fixture.status.value = const VaultStatus(phase: VaultPhase.unlocked);
          await settle(tester);
        case 'profile':
          final next = Profile(
            id: ProfileId.generate(),
            name: 'Other synthetic workspace',
            kind: ProfileKind.local,
            createdAt: DateTime.now().toUtc(),
          );
          fixture.profiles.value = ProfilesState(profiles: [next], activeId: next.id);
          await settle(tester);
        case 'focus':
          FocusManager.instance.primaryFocus!.unfocus();
          await tester.pump();
          await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
          await tester.pump();
        case 'permission':
          // Even a revoke/re-enable without a rendered intermediate disabled
          // state invalidates the captured permission epoch.
          await controller.setPermissions(tab, const RdpSessionPermissions());
          await controller.setPermissions(tab, const RdpSessionPermissions(clipboardEnabled: true));
          await tester.pump();
      }
      fixture.service.confirmedOfferGate!.complete('late-ack');
      await settle(tester);
      expect(fixture.service.pasteCommits, isEmpty);
      expect(tab.clipboardPastePending, isFalse);
      await fixture.dispose(tester);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }

  testWidgets('a late local read after lock never offers its text', (tester) async {
    final fixture = _Fixture();
    final read = Completer<Object?>();
    _mockLocalClipboard(() => read.future);
    await _connectedWithClipboard(tester, fixture);
    await _pasteShortcut(tester);
    await settle(tester);
    fixture.status.value = const VaultStatus(phase: VaultPhase.locked);
    await tester.pump();
    read.complete({'text': 'Demo'});
    await settle(tester);
    expect(fixture.service.clipboardOffers, isEmpty);
    expect(fixture.service.pasteCommits, isEmpty);
    await fixture.dispose(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final invalid in ['empty', 'limit', 'utf8', 'nul', 'refused']) {
    testWidgets('RDP local clipboard $invalid consumes shortcut without pasting old remote text', (tester) async {
      final fixture = _Fixture();
      _mockLocalClipboard(
        () async => {
          'text': switch (invalid) {
            'empty' => '',
            'limit' => 'x' * 65537,
            'utf8' => 'я' * 32769,
            'nul' => 'demo\x00text',
            _ => 'Demo',
          },
        },
      );
      await _connectedWithClipboard(tester, fixture);
      if (invalid == 'refused') fixture.service.confirmedOfferFailure = const RdpFailure('clipboard_unavailable');
      fixture.service.inputs.clear();
      await _pasteShortcut(tester);
      await settle(tester);
      expect(fixture.service.pasteCommits, isEmpty);
      expect(fixture.service.clipboardOffers.length, invalid == 'refused' ? 1 : 0);
      expect(
        fixture.service.inputs.expand((batch) => batch).whereType<RdpScancodeInput>().any((k) => k.code == 0x2f),
        isFalse,
      );
      final context = tester.element(find.byType(RdpScreen));
      final label = invalid == 'empty'
          ? context.l10n.rdpClipboardEmpty
          : invalid == 'refused'
          ? context.l10n.rdpClipboardUnavailable
          : context.l10n.rdpClipboardLimit;
      expect(find.text(label), findsOneWidget);
      expect(ProviderScope.containerOf(context).read(rdpWorkspaceProvider).active!.status.phase, RdpPhase.connected);
      await fixture.dispose(tester);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }

  if (_guideCapture) {
    for (final language in ['ru', 'en']) {
      testWidgets('demo RDP guide tabs and permissions $language', (tester) async {
        final fixture = _Fixture();
        await _loadGuideFonts(tester);
        await fixture.pump(tester, language: language, size: const Size(1280, 860));
        for (var i = 0; i < 2; i++) {
          await _fill(tester);
          await _approve(tester);
        }
        expect(tester.widget<GlassTabStrip>(find.byType(GlassTabStrip)).tabs.length, 2);
        await _captureGuide(tester, 'rdp-tabs-$language');
        await tapKey(tester, 'rdp-permissions');
        expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('rdp-allow-clipboard'))).value, isFalse);
        fixture.service.pickedDirectory = const RdpDirectoryGrant(id: 'demo-grant', name: 'Demo documents');
        await tapKey(tester, 'rdp-pick-folder');
        expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('rdp-allow-folder-write'))).value, isFalse);
        expect(find.byKey(const ValueKey('rdp-folder-windows-path')).hitTestable(), findsOneWidget);
        expect(find.text(tester.element(find.byType(RdpScreen)).l10n.rdpFolderLimits).hitTestable(), findsOneWidget);
        expect(find.byKey(const ValueKey('rdp-password')), findsNothing);
        expect(tester.takeException(), isNull);
        await _captureGuide(tester, 'rdp-permissions-$language');
        await fixture.dispose(tester);
      }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
      testWidgets('demo saved RDP host editor guide $language', (tester) async {
        final backend = testBackend(seed: false);
        addTearDown(backend.dispose);
        await _loadGuideFonts(tester);
        await backend.debugCreateUnlockedLocalProfile(name: 'RDP demo');
        setTestLocale(backend, language == 'ru' ? AppLocale.ru : AppLocale.en);
        await backend.settings.updateLocal(backend.settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
        tester.view.physicalSize = const Size(1280, 860);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.reset);
        await tester.pumpWidget(
          ProviderScope(
            overrides: [appServicesProvider.overrideWithValue(backend.services)],
            retry: (_, _) => null,
            child: const RepaintBoundary(key: ValueKey('rdp-demo-surface'), child: ConsoleCryptApp()),
          ),
        );
        await settle(tester);
        await tapKey(tester, 'add-host');
        await tester.tap(find.descendant(of: find.byKey(const ValueKey('host-protocol')), matching: find.text('RDP')));
        await settle(tester);
        await enterKey(tester, 'host-name', 'Windows demo');
        await enterKey(tester, 'host-address', 'windows.example.test');
        await enterKey(tester, 'host-username', 'demo');
        await enterKey(tester, 'host-rdp-domain', 'DEMO');
        expect(find.byKey(const ValueKey('jump-mode')), findsNothing);
        expect(find.byKey(const ValueKey('host-rdp-domain')), findsOneWidget);
        FocusManager.instance.primaryFocus?.unfocus();
        Scrollable.of(tester.element(find.byKey(const ValueKey('host-name')))).position.jumpTo(0);
        await settle(tester);
        expect(tester.takeException(), isNull);
        await _captureGuide(tester, 'rdp-host-editor-$language');
        await tester.pumpWidget(const SizedBox.shrink());
        await settle(tester);
      }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
    }
  }

  test('RDP route uses the same unlocked-vault gate', () {
    expect(redirectFor(AppStage.locked, AppRoutes.rdp), AppRoutes.unlock);
    expect(redirectFor(AppStage.unlocked, AppRoutes.rdp), isNull);
    expect(redirectFor(AppStage.needsVault, AppRoutes.rdp), AppRoutes.onboarding);
  });

  testWidgets('certificate is full length and no authentication occurs before explicit comparison', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    expect(fixture.service.probeCount, 1);
    expect(fixture.service.options, isEmpty);
    final password = tester.widget<SecretField>(find.byKey(const ValueKey('rdp-password')));
    expect(password.controller.text.isEmpty, isTrue);
    final fingerprint = tester.widget<SelectableText>(find.byKey(const ValueKey('rdp-certificate-fingerprint')));
    expect(fingerprint.data!.split(':'), hasLength(32));
    expect(isEnabled(tester, 'rdp-certificate-accept'), isFalse);
    await tapKey(tester, 'rdp-certificate-cancel');
    expect(fixture.service.options, isEmpty);
    await fixture.dispose(tester);
  });

  testWidgets('successful login adopts a tab, exact pin, and wipes native credential borrow', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    expect(fixture.service.options, hasLength(1));
    expect(fixture.service.pins, [fixture.service.fingerprint]);
    expect(fixture.service.borrowedPassword!.every((byte) => byte == 0), isTrue);
    expect(find.byKey(const ValueKey('rdp-tab-rdp-1')), findsOneWidget);
    await tester.tap(
      find.descendant(of: find.byKey(const ValueKey('rdp-tab-rdp-1')), matching: find.byType(GlassIconButton)),
    );
    await settle(tester);
    expect(fixture.service.disconnected, ['rdp-1']);
    await fixture.dispose(tester);
  });

  testWidgets('lock/unlock removes both nested trust routes and cannot reuse old decision', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    fixture.status.value = const VaultStatus(phase: VaultPhase.locked);
    await tester.pump();
    fixture.status.value = const VaultStatus(phase: VaultPhase.unlocked);
    await settle(tester);
    expect(find.byKey(const ValueKey('rdp-certificate-fingerprint')), findsNothing);
    expect(find.byKey(const ValueKey('rdp-connect-submit')), findsNothing);
    expect(fixture.service.options, isEmpty);
    await fixture.dispose(tester);
  });

  testWidgets('delayed authentication after profile switch is disconnected and password wiped', (tester) async {
    final fixture = _Fixture();
    fixture.service.connectGate = Completer();
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    expect(fixture.service.borrowedPassword!.any((byte) => byte != 0), isTrue);
    final next = Profile(
      id: ProfileId.generate(),
      name: 'Other synthetic workspace',
      kind: ProfileKind.local,
      createdAt: DateTime.now().toUtc(),
    );
    fixture.profiles.value = ProfilesState(profiles: [next], activeId: next.id);
    await settle(tester);
    fixture.service.connectGate!.complete(RdpSessionInfo(id: 'late', width: 1280, height: 720));
    await settle(tester);
    expect(fixture.service.disconnected, ['late']);
    expect(fixture.service.borrowedPassword!.every((byte) => byte == 0), isTrue);
    expect(find.byKey(const ValueKey('rdp-tab-late')), findsNothing);
    await fixture.dispose(tester);
  });

  testWidgets('a probe for a different endpoint cannot authorize credential dispatch', (tester) async {
    final fixture = _Fixture();
    fixture.service.probeGate = Completer();
    await fixture.pump(tester);
    await _fill(tester);
    fixture.service.probeGate!.complete(
      RdpCertificate(address: 'other.example.test', port: 3389, sha256: fixture.service.fingerprint),
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('rdp-certificate-fingerprint')), findsNothing);
    expect(fixture.service.options, isEmpty);
    await fixture.dispose(tester);
  });

  testWidgets('cancel during probe cannot reopen trust dialog during exit animation', (tester) async {
    final fixture = _Fixture();
    fixture.service.probeGate = Completer();
    await fixture.pump(tester);
    await _fill(tester);
    await tester.tap(find.byKey(const ValueKey('rdp-connect-cancel')));
    fixture.service.probeGate!.complete(
      RdpCertificate(address: 'example.test', port: 3389, sha256: fixture.service.fingerprint),
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('rdp-certificate-fingerprint')), findsNothing);
    expect(fixture.service.options, isEmpty);
    await fixture.dispose(tester);
  });

  testWidgets('compact common tabs retain independent close controls and four-session cap', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester, size: const Size(600, 880));
    for (var i = 0; i < 4; i++) {
      await _fill(tester);
      await _approve(tester);
    }
    expect(find.byType(GlassTabStrip), findsOneWidget);
    final strip = tester.widget<GlassTabStrip>(find.byType(GlassTabStrip));
    expect(strip.tabs.length, 4);
    expect(strip.onAdd, isNull);
    strip.onClose!(1);
    await settle(tester);
    expect(fixture.service.disconnected, ['rdp-2']);
    expect(tester.widget<GlassTabStrip>(find.byType(GlassTabStrip)).tabs.length, 3);
    expect(tester.takeException(), isNull);
    await fixture.dispose(tester);
  });

  testWidgets('clicking parallel tabs routes Unicode only to the selected desktop', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    await _fill(tester);
    await _approve(tester);
    await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
    await tester.pump();
    tester.testTextInput.enterText('привет');
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('rdp-tab-rdp-1')));
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('rdp-input-surface')));
    await tester.pump();
    tester.testTextInput.enterText('hello');
    await tester.pump();
    final destinations = <String>[];
    for (var i = 0; i < fixture.service.inputs.length; i++) {
      if (fixture.service.inputs[i].any((event) => event is RdpUnicodeInput)) {
        destinations.add(fixture.service.inputSessionIds[i]);
      }
    }
    expect(destinations, ['rdp-2', 'rdp-1']);
    expect(tester.takeException(), isNull);
    await fixture.dispose(tester);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('clipboard is opt-in and toolbar Send never performs a paste', (tester) async {
    final fixture = _Fixture();
    var clipboardReads = 0;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.getData') {
          clipboardReads++;
          return {'text': 'Demo text'};
        }
        return null;
      },
    );
    addTearDown(
      () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    expect(fixture.service.currentPermissions.clipboardEnabled, isFalse);
    expect(isEnabled(tester, 'rdp-send-clipboard'), isFalse);
    expect(clipboardReads, 0);
    expect(fixture.service.clipboardRequests, 0);
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-allow-clipboard');
    await tapKey(tester, 'rdp-permissions-apply');
    expect(fixture.service.currentPermissions.clipboardEnabled, isTrue);
    expect(clipboardReads, 0);
    expect(fixture.service.clipboardRequests, 0);
    await tapKey(tester, 'rdp-send-clipboard');
    expect(fixture.service.pasteCommits, isEmpty);
    expect(clipboardReads, 1);
    expect(fixture.service.clipboardOffers.length, 1);
    fixture.service.remoteClipboard = 'Демонстрационный текст';
    await tapKey(tester, 'rdp-receive-clipboard');
    expect(fixture.service.clipboardRequests, 1);
    expect(fixture.clipboard.writes, 1);
    expect(fixture.clipboard.byteCount > 0, isTrue);
    await fixture.dispose(tester);
  });

  testWidgets('permissions require native acknowledgement and failure cannot claim clipboard enabled', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    fixture.service.permissionGate = Completer();
    fixture.service.permissionFailure = const RdpFailure('permissions');
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-allow-clipboard');
    await tapKey(tester, 'rdp-permissions-apply');
    expect(fixture.service.currentPermissions.clipboardEnabled, isFalse);
    expect(isEnabled(tester, 'rdp-permissions-cancel'), isFalse);
    fixture.service.permissionGate!.complete();
    await settle(tester);
    expect(fixture.service.currentPermissions.clipboardEnabled, isFalse);
    await tapKey(tester, 'rdp-permissions-cancel');
    expect(isEnabled(tester, 'rdp-send-clipboard'), isFalse);
    await fixture.dispose(tester);
  });

  testWidgets('clipboard revocation discards an in-flight remote response', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-allow-clipboard');
    await tapKey(tester, 'rdp-permissions-apply');
    fixture.service.clipboardGate = Completer();
    await tapKey(tester, 'rdp-receive-clipboard');
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-allow-clipboard');
    await tapKey(tester, 'rdp-permissions-apply');
    fixture.service.clipboardGate!.complete('Demo text');
    await settle(tester);
    expect(fixture.clipboard.writes, 0);
    expect(fixture.service.currentPermissions.clipboardEnabled, isFalse);
    await fixture.dispose(tester);
  });

  testWidgets('remote text above native clipboard limit is never copied', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-allow-clipboard');
    await tapKey(tester, 'rdp-permissions-apply');
    fixture.service.remoteClipboard = List.filled(40000, 'Ж').join();
    await tapKey(tester, 'rdp-receive-clipboard');
    expect(fixture.clipboard.writes, 0);
    await fixture.dispose(tester);
  });

  testWidgets('late received clipboard after lock is discarded', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-allow-clipboard');
    await tapKey(tester, 'rdp-permissions-apply');
    fixture.service.clipboardGate = Completer();
    await tapKey(tester, 'rdp-receive-clipboard');
    expect(fixture.service.clipboardTakes, 1);
    fixture.status.value = const VaultStatus(phase: VaultPhase.locked);
    await settle(tester);
    fixture.service.clipboardGate!.complete('Demo remote text');
    await settle(tester);
    expect(fixture.clipboard.writes, 0);
    await fixture.dispose(tester);
  });

  testWidgets('selected folder defaults read-only, requires explicit write and releases on close', (tester) async {
    final fixture = _Fixture();
    fixture.service.pickedDirectory = const RdpDirectoryGrant(id: 'opaque-demo-grant', name: 'Demo documents');
    await fixture.pump(tester);
    await tapKey(tester, 'rdp-new-connection');
    await tapKey(tester, 'rdp-pick-folder');
    final writable = tester.widget<CheckboxListTile>(find.byKey(const ValueKey('rdp-allow-folder-write')));
    expect(writable.value, isFalse);
    await tapKey(tester, 'rdp-allow-folder-write');
    await enterKey(tester, 'rdp-address', 'example.test');
    await enterKey(tester, 'rdp-username', 'demo');
    await enterKey(tester, 'rdp-password', List.generate(24, (_) => Random.secure().nextInt(10)).join());
    await tapKey(tester, 'rdp-connect-submit');
    await _approve(tester);
    expect(fixture.service.currentPermissions.directoryGrantId, 'opaque-demo-grant');
    expect(fixture.service.currentPermissions.directoryWritable, isTrue);
    expect(fixture.service.releasedGrants, isEmpty);
    final strip = tester.widget<GlassTabStrip>(find.byType(GlassTabStrip));
    strip.onClose!(0);
    await settle(tester);
    expect(fixture.service.releasedGrants, ['opaque-demo-grant']);
    await fixture.dispose(tester);
  });

  for (final language in ['ru', 'en']) {
    testWidgets('folder destination and limits are visible before connection in compact $language layout', (
      tester,
    ) async {
      final fixture = _Fixture();
      fixture.service.pickedDirectory = const RdpDirectoryGrant(id: 'demo-choice', name: 'Demo documents');
      await fixture.pump(tester, language: language, size: const Size(420, 880));
      await tapKey(tester, 'rdp-new-connection');
      await tapKey(tester, 'rdp-pick-folder');
      final l = tester.element(find.byType(RdpScreen)).l10n;
      expect(find.text(l.rdpFolderWindowsPath), findsOneWidget);
      expect(find.text(l.rdpFolderLimits), findsOneWidget);
      expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('rdp-allow-folder-write'))).value, isFalse);
      expect(tester.takeException(), isNull);
      await fixture.dispose(tester);
    });
  }

  testWidgets('folder replacement is readonly and removal revokes the active native capability', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    fixture.service.pickedDirectory = const RdpDirectoryGrant(id: 'folder-one', name: 'Demo one');
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-pick-folder');
    await tapKey(tester, 'rdp-allow-folder-write');
    await tapKey(tester, 'rdp-permissions-apply');
    expect(fixture.service.currentPermissions.directoryWritable, isTrue);
    fixture.service.pickedDirectory = const RdpDirectoryGrant(id: 'folder-two', name: 'Demo two');
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-pick-folder');
    expect(tester.widget<CheckboxListTile>(find.byKey(const ValueKey('rdp-allow-folder-write'))).value, isFalse);
    await tapKey(tester, 'rdp-permissions-apply');
    expect(fixture.service.currentPermissions.directoryGrantId, 'folder-two');
    expect(fixture.service.currentPermissions.directoryWritable, isFalse);
    expect(fixture.service.releasedGrants, ['folder-one']);
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-remove-folder');
    await tapKey(tester, 'rdp-permissions-apply');
    expect(fixture.service.currentPermissions.directoryGrantId, isNull);
    expect(fixture.service.currentPermissions.directoryWritable, isFalse);
    expect(fixture.service.releasedGrants, ['folder-one', 'folder-two']);
    expect(find.byKey(const ValueKey('rdp-folder-status')), findsNothing);
    await fixture.dispose(tester);
  });

  testWidgets('remote folder acceptance and rejection are distinct from platform support', (tester) async {
    final fixture = _Fixture();
    fixture.service.nextPoll = const RdpPollResult(
      status: RdpStatus(RdpPhase.connected),
      folderState: RdpFolderState.pending,
    );
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    fixture.service.pickedDirectory = const RdpDirectoryGrant(id: 'chosen-folder', name: 'Demo documents');
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-pick-folder');
    await tapKey(tester, 'rdp-permissions-apply');
    final l = tester.element(find.byType(RdpScreen)).l10n;
    expect(find.text(l.rdpFolderPending), findsOneWidget);
    expect(find.text(l.rdpFolderReady), findsNothing);
    fixture.service.nextPoll = const RdpPollResult(
      status: RdpStatus(RdpPhase.connected),
      folderState: RdpFolderState.ready,
    );
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pump();
    expect(find.text(l.rdpFolderReady), findsOneWidget);
    fixture.service.nextPoll = const RdpPollResult(
      status: RdpStatus(RdpPhase.connected),
      folderState: RdpFolderState.denied,
    );
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pump();
    expect(find.text(l.rdpFolderDenied), findsOneWidget);
    expect(find.text(l.rdpFolderReady), findsNothing);
    expect(isEnabled(tester, 'rdp-secure-attention'), isTrue);
    expect(fixture.service.disconnected, isEmpty);
    await fixture.dispose(tester);
  });

  testWidgets('native folder failure offers an action and successful retry removes the error', (tester) async {
    final fixture = _Fixture();
    fixture.service.pickerFailure = const RdpFailure('directory_grant_unavailable');
    await fixture.pump(tester);
    await tapKey(tester, 'rdp-new-connection');
    await tapKey(tester, 'rdp-pick-folder');
    final l = tester.element(find.byType(RdpScreen)).l10n;
    expect(find.text(l.rdpFolderChooseFailed), findsOneWidget);
    fixture.service.pickerFailure = null;
    fixture.service.pickedDirectory = const RdpDirectoryGrant(id: 'retry-folder', name: 'Demo documents');
    await tapKey(tester, 'rdp-pick-folder');
    expect(find.text(l.rdpFolderChooseFailed), findsNothing);
    expect(find.text(l.rdpFolderWindowsPath), findsOneWidget);
    await fixture.dispose(tester);
  });

  testWidgets('unsupported folder platform does not promise server redirection', (tester) async {
    final fixture = _Fixture();
    fixture.service.supported = const RdpCapabilities(clipboardSupported: true, folderSupported: false);
    await fixture.pump(tester);
    await tapKey(tester, 'rdp-new-connection');
    expect(isEnabled(tester, 'rdp-pick-folder'), isFalse);
    expect(find.text(tester.element(find.byType(RdpScreen)).l10n.rdpFolderUnsupported), findsOneWidget);
    expect(fixture.service.folderPicks, 0);
    await fixture.dispose(tester);
  });

  testWidgets('clipboard server refusal gives useful feedback without disconnecting', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-allow-clipboard');
    await tapKey(tester, 'rdp-permissions-apply');
    fixture.service.clipboardFailure = const RdpFailure('clipboard_unavailable');
    await tapKey(tester, 'rdp-receive-clipboard');
    final l = tester.element(find.byType(RdpScreen)).l10n;
    expect(find.text(l.rdpClipboardUnavailable), findsOneWidget);
    expect(isEnabled(tester, 'rdp-secure-attention'), isTrue);
    expect(fixture.clipboard.writes, 0);
    expect(fixture.service.disconnected, isEmpty);
    await fixture.dispose(tester);
  });

  testWidgets('cancelled or replaced folder choices release unused native capabilities', (tester) async {
    final fixture = _Fixture();
    fixture.service.pickedDirectory = const RdpDirectoryGrant(id: 'choice-1', name: 'Demo one');
    await fixture.pump(tester);
    await tapKey(tester, 'rdp-new-connection');
    await tapKey(tester, 'rdp-pick-folder');
    fixture.service.pickedDirectory = const RdpDirectoryGrant(id: 'choice-2', name: 'Demo two');
    await tapKey(tester, 'rdp-pick-folder');
    expect(fixture.service.releasedGrants, ['choice-1']);
    await tapKey(tester, 'rdp-connect-cancel');
    expect(fixture.service.releasedGrants, ['choice-1', 'choice-2']);
    await fixture.dispose(tester);
  });

  testWidgets('late folder choice after profile lock releases rather than attaching', (tester) async {
    final fixture = _Fixture();
    fixture.service.pickerGate = Completer();
    await fixture.pump(tester);
    await tapKey(tester, 'rdp-new-connection');
    await tapKey(tester, 'rdp-pick-folder');
    fixture.status.value = const VaultStatus(phase: VaultPhase.locked);
    await settle(tester);
    fixture.service.pickerGate!.complete(const RdpDirectoryGrant(id: 'late-choice', name: 'Demo folder'));
    await settle(tester);
    expect(fixture.service.releasedGrants, ['late-choice']);
    expect(find.byKey(const ValueKey('rdp-shared-folder-name')), findsNothing);
    await fixture.dispose(tester);
  });

  testWidgets('saved host uses fresh readonly ticket and native password without Dart exposure', (tester) async {
    final fixture = _Fixture();
    fixture.savedHost = Host.create(name: 'Old label', address: 'old.example.test');
    fixture.service.savedTicket = RdpSavedHostTicket(
      hostId: fixture.savedHost!.id.value,
      name: 'Fresh demo host',
      options: RdpConnectionOptions(address: 'fresh.example.test', port: 3390, username: 'fresh-demo', domain: 'DEMO'),
      certificate: RdpCertificate(
        address: 'fresh.example.test',
        port: 3390,
        sha256: fixture.service.fingerprint,
        snapshotStamp: 'opaque-native-snapshot',
      ),
      hasSavedPassword: true,
    );
    await fixture.pump(tester);
    await tapKey(tester, 'launch-saved');
    final address = tester.widget<TextField>(find.byKey(const ValueKey('rdp-address')));
    expect(address.readOnly, isTrue);
    expect(address.controller!.text, 'fresh.example.test');
    expect(find.byKey(const ValueKey('rdp-password')), findsNothing);
    await tapKey(tester, 'rdp-connect-submit');
    await _approve(tester);
    expect(fixture.service.usedTicket, same(fixture.service.savedTicket));
    expect(fixture.service.borrowedPassword, isNull);
    expect(fixture.service.probeCount, 0);
    expect(tester.widget<GlassTabStrip>(find.byType(GlassTabStrip)).tabs.single.title, 'Fresh demo host');
    await fixture.dispose(tester);
  });

  testWidgets('saved host without password accepts only a transient wiped native borrow', (tester) async {
    final fixture = _Fixture();
    fixture.savedHost = Host.create(name: 'Demo', address: 'old.example.test');
    fixture.service.savedTicket = RdpSavedHostTicket(
      hostId: fixture.savedHost!.id.value,
      name: 'Demo',
      options: RdpConnectionOptions(address: 'example.test', username: 'demo'),
      certificate: RdpCertificate(
        address: 'example.test',
        port: 3389,
        sha256: fixture.service.fingerprint,
        snapshotStamp: 'opaque-native-snapshot',
      ),
      hasSavedPassword: false,
    );
    await fixture.pump(tester);
    await tapKey(tester, 'launch-saved');
    await enterKey(tester, 'rdp-password', List.generate(24, (_) => Random.secure().nextInt(10)).join());
    await tapKey(tester, 'rdp-connect-submit');
    await _approve(tester);
    expect(fixture.service.usedTicket, same(fixture.service.savedTicket));
    expect(fixture.service.borrowedPassword!.every((byte) => byte == 0), isTrue);
    await fixture.dispose(tester);
  });

  testWidgets('lock during native permission apply releases picked capability after borrower settles', (tester) async {
    final fixture = _Fixture();
    await fixture.pump(tester);
    await _fill(tester);
    await _approve(tester);
    fixture.service.pickedDirectory = const RdpDirectoryGrant(id: 'pending-folder', name: 'Demo folder');
    fixture.service.permissionGate = Completer();
    await tapKey(tester, 'rdp-permissions');
    await tapKey(tester, 'rdp-pick-folder');
    await tapKey(tester, 'rdp-permissions-apply');
    fixture.status.value = const VaultStatus(phase: VaultPhase.locked);
    await settle(tester);
    expect(fixture.service.releasedGrants, isEmpty);
    fixture.service.permissionGate!.complete();
    await settle(tester);
    expect(fixture.service.releasedGrants, ['pending-folder']);
    expect(find.byKey(const ValueKey('rdp-permissions-apply')), findsNothing);
    await fixture.dispose(tester);
  });

  testWidgets('late saved-host probe after lock never opens trust or credential form', (tester) async {
    final fixture = _Fixture();
    fixture.savedHost = Host.create(name: 'Demo', address: 'example.test');
    fixture.service.savedProbeGate = Completer();
    await fixture.pump(tester);
    await tapKey(tester, 'launch-saved');
    fixture.status.value = const VaultStatus(phase: VaultPhase.locked);
    await settle(tester);
    fixture.service.savedProbeGate!.complete(
      RdpSavedHostTicket(
        hostId: fixture.savedHost!.id.value,
        name: 'Demo',
        options: RdpConnectionOptions(address: 'example.test', username: 'demo'),
        certificate: RdpCertificate(
          address: 'example.test',
          port: 3389,
          sha256: fixture.service.fingerprint,
          snapshotStamp: 'opaque-native-snapshot',
        ),
        hasSavedPassword: true,
      ),
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('rdp-connect-submit')), findsNothing);
    expect(fixture.service.options, isEmpty);
    await fixture.dispose(tester);
  });

  for (final language in ['ru', 'en']) {
    testWidgets('direct connection form fits compact $language layout without saved defaults', (tester) async {
      final fixture = _Fixture();
      await fixture.pump(tester, language: language, size: const Size(420, 880));
      await tapKey(tester, 'rdp-new-connection');
      final address = tester.widget<TextField>(find.byKey(const ValueKey('rdp-address')));
      expect(address.controller!.text.isEmpty, isTrue);
      expect(tester.takeException(), isNull);
      await fixture.dispose(tester);
    });
  }
}
