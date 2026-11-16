import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart' show DependencyOriginalGuard;
import 'package:consolecrypt/automation/automation_deployment_environment_selection.dart';
import 'package:consolecrypt/automation/automation_project_credentials.dart';
import 'package:consolecrypt/automation/automation_project_credentials_dialog.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import '../support/project_environment_fixture.dart' as f;

class _Api extends Fake implements AutomationService, AutomationProjectCredentialsService {
  bool configured = true;
  Exception? readFailure, saveFailure;
  AutomationJson? contextOverride;
  int reads = 0;
  final saves = <AutomationJson>[];
  Completer<AutomationJson>? pending;
  @override
  Future<AutomationJson> projectCredentialsContext(String projectId) async {
    expect(projectId, f.projectId);
    reads++;
    if (readFailure != null) throw readFailure!;
    return pending?.future ?? contextOverride ?? f.context(configured: configured);
  }

  @override
  Future<AutomationJson> saveProjectCredentials(AutomationJson request) async {
    if (saveFailure != null) throw saveFailure!;
    saves.add(jsonDecode(jsonEncode(request)) as AutomationJson);
    return {
      'version': 1,
      'project': {'id': f.projectId, 'name': 'Owned project', 'read_only': false, 'revision': 2},
      'frame': f.frame(revision: 2),
      'policy': request['policy'],
    };
  }
}

class _Harness {
  final api = _Api(), live = ValueNotifier(true), events = RuntimeNoticeEventAdapter();
  final scope = const AutomationScope(f.profileId, true);
  AutomationProject? saved;
  final host = Host(
    id: const ObjectId(f.hostId),
    name: 'Reference SSH',
    address: 'metadata.example.test',
    createdAt: DateTime.utc(2024),
    updatedAt: DateTime.utc(2024),
  );
  final credential = Credential(
    id: const ObjectId(f.credentialId),
    name: 'Existing password',
    kind: CredentialKind.password,
    createdAt: DateTime.utc(2024),
    updatedAt: DateTime.utc(2024),
  );
  void establish() => events.acceptScope(
    originalEpoch: events.epoch,
    nativeProfile: {'id': f.profileId, 'active': true, 'vault_state': 'unlocked', 'notification_scope_id': f.nonce},
  );
  Future<void> pump(
    WidgetTester tester, {
    bool selector = false,
    void Function(bool, ProjectCredentialsContext?, String?, bool Function())? changed,
  }) async {
    establish();
    final backend = testBackend();
    addTearDown(backend.dispose);
    addTearDown(events.dispose);
    addTearDown(live.dispose);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          automationScopeProvider.overrideWithValue(scope),
          automationServiceProvider.overrideWithValue(api),
          criticalRuntimeEventsProvider.overrideWithValue(events),
          hostsProvider.overrideWith((ref) => Stream.value([host])),
          credentialsProvider.overrideWith(
            (ref) => Stream.value([
              credential,
              Credential(
                id: const ObjectId(f.projectId),
                name: 'Agent excluded',
                kind: CredentialKind.osSshAgent,
                createdAt: DateTime.utc(2024),
                updatedAt: DateTime.utc(2024),
              ),
            ]),
          ),
        ],
        child: MaterialApp(
          locale: const Locale('en'),
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          home: Scaffold(
            body: selector
                ? AutomationDeploymentEnvironmentSelection(
                    scope: scope,
                    project: f.project,
                    service: api,
                    onChanged: changed!,
                  )
                : Builder(
                    builder: (context) => TextButton(
                      key: const ValueKey('open-policy'),
                      onPressed: () async {
                        saved = await showGlassDialog<AutomationProject>(
                          context,
                          builder: (_) => DependencyOriginalGuard(
                            original: live,
                            builder: (_) => AutomationProjectCredentialsDialog(
                              scope: scope,
                              project: f.project,
                              service: api,
                              originalCurrent: () => live.value,
                              originalLive: live,
                            ),
                          ),
                        );
                      },
                      child: const Text('Open policy'),
                    ),
                  ),
          ),
        ),
      ),
    );
    await settle(tester);
    if (!selector) await tapKey(tester, 'open-policy');
  }
}

void main() {
  testWidgets('coded schema refusal is visible after the actual metadata read catch', (tester) async {
    final h = _Harness();
    h.api.readFailure = const AppException(
      AppErrorCode.validation,
      'DO_NOT_RENDER_PRIVATE_TOKEN',
      reason: AppErrorReason.invalidField,
      args: {'field': 'automation_credentials', 'rule': 'project_environment_schema_read_only'},
    );
    await h.pump(tester);
    expect(find.textContaining('unsupported schema and is read-only'), findsOneWidget);
    expect(find.textContaining('DO_NOT_RENDER'), findsNothing);
    expect(h.api.reads, 1);
    expect(h.api.saves, isEmpty);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('deployment-save'))).onPressed, isNull);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('coded prepare-busy refusal is actionable after the actual save catch without retry', (tester) async {
    final h = _Harness();
    h.api.saveFailure = const AppException(
      AppErrorCode.validation,
      'DO_NOT_RENDER_PRIVATE_TOKEN',
      reason: AppErrorReason.invalidField,
      args: {'field': 'automation_credentials', 'rule': 'project_environment_prepare_busy'},
    );
    await h.pump(tester);
    await tapKey(tester, 'deployment-save');
    expect(find.textContaining('Let it finish or explicitly cancel its review'), findsOneWidget);
    expect(find.textContaining('DO_NOT_RENDER'), findsNothing);
    await tester.pump(const Duration(seconds: 5));
    expect(h.api.saves, isEmpty);
    expect(h.api.reads, 1);
    expect(h.saved, isNull);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('removing project policy needs explicit warning consent and a separate save', (tester) async {
    final h = _Harness();
    await h.pump(tester);
    await tapKey(tester, 'deployment-remove-policy');
    expect(h.api.saves, isEmpty);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(h.api.saves, isEmpty);
    await tapKey(tester, 'deployment-remove-policy');
    await tapKey(tester, 'deployment-remove-accept');
    expect(h.api.saves, isEmpty);
    await tapKey(tester, 'deployment-save');
    expect(h.api.saves.single['policy'], isNull);
    expect(h.api.saves.single['frame'], f.frame());
    expect(h.saved?.revision, 2);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('future project policy stays unavailable and cannot be rewritten as legacy', (tester) async {
    final h = _Harness();
    h.api.contextOverride = f.context()..['policy'] = {'version': 2, 'secret': 'NOT_RENDERED'};
    await h.pump(tester);
    expect(find.textContaining('NOT_RENDERED'), findsNothing);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('deployment-save'))).onPressed, isNull);
    expect(h.api.reads, 1);
    expect(h.api.saves, isEmpty);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('settings metadata opening and cancel make no write or credential resolution', (tester) async {
    final h = _Harness();
    await h.pump(tester);
    expect(h.api.reads, 1);
    expect(h.api.saves, isEmpty);
    expect(find.text('Reference SSH'), findsOneWidget);
    expect(find.text('Existing password'), findsOneWidget);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(h.api.saves, isEmpty);
    expect(h.saved, isNull);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('editing a selected environment saves exact original frame and references only', (tester) async {
    final h = _Harness();
    await h.pump(tester);
    await tester.enterText(find.byKey(const ValueKey('deployment-name')), 'Reviewed environment');
    FocusManager.instance.primaryFocus?.unfocus();
    await tester.pumpAndSettle();
    await tapKey(tester, 'deployment-production');
    await tester.pump();
    await tapKey(tester, 'deployment-save');
    expect(h.api.saves, hasLength(1));
    expect(h.api.saves.single['frame'], f.frame());
    final env = (((h.api.saves.single['policy'] as Map)['environments'] as List).single as Map);
    expect(env['name'], 'Reviewed environment');
    expect(env['production'], true);
    expect(env['overrides'], [
      {'host_id': f.hostId, 'credential_id': f.credentialId},
    ]);
    expect(h.saved?.revision, 2);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('legacy project explicitly chooses an existing SSH credential with no secret-create entry', (
    tester,
  ) async {
    final h = _Harness()..api.configured = false;
    await h.pump(tester);
    expect(h.api.saves, isEmpty);
    await tapKey(tester, 'deployment-add-environment');
    await tester.enterText(find.byKey(const ValueKey('deployment-name')), 'New environment');
    FocusManager.instance.primaryFocus?.unfocus();
    await tester.pumpAndSettle();
    await tapKey(tester, 'deployment-add-reference');
    await tester.tap(find.text('Reference SSH'));
    await settle(tester);
    expect(find.text('Agent excluded'), findsNothing);
    expect(find.byKey(const ValueKey('picker-new-credential')), findsNothing);
    expect(h.api.saves, isEmpty);
    await tester.tap(find.byKey(const ValueKey('pick-credential-Existing password')));
    await settle(tester);
    await tapKey(tester, 'deployment-save');
    expect(h.api.saves, hasLength(1));
    expect(jsonEncode(h.api.saves.single), contains(f.credentialId));
    expect(jsonEncode(h.api.saves.single), isNot(contains('secret_id')));
    expect(h.host.credentialId, isNull);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('late context after original retirement never enables save or publishes a project', (tester) async {
    final h = _Harness();
    h.api.pending = Completer<AutomationJson>();
    await h.pump(tester);
    h.live.value = false;
    await settle(tester);
    h.api.pending!.complete(f.context());
    await settle(tester);
    expect(find.byKey(const ValueKey('deployment-save')), findsNothing);
    expect(h.api.saves, isEmpty);
    expect(h.saved, isNull);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('run environment requires explicit selection; original boundary retires it without another read', (
    tester,
  ) async {
    final h = _Harness();
    bool ready = false;
    String? selected;
    bool Function()? original;
    await h.pump(
      tester,
      selector: true,
      changed: (value, _, id, current) {
        ready = value;
        selected = id;
        original = current;
      },
    );
    expect(h.api.reads, 1);
    expect(h.api.saves, isEmpty);
    expect(ready, false);
    expect(selected, isNull);
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('deployment-run-selection'))).onChanged!(
      f.environmentId,
    );
    await tester.pump();
    expect(ready, true);
    expect(selected, f.environmentId);
    expect(original!(), true);
    h.events.boundary();
    await settle(tester);
    expect(ready, false);
    expect(original!(), false);
    expect(h.api.reads, 1);
    expect(h.api.saves, isEmpty);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
  testWidgets('legacy context leaves run selection absent without automatic save or prepare', (tester) async {
    final h = _Harness()..api.configured = false;
    bool ready = false;
    String? selected;
    await h.pump(
      tester,
      selector: true,
      changed: (value, _, id, _) {
        ready = value;
        selected = id;
      },
    );
    expect(ready, true);
    expect(selected, isNull);
    expect(find.byKey(const ValueKey('deployment-run-selection')), findsNothing);
    expect(h.api.reads, 1);
    expect(h.api.saves, isEmpty);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
}
