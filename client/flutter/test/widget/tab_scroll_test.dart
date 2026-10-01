import 'package:consolecrypt/core/glass/glass.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import 'glass_kit_test.dart' show pumpGlass;

SingleChildScrollView _scroll(WidgetTester tester) => tester.widget(find.byKey(const ValueKey('glass-tabs-scroll')));

void main() {
  testWidgets('narrow desktop keeps session actions below scrollable tabs and preserves the active tab', (
    tester,
  ) async {
    await pumpGlass(
      tester,
      Center(
        child: GlassTabStrip(
          tabs: [for (var i = 0; i < 7; i++) GlassTab(key: ValueKey('compact-host-$i'), title: 'Server $i')],
          activeIndex: 6,
          onSelect: (_) {},
          onAdd: () {},
          trailing: const SizedBox(
            width: 232,
            height: 28,
            child: Text('Session actions', key: ValueKey('session-actions')),
          ),
        ),
      ),
      size: const Size(390, 300),
    );
    await settle(tester);
    expect(find.text('(7)'), findsOneWidget);
    final track = tester.getRect(find.byKey(const ValueKey('glass-tabs-scroll')));
    final actions = tester.getRect(find.byKey(const ValueKey('session-actions')));
    expect(actions.top, greaterThan(track.bottom));
    final active = tester.getRect(find.byKey(const ValueKey('compact-host-6')));
    expect(active.left, greaterThanOrEqualTo(track.left - 1));
    expect(active.right, lessThanOrEqualTo(track.right + 1));
    tester.view.physicalSize = const Size(1100, 300);
    await settle(tester);
    final wideTrack = tester.getRect(find.byKey(const ValueKey('glass-tabs-scroll')));
    final wideActions = tester.getRect(find.byKey(const ValueKey('session-actions')));
    expect(wideActions.center.dy, closeTo(wideTrack.center.dy, 1));
    tester.view.physicalSize = const Size(390, 300);
    await settle(tester);
    expect(find.text('(7)'), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  for (final reduceMotion in [false, true]) {
    testWidgets('overflow arrows, wheel and active-tab reveal work (reduce motion: $reduceMotion)', (tester) async {
      var active = 0;
      var count = 7;
      late StateSetter update;
      await pumpGlass(
        tester,
        StatefulBuilder(
          builder: (context, setState) {
            update = setState;
            return Center(
              child: GlassTabStrip(
                tabs: [for (var i = 0; i < count; i++) GlassTab(key: ValueKey('scroll-host-$i'), title: 'Server $i')],
                activeIndex: active,
                onSelect: (i) => setState(() => active = i),
              ),
            );
          },
        ),
        size: const Size(500, 300),
        appearance: GlassAppearance(reduceMotion: reduceMotion),
      );
      await settle(tester);
      expect(isEnabled(tester, 'glass-tabs-scroll-left'), isFalse);
      expect(isEnabled(tester, 'glass-tabs-scroll-right'), isTrue);
      expect(find.text('(7)'), findsOneWidget);
      expect(find.byKey(const ValueKey('glass-tabs-count')), findsOneWidget);
      await tapKey(tester, 'glass-tabs-scroll-right');
      final afterClick = _scroll(tester).controller!.offset;
      expect(afterClick, greaterThan(0));
      expect(isEnabled(tester, 'glass-tabs-scroll-left'), isTrue);
      await tester.sendEventToBinding(
        PointerScrollEvent(
          position: tester.getCenter(find.byKey(const ValueKey('glass-tabs-scroll'))),
          scrollDelta: const Offset(0, -90),
        ),
      );
      await settle(tester);
      expect(_scroll(tester).controller!.offset, lessThan(afterClick));
      update(() => active = count - 1);
      await settle(tester);
      final viewport = tester.getRect(find.byKey(const ValueKey('glass-tabs-scroll')));
      final lastTab = tester.getRect(find.byKey(ValueKey('scroll-host-${count - 1}')));
      expect(lastTab.left, greaterThanOrEqualTo(viewport.left - 1));
      expect(lastTab.right, lessThanOrEqualTo(viewport.right + 1));
      expect(isEnabled(tester, 'glass-tabs-scroll-right'), isFalse);
      expect(find.text('(7)'), findsOneWidget);
      update(() => active = 0);
      await settle(tester);
      expect(_scroll(tester).controller!.offset, closeTo(0, 1));
      tester.view.physicalSize = const Size(1600, 300);
      await settle(tester);
      expect(find.byKey(const ValueKey('glass-tabs-scroll-left')), findsNothing);
      expect(find.byKey(const ValueKey('glass-tabs-scroll-right')), findsNothing);
      expect(find.byKey(const ValueKey('glass-tabs-count')), findsNothing);
      tester.view.physicalSize = const Size(500, 300);
      await settle(tester);
      expect(find.byKey(const ValueKey('glass-tabs-scroll-right')), findsOneWidget);
      expect(find.text('(7)'), findsOneWidget);
      update(() => count = 1);
      await settle(tester);
      expect(find.byKey(const ValueKey('glass-tabs-scroll-left')), findsNothing);
      expect(find.byKey(const ValueKey('glass-tabs-scroll-right')), findsNothing);
      expect(find.byKey(const ValueKey('glass-tabs-count')), findsNothing);
      expect(tester.takeException(), isNull);
    }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  }
}
