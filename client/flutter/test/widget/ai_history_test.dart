import 'package:consolecrypt/ai/ai_chat_controller.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

ProviderContainer _container(WidgetTester tester) =>
    ProviderScope.containerOf(tester.element(find.byType(Navigator).first));

void main() {
  testWidgets('a conversation from the history comes back and goes on', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend);
    final c = _container(tester);
    await tapKey(tester, 'nav-ai');
    await enterKey(tester, 'ai-input', 'How do I check disk usage?');
    await tapKey(tester, 'ai-send');
    expect(c.read(aiChatControllerProvider).messages, hasLength(2));
    await tapKey(tester, 'ai-new-conversation');
    expect(c.read(aiChatControllerProvider).messages, isEmpty);

    await tapKey(tester, 'ai-history');
    expect(find.byKey(const ValueKey('ai-history-dialog')), findsOneWidget);
    expect(find.text('How do I check disk usage?'), findsOneWidget);
    expect(find.textContaining('2 messages'), findsOneWidget);
    expect(find.textContaining('until the vault locks'), findsOneWidget);
    await tester.tap(find.text('How do I check disk usage?'));
    await settle(tester);
    expect(find.byKey(const ValueKey('ai-history-dialog')), findsNothing);
    final restored = c.read(aiChatControllerProvider).messages;
    expect(restored.map((m) => m.role), [ChatRole.user, ChatRole.assistant]);
    expect(restored.first.content, 'How do I check disk usage?');

    // The next question continues the same conversation.
    await enterKey(tester, 'ai-input', 'And memory?');
    await tapKey(tester, 'ai-send');
    final listed = await backend.ai.listConversations();
    expect(listed, hasLength(1));
    expect(listed.single.messages, hasLength(4));

    // Deleting it from the history.
    await tapKey(tester, 'ai-history');
    await tapKey(tester, 'ai-history-delete-${listed.single.id.value}');
    expect(find.byKey(const ValueKey('ai-history-empty')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
