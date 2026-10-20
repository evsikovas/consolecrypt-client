import 'dart:async';

import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:flutter/foundation.dart';

final class DbResultSet {
  DbResultSet({required this.index, required this.columns, this.editableTable});
  final int index;
  final List<DbColumn> columns;
  final DbObjectRef? editableTable;
  final rows = <List<DbCell>>[];
  int elapsedMs = 0, revision = 0;
  int? affectedRows;
  bool hasMore = false, truncated = false;
  void clear() {
    rows.clear();
    revision++;
  }
}

/// Keeps a bounded in-memory result. It never persists cell values and never
/// restarts SQL to fetch another page. Late results are fenced by both the
/// caller's profile/session predicate and this controller's query generation.
class DbResultController extends ChangeNotifier {
  DbResultController({
    required this.service,
    required this.sessionId,
    required this.isCurrent,
    this.memoryLimit = 64 * 1024 * 1024,
  });
  final DbQueryService service;
  final String sessionId;
  final bool Function() isCurrent;
  final int memoryLimit;
  final results = <DbResultSet>[];
  final notices = <String>[];
  String? queryId, errorCode;
  bool running = false, loadingMore = false, cancelled = false, truncated = false;
  int _generation = 0, _bytes = 0;
  bool _disposed = false;
  StreamSubscription<DbQueryEvent>? _subscription;
  Timer? _notification;
  bool get _current => !_disposed && isCurrent();
  void _notify() {
    if (!_current || _notification != null) return;
    _notification = Timer(const Duration(milliseconds: 16), () {
      _notification = null;
      if (_current) notifyListeners();
    });
  }

  void _release(String id, {bool cancel = false}) {
    unawaited(() async {
      if (cancel) {
        try {
          await service.cancelQuery(sessionId, id);
        } catch (_) {}
      }
      try {
        await service.releaseQuery(sessionId, id);
      } catch (_) {}
    }());
  }

  Future<void> execute(DbQueryRequest request) async {
    if (!_current || running || loadingMore) return;
    final generation = ++_generation;
    await _subscription?.cancel();
    if (!_current || generation != _generation) return;
    _subscription = null;
    final old = queryId;
    queryId = null;
    if (old != null) _release(old);
    for (final result in results) {
      result.clear();
    }
    results.clear();
    notices.clear();
    _bytes = 0;
    running = true;
    cancelled = false;
    truncated = false;
    errorCode = null;
    _notify();
    try {
      final id = await service.execute(sessionId, request);
      if (!_current || generation != _generation) {
        _release(id, cancel: true);
        return;
      }
      queryId = id;
      _subscription = service
          .watchQuery(sessionId, id)
          .listen(
            (event) {
              if (!_current || generation != _generation) return;
              _event(event);
            },
            onError: (Object _) {
              if (!_current || generation != _generation) return;
              running = false;
              loadingMore = false;
              errorCode = 'query_failed';
              _notify();
            },
            onDone: () {
              if (!_current || generation != _generation) return;
              if (running) {
                errorCode = 'session_closed';
                running = false;
              }
              loadingMore = false;
              _notify();
            },
          );
    } catch (_) {
      if (_current && generation == _generation) {
        running = false;
        errorCode = 'query_failed';
        _notify();
      }
    }
  }

  void _limit() {
    truncated = true;
    for (final result in results) {
      result.truncated = true;
      result.hasMore = false;
    }
    unawaited(cancel());
  }

  void _event(DbQueryEvent event) {
    DbResultSet? result() => results.where((r) => r.index == event.resultIndex).firstOrNull;
    switch (event.kind) {
      case DbQueryEventKind.started:
        if (results.length >= 64 || event.columns.length > 4096 || result() != null) {
          _limit();
          break;
        }
        final columnBytes = event.columns.fold<int>(0, (n, c) => n + 64 + (c.name.length + c.typeName.length) * 2);
        if (_bytes + columnBytes > memoryLimit) {
          _limit();
          break;
        }
        _bytes += columnBytes;
        results.add(
          DbResultSet(
            index: event.resultIndex,
            columns: List.unmodifiable(event.columns),
            editableTable: event.editableTable,
          ),
        );
      case DbQueryEventKind.rows:
        final target = result();
        if (target == null || event.rows.length > 128) {
          errorCode = 'invalid_result';
          unawaited(cancel());
          break;
        }
        var eventBytes = 0;
        for (final row in event.rows) {
          if (row.length != target.columns.length) {
            errorCode = 'invalid_result';
            unawaited(cancel());
            break;
          }
          final bytes = row.fold<int>(32, (n, cell) => n + 32 + cell.text.length * 2);
          eventBytes += bytes;
          if (eventBytes > 2 * 1024 * 1024 || _bytes + bytes > memoryLimit || target.rows.length >= 100000) {
            _limit();
            break;
          }
          target.rows.add(List.unmodifiable(row));
          _bytes += bytes;
          target.revision++;
        }
      case DbQueryEventKind.notice:
        final message = event.message;
        if (message != null && notices.length < 100 && message.length <= 16384) {
          notices.add(message);
          _bytes += message.length * 2;
        }
        if (_bytes > memoryLimit) _limit();
      case DbQueryEventKind.paused:
        final target = result();
        if (target != null) target.hasMore = event.hasMore;
        running = false;
        loadingMore = false;
      case DbQueryEventKind.statementDone:
        final target = result();
        if (target != null) {
          target.elapsedMs = event.elapsedMs;
          target.affectedRows = event.affectedRows;
          target.truncated = event.truncated;
          target.hasMore = false;
        }
        loadingMore = false;
      case DbQueryEventKind.done:
        running = false;
        loadingMore = false;
      case DbQueryEventKind.cancelled:
        running = false;
        loadingMore = false;
        cancelled = true;
      case DbQueryEventKind.error:
        running = false;
        loadingMore = false;
        errorCode = event.code ?? 'query_failed';
    }
    _notify();
  }

  Future<void> more({int rows = 1000}) async {
    final id = queryId, generation = _generation;
    if (!_current || id == null || running || loadingMore || truncated || !results.any((r) => r.hasMore)) return;
    loadingMore = true;
    running = true;
    _notify();
    try {
      await service.loadMore(sessionId, id, rows: rows.clamp(1, 1000));
    } catch (_) {
      if (_current && generation == _generation) {
        running = false;
        loadingMore = false;
        errorCode = 'query_failed';
        _notify();
      }
    }
  }

  Future<void> cancel() async {
    if (!_current) return;
    final id = queryId;
    _generation++;
    final subscription = _subscription;
    _subscription = null;
    unawaited(subscription?.cancel() ?? Future<void>.value());
    running = false;
    loadingMore = false;
    cancelled = true;
    for (final result in results) {
      result.hasMore = false;
    }
    _notify();
    if (id != null) {
      try {
        await service.cancelQuery(sessionId, id);
      } catch (_) {}
    }
  }

  @override
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _generation++;
    _notification?.cancel();
    unawaited(_subscription?.cancel() ?? Future<void>.value());
    final id = queryId;
    if (id != null) _release(id, cancel: running || loadingMore || results.any((r) => r.hasMore));
    queryId = null;
    for (final result in results) {
      result.clear();
    }
    results.clear();
    notices.clear();
    super.dispose();
  }
}
