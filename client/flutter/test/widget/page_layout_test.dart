import 'dart:ui' as ui;

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

Future<List<int>> _edgeAlpha(WidgetTester tester) async {
  final boundary = tester.renderObject<RenderRepaintBoundary>(find.byKey(const ValueKey('scroll-capture')));
  return (await tester.runAsync(() async {
    final image = await boundary.toImage();
    final data = (await image.toByteData(format: ui.ImageByteFormat.rawRgba))!;
    final result = [
      for (final y in [1, 50, 98]) data.getUint8((y * image.width + 50) * 4 + 3),
    ];
    image.dispose();
    return result;
  }))!;
}

void main() {
  for (final width in [1024.0, 1600.0]) {
    for (final scale in [1.0, 1.4]) {
      testWidgets('page headings, actions and first content do not overlap at $width / $scale', (tester) async {
        tester.platformDispatcher.textScaleFactorTestValue = scale;
        addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
        final backend = testBackend();
        addTearDown(backend.dispose);
        await backend.debugSignInDemoAndUnlock();
        await pumpApp(tester, backend, locale: AppLocale.ru, size: Size(width, 1000));
        expect(tester.takeException(), isNull, reason: 'initial app shell');

        for (final branch in ['sharing', 'sync', 'backups']) {
          await tapKey(tester, 'nav-$branch');
          final page = find.byType(PageScaffold);
          final scaffold = tester.widget<PageScaffold>(page);
          final toolbar = tester.getRect(find.byType(GlassToolbar));
          final title = tester.getRect(find.descendant(of: page, matching: find.text(scaffold.title)).first);
          var headerBottom = title.bottom;
          if (scaffold.subtitle != null) {
            final subtitle = tester.getRect(find.text(scaffold.subtitle!));
            expect(subtitle.top, greaterThanOrEqualTo(title.bottom), reason: '$branch subtitle follows title');
            headerBottom = subtitle.bottom;
          }
          expect(title.top - toolbar.bottom, greaterThanOrEqualTo(24), reason: '$branch clears top toolbar');
          for (final action in scaffold.actions) {
            final rect = tester.getRect(find.byWidget(action));
            expect(rect.top, greaterThanOrEqualTo(toolbar.bottom + 24), reason: '$branch action clears toolbar');
            if (rect.bottom > headerBottom) headerBottom = rect.bottom;
          }
          final firstContent = tester.getRect(find.byWidget(scaffold.body));
          expect(firstContent.top - headerBottom, greaterThanOrEqualTo(24), reason: '$branch content clears actions');
          expect(tester.takeException(), isNull, reason: '$branch at $width / $scale');
        }
      }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
    }
  }

  testWidgets('scroll fades preserve the first and last row at the viewport boundaries', (tester) async {
    final controller = ScrollController();
    addTearDown(controller.dispose);
    final contentHeight = ValueNotifier<double>(300);
    addTearDown(contentHeight.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Center(
          child: RepaintBoundary(
            key: const ValueKey('scroll-capture'),
            child: SizedBox(
              width: 100,
              height: 100,
              child: ScrollEdgeEffect(
                bottom: true,
                child: SingleChildScrollView(
                  controller: controller,
                  child: ValueListenableBuilder<double>(
                    valueListenable: contentHeight,
                    builder: (context, height, _) => SizedBox(
                      height: height,
                      child: const ColoredBox(color: Colors.white),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final position = controller.position;
    final first = await _edgeAlpha(tester);
    expect(first[0], 255, reason: 'top row must not disappear beneath the page header at rest');
    expect(first[1], 255);
    expect(first[2], lessThan(50), reason: 'more content below is indicated');

    controller.jumpTo(80);
    await tester.pumpAndSettle();
    final middle = await _edgeAlpha(tester);
    expect(middle[0], lessThan(50));
    expect(middle[2], lessThan(50));
    expect(controller.position, same(position), reason: 'changing edge visibility keeps the same scroll state');

    controller.jumpTo(controller.position.maxScrollExtent);
    await tester.pumpAndSettle();
    final last = await _edgeAlpha(tester);
    expect(last[0], lessThan(50));
    expect(last[2], 255, reason: 'last row stays fully visible at the end');

    // A short list after refresh no longer needs either fade; this is a
    // metrics change without a user scroll gesture.
    contentHeight.value = 100;
    await tester.pumpAndSettle();
    expect(await _edgeAlpha(tester), [255, 255, 255]);
    expect(tester.takeException(), isNull);
  });
}
