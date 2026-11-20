import 'dart:async';

import 'package:consolecrypt/core/bridge/core_prompt_presenter.dart';
import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/mock/mock_prompt_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/prompt_service.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

// UI-only lifecycle double; native SQLCipher/cancellation tests independently
// cover the actual transition lease. It never runs a job or reads credentials.
class _Transitions implements PromptService, AutomationTransitionPromptService {
  final delegate = MockPromptService();
  final replies = <String>[];
  final waits = <String, Completer<void>>{};
  bool simulate(CorePrompt prompt) => delegate.simulate(prompt);
  void finish(String id) {
    final wait = waits[id];
    if (wait != null && !wait.isCompleted) wait.complete();
  }

  @override
  List<CorePrompt> get pending => delegate.pending;
  @override
  Stream<List<CorePrompt>> watchPending() => delegate.watchPending();
  @override
  Future<void> answerHostKey(String id, HostKeyDecision decision) => delegate.answerHostKey(id, decision);
  @override
  Future<void> answerSecret(String id, SecretText? secret) => delegate.answerSecret(id, secret);
  @override
  Future<void> answerAutomationTransition(String id, String decision) async {
    replies.add('$id:$decision');
    if (decision == 'abort') {
      finish(id);
    } else {
      await waits.putIfAbsent(id, Completer<void>.new).future;
    }
    await delegate.answerSecret(id, null); // Removes only this synthetic card.
  }

  @override
  void Function() attachPresenter() {
    final detach = delegate.attachPresenter();
    return () {
      for (final prompt in [...pending]) {
        if (prompt is AutomationTransitionCorePrompt) {
          unawaited(answerAutomationTransition(prompt.requestId, 'abort'));
        }
      }
      detach();
    };
  }

  Future<void> dispose() async {
    for (final id in waits.keys) {
      finish(id);
    }
    await delegate.dispose();
  }
}

Future<void> _settle(WidgetTester tester) async {
  for (var i = 0; i < 5; i++) {
    await tester.pump();
  }
}

Future<_Transitions> _pump(WidgetTester tester) async {
  final backend = MockBackend(config: const MockConfig.test(seedDemoData: false));
  final prompts = _Transitions();
  addTearDown(backend.dispose);
  addTearDown(prompts.dispose);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        promptServiceProvider.overrideWithValue(prompts),
      ],
      child: const MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        locale: Locale('en'),
        home: CorePromptPresenter(child: Scaffold(body: Text('Locked profile fixture'))),
      ),
    ),
  );
  await _settle(tester);
  return prompts;
}

const _question = AutomationTransitionCorePrompt(requestId: 'transition', jobCount: 2, operation: 'close');

void main() {
  testWidgets('wait remains cancellable while locked and never hides a worker password prompt', (tester) async {
    final prompts = await _pump(tester);
    prompts.simulate(_question);
    await _settle(tester);
    expect(find.byType(TextField), findsNothing);
    await tester.tap(find.byKey(const ValueKey('automation-transition-wait')));
    await _settle(tester);
    expect(prompts.replies, ['transition:wait']);
    expect(find.byType(LinearProgressIndicator), findsOneWidget);
    prompts.simulate(const PasswordCorePrompt(requestId: 'worker-auth', hostName: 'Worker auth fixture'));
    await _settle(tester);
    expect(find.text('Worker auth fixture'), findsOneWidget);
    await tester.enterText(find.byType(TextField), 'synthetic-password');
    await tester.tap(find.text('Connect'));
    await _settle(tester);
    expect(find.byType(LinearProgressIndicator), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('automation-transition-abort')));
    await _settle(tester);
    expect(prompts.replies, ['transition:wait', 'transition:abort']);
    expect(prompts.pending, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('cancel jobs keeps progress visible until native worker cleanup completes', (tester) async {
    final prompts = await _pump(tester);
    prompts.simulate(_question);
    await _settle(tester);
    await tester.tap(find.byKey(const ValueKey('automation-transition-cancel-jobs')));
    await _settle(tester);
    expect(prompts.replies, ['transition:cancel_jobs']);
    expect(prompts.pending, hasLength(1));
    expect(find.byType(LinearProgressIndicator), findsOneWidget);
    prompts.finish('transition');
    await _settle(tester);
    expect(prompts.pending, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('unknown outcome requires explicit acknowledgement on a narrow screen', (tester) async {
    await tester.binding.setSurfaceSize(const Size(390, 640));
    addTearDown(() => tester.binding.setSurfaceSize(null));
    final prompts = await _pump(tester);
    prompts.simulate(
      const AutomationTransitionCorePrompt(
        requestId: 'uncertain',
        jobCount: 1,
        unknownCount: 1,
        operation: 'switch_profile',
      ),
    );
    await _settle(tester);
    expect(tester.widget<OutlinedButton>(find.byKey(const ValueKey('automation-transition-wait'))).onPressed, isNull);
    expect(
      tester.widget<FilledButton>(find.byKey(const ValueKey('automation-transition-cancel-jobs'))).onPressed,
      isNull,
    );
    await tester.ensureVisible(find.byType(Checkbox));
    await tester.pump();
    await tester.tap(find.byType(Checkbox));
    await _settle(tester);
    await tester.tap(find.byKey(const ValueKey('automation-transition-cancel-jobs')));
    await _settle(tester);
    expect(prompts.replies, ['uncertain:cancel_jobs_accept_unknown']);
    prompts.finish('uncertain');
    await _settle(tester);
    expect(tester.takeException(), isNull);
  });
  testWidgets('unmounting a waiting presenter aborts only its transition', (tester) async {
    final prompts = await _pump(tester);
    prompts.simulate(_question);
    await _settle(tester);
    await tester.tap(find.byKey(const ValueKey('automation-transition-wait')));
    await _settle(tester);
    await tester.pumpWidget(const SizedBox());
    await _settle(tester);
    expect(prompts.replies, ['transition:wait', 'transition:abort']);
    expect(prompts.pending, isEmpty);
    expect(tester.takeException(), isNull);
  });
}
