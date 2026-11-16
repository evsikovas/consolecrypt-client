import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_review_cards.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

Future<void> pumpReview(WidgetTester tester, Widget child, {Locale locale = const Locale('en')}) async {
  tester.view.physicalSize = const Size(430, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(
    theme: AppTheme.dark(platform: TargetPlatform.windows), locale: locale,
    localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
    supportedLocales: AppLocalizations.supportedLocales,
    home: Scaffold(body: SingleChildScrollView(child: Padding(padding: const EdgeInsets.all(12), child: child))),
  ));
  await tester.pumpAndSettle();
}
AutomationJson privileges({Object? accounts, Object? escalation = 'not_verified'}) => {
  'privilege_review': {'escalation': escalation, 'accounts': accounts ?? [{'host_id': 'host', 'username': 'deploy'}]},
  'targets': [{'host_id': 'host', 'username': 'deploy', 'alias': 'reviewed_alias'}],
};
AutomationJson declaredSummary({int count = 1}) => {
  'version': 1, 'source': 'ansible_declared_v1', 'truncated': false,
  'unresolved': ['runtime_overrides', 'roles', 'task_includes'],
  'records': [for (var i = 1; i <= count; i++) {
    'play': 1, 'area': 'tasks', 'indices': [i], 'branches': <String>[],
    'become': i == 1 ? 'enabled' : 'disabled', 'user': 'literal_hidden',
    'method': i == 1 ? 'literal_hidden' : 'dynamic', 'knownMethod': i == 1 ? 'sudo' : null,
  }],
};
void main() {
  testWidgets('RUN02 declaration review is collapsed informational metadata with hidden values and no permission claim', (tester) async {
    await pumpReview(tester, AutomationReviewDeclarations(summary: declaredSummary(count: 2)));
    expect(find.byKey(const ValueKey('automation-declaration-0')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('automation-review-declarations')));
    await tester.pumpAndSettle();
    expect(find.text('Play 1 · tasks / 1'), findsOneWidget);
    expect(find.text('become: Declared on; become_user: Value hidden; become_method: sudo'), findsOneWidget);
    expect(find.text('become: Declared off; become_user: Value hidden; become_method: Dynamic; unresolved'), findsOneWidget);
    expect(find.textContaining('Effective privileges remain unverified'), findsOneWidget);
    expect(find.byType(FilledButton), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('RUN02 declaration malformed future or private data stays unavailable rather than entering UI', (tester) async {
    for (final key in ['knownMethod', 'user']) {
      final summary = declaredSummary();
      ((summary['records'] as List).single as AutomationJson)[key] = 'PRIVATE_DECLARATION_VALUE';
      await pumpReview(tester, AutomationReviewDeclarations(key: ValueKey('invalid-$key'), summary: summary));
      await tester.tap(find.byKey(const ValueKey('automation-review-declarations')));
      await tester.pumpAndSettle();
      expect(find.textContaining('PRIVATE'), findsNothing);
      expect(find.text('Unknown'), findsOneWidget);
      expect(find.byKey(const ValueKey('automation-declaration-0')), findsNothing);
    }
    await pumpReview(tester, AutomationReviewDeclarations(key: const ValueKey('future'), summary: declaredSummary()..['version'] = 1.0));
    await tester.tap(find.byKey(const ValueKey('automation-review-declarations'))); await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('automation-declaration-0')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('RUN02 declaration known empty and truncated scan never promises no escalation', (tester) async {
    await pumpReview(tester, AutomationReviewDeclarations(summary: declaredSummary(count: 0)..['truncated'] = true));
    await tester.tap(find.byKey(const ValueKey('automation-review-declarations'))); await tester.pumpAndSettle();
    expect(find.textContaining('No explicit escalation directives'), findsOneWidget);
    expect(find.textContaining('scan limit was reached'), findsOneWidget);
    expect(find.textContaining('without sudo'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('RUN02 declaration rows are bounded and Russian narrow view keeps all metadata private', (tester) async {
    await pumpReview(tester, AutomationReviewDeclarations(summary: declaredSummary(count: 45)), locale: const Locale('ru'));
    await tester.tap(find.byKey(const ValueKey('automation-review-declarations'))); await tester.pumpAndSettle();
    Finder rows() => find.byWidgetPredicate((w) => w.key is ValueKey<String> &&
      (w.key! as ValueKey<String>).value.startsWith('automation-declaration-'));
    expect(rows(), findsNWidgets(20));
    await tester.ensureVisible(find.byKey(const ValueKey('automation-declarations-next')));
    await tester.tap(find.byKey(const ValueKey('automation-declarations-next'))); await tester.pumpAndSettle();
    expect(rows(), findsNWidgets(20));
    await tester.tap(find.byKey(const ValueKey('automation-declarations-next'))); await tester.pumpAndSettle();
    expect(rows(), findsNWidgets(5));
    expect(tester.takeException(), isNull);
  });

  testWidgets('RUN02 extra variable values and nested credential references never enter the review tree', (tester) async {
    const secretRef = 'b8a93eaf-45e6-4991-aaf0-89cfcaefcfdf';
    await pumpReview(tester, const AutomationReviewVariables(parameters: {
      'extra_vars': {'plain': 'PRIVATE_STRING', 'number': 735919, 'nested': {'credentialId': secretRef},
        secretRef: 'PRIVATE_KEY_VALUE', 'boolean': true},
      'tags': ['PRIVATE_TAG'], 'skip_tags': ['PRIVATE_SKIP'], 'diff': false,
      'vault': {'credentialId': 'PRIVATE_VAULT_REF'},
    }));
    for (final private in ['PRIVATE_STRING', '735919', secretRef, 'PRIVATE_KEY_VALUE', 'PRIVATE_TAG',
      'PRIVATE_SKIP', 'PRIVATE_VAULT_REF', 'credentialId']) {
      expect(find.textContaining(private), findsNothing);
    }
    expect(find.text('plain'), findsOneWidget);
    expect(find.text('number'), findsOneWidget);
    expect(find.text('Value hidden'), findsNWidgets(5));
    expect(find.text('Private variable name'), findsOneWidget);
    final checkboxes = tester.widgetList<CheckboxListTile>(find.byType(CheckboxListTile)).toList();
    expect(checkboxes.map((v) => v.value), [false, true]);
    expect(checkboxes.every((v) => v.onChanged == null), true);
    expect(find.byType(SelectableText), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('RUN02 review variable rows paginate at twenty without dumping values', (tester) async {
    await pumpReview(tester, AutomationReviewVariables(parameters: {
      'extra_vars': {for (var i = 0; i < 45; i++) 'var_${i.toString().padLeft(2, '0')}': 'PRIVATE'},
    }));
    Finder rows() => find.byWidgetPredicate((w) => w.key is ValueKey<String> &&
      (w.key! as ValueKey<String>).value.startsWith('automation-reviewed-variable-'));
    expect(rows(), findsNWidgets(20)); expect(find.text('var_20'), findsNothing);
    await tester.ensureVisible(find.byKey(const ValueKey('automation-reviewed-variables-next')));
    await tester.tap(find.byKey(const ValueKey('automation-reviewed-variables-next'))); await tester.pumpAndSettle();
    expect(rows(), findsNWidgets(20)); expect(find.text('var_00'), findsNothing); expect(find.text('var_20'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('automation-reviewed-variables-next'))); await tester.pumpAndSettle();
    expect(rows(), findsNWidgets(5)); expect(find.text('var_44'), findsOneWidget);
    expect(find.textContaining('PRIVATE'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('RUN02 absent or malformed variables stay unknown while a known empty map stays empty', (tester) async {
    await pumpReview(tester, const AutomationReviewVariables(parameters: {'extra_vars': <String, Object?>{}}));
    expect(find.text('No extra variables'), findsOneWidget);
    await pumpReview(tester, const AutomationReviewVariables(parameters: {'extra_vars': ['PRIVATE']}));
    expect(find.text('No extra variables'), findsNothing); expect(find.textContaining('PRIVATE'), findsNothing);
    expect(find.text('Value hidden'), findsNothing); expect(tester.takeException(), isNull);
  });
  testWidgets('RUN02 privilege review shows actual frozen account and explicitly unverified escalation', (tester) async {
    await pumpReview(tester, AutomationReviewPrivileges(review: privileges()));
    expect(find.text('reviewed_alias: deploy'), findsOneWidget);
    expect(find.text('Privilege escalation: Not verified'), findsOneWidget);
    expect(find.textContaining('not verified; review the project'), findsOneWidget);
    expect(find.text('host'), findsNothing); expect(find.textContaining('without sudo'), findsNothing);
  });
  testWidgets('RUN02 missing foreign duplicated or mismatching accounts never fall back to target credentials', (tester) async {
    for (final accounts in [<Object?>[], [{'host_id': 'other', 'username': 'deploy'}],
      [{'host_id': 'host', 'username': 'PRIVATE_BAD_ACCOUNT'}],
      [{'host_id': 'host', 'username': 'deploy', 'credentialId': 'PRIVATE_REF'}]]) {
      await pumpReview(tester, AutomationReviewPrivileges(review: privileges(accounts: accounts)));
      expect(find.text('reviewed_alias: deploy'), findsNothing);
      expect(find.textContaining('PRIVATE'), findsNothing);
    }
    final noSummary = privileges()..remove('privilege_review');
    await pumpReview(tester, AutomationReviewPrivileges(review: noSummary));
    expect(find.text('reviewed_alias: deploy'), findsNothing);
    expect(find.text('Privilege escalation: Not verified'), findsNothing);
  });
  testWidgets('RUN02 future escalation stays unknown and Russian narrow review has no overflow', (tester) async {
    await pumpReview(tester, AutomationReviewPrivileges(review: privileges(escalation: 'future_no_sudo')),
      locale: const Locale('ru'));
    expect(find.textContaining('future_no_sudo'), findsNothing);
    expect(tester.takeException(), isNull);
  });
}
