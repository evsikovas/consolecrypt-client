import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_ai_context_picker.dart';
import 'package:consolecrypt/automation/automation_ai_dialog.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/ai_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart' show settle, tapKey;
import '../unit/automation_ai_context_test.dart' as f;

class Port extends Fake implements AutomationAiContextService, AutomationAiService, AutomationInventoryService {
  final requests = <AutomationJson>[], cancelled = <String>[], discarded = <String>[];
  Completer<AutomationJson>? pending;
  int metadataReads = 0;
  @override
  Future<AutomationJson> captureAiContext({
    required String operationId,
    required String profileId,
    required String notificationScopeId,
    required String projectId,
    required int expectedProjectRevision,
    required AutomationAiContextKind kind,
    required String subjectId,
  }) async {
    requests.add({
      'operationId': operationId,
      'profileId': profileId,
      'notificationScopeId': notificationScopeId,
      'projectId': projectId,
      'expectedProjectRevision': expectedProjectRevision,
      'kind': kind.name,
      'subjectId': subjectId,
    });
    return pending?.future ?? response(kind: kind, subject: subjectId);
  }

  AutomationJson response({AutomationAiContextKind kind = AutomationAiContextKind.hostOs, String? subject}) {
    final now = DateTime.now().toUtc();
    return f.capture(runner: kind == AutomationAiContextKind.runnerVersions)
      ..['subjectId'] = subject ?? f.id(5)
      ..['capturedUtc'] = now.toIso8601String()
      ..['expiresUtc'] = now.add(const Duration(seconds: 120)).toIso8601String();
  }

  @override
  Future<void> cancelAiContext(String operationId) async {
    cancelled.add(operationId);
  }

  @override
  Future<void> discardAi(String reviewId) async {
    discarded.add(reviewId);
  }

  @override
  Future<AutomationJson> inventory(String projectId) async {
    metadataReads++;
    return {
      'version': 1,
      'profile_id': f.id(2),
      'notification_scope_id': f.id(3),
      'project_id': projectId,
      'project_revision': 7,
      'inventory': {
        'version': 1,
        'groups': {
          'web': [f.id(5)],
        },
      },
    };
  }
}

class DialogPort extends Port {
  int legacy = 0;
  AutomationJson? prepared;
  @override
  Future<AutomationJson> prepareAi({
    required String providerId,
    required String task,
    required String path,
    String? selectedText,
  }) async {
    legacy++;
    return preview();
  }

  AutomationJson preview() => {
    'review_id': f.id(20),
    'provider_name': 'Context fixture',
    'locality': 'public',
    'preview': 'Sanitized selected context',
  };
  @override
  Future<AutomationJson> prepareAiWithContext({
    required String profileId,
    required String notificationScopeId,
    required String projectId,
    required int expectedProjectRevision,
    String? selectedHostId,
    required String providerId,
    required String task,
    required String path,
    String? selectedText,
    required List<AutomationJson> selections,
    required List<String> selectedGroups,
    required List<String> selectedTags,
  }) async {
    prepared = {
      'profileId': profileId,
      'notificationScopeId': notificationScopeId,
      'projectId': projectId,
      'expectedProjectRevision': expectedProjectRevision,
      'selectedHostId': selectedHostId,
      'selections': selections,
      'selectedGroups': selectedGroups,
      'selectedTags': selectedTags,
    };
    return preview();
  }
}

class NoModelCalls extends Fake implements AiService {}

final host = Host(
  id: ObjectId(f.id(5)),
  name: 'Chosen Linux host',
  address: '192.0.2.1',
  createdAt: DateTime.utc(2026),
  updatedAt: DateTime.utc(2026),
  tags: const ['web-tag'],
);
final runner = AutomationRunnerProfile(id: f.id(6), name: 'Chosen Runner', readOnly: false, revision: 9);

class Harness {
  Port port = Port();
  final selections = <(AutomationJson?, bool)>[], busy = <bool>[];
  bool current = true;
  String nonce = f.id(3);
  Widget child() => MaterialApp(
    theme: AppTheme.dark(platform: TargetPlatform.windows),
    locale: const Locale('en'),
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
    home: Scaffold(
      body: SingleChildScrollView(
        child: AutomationAiContextPicker(
          key: const ValueKey('picker'),
          service: port,
          ai: port,
          inventory: port,
          projectId: f.id(4),
          profileId: f.id(2),
          notificationScopeId: nonce,
          initialProjectRevision: 7,
          hosts: [host],
          runners: [runner],
          isCurrent: () => current,
          enabled: true,
          onSelection: selections.add,
          onBusy: busy.add,
        ),
      ),
    ),
  );
}

Future<void> chooseHost(WidgetTester tester) async {
  await tester.tap(find.byKey(const ValueKey('automation-ai-context-host')));
  await settle(tester);
  await tester.tap(find.text('Chosen Linux host').last);
  await settle(tester);
}

Future<void> captureHost(WidgetTester tester) async {
  await chooseHost(tester);
  await tapKey(tester, 'automation-ai-context-capture-host');
}

void main() {
  testWidgets(
    'opening/selection performs no capture; explicit capture keeps fields unchecked and passes original Native frame/revision',
    (tester) async {
      final h = Harness();
      await tester.pumpWidget(h.child());
      await settle(tester);
      expect(h.port.requests, isEmpty);
      expect(h.port.metadataReads, 0);
      await chooseHost(tester);
      expect(h.port.requests, isEmpty);
      await tapKey(tester, 'automation-ai-context-capture-host');
      expect(h.port.requests.single, containsPair('notificationScopeId', f.id(3)));
      expect(h.port.requests.single, containsPair('profileId', f.id(2)));
      expect(h.port.requests.single, containsPair('expectedProjectRevision', 7));
      expect(h.selections.last.$1, isNull);
      final key = find.byKey(const ValueKey('automation-ai-context-field-hostOs-osId'));
      await tester.ensureVisible(key);
      await tester.pump();
      await tester.tap(key);
      await settle(tester);
      expect(h.selections.last.$1!['selections'], [
        {
          'captureId': f.id(1),
          'fieldIds': ['osId'],
        },
      ]);
      expect(h.selections.last.$1.toString(), isNot(contains('debian')));
      expect(find.textContaining('Received (UTC)'), findsOneWidget);
      expect(find.textContaining('Expires (UTC)'), findsOneWidget);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    },
  );
  testWidgets('cancel pending capture holds actual future/busy; late Ready discarded and never selected', (
    tester,
  ) async {
    final h = Harness();
    h.port.pending = Completer<AutomationJson>();
    await tester.pumpWidget(h.child());
    await settle(tester);
    await captureHost(tester);
    final op = h.port.requests.single['operationId'];
    await tapKey(tester, 'automation-ai-context-cancel-capture');
    expect(h.port.cancelled, [op]);
    expect(h.busy.last, true);
    expect(
      tester.widget<TextButton>(find.byKey(const ValueKey('automation-ai-context-capture-host'))).onPressed,
      isNull,
    );
    h.port.pending!.complete(h.port.response());
    await settle(tester);
    expect(h.busy.last, false);
    expect(h.port.discarded, [f.id(1)]);
    expect(h.selections.every((value) => value.$1 == null), true);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });
  testWidgets('replacement retires original owner and cancels/discards original pending capture only', (tester) async {
    final h = Harness();
    final original = h.port..pending = Completer<AutomationJson>();
    await tester.pumpWidget(h.child());
    await settle(tester);
    await captureHost(tester);
    h.port = Port();
    await tester.pumpWidget(h.child());
    await settle(tester);
    expect(original.cancelled, [original.requests.single['operationId']]);
    expect(h.port.cancelled, isEmpty);
    original.pending!.complete(original.response());
    await settle(tester);
    expect(original.discarded, [f.id(1)]);
    expect(h.port.discarded, isEmpty);
    expect(h.port.requests, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('expiry invalidates selected refs with no recapture; explicit uncheck can omit expired context', (
    tester,
  ) async {
    final h = Harness();
    await tester.pumpWidget(h.child());
    await settle(tester);
    await captureHost(tester);
    await tapKey(tester, 'automation-ai-context-field-hostOs-osId');
    expect(h.selections.last.$2, true);
    await tester.pump(const Duration(seconds: 121));
    await tester.pump();
    expect(h.selections.last.$2, false);
    expect(h.port.requests.length, 1);
    await tapKey(tester, 'automation-ai-context-field-hostOs-osId');
    expect(h.selections.last, (null, true));
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });
  testWidgets('groups and saved tags are explicit metadata choices without OS capture', (tester) async {
    final h = Harness();
    await tester.pumpWidget(h.child());
    await settle(tester);
    await chooseHost(tester);
    await tapKey(tester, 'automation-ai-context-read-groups');
    expect(h.port.metadataReads, 1);
    expect(h.port.requests, isEmpty);
    await tapKey(tester, 'automation-ai-context-group-web');
    await tapKey(tester, 'automation-ai-context-tag-web-tag');
    expect(h.selections.last.$1!['selectedGroups'], ['web']);
    expect(h.selections.last.$1!['selectedTags'], ['web-tag']);
    expect(h.selections.last.$1!['selectedHostId'], host.id.value);
    expect(h.selections.last.$1!['expectedProjectRevision'], 7);
    expect(h.selections.last.$1!['selections'], isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });
  testWidgets('Runner probe versions and project pins have honest distinct labels and refs-only inclusion', (
    tester,
  ) async {
    final h = Harness();
    await tester.pumpWidget(h.child());
    await settle(tester);
    await tester.tap(find.byKey(const ValueKey('automation-ai-context-runner')));
    await settle(tester);
    await tester.tap(find.text('Chosen Runner').last);
    await settle(tester);
    expect(h.port.requests, isEmpty);
    await tapKey(tester, 'automation-ai-context-capture-runner');
    expect(h.port.requests.single['kind'], 'runnerVersions');
    expect(h.port.requests.single['subjectId'], f.id(6));
    expect(find.text('2.19.0'), findsOneWidget);
    expect(find.text('ansible.posix · 2.0.0'), findsOneWidget);
    expect(find.textContaining('not a list of every globally installed collection'), findsOneWidget);
    await tapKey(tester, 'automation-ai-context-field-runnerVersions-collections');
    expect(h.selections.last.$1!['selections'], [
      {
        'captureId': f.id(1),
        'fieldIds': ['collections'],
      },
    ]);
    expect(h.selections.last.$1.toString(), isNot(contains('ansible.posix')));
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });

  Future<void> dialog(WidgetTester tester, DialogPort port, {bool originalNonce = true}) async {
    final provider = AiProviderConfig.draft(AiProviderKind.openaiCompatible)
        .copyWith(name: 'Context fixture', chatModel: 'fixture');
    final scope = AutomationScope(ObjectId(f.id(2)), true);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          automationScopeProvider.overrideWithValue(scope),
          aiProvidersProvider.overrideWith((ref) => Stream.value([provider])),
          aiServiceProvider.overrideWithValue(NoModelCalls()),
          hostsProvider.overrideWith((ref) => Stream.value([host])),
          automationRunnersProvider.overrideWith(
            (ref) async => AutomationRunnerCatalog(readOnly: false, runners: [runner]),
          ),
        ],
        child: MaterialApp(
          theme: AppTheme.dark(platform: TargetPlatform.windows),
          locale: const Locale('en'),
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          home: Scaffold(
            body: AutomationAiDialog(
              scope: scope,
              service: port,
              path: 'playbook.yml',
              before: '---\n',
              projectId: f.id(4),
              projectRevision: 7,
              nativeContextScope: originalNonce
                  ? RuntimeNoticeScope.fromProfile({
                      'active': true,
                      'vault_state': 'unlocked',
                      'id': f.id(2),
                      'notification_scope_id': f.id(3),
                    })
                  : null,
            ),
          ),
        ),
      ),
    );
    await settle(tester);
  }

  testWidgets(
    'existing AI dialog sends group/tag refs only through contextual prepare, never generic fallback or capture',
    (tester) async {
      final port = DialogPort();
      await dialog(tester, port);
      final task = find.byKey(const ValueKey('automation-ai-task'));
      await tester.enterText(task, 'Explain the explicitly selected project context');
      await tester.tap(find.byType(GlassSelect<String?>).first);
      await settle(tester);
      await tester.tap(find.text('Context fixture · fixture').last);
      await settle(tester);
      final scroll = find.descendant(of: find.byType(AutomationAiDialog), matching: find.byType(Scrollable)).first;
      await tester.scrollUntilVisible(
        find.byKey(const ValueKey('automation-ai-selected-context')),
        200,
        scrollable: scroll,
      );
      await tapKey(tester, 'automation-ai-selected-context');
      await tapKey(tester, 'automation-ai-context-host');
      await tester.tap(find.text('Chosen Linux host').last);
      await settle(tester);
      await tapKey(tester, 'automation-ai-context-read-groups');
      await tapKey(tester, 'automation-ai-context-group-web');
      await tapKey(tester, 'automation-ai-context-tag-web-tag');
      expect(port.requests, isEmpty);
      expect(port.prepared, isNull);
      await tapKey(tester, 'automation-ai-preview');
      expect(port.legacy, 0);
      expect(port.prepared, {
        'profileId': f.id(2),
        'notificationScopeId': f.id(3),
        'projectId': f.id(4),
        'expectedProjectRevision': 7,
        'selectedHostId': host.id.value,
        'selections': <AutomationJson>[],
        'selectedGroups': ['web'],
        'selectedTags': ['web-tag'],
      });
      expect(find.text('Sanitized selected context'), findsOneWidget);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    },
  );
  testWidgets(
    'existing AI dialog without original Native nonce refuses contextual capture instead of adopting latest scope',
    (tester) async {
      final port = DialogPort();
      await dialog(tester, port, originalNonce: false);
      await tester.scrollUntilVisible(
        find.byKey(const ValueKey('automation-ai-context-unavailable')),
        200,
        scrollable: find.descendant(of: find.byType(AutomationAiDialog), matching: find.byType(Scrollable)).first,
      );
      expect(find.byKey(const ValueKey('automation-ai-context-unavailable')), findsOneWidget);
      expect(find.byType(AutomationAiContextPicker), findsNothing);
      expect(port.requests, isEmpty);
      expect(port.metadataReads, 0);
      expect(port.prepared, isNull);
      expect(port.legacy, 0);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    },
  );
}
