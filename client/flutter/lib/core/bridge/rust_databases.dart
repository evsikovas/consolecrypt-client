import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:consolecrypt/src/rust/api/database.dart' as native;
import 'package:consolecrypt/src/rust/api/inventory.dart' as rs_inv;

/// Native databases: connections live in the vault, sessions and query streams
/// in the core. Passwords cross the bridge only as wiped byte buffers.
final class RustDbService implements DbService, DbQueryService, DbHistoryService {
  RustDbService(this._hub);
  final RustBackend _hub;
  final _sessions = <String, DbSessionInfo>{};
  final _local = StreamController<void>.broadcast();

  static Never _unsupported() =>
      throw const AppException(AppErrorCode.unsupported, 'Not available in this preview'); // l10n-ignore: diagnostic

  static Future<T> _secret<T>(SecretText? value, Future<T> Function(Uint8List? bytes) call) =>
      value == null ? call(null) : withSecret(value, call);

  DbSessionInfo _session(String json) {
    final info = DbSessionInfo.fromJson(jsonDecode(json) as DbJson);
    _sessions[info.id] = info;
    return info;
  }

  @override
  Stream<void> changes() => Stream<void>.multi((controller) {
    void changed(Json event) {
      if (const {
        'objects_changed',
        'conflict_resolved',
        'lagged',
        'vault_unlocked',
        'vault_locked',
        'profiles_changed',
      }.contains(event['type'])) {
        controller.add(null);
      }
    }

    _hub.eventListeners.add(changed);
    final local = _local.stream.listen((_) => controller.add(null));
    controller.add(null);
    controller.onCancel = () {
      _hub.eventListeners.remove(changed);
      unawaited(local.cancel());
    };
  }, isBroadcast: true);

  @override
  Future<List<DbConnection>> connections() async {
    final list = jsonDecode(await guard(native.dbConnections)) as List;
    return List.unmodifiable(list.cast<DbJson>().map(DbConnection.fromJson));
  }

  @override
  Future<DbConnectionChoices> connectionChoices() async {
    final hosts = (jsonDecode(await guard(rs_inv.hostsList)) as List).cast<Json>();
    final jumps = (jsonDecode(await guard(rs_inv.jumpProfilesList)) as List).cast<Json>();
    final groups = (jsonDecode(await guard(rs_inv.groupsList)) as List).cast<Json>();
    return DbConnectionChoices(
      routes: [
        for (final host in hosts)
          if ((host['protocol'] as String? ?? 'ssh') == 'ssh')
            DbRouteChoice(
              route: DbRoute(kind: DbRouteKind.ssh, reference: host['id'] as String),
              label: host['name'] as String? ?? host['address'] as String? ?? '',
            ),
        for (final jump in jumps)
          DbRouteChoice(
            route: DbRoute(kind: DbRouteKind.jump, reference: jump['id'] as String),
            label: jump['name'] as String? ?? '',
          ),
      ],
      groups: [for (final group in groups) DbGroupChoice(id: group['id'] as String, label: group['name'] as String? ?? '')],
    );
  }

  @override
  Future<DbConnection> saveConnection(DbConnectionDraft draft) async {
    try {
      final password = draft.savePassword ? draft.password : null;
      final saved = await _secret(
        password,
        (pw) => _secret(
          draft.privateKey,
          (key) => guard(
            () => native.dbSaveConnection(
              connectionJson: jsonEncode(draft.metadata.toJson()),
              password: pw,
              privateKey: key,
            ),
          ),
        ),
      );
      _local.add(null);
      return DbConnection.fromJson(jsonDecode(saved) as DbJson);
    } finally {
      draft.wipe();
    }
  }

  @override
  Future<void> deleteConnection(String id) async {
    await guard(() => native.dbDeleteConnection(id: id));
    _local.add(null);
  }

  @override
  Future<DbConnectionCheck> testConnection(DbConnectionDraft draft) async {
    try {
      final json = await _secret(
        draft.password,
        (pw) => guard(() => native.dbTestConnection(connectionJson: jsonEncode(draft.metadata.toJson()), password: pw)),
      );
      final info = DbSessionInfo.fromJson(jsonDecode(json) as DbJson);
      return DbConnectionCheck(serverVersion: info.serverVersion, serverReadOnly: info.serverReadOnly);
    } finally {
      draft.wipe();
    }
  }

  @override
  Future<DbImportReview> importConnectionString(SecretText uri) {
    uri.wipe();
    _unsupported();
  }

  @override
  Future<DbImportReview> importPgpass(SecretText contents) {
    contents.wipe();
    _unsupported();
  }

  @override
  Future<void> confirmImport(String ticket, List<DbImportChoice> choices) => _unsupported();

  @override
  Future<void> discardImport(String ticket) async {}

  // Sessions and queries.

  @override
  Future<DbSessionInfo> connect(String connectionId, {SecretText? temporaryPassword}) async => _session(
    await _secret(
      temporaryPassword,
      (pw) => guard(() => native.dbConnect(connectionId: connectionId, password: pw)),
    ),
  );

  @override
  Future<void> disconnect(String sessionId) async {
    _sessions.remove(sessionId);
    await guard(() => native.dbDisconnect(sessionId: sessionId));
  }

  @override
  Future<DbPrivilegeReport> checkPrivileges(String sessionId) async =>
      DbPrivilegeReport(serverReadOnly: _sessions[sessionId]?.serverReadOnly ?? DbServerReadOnly.unverified);

  @override
  Future<DbSessionInfo> allowSessionWrite(String sessionId) async =>
      _session(await guard(() => native.dbSetSessionWrite(sessionId: sessionId, allow: true)));

  @override
  Future<DbSessionInfo> revokeSessionWrite(String sessionId) async =>
      _session(await guard(() => native.dbSetSessionWrite(sessionId: sessionId, allow: false)));

  @override
  Future<DbTreePage> tree(String sessionId, {DbObjectRef? parent, String? cursor, bool refresh = false}) async =>
      DbTreePage.fromJson(
        jsonDecode(
              await guard(
                () => native.dbTree(
                  sessionId: sessionId,
                  parentJson: parent == null ? null : jsonEncode(parent.toJson()),
                ),
              ),
            )
            as DbJson,
      );

  @override
  Future<SecretText> ddl(String sessionId, DbObjectRef object) => _unsupported();

  @override
  Future<String> identifier(String sessionId, DbObjectRef object) =>
      guard(() => native.dbIdentifier(sessionId: sessionId, objectJson: jsonEncode(object.toJson())));

  @override
  Future<String> objectQuery(String sessionId, DbObjectRef object, {int limit = 100}) =>
      guard(() => native.dbObjectQuery(sessionId: sessionId, objectJson: jsonEncode(object.toJson()), limit: limit));

  @override
  Future<DbPreparedQuery> prepareQuery(
    String sessionId,
    String text, {
    required DbRunMode mode,
    required int selectionStart,
    required int selectionEnd,
  }) async {
    final json =
        jsonDecode(
              await guard(
                () => native.dbPrepareQuery(
                  sessionId: sessionId,
                  text: text,
                  mode: mode.name,
                  selectionStart: selectionStart,
                  selectionEnd: selectionEnd,
                ),
              ),
            )
            as DbJson;
    return DbPreparedQuery(
      text: json['text'] as String,
      parameterNames: (json['parameter_names'] as List).cast<String>(),
    );
  }

  @override
  Future<String> formatQuery(DbEngine engine, String text) =>
      guard(() => native.dbFormatQuery(engine: engine.wireName, text: text));

  @override
  Future<List<DbCompletion>> completeQuery(String sessionId, String text, int cursor) async => const [];

  @override
  Future<DbQueryReview> reviewQuery(String sessionId, DbQueryRequest request) async {
    final json =
        jsonDecode(await guard(() => native.dbReview(sessionId: sessionId, requestJson: jsonEncode(request.toJson()))))
            as DbJson;
    return DbQueryReview(
      ticket: json['ticket'] as String,
      risk: switch (json['risk']) {
        'read_only' => DbQueryRisk.read,
        'write' => DbQueryRisk.write,
        _ => DbQueryRisk.dangerous,
      },
      expiresAt: DateTime.now().add(Duration(seconds: json['expires_in_seconds'] as int? ?? 60)),
      reasonCodes: (json['reason_codes'] as List? ?? const []).cast<String>(),
    );
  }

  @override
  Future<String> execute(String sessionId, DbQueryRequest request) =>
      guard(() => native.dbExecute(sessionId: sessionId, requestJson: jsonEncode(request.toJson())));

  @override
  Stream<DbQueryEvent> watchQuery(String sessionId, String queryId) async* {
    while (true) {
      final List<DbQueryEvent> events;
      try {
        events = (jsonDecode(await guard(() => native.dbPoll(queryId: queryId, waitMs: 500))) as List)
            .cast<DbJson>()
            .map(DbQueryEvent.fromJson)
            .toList();
      } on AppException {
        return; // released or session closed
      }
      for (final event in events) {
        yield event;
        if (const {DbQueryEventKind.done, DbQueryEventKind.cancelled, DbQueryEventKind.error}.contains(event.kind)) {
          return;
        }
      }
    }
  }

  @override
  Future<void> loadMore(String sessionId, String queryId, {int rows = 1000}) =>
      guard(() => native.dbLoadMore(queryId: queryId, rows: rows));

  @override
  Future<void> cancelQuery(String sessionId, String queryId) => guard(() => native.dbCancel(queryId: queryId));

  @override
  Future<void> releaseQuery(String sessionId, String queryId) => guard(() => native.dbRelease(queryId: queryId));

  @override
  Future<DbExplainPlan> explain(String sessionId, DbQueryRequest request, {bool analyze = false}) => _unsupported();

  // History.

  @override
  Future<DbHistoryPolicy> historyPolicy(String connectionId) async => DbHistoryPolicy.local;

  @override
  Future<void> setHistoryPolicy(String connectionId, DbHistoryPolicy policy) => _unsupported();

  @override
  Future<DbHistoryPage> history(
    String connectionId, {
    String query = '',
    bool? succeeded,
    bool favoritesOnly = false,
    String? cursor,
    int limit = 100,
  }) async {
    final json =
        jsonDecode(
              await guard(
                () => native.dbHistory(
                  connectionId: connectionId,
                  query: query,
                  succeeded: succeeded,
                  favoritesOnly: favoritesOnly,
                  cursor: cursor,
                  limit: limit,
                ),
              ),
            )
            as DbJson;
    return DbHistoryPage(
      entries: [
        for (final entry in (json['entries'] as List).cast<DbJson>())
          DbHistoryEntry(
            id: entry['id'] as String,
            connectionId: entry['connection_id'] as String,
            engine: DbEngine.parse(entry['engine'] as String),
            text: entry['text'] as String,
            createdAt: DateTime.parse(entry['created_at'] as String),
            elapsedMs: entry['elapsed_ms'] as int? ?? 0,
            rows: entry['rows'] as int? ?? 0,
            errorCode: entry['error_code'] as String?,
            favorite: entry['favorite'] as bool? ?? false,
          ),
      ],
      nextCursor: json['next_cursor'] as String?,
    );
  }

  @override
  Future<void> setFavorite(String historyId, bool favorite) =>
      guard(() => native.dbHistoryFavorite(historyId: historyId, favorite: favorite));

  @override
  Future<void> saveHistorySnippet(String historyId, String name) => _unsupported();

  @override
  Future<void> deleteHistory(String connectionId) => guard(() => native.dbDeleteHistory(connectionId: connectionId));
}
