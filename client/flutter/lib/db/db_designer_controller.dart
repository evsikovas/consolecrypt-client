import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/db/db_designer_models.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_tree_overlay.dart';
import 'package:material_ui/material_ui.dart';

/// One open table designer: the draft design, its undo history and the
/// latest plan (DDL + validation) from the core. Nothing runs on the server
/// until the user saves through the review dialog.
class DbDesignerController extends ChangeNotifier {
  DbDesignerController._({
    required this.id,
    required this.service,
    required this.sessionId,
    required this.dialect,
    required DbTableDesign design,
    this.original,
  }) : design = design,
       _baseline = design.encode() {
    schedulePlan(immediate: true);
  }

  /// A new table (or collection) in [container] (a schema or database).
  factory DbDesignerController.create({
    required int id,
    required DbTableDesignService service,
    required String sessionId,
    required DbDesignDialect dialect,
    required DbObjectRef container,
    required String name,
  }) {
    final engine = dialect.engine;
    final schema = container.kind == DbObjectKind.schema
        ? container.name
        : (dialect.schemas ? (container.schema ?? 'public') : null);
    final database = container.kind == DbObjectKind.database ? container.name : container.database;
    final design = DbTableDesign(database: database, schema: schema, name: name);
    if (!dialect.collection) {
      final key = DbDesignColumn(
        name: 'id', // l10n-ignore: column name
        dataType: switch (engine) {
          DbEngine.clickhouse => 'UInt64',
          _ => 'bigint',
        },
        nullable: false,
        identity: dialect.identityKinds.contains('by_default')
            ? 'by_default'
            : dialect.identityKinds.contains('auto_increment')
            ? 'auto_increment'
            : '',
        unsigned: dialect.has('unsigned'),
      );
      design.columns.add(key);
      if (dialect.has('primary_key')) design.primaryKey.columns.add(key.name);
    }
    return DbDesignerController._(id: id, service: service, sessionId: sessionId, dialect: dialect, design: design);
  }

  /// An existing table loaded from the server.
  factory DbDesignerController.alter({
    required int id,
    required DbTableDesignService service,
    required String sessionId,
    required DbDesignDialect dialect,
    required DbTableDesign loaded,
  }) => DbDesignerController._(
    id: id,
    service: service,
    sessionId: sessionId,
    dialect: dialect,
    design: loaded.copy(),
    original: loaded,
  );

  final int id;
  final DbTableDesignService service;
  final String sessionId;
  final DbDesignDialect dialect;
  DbEngine get engine => dialect.engine;

  /// The structure on the server (alter mode); null while creating.
  DbTableDesign? original;
  DbTableDesign design;
  String _baseline;
  final _undo = <String>[];
  String? _coalesce;
  DateTime _lastEdit = DateTime.fromMillisecondsSinceEpoch(0);

  DbDesignPlan plan = const DbDesignPlan();
  bool planning = false, planFailed = false;
  int _planGeneration = 0;
  Timer? _planTimer;
  bool _disposed = false;

  bool get alter => original != null;
  bool get dirty => design.encode() != _baseline;

  /// Unsaved work: any change, or a table that does not exist yet.
  bool get pending => !alter || dirty;
  bool get canUndo => _undo.isNotEmpty;

  /// Saving is possible (a plan still being made is checked again on save).
  bool get canSave => pending && !plan.hasErrors && (planning || plan.statements.isNotEmpty);

  /// Name for tabs and dialogs.
  String get title => design.name.trim().isEmpty ? (original?.name ?? '') : design.name.trim();

  /// Applies [change] to the design as one undo step. Typing into the same
  /// field ([coalesce]) within a second stays one step.
  void edit(void Function(DbTableDesign design) change, {String? coalesce}) {
    final now = DateTime.now();
    final merge = coalesce != null && coalesce == _coalesce && now.difference(_lastEdit).inMilliseconds < 1000;
    final before = design.encode();
    change(design);
    if (design.encode() == before) return;
    if (!merge) {
      _undo.add(before);
      if (_undo.length > 200) _undo.removeAt(0);
    }
    _coalesce = coalesce;
    _lastEdit = now;
    notifyListeners();
    schedulePlan();
  }

  void undo() {
    if (_undo.isEmpty) return;
    design = DbTableDesign.fromJson(_decode(_undo.removeLast()));
    _coalesce = null;
    notifyListeners();
    schedulePlan();
  }

  /// Drops every pending change.
  void revertAll() {
    if (!dirty) return;
    _undo.add(design.encode());
    design = DbTableDesign.fromJson(_decode(_baseline));
    _coalesce = null;
    notifyListeners();
    schedulePlan();
  }

  static DbJson _decode(String text) => jsonDecode(text) as DbJson;

  void schedulePlan({bool immediate = false}) {
    _planTimer?.cancel();
    planning = true;
    _planTimer = Timer(immediate ? Duration.zero : const Duration(milliseconds: 250), () => unawaited(refreshPlan()));
  }

  Future<void> refreshPlan() async {
    final generation = ++_planGeneration;
    planning = true;
    try {
      final next = await service.planDesign(engine, original, design.copy());
      if (_disposed || generation != _planGeneration) return;
      plan = next;
      planFailed = false;
    } catch (_) {
      if (_disposed || generation != _planGeneration) return;
      planFailed = true;
    }
    planning = false;
    notifyListeners();
  }

  /// After a successful save: the design is now what the server has.
  void saved(DbTableDesign? reloaded) {
    final current = reloaded ?? _markLoaded(design.copy());
    original = current;
    design = current.copy();
    _baseline = design.encode();
    _undo.clear();
    notifyListeners();
    schedulePlan(immediate: true);
  }

  static DbTableDesign _markLoaded(DbTableDesign design) {
    design.original = design.name;
    for (final column in design.columns) {
      column.original = column.name;
    }
    if (design.primaryKey.columns.isNotEmpty) design.primaryKey.original = design.primaryKey.name;
    for (final key in design.uniques) {
      key.original = key.name;
    }
    for (final key in design.foreignKeys) {
      key.original = key.name;
    }
    for (final index in design.indexes) {
      index.original = index.name;
    }
    for (final check in design.checks) {
      check.original = check.name;
    }
    return design;
  }

  // Columns ----------------------------------------------------------------

  DbDesignColumn newColumn() {
    var n = design.columns.length + 1;
    String name() => 'column$n'; // l10n-ignore: generated column name
    while (design.columns.any((c) => c.name == name())) {
      n++;
    }
    return DbDesignColumn(name: name(), dataType: dialect.defaultType, nullable: engine != DbEngine.clickhouse);
  }

  void addColumn() => edit((d) => d.columns.add(newColumn()));

  void duplicateColumn(int index) => edit((d) {
    final copy = d.columns[index].copy()
      ..original = null
      ..identity = '';
    var name = '${copy.name}_copy'; // l10n-ignore: generated column name
    while (d.columns.any((c) => c.name == name)) {
      name = '${name}_1';
    }
    d.columns.insert(index + 1, copy..name = name);
  });

  void removeColumn(int index) => edit((d) {
    final name = d.columns.removeAt(index).name;
    _forEachColumnList(d, (list) => list.remove(name));
    d.uniques.removeWhere((k) => k.columns.isEmpty);
    d.indexes.removeWhere((k) => k.columns.isEmpty);
    d.foreignKeys.removeWhere((k) => k.columns.isEmpty);
  });

  /// Brings back a server column the draft drops.
  void restoreColumn(String name) => edit((d) {
    final column = original?.columns.where((c) => c.name == name).firstOrNull;
    if (column != null && !d.columns.any((c) => c.original == name)) d.columns.add(column.copy());
  });

  void moveColumn(int from, int to) => edit((d) {
    if (from == to || from < 0 || from >= d.columns.length) return;
    final column = d.columns.removeAt(from);
    d.columns.insert(to.clamp(0, d.columns.length), column);
  });

  /// Renames a column and every key, index and foreign key that uses it.
  void renameColumn(int index, String name) => edit((d) {
    final old = d.columns[index].name;
    d.columns[index].name = name;
    _forEachColumnList(d, (list) {
      for (var i = 0; i < list.length; i++) {
        if (list[i] == old) list[i] = name;
      }
    });
  }, coalesce: 'column.$index.name');

  void updateColumn(int index, void Function(DbDesignColumn column) change, {String? field}) =>
      edit((d) => change(d.columns[index]), coalesce: field == null ? null : 'column.$index.$field');

  static void _forEachColumnList(DbTableDesign d, void Function(List<String> list) visit) {
    visit(d.primaryKey.columns);
    for (final key in d.uniques) {
      visit(key.columns);
    }
    for (final index in d.indexes) {
      visit(index.columns);
    }
    for (final key in d.foreignKeys) {
      visit(key.columns);
    }
  }

  bool isPrimary(String column) => design.primaryKey.columns.contains(column);

  void togglePrimary(String column) => edit((d) {
    if (!d.primaryKey.columns.remove(column)) {
      d.primaryKey.columns.add(column);
      // Key columns cannot be NULL.
      for (final c in d.columns.where((c) => c.name == column)) {
        if (engine != DbEngine.clickhouse) c.nullable = false;
      }
    }
  });

  bool isUnique(String column) => design.uniques.any((k) => k.columns.length == 1 && k.columns.first == column);

  void toggleUnique(String column) => edit((d) {
    final existing = d.uniques.where((k) => k.columns.length == 1 && k.columns.first == column).toList();
    if (existing.isEmpty) {
      d.uniques.add(DbDesignKey(columns: [column]));
    } else {
      d.uniques.removeWhere(existing.contains);
    }
  });

  // Markers ------------------------------------------------------------------

  DbDesignColumn? _originalColumn(DbDesignColumn column) =>
      original?.columns.where((c) => c.name == column.original).firstOrNull;

  /// New or changed compared with the server (or with the start of a new
  /// table: every column of a new table is new).
  DbDraftState? columnState(int index) {
    final column = design.columns[index];
    if (!alter || column.original == null) return DbDraftState.added;
    final before = _originalColumn(column);
    if (before == null) return DbDraftState.added;
    final a = before.toJson(), b = column.toJson();
    return a.toString() == b.toString() ? null : DbDraftState.changed;
  }

  /// Columns of the server table that the draft drops.
  List<String> get droppedColumns => [
    for (final column in original?.columns ?? const <DbDesignColumn>[])
      if (!design.columns.any((c) => c.original == column.name)) column.name,
  ];

  bool get headerChanged =>
      alter &&
      (design.name != original!.name || design.comment != original!.comment || design.schema != original!.schema);

  List<DbDesignIssue> issuesFor(String prefix) => [
    for (final issue in plan.issues)
      if (issue.target == prefix || issue.target.startsWith('$prefix.')) issue,
  ];

  // Tree references -------------------------------------------------------------

  /// The table (or collection) as the structure tree names it.
  DbObjectRef tableRef([DbTableDesign? of]) {
    final d = of ?? design;
    return DbObjectRef(
      database: engine == DbEngine.postgresql ? '' : d.database,
      schema: engine == DbEngine.postgresql ? d.schema : null,
      kind: dialect.collection ? DbObjectKind.collection : DbObjectKind.table,
      name: d.name,
    );
  }

  /// The schema or database node the table lives in.
  bool inContainer(DbObjectRef parent) => switch (parent.kind) {
    DbObjectKind.schema => engine == DbEngine.postgresql && parent.name == design.schema,
    DbObjectKind.database => parent.name == design.database,
    _ => false,
  };

  bool isTable(DbObjectRef object, DbTableDesign d) =>
      (object.kind == DbObjectKind.table || object.kind == DbObjectKind.collection) &&
      object.name == d.name &&
      (engine == DbEngine.postgresql ? object.schema == d.schema : object.database == d.database);

  DbTreeNode columnNode(DbDesignColumn column, String table) => DbTreeNode(
    reference: DbObjectRef(
      database: engine == DbEngine.postgresql ? '' : design.database,
      schema: engine == DbEngine.postgresql ? design.schema : null,
      kind: DbObjectKind.column,
      name: column.name,
      parentName: table,
    ),
    hasChildren: false,
    detail: column.typeLabel,
    primaryKey: isPrimary(column.name),
  );

  @override
  void dispose() {
    _disposed = true;
    _planTimer?.cancel();
    super.dispose();
  }
}

/// The open designers of one workspace, shown in its structure tree as
/// drafts: new tables and columns under their parents, changed ones marked.
class DbDesignerHub extends ChangeNotifier implements DbTreeOverlay {
  DbDesignerHub({required this.onFocus});
  final void Function(DbDesignerController designer) onFocus;
  final designers = <DbDesignerController>[];
  final _reloads = <DbObjectRef>[], _reveals = <DbObjectRef>[];

  void add(DbDesignerController designer, {DbObjectRef? reveal}) {
    designers.add(designer);
    if (reveal != null) _reveals.add(reveal);
    designer.addListener(notifyListeners);
    notifyListeners();
  }

  void remove(DbDesignerController designer) {
    designers.remove(designer);
    designer.removeListener(notifyListeners);
    designer.dispose();
    notifyListeners();
  }

  void clear() {
    for (final designer in List.of(designers)) {
      designer
        ..removeListener(notifyListeners)
        ..dispose();
    }
    designers.clear();
    _reloads.clear();
  }

  /// A saved design: reload its container and the table itself.
  void reload(List<DbObjectRef> parents) {
    _reloads.addAll(parents);
    notifyListeners();
  }

  @override
  List<DbObjectRef> takeReveals() {
    final out = List.of(_reveals);
    _reveals.clear();
    return out;
  }

  @override
  List<DbObjectRef> takeReloads() {
    final out = List.of(_reloads);
    _reloads.clear();
    return out;
  }

  @override
  List<DbDraftNode> childrenOf(DbObjectRef? parent) {
    if (parent == null) return const [];
    final out = <DbDraftNode>[];
    for (final d in designers) {
      if (!d.alter && d.inContainer(parent) && d.design.name.trim().isNotEmpty) {
        out.add(
          DbDraftNode(
            node: DbTreeNode(reference: d.tableRef(), hasChildren: d.design.columns.isNotEmpty),
            state: DbDraftState.added,
          ),
        );
      } else if (!d.alter && d.isTable(parent, d.design)) {
        for (final column in d.design.columns) {
          out.add(DbDraftNode(node: d.columnNode(column, d.design.name), state: DbDraftState.added));
        }
      } else if (d.alter && d.isTable(parent, d.original!)) {
        for (final column in d.design.columns.where((c) => c.original == null)) {
          out.add(DbDraftNode(node: d.columnNode(column, d.original!.name), state: DbDraftState.added));
        }
      }
    }
    return out;
  }

  @override
  DbDraftState? stateOf(DbObjectRef object) {
    for (final d in designers.where((d) => d.alter && d.dirty)) {
      final original = d.original!;
      if (d.isTable(object, original)) return DbDraftState.changed;
      if (object.kind == DbObjectKind.column && object.parentName == original.name) {
        final at = d.design.columns.indexWhere((c) => c.original == object.name);
        if (at < 0) return DbDraftState.changed; // dropped
        if (d.columnState(at) != null) return DbDraftState.changed;
      }
    }
    return null;
  }

  DbDesignerController? _owner(DbObjectRef object) {
    for (final d in designers) {
      final table = d.alter ? d.original! : d.design;
      if (d.isTable(object, table) || (object.kind == DbObjectKind.column && object.parentName == table.name)) {
        if (d.alter && !d.dirty) continue;
        return d;
      }
    }
    return null;
  }

  @override
  bool isDraft(DbObjectRef object) {
    for (final d in designers.where((d) => !d.alter)) {
      if (d.isTable(object, d.design)) return true;
      if (object.kind == DbObjectKind.column && object.parentName == d.design.name) return true;
    }
    return false;
  }

  @override
  void focus(DbObjectRef object) {
    final owner = _owner(object);
    if (owner != null) onFocus(owner);
  }

  @override
  void dispose() {
    clear();
    super.dispose();
  }
}
