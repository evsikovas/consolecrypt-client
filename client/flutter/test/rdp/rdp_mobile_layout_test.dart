import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/rdp_mobile.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/rdp/rdp_fullscreen.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_service.dart';
import 'package:consolecrypt/rdp/rdp_view.dart';
import 'package:consolecrypt/rdp/rdp_window.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import 'fake_rdp_service.dart';

void main() {
  for (final size in [const Size(390, 844), const Size(844, 390)]) {
    for (final theme in [AppThemeMode.dark, AppThemeMode.light]) {
      testWidgets('phone fullscreen $size $theme keeps a stable desktop under the keyboard', (tester) async {
        if (_capture) await _loadFonts(tester);
        final backend = testBackend();
        addTearDown(backend.dispose);
        setTestLocale(backend, _captureLanguage == 'en' ? AppLocale.en : AppLocale.ru);
        await backend.debugSignInDemoAndUnlock();
        await backend.settings.updateLocal(
          backend.settings.currentLocal.copyWith(
            themeMode: theme,
            rdpMobile: const RdpMobileSettings(hideIntroduction: true),
          ),
        );
        final service = FakeRdpService();
        final window = _MobileWindow();
        if (_capture) {
          service.nextPoll = RdpPollResult(status: const RdpStatus(RdpPhase.connected), frame: _demoFrame());
        }
        tester.view.physicalSize = size;
        tester.view.devicePixelRatio = 1;
        tester.view.padding = const FakeViewPadding(top: 24, bottom: 24);
        tester.view.viewPadding = const FakeViewPadding(top: 24, bottom: 24);
        addTearDown(tester.view.reset);
        await tester.pumpWidget(
          ProviderScope(
            retry: (_, _) => null,
            overrides: [
              appServicesProvider.overrideWithValue(backend.services),
              if (_capture) developerControlsProvider.overrideWithValue(null),
              rdpServiceProvider.overrideWithValue(service),
              rdpWindowProvider.overrideWithValue(window),
            ],
            child: const RepaintBoundary(key: ValueKey('rdp-mobile-demo'), child: ConsoleCryptApp()),
          ),
        );
        await settle(tester);
        final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
        container.read(routerProvider).go(AppRoutes.rdp);
        final controller = container.read(rdpWorkspaceProvider);
        final session = await controller.connect(
          RdpConnectionOptions(address: 'example.test', username: 'demo'),
          Uint8List(0),
          service.fingerprint,
          isCurrent: () => true,
        );
        await settle(tester);
        expect(tester.takeException(), isNull);
        expect(find.byType(RdpFullscreen), findsOneWidget);
        expect(find.byType(RdpView), findsOneWidget);
        expect(find.byKey(const ValueKey('mobile-navigation')), findsNothing);
        expect(find.byKey(const ValueKey('rdp-tab-rdp-1')), findsNothing);
        expect(find.byKey(const ValueKey('rdp-mobile-controls')), findsNothing);
        expect(find.byKey(const ValueKey('rdp-mobile-menu')), findsOneWidget);
        final menuBounds = tester.getRect(find.byKey(const ValueKey('rdp-mobile-menu')));
        expect(menuBounds.width, closeTo(44, .001));
        expect(menuBounds.height, closeTo(44, .001));
        expect(menuBounds.right, lessThanOrEqualTo(size.width + .001));
        expect(window.calls, ['enter']);
        final viewState = tester.state(find.byType(RdpView));
        final before = tester.getSize(find.byKey(const ValueKey('rdp-input-surface')));
        expect(before, Size(size.width, size.height - 48));
        final requested = service.inputs.expand((batch) => batch).whereType<RdpResizeInput>();
        expect(requested, isNotEmpty);
        expect(requested.last.width / requested.last.height, closeTo(before.aspectRatio, .005));
        expect(requested.last.desktopScale, 125);
        if (_capture) {
          // Demonstrate an accepting server at the requested aspect ratio.
          // Pixels are synthetic; the surrounding controls are real widgets.
          service.nextPoll = RdpPollResult(
            status: const RdpStatus(RdpPhase.connected),
            frame: _demoFrame(width: requested.last.width, height: requested.last.height, sequence: 2),
          );
          await settle(tester);
        }
        final name = size.width < size.height ? 'portrait' : 'landscape';
        if (_capture) await _captureWorkspace(tester, '$name-${theme.name}');
        await tapKey(tester, 'rdp-mobile-menu');
        expect(find.byKey(const ValueKey('rdp-mobile-panel')), findsOneWidget);
        if (_capture) await _captureWorkspace(tester, '$name-controls-${theme.name}');
        await tapKey(tester, 'rdp-mobile-keyboard');
        expect(tester.testTextInput.isVisible, isTrue);
        tester.view.viewInsets = const FakeViewPadding(bottom: 200);
        tester.view.padding = const FakeViewPadding(top: 24);
        await settle(tester);
        expect(tester.takeException(), isNull);
        expect(tester.getSize(find.byKey(const ValueKey('rdp-input-surface'))), before);
        final strip = tester.getRect(find.byKey(const ValueKey('rdp-keyboard-strip')));
        expect(strip.bottom, lessThanOrEqualTo(size.height - 200));
        expect(strip.height, 40);
        if (_capture) await _captureWorkspace(tester, '$name-keyboard-${theme.name}');
        tester.testTextInput.enterText('привет');
        await tester.pump();
        tester.view.physicalSize = Size(size.height, size.width);
        await settle(tester);
        expect(tester.takeException(), isNull);
        expect(tester.state(find.byType(RdpView)), same(viewState));
        tester.testTextInput.enterText('hello');
        await tester.pump();
        expect(
          service.inputs.expand((b) => b).whereType<RdpUnicodeInput>().map((i) => i.text),
          containsAllInOrder(['привет', 'hello']),
        );
        tester.view.viewInsets = FakeViewPadding.zero;
        tester.view.padding = const FakeViewPadding(top: 24, bottom: 24);
        await settle(tester);
        expect(tester.testTextInput.isVisible, isFalse);
        await tester.binding.handlePopRoute();
        await tester.pump(const Duration(milliseconds: 100));
        expect(find.byKey(const ValueKey('rdp-mobile-panel')), findsOneWidget);
        await tester.binding.handlePopRoute();
        await settle(tester);
        expect(find.byType(RdpFullscreen), findsNothing);
        expect(find.byKey(const ValueKey('mobile-navigation')), findsOneWidget);
        expect(window.calls, ['enter', 'leave']);
        expect(controller.active, same(session));
        expect(service.disconnected, isEmpty);
        await tester.pumpWidget(const SizedBox.shrink());
        await settle(tester);
        expect(service.disconnected, ['rdp-1']);
      }, variant: const TargetPlatformVariant({TargetPlatform.android, TargetPlatform.iOS}));
    }
  }
}

class _MobileWindow extends RdpWindow {
  final calls = <String>[];

  @override
  bool get desktop => false;

  @override
  Future<void> enter() async => calls.add('enter');

  @override
  Future<void> leave() async => calls.add('leave');
}

const _capture = bool.fromEnvironment('CC_RDP_MOBILE_CAPTURE');
const _captureLanguage = String.fromEnvironment('CC_RDP_MOBILE_LOCALE', defaultValue: 'ru');
const _output = String.fromEnvironment('CC_RDP_MOBILE_OUTPUT', defaultValue: '/tmp/consolecrypt-rdp-mobile-review');

Future<void> _loadFonts(WidgetTester tester) => tester.runAsync(() async {
  final text = FontLoader('Roboto')
    ..addFont(File('/System/Library/Fonts/SFNS.ttf').readAsBytes().then(ByteData.sublistView));
  await text.load();
  final icons = FontLoader('MaterialIcons')..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'));
  await icons.load();
});

Future<void> _captureWorkspace(WidgetTester tester, String name) async {
  if (defaultTargetPlatform != TargetPlatform.android) return;
  // Native image decoding runs outside the fake test clock. No server or
  // operating-system keyboard is involved; the bottom inset remains reserved.
  await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 50)));
  await tester.pump();
  final boundary = tester.renderObject<RenderRepaintBoundary>(find.byKey(const ValueKey('rdp-mobile-demo')));
  await tester.runAsync(() async {
    final image = await boundary.toImage(pixelRatio: 2);
    try {
      final png = await image.toByteData(format: ui.ImageByteFormat.png);
      if (png == null) throw StateError('rdp_mobile_capture_unavailable');
      await Directory(_output).create(recursive: true);
      await File('$_output/mobile-rdp-$name-$_captureLanguage.png').writeAsBytes(png.buffer.asUint8List());
    } finally {
      image.dispose();
    }
  });
}

RdpFrame _demoFrame({int width = 1280, int height = 720, int sequence = 1}) {
  final pixels = Uint8List(width * height * 4);
  for (var y = 0; y < height; y++) {
    for (var x = 0; x < width; x++) {
      final dx = x * 1280 ~/ width, dy = y * 720 ~/ height;
      final color = dy >= 674
          ? (22, 29, 43)
          : dx >= 110 && dx < 1170 && dy >= 70 && dy < 610
          ? dy < 118
                ? (40, 49, 66)
                : dx < 310
                ? (226, 231, 239)
                : dx >= 352 && dx < 1100 && dy >= 164 && dy < 550 && dy % 72 < 35
                ? (215, 225, 240)
                : (246, 248, 252)
          : (24, 55 + x * 45 ~/ width, 106 + y * 65 ~/ height);
      final offset = (y * width + x) * 4;
      pixels[offset] = color.$1;
      pixels[offset + 1] = color.$2;
      pixels[offset + 2] = color.$3;
      pixels[offset + 3] = 255;
    }
  }
  return RdpFrame(sequence: sequence, width: width, height: height, rgba: pixels);
}
