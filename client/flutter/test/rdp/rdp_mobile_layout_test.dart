import 'dart:io';
import 'dart:math' as math;
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
          service.nextPoll = RdpPollResult(
            status: const RdpStatus(RdpPhase.connected),
            frame: await _windowsFrame(tester),
          );
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
            frame: await _windowsFrame(tester, width: requested.last.width, height: requested.last.height, sequence: 2),
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

/// A drawn Windows 11-like desktop (wallpaper, desktop icons, an Explorer
/// window and the taskbar) in the capture language. Pixels are synthetic;
/// no server is involved.
Future<RdpFrame> _windowsFrame(WidgetTester tester, {int width = 1280, int height = 720, int sequence = 1}) async {
  const ru = _captureLanguage != 'en';
  final bytes = await tester.runAsync(() async {
    final recorder = ui.PictureRecorder();
    final canvas = Canvas(recorder);
    final w = width.toDouble(), h = height.toDouble();
    final u = math.min(w, h) / 720 * 1.15;
    final portrait = h > w;

    void text(String value, Offset at, double size, Color color, {FontWeight weight = FontWeight.w400, double? max}) {
      final painter = TextPainter(
        text: TextSpan(
          text: value,
          style: TextStyle(fontFamily: 'Roboto', fontSize: size, color: color, fontWeight: weight),
        ),
        textDirection: TextDirection.ltr,
        maxLines: 1,
        ellipsis: '…',
      )..layout(maxWidth: max ?? double.infinity);
      painter.paint(canvas, at);
    }

    void icon(IconData data, Offset at, double size, Color color) {
      final painter = TextPainter(
        text: TextSpan(
          text: String.fromCharCode(data.codePoint),
          style: TextStyle(fontFamily: 'MaterialIcons', fontSize: size, color: color),
        ),
        textDirection: TextDirection.ltr,
      )..layout();
      painter.paint(canvas, at);
    }

    RRect box(double l, double t, double r, double b, double radius) =>
        RRect.fromLTRBR(l, t, r, b, Radius.circular(radius));

    // Wallpaper: deep blue with a soft "bloom".
    canvas.drawRect(
      Rect.fromLTWH(0, 0, w, h),
      Paint()
        ..shader = ui.Gradient.linear(
          Offset.zero,
          Offset(w, h),
          const [Color(0xff07193d), Color(0xff0d3b8a), Color(0xff1667c9)],
          const [0, .55, 1],
        ),
    );
    for (final (dx, dy, r, a) in [(.62, .55, .55, .55), (.75, .42, .35, .45), (.5, .68, .3, .35)]) {
      canvas.drawCircle(
        Offset(w * dx, h * dy),
        math.max(w, h) * r,
        Paint()
          ..shader = ui.Gradient.radial(Offset(w * dx, h * dy), math.max(w, h) * r, [
            Color.fromRGBO(90, 170, 255, a),
            const Color(0x0047a0ff),
          ]),
      );
    }

    final taskbar = 48 * u;
    // Desktop icons.
    for (final (i, (data, label)) in [
      (Icons.delete_outline_rounded, ru ? 'Корзина' : 'Recycle Bin'),
      (Icons.computer_rounded, ru ? 'Этот компьютер' : 'This PC'),
    ].indexed) {
      final top = 14 * u + i * 86 * u;
      icon(data, Offset(22 * u, top), 40 * u, Colors.white);
      text(label, Offset(6 * u, top + 44 * u), 10.5 * u, Colors.white, max: 100 * u);
    }

    // Explorer window.
    final left = portrait ? 12 * u : w * .14, right = portrait ? w - 12 * u : w * .86;
    final top = portrait ? 196 * u : h * .07,
        bottom = portrait ? math.min(h * .78, top + 620 * u) : h - taskbar - 22 * u;
    final window = box(left, top, right, bottom, 8 * u);
    canvas.drawShadow(Path()..addRRect(window), Colors.black, 18 * u, false);
    canvas.drawRRect(window, Paint()..color = const Color(0xfff3f3f3));
    canvas.save();
    canvas.clipRRect(window);
    const title = ru ? 'Документы' : 'Documents';
    canvas.drawRect(Rect.fromLTRB(left, top, right, top + 34 * u), Paint()..color = const Color(0xffe8edf4));
    icon(Icons.folder_rounded, Offset(left + 12 * u, top + 8 * u), 18 * u, const Color(0xffffc83d));
    text(title, Offset(left + 36 * u, top + 9 * u), 12 * u, const Color(0xff1b1b1b));
    for (final (i, data) in [Icons.close_rounded, Icons.crop_square_rounded, Icons.remove_rounded].indexed) {
      icon(data, Offset(right - (i + 1) * 40 * u + 12 * u, top + 8 * u), 16 * u, const Color(0xff333333));
    }
    // Toolbar.
    final bar = top + 34 * u;
    canvas.drawRect(Rect.fromLTRB(left, bar, right, bar + 40 * u), Paint()..color = const Color(0xfff9f9f9));
    icon(Icons.add_circle_outline_rounded, Offset(left + 12 * u, bar + 11 * u), 18 * u, const Color(0xff0067c0));
    text(ru ? 'Создать' : 'New', Offset(left + 34 * u, bar + 12 * u), 12 * u, const Color(0xff1b1b1b));
    var x = left + 100 * u;
    for (final data in [
      Icons.content_cut_rounded,
      Icons.content_copy_rounded,
      Icons.content_paste_rounded,
      Icons.drive_file_rename_outline_rounded,
      Icons.ios_share_rounded,
      Icons.delete_outline_rounded,
    ]) {
      if (x > right - 40 * u) break;
      icon(data, Offset(x, bar + 11 * u), 18 * u, const Color(0xff444444));
      x += 34 * u;
    }
    if (!portrait) {
      text(ru ? 'Сортировать' : 'Sort', Offset(x + 16 * u, bar + 12 * u), 12 * u, const Color(0xff1b1b1b));
      text(ru ? 'Просмотреть' : 'View', Offset(x + 110 * u, bar + 12 * u), 12 * u, const Color(0xff1b1b1b));
    }
    // Address and search.
    final address = bar + 40 * u;
    canvas.drawRect(Rect.fromLTRB(left, address, right, address + 38 * u), Paint()..color = const Color(0xfff3f3f3));
    x = left + 10 * u;
    for (final data in [Icons.arrow_back_rounded, Icons.arrow_forward_rounded, Icons.arrow_upward_rounded]) {
      icon(data, Offset(x, address + 10 * u), 16 * u, const Color(0xff666666));
      x += 26 * u;
    }
    final searchWidth = portrait ? 0.0 : 190 * u;
    canvas.drawRRect(
      box(x + 6 * u, address + 6 * u, right - searchWidth - 18 * u, address + 32 * u, 4 * u),
      Paint()..color = Colors.white,
    );
    text(
      ru ? 'Этот компьютер  ›  Документы' : 'This PC  ›  Documents',
      Offset(x + 16 * u, address + 12 * u),
      11.5 * u,
      const Color(0xff1b1b1b),
      max: right - searchWidth - x - 40 * u,
    );
    if (!portrait) {
      canvas.drawRRect(
        box(right - searchWidth - 8 * u, address + 6 * u, right - 10 * u, address + 32 * u, 4 * u),
        Paint()..color = Colors.white,
      );
      icon(Icons.search_rounded, Offset(right - 34 * u, address + 11 * u), 15 * u, const Color(0xff666666));
      text(
        ru ? 'Поиск: Документы' : 'Search Documents',
        Offset(right - searchWidth + 4 * u, address + 12 * u),
        11 * u,
        const Color(0xff777777),
      );
    }
    // Navigation pane.
    final body = address + 38 * u;
    final nav = portrait ? 0.0 : 170 * u;
    if (!portrait) {
      canvas.drawRect(Rect.fromLTRB(left, body, left + nav, bottom), Paint()..color = const Color(0xfff3f3f3));
      for (final (i, (data, label, color)) in [
        (Icons.home_rounded, ru ? 'Главная' : 'Home', const Color(0xff0067c0)),
        (Icons.desktop_windows_rounded, ru ? 'Рабочий стол' : 'Desktop', const Color(0xff2b88d8)),
        (Icons.download_rounded, ru ? 'Загрузки' : 'Downloads', const Color(0xff0f7b0f)),
        (Icons.description_rounded, ru ? 'Документы' : 'Documents', const Color(0xff0067c0)),
        (Icons.image_rounded, ru ? 'Изображения' : 'Pictures', const Color(0xff2b88d8)),
        (Icons.music_note_rounded, ru ? 'Музыка' : 'Music', const Color(0xffd13438)),
        (Icons.computer_rounded, ru ? 'Этот компьютер' : 'This PC', const Color(0xff444444)),
        (Icons.lan_rounded, ru ? 'Сеть' : 'Network', const Color(0xff444444)),
      ].indexed) {
        final y = body + 10 * u + i * 30 * u;
        if (y > bottom - 30 * u) break;
        if (i == 3) {
          canvas.drawRRect(
            box(left + 6 * u, y - 3 * u, left + nav - 6 * u, y + 25 * u, 4 * u),
            Paint()..color = const Color(0xffdde8f3),
          );
        }
        icon(data, Offset(left + 14 * u, y + 2 * u), 16 * u, color);
        text(label, Offset(left + 38 * u, y + 4 * u), 11.5 * u, const Color(0xff1b1b1b), max: nav - 46 * u);
      }
    }
    // File list.
    final listLeft = left + nav + 12 * u, listRight = right - 12 * u;
    final columns = portrait ? [0.0, .78] : [0.0, .44, .70, .88];
    final headers = ru ? ['Имя', 'Дата изменения', 'Тип', 'Размер'] : ['Name', 'Date modified', 'Type', 'Size'];
    final shown = portrait ? [headers[0], headers[3]] : headers;
    for (final (i, label) in shown.indexed) {
      text(
        label,
        Offset(listLeft + (listRight - listLeft) * columns[i], body + 10 * u),
        11 * u,
        const Color(0xff555555),
      );
    }
    canvas.drawRect(
      Rect.fromLTRB(listLeft, body + 30 * u, listRight, body + 31 * u),
      Paint()..color = const Color(0xffe0e0e0),
    );
    final rows = [
      (
        Icons.folder_rounded,
        const Color(0xffffc83d),
        ru ? 'Проекты' : 'Projects',
        ru ? 'Папка с файлами' : 'File folder',
        '',
      ),
      (
        Icons.folder_rounded,
        const Color(0xffffc83d),
        ru ? 'Скриншоты' : 'Screenshots',
        ru ? 'Папка с файлами' : 'File folder',
        '',
      ),
      (
        Icons.description_rounded,
        const Color(0xff185abd),
        ru ? 'Отчёт за квартал.docx' : 'Quarterly report.docx',
        'Word',
        '84 KB',
      ),
      (
        Icons.table_chart_rounded,
        const Color(0xff107c41),
        ru ? 'Бюджет 2026.xlsx' : 'Budget 2026.xlsx',
        'Excel',
        '31 KB',
      ),
      (
        Icons.slideshow_rounded,
        const Color(0xffc43e1c),
        ru ? 'Презентация.pptx' : 'Presentation.pptx',
        'PowerPoint',
        '2,4 MB',
      ),
      (
        Icons.article_rounded,
        const Color(0xff666666),
        ru ? 'заметки.txt' : 'notes.txt',
        ru ? 'Текстовый документ' : 'Text document',
        '2 KB',
      ),
      (Icons.terminal_rounded, const Color(0xff012456), 'deploy.ps1', 'PowerShell', '6 KB'),
    ];
    for (final (i, row) in rows.indexed) {
      final y = body + 38 * u + i * 30 * u;
      if (y > bottom - 34 * u) break;
      if (i == 2) {
        canvas.drawRRect(
          box(listLeft - 4 * u, y - 4 * u, listRight + 4 * u, y + 24 * u, 4 * u),
          Paint()..color = const Color(0xffcce4f7),
        );
      }
      icon(row.$1, Offset(listLeft, y + 1 * u), 16 * u, row.$2);
      final nameWidth = (listRight - listLeft) * (portrait ? .74 : .40);
      text(row.$3, Offset(listLeft + 24 * u, y + 3 * u), 11.5 * u, const Color(0xff1b1b1b), max: nameWidth);
      final cells = portrait ? [row.$5] : ['${20 - i}.10.2026 ${9 + i}:4${i % 6}', row.$4, row.$5];
      for (final (c, value) in cells.indexed) {
        text(
          value,
          Offset(listLeft + (listRight - listLeft) * columns[c + 1], y + 3 * u),
          11 * u,
          const Color(0xff555555),
          max: (listRight - listLeft) * .24,
        );
      }
    }
    text(ru ? 'Элементов: 7' : '7 items', Offset(listLeft, bottom - 22 * u), 10.5 * u, const Color(0xff666666));
    canvas.restore();

    // Taskbar.
    final tb = h - taskbar;
    canvas.drawRect(Rect.fromLTRB(0, tb, w, h), Paint()..color = const Color(0xee1c1f26));
    canvas.drawRect(Rect.fromLTRB(0, tb, w, tb + 1), Paint()..color = const Color(0x22ffffff));
    final apps = <(IconData?, Color)>[
      (null, const Color(0xff4cc2ff)),
      (Icons.search_rounded, Colors.white),
      (Icons.folder_rounded, const Color(0xffffc83d)),
      (Icons.public_rounded, const Color(0xff36c3a6)),
      (Icons.terminal_rounded, Colors.white),
      (Icons.settings_rounded, Colors.white),
    ];
    final step = 44 * u;
    var ax = w / 2 - apps.length * step / 2 + (portrait ? -50 * u : 0);
    for (final (i, (data, color)) in apps.indexed) {
      final c = Offset(ax + step / 2, tb + taskbar / 2);
      if (i == 2) {
        canvas.drawRRect(
          box(c.dx - 18 * u, c.dy - 18 * u, c.dx + 18 * u, c.dy + 18 * u, 4 * u),
          Paint()..color = const Color(0x22ffffff),
        );
        canvas.drawRRect(
          box(c.dx - 7 * u, c.dy + 15 * u, c.dx + 7 * u, c.dy + 18 * u, 2 * u),
          Paint()..color = const Color(0xff4cc2ff),
        );
      }
      if (data == null) {
        // Start: four blue panes.
        final s = 9 * u, g = 1.5 * u;
        for (final (dx, dy) in [(-1, -1), (0, -1), (-1, 0), (0, 0)]) {
          canvas.drawRect(
            Rect.fromLTWH(c.dx + dx * (s + g) + (dx == 0 ? g : 0), c.dy + dy * (s + g) + (dy == 0 ? g : 0), s, s),
            Paint()..color = color,
          );
        }
      } else {
        icon(data, c - Offset(11 * u, 11 * u), 22 * u, color);
      }
      ax += step;
    }
    // Tray.
    const clock = ru ? '14:32' : '2:32 PM';
    const date = ru ? '21.10.2026' : '10/21/2026';
    text(clock, Offset(w - 78 * u, tb + 9 * u), 11 * u, Colors.white);
    text(date, Offset(w - 78 * u, tb + 25 * u), 10.5 * u, Colors.white);
    text(ru ? 'РУС' : 'ENG', Offset(w - 118 * u, tb + 17 * u), 10.5 * u, Colors.white);
    if (!portrait) {
      for (final (i, data) in [Icons.battery_full_rounded, Icons.volume_up_rounded, Icons.wifi_rounded].indexed) {
        icon(data, Offset(w - 150 * u - i * 22 * u, tb + 15 * u), 16 * u, Colors.white);
      }
      icon(Icons.keyboard_arrow_up_rounded, Offset(w - 230 * u, tb + 15 * u), 16 * u, Colors.white);
    }

    final image = await recorder.endRecording().toImage(width, height);
    try {
      return (await image.toByteData(format: ui.ImageByteFormat.rawRgba))!.buffer.asUint8List();
    } finally {
      image.dispose();
    }
  });
  return RdpFrame(sequence: sequence, width: width, height: height, rgba: bytes!);
}
