import 'dart:async';

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_portable_dialog.dart';
import 'package:consolecrypt/automation/automation_portable_host.dart';
import 'package:consolecrypt/automation/automation_portable_native_adapter.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart' show testBackend, settle;
import '../unit/automation_portable_controller_test.dart' as f;
import '../unit/automation_portable_native_adapter_test.dart' as g;
import 'automation_portable_dialog_test.dart' show labels;

class Projects extends Fake implements AutomationService {}

class Gateway extends g.Gateway {
  Gateway(this.profile);
  final String profile;
  bool autoReply = true;
  int passwordCopies = 0;
  Map<String, dynamic> get nativeScope => {'version': 1, 'profileId': profile, 'notificationScopeId': f.nonce};
  @override
  Future<Map<String, dynamic>> captureScope() async {
    captures++;
    return scopeWait?.future ?? nativeScope;
  }

  PortableNativeInvocation invocation(g.Call call, Map<String, dynamic> response) => PortableNativeInvocation(
    registered: call.registration,
    startBody: () {
      call.invocation.startBody();
      if (autoReply) {
        call.result.complete(response);
        call.joined.complete();
      }
    },
    result: call.result.future,
    joined: call.joined.future,
  );
  @override
  PortableNativeInvocation context({
    required PortableScope scope,
    required String operationId,
    required String projectId,
    required int projectRevision,
  }) {
    final call = start(scope, operationId, 'context');
    return invocation(call, f.contextJson()..['scope'] = nativeScope);
  }

  @override
  PortableNativeInvocation export({
    required PortableScope scope,
    required String operationId,
    required Map<String, dynamic> request,
    required SecretText password,
    required String destination,
  }) {
    // Fake only verifies that transport input is available synchronously. A
    // production gateway must own a separate bounded copy until actual ACK.
    expect(password.expose(), 'synthetic package password');
    passwordCopies++;
    expect(request['scope'], nativeScope);
    expect(destination, '/owned/new.ccproject');
    return invocation(start(scope, operationId, 'export'), {'version': 1, 'encryptedSha256': 'c' * 64, 'bytes': 1000});
  }

  @override
  PortableNativeInvocation inspect({
    required PortableScope scope,
    required String operationId,
    required String source,
    required SecretText password,
  }) {
    expect(password.expose(), 'synthetic package password');
    passwordCopies++;
    expect(source, '/owned/input.ccproject');
    return invocation(start(scope, operationId, 'inspect'), {
      'version': 1,
      'encryptedSha256': 'c' * 64,
      'summary': {'sourceDigest': 'a' * 64, 'portableDigest': 'd' * 64, 'files': 1, 'bytes': 24, 'exclusions': 1},
      'publicationSupported': false,
      'reason': 'managed_publication_required',
    });
  }

  @override
  Future<void> cancel({required PortableScope scope, required String operationId}) async {
    await super.cancel(scope: scope, operationId: operationId);
    for (final call in calls.where((c) => c.id == operationId)) {
      if (!call.joined.isCompleted) call.joined.complete();
      if (call.bodyStarts > 0 && !call.result.isCompleted) {
        call.result.completeError(const PortablePackageFailure('portable_context_changed'));
      }
    }
  }
}

Future<(ProviderContainer, Gateway, RuntimeNoticeEventAdapter, f.Files, ValueNotifier<bool>)> open(
  WidgetTester tester, {
  bool importMode = false,
  bool available = true,
  bool delayedScope = false,
  bool delayedRegistration = false,
}) async {
  final backend = testBackend(seed: false);
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned portable host');
  final profile = backend.services.profiles.currentProfiles.active!.id.value;
  final scope = AutomationScope(profile, true),
      service = Projects(),
      hub = RuntimeNoticeEventAdapter(),
      files = f.Files(),
      source = ValueNotifier(true),
      gateway = Gateway(profile);
  if (delayedScope) gateway.scopeWait = Completer<Map<String, dynamic>>();
  if (delayedRegistration) gateway.registrationWait = Completer<void>();
  addTearDown(hub.dispose);
  addTearDown(source.dispose);
  late final GoRouter router;
  router = GoRouter(
    initialLocation: AppRoutes.automation,
    routes: [
      GoRoute(
        path: AppRoutes.automation,
        builder: (_, _) => Scaffold(
          body: Consumer(
            builder: (context, ref, _) {
              ref.watch(activeProfileProvider);
              return TextButton(
                key: const ValueKey('portable-host-open'),
                onPressed: () => showAutomationPortableOperation(
                  context,
                  ref,
                  originalScope: scope,
                  originalService: service,
                  sourceCurrent: () => source.value,
                  sourceChanges: source,
                  labels: PortableHostLabels(dialog: labels, preparing: 'Preparing review', unavailable: 'Unavailable'),
                  importMode: importMode,
                  gateway: available ? gateway : null,
                  projectId: importMode ? null : f.project,
                  projectRevision: importMode ? null : 1,
                ),
                child: const Text('Open package action'),
              );
            },
          ),
        ),
      ),
      GoRoute(
        path: AppRoutes.terminal,
        builder: (_, _) => const Scaffold(body: Text('Existing terminal surface')),
      ),
    ],
  );
  addTearDown(router.dispose);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        automationScopeProvider.overrideWithValue(scope),
        automationServiceProvider.overrideWithValue(service),
        criticalRuntimeEventsProvider.overrideWithValue(hub),
        fileDialogServiceProvider.overrideWithValue(files),
        routerProvider.overrideWithValue(router),
      ],
      child: MaterialApp.router(
        routerConfig: router,
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      ),
    ),
  );
  await settle(tester);
  final container = ProviderScope.containerOf(tester.element(find.byKey(const ValueKey('portable-host-open'))));
  return (container, gateway, hub, files, source);
}

Future<void> tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.tap(finder);
  await settle(tester);
}

Future<void> password(WidgetTester tester, {bool confirm = false}) async {
  await tester.enterText(
    find.descendant(of: find.byKey(const ValueKey('portable-password')), matching: find.byType(EditableText)),
    'synthetic package password',
  );
  if (confirm) {
    await tester.enterText(
      find.descendant(of: find.byKey(const ValueKey('portable-confirm-password')), matching: find.byType(EditableText)),
      'synthetic package password',
    );
  }
}

void main() {
  testWidgets(
    'explicit menu-action host captures only on click; missing gateway is framed unavailable with no fallback',
    (tester) async {
      final (_, gateway, _, files, _) = await open(tester, available: false);
      expect(gateway.captures, 0);
      await tap(tester, 'portable-host-open');
      expect(find.byKey(const ValueKey('portable-capture-failure')), findsOneWidget);
      expect(gateway.captures, 0);
      expect(gateway.calls, isEmpty);
      expect(files.opens, 0);
      expect(files.saves, 0);
      await tap(tester, 'portable-capture-cancel');
      expect(find.byKey(const ValueKey('portable-host-open')), findsOneWidget);
    },
  );
  testWidgets('cancel pending initial capture denies late Native scope and never opens file picker or context body', (
    tester,
  ) async {
    final (_, gateway, _, files, _) = await open(tester, delayedScope: true);
    await tap(tester, 'portable-host-open');
    expect(find.byKey(const ValueKey('portable-capture-pending')), findsOneWidget);
    await tap(tester, 'portable-capture-cancel');
    gateway.scopeWait!.complete(gateway.nativeScope);
    await settle(tester);
    expect(gateway.calls, isEmpty);
    expect(files.saves, 0);
    expect(files.opens, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('original hub epoch loss during capture cannot adopt same-profile reopened nonce', (tester) async {
    final (_, gateway, hub, files, _) = await open(tester, delayedScope: true);
    await tap(tester, 'portable-host-open');
    hub.boundary();
    gateway.scopeWait!.complete({
      ...gateway.nativeScope,
      'notificationScopeId': '44444444-4444-4444-8444-444444444444',
    });
    await settle(tester);
    expect(find.byType(AutomationPortableDialog), findsNothing);
    expect(gateway.calls, isEmpty);
    expect(files.saves, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('route hide before registered ACK cancels original read and denies late body; terminal remains', (
    tester,
  ) async {
    final (container, gateway, _, _, _) = await open(tester, delayedRegistration: true);
    await tap(tester, 'portable-host-open');
    expect(gateway.calls.single.bodyStarts, 0);
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    gateway.registrationWait!.complete();
    await settle(tester);
    expect(gateway.calls.single.bodyStarts, 0);
    expect(gateway.cancels.every((x) => x.scope.profileId == gateway.profile), isTrue);
    expect(find.text('Existing terminal surface'), findsOneWidget);
    expect(find.byType(AutomationPortableDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('source request retirement before registered ACK cancels only original operation', (tester) async {
    final (_, gateway, _, files, source) = await open(tester, delayedRegistration: true);
    await tap(tester, 'portable-host-open');
    source.value = false;
    await settle(tester);
    gateway.registrationWait!.complete();
    await settle(tester);
    expect(gateway.calls.single.bodyStarts, 0);
    expect(gateway.cancels, isNotEmpty);
    expect(files.saves, 0);
    expect(find.byType(AutomationPortableDialog), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('domain gap retains original source review and does not auto-pick export files', (tester) async {
    final (_, gateway, hub, files, _) = await open(tester);
    await tap(tester, 'portable-host-open');
    hub.streamLost();
    await settle(tester);
    expect(find.byType(AutomationPortableDialog), findsOneWidget);
    expect(gateway.calls.single.kind, 'context');
    expect(gateway.cancels, isEmpty);
    expect(files.saves, 0);
    expect(files.opens, 0);
  });
  testWidgets('explicit inspect chooses encrypted input only after package password and returns readonly summary', (
    tester,
  ) async {
    final (_, gateway, _, files, _) = await open(tester, importMode: true);
    await tap(tester, 'portable-host-open');
    expect(gateway.calls, isEmpty);
    expect(files.opens, 0);
    await password(tester);
    await tap(tester, 'portable-submit');
    expect(files.opens, 1);
    expect(files.saves, 0);
    expect(gateway.passwordCopies, 1);
    expect(gateway.calls.map((c) => c.kind), ['inspect']);
    expect(find.text(labels.publicationBlocked), findsOneWidget);
    expect(find.byKey(const ValueKey('portable-exported')), findsNothing);
  });
  testWidgets(
    'whole project export still requires explicit submit and the trusted save picker',
    (tester) async {
      final (_, gateway, _, files, _) = await open(tester);
      await tap(tester, 'portable-host-open');
      expect(files.saves, 0);
      await password(tester, confirm: true);
      await tap(tester, 'portable-submit');
      expect(files.saves, 1);
      expect(files.finishes, 1);
      expect(gateway.passwordCopies, 1);
      expect(gateway.calls.map((c) => c.kind), ['context', 'export']);
      expect(find.byKey(const ValueKey('portable-exported')), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );
}
