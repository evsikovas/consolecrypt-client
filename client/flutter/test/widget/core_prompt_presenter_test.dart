// Default presenter of core prompts raised outside terminal tabs
// (lib/core/bridge/core_prompt_presenter.dart) on the mock PromptService.
import 'package:consolecrypt/core/bridge/core_prompt_presenter.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

Future<MockBackend> _pump(WidgetTester tester) async {
  final backend = MockBackend(config: const MockConfig.test(seedDemoData: false));
  addTearDown(backend.dispose);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [appServicesProvider.overrideWithValue(backend.services)],
      child: const MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        locale: Locale('en'),
        home: CorePromptPresenter(child: Scaffold(body: Text('content'))),
      ),
    ),
  );
  await tester.pump();
  return backend;
}

/// Stream deliveries hop through microtasks before `setState` lands.
Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 3; i++) {
    await tester.pump();
  }
}

void main() {
  testWidgets('unknown host key: fingerprint shown, accept once answers', (tester) async {
    final backend = await _pump(tester);
    expect(backend.prompts.hasPresenter, isTrue);
    expect(
      backend.prompts.simulate(
        const HostKeyCorePrompt(
          requestId: 'r1',
          host: 'db.internal',
          port: 22,
          hostPattern: 'db.internal',
          hostName: 'db',
          keyType: 'ssh-ed25519',
          fingerprintSha256: 'SHA256:q5N3fake',
        ),
      ),
      isTrue,
    );
    await _settle(tester);
    expect(find.textContaining('SHA256:q5N3fake', findRichText: true), findsOneWidget);
    await tester.tap(find.text('Accept once'));
    await _settle(tester);
    expect(backend.prompts.answers['r1'], HostKeyDecision.acceptOnce);
    expect(find.textContaining('SHA256:q5N3fake', findRichText: true), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('password prompt: typed secret is sent, cancel sends none', (tester) async {
    final backend = await _pump(tester);
    backend.prompts.simulate(const PasswordCorePrompt(requestId: 'r2', hostName: 'web'));
    await _settle(tester);
    await tester.enterText(find.byType(TextField), 'hunter2');
    await tester.tap(find.text('Connect'));
    await _settle(tester);
    expect(backend.prompts.answers['r2'], 'secret');
    backend.prompts.simulate(
      const PassphraseCorePrompt(requestId: 'r3', credentialId: ObjectId('c1'), credentialName: 'deploy key'),
    );
    await _settle(tester);
    expect(find.text('deploy key'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await _settle(tester);
    expect(backend.prompts.answers['r3'], 'cancelled');
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('unmounting the presenter detaches it', (tester) async {
    final backend = await _pump(tester);
    await tester.pumpWidget(const SizedBox());
    expect(backend.prompts.hasPresenter, isFalse);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
