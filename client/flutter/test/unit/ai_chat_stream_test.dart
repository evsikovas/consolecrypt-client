import 'dart:async';

import 'package:consolecrypt/core/bridge/ai_chat_stream.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';

const _report = SanitizationReport(profile: PrivacyProfile.local, redactions: 2, remote: false);
const _started = '{"type":"started","request_id":"request-1","conversation_id":"conversation-1"}';

void main() {
  test('cancellation aborts native generation while waiting for its first answer token', () async {
    var sourceCancelled = false;
    final source = StreamController<String>(onCancel: () => sourceCancelled = true);
    final cancelled = <String>[];
    final conversations = <String>[];
    final events = <AiStreamEvent>[];
    final subscription = bridgeAiChatStream(
      chunks: () => source.stream,
      cancel: (id) async => cancelled.add(id),
      onConversation: conversations.add,
      report: (_) => _report,
    ).listen(events.add);
    source.add(_started);
    await Future<void>.delayed(Duration.zero);
    // No delta/done is ever delivered: Stop must still complete immediately.
    await subscription.cancel().timeout(const Duration(seconds: 1));
    expect(cancelled, ['request-1']);
    expect(conversations, ['conversation-1']);
    expect(sourceCancelled, isTrue);
    expect(events, isEmpty);
    await source.close();
  });

  test('completed native answers preserve text and sanitizer report without cancelling', () async {
    final cancelled = <String>[];
    final events = await bridgeAiChatStream(
      chunks: () => Stream.fromIterable([
        _started,
        '{"type":"delta","text":"hello "}',
        '{"type":"delta","text":"world"}',
        '{"type":"done"}',
      ]),
      cancel: (id) async => cancelled.add(id),
      onConversation: (_) {},
      report: (_) => _report,
    ).toList();
    expect(events.whereType<AiDelta>().map((e) => e.text), ['hello ', 'world']);
    expect(events.whereType<AiCompleted>().single.fullText, 'hello world');
    expect(events.whereType<AiCompleted>().single.report, same(_report));
    expect(cancelled, isEmpty);
  });

  test('malformed native event closes the stream and cancels its active request', () async {
    final cancelled = <String>[];
    final events = await bridgeAiChatStream(
      chunks: () => Stream.fromIterable([_started, 'invalid json']),
      cancel: (id) async => cancelled.add(id),
      onConversation: (_) {},
      report: (_) => _report,
    ).toList();
    await Future<void>.delayed(Duration.zero);
    expect(events.single, isA<AiFailed>());
    expect(cancelled, ['request-1']);
  });
}
