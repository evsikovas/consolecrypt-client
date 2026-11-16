import 'package:consolecrypt/automation/automation_job_admission_audit.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/automation_audit_fixture.dart' as f;

Widget host(AutomationJson row, {Locale locale = const Locale('en'), double scale = 1}) => MaterialApp(
  locale: locale,
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
    child: child!,
  ),
  home: Scaffold(
    body: SingleChildScrollView(
      child: AutomationJobAdmissionAuditView(job: row, expanded: true, page: 0, onExpanded: (_) {}, onPage: (_) {}),
    ),
  ),
);
void main() {
  testWidgets('recorded admission shows original device purpose digests and targets without credentials or authority', (
    t,
  ) async {
    await t.pumpWidget(host(f.fixture(production: true)));
    await t.pumpAndSettle();
    expect(find.text(f.deviceId), findsOneWidget);
    expect(find.text('jobs.execute'), findsOneWidget);
    expect(find.text('e' * 64), findsOneWidget);
    expect(find.byKey(ValueKey('audit-target-${f.hostId(0)}')), findsOneWidget);
    expect(find.textContaining('HIDDEN_'), findsNothing);
    expect(find.byType(FilledButton), findsNothing);
    expect(find.textContaining('does not record tunnel activity'), findsOneWidget);
    expect(find.textContaining('immutable log'), findsOneWidget);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
  });
  testWidgets('legacy invalid foreign and unavailable audit never renders signer or an approved outcome', (t) async {
    final rows = [
      f.fixture()..remove('admission_audit'),
      f.fixture()..['admission_audit'] = {'version': 1, 'status': 'unrecorded', 'reason': 'runner_unrecorded'},
      f.fixture()..['admission_audit'] = {'version': 1, 'status': 'unavailable', 'reason': 'binding_mismatch'},
      f.fixture()..['job_id'] = f.projectId,
    ];
    for (final row in rows) {
      await t.pumpWidget(const SizedBox());
      await t.pumpWidget(host(row));
      await t.pumpAndSettle();
      expect(find.text(f.deviceId), findsNothing);
      expect(find.text('jobs.execute'), findsNothing);
      expect(find.byType(FilledButton), findsNothing);
      expect(find.textContaining('succeeded'), findsNothing);
      expect(t.takeException(), isNull);
    }
  });
  testWidgets('RU narrow read-only audit keeps exact bounded IDs and all target metadata without overflow', (t) async {
    await t.binding.setSurfaceSize(const Size(500, 1000));
    addTearDown(() => t.binding.setSurfaceSize(null));
    await t.pumpWidget(host(f.fixture(), locale: const Locale('ru'), scale: 1.5));
    await t.pumpAndSettle();
    expect(find.text(f.deviceId), findsOneWidget);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
  });
}
