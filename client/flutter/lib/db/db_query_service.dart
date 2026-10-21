import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_models.dart';

enum DbRunMode { current, selection, script }

enum DbQueryRisk { read, write, dangerous, blocked }

enum DbHistoryPolicy { local, encryptedSync, disabled }

final class DbPrivilegeReport {
  const DbPrivilegeReport({required this.serverReadOnly, this.hasWritePrivileges, this.warningCodes = const []});
  final DbServerReadOnly serverReadOnly;
  final bool? hasWritePrivileges;
  final List<String> warningCodes;
}

final class DbQueryReview {
  const DbQueryReview({
    required this.ticket,
    required this.risk,
    required this.expiresAt,
    this.reasonCodes = const [],
    this.suggestion,
  });
  final String ticket;
  final DbQueryRisk risk;
  final DateTime expiresAt;
  final List<String> reasonCodes;

  /// A runnable read-only alternative to a blocked query (Redis KEYS → SCAN).
  final String? suggestion;
}

final class DbPreparedQuery {
  const DbPreparedQuery({required this.text, this.parameterNames = const []});
  final String text;
  final List<String> parameterNames;
  @override
  String toString() => 'DbPreparedQuery(<redacted>)';
}

final class DbCompletion {
  const DbCompletion({required this.insertText, required this.label, this.detail = ''});
  final String insertText, label, detail;
}

final class DbExplainPlan {
  const DbExplainPlan({required this.engine, required this.document, this.analyzed = false});
  final DbEngine engine;
  final DbJson document;
  final bool analyzed;
  @override
  String toString() => 'DbExplainPlan(<redacted>)';
}

final class DbHistoryEntry {
  const DbHistoryEntry({
    required this.id,
    required this.connectionId,
    required this.engine,
    required this.text,
    required this.createdAt,
    this.elapsedMs = 0,
    this.rows = 0,
    this.errorCode,
    this.favorite = false,
    this.productionWrite = false,
  });
  final String id, connectionId, text;
  final DbEngine engine;
  final DateTime createdAt;
  final int elapsedMs, rows;
  final String? errorCode;
  final bool favorite;

  /// Changed data of a production connection in its temporary write window.
  final bool productionWrite;
  @override
  String toString() => 'DbHistoryEntry(<redacted>)';
}

final class DbHistoryPage {
  const DbHistoryPage({required this.entries, this.nextCursor});
  final List<DbHistoryEntry> entries;
  final String? nextCursor;
}

abstract interface class DbQueryService {
  Future<DbSessionInfo> connect(String connectionId, {SecretText? temporaryPassword});
  Future<void> disconnect(String sessionId);
  Future<DbPrivilegeReport> checkPrivileges(String sessionId);
  Future<DbSessionInfo> allowSessionWrite(String sessionId);
  Future<DbSessionInfo> revokeSessionWrite(String sessionId);
  Future<DbTreePage> tree(String sessionId, {DbObjectRef? parent, String? cursor, bool refresh = false});
  Future<SecretText> ddl(String sessionId, DbObjectRef object);
  Future<String> identifier(String sessionId, DbObjectRef object);
  Future<String> objectQuery(String sessionId, DbObjectRef object, {int limit = 100});
  Future<DbPreparedQuery> prepareQuery(
    String sessionId,
    String text, {
    required DbRunMode mode,
    required int selectionStart,
    required int selectionEnd,
  });
  Future<String> formatQuery(DbEngine engine, String text);
  Future<List<DbCompletion>> completeQuery(String sessionId, String text, int cursor);
  Future<DbQueryReview> reviewQuery(String sessionId, DbQueryRequest request);
  Future<String> execute(String sessionId, DbQueryRequest request);
  Stream<DbQueryEvent> watchQuery(String sessionId, String queryId);
  Future<void> loadMore(String sessionId, String queryId, {int rows = 1000});
  Future<void> cancelQuery(String sessionId, String queryId);
  Future<void> releaseQuery(String sessionId, String queryId);
  Future<DbExplainPlan> explain(String sessionId, DbQueryRequest request, {bool analyze = false});
}

/// The vault passphrase did not match when unlocking production writes.
final class DbWrongPassphrase implements Exception {
  const DbWrongPassphrase();
}

/// Temporary write windows of production sessions. The core checks the vault
/// passphrase and the typed connection name, owns the expiry and re-locks on
/// disconnect, vault lock and profile change.
abstract interface class DbProductionWriteService {
  /// Live write state of a session (asked again when a window runs out).
  Future<DbSessionInfo> sessionState(String sessionId);

  /// Opens the window for [minutes] (5, 15 or 30); throws [DbWrongPassphrase].
  Future<DbSessionInfo> unlockProduction(
    String sessionId, {
    required String typedConnectionName,
    required SecretText passphrase,
    required int minutes,
  });
  Future<DbSessionInfo> lockProduction(String sessionId);
}

abstract interface class DbHistoryService {
  Future<DbHistoryPolicy> historyPolicy(String connectionId);
  Future<void> setHistoryPolicy(String connectionId, DbHistoryPolicy policy);
  Future<DbHistoryPage> history(
    String connectionId, {
    String query = '',
    bool? succeeded,
    bool favoritesOnly = false,
    String? cursor,
    int limit = 100,
  });
  Future<void> setFavorite(String historyId, bool favorite);
  Future<void> saveHistorySnippet(String historyId, String name);
  Future<void> deleteHistory(String connectionId);
}

final class DbResultSelection {
  const DbResultSelection({this.rows, this.columns});

  /// Null selects all loaded rows/columns; these are result indices, not SQL.
  final List<int>? rows, columns;
  DbJson toJson() => {'rows': rows, 'columns': columns};
}

final class DbExportDestination {
  const DbExportDestination({required this.id, required this.label});
  final String id, label;
}

abstract interface class DbExportService {
  Future<SecretText> copyResult(
    String sessionId,
    String queryId,
    int resultIndex,
    DbResultSelection selection,
    DbExportOptions options,
  );
  Future<DbExportDestination?> chooseExportDestination(DbExportOptions options);
  Future<String> startExport(
    String sessionId,
    String queryId,
    int resultIndex,
    DbExportOptions options,
    DbExportDestination destination,
  );
  Stream<DbExportProgress> watchExport(String sessionId, String exportId);
  Future<void> cancelExport(String sessionId, String exportId);
}

final class DbCellEdit {
  const DbCellEdit({required this.originalPrimaryKey, required this.column, required this.value});
  final Map<String, DbCell> originalPrimaryKey;
  final String column;
  final DbCell value;
  DbJson toJson() => {
    'original_primary_key': originalPrimaryKey.map((key, value) => MapEntry(key, value.toJson())),
    'column': column,
    'value': value.toJson(),
  };
  @override
  String toString() => 'DbCellEdit(<redacted>)';
}

final class DbCellEditReview {
  const DbCellEditReview({required this.ticket, required this.sql, required this.expiresAt, required this.rows});
  final String ticket;
  final SecretText sql;
  final DateTime expiresAt;
  final int rows;
}

abstract interface class DbCellEditService {
  Future<DbCellEditReview> reviewCellEdits(String sessionId, DbObjectRef table, List<DbCellEdit> edits);
  Future<void> commitCellEdits(String sessionId, String ticket);
  Future<void> discardCellEdits(String sessionId, String ticket);
}

/// Explicit data sampling differs from metadata browsing and always needs review.
abstract interface class DbObjectInspectService {
  Future<DbJson> inspectObject(String sessionId, DbObjectRef object, {int? sampleDocuments});
}

abstract interface class DbQueryReviewService {
  Future<void> discardQueryReview(String sessionId, String ticket);
}
