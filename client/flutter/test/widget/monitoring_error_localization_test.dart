import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  Future<void> render(WidgetTester tester, Json error) async {
    await tester.pumpWidget(
      MaterialApp(
        locale: const Locale('ru'),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: MonitoringResultView(
            result: QueryOutcome.fromJson({
              'status': 'error',
              'httpStatus': 422,
              'error': error,
              'partial': true,
              'warnings': ['Untrusted fixture warning'],
              'infos': ['Untrusted fixture annotation'],
            }),
          ),
        ),
      ),
    );
    await tester.pump();
  }

  testWidgets('Russian result hides Native English diagnostics and keeps status and warnings', (tester) async {
    await render(tester, {'code': 'invalid_query', 'message': 'Native English diagnostic', 'httpStatus': 422});
    expect(find.textContaining(AppLocalizationsRu().monitoringErrorQuery), findsOneWidget);
    expect(find.textContaining('Native English diagnostic'), findsNothing);
    expect(find.textContaining('HTTP 422'), findsNWidgets(2));
    expect(find.byKey(const ValueKey('monitoring-partial')), findsOneWidget);
    expect(find.textContaining('Untrusted fixture warning'), findsOneWidget);
    expect(find.textContaining('Untrusted fixture annotation'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('actual PromQL parser feedback is displayed literally within Russian context', (tester) async {
    const message = 'parse error: <b>fixture</b> unexpected input';
    await render(tester, {'type': 'bad_data', 'message': message, 'httpStatus': 422});
    expect(find.textContaining(AppLocalizationsRu().monitoringErrorServer), findsOneWidget);
    expect(find.textContaining(message), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
