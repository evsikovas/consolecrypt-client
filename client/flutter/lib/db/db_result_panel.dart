import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_ddl_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_dialogs.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_result_controller.dart';
import 'package:consolecrypt/db/db_result_edits.dart';
import 'package:consolecrypt/db/db_result_grid.dart';
import 'package:consolecrypt/db/db_result_views.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

/// How the result panel shows the rows.
enum DbResultView { table, text, record }

/// Auto-refresh intervals in seconds (null: off).
const dbAutoRefreshSeconds = <int?>[null, 5, 10, 30, 60, 300];

/// Rows loaded per "load next" when the connection has no remembered size.
const dbDefaultFetchSize = 200;

/// Per query tab: pending changes, the current row, the view and the
/// auto-refresh interval. Lives as long as the tab.
final class DbResultTabState {
  final edits = DbResultEdits();
  final cursor = ValueNotifier<int?>(null);
  DbResultView view = DbResultView.table;
  int? autoRefreshSeconds;

  void dispose() {
    edits.dispose();
    cursor.dispose();
  }
}

/// Why rows of [result] cannot be changed here, or null when they can.
String? dbEditBlockReason(
  AppLocalizations l, {
  required Object? backend,
  required DbSessionInfo session,
  required DbResultSet result,
  required bool mobile,
}) {
  if (backend is! DbCellEditService) return l.dbEditUnsupported;
  if (!const {DbEngine.postgresql, DbEngine.mysql, DbEngine.mariadb, DbEngine.cassandra}.contains(session.engine)) {
    return l.dbEditEngineUnsupported;
  }
  if (mobile) return l.dbEditDesktopOnly;
  if (result.editableTable == null || !result.columns.any((c) => c.primaryKey)) return l.dbEditNoKey;
  if (!session.writable) return session.production ? l.dbEditProductionLocked : l.dbEditReadOnly;
  return null;
}

/// Whether rows can be added and deleted, not only changed: CQL updates
/// are upserts, so Cassandra / ScyllaDB grids change existing cells only.
bool dbRowsAddable(Object? backend, DbSessionInfo session) =>
    backend is DbRowEditService && session.engine != DbEngine.cassandra;

/// A reviewed set of statements: the ticket to commit, the display SQL and
/// the Cassandra partitions it spans (each applied on its own).
typedef _Review = ({String ticket, SecretText sql, DbQueryRisk risk, DateTime expiresAt, int partitions});

Future<_Review> _review(Object backend, DbSessionInfo session, DbResultSet result, DbResultEdits edits) async {
  if (dbRowsAddable(backend, session)) {
    backend as DbRowEditService;
    final review = await backend.reviewRowChanges(session.id, result.editableTable!, edits.changes(result));
    return (ticket: review.ticket, sql: review.sql, risk: review.risk, expiresAt: review.expiresAt, partitions: 0);
  }
  // Cell edits only (no row service, or CQL): one UPDATE per changed row;
  // rows of one Cassandra partition in one conditional BATCH.
  if (backend is! DbCellEditService || edits.added.isNotEmpty || edits.deleted.isNotEmpty) {
    throw StateError('row edits unsupported');
  }
  final cells = [
    for (final MapEntry(key: (row, column), :value) in edits.cells.entries)
      DbCellEdit(
        originalPrimaryKey: {
          for (var i = 0; i < result.columns.length; i++)
            if (result.columns[i].primaryKey) result.columns[i].name: result.rows[row][i],
        },
        column: result.columns[column].name,
        value: value,
      ),
  ];
  final review = await backend.reviewCellEdits(session.id, result.editableTable!, cells);
  return (
    ticket: review.ticket,
    sql: review.sql,
    risk: DbQueryRisk.write,
    expiresAt: review.expiresAt,
    partitions: review.partitions,
  );
}

/// Loads the table's columns once (required and default columns of new rows).
Future<void> dbLoadEditColumns(Object? backend, DbSessionInfo session, DbResultSet result, DbResultEdits edits) async {
  if (edits.columns != null || backend is! DbRowEditService || result.editableTable == null) return;
  try {
    edits.columns = await backend.editColumns(session.id, result.editableTable!);
  } catch (_) {
    edits.columns = const [];
  }
}

/// Saves the pending changes of [result]: the core writes the statements,
/// the user reviews them (with the vault passphrase), then they run in one
/// transaction. Returns true when they were applied.
Future<bool> saveResultEdits(
  BuildContext context, {
  required Object? backend,
  required DbSessionInfo session,
  required DbResultSet result,
  required DbResultTabState state,
  required DbResultController controller,
  required bool Function() isCurrent,
  VoidCallback? onStale,
}) async {
  final edits = state.edits, l = context.l10n;
  if (edits.isEmpty || backend is! DbCellEditService || result.editableTable == null) return false;
  if (edits.added.isNotEmpty) await dbLoadEditColumns(backend, session, result, edits);
  if (!context.mounted || !isCurrent()) return false;
  final incomplete = edits.firstIncomplete(result);
  if (incomplete != null) {
    state.cursor.value = incomplete;
    showSnack(context, l.dbRequiredMissing(edits.missing(result, incomplete).join(', ')), tone: GlassTone.warning);
    return false;
  }
  _Review? review;
  try {
    review = await _review(backend, session, result, edits);
    if (!context.mounted || !isCurrent()) return false;
    final approved = await showAppDialog<bool>(
      context,
      secure: true,
      builder: (_) => DbQueryReviewDialog(
        query: review!.sql.expose(),
        review: DbQueryReview(ticket: review.ticket, risk: review.risk, expiresAt: review.expiresAt),
        message: review.partitions > 1 ? l.dbEditPartitions(review.partitions) : null,
      ),
    );
    if (!isCurrent() || approved != true) return false;
    if (!DateTime.now().isBefore(review.expiresAt)) {
      if (context.mounted) showSnack(context, l.dbReviewExpired, error: true);
      return false;
    }
    await backend.commitCellEdits(session.id, review.ticket);
    if (!isCurrent()) return true;
    edits.clear();
    var refreshed = false;
    try {
      refreshed = await controller.refresh();
    } catch (_) {}
    if (!refreshed) onStale?.call();
    return true;
  } catch (_) {
    if (context.mounted && isCurrent()) showSnack(context, l.dbSaveFailed, error: true);
    return false;
  } finally {
    review?.sql.wipe();
    if (review != null) {
      try {
        await backend.discardCellEdits(session.id, review.ticket);
      } catch (_) {}
    }
  }
}

enum _PendingChoice { save, discard }

/// Asks what to do with unsaved changes before they would be lost: save
/// them (true when saved), discard them (true) or stay (false).
Future<bool> resolvePendingEdits(
  BuildContext context, {
  required DbResultTabState state,
  required Future<bool> Function() save,
}) async {
  if (state.edits.isEmpty) return true;
  final l = context.l10n;
  final choice = await showAppDialog<_PendingChoice>(
    context,
    builder: (context) => GlassDialog(
      key: const ValueKey('db-pending-prompt'),
      title: l.dbUnsavedTitle,
      icon: Icons.edit_note_rounded,
      content: Text(l.dbUnsavedMessage(state.edits.count)),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('db-pending-cancel'),
          label: l.commonCancel,
          autofocus: true,
          onPressed: () => closeDialog<_PendingChoice>(context),
        ),
        GlassButton(
          key: const ValueKey('db-pending-discard'),
          label: l.dbDiscardEdits,
          onPressed: () => closeDialog(context, _PendingChoice.discard),
        ),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('db-pending-save'),
        label: l.dbSave,
        onPressed: () => closeDialog(context, _PendingChoice.save),
      ),
    ),
  );
  switch (choice) {
    case _PendingChoice.save:
      return save();
    case _PendingChoice.discard:
      state.edits.clear();
      return true;
    case null:
      return false;
  }
}

/// Keyboard shortcut text for tooltips (⌘ on macOS, Ctrl elsewhere).
String dbShortcut(String mac, String other) => AppPlatform.isMacOS ? mac : other;

/// The result area of a query tab: grid, text or record view with a view
/// switcher on the left, a bottom toolbar (refresh and auto-refresh, save /
/// cancel of pending changes, row actions, row navigation, export, fetch
/// size and paging) and a status line.
class DbResultPanel extends ConsumerStatefulWidget {
  const DbResultPanel({
    required this.controller,
    required this.state,
    required this.session,
    required this.connection,
    required this.backend,
    required this.resultIndex,
    required this.onResultIndex,
    required this.selectedRows,
    required this.onSelection,
    required this.isCurrent,
    required this.onDraft,
    required this.onStale,
    required this.onExport,
    required this.onCopyCell,
    this.preparing = false,
    this.stale = false,
    this.onImport,
    this.actions = const [],
    super.key,
  });
  final DbResultController controller;
  final DbResultTabState state;
  final DbSessionInfo session;
  final DbConnection connection;
  final Object? backend;
  final int resultIndex;
  final ValueChanged<int> onResultIndex;
  final List<int> selectedRows;
  final ValueChanged<List<int>> onSelection;
  final bool Function() isCurrent;
  final ValueChanged<String> onDraft;

  /// The data changed and the result could not be refreshed.
  final ValueChanged<bool> onStale;
  final void Function(DbResultSet result, DbResultSelection? selection) onExport;

  /// Copies the shown value of a cell (pending values included).
  final void Function(DbColumn column, DbCell cell) onCopyCell;

  /// Imports a file into the result's table (null hides the action).
  final void Function(DbResultSet result)? onImport;
  final bool preparing, stale;

  /// Extra header actions (messages).
  final List<Widget> actions;

  @override
  ConsumerState<DbResultPanel> createState() => _DbResultPanelState();
}

class _DbResultPanelState extends ConsumerState<DbResultPanel> with WidgetsBindingObserver {
  final _fetch = TextEditingController();
  Timer? _auto;
  bool _hidden = false, _saving = false;
  bool get _mobile => defaultTargetPlatform == TargetPlatform.android || defaultTargetPlatform == TargetPlatform.iOS;
  DbResultTabState get _state => widget.state;
  DbResultEdits get _edits => _state.edits;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _edits.addListener(_changed);
    _state.cursor.addListener(_changed);
    _fetch.text = '${_fetchSize()}';
    _schedule();
  }

  @override
  void didUpdateWidget(DbResultPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.state, widget.state)) {
      oldWidget.state.edits.removeListener(_changed);
      oldWidget.state.cursor.removeListener(_changed);
      _edits.addListener(_changed);
      _state.cursor.addListener(_changed);
      _schedule();
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _auto?.cancel();
    _edits.removeListener(_changed);
    _state.cursor.removeListener(_changed);
    _fetch.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _hidden = state == AppLifecycleState.hidden || state == AppLifecycleState.paused;
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  int _fetchSize() {
    final saved = ref.read(localSettingsProvider).value?.dbFetchSizes[widget.connection.id];
    return (saved ?? dbDefaultFetchSize).clamp(1, 1000);
  }

  Future<void> _setFetchSize(String text) async {
    final rows = int.tryParse(text.trim());
    if (rows == null || rows < 1 || rows > 1000) {
      _fetch.text = '${_fetchSize()}';
      showSnack(context, context.l10n.dbFetchSizeInvalid, error: true);
      return;
    }
    _fetch.text = '$rows';
    final settings = ref.read(settingsServiceProvider);
    final sizes = {...settings.currentLocal.dbFetchSizes, widget.connection.id: rows};
    if (sizes.length > 200) sizes.remove(sizes.keys.first);
    try {
      await settings.updateLocal(settings.currentLocal.copyWith(dbFetchSizes: sizes));
    } catch (_) {}
    if (mounted) setState(() {});
  }

  DbResultSet? get _result {
    final results = widget.controller.results;
    return results.where((r) => r.index == widget.resultIndex).firstOrNull ?? results.firstOrNull;
  }

  String? _blocked(DbResultSet result) => dbEditBlockReason(
    context.l10n,
    backend: widget.backend,
    session: widget.session,
    result: result,
    mobile: _mobile,
  );

  bool _rowsEditable(DbResultSet result) => _blocked(result) == null && dbRowsAddable(widget.backend, widget.session);

  // ---- auto-refresh ----------------------------------------------------

  void _schedule() {
    _auto?.cancel();
    _auto = null;
    final seconds = _state.autoRefreshSeconds;
    if (seconds == null) return;
    _auto = Timer.periodic(Duration(seconds: seconds), (_) => _tick());
  }

  void _tick() {
    final controller = widget.controller;
    // Only a read query refreshes, never over pending changes, a running
    // query or a hidden window.
    if (!mounted ||
        !widget.isCurrent() ||
        _hidden ||
        _saving ||
        widget.preparing ||
        controller.running ||
        controller.loadingMore ||
        !controller.repeatable ||
        _edits.isNotEmpty) {
      return;
    }
    unawaited(controller.refresh().catchError((Object _) => false));
  }

  void _setAuto(int? seconds) {
    setState(() => _state.autoRefreshSeconds = seconds);
    _schedule();
  }

  // ---- actions -----------------------------------------------------------

  Future<bool> _save() async {
    final result = _result;
    if (result == null || _saving) return false;
    setState(() => _saving = true);
    try {
      return await saveResultEdits(
        context,
        backend: widget.backend,
        session: widget.session,
        result: result,
        state: _state,
        controller: widget.controller,
        isCurrent: widget.isCurrent,
        onStale: () => widget.onStale(true),
      );
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _refresh() async {
    final controller = widget.controller;
    if (!controller.repeatable || controller.running || widget.preparing) return;
    if (!await resolvePendingEdits(context, state: _state, save: _save) || !mounted) return;
    try {
      final ok = await controller.refresh();
      if (!ok && mounted) showSnack(context, context.l10n.dbRefreshUnavailable, error: true);
      if (ok) widget.onStale(false);
    } catch (_) {
      if (mounted) showSnack(context, context.l10n.dbOperationFailed, error: true);
    }
  }

  Future<void> _showSql() async {
    final result = _result, backend = widget.backend;
    if (result == null || _edits.isEmpty || backend == null) return;
    _Review? review;
    try {
      review = await _review(backend, widget.session, result, _edits);
      // Only shown: the ticket is dropped at once.
      if (backend is DbCellEditService) unawaited(backend.discardCellEdits(widget.session.id, review.ticket));
      if (!mounted || !widget.isCurrent()) return;
      final value = SecretText(review.sql.expose());
      final open = await showAppDialog<bool>(
        context,
        secure: true,
        builder: (_) => DbDdlDialog(title: context.l10n.dbPendingSql, value: value, engine: widget.session.engine),
      );
      if (open == true && widget.isCurrent() && !value.isWiped) widget.onDraft(value.expose());
      value.wipe();
    } catch (_) {
      if (mounted) showSnack(context, context.l10n.dbOperationFailed, error: true);
    } finally {
      review?.sql.wipe();
    }
  }

  void _cancelEdits() => _edits.clear();

  void _editCell(DbResultSet result, int row, int column, DbCell value) {
    if (!widget.isCurrent() || _blocked(result) != null) return;
    // CQL key columns identify the row and cannot be updated.
    if (widget.session.engine == DbEngine.cassandra && result.columns[column].primaryKey) {
      showSnack(context, context.l10n.dbEditCqlKey, tone: GlassTone.warning);
      return;
    }
    _edits.setCell(result, row, column, value);
  }

  Future<void> _addRow({bool copy = false}) async {
    final result = _result, copyOf = copy ? _state.cursor.value : null;
    if (copy && copyOf == null) return;
    if (result == null || !_rowsEditable(result)) return;
    await dbLoadEditColumns(widget.backend, widget.session, result, _edits);
    if (!mounted || !widget.isCurrent() || !identical(result, _result)) return;
    final row = copyOf == null ? _edits.addRow(result) : _edits.duplicateRow(result, copyOf);
    _state.cursor.value = row;
  }

  void _deleteRow() {
    final result = _result, row = _state.cursor.value;
    if (result == null || row == null || !_rowsEditable(result)) return;
    final rows = widget.selectedRows.length > 1 ? widget.selectedRows : [row];
    final removedNew = rows.where((r) => r >= result.rows.length).toList()..sort((a, b) => b.compareTo(a));
    for (final r in [...rows.where((r) => r < result.rows.length), ...removedNew]) {
      _edits.toggleDelete(result, r);
    }
    final total = result.rows.length + _edits.added.length;
    if ((_state.cursor.value ?? 0) >= total) _state.cursor.value = total == 0 ? null : total - 1;
  }

  void _move(int Function(int current, int total) to) {
    final result = _result;
    if (result == null) return;
    final total = result.rows.length + _edits.added.length;
    if (total == 0) return;
    _state.cursor.value = to(_state.cursor.value ?? -1, total).clamp(0, total - 1);
  }

  void _setView(DbResultView view) {
    setState(() => _state.view = view);
    final result = _result;
    if (view == DbResultView.record && _state.cursor.value == null && result != null) {
      if (result.rows.isNotEmpty || _edits.added.isNotEmpty) _state.cursor.value = 0;
    }
  }

  Future<void> _switchResult(int index) async {
    if (index == widget.resultIndex) return;
    if (!await resolvePendingEdits(context, state: _state, save: _save) || !mounted) return;
    _state.cursor.value = null;
    widget.onResultIndex(index);
  }

  // ---- building ------------------------------------------------------------

  Widget _tool(
    String key,
    IconData icon,
    String tooltip,
    VoidCallback? onPressed, {
    bool selected = false,
    Color? color,
  }) => GlassIconButton(
    key: ValueKey(key),
    icon: icon,
    tooltip: tooltip,
    size: 28,
    iconSize: 16,
    selected: selected,
    color: onPressed == null ? null : color,
    style: GlassIconButtonStyle.plain,
    onPressed: onPressed,
  );

  Widget _divider() => Container(
    width: 1,
    height: 16,
    margin: const EdgeInsets.symmetric(horizontal: GlassSpacing.s4),
    color: GlassTokens.of(context).surfaces.separator,
  );

  String _autoLabel(AppLocalizations l, int? seconds) => switch (seconds) {
    null => l.dbAutoRefreshOff,
    < 60 => l.dbEverySeconds(seconds),
    _ => l.dbEveryMinutes(seconds ~/ 60),
  };

  Widget _toolbar(DbResultSet? result) {
    final l = context.l10n, tokens = GlassTokens.of(context), controller = widget.controller;
    final busy = controller.running || widget.preparing || _saving;
    final pending = _edits.isNotEmpty;
    final blocked = result == null ? l.dbNoResults : _blocked(result);
    final rowsEditable = result != null && _rowsEditable(result);
    final rowBlocked = blocked ?? (rowsEditable ? null : l.dbEditUnsupported);
    final cursor = _state.cursor.value;
    final total = result == null ? 0 : result.rows.length + _edits.added.length;
    final hasMore = result?.hasMore ?? false;
    String tip(String label, String? keys, [String? why]) =>
        why != null ? '$label — $why' : (keys == null ? label : '$label ($keys)');
    final auto = _state.autoRefreshSeconds;
    return SizedBox(
      key: const ValueKey('db-result-toolbar'),
      height: 34,
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s6),
        child: Row(
          children: [
            _tool(
              'db-result-refresh',
              Icons.refresh_rounded,
              tip(l.dbRefresh, dbShortcut('F5', 'F5'), controller.repeatable ? null : l.dbRefreshReadOnly),
              controller.repeatable && !busy ? _refresh : null,
            ),
            GlassMenuButton<int>(
              entries: [
                for (final seconds in dbAutoRefreshSeconds)
                  GlassMenuItem(
                    key: ValueKey('db-auto-refresh-${seconds ?? 0}'),
                    value: seconds ?? 0,
                    label: _autoLabel(l, seconds),
                    checked: auto == seconds,
                  ),
              ],
              onSelected: (seconds) => _setAuto(seconds == 0 ? null : seconds),
              builder: (context, open) => _tool(
                'db-result-auto-refresh',
                auto == null ? Icons.timer_outlined : Icons.timer_rounded,
                controller.repeatable || auto != null
                    ? '${l.dbAutoRefresh}: ${_autoLabel(l, auto)}' // l10n-ignore: label and value
                    : tip(l.dbAutoRefresh, null, l.dbRefreshReadOnly),
                controller.repeatable || auto != null ? open : null,
                selected: auto != null,
              ),
            ),
            _divider(),
            GlassButton(
              key: const ValueKey('db-apply-edits'),
              label: pending ? '${l.dbSave} (${_edits.count})' : l.dbSave,
              icon: Icons.save_outlined,
              size: GlassControlSize.sm,
              style: pending ? GlassButtonStyle.prominent : GlassButtonStyle.plain,
              onPressed: pending && !busy && blocked == null ? _save : null,
            ),
            GlassMenuButton<String>(
              entries: [
                GlassMenuItem(
                  value: 'save',
                  label: l.dbSave,
                  icon: Icons.save_outlined,
                  shortcut: dbShortcut('⌘S', 'Ctrl+S'),
                  enabled: pending && !busy && blocked == null,
                ),
                GlassMenuItem(
                  key: const ValueKey('db-show-sql'),
                  value: 'sql',
                  label: l.dbShowSql,
                  icon: Icons.code_rounded,
                  enabled: pending && blocked == null,
                ),
              ],
              onSelected: (value) => value == 'save' ? unawaited(_save()) : unawaited(_showSql()),
              builder: (context, open) => _tool(
                'db-result-save-menu',
                Icons.arrow_drop_down_rounded,
                l.dbSaveOptions,
                pending && blocked == null ? open : null,
              ),
            ),
            _tool(
              'db-discard-edits',
              Icons.undo_rounded,
              tip(l.dbDiscardEdits, dbShortcut('⌥⌘Z', 'Ctrl+Alt+Z')),
              pending && !busy ? _cancelEdits : null,
            ),
            _divider(),
            _tool(
              'db-row-edit',
              Icons.edit_outlined,
              tip(l.dbEditRow, dbShortcut('⇧↵', 'Shift+Enter'), blocked),
              blocked == null && cursor != null ? () => _setView(DbResultView.record) : null,
            ),
            _tool(
              'db-row-add',
              Icons.add_rounded,
              tip(l.dbAddRow, dbShortcut('⌥⌘N', 'Ctrl+Alt+N'), rowBlocked),
              rowBlocked == null && !busy ? () => unawaited(_addRow()) : null,
              color: tokens.palette.success,
            ),
            _tool(
              'db-row-duplicate',
              Icons.copy_all_rounded,
              tip(l.dbDuplicateRow, dbShortcut('⌘D', 'Ctrl+D'), rowBlocked),
              rowBlocked == null && cursor != null && !busy ? () => unawaited(_addRow(copy: true)) : null,
            ),
            _tool(
              'db-row-delete',
              Icons.remove_rounded,
              tip(l.dbDeleteRow, dbShortcut('⌘⌫', 'Ctrl+Delete'), rowBlocked),
              rowBlocked == null && cursor != null && !busy ? _deleteRow : null,
              color: tokens.palette.danger,
            ),
            _divider(),
            _tool(
              'db-row-first',
              Icons.first_page_rounded,
              tip(l.dbFirstRow, dbShortcut('⌘↑', 'Ctrl+Home')),
              total > 0 ? () => _move((_, _) => 0) : null,
            ),
            _tool(
              'db-row-previous',
              Icons.chevron_left_rounded,
              tip(l.dbPreviousRow, dbShortcut('⌥↑', 'Alt+↑')),
              total > 0 && (cursor ?? 0) > 0 ? () => _move((c, _) => c - 1) : null,
            ),
            _tool(
              'db-row-next',
              Icons.chevron_right_rounded,
              tip(l.dbNextRow, dbShortcut('⌥↓', 'Alt+↓')),
              total > 0 && (cursor ?? -1) < total - 1 ? () => _move((c, _) => c + 1) : null,
            ),
            _tool(
              'db-row-last',
              Icons.last_page_rounded,
              tip(l.dbLastRow, dbShortcut('⌘↓', 'Ctrl+End')),
              total > 0 ? () => _move((_, t) => t - 1) : null,
            ),
            _divider(),
            _tool(
              'db-export',
              Icons.file_download_outlined,
              tip(l.dbExportData, dbShortcut('⇧⌘E', 'Ctrl+Shift+E')),
              result != null ? () => _export(result) : null,
            ),
            if (widget.onImport != null)
              _tool(
                'db-import',
                Icons.file_upload_outlined,
                tip(l.dbImportData, null, rowBlocked),
                result != null && rowBlocked == null && !pending ? () => widget.onImport!(result) : null,
              ),
            GlassMenuButton<int>(
              entries: [
                for (final rows in const [50, 100, 200, 500, 1000])
                  GlassMenuItem(value: rows, label: l.dbFetchRows(rows), checked: _fetchSize() == rows),
              ],
              onSelected: (rows) => unawaited(_setFetchSize('$rows')),
              builder: (context, open) => _tool('db-result-settings', Icons.settings_outlined, l.dbFetchSize, open),
            ),
            SizedBox(
              width: 64,
              child: Tooltip(
                message: l.dbFetchSize,
                child: GlassField(
                  key: const ValueKey('db-fetch-size'),
                  controller: _fetch,
                  keyboardType: TextInputType.number,
                  inputFormatters: [FilteringTextInputFormatter.digitsOnly, LengthLimitingTextInputFormatter(4)],
                  onSubmitted: (text) => unawaited(_setFetchSize(text)),
                ),
              ),
            ),
            _tool(
              'db-result-load-next',
              Icons.playlist_add_rounded,
              l.dbLoadNext,
              hasMore && !busy && !controller.loadingMore ? () => unawaited(controller.more(rows: _fetchSize())) : null,
            ),
            _tool(
              'db-result-fetch-all',
              Icons.playlist_add_check_rounded,
              l.dbFetchAll,
              hasMore && !busy && !controller.loadingMore ? () => unawaited(controller.fetchAll()) : null,
            ),
          ],
        ),
      ),
    );
  }

  void _export(DbResultSet result) => widget.onExport(
    result,
    widget.selectedRows.isEmpty ? null : DbResultSelection(rows: List.of(widget.selectedRows)),
  );

  Widget _status(DbResultSet? result) {
    final l = context.l10n, tokens = GlassTokens.of(context), controller = widget.controller;
    final busy = widget.preparing || controller.running;
    final failed = controller.errorCode != null;
    final locale = Localizations.localeOf(context).toLanguageTag();
    final started = controller.startedAt, finished = controller.finishedAt;
    String? timing;
    if (result != null && started != null && finished != null) {
      final seconds = finished.difference(started).inMilliseconds / 1000;
      timing = l.dbFetchedTiming(
        NumberFormat('0.00', locale).format(seconds),
        DateFormat.yMd(locale).format(finished),
        DateFormat.Hms(locale).format(finished),
      );
    }
    final facts = [
      if (result != null && result.columns.isNotEmpty) l.dbRowsFetched(result.rows.length),
      ?timing,
      if (result?.hasMore ?? false) l.dbMoreAvailable,
      if (result != null && widget.selectedRows.isNotEmpty) l.dbSelectedRows(widget.selectedRows.length),
      if (result?.affectedRows != null && result!.columns.isEmpty) l.dbRowsAffected(result.affectedRows!),
      if (_edits.isNotEmpty) l.dbPendingChanges(_edits.count),
      if (_state.autoRefreshSeconds != null) _autoLabel(l, _state.autoRefreshSeconds),
    ];
    return Container(
      key: const ValueKey('db-status-bar'),
      height: 24,
      padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12),
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: tokens.surfaces.separator)),
      ),
      child: Row(
        children: [
          Container(
            width: 7,
            height: 7,
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: busy
                  ? tokens.palette.accent
                  : failed
                  ? tokens.palette.danger
                  : _edits.isNotEmpty
                  ? tokens.palette.warning
                  : tokens.palette.success,
            ),
          ),
          const SizedBox(width: GlassSpacing.s6),
          Expanded(
            child: Text(
              [
                busy
                    ? l.dbRunning
                    : failed
                    ? l.dbOperationFailed
                    : controller.cancelled
                    ? l.dbCancelled
                    : l.dbStatusReady,
                ...facts,
              ].join('  ·  '), // l10n-ignore: Visual separator between status facts.
              key: const ValueKey('db-status-text'),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: tokens.typography.caption.copyWith(
                color: tokens.secondaryLabel,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
          ),
          if (result?.truncated ?? false)
            Tooltip(
              message: l.dbTruncated,
              child: Icon(Icons.warning_amber_rounded, size: 14, color: tokens.palette.warning),
            ),
        ],
      ),
    );
  }

  Widget _switcher() {
    final l = context.l10n;
    Widget view(DbResultView value, IconData icon, String label, String keys) => Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: _tool(
        'db-view-${value.name}',
        icon,
        '$label ($keys)', // l10n-ignore: label and shortcut
        () => _setView(value),
        selected: _state.view == value,
      ),
    );
    return Container(
      key: const ValueKey('db-result-views'),
      width: 34,
      padding: const EdgeInsets.only(top: GlassSpacing.s6),
      decoration: BoxDecoration(
        border: Border(right: BorderSide(color: GlassTokens.of(context).surfaces.separator)),
      ),
      child: Column(
        children: [
          view(DbResultView.table, Icons.grid_on_rounded, l.dbViewTable, dbShortcut('⌥⌘1', 'Ctrl+Alt+1')),
          view(DbResultView.text, Icons.notes_rounded, l.dbViewText, dbShortcut('⌥⌘2', 'Ctrl+Alt+2')),
          view(DbResultView.record, Icons.view_agenda_outlined, l.dbViewRecord, dbShortcut('⌥⌘3', 'Ctrl+Alt+3')),
        ],
      ),
    );
  }

  Map<ShortcutActivator, VoidCallback> _shortcuts(DbResultSet? result) {
    final mac = AppPlatform.isMacOS;
    SingleActivator command(LogicalKeyboardKey key, {bool shift = false, bool alt = false}) =>
        SingleActivator(key, meta: mac, control: !mac, shift: shift, alt: alt);
    return {
      const SingleActivator(LogicalKeyboardKey.f5): () => unawaited(_refresh()),
      command(LogicalKeyboardKey.keyR): () => unawaited(_refresh()),
      command(LogicalKeyboardKey.keyS): () => unawaited(_save()),
      command(LogicalKeyboardKey.keyZ, alt: true): _cancelEdits,
      command(LogicalKeyboardKey.keyN, alt: true): () => unawaited(_addRow()),
      command(LogicalKeyboardKey.keyD): () => unawaited(_addRow(copy: true)),
      command(mac ? LogicalKeyboardKey.backspace : LogicalKeyboardKey.delete): _deleteRow,
      const SingleActivator(LogicalKeyboardKey.enter, shift: true): () {
        if (result != null && _blocked(result) == null && _state.cursor.value != null) _setView(DbResultView.record);
      },
      command(mac ? LogicalKeyboardKey.arrowUp : LogicalKeyboardKey.home): () => _move((_, _) => 0),
      command(mac ? LogicalKeyboardKey.arrowDown : LogicalKeyboardKey.end): () => _move((_, t) => t - 1),
      const SingleActivator(LogicalKeyboardKey.arrowUp, alt: true): () => _move((c, _) => c - 1),
      const SingleActivator(LogicalKeyboardKey.arrowDown, alt: true): () => _move((c, _) => c + 1),
      command(LogicalKeyboardKey.keyE, shift: true): () {
        if (result != null) _export(result);
      },
      command(LogicalKeyboardKey.digit1, alt: true): () => _setView(DbResultView.table),
      command(LogicalKeyboardKey.digit2, alt: true): () => _setView(DbResultView.text),
      command(LogicalKeyboardKey.digit3, alt: true): () => _setView(DbResultView.record),
    };
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context), controller = widget.controller;
    final results = controller.results, result = _result;
    final editable = result != null && _blocked(result) == null;
    final title = [
      Icon(Icons.table_rows_outlined, size: 15, color: tokens.secondaryLabel),
      const SizedBox(width: GlassSpacing.s6),
      Text(
        l.dbResults,
        style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel, fontWeight: FontWeight.w600),
      ),
      if (results.length > 1) ...[
        const SizedBox(width: GlassSpacing.s8),
        GlassSegmented<int>(
          key: const ValueKey('db-result-sets'),
          inChrome: false,
          segments: [for (final item in results) GlassSegment(value: item.index, label: '${item.index + 1}')],
          selected: result?.index ?? 0,
          onChanged: (index) => unawaited(_switchResult(index)),
        ),
      ],
      const SizedBox(width: GlassSpacing.s8),
    ];
    Widget body;
    if (result == null) {
      body = Center(
        child: Text(l.dbNoResults, style: tokens.typography.caption.copyWith(color: tokens.palette.tertiary)),
      );
    } else {
      body = switch (_state.view) {
        DbResultView.table => DbResultGrid(
          key: ValueKey('grid:${result.index}'),
          result: result,
          leading: title,
          trailing: widget.actions,
          edits: _edits,
          cursor: _state.cursor,
          onCopy: widget.backend is DbExportService ? (selection) => widget.onExport(result, selection) : null,
          onCopyCell: (row, column) =>
              widget.onCopyCell(result.columns[column], _edits.value(result, row, column) ?? const DbCell.nullValue()),
          onCellEdit: editable ? (row, column, value) => _editCell(result, row, column, value) : null,
          onSelection: widget.onSelection,
          loadingMore: controller.loadingMore,
        ),
        DbResultView.text => DbResultTextView(
          key: ValueKey('text:${result.index}'),
          result: result,
          edits: _edits,
          onCopy: (text) => unawaited(copyPlainWithNotice(context, ref, text, what: l.copyWhatValue)),
        ),
        DbResultView.record => DbRecordView(
          key: ValueKey('record:${result.index}'),
          result: result,
          edits: _edits,
          row: switch (_state.cursor.value) {
            final int row when row < result.rows.length + _edits.added.length => row,
            _ => result.rows.isEmpty && _edits.added.isEmpty ? null : 0,
          },
          onCellEdit: editable ? (row, column, value) => _editCell(result, row, column, value) : null,
          onResetCell: editable ? (row, column) => _edits.resetCell(result, row, column) : null,
        ),
      };
      if (_state.view != DbResultView.table) {
        body = Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SizedBox(
              height: 38,
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s6),
                child: Row(children: [...title, const Spacer(), ...widget.actions]),
              ),
            ),
            Container(height: 1, color: tokens.surfaces.separator),
            Expanded(child: body),
          ],
        );
      }
    }
    return CallbackShortcuts(
      bindings: _shortcuts(result),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (result == null) ...[
            SizedBox(
              height: 36,
              child: Padding(
                padding: const EdgeInsets.only(left: GlassSpacing.s12, right: GlassSpacing.s4),
                child: Row(children: [...title, const Spacer(), ...widget.actions]),
              ),
            ),
            Container(height: 1, color: tokens.surfaces.separator),
          ],
          SizedBox(height: 2, child: widget.preparing || controller.running ? const LinearProgressIndicator() : null),
          if (controller.errorCode != null || widget.stale)
            Padding(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s6, GlassSpacing.s8, 0),
              child: InfoBanner(
                tone: controller.errorCode != null ? BannerTone.danger : BannerTone.warning,
                message: controller.errorCode != null ? l.dbOperationFailed : l.dbResultStale,
              ),
            ),
          Expanded(
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (result != null) _switcher(),
                Expanded(child: body),
              ],
            ),
          ),
          Container(height: 1, color: tokens.surfaces.separator),
          _toolbar(result),
          _status(result),
        ],
      ),
    );
  }
}
