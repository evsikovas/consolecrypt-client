import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_reader_fixture.dart' as f;
import 'agent_reader_history_test.dart' as history;
import 'agent_reader_test.dart' as old;

void main() {
  for (final locale in ['en', 'ru']) {
    testWidgets('Agent version in $locale appears only after the original explicit heartbeat read', (tester) async {
      final port = f.Port(f.selection());
      final status = f.statusJson(port.selection);
      (status['associationChoices'] as Map<String, dynamic>)['items'] = [f.choice()];
      port.statusResponse = status;
      final AppLocalizations l = locale == 'en' ? AppLocalizationsEn() : AppLocalizationsRu();
      await tester.pumpWidget(old.host(port, f.Files(), locale: Locale(locale)));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('agent-heartbeat-version')), findsNothing);
      expect(port.requests, isEmpty);
      await history.ready(tester);
      expect(find.byKey(const ValueKey('agent-heartbeat-version')), findsNothing);
      await old.tap(tester, 'agent-consume');
      expect(find.text('${l.agentReaderVersion}: 1.0.0'), findsOneWidget);
      expect(port.requests, hasLength(1));
      expect(port.handles.single.confirms, 1);
      expect(port.handles.single.reads, 1);
      await tester.pump(const Duration(seconds: 2));
      await tester.pump();
      expect(port.requests, hasLength(1));
      expect(port.handles.single.reads, 1);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    });
  }
}
