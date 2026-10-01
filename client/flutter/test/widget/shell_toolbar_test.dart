import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  for (final locale in [AppLocale.en, AppLocale.ru]) {
    for (final width in [1000.0, 1600.0]) {
      testWidgets('compact connection action and search at $width in ${locale.name}', (tester) async {
        final backend = testBackend();
        addTearDown(backend.dispose);
        await backend.debugSignInDemoAndUnlock();
        await pumpApp(tester, backend, locale: locale, size: Size(width, 1000));

        final connection = find.byKey(const ValueKey('new-connection'));
        final button = tester.widget<GlassIconButton>(connection);
        expect(button.icon, Icons.add_rounded);
        expect(button.tooltip, contains(locale == AppLocale.ru ? 'Новое подключение' : 'New connection'));
        expect(tester.getSize(connection), const Size(40, 40));
        expect(find.descendant(of: connection, matching: find.byType(Text)), findsNothing);
        final search = find.byKey(const ValueKey('open-palette'));
        expect(tester.getSize(search).width, width == 1600 ? greaterThan(420) : greaterThan(150));
        final toolbar = tester.getRect(find.byType(GlassToolbar));
        final menu = tester.getRect(find.byKey(const ValueKey('shell-menu')));
        expect(toolbar.right - menu.right, closeTo(12, 1), reason: 'unused toolbar width belongs to search');
        expect(
          tester.getRect(connection).left - tester.getRect(search).right,
          closeTo(12, 1),
          reason: 'search stretches to the connection action',
        );
        if (width == 1600) expect(tester.getSize(search).width, greaterThan(700));
        expect(tester.takeException(), isNull);

        final sync = find.byKey(const ValueKey('sync-indicator'));
        final status = tester.widget<GlassStatusPill>(sync);
        expect(status.label, locale == AppLocale.ru ? 'Синх.' : 'Sync');
        expect(status.tone, GlassTone.success);
        expect(status.icon, isNull, reason: 'successful sync uses the green status dot');
        expect(status.tooltip, contains(locale == AppLocale.ru ? 'Синхронизировано' : 'Synced'));

        backend.sync.setOffline(true);
        await settle(tester);
        final offline = tester.widget<GlassStatusPill>(sync);
        expect(offline.tone, GlassTone.warning);
        expect(offline.label, contains(locale == AppLocale.ru ? 'Офлайн' : 'Offline'));

        await tapKey(tester, 'new-connection');
        expect(find.byKey(const ValueKey('host-picker-search')), findsOneWidget);
      }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
    }
  }
}
