import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_agent_installer_dialog.dart';
import 'package:consolecrypt/automation/automation_dependency_dialog.dart'
    show DependencyOriginalGuard;
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';
import '../support/agent_installer_fixture.dart' as f;

class _Api extends Fake implements AutomationAgentInstallerService {
  int reads = 0;
  final requests = <AutomationJson>[];
  Completer<AutomationJson>? pending;
  AppException? failure;
  @override
  Future<AutomationJson> agentInstallerContext(AutomationJson frame) async {
    reads++;
    expect(frame, {
      'version': 1,
      'profileId': f.profileId,
      'notificationScopeId': f.nonce,
    });
    return f.context();
  }

  @override
  Future<AutomationJson> createAgentInstallerProject(
    AutomationJson request,
  ) async {
    requests.add(jsonDecode(jsonEncode(request)) as AutomationJson);
    if (failure case final error?) throw error;
    return pending?.future ?? f.created(request);
  }
}

class _Files extends Fake implements FileDialogService {
  Completer<String?>? pending;
  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async =>
      pending?.future ?? 'D:/public/candidate.${extensions.single}';
}

class _Harness {
  final api = _Api(),
      files = _Files(),
      events = RuntimeNoticeEventAdapter(),
      live = ValueNotifier(true);
  final scope = const AutomationScope(f.profileId, true);
  AutomationProject? selected;
  Future<void> pump(
    WidgetTester tester, {
    Locale locale = const Locale('en'),
    double scale = 1,
  }) async {
    events.acceptScope(
      originalEpoch: events.epoch,
      nativeProfile: {
        'id': f.profileId,
        'active': true,
        'vault_state': 'unlocked',
        'notification_scope_id': f.nonce,
      },
    );
    final backend = testBackend();
    addTearDown(backend.dispose);
    addTearDown(events.dispose);
    addTearDown(live.dispose);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          appServicesProvider.overrideWithValue(backend.services),
          automationScopeProvider.overrideWithValue(scope),
          criticalRuntimeEventsProvider.overrideWithValue(events),
          fileDialogServiceProvider.overrideWithValue(files),
        ],
        child: MaterialApp(
          locale: locale,
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [
            AppLocalizations.delegate,
            ...GlobalMaterialLocalizations.delegates,
          ],
          builder: (context, child) => MediaQuery(
            data: MediaQuery.of(context)
                .copyWith(textScaler: TextScaler.linear(scale)),
            child: child!,
          ),
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                key: const ValueKey('open-installer'),
                onPressed: () async {
                  selected = await showGlassDialog<AutomationProject>(
                    context,
                    builder: (_) => DependencyOriginalGuard(
                      original: live,
                      builder: (_) => AutomationAgentInstallerDialog(
                        scope: scope,
                        service: api,
                        originalCurrent: () => live.value,
                        originalLive: live,
                      ),
                    ),
                  );
                },
                child: const Text('Open'),
              ),
            ),
          ),
        ),
      ),
    );
    await tapKey(tester, 'open-installer');
  }

  Future<void> fill(WidgetTester tester) async {
    await tester.enterText(
      find.byKey(const ValueKey('agent-installer-name')),
      'Owned deployment',
    );
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester);
    await tapKey(tester, 'agent-installer-host');
    await tester.tap(find.text('Existing SSH source target').last);
    await settle(tester);
    await tapKey(tester, 'agent-installer-artifact');
    await tapKey(tester, 'agent-installer-manifest');
  }
}

void main() {
  testWidgets(
    'spent source creation preserves coded vault alarm alongside unknown outcome withoutretry',
    (tester) async {
      final h = _Harness();
      await h.pump(tester);
      await h.fill(tester);
      h.api.failure = const AppException(
        AppErrorCode.vaultKeyMismatch,
        'PRIVATE RAW must never render',
      );
      await tapKey(tester, 'agent-installer-create');
      final l = AppLocalizations.of(
        tester.element(find.byKey(const ValueKey('agent-installer-dialog'))),
      );
      expect(find.text(l.errorVaultKeyMismatch), findsOneWidget);
      expect(find.text(l.agentInstallOutcomeUnknown), findsOneWidget);
      expect(find.textContaining('PRIVATE RAW'), findsNothing);
      expect(h.api.requests, hasLength(1));
      expect(
        tester
            .widget<TextButton>(
              find.byKey(const ValueKey('agent-installer-create')),
            )
            .onPressed,
        null,
      );
    },
  );

  testWidgets(
    'review only reads metadata and cancel creates no project or run',
    (tester) async {
      final h = _Harness();
      await h.pump(tester);
      expect(h.api.reads, 1);
      expect(h.api.requests, isEmpty);
      expect(find.textContaining('fixed Rust verifier'), findsOneWidget);
      expect(find.textContaining('six Agent scopes'), findsOneWidget);
      await tapKey(tester, 'agent-installer-cancel');
      expect(h.api.requests, isEmpty);
      expect(h.selected, null);
    },
  );
  testWidgets(
    'explicit source creation freezes actual host and public file choices; runtime remains unknown',
    (tester) async {
      final h = _Harness();
      await h.pump(tester);
      await h.fill(tester);
      expect(h.api.requests, isEmpty);
      await tapKey(tester, 'agent-installer-create');
      expect(h.api.requests, hasLength(1));
      final r = h.api.requests.single;
      expect(r['hostId'], f.hostId);
      expect(r['expectedHostRevision'], '9007199254740993');
      expect(r['start'], false);
      expect(r['artifactPath'], 'D:/public/candidate.zip');
      expect(r['manifestPath'], 'D:/public/candidate.json');
      expect(
        find.textContaining('Actual running Agent version: Unknown'),
        findsOneWidget,
      );
      expect(find.textContaining('not been verified here'), findsOneWidget);
      await tapKey(tester, 'agent-installer-open');
      expect(h.selected?.id, f.projectId);
      expect(h.api.requests, hasLength(1));
    },
  );
  testWidgets(
    'owner retires while public picker is pending: late file causes zero creation',
    (tester) async {
      final h = _Harness();
      await h.pump(tester);
      h.files.pending = Completer<String?>();
      await tapKey(tester, 'agent-installer-artifact');
      h.live.value = false;
      await settle(tester);
      h.files.pending!.complete('D:/public/late.zip');
      await settle(tester);
      expect(h.api.requests, isEmpty);
      expect(h.selected, null);
    },
  );
  testWidgets(
    'a dispatched create never auto-retries or publishes into retired route',
    (tester) async {
      final h = _Harness();
      await h.pump(tester);
      await h.fill(tester);
      h.api.pending = Completer<AutomationJson>();
      await tapKey(tester, 'agent-installer-create');
      expect(h.api.requests, hasLength(1));
      h.live.value = false;
      await settle(tester);
      h.api.pending!.complete(f.created(h.api.requests.single));
      await settle(tester);
      expect(h.selected, null);
      expect(h.api.requests, hasLength(1));
    },
  );
  testWidgets('Russian bootstrap disclosure fits narrow enlarged layout', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(500, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final h = _Harness();
    await h.pump(tester, locale: const Locale('ru'), scale: 1.5);
    expect(find.textContaining('Администратор целевого хоста'), findsOneWidget);
    expect(tester.takeException(), null);
    expect(h.api.requests, isEmpty);
  });
}
