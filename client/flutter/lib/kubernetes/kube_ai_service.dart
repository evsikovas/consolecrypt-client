import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';

abstract interface class KubeAiService {
  Future<KubeAiReview> prepareKubeAi(
    String sessionId, {
    String? providerId,
    required String question,
    KubeResourceRef? selected,
    bool configMapValues = false,
  });
  Stream<KubeAiEvent> askKubeAi(String sessionId, KubeAiReview review, {required bool approveExternal});
  Future<void> discardKubeAi(String sessionId, String ticket);
  Future<void> revokeKubeAi(String sessionId);
}

abstract interface class KubeAiHistoryService {
  Future<List<KubeJson>> kubeAiHistory(String contextId);
  Future<void> deleteKubeAiConversation(String conversationId);
}

abstract interface class KubeAiChatService {
  Future<KubeAiReview> prepareKubeFollowUp(
    String sessionId,
    String conversationId, {
    String? providerId,
    required String question,
    KubeResourceRef? selected,
    bool configMapValues = false,
  });
}

final class KubeAiReview {
  const KubeAiReview({
    required this.ticket,
    required this.providerName,
    required this.contextName,
    required this.external,
    required this.toolSupport,
    this.historyMessages = 0,
  });
  factory KubeAiReview.fromJson(KubeJson json) => KubeAiReview(
    ticket: json['ticket'] as String,
    providerName: json['provider_name'] as String,
    contextName: json['context_name'] as String,
    external: json['external'] as bool,
    toolSupport: json['tool_support'] as bool,
    historyMessages: json['history_messages'] as int? ?? 0,
  );
  final String ticket;
  final String providerName;
  final String contextName;
  final bool external;
  final bool toolSupport;
  final int historyMessages;
}

final class KubeAiEvent {
  const KubeAiEvent(this.type, this.data);
  final String type;
  final KubeJson data;
  String get content => data['content'] as String? ?? '';
  String? get tool => data['tool'] as String?;
  String? get code => data['code'] as String?;
  bool get truncated => data['truncated'] == true;
}

final class KubeAiPoll {
  const KubeAiPoll(this.events, this.closed);
  factory KubeAiPoll.fromBytes(Uint8List bytes) {
    try {
      final json = jsonDecode(utf8.decode(bytes)) as KubeJson;
      return KubeAiPoll(
        (json['events'] as List)
            .cast<KubeJson>()
            .map((item) => KubeAiEvent(item['type'] as String, item))
            .toList(growable: false),
        json['closed'] == true,
      );
    } finally {
      bytes.fillRange(0, bytes.length, 0);
    }
  }
  final List<KubeAiEvent> events;
  final bool closed;
}
