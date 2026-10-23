import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_selectable_text.dart';
import 'package:consolecrypt/db/db_ddl_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_object_details.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_sqlite_dialect.dart';
import 'package:consolecrypt/db/db_tree_overlay.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// A column's icon by its declared SQLite type (type affinity rules).
IconData _sqliteTypeIcon(String type) {
  final t = type.toUpperCase();
  if (t.contains('BOOL')) return Icons.toggle_on_outlined;
  if (t.contains('DATE') || t.contains('TIME')) return Icons.event_outlined;
  return switch (DbSqliteDialect.affinity(t)) {
    'TEXT' => Icons.text_fields_rounded,
    'BLOB' => Icons.data_object_rounded,
    _ => Icons.onetwothree_rounded,
  };
}

IconData _kindIcon(DbTreeNode node, DbEngine engine) => switch (node.reference.kind) {
  DbObjectKind.column when engine == DbEngine.sqlite && !node.primaryKey && node.foreignKey == null => _sqliteTypeIcon(
    node.detail ?? '',
  ),
  DbObjectKind.database => Icons.storage_rounded,
  DbObjectKind.redisDatabase => node.reference.parentName == null ? Icons.storage_rounded : Icons.folder_outlined,
  DbObjectKind.schema => Icons.account_tree_outlined,
  DbObjectKind.table || DbObjectKind.collection => Icons.table_chart_outlined,
  DbObjectKind.view || DbObjectKind.materializedView => Icons.view_quilt_outlined,
  DbObjectKind.sequence => Icons.format_list_numbered,
  DbObjectKind.function || DbObjectKind.procedure => Icons.functions,
  DbObjectKind.column =>
    node.primaryKey ? Icons.key : (node.foreignKey != null ? Icons.link : Icons.view_column_outlined),
  DbObjectKind.indexObject => Icons.bolt_outlined,
  DbObjectKind.constraint => Icons.rule,
  DbObjectKind.trigger => Icons.flash_on_outlined,
  DbObjectKind.dictionary => Icons.menu_book_outlined,
  DbObjectKind.projection => Icons.layers_outlined,
  DbObjectKind.redisKey => Icons.vpn_key_outlined,
  DbObjectKind.key => Icons.key_outlined,
  DbObjectKind.foreignKey => Icons.link,
  DbObjectKind.reference => Icons.call_received_rounded,
  DbObjectKind.dataType => Icons.category_outlined,
  DbObjectKind.userType => Icons.category_outlined,
  DbObjectKind.aggregate => Icons.functions,
};

/// Indexes and triggers show their statement on engines that keep it
/// (SQLite stores every `CREATE` statement); CQL also describes keyspaces
/// and secondary indexes.
bool _describes(DbEngine engine, DbObjectKind kind) =>
    _describable.contains(kind) ||
    (engine == DbEngine.sqlite && (kind == DbObjectKind.indexObject || kind == DbObjectKind.trigger)) ||
    (engine == DbEngine.cassandra && _describableCql.contains(kind));

/// Objects whose creation statement or description can be shown.
const _describable = {
  DbObjectKind.table,
  DbObjectKind.view,
  DbObjectKind.materializedView,
  DbObjectKind.sequence,
  DbObjectKind.function,
  DbObjectKind.procedure,
  DbObjectKind.dictionary,
  DbObjectKind.collection,
  DbObjectKind.redisKey,
  DbObjectKind.userType,
  DbObjectKind.aggregate,
};

/// Also describable on CQL: keyspaces and secondary indexes.
const _describableCql = {DbObjectKind.database, DbObjectKind.indexObject};

final class _Branch {
  final nodes = <DbTreeNode>[];
  final cursors = <String>{};
  String? cursor;
  bool loading = false, failed = false;
  DateTime? loadedAt;
  int generation = 0;
}

/// Client-side groups of a node's children, like the folders of the classic
/// SQL clients' navigators. Only shown when a node has several kinds.
/// Declaration order is display order: a table's columns, keys, foreign
/// keys, indexes, references and triggers; a file's tables, views, indexes,
/// sequences, triggers and data types; a keyspace's tables, views, types and
/// functions.
enum _Folder {
  tables,
  views,
  types,
  functions,
  dictionaries,
  columns,
  keys,
  foreignKeys,
  indexes,
  references,
  sequences,
  constraints,
  triggers,
  projections,
  dataTypes,
}

_Folder? _folderOf(DbObjectKind kind) => switch (kind) {
  DbObjectKind.table || DbObjectKind.collection => _Folder.tables,
  DbObjectKind.view || DbObjectKind.materializedView => _Folder.views,
  DbObjectKind.function || DbObjectKind.procedure || DbObjectKind.aggregate => _Folder.functions,
  DbObjectKind.userType => _Folder.types,
  DbObjectKind.sequence => _Folder.sequences,
  DbObjectKind.dictionary => _Folder.dictionaries,
  DbObjectKind.column => _Folder.columns,
  DbObjectKind.indexObject => _Folder.indexes,
  DbObjectKind.constraint => _Folder.constraints,
  DbObjectKind.trigger => _Folder.triggers,
  DbObjectKind.projection => _Folder.projections,
  DbObjectKind.key => _Folder.keys,
  DbObjectKind.foreignKey => _Folder.foreignKeys,
  DbObjectKind.reference => _Folder.references,
  DbObjectKind.dataType => _Folder.dataTypes,
  _ => null,
};

String _folderLabel(AppLocalizations l, _Folder folder, {DbObjectRef? parent}) => switch (folder) {
  // All triggers of a database file, next to its tables.
  _Folder.triggers when parent?.kind == DbObjectKind.database => l.dbFolderTableTriggers,
  _Folder.tables => l.dbFolderTables,
  _Folder.views => l.dbFolderViews,
  _Folder.types => l.dbFolderTypes,
  _Folder.functions => l.dbFolderFunctions,
  _Folder.sequences => l.dbFolderSequences,
  _Folder.dictionaries => l.dbFolderDictionaries,
  _Folder.columns => l.dbFolderColumns,
  _Folder.indexes => l.dbFolderIndexes,
  _Folder.constraints => l.dbFolderConstraints,
  _Folder.triggers => l.dbFolderTriggers,
  _Folder.projections => l.dbFolderProjections,
  _Folder.keys => l.dbFolderKeys,
  _Folder.foreignKeys => l.dbFolderForeignKeys,
  _Folder.references => l.dbFolderReferences,
  _Folder.dataTypes => l.dbFolderDataTypes,
};

/// Folders that start open: the ones people reach for first.
const _openByDefault = {_Folder.tables, _Folder.columns};

/// Expanded objects whose contents are framed as one highlighted block.
const _blockKinds = {
  DbObjectKind.table,
  DbObjectKind.view,
  DbObjectKind.materializedView,
  DbObjectKind.collection,
  DbObjectKind.dictionary,
};

const _rowHeight = 26.0, _indentStep = 14.0;
double _indent(int depth) => 6.0 + depth * _indentStep;

/// One visible line: an object, a folder or a "load more"/progress line.
final class _Row {
  _Row({required this.depth, required this.parent, this.node, this.folder, this.folderId, this.count = 0});
  final int depth;
  final DbObjectRef? parent;
  final DbTreeNode? node;
  final _Folder? folder;
  final String? folderId;
  final int count;

  /// The expanded object block this line belongs to (id and depth).
  String? block;
  int blockDepth = 0;
  bool blockStart = false, blockEnd = false;
}

/// Objects whose first rows open in the data preview tab.
const _previewable = [
  DbObjectKind.table,
  DbObjectKind.view,
  DbObjectKind.materializedView,
  DbObjectKind.collection,
  DbObjectKind.redisKey,
  DbObjectKind.redisDatabase,
];

class DbObjectTree extends ConsumerStatefulWidget {
  const DbObjectTree({
    required this.session,
    required this.service,
    required this.onDraft,
    required this.onInsert,
    required this.onPreview,
    this.onSelect,
    this.onImportCsv,
    this.onCreateFromCsv,
    this.overlay,
    this.extraMenu,
    this.onExtra,
    super.key,
  });
  final DbSessionInfo session;
  final DbQueryService service;

  /// Generated text for the editor ([onDraft]) or an identifier for the caret.
  final ValueChanged<String> onDraft, onInsert;

  /// The first rows of [object]: [text] is its read query, run at once.
  final void Function(DbObjectRef object, String text) onPreview;
  final ValueChanged<DbObjectRef>? onSelect;

  /// CSV import into a table, and a new table typed from a CSV file in a
  /// schema or database (null hides them).
  final ValueChanged<DbObjectRef>? onImportCsv, onCreateFromCsv;

  /// Unsaved designer drafts shown among the loaded objects.
  final DbTreeOverlay? overlay;

  /// More actions for an object (or, for a folder, its parent) and their
  /// handler (the table designer).
  final List<GlassMenuEntry<String>> Function(DbObjectRef object)? extraMenu;
  final void Function(String action, DbObjectRef object)? onExtra;
  @override
  ConsumerState<DbObjectTree> createState() => _DbObjectTreeState();
}

class _DbObjectTreeState extends ConsumerState<DbObjectTree> {
  late final DbScope _scope = ref.read(dbScopeProvider);
  final _branches = <String, _Branch>{}, _expanded = <String>{};
  final _folders = <String, bool>{};

  /// Draft states of the rows being built (from [DbObjectTree.overlay]).
  final _draftStates = <String, DbDraftState>{};
  String _filter = '';
  String? _selected;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));
  @override
  void initState() {
    super.initState();
    widget.overlay?.addListener(_overlayChanged);
    unawaited(_load(null));
  }

  @override
  void didUpdateWidget(DbObjectTree old) {
    super.didUpdateWidget(old);
    if (!identical(old.overlay, widget.overlay)) {
      old.overlay?.removeListener(_overlayChanged);
      widget.overlay?.addListener(_overlayChanged);
    }
  }

  @override
  void dispose() {
    widget.overlay?.removeListener(_overlayChanged);
    super.dispose();
  }

  /// Drafts changed: rebuild; saved designs reload their parents.
  void _overlayChanged() {
    if (!_current) return;
    setState(() {});
    for (final parent in widget.overlay?.takeReloads() ?? const <DbObjectRef>[]) {
      if (_branches.containsKey(parent.id)) unawaited(_load(parent, refresh: true));
    }
    for (final parent in widget.overlay?.takeReveals() ?? const <DbObjectRef>[]) {
      if (_expanded.add(parent.id)) unawaited(_load(parent));
    }
  }

  Future<void> _load(DbObjectRef? parent, {bool more = false, bool refresh = false}) async {
    if (!_current) return;
    final key = parent?.id ?? '';
    final branch = _branches.putIfAbsent(key, _Branch.new);
    if (branch.loading) return;
    if (!more && !refresh && branch.loadedAt != null && DateTime.now().difference(branch.loadedAt!).inSeconds < 30) {
      return;
    }
    final cursor = more ? branch.cursor : null;
    if (more && cursor == null) return;
    final generation = ++branch.generation;
    setState(() {
      branch.loading = true;
      branch.failed = false;
    });
    try {
      final page = await widget.service
          .tree(widget.session.id, parent: parent, cursor: cursor, refresh: refresh)
          .timeout(const Duration(seconds: 30));
      if (!_current || generation != branch.generation) return;
      if (page.nodes.length > 2000 || (more && page.nextCursor != null && branch.cursors.contains(page.nextCursor))) {
        throw const FormatException('Invalid database tree page');
      }
      setState(() {
        if (!more) {
          branch.nodes.clear();
          branch.cursors.clear();
        }
        final ids = branch.nodes.map((n) => n.reference.id).toSet();
        for (final node in page.nodes) {
          if (branch.nodes.length >= 10000) break;
          if (ids.add(node.reference.id)) branch.nodes.add(node);
        }
        if (cursor != null) branch.cursors.add(cursor);
        branch.cursor = branch.nodes.length < 10000 ? page.nextCursor : null;
        branch.loadedAt = DateTime.now();
      });
    } catch (_) {
      if (_current && generation == branch.generation) setState(() => branch.failed = true);
    } finally {
      if (_current && generation == branch.generation) setState(() => branch.loading = false);
    }
  }

  Future<void> _action(String action, DbObjectRef object) async {
    if (!_current) return;
    if (widget.onExtra != null && !const {'insert', 'query', 'ddl', 'inspect', 'sample'}.contains(action)) {
      widget.onExtra!(action, object);
      return;
    }
    SecretText? value;
    try {
      switch (action) {
        case 'import-csv':
          widget.onImportCsv?.call(object);
          return;
        case 'create-from-csv':
          widget.onCreateFromCsv?.call(object);
          return;
        case 'insert':
          final identifier = await widget.service.identifier(widget.session.id, object);
          if (_current) widget.onInsert(identifier);
          return;
        case 'query':
          final text = await widget.service.objectQuery(widget.session.id, object);
          if (_current) widget.onPreview(object, text);
          return;
        case 'ddl':
          value = await widget.service.ddl(widget.session.id, object);
          if (!mounted || !_current) return;
          final open = await showAppDialog<bool>(
            context,
            secure: true,
            builder: (_) => DbDdlDialog(title: object.name, value: value!, engine: widget.session.engine),
          );
          if (open == true && _current && !value.isWiped) {
            final blocks = widget.session.engine.semicolons ? splitSqlStatements(value.expose()) : [value.expose()];
            widget.onDraft(blocks.join('\n\n'));
          }
          return;
        case 'inspect':
        case 'sample':
          final service = widget.service;
          if (service is! DbObjectInspectService) return;
          if (action == 'sample') {
            final approved = await showAppDialog<bool>(
              context,
              secure: true,
              builder: (context) => GlassDialog(
                title: context.l10n.dbSampleSchema,
                content: Text(context.l10n.dbSampleHelp),
                secondaryActions: [
                  GlassButton(label: context.l10n.commonCancel, onPressed: () => closeDialog<bool>(context, false)),
                ],
                primaryAction: GlassButton(
                  label: context.l10n.commonContinue,
                  onPressed: () => closeDialog<bool>(context, true),
                ),
              ),
            );
            if (!_current || approved != true) return;
          }
          final document = await (service as DbObjectInspectService).inspectObject(
            widget.session.id,
            object,
            sampleDocuments: action == 'sample' ? 20 : null,
          );
          value = SecretText(jsonEncode(document));
      }
      if (!mounted || !_current || value == null) return;
      await showAppDialog<void>(
        context,
        secure: true,
        builder: (_) => DbObjectDetailsDialog(title: object.name, document: value!),
      );
    } catch (_) {
      if (mounted && _current) {
        showSnack(context, context.l10n.dbOperationFailed, error: true);
      }
    } finally {
      value?.wipe();
    }
  }

  List<_Row> _visible() {
    final rows = <_Row>[];
    _draftStates.clear();
    bool matches(DbTreeNode node) => _filter.isEmpty || node.reference.name.toLowerCase().contains(_filter);
    void walk(DbObjectRef? parent, int depth, String? block, int blockDepth) {
      final branch = _branches[parent?.id ?? ''];
      final drafts = widget.overlay?.childrenOf(parent) ?? const <DbDraftNode>[];
      if ((branch == null && drafts.isEmpty) || depth > 16) return;
      _Row add(_Row row) {
        row
          ..block = block
          ..blockDepth = blockDepth;
        rows.add(row);
        return row;
      }

      void emit(DbTreeNode node, int at) {
        final object = node.reference, open = _expanded.contains(object.id);
        // Keep expanded ancestors visible so search never hides navigation.
        if (!matches(node) && !open) return;
        final row = add(_Row(depth: at, parent: parent, node: node));
        if (!open) return;
        if (_blockKinds.contains(object.kind)) {
          row
            ..block = object.id
            ..blockDepth = at;
          walk(object, at + 1, object.id, at);
        } else {
          walk(object, at + 1, block, blockDepth);
        }
      }

      final groups = <_Folder?, List<DbTreeNode>>{};
      final loaded = branch?.nodes ?? const <DbTreeNode>[];
      for (final d in drafts) {
        _draftStates[d.node.reference.id] = d.state;
      }
      final ids = {for (final node in loaded) node.reference.id};
      for (final node in [
        ...loaded,
        for (final d in drafts)
          if (!ids.contains(d.node.reference.id)) d.node,
      ]) {
        groups.putIfAbsent(_folderOf(node.reference.kind), () => []).add(node);
      }
      final folders = groups.keys.whereType<_Folder>().toList()..sort((a, b) => a.index.compareTo(b.index));
      for (final node in groups[null] ?? const <DbTreeNode>[]) {
        emit(node, depth);
      }
      if (folders.length < 2) {
        for (final folder in folders) {
          for (final node in groups[folder]!) {
            emit(node, depth);
          }
        }
      } else {
        for (final folder in folders) {
          final children = groups[folder]!, id = '${parent?.id ?? ''}#${folder.name}';
          if (_filter.isNotEmpty && !children.any((n) => matches(n) || _expanded.contains(n.reference.id))) continue;
          add(_Row(depth: depth, parent: parent, folder: folder, folderId: id, count: children.length));
          if (_filter.isNotEmpty || (_folders[id] ?? _openByDefault.contains(folder))) {
            for (final node in children) {
              emit(node, depth + 1);
            }
          }
        }
      }
      if (branch != null && (branch.loading || branch.failed || branch.cursor != null)) {
        add(_Row(depth: depth, parent: parent));
      }
    }

    walk(null, 0, null, 0);
    for (var i = 0; i < rows.length; i++) {
      final block = rows[i].block;
      if (block == null) continue;
      rows[i]
        ..blockStart = i == 0 || rows[i - 1].block != block
        ..blockEnd = i == rows.length - 1 || rows[i + 1].block != block;
    }
    return rows;
  }

  /// Actions of one object, offered from the row menu and right click.
  List<GlassMenuEntry<String>> _menu(AppLocalizations l, DbObjectRef object) => [
    if (_previewable.contains(object.kind))
      GlassMenuItem(value: 'query', label: l.dbFirstRows, icon: Icons.table_rows_outlined),
    GlassMenuItem(value: 'insert', label: l.dbInsertName, icon: Icons.input_rounded),
    if (_describes(widget.session.engine, object.kind))
      GlassMenuItem(value: 'ddl', label: l.dbDdl, icon: Icons.code_rounded),
    if (widget.service is DbObjectInspectService && dbHasDetails(widget.session.engine, object)) ...[
      GlassMenuItem(value: 'inspect', label: l.dbInspect, icon: Icons.info_outline_rounded),
      if (object.kind == DbObjectKind.collection)
        GlassMenuItem(value: 'sample', label: l.dbSampleSchema, icon: Icons.schema_outlined),
    ],
    if (widget.onImportCsv != null && object.kind == DbObjectKind.table)
      GlassMenuItem(value: 'import-csv', label: l.dbImportCsv, icon: Icons.file_upload_outlined),
    if (widget.onCreateFromCsv != null &&
        (object.kind == DbObjectKind.schema ||
            (object.kind == DbObjectKind.database && widget.session.engine != DbEngine.postgresql)))
      GlassMenuItem(value: 'create-from-csv', label: l.dbCsvCreateTable, icon: Icons.table_chart_outlined),
    ...?widget.extraMenu?.call(object),
  ];

  Future<void> _menuAt(DbObjectRef object, Rect anchor) async {
    final value = await showGlassMenu<String>(context: context, anchor: anchor, entries: _menu(context.l10n, object));
    if (value != null && _current) await _action(value, object);
  }

  void _toggle(DbObjectRef object) {
    final expanded = _expanded.contains(object.id);
    setState(() => expanded ? _expanded.remove(object.id) : _expanded.add(object.id));
    if (!expanded && !(widget.overlay?.isDraft(object) ?? false)) unawaited(_load(object));
  }

  Widget _line(BuildContext context, _Row row) {
    final l = context.l10n, tokens = GlassTokens.of(context), p = tokens.palette;
    final node = row.node, indent = _indent(row.depth);
    Widget chevron(bool open) => SizedBox(
      width: 16,
      child: Icon(
        open ? Icons.expand_more_rounded : Icons.chevron_right_rounded,
        size: 16,
        color: tokens.secondaryLabel,
      ),
    );
    Widget decorated(Widget child) => CustomPaint(
      painter: _RowDecor(
        row: row,
        guide: tokens.surfaces.separator,
        block: tokens.surfaces.fillField,
        accent: p.accent.withValues(alpha: .55),
      ),
      child: child,
    );
    if (row.folder case final folder?) {
      final id = row.folderId!, open = _filter.isNotEmpty || (_folders[id] ?? _openByDefault.contains(folder));
      final parent = row.parent;
      final extra = parent == null || folder != _Folder.tables
          ? const <GlassMenuEntry<String>>[]
          : widget.extraMenu?.call(parent) ?? const <GlassMenuEntry<String>>[];
      void folderMenu(Offset at) {
        if (extra.isEmpty || parent == null) return;
        unawaited(
          showGlassMenu<String>(context: context, anchor: Rect.fromLTWH(at.dx, at.dy, 1, 1), entries: extra).then((v) {
            if (v != null && _current) widget.onExtra?.call(v, parent);
          }),
        );
      }

      return decorated(
        GestureDetector(
          onSecondaryTapUp: (details) => folderMenu(details.globalPosition),
          onLongPressStart: (details) => folderMenu(details.globalPosition),
          child: GlassInteractive(
            key: ValueKey('db-folder-$id'),
            semanticLabel: _folderLabel(l, folder, parent: row.parent),
            onPressed: () => setState(() => _folders[id] = !open),
            builder: (context, state) => DecoratedBox(
              decoration: ShapeDecoration(
                color: state.hovered ? tokens.surfaces.fillHover : Colors.transparent,
                shape: GlassRadii.shape(tokens.radii.row),
              ),
              child: Padding(
                padding: EdgeInsets.only(left: indent, right: GlassSpacing.s6),
                child: Row(
                  children: [
                    chevron(open),
                    const SizedBox(width: 4),
                    Icon(
                      open ? Icons.folder_open_outlined : Icons.folder_outlined,
                      size: 15,
                      color: tokens.secondaryLabel,
                    ),
                    const SizedBox(width: 6),
                    Text(
                      _folderLabel(l, folder, parent: row.parent),
                      style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                    ),
                    const SizedBox(width: 6),
                    Text('${row.count}', style: tokens.typography.caption.copyWith(color: p.tertiary)),
                  ],
                ),
              ),
            ),
          ),
        ),
      );
    }
    if (node == null) {
      final branch = _branches[row.parent?.id ?? '']!;
      return decorated(
        Padding(
          padding: EdgeInsets.only(left: indent + 20),
          child: Align(
            alignment: Alignment.centerLeft,
            child: branch.loading
                ? const SizedBox(width: 80, child: LinearProgressIndicator(minHeight: 2))
                : GlassButton.plain(
                    label: branch.failed ? l.commonRetry : l.dbLoadMore,
                    onPressed: () => _load(row.parent, more: branch.cursor != null, refresh: branch.failed),
                  ),
          ),
        ),
      );
    }
    final object = node.reference, expanded = _expanded.contains(object.id), selected = _selected == object.id;
    // SQLite columns read «name (TYPE)» next to their type icon.
    final sqliteColumn =
        widget.session.engine == DbEngine.sqlite &&
        object.kind == DbObjectKind.column &&
        (node.detail?.isNotEmpty ?? false);
    final draft = _draftStates[object.id] ?? widget.overlay?.stateOf(object);
    final draftColor = switch (draft) {
      DbDraftState.added => p.success,
      DbDraftState.changed => p.warning,
      null => null,
    };
    return decorated(
      GestureDetector(
        onSecondaryTapUp: (details) =>
            unawaited(_menuAt(object, Rect.fromLTWH(details.globalPosition.dx, details.globalPosition.dy, 1, 1))),
        onLongPressStart: (details) =>
            unawaited(_menuAt(object, Rect.fromLTWH(details.globalPosition.dx, details.globalPosition.dy, 1, 1))),
        // Tables and views show their first rows; other branches open.
        onDoubleTap: _previewable.contains(object.kind)
            ? () => unawaited(_action('query', object))
            : node.hasChildren
            ? () => _toggle(object)
            : () => unawaited(_action('insert', object)),
        child: GlassInteractive(
          key: ValueKey('db-object-${object.id}'),
          selected: selected,
          semanticLabel: object.name,
          onPressed: () {
            setState(() => _selected = object.id);
            widget.onSelect?.call(object);
            if (draft != null) widget.overlay?.focus(object);
            if (node.hasChildren) _toggle(object);
          },
          builder: (context, state) => DecoratedBox(
            decoration: ShapeDecoration(
              color: selected
                  ? tokens.sidebarSelection
                  : state.hovered
                  ? tokens.surfaces.fillHover
                  : Colors.transparent,
              shape: GlassRadii.shape(tokens.radii.row),
            ),
            child: Padding(
              padding: EdgeInsets.only(left: indent, right: 2),
              child: Row(
                children: [
                  if (node.hasChildren) chevron(expanded) else const SizedBox(width: 16),
                  const SizedBox(width: 4),
                  Icon(
                    _kindIcon(node, widget.session.engine),
                    size: 15,
                    color: node.primaryKey
                        ? p.warning
                        : node.foreignKey != null
                        ? p.accent
                        : expanded && _blockKinds.contains(object.kind)
                        ? p.accent
                        : tokens.secondaryLabel,
                  ),
                  const SizedBox(width: 6),
                  Flexible(
                    child: Text(
                      sqliteColumn ? '${object.name} (${node.detail})' : object.name, // l10n-ignore: name (type)
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style:
                          (selected || (expanded && _blockKinds.contains(object.kind))
                                  ? tokens.typography.bodyEmph
                                  : tokens.typography.body)
                              .copyWith(color: draftColor ?? (selected ? p.accent : p.label)),
                    ),
                  ),
                  if (draftColor != null) ...[
                    const SizedBox(width: 4),
                    Tooltip(
                      message: draft == DbDraftState.added ? l.dbDesignerDraftNew : l.dbDesignerDraftChanged,
                      child: Semantics(
                        label: l.dbDesignerUnsavedMark,
                        child: Container(
                          key: ValueKey('db-draft-${object.id}'),
                          width: 6,
                          height: 6,
                          decoration: BoxDecoration(color: draftColor, shape: BoxShape.circle),
                        ),
                      ),
                    ),
                  ],
                  if (node.detail case final detail? when detail.isNotEmpty && !sqliteColumn) ...[
                    const SizedBox(width: 6),
                    Expanded(
                      child: Text(
                        detail,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: tokens.typography.caption.copyWith(color: p.tertiary),
                      ),
                    ),
                  ] else
                    const Spacer(),
                  if (state.hovered || selected)
                    GlassMenuButton<String>(
                      key: ValueKey('db-object-menu-${object.id}'),
                      entries: _menu(l, object),
                      onSelected: (value) => unawaited(_action(value, object)),
                      builder: (context, open) => GlassIconButton(
                        icon: Icons.more_horiz_rounded,
                        tooltip: MaterialLocalizations.of(context).moreButtonTooltip,
                        size: 22,
                        iconSize: 14,
                        style: GlassIconButtonStyle.plain,
                        onPressed: open,
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    ref.watch(dbScopeProvider);
    if (!_current) return const SizedBox.shrink();
    final l = context.l10n, rows = _visible();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s8, GlassSpacing.s4, GlassSpacing.s6),
          child: Row(
            children: [
              Expanded(
                child: GlassField(
                  key: const ValueKey('db-tree-search'),
                  search: true,
                  placeholder: l.dbTreeSearch,
                  onChanged: (v) => setState(() => _filter = v.toLowerCase()),
                ),
              ),
              GlassIconButton(
                key: const ValueKey('db-tree-collapse'),
                icon: Icons.unfold_less_rounded,
                tooltip: l.dbCollapseAll,
                size: 28,
                iconSize: 16,
                style: GlassIconButtonStyle.plain,
                onPressed: () => setState(() {
                  _expanded.clear();
                  _folders.clear();
                }),
              ),
              GlassIconButton(
                key: const ValueKey('db-tree-refresh'),
                icon: Icons.refresh_rounded,
                tooltip: l.dbRefresh,
                size: 28,
                iconSize: 16,
                style: GlassIconButtonStyle.plain,
                onPressed: () {
                  _expanded.clear();
                  _branches.clear();
                  _load(null, refresh: true);
                },
              ),
            ],
          ),
        ),
        Expanded(
          child: ScrollEdgeEffect(
            bottom: true,
            extent: 12,
            child: ListView.builder(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s4, 0, GlassSpacing.s4, GlassSpacing.s8),
              itemCount: rows.length,
              itemExtent: _rowHeight,
              itemBuilder: (context, index) => _line(context, rows[index]),
            ),
          ),
        ),
      ],
    );
  }
}

/// Indent guides, and the frame of an expanded table: a tinted band from its
/// header to its last child with an accent rule along the children.
class _RowDecor extends CustomPainter {
  _RowDecor({required this.row, required this.guide, required this.block, required this.accent});
  final _Row row;
  final Color guide, block, accent;

  @override
  void paint(Canvas canvas, Size size) {
    final line = Paint()
      ..color = guide
      ..strokeWidth = 1;
    final inBlock = row.block != null;
    if (inBlock) {
      final left = _indent(row.blockDepth) - 3;
      final radius = Radius.circular(row.blockStart || row.blockEnd ? 7 : 0);
      canvas.drawRRect(
        RRect.fromLTRBAndCorners(
          left,
          0,
          size.width,
          size.height,
          topLeft: row.blockStart ? radius : Radius.zero,
          topRight: row.blockStart ? radius : Radius.zero,
          bottomLeft: row.blockEnd ? radius : Radius.zero,
          bottomRight: row.blockEnd ? radius : Radius.zero,
        ),
        Paint()..color = block,
      );
      if (!row.blockStart) {
        final x = _indent(row.blockDepth) + 8;
        canvas.drawLine(
          Offset(x, 0),
          Offset(x, row.blockEnd ? size.height - 6 : size.height),
          Paint()
            ..color = accent
            ..strokeWidth = 1.5,
        );
      }
    }
    for (var level = 0; level < row.depth; level++) {
      if (inBlock && level == row.blockDepth) continue;
      final x = _indent(level) + 8;
      canvas.drawLine(Offset(x, 0), Offset(x, size.height), line);
    }
  }

  @override
  bool shouldRepaint(_RowDecor old) =>
      old.row.depth != row.depth ||
      old.row.block != row.block ||
      old.row.blockStart != row.blockStart ||
      old.row.blockEnd != row.blockEnd ||
      old.guide != guide ||
      old.block != block ||
      old.accent != accent;
}

class DbProtectedTextDialog extends ConsumerStatefulWidget {
  const DbProtectedTextDialog({required this.title, required this.value, super.key});
  final String title;
  final SecretText value;
  @override
  ConsumerState<DbProtectedTextDialog> createState() => _DbProtectedTextDialogState();
}

class _DbProtectedTextDialogState extends ConsumerState<DbProtectedTextDialog> {
  late final _scope = ref.read(dbScopeProvider);
  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        widget.value.wipe();
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider)) || widget.value.isWiped) {
      return const SizedBox.shrink();
    }
    return GlassDialog(
      title: widget.title,
      content: SizedBox(
        width: 760,
        child: SingleChildScrollView(
          child: SecretSelectableText(
            widget.value.expose(),
            style: const TextStyle(fontFamily: 'monospace'),
            onCopy: (selected) => copySecretWithNotice(
              context,
              ref,
              selected,
              what: context.l10n.copyWhatText,
              isCurrent: () => mounted && _scope.unlocked && !widget.value.isWiped,
            ),
          ),
        ),
      ),
      secondaryActions: [GlassButton(label: context.l10n.commonClose, onPressed: () => closeDialog<void>(context))],
    );
  }
}
