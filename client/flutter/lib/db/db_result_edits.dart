import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_result_controller.dart';
import 'package:flutter/foundation.dart';

/// A column of an editable table as the core describes it for adding rows.
final class DbEditColumn {
  const DbEditColumn({
    required this.name,
    this.typeName = '',
    this.nullable = true,
    this.hasDefault = false,
    this.generated = false,
    this.key = false,
  });
  final String name, typeName;
  final bool nullable;

  /// A default, identity or auto-increment value exists.
  final bool hasDefault;

  /// Computed by the server; never written.
  final bool generated;

  /// Part of the key that identifies rows.
  final bool key;

  /// A new row must give this column a value.
  bool get required => !nullable && !hasDefault && !generated;

  factory DbEditColumn.fromJson(DbJson json) => DbEditColumn(
    name: json['name'] as String,
    typeName: json['type_name'] as String? ?? '',
    nullable: json['nullable'] as bool? ?? true,
    hasDefault: json['has_default'] as bool? ?? false,
    generated: json['generated'] as bool? ?? false,
    key: json['key'] as bool? ?? false,
  );
}

enum DbRowChangeKind { insert, update, delete }

/// One pending row change sent to the core, which writes the statements.
final class DbRowChange {
  const DbRowChange.insert(this.values) : kind = DbRowChangeKind.insert, originalPrimaryKey = const {};
  const DbRowChange.update(this.originalPrimaryKey, this.values) : kind = DbRowChangeKind.update;
  const DbRowChange.delete(this.originalPrimaryKey) : kind = DbRowChangeKind.delete, values = const {};
  final DbRowChangeKind kind;
  final Map<String, DbCell> originalPrimaryKey, values;

  DbJson toJson() => {
    'kind': kind.name,
    if (kind != DbRowChangeKind.insert)
      'original_primary_key': originalPrimaryKey.map((key, value) => MapEntry(key, value.toJson())),
    if (kind != DbRowChangeKind.delete) 'values': values.map((key, value) => MapEntry(key, value.toJson())),
  };

  @override
  String toString() => 'DbRowChange(${kind.name}, <redacted>)';
}

/// The reviewed statements: [sql] has the values written out for display;
/// what runs keeps them bound. Committed with [DbCellEditService.commitCellEdits].
final class DbRowChangeReview {
  const DbRowChangeReview({
    required this.ticket,
    required this.sql,
    required this.expiresAt,
    this.risk = DbQueryRisk.write,
    this.inserted = 0,
    this.updated = 0,
    this.deleted = 0,
  });
  final String ticket;
  final SecretText sql;
  final DateTime expiresAt;
  final DbQueryRisk risk;
  final int inserted, updated, deleted;
}

/// Added, duplicated and deleted rows next to cell edits.
abstract interface class DbRowEditService implements DbCellEditService {
  Future<List<DbEditColumn>> editColumns(String sessionId, DbObjectRef table);
  Future<DbRowChangeReview> reviewRowChanges(String sessionId, DbObjectRef table, List<DbRowChange> changes);
}

/// Pending changes of one result: values typed into loaded cells, rows
/// marked for deletion and new rows. Nothing runs until they are saved.
/// Row indexes past the loaded rows address [added] rows.
class DbResultEdits extends ChangeNotifier {
  /// Typed values of loaded rows by (row, column).
  final cells = <(int, int), DbCell>{};

  /// Loaded rows to delete.
  final deleted = <int>{};

  /// New rows; a null value is left to the server (its default or NULL).
  final added = <List<DbCell?>>[];

  /// The table's columns once asked for (required and default columns).
  List<DbEditColumn>? columns;

  bool get isEmpty => cells.isEmpty && deleted.isEmpty && added.isEmpty;
  bool get isNotEmpty => !isEmpty;

  /// Rows the save changes: edited loaded rows, deleted rows and new rows.
  int get count => {for (final (row, _) in cells.keys) row}.difference(deleted).length + deleted.length + added.length;

  bool isAdded(DbResultSet result, int row) => row >= result.rows.length;
  bool isDeleted(int row) => deleted.contains(row);

  /// The shown value of a cell: typed, added (null = not given) or loaded.
  DbCell? value(DbResultSet result, int row, int column) {
    final loaded = result.rows.length;
    if (row >= loaded) {
      final values = added.elementAtOrNull(row - loaded);
      return values == null || column >= values.length ? null : values[column];
    }
    return cells[(row, column)] ?? result.rows[row][column];
  }

  /// Whether a cell holds a value that is not saved yet.
  bool changed(DbResultSet result, int row, int column) =>
      row >= result.rows.length ? value(result, row, column) != null : cells.containsKey((row, column));

  void setCell(DbResultSet result, int row, int column, DbCell value) {
    final loaded = result.rows.length;
    if (row >= loaded) {
      final values = added.elementAtOrNull(row - loaded);
      if (values == null) return;
      values[column] = value;
    } else {
      final original = result.rows[row][column];
      if (original.kind == value.kind && original.text == value.text) {
        cells.remove((row, column));
      } else {
        cells[(row, column)] = value;
      }
    }
    notifyListeners();
  }

  /// Leaves a cell of a new row to the server again.
  void resetCell(DbResultSet result, int row, int column) {
    final values = added.elementAtOrNull(row - result.rows.length);
    if (values == null) return;
    values[column] = null;
    notifyListeners();
  }

  /// Appends an empty row and returns its row index.
  int addRow(DbResultSet result) {
    added.add(List<DbCell?>.filled(result.columns.length, null));
    notifyListeners();
    return result.rows.length + added.length - 1;
  }

  /// Appends a copy of [row] without key, generated, default-keyed or
  /// masked values (the server or the user gives those); returns its index.
  int duplicateRow(DbResultSet result, int row) {
    final copy = <DbCell?>[
      for (var column = 0; column < result.columns.length; column++)
        _copied(result, column) ? value(result, row, column) : null,
    ];
    added.add(copy);
    notifyListeners();
    return result.rows.length + added.length - 1;
  }

  bool _copied(DbResultSet result, int column) {
    final info = result.columns[column];
    if (info.primaryKey || info.sensitive) return false;
    final meta = columns?.where((c) => c.name == info.name).firstOrNull;
    return meta == null || !(meta.key || meta.generated);
  }

  /// Marks a loaded row for deletion (again: unmarks it); a new row is
  /// simply removed.
  void toggleDelete(DbResultSet result, int row) {
    final loaded = result.rows.length;
    if (row >= loaded) {
      if (row - loaded < added.length) added.removeAt(row - loaded);
    } else if (!deleted.remove(row)) {
      deleted.add(row);
    }
    notifyListeners();
  }

  void clear() {
    if (isEmpty) return;
    cells.clear();
    deleted.clear();
    added.clear();
    notifyListeners();
  }

  /// Columns a new row still lacks: required by the table and not given.
  List<String> missing(DbResultSet result, int row) {
    final meta = columns;
    if (meta == null || row < result.rows.length) return const [];
    return [
      for (var column = 0; column < result.columns.length; column++)
        if (meta.where((c) => c.name == result.columns[column].name).firstOrNull?.required ?? false)
          if ((value(result, row, column)?.kind ?? DbCellKind.nullValue) == DbCellKind.nullValue)
            result.columns[column].name,
    ];
  }

  /// The first new row that misses a required value, or null.
  int? firstIncomplete(DbResultSet result) {
    for (var i = 0; i < added.length; i++) {
      if (missing(result, result.rows.length + i).isNotEmpty) return result.rows.length + i;
    }
    return null;
  }

  /// The changes for the core, keyed by the loaded key columns.
  List<DbRowChange> changes(DbResultSet result) {
    Map<String, DbCell> key(int row) => {
      for (var i = 0; i < result.columns.length; i++)
        if (result.columns[i].primaryKey) result.columns[i].name: result.rows[row][i],
    };
    final byRow = <int, Map<String, DbCell>>{};
    for (final MapEntry(key: (row, column), :value) in cells.entries) {
      if (deleted.contains(row) || row >= result.rows.length) continue;
      (byRow[row] ??= {})[result.columns[column].name] = value;
    }
    final rows = byRow.keys.toList()..sort();
    return [
      for (final row in deleted.toList()..sort())
        if (row < result.rows.length) DbRowChange.delete(key(row)),
      for (final row in rows) DbRowChange.update(key(row), byRow[row]!),
      for (final values in added)
        DbRowChange.insert({
          for (var i = 0; i < values.length && i < result.columns.length; i++)
            if (values[i] case final value?) result.columns[i].name: value,
        }),
    ];
  }

  @override
  void dispose() {
    cells.clear();
    deleted.clear();
    added.clear();
    columns = null;
    super.dispose();
  }
}
