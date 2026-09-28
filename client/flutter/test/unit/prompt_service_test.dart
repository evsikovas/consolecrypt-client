// PromptService contract on the in-memory mock (the FRB implementation is
// covered by integration_test/rust_core_test.dart).
import 'package:consolecrypt/core/mock/mock_prompt_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  const hostKey = HostKeyCorePrompt(
    requestId: 'r1',
    host: 'db',
    port: 22,
    hostPattern: 'db',
    keyType: 'ssh-ed25519',
    fingerprintSha256: 'SHA256:abc',
  );
  const password = PasswordCorePrompt(requestId: 'r2', hostName: 'db');

  test('prompts are declined without a presenter', () {
    final s = MockPromptService();
    expect(s.simulate(hostKey), isFalse);
    expect(s.answers['r1'], HostKeyDecision.reject);
    expect(s.pending, isEmpty);
  });

  test('a presenter sees prompts in order and answers them', () async {
    final s = MockPromptService();
    final detach = s.attachPresenter();
    final seen = <List<CorePrompt>>[];
    final sub = s.watchPending().listen(seen.add);
    expect(s.simulate(hostKey), isTrue);
    expect(s.simulate(password), isTrue);
    expect(s.pending.map((p) => p.requestId), ['r1', 'r2']);
    await s.answerHostKey('r1', HostKeyDecision.acceptOnce);
    final secret = SecretText('hunter2');
    await s.answerSecret('r2', secret);
    expect(secret.isWiped, isTrue);
    expect(s.answers, {'r1': HostKeyDecision.acceptOnce, 'r2': 'secret'});
    expect(s.pending, isEmpty);
    await Future<void>.delayed(Duration.zero);
    expect(seen.first, isEmpty, reason: 'current value first');
    await sub.cancel();
    detach();
  });

  test('detaching the last presenter declines what is pending', () async {
    final s = MockPromptService();
    final detach = s.attachPresenter();
    s.simulate(hostKey);
    s.simulate(password);
    detach();
    detach(); // idempotent
    await Future<void>.delayed(Duration.zero);
    expect(s.pending, isEmpty);
    expect(s.answers, {'r1': HostKeyDecision.reject, 'r2': 'cancelled'});
    expect(s.hasPresenter, isFalse);
  });
}
