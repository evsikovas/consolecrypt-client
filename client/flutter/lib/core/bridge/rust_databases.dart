import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_account.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/models/step_up.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/db/db_ai_service.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:consolecrypt/src/rust/api/database.dart' as native;
import 'package:consolecrypt/src/rust/api/inventory.dart' as rs_inv;
import 'package:consolecrypt/src/rust/api/kubernetes.dart' as rs_kube;
import 'package:file_selector/file_selector.dart';

/// Native databases: connections live in the vault, sessions and query streams
/// in the core. Passwords cross the bridge only as wiped byte buffers.
final class RustDbService
    implements
        DbService,
        DbQueryService,
        DbHistoryService,
        DbPgpassFileService,
        DbExportService,
        DbCellEditService,
        DbObjectInspectService,
        DbProductionWriteService,
        DbAiService {
  RustDbService(this._hub);
  final RustBackend _hub;
  final _sessions = <String, DbSessionInfo>{};
  final _local = StreamController<void>.broadcast();

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
    final tunnels = (jsonDecode(await guard(rs_inv.tunnelsList)) as List).cast<Json>();
    // Kubernetes contexts are optional: a failed listing hides only that route.
    var contexts = const <Json>[];
    try {
      contexts = (jsonDecode(await guard(rs_kube.kubeContextsList)) as List).cast<Json>();
    } on AppException {
      contexts = const [];
    }
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
        for (final context in contexts)
          DbRouteChoice(
            route: DbRoute(kind: DbRouteKind.kubernetes, reference: context['id'] as String),
            label: context['name'] as String? ?? '',
          ),
        // Only local forwards carry a database connection.
        for (final tunnel in tunnels)
          if (tunnel['kind'] == 'local' && tunnel['target_host'] is String && tunnel['target_port'] is int)
            DbRouteChoice(
              route: DbRoute(kind: DbRouteKind.tunnel, reference: tunnel['id'] as String),
              label: tunnel['name'] as String? ?? '',
              host: tunnel['target_host'] as String,
              port: tunnel['target_port'] as int,
              localPort: tunnel['bind_port'] as int?,
            ),
      ],
      groups: [
        for (final group in groups) DbGroupChoice(id: group['id'] as String, label: group['name'] as String? ?? ''),
      ],
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
      return DbConnectionCheck(
        serverVersion: info.serverVersion,
        serverReadOnly: info.serverReadOnly,
        transportSecurity: info.transportSecurity,
      );
    } finally {
      draft.wipe();
    }
  }

  static DbImportReview _review(String json) {
    final review = jsonDecode(json) as DbJson;
    return DbImportReview(
      ticket: review['ticket'] as String,
      expiresAt: DateTime.now().add(Duration(seconds: review['expires_in_seconds'] as int? ?? 600)),
      entries: [
        for (final entry in (review['entries'] as List).cast<DbJson>())
          DbImportEntry(
            index: entry['index'] as int,
            name: entry['name'] as String,
            engine: DbEngine.parse(entry['engine'] as String),
            host: entry['host'] as String,
            port: entry['port'] as int,
            database: entry['database'] as String? ?? '',
            username: entry['username'] as String? ?? '',
            hasPassword: entry['has_password'] as bool? ?? false,
            warningCodes: (entry['warning_codes'] as List? ?? const []).cast<String>(),
          ),
      ],
    );
  }

  @override
  Future<DbImportReview> importConnectionString(SecretText uri) async {
    try {
      return _review(await withSecret(uri, (bytes) => guard(() => native.dbImportUri(uri: bytes))));
    } finally {
      uri.wipe();
    }
  }

  @override
  Future<DbImportReview> importPgpass(SecretText contents) async {
    try {
      return _review(await withSecret(contents, (bytes) => guard(() => native.dbImportPgpass(contents: bytes))));
    } finally {
      contents.wipe();
    }
  }

  /// `~/.pgpass` (or `$PGPASSFILE`) when present, otherwise a file chooser.
  @override
  Future<DbImportReview?> selectPgpassFile() async {
    final found = await guard(native.dbImportDefaultPgpass);
    if (found != 'null') return _review(found);
    final file = await openFile();
    if (file == null) return null;
    final bytes = await file.readAsBytes();
    final contents = SecretText.fromBytes(bytes);
    bytes.fillRange(0, bytes.length, 0);
    return importPgpass(contents);
  }

  @override
  Future<void> confirmImport(String ticket, List<DbImportChoice> choices) async {
    await guard(
      () => native.dbConfirmImport(ticket: ticket, choicesJson: jsonEncode([for (final c in choices) c.toJson()])),
    );
    _local.add(null);
  }

  @override
  Future<void> discardImport(String ticket) => guard(() => native.dbDiscardImport(ticket: ticket));

  // Sessions and queries.

  @override
  Future<DbSessionInfo> connect(String connectionId, {SecretText? temporaryPassword}) async => _session(
    await _secret(temporaryPassword, (pw) => guard(() => native.dbConnect(connectionId: connectionId, password: pw))),
  );

  @override
  Future<void> disconnect(String sessionId) async {
    _sessions.remove(sessionId);
    await guard(() => native.dbDisconnect(sessionId: sessionId));
  }

  @override
  Future<DbPrivilegeReport> checkPrivileges(String sessionId) async {
    final json = jsonDecode(await guard(() => native.dbPrivileges(sessionId: sessionId))) as DbJson;
    return DbPrivilegeReport(
      serverReadOnly: switch (json['server_read_only']) {
        'enforced' => DbServerReadOnly.enforced,
        'role_verified' => DbServerReadOnly.roleVerified,
        _ => DbServerReadOnly.unverified,
      },
      hasWritePrivileges: json['has_write_privileges'] as bool?,
      warningCodes: (json['warning_codes'] as List? ?? const []).cast<String>(),
    );
  }

  @override
  Future<DbSessionInfo> allowSessionWrite(String sessionId) async =>
      _session(await guard(() => native.dbSetSessionWrite(sessionId: sessionId, allow: true)));

  @override
  Future<DbSessionInfo> revokeSessionWrite(String sessionId) async =>
      _session(await guard(() => native.dbSetSessionWrite(sessionId: sessionId, allow: false)));

  @override
  Future<DbSessionInfo> sessionState(String sessionId) async =>
      _session(await guard(() => native.dbSessionState(sessionId: sessionId)));

  @override
  Future<DbSessionInfo> unlockProduction(String sessionId, {required StepUpProof proof, required int minutes}) async {
    try {
      return _session(
        await withStepUp(
          proof,
          (passphrase, token) => guard(
            () => native.dbUnlockProduction(
              sessionId: sessionId,
              passphrase: passphrase,
              presenceToken: token,
              minutes: minutes,
            ),
          ),
        ),
      );
    } on AppException catch (error) {
      if (error.code == AppErrorCode.wrongPassphrase) throw const DbWrongPassphrase();
      rethrow;
    }
  }

  @override
  Future<DbSessionInfo> lockProduction(String sessionId) async =>
      _session(await guard(() => native.dbLockProduction(sessionId: sessionId)));

  @override
  Future<DbTreePage> tree(String sessionId, {DbObjectRef? parent, String? cursor, bool refresh = false}) async =>
      DbTreePage.fromJson(
        jsonDecode(
          await guard(
            () => native.dbTree(
              sessionId: sessionId,
              parentJson: parent == null ? null : jsonEncode(parent.toJson()),
              cursor: cursor,
            ),
          ),
        ) as DbJson,
      );

  @override
  Future<SecretText> ddl(String sessionId, DbObjectRef object) async =>
      SecretText(await guard(() => native.dbDdl(sessionId: sessionId, objectJson: jsonEncode(object.toJson()))));

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
    final json = jsonDecode(
      await guard(
        () => native.dbPrepareQuery(
          sessionId: sessionId,
          text: text,
          mode: mode.name,
          selectionStart: selectionStart,
          selectionEnd: selectionEnd,
        ),
      ),
    ) as DbJson;
    return DbPreparedQuery(
      text: json['text'] as String,
      parameterNames: (json['parameter_names'] as List).cast<String>(),
    );
  }

  @override
  Future<String> formatQuery(DbEngine engine, String text) =>
      guard(() => native.dbFormatQuery(engine: engine.wireName, text: text));

  @override
  Future<List<DbCompletion>> completeQuery(String sessionId, String text, int cursor) async {
    // The core counts UTF-8 bytes; the editor counts UTF-16 code units.
    final offset = utf8.encode(text.substring(0, cursor.clamp(0, text.length))).length;
    final list =
        jsonDecode(await guard(() => native.dbComplete(sessionId: sessionId, text: text, cursor: offset))) as List;
    return [
      for (final item in list.cast<DbJson>())
        DbCompletion(
          insertText: item['insert_text'] as String,
          label: item['label'] as String,
          detail: item['detail'] as String? ?? '',
        ),
    ];
  }

  @override
  Future<DbQueryReview> reviewQuery(String sessionId, DbQueryRequest request) async {
    final json = jsonDecode(
      await guard(() => native.dbReview(sessionId: sessionId, requestJson: jsonEncode(request.toJson()))),
    ) as DbJson;
    return DbQueryReview(
      ticket: json['ticket'] as String,
      risk: switch (json['risk']) {
        'read_only' => DbQueryRisk.read,
        'write' => DbQueryRisk.write,
        'blocked' => DbQueryRisk.blocked,
        _ => DbQueryRisk.dangerous,
      },
      suggestion: json['suggestion'] as String?,
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
  Future<DbExplainPlan> explain(String sessionId, DbQueryRequest request, {bool analyze = false}) async {
    final json = jsonDecode(
      await guard(
        () => native.dbExplain(sessionId: sessionId, requestJson: jsonEncode(request.toJson()), analyze: analyze),
      ),
    ) as DbJson;
    final plan = json['plan'];
    return DbExplainPlan(
      engine: DbEngine.parse(json['engine'] as String),
      document: plan is Map ? plan.cast<String, Object?>() : {'plan': plan},
      analyzed: json['analyzed'] as bool? ?? analyze,
    );
  }

  // History.

  @override
  Future<DbHistoryPolicy> historyPolicy(String connectionId) async =>
      switch (jsonDecode(await guard(() => native.dbHistoryPolicy(connectionId: connectionId)))) {
        'encrypted_sync' => DbHistoryPolicy.encryptedSync,
        'disabled' => DbHistoryPolicy.disabled,
        _ => DbHistoryPolicy.local,
      };

  @override
  Future<void> setHistoryPolicy(String connectionId, DbHistoryPolicy policy) => guard(
    () => native.dbSetHistoryPolicy(
      connectionId: connectionId,
      policy: switch (policy) {
        DbHistoryPolicy.local => 'local',
        DbHistoryPolicy.encryptedSync => 'encrypted_sync',
        DbHistoryPolicy.disabled => 'disabled',
      },
    ),
  );

  @override
  Future<DbHistoryPage> history(
    String connectionId, {
    String query = '',
    bool? succeeded,
    bool favoritesOnly = false,
    String? cursor,
    int limit = 100,
  }) async {
    final json = jsonDecode(
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
    ) as DbJson;
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
            productionWrite: entry['production_write'] as bool? ?? false,
          ),
      ],
      nextCursor: json['next_cursor'] as String?,
    );
  }

  @override
  Future<void> setFavorite(String historyId, bool favorite) =>
      guard(() => native.dbHistoryFavorite(historyId: historyId, favorite: favorite));

  @override
  Future<void> saveHistorySnippet(String historyId, String name) =>
      guard(() => native.dbSaveHistorySnippet(historyId: historyId, name: name));

  @override
  Future<void> deleteHistory(String connectionId) => guard(() => native.dbDeleteHistory(connectionId: connectionId));

  // Copy and export.

  @override
  Future<SecretText> copyResult(
    String sessionId,
    String queryId,
    int resultIndex,
    DbResultSelection selection,
    DbExportOptions options,
  ) async => SecretText(
    await guard(
      () => native.dbCopyResult(
        queryId: queryId,
        resultIndex: resultIndex,
        selectionJson: jsonEncode(selection.toJson()),
        optionsJson: jsonEncode(options.toJson()),
      ),
    ),
  );

  @override
  Future<DbExportDestination?> chooseExportDestination(DbExportOptions options) async {
    final extension = switch (options.format) {
      DbExportFormat.csv => 'csv',
      DbExportFormat.tsv => 'tsv',
      DbExportFormat.json => 'json',
      DbExportFormat.ndjson => 'ndjson',
      DbExportFormat.markdown => 'md',
      DbExportFormat.insert => 'sql',
    };
    final location = await getSaveLocation(suggestedName: 'result.$extension'); // l10n-ignore: file name
    if (location == null) return null;
    return DbExportDestination(id: location.path, label: location.path.split(Platform.pathSeparator).last);
  }

  @override
  Future<String> startExport(
    String sessionId,
    String queryId,
    int resultIndex,
    DbExportOptions options,
    DbExportDestination destination,
  ) => guard(
    () => native.dbExportStart(
      queryId: queryId,
      resultIndex: resultIndex,
      optionsJson: jsonEncode(options.toJson()),
      path: destination.id,
    ),
  );

  @override
  Stream<DbExportProgress> watchExport(String sessionId, String exportId) async* {
    while (true) {
      final progress = DbExportProgress.fromJson(
        jsonDecode(await guard(() => native.dbExportProgress(exportId: exportId))) as DbJson,
      );
      yield progress;
      if (progress.done || progress.cancelled || progress.error != null) return;
      await Future<void>.delayed(const Duration(milliseconds: 300));
    }
  }

  @override
  Future<void> cancelExport(String sessionId, String exportId) =>
      guard(() => native.dbExportCancel(exportId: exportId));

  // Cell edits (non-production, desktop).

  @override
  Future<DbCellEditReview> reviewCellEdits(String sessionId, DbObjectRef table, List<DbCellEdit> edits) async {
    final json = jsonDecode(
      await guard(
        () => native.dbReviewCellEdits(
          sessionId: sessionId,
          tableJson: jsonEncode(table.toJson()),
          editsJson: jsonEncode([for (final edit in edits) edit.toJson()]),
        ),
      ),
    ) as DbJson;
    return DbCellEditReview(
      ticket: json['ticket'] as String,
      sql: SecretText(json['sql'] as String),
      expiresAt: DateTime.now().add(Duration(seconds: json['expires_in_seconds'] as int? ?? 60)),
      rows: json['rows'] as int,
    );
  }

  @override
  Future<void> commitCellEdits(String sessionId, String ticket) =>
      guard(() => native.dbCommitCellEdits(sessionId: sessionId, ticket: ticket));

  @override
  Future<void> discardCellEdits(String sessionId, String ticket) =>
      guard(() => native.dbDiscardCellEdits(ticket: ticket));

  @override
  Future<DbJson> inspectObject(String sessionId, DbObjectRef object, {int? sampleDocuments}) async {
    final value = jsonDecode(
      await guard(
        () => native.dbInspect(sessionId: sessionId, objectJson: jsonEncode(object.toJson()), sample: sampleDocuments),
      ),
    );
    return value is Map ? value.cast<String, dynamic>() : {'value': value};
  }

  // Query assistant.

  static DbAiProvider _provider(DbJson json) =>
      DbAiProvider(id: json['id'] as String, name: json['name'] as String, external: json['external'] as bool? ?? true);

  @override
  Future<List<DbAiProvider>> aiProviders() async => [
    for (final item in (jsonDecode(await guard(native.dbAiProviders)) as List).cast<DbJson>()) _provider(item),
  ];

  @override
  Future<DbAiReview> prepareAi(String sessionId, String providerId, DbAiRequest request) async {
    final json = jsonDecode(
      await guard(
        () => native.dbAiPrepare(
          sessionId: sessionId,
          providerId: providerId,
          requestJson: jsonEncode({
            'action': request.action.name,
            'question': request.question,
            'query': request.query,
            'error_code': request.errorCode,
            'plan': request.plan,
            'object': request.object?.toJson(),
            'target_engine': request.targetEngine?.wireName,
            'conversation_id': request.conversationId,
            'query_id': request.queryId,
            'result_index': request.resultIndex,
            'selected_rows': request.selectedRows,
            'context_limit': request.contextLimit,
          }),
        ),
      ),
    ) as DbJson;
    return DbAiReview(
      ticket: json['ticket'] as String,
      provider: _provider(json['provider'] as DbJson),
      connectionName: json['connection_name'] as String? ?? '',
      expiresAt: DateTime.now().add(Duration(seconds: json['expires_in_seconds'] as int? ?? 120)),
      schemaObjects: (json['schema_objects'] as List? ?? const []).cast<String>(),
      hasSelectedRows: json['has_selected_rows'] as bool? ?? false,
      historyMessages: json['history_messages'] as int? ?? 0,
    );
  }

  @override
  Stream<DbAiEvent> askAi(String sessionId, String ticket, DbAiConsent consent) async* {
    await guard(() => native.dbAiAsk(sessionId: sessionId, ticket: ticket, consentJson: jsonEncode(consent.toJson())));
    while (true) {
      final List<DbJson> events;
      try {
        events = (jsonDecode(await guard(() => native.dbAiPoll(ticket: ticket, waitMs: 1000))) as List).cast<DbJson>();
      } on AppException {
        return; // cancelled or locked
      }
      for (final event in events) {
        final kind = DbAiEventKind.values.firstWhere((k) => k.name == event['kind'], orElse: () => DbAiEventKind.error);
        final content = event['content'] as String?;
        yield DbAiEvent(
          kind: kind,
          content: content == null ? null : SecretText(content),
          tool: event['tool'] as String?,
          code: event['code'] as String?,
          conversationId: event['conversation_id'] as String?,
          isDdl: event['is_ddl'] as bool? ?? false,
        );
        if (kind == DbAiEventKind.done || kind == DbAiEventKind.error) return;
      }
    }
  }

  @override
  Future<void> cancelAi(String sessionId, String ticket) => guard(() => native.dbAiCancel(ticket: ticket));

  @override
  Future<void> revokeAiConsent(String sessionId) => guard(() => native.dbAiRevoke(sessionId: sessionId));
}
