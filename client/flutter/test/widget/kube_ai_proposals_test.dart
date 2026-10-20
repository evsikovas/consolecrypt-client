import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/ai_service.dart';
import 'package:consolecrypt/core/services/snippet_service.dart';
import 'package:consolecrypt/kubernetes/kube_ai_panel.dart';
import 'package:consolecrypt/kubernetes/kube_ai_proposals.dart';
import 'package:consolecrypt/kubernetes/kube_ai_service.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_context_terminal_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_context_terminal_service.dart';
import 'package:consolecrypt/kubernetes/kube_create_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

final _scope = NotifierProvider<_Scope, KubeScope>(_Scope.new);

class _Scope extends Notifier<KubeScope> {
  @override
  KubeScope build() => const KubeScope('profile', true);
  void lock() => state = const KubeScope('profile', false);
}

const _session = KubeSession(id: 'session', contextId: 'context', serverVersion: 'v1', production: false);
const _deployment = KubeResourceKind(
  group: 'apps',
  version: 'v1',
  resource: 'deployments',
  kind: 'Deployment',
  namespaced: true,
);

class _Snippets implements SnippetService {
  final assessed = <String>[];
  @override
  Future<RiskAssessment> assessRisk(
    String command, {
    RiskLevel declared = RiskLevel.unknown,
    SnippetSource source = SnippetSource.user,
    SnippetType? dialect,
  }) async {
    expect(source, SnippetSource.ai);
    assessed.add(command);
    return const RiskAssessment(effective: RiskLevel.unknown, local: RiskLevel.readOnly, declared: RiskLevel.unknown);
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class _Service
    implements KubeAiService, KubeAiChatService, KubeContextTerminalService, KubeBrowserService, KubeManifestService {
  final questions = <String>[], followUps = <String>[], opened = <(String, String, String)>[];
  final providerIds = <String?>[];
  // Closed by each widget fixture's teardown.
  // ignore: close_sinks
  final events = StreamController<KubeAiEvent>.broadcast();
  int asked = 0, revoked = 0, creates = 0, discoveries = 0;
  KubeAiReview _review(int history) => KubeAiReview(
    ticket: 'ticket',
    providerName: 'External provider',
    contextName: 'team-context',
    external: true,
    toolSupport: true,
    historyMessages: history,
  );
  @override
  Future<KubeAiReview> prepareKubeAi(
    String sessionId, {
    String? providerId,
    required String question,
    KubeResourceRef? selected,
    bool configMapValues = false,
  }) async {
    questions.add(question);
    providerIds.add(providerId);
    return _review(0);
  }

  @override
  Future<KubeAiReview> prepareKubeFollowUp(
    String sessionId,
    String conversationId, {
    String? providerId,
    required String question,
    KubeResourceRef? selected,
    bool configMapValues = false,
  }) async {
    followUps.add(conversationId);
    return _review(2);
  }

  @override
  Stream<KubeAiEvent> askKubeAi(String sessionId, KubeAiReview review, {required bool approveExternal}) {
    expect(approveExternal, isTrue);
    asked++;
    return events.stream;
  }

  @override
  Future<void> revokeKubeAi(String sessionId) async => revoked++;
  @override
  Future<void> discardKubeAi(String sessionId, String ticket) async {}
  @override
  Future<KubeContextTerminalProfiles> contextTerminalProfiles() async => const KubeContextTerminalProfiles(
    profiles: [KubeContextTerminalProfile(id: 'macos:zsh', label: 'zsh', shell: 'zsh')],
    wslSupported: false,
  );
  @override
  Future<String> openContextTerminal(String sessionId, String namespace, String profileId) async {
    opened.add((sessionId, namespace, profileId));
    return 'native-terminal';
  }

  @override
  Future<List<KubeResourceKind>> discover(String sessionId) async {
    discoveries++;
    return [
      _deployment,
      const KubeResourceKind(group: '', version: 'v1', resource: 'secrets', kind: 'Secret', namespaced: true),
    ];
  }

  @override
  Future<KubeJson> resourceSchema(String sessionId, KubeResourceKind kind) async => {};
  @override
  Future<KubeResourceDetail> createResource(
    String sessionId,
    KubeResourceRef resource,
    SecretText manifest, {
    bool dryRun = true,
  }) async {
    creates++;
    return KubeResourceDetail(object: const {}, yaml: manifest.expose());
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<ProviderContainer> _pump(
  WidgetTester tester,
  Widget child,
  _Service service,
  _Snippets snippets, {
  List<AiProviderConfig> providers = const [],
}) async {
  tester.view.physicalSize = const Size(1100, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  addTearDown(service.events.close);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        kubeServiceProvider.overrideWithValue(const UnavailableKubeService()),
        kubeScopeProvider.overrideWith((ref) => ref.watch(_scope)),
        aiProvidersProvider.overrideWith((ref) => Stream.value(providers)),
        aiServiceProvider.overrideWithValue(_Ai()),
        snippetServiceProvider.overrideWithValue(snippets),
        kubeWriteAccessProvider(_session).overrideWith((ref) => const KubeWriteAccess(production: false)),
      ],
      child: MaterialApp(
        locale: const Locale('en'),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(body: child),
      ),
    ),
  );
  await _settle(tester);
  return ProviderScope.containerOf(tester.element(find.byType(Scaffold)));
}

Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 5; i++) {
    await tester.pump(const Duration(milliseconds: 200));
  }
}

Future<void> _ask(WidgetTester tester, String text) async {
  await tester.enterText(find.byKey(const ValueKey('kube-ai-question')), text);
  await _settle(tester);
  await tester.tap(find.byKey(const ValueKey('kube-ai-send')));
  await _settle(tester);
}

class _Ai implements AiService {
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  testWidgets('default provider follows the device-local model chosen on this computer', (tester) async {
    final service = _Service();
    final at = DateTime.utc(2026, 10, 20);
    await _pump(
      tester,
      KubeAiPanel(session: _session, contextName: 'team-context', service: service),
      service,
      _Snippets(),
      providers: [
        AiProviderConfig(
          id: const ObjectId('00000000-0000-4000-8000-0000000000d5'),
          name: 'DeepSeek',
          provider: AiProviderKind.openaiCompatible,
          baseUrl: 'https://api.example.test',
          chatModel: 'remote',
          createdAt: at,
          updatedAt: at,
        ),
        AiProviderConfig(
          id: deviceLocalAiProviderId,
          name: 'ConsoleCrypt',
          provider: AiProviderKind.openaiCompatible,
          baseUrl: 'http://127.0.0.1',
          chatModel: 'local',
          privacyProfile: PrivacyProfile.local,
          isDefault: true,
          createdAt: at,
          updatedAt: at,
        ),
      ],
    );
    await _ask(tester, 'What is failing?');
    expect(service.providerIds, [deviceLocalAiProviderId.value]);
    expect(tester.takeException(), isNull);
  });

  testWidgets('native conversation is reused only after fresh external consent, then cleared on lock', (tester) async {
    final service = _Service();
    final container = await _pump(
      tester,
      KubeAiPanel(session: _session, contextName: 'team-context', service: service),
      service,
      _Snippets(),
    );
    await _ask(tester, 'Explain the deployment');
    expect(service.asked, 0);
    await tester.tap(find.byKey(const ValueKey('kube-ai-consent')));
    await _settle(tester);
    expect(service.asked, 1);
    service.events.add(const KubeAiEvent('answer', {'content': 'First answer'}));
    service.events.add(const KubeAiEvent('conversation', {'conversation_id': 'saved-conversation'}));
    service.events.add(const KubeAiEvent('done', {}));
    await _settle(tester);
    expect(find.text('First answer'), findsOneWidget);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('kube-ai-question'))).enabled, isTrue);
    await _ask(tester, 'And its health?');
    // Stream cancellation may finish outside the widget fake-async zone.
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
    await _settle(tester);
    expect(service.revoked, 2, reason: 'second send must first revoke the completed native turn');
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('kube-ai-question'))).controller!.text,
      'And its health?',
    );
    expect(service.questions, ['Explain the deployment']);
    expect(service.followUps, ['saved-conversation']);
    expect(service.asked, 1);
    expect(find.textContaining(': 2'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('kube-ai-consent')));
    await _settle(tester);
    expect(service.asked, 2);
    service.events.add(const KubeAiEvent('answer', {'content': 'Second answer'}));
    service.events.add(const KubeAiEvent('done', {}));
    await _settle(tester);
    expect(find.text('First answer'), findsOneWidget);
    expect(find.text('Second answer'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('kube-ai-options')));
    await _settle(tester);
    await tester.ensureVisible(find.byKey(const ValueKey('kube-ai-new-conversation')));
    await tester.tap(find.byKey(const ValueKey('kube-ai-new-conversation')));
    await _settle(tester);
    expect(find.text('First answer'), findsNothing);
    service.events.add(const KubeAiEvent('answer', {'content': 'late stale answer'}));
    await _settle(tester);
    expect(find.text('late stale answer'), findsNothing);
    await _ask(tester, 'New independent question');
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
    await _settle(tester);
    expect(service.questions, ['Explain the deployment', 'New independent question']);
    expect(service.followUps, ['saved-conversation']);
    container.read(_scope.notifier).lock();
    await _settle(tester);
    expect(find.text('First answer'), findsNothing);
    expect(service.revoked, greaterThan(1));
    expect(tester.takeException(), isNull);
  });

  testWidgets('mobile command proposal never offers a local terminal action', (tester) async {
    final service = _Service();
    await _pump(
      tester,
      KubeAiProposals(
        answer: '```sh\nkubectl get pods\n```',
        session: _session,
        contextName: 'team-context',
        namespace: 'team',
        service: service,
        onReviewedCommand: (_, _) async => fail('mobile cannot insert'),
      ),
      service,
      _Snippets(),
    );
    expect(find.byKey(const ValueKey('kube-ai-proposal-review')), findsNothing);
    expect(service.opened, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.android));

  testWidgets('command proposal requires risk acknowledgement then inserts into a new scoped terminal without Enter', (
    tester,
  ) async {
    final service = _Service(), snippets = _Snippets();
    final inserted = <(String, String)>[];
    await _pump(
      tester,
      KubeAiProposals(
        answer: '```sh\nkubectl get pods\n```',
        session: _session,
        contextName: 'team-context',
        namespace: 'team',
        service: service,
        onReviewedCommand: (id, command) async => inserted.add((id, command)),
      ),
      service,
      snippets,
    );
    expect(service.opened, isEmpty);
    await tester.tap(find.byKey(const ValueKey('kube-ai-proposal-review')));
    await _settle(tester);
    expect(snippets.assessed, ['kubectl get pods']);
    expect(service.opened, isEmpty);
    expect(find.text('team-context · team'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('run-acknowledge')));
    await _settle(tester);
    await tester.tap(find.byKey(const ValueKey('run-confirm')));
    await _settle(tester);
    expect(find.byType(KubeContextTerminalDialog), findsOneWidget);
    expect(service.opened, isEmpty);
    await tester.tap(find.text('Continue'));
    await _settle(tester);
    expect(service.opened, [('session', 'team', 'macos:zsh')]);
    expect(inserted, [('native-terminal', 'kubectl get pods')]);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('locking during command review removes protected preview and prevents terminal creation', (tester) async {
    final service = _Service();
    final container = await _pump(
      tester,
      KubeAiProposals(
        answer: '```sh\nkubectl get pods\n```',
        session: _session,
        contextName: 'team-context',
        namespace: 'team',
        service: service,
        onReviewedCommand: (_, _) async => fail('must not insert'),
      ),
      service,
      _Snippets(),
    );
    await tester.tap(find.byKey(const ValueKey('kube-ai-proposal-review')));
    await _settle(tester);
    container.read(_scope.notifier).lock();
    await _settle(tester);
    expect(find.byKey(const ValueKey('run-confirmation')), findsNothing);
    expect(service.opened, isEmpty);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('YAML proposal populates existing create review without automatic dry-run or apply', (tester) async {
    final service = _Service();
    const manifest =
        'apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: api\n  namespace: team\nspec: {replicas: 2}';
    await _pump(
      tester,
      KubeAiProposals(
        answer: '```yaml\n$manifest\n```',
        session: _session,
        contextName: 'team-context',
        namespace: 'default',
        service: service,
      ),
      service,
      _Snippets(),
    );
    expect(service.discoveries, 0);
    await tester.tap(find.byKey(const ValueKey('kube-ai-proposal-review')));
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 200)));
    await _settle(tester);
    expect(find.byType(KubeCreateDialog), findsOneWidget);
    final dialog = tester.widget<KubeCreateDialog>(find.byType(KubeCreateDialog));
    expect(dialog.namespace, 'team');
    expect(dialog.initialName, 'api');
    expect(dialog.initialYaml, manifest);
    expect(service.creates, 0);
    expect(tester.takeException(), isNull);
  });
}
