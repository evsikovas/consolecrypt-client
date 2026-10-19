import 'dart:async';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';

/// Explicit cancellation is needed here: cancelling an async* generator while
/// it awaits the next native chunk would defer its finally block until a token
/// arrives. The native request must stop even before the first answer token.
Stream<AiStreamEvent> bridgeAiChatStream({
  required Stream<String> Function() chunks,
  required Future<void> Function(String requestId) cancel,
  required void Function(String conversationId) onConversation,
  required SanitizationReport Function(Json summary) report,
}) => Stream<AiStreamEvent>.multi((output) {
  final text = StringBuffer();
  String? requestId;
  var finished = false;
  var cancelled = false;
  StreamSubscription<String>? subscription;
  output.onCancel = () async {
    cancelled = true;
    final id = requestId;
    if (!finished && id != null) {
      try {
        await cancel(id);
      } catch (_) {
        // Lock/shutdown may already have cancelled and removed this request.
      }
    }
    await subscription?.cancel();
  };
  void fail(Object error) {
    if (cancelled) return;
    output.add(AiFailed(toAppException(error).message));
    unawaited(output.close());
  }

  try {
    subscription = chunks().listen(
      (json) {
        if (cancelled || finished) return;
        try {
          final chunk = decodeObject(json);
          switch (chunk['type']) {
            case 'started':
              requestId = chunk['request_id'] as String?;
              onConversation(chunk['conversation_id']! as String);
            case 'delta':
              final delta = chunk['text'] as String? ?? '';
              text.write(delta);
              output.add(AiDelta(delta));
            case 'done':
              final summary = report(chunk);
              finished = true;
              output.add(AiCompleted(fullText: text.toString(), report: summary));
              unawaited(output.close());
            case 'error':
              finished = true;
              output.add(AiFailed(chunk['message'] as String? ?? ''));
              unawaited(output.close());
          }
        } catch (error) {
          fail(error);
        }
      },
      onError: fail,
      onDone: () => unawaited(output.close()),
    );
  } catch (error) {
    fail(error);
  }
});
