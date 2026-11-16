import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/automation_ai_dialog.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_run_dialog.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import '../unit/job_metric_correlation_test.dart' as f;

// Native behavior is covered by its real tests; this synthetic service checks
// the production UI's explicit choices, draft/CAS and fresh approval boundaries.
class Api extends Fake implements AutomationService, AutomationAiService,
    AutomationJobService, AutomationEventService {
  String profile = f.profile, text = '---\n- hosts: all\n  tasks: []\n';
  int revision = 1, saves = 0, previews = 0, generations = 0, reads = 0, confirmations = 0;
  String? task, savedExpected;
  Completer<AutomationProjectFile>? pendingRead;
  Completer<AutomationJson>? pendingPage;
  final provider = AiProviderConfig.draft(AiProviderKind.openaiCompatible)
      .copyWith(name: 'AI repair fixture', chatModel: 'fixture');
  AutomationProject get project => AutomationProject(id: f.project, name: 'Repair project', readOnly: false, revision: revision);
  AutomationJson row() {
    final row = f.row(state: 'failed', revision: 3);
    (row['manifest'] as AutomationJson).addAll(<String, Object>{'tenant_id': profile, 'playbook': 'playbook.yml', 'mode': 'apply'});
    (((row['manifest'] as AutomationJson)['targets'] as List).single as Map<String, Object>)['alias'] = 'approved';
    row['runner_result'] = {'version': 1, 'job_id': f.uuid(100), 'manifest_digest': f.digest,
      'result': {'exit_code': 2, 'remote_changes_may_be_partial': true,
        'summary': {'stats': {'changed': {'approved': 2}, 'failures': {'approved': 1}}}}};
    return row;
  }
  @override Future<AutomationJson> jobs() async => f.catalog([row()]);
  @override Future<AutomationProjectCatalog> projects() async => AutomationProjectCatalog(
    version: 1, readOnly: false, deviceLocal: true, projects: [project]);
  @override Future<AutomationProjectRevision> files(String id) async => AutomationProjectRevision(
    digest: 'revision-$revision', bytes: text.length,
    entries: const [AutomationFileEntry(path: 'playbook.yml', directory: false)]);
  @override Future<AutomationProjectFile> readFile(String id, String path) async {
    reads++;
    return pendingRead?.future ?? AutomationProjectFile(path: path, text: text, sha256: 'file-$revision', lineEndings: 'lf');
  }
  @override Future<AutomationProjectFile> saveFile(String id, {required String path,
    required String text, String? expectedSha256}) async {
    savedExpected = expectedSha256; this.text = text; saves++; revision++;
    return AutomationProjectFile(path: path, text: text, sha256: 'file-$revision', lineEndings: 'lf');
  }
  @override Future<AutomationJson> prepareAi({required String providerId, required String task,
    required String path, String? selectedText}) async {
    previews++; this.task = task;
    return {'review_id': 'one-use-ai', 'provider_name': 'AI repair fixture',
      'locality': 'public', 'preview': 'Prepared selected context'};
  }
  @override Future<AutomationJson> generateAi(String reviewId) async {
    generations++;
    return {'version': 1, 'description': 'Repair proposal',
      'files': [{'path': 'playbook.yml', 'content': '---\n# corrected draft\n- hosts: all\n  tasks: []\n'}],
      'variables': <AutomationJson>[], 'dependencies': <AutomationJson>[],
      'expected_changes': <String>[], 'checks': <String>[], 'warnings': <String>[]};
  }
  @override Future<void> discardAi(String reviewId) async {}
  @override Future<AutomationJson> confirmRun(String reviewId) async { confirmations++; return {}; }
  @override Future<AutomationJson> jobEvents(String id, {required int after, int limit = 50}) async =>
    pendingPage?.future ?? {'job_id': id, 'manifest_digest': f.digest, 'cursor': after + 2,
      'output_truncated': false, 'events': [
        for (var n = 1; n <= 2; n++) {'version': 1, 'job_id': id, 'sequence': after + n,
          'timestamp': '2026-11-13T00:00:00Z', 'type': 'runner_on_failed',
          'payload': {'host': 'approved', 'task': n == 1 ? 'Configure service' : 'Hidden task',
            'task_action': 'ansible.builtin.service', 'res': n == 1
              ? {'msg': 'service does not exist', 'rc': 5, 'changed': false, 'private_extra': 'DO-NOT-SEND'}
              : {'_ansible_no_log': true, 'msg': 'SECRET-NOLOG', 'rc': 90}}},
      ]};
}

Future<(Api, ProviderContainer)> open(WidgetTester tester, {Api? api}) async {
  final service = api ?? Api(), backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Repair fixture');
  service.profile = backend.services.profiles.currentProfiles.active!.id.value;
  await pumpApp(tester, backend, overrides: [
    automationServiceProvider.overrideWithValue(service),
    aiProvidersProvider.overrideWith((ref) => Stream.value([service.provider])),
  ]);
  final container = ProviderScope.containerOf(tester.element(find.byType(MaterialApp).first));
  container.read(routerProvider).go(AppRoutes.automation);
  await settle(tester);
  await tapKey(tester, 'automation-workspace-jobs');
  return (service, container);
}
Future<void> suggest(WidgetTester tester) async {
  expect(find.byKey(ValueKey('automation-repair-${f.uuid(100)}')), findsOneWidget,
    reason: tester.allWidgets.whereType<Text>().map((v) => v.data).join(' | '));
  await tapKey(tester, 'automation-repair-${f.uuid(100)}');
}
Future<void> preview(WidgetTester tester) async {
  final scrollable = find.descendant(of: find.byType(AutomationAiDialog), matching: find.byType(Scrollable)).first;
  tester.state<ScrollableState>(scrollable).position.jumpTo(0);
  await tester.pump();
  await tester.tap(find.byType(GlassSelect<String?>).last);
  await settle(tester);
  await tester.tap(find.text('AI repair fixture · fixture').last);
  await settle(tester);
  await tapKey(tester, 'automation-ai-preview');
}

void main() {
  test('diagnostic projection is closed, bounded and no_log precedes all result fields', () {
    expect(automationRepairDiagnostic({'sequence': 7, 'type': 'runner_on_failed', 'censored': true,
      'task': 'SECRET', 'error': 'SECRET', 'rc': 4}),
      {'sequence': 7, 'status': 'runner_on_failed', 'censored': true});
    final selected = automationRepairDiagnostic({'sequence': 1, 'type': 'runner_on_failed',
      'error': '\u001b[31mfailure\u001b[0m', 'endpoint': 'SECRET', 'stdout': 'SECRET'});
    expect(selected['error'], 'failure');
    expect(selected.toString(), isNot(contains('SECRET')));
  });
  testWidgets('failed job opens current playbook without send; explicit selected diagnostics only then draft Save and fresh Run', (tester) async {
    final (api, _) = await open(tester);
    final original = api.text;
    await suggest(tester);
    expect(api.previews, 0); expect(api.generations, 0); expect(api.saves, 0);
    expect(find.byType(AutomationAiDialog), findsOneWidget);
    await tester.scrollUntilVisible(find.byKey(const ValueKey('automation-repair-load-events')), 200,
      scrollable: find.descendant(of: find.byType(AutomationAiDialog), matching: find.byType(Scrollable)).first);
    await tapKey(tester, 'automation-repair-load-events');
    await tester.scrollUntilVisible(find.byKey(const ValueKey('automation-repair-event-2')), 150,
      scrollable: find.descendant(of: find.byType(AutomationAiDialog), matching: find.byType(Scrollable)).first);
    final hidden = tester.widget<CheckboxListTile>(find.byKey(const ValueKey('automation-repair-event-2')));
    expect(hidden.onChanged, isNull);
    expect(find.textContaining('SECRET-NOLOG'), findsNothing);
    await tapKey(tester, 'automation-repair-event-1');
    await preview(tester);
    expect(api.task, contains('service does not exist'));
    expect(api.task, isNot(contains('SECRET-NOLOG')));
    expect(api.task, isNot(contains('DO-NOT-SEND')));
    expect(api.generations, 0);
    await tapKey(tester, 'automation-ai-generate');
    expect(api.text, original); expect(api.saves, 0); expect(api.confirmations, 0);
    await tapKey(tester, 'automation-ai-use');
    await tapKey(tester, 'automation-save');
    expect(api.saves, 1); expect(api.revision, 2); expect(api.savedExpected, 'file-1');
    await tapKey(tester, 'automation-open-run');
    final run = tester.widget<AutomationRunDialog>(find.byType(AutomationRunDialog));
    expect(run.retryOf?['job_id'], f.uuid(100));
    expect(find.byKey(const ValueKey('automation-retry-prior-results')), findsOneWidget);
    expect(find.textContaining('changes may already have been applied'), findsWidgets);
    expect(api.confirmations, 0);
  });
  testWidgets('without diagnostic selection preview includes no journal task/error', (tester) async {
    final (api, _) = await open(tester);
    await suggest(tester);
    await tester.scrollUntilVisible(find.byKey(const ValueKey('automation-repair-load-events')), 200,
      scrollable: find.descendant(of: find.byType(AutomationAiDialog), matching: find.byType(Scrollable)).first);
    await tapKey(tester, 'automation-repair-load-events');
    await preview(tester);
    expect(api.task, isNot(contains('service does not exist')));
    expect(api.task, isNot(contains('Configure service')));
    expect(api.saves, 0); expect(api.confirmations, 0);
  });
  testWidgets('pending current-file read then Terminal hide/return cannot open a late repair dialog', (tester) async {
    final api = Api()..pendingRead = Completer<AutomationProjectFile>();
    final (_, container) = await open(tester, api: api);
    await suggest(tester);
    expect(api.reads, 1);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    api.pendingRead!.complete(const AutomationProjectFile(path: 'playbook.yml', text: 'late', sha256: 'late', lineEndings: 'lf'));
    await settle(tester);
    expect(find.byType(AutomationAiDialog), findsNothing);
    expect(api.previews, 0); expect(api.saves, 0); expect(api.confirmations, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('pending diagnostic page cancelled by navigation never populates a new dialog', (tester) async {
    final api = Api()..pendingPage = Completer<AutomationJson>();
    final (_, container) = await open(tester, api: api);
    await suggest(tester);
    await tester.scrollUntilVisible(find.byKey(const ValueKey('automation-repair-load-events')), 200,
      scrollable: find.descendant(of: find.byType(AutomationAiDialog), matching: find.byType(Scrollable)).first);
    await tapKey(tester, 'automation-repair-load-events');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(find.byType(AutomationAiDialog), findsNothing);
    api.pendingPage!.complete({'job_id': f.uuid(100), 'manifest_digest': f.digest, 'cursor': 0,
      'output_truncated': false, 'events': <AutomationJson>[]});
    await settle(tester);
    expect(api.previews, 0); expect(api.saves, 0); expect(api.confirmations, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('dirty editor Cancel preserves the document and does not read or prepare a repair', (tester) async {
    final (api, _) = await open(tester);
    await tapKey(tester, 'automation-workspace-projects');
    await tapKey(tester, 'automation-project-${f.project}');
    await tapKey(tester, 'automation-file-playbook.yml');
    await enterKey(tester, 'automation-editor', '---\n# unsaved manual change\n');
    final reads = api.reads;
    await tapKey(tester, 'automation-workspace-jobs');
    await suggest(tester);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(api.reads, reads); expect(api.saves, 0); expect(api.previews, 0);
    await tapKey(tester, 'automation-workspace-projects');
    expect(tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      contains('unsaved manual change'));
  });
  testWidgets('same profile service replacement denies pending original repair file result', (tester) async {
    final api = Api()..pendingRead = Completer<AutomationProjectFile>();
    final (_, container) = await open(tester, api: api);
    await suggest(tester);
    final replacement = Api()..profile = api.profile;
    container.updateOverrides([
      appServicesProvider.overrideWithValue(container.read(appServicesProvider)),
      automationServiceProvider.overrideWithValue(replacement),
      aiProvidersProvider.overrideWith((ref) => Stream.value([api.provider])),
    ]);
    await settle(tester);
    api.pendingRead!.complete(const AutomationProjectFile(path: 'playbook.yml', text: 'late', sha256: 'late', lineEndings: 'lf'));
    await settle(tester);
    expect(find.byType(AutomationAiDialog), findsNothing);
    expect(api.previews, 0); expect(replacement.previews, 0);
    expect(api.saves, 0); expect(replacement.saves, 0);
    expect(tester.takeException(), isNull);
  });

}
