import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/rdp/rdp_fullscreen.dart';
import 'package:consolecrypt/rdp/rdp_providers.dart';
import 'package:consolecrypt/rdp/rdp_screen.dart';
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
    testWidgets('mobile RDP shell fits $size with keyboard and preserves input across rotation', (tester) async {
      if (_capture) await _loadFonts(tester);
      final backend = testBackend();
      addTearDown(backend.dispose);
      setTestLocale(backend, _captureLanguage == 'en' ? AppLocale.en : AppLocale.ru);
      await backend.debugSignInDemoAndUnlock();
      if (_capture) {
        await backend.settings.updateLocal(backend.settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
      }
      final service = FakeRdpService();
      if (_capture) {
        service.nextPoll = RdpPollResult(status: const RdpStatus(RdpPhase.connected), frame: _demoFrame());
      }
      tester.view.physicalSize = const Size(390, 844);
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
      tester.view.physicalSize = size;
      await settle(tester);
      expect(tester.takeException(), isNull, reason: 'connected workspace before keyboard');
      expect(find.byKey(const ValueKey('mobile-shell')), findsOneWidget);
      expect(find.byKey(const ValueKey('rdp-mobile-workspace')), findsOneWidget);
      expect(find.descendant(of: find.byType(RdpScreen), matching: find.byType(PageScaffold)), findsNothing);
      expect(find.byKey(const ValueKey('rdp-tab-rdp-1')), findsOneWidget);
      expect(find.byKey(const ValueKey('mobile-navigation')), findsOneWidget);
      expect(tester.widget<GlassIconButton>(find.byKey(const ValueKey('rdp-secure-attention'))).icon, Icons.security);
      final viewState = tester.state(find.byType(RdpView));
      await tapKey(tester, 'rdp-touchpad-toggle');
      if (_capture && size.width < size.height) await _captureWorkspace(tester, 'portrait-controls');
      await tapKey(tester, 'rdp-mobile-keyboard');
      expect(tester.testTextInput.hasAnyClients, isTrue);
      expect(tester.testTextInput.isVisible, isTrue);
      tester.view.viewInsets = const FakeViewPadding(bottom: 200);
      tester.view.padding = const FakeViewPadding(top: 24);
      await settle(tester);
      _expectKeyboardWorkspace(tester, size);
      if (_capture) {
        await _captureWorkspace(tester, size.width < size.height ? 'portrait-keyboard' : 'landscape-keyboard');
      }
      expect(tester.state(find.byType(RdpView)), same(viewState));
      tester.testTextInput.enterText('привет');
      await tester.pump();

      final rotated = Size(size.height, size.width);
      tester.view.physicalSize = rotated;
      await settle(tester);
      _expectKeyboardWorkspace(tester, rotated);
      expect(tester.state(find.byType(RdpView)), same(viewState));
      expect(controller.active, same(session));
      expect(service.disconnected, isEmpty);
      tester.testTextInput.enterText('hello');
      await tester.pump();
      expect(
        service.inputs.expand((batch) => batch).whereType<RdpUnicodeInput>().map((input) => input.text),
        containsAllInOrder(['привет', 'hello']),
      );

      await tapKey(tester, 'rdp-mobile-keyboard');
      tester.view.viewInsets = FakeViewPadding.zero;
      tester.view.padding = const FakeViewPadding(top: 24, bottom: 24);
      await settle(tester);
      expect(tester.takeException(), isNull, reason: 'restored workspace after keyboard');
      expect(find.byKey(const ValueKey('rdp-tab-rdp-1')), findsOneWidget);
      expect(find.byKey(const ValueKey('rdp-permissions')), findsOneWidget);
      expect(find.byKey(const ValueKey('mobile-navigation')), findsOneWidget);
      expect(tester.state(find.byType(RdpView)), same(viewState));
      expect(controller.active, same(session));
      expect(service.disconnected, isEmpty);
      await tester.pumpWidget(const SizedBox.shrink());
      await settle(tester);
      expect(service.disconnected, ['rdp-1']);
    }, variant: const TargetPlatformVariant({TargetPlatform.android, TargetPlatform.iOS}));

    testWidgets('mobile fullscreen RDP at $size keeps keyboard and touchpad working', (tester) async {
      final backend = testBackend();
      addTearDown(backend.dispose);
      setTestLocale(backend, AppLocale.ru);
      await backend.debugSignInDemoAndUnlock();
      final service = FakeRdpService();
      final window = _MobileWindow();
      tester.view.physicalSize = const Size(390, 844);
      tester.view.devicePixelRatio = 1;
      tester.view.padding = const FakeViewPadding(top: 24, bottom: 24);
      tester.view.viewPadding = const FakeViewPadding(top: 24, bottom: 24);
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        ProviderScope(
          retry: (_, _) => null,
          overrides: [
            appServicesProvider.overrideWithValue(backend.services),
            rdpServiceProvider.overrideWithValue(service),
            rdpWindowProvider.overrideWithValue(window),
          ],
          child: const ConsoleCryptApp(),
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
      tester.view.physicalSize = size;
      await settle(tester);
      await tapKey(tester, 'rdp-expand');
      expect(find.byType(RdpFullscreen), findsOneWidget);
      expect(find.byType(RdpView), findsOneWidget);
      expect(find.byKey(const ValueKey('mobile-navigation')), findsNothing);
      expect(find.byKey(const ValueKey('rdp-fullscreen-minimize')), findsNothing);
      expect(window.calls, ['enter']);
      final viewState = tester.state(find.byType(RdpView));
      await tapKey(tester, 'rdp-touchpad-toggle');
      await tapKey(tester, 'rdp-mobile-keyboard');
      tester.view.viewInsets = const FakeViewPadding(bottom: 200);
      tester.view.padding = const FakeViewPadding(top: 24);
      await settle(tester);
      expect(tester.takeException(), isNull, reason: 'fullscreen keyboard at $size');
      expect(tester.testTextInput.isVisible, isTrue);
      tester.testTextInput.enterText('полный экран');
      await tester.pump();

      // Allow the connection bar to hide, then rotate with the IME still open.
      await tester.pump(const Duration(seconds: 4));
      await settle(tester);
      final rotated = Size(size.height, size.width);
      tester.view.physicalSize = rotated;
      await settle(tester);
      expect(tester.takeException(), isNull, reason: 'fullscreen keyboard after rotation');
      expect(tester.state(find.byType(RdpView)), same(viewState));
      final surface = tester.getRect(find.byKey(const ValueKey('rdp-input-surface')));
      final controls = tester.getRect(find.byKey(const ValueKey('rdp-mobile-controls')));
      expect(surface.height, greaterThanOrEqualTo(80));
      expect(controls.bottom, lessThanOrEqualTo(rotated.height - 200));
      expect(find.byKey(const ValueKey('rdp-mobile-keyboard')).hitTestable(), findsOneWidget);
      service.inputs.clear();
      await tester.dragFrom(surface.bottomRight - const Offset(32, 24), const Offset(-28, -12));
      await tester.pump();
      expect(service.inputs.expand((batch) => batch).whereType<RdpPointerInput>(), isNotEmpty);
      await tapKey(tester, 'rdp-mobile-left-click');
      await tapKey(tester, 'rdp-mobile-right-click');
      expect(
        service.inputs
            .expand((batch) => batch)
            .whereType<RdpPointerInput>()
            .where((input) => input.down)
            .map((input) => input.button),
        containsAll([RdpPointerButton.left, RdpPointerButton.right]),
      );
      tester.testTextInput.enterText('hello');
      await tester.pump();
      expect(
        service.inputs.expand((batch) => batch).whereType<RdpUnicodeInput>().map((input) => input.text),
        contains('hello'),
      );
      expect(controller.active, same(session));
      expect(service.disconnected, isEmpty);

      // Android Back dismisses the keyboard before the next Back leaves the
      // route. The restore control must remain available after bar auto-hide.
      tester.view.viewInsets = FakeViewPadding.zero;
      tester.view.padding = const FakeViewPadding(top: 24, bottom: 24);
      await settle(tester);
      expect(tester.testTextInput.isVisible, isFalse);
      if (size.width < size.height) {
        await tapKey(tester, 'rdp-fullscreen-reveal');
        await tapKey(tester, 'rdp-fullscreen-restore');
      } else {
        await tester.binding.handlePopRoute();
        await settle(tester);
      }
      expect(tester.takeException(), isNull, reason: 'fullscreen exit restores shell');
      expect(find.byType(RdpFullscreen), findsNothing);
      expect(find.byKey(const ValueKey('rdp-mobile-workspace')), findsOneWidget);
      expect(window.calls, ['enter', 'leave']);
      expect(controller.active, same(session));
      expect(service.disconnected, isEmpty);
      await tester.pumpWidget(const SizedBox.shrink());
      await settle(tester);
      expect(service.disconnected, ['rdp-1']);
    }, variant: const TargetPlatformVariant({TargetPlatform.android, TargetPlatform.iOS}));
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

RdpFrame _demoFrame() {
  const width = 1280, height = 720;
  final pixels = Uint8List(width * height * 4);
  for (var y = 0; y < height; y++) {
    for (var x = 0; x < width; x++) {
      final color = y >= 674
          ? (22, 29, 43)
          : x >= 110 && x < 1170 && y >= 70 && y < 610
          ? y < 118
                ? (40, 49, 66)
                : x < 310
                ? (226, 231, 239)
                : x >= 352 && x < 1100 && y >= 164 && y < 550 && y % 72 < 35
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
  return RdpFrame(sequence: 1, width: width, height: height, rgba: pixels);
}

void _expectKeyboardWorkspace(WidgetTester tester, Size size) {
  expect(tester.takeException(), isNull, reason: 'RDP keyboard workspace at $size');
  expect(find.byKey(const ValueKey('rdp-tab-rdp-1')), findsNothing);
  expect(find.byKey(const ValueKey('rdp-permissions')), findsNothing);
  expect(find.byKey(const ValueKey('mobile-ai')), findsNothing);
  expect(find.byKey(const ValueKey('mobile-navigation')), findsNothing);
  final surface = tester.getRect(find.byKey(const ValueKey('rdp-input-surface')));
  final controls = tester.getRect(find.byKey(const ValueKey('rdp-mobile-controls')));
  expect(surface.top, greaterThanOrEqualTo(24));
  expect(surface.height, greaterThanOrEqualTo(80));
  expect(surface.bottom, lessThanOrEqualTo(controls.top));
  expect(controls.bottom, lessThanOrEqualTo(size.height - 200));
  expect(find.byKey(const ValueKey('rdp-mobile-keyboard')).hitTestable(), findsOneWidget);
  expect(find.byKey(const ValueKey('rdp-mobile-left-click')).hitTestable(), findsOneWidget);
  expect(find.byKey(const ValueKey('rdp-mobile-right-click')).hitTestable(), findsOneWidget);
}
