import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_models.dart';

enum DbAiAction { compose, explain, fix, optimize, translate, snippet, examples, result }

final class DbAiProvider {
  const DbAiProvider({required this.id, required this.name, required this.external});
  final String id, name;
  final bool external;
}

final class DbAiRequest {
  const DbAiRequest({
    required this.action,
    required this.question,
    this.query,
    this.errorCode,
    this.plan,
    this.object,
    this.targetEngine,
    this.conversationId,
    this.queryId,
    this.resultIndex,
    this.selectedRows = const [],
    this.contextLimit = 12,
  });
  final DbAiAction action;
  final String question;
  final String? query, errorCode, conversationId, queryId;
  final DbJson? plan;
  final DbObjectRef? object;
  final DbEngine? targetEngine;
  final int? resultIndex;
  final List<int> selectedRows;
  final int contextLimit;
  @override
  String toString() => 'DbAiRequest(<redacted>)';
}

final class DbAiReview {
  const DbAiReview({
    required this.ticket,
    required this.provider,
    required this.connectionName,
    required this.expiresAt,
    this.schemaObjects = const [],
    this.hasSelectedRows = false,
    this.historyMessages = 0,
  });
  final String ticket, connectionName;
  final DbAiProvider provider;
  final DateTime expiresAt;
  final List<String> schemaObjects;
  final bool hasSelectedRows;
  final int historyMessages;
}

final class DbAiConsent {
  const DbAiConsent({
    this.schema = false,
    this.selectedRows = false,
    this.sampleRows = false,
    this.runReadonly = false,
  });
  final bool schema, selectedRows, sampleRows, runReadonly;
  DbJson toJson() => {
    'schema': schema,
    'selected_rows': selectedRows,
    'sample_rows': sampleRows,
    'run_readonly': runReadonly,
  };
}

enum DbAiEventKind { context, tool, draft, explanation, conversation, error, done }

final class DbAiEvent {
  const DbAiEvent({required this.kind, this.content, this.tool, this.code, this.conversationId, this.isDdl = false});
  final DbAiEventKind kind;

  /// Owned sensitive buffer: panel must wipe on lock, disposal, or replacement.
  final SecretText? content;
  final String? tool, code, conversationId;
  final bool isDdl;
  void wipe() => content?.wipe();
  @override
  String toString() => 'DbAiEvent(${kind.name}, <redacted>)';
}

abstract interface class DbAiService {
  Future<List<DbAiProvider>> aiProviders();
  Future<DbAiReview> prepareAi(String sessionId, String providerId, DbAiRequest request);
  Stream<DbAiEvent> askAi(String sessionId, String ticket, DbAiConsent consent);
  Future<void> cancelAi(String sessionId, String ticket);
  Future<void> revokeAiConsent(String sessionId);
}
