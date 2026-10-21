import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/db/db_ai_panel.dart';
import 'package:consolecrypt/db/db_ai_service.dart';
import 'package:consolecrypt/db/db_editor.dart';
import 'package:consolecrypt/db/db_errors.dart';
import 'package:consolecrypt/db/db_export_dialog.dart';
import 'package:consolecrypt/db/db_history_panel.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_plan_view.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_dialogs.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_result_controller.dart';
import 'package:consolecrypt/db/db_result_grid.dart';
import 'package:consolecrypt/db/db_tree.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final class _QueryTab {
  _QueryTab({required this.id, required this.controller, required this.result});
  final int id;
  final DbEditorController controller;
  final DbResultController result;
  final edits = <DbCellEdit>[];
  int resultIndex = 0;
  List<int> selectedRows = [];
  bool preparing = false, stale = false;
  void dispose() {
    controller.clear();
    controller.dispose();
    result.dispose();
    edits.clear();
    selectedRows.clear();
  }
}

/// An explicitly opened connection workspace. All query/result ownership is
/// bounded by this widget's vault scope and opaque native session.
class DbWorkspace extends ConsumerStatefulWidget {
  const DbWorkspace({required this.connection, required this.onClose, this.onSessionChanged, super.key});
  final DbConnection connection;
  final VoidCallback onClose;
  final ValueChanged<DbSessionInfo?>? onSessionChanged;
  @override
  ConsumerState<DbWorkspace> createState() => _DbWorkspaceState();
}

class _DbWorkspaceState extends ConsumerState<DbWorkspace> with WidgetsBindingObserver {
  late final _scope = ref.read(dbScopeProvider);
  late final Object? _backend = ref.read(dbServiceProvider);
  DbQueryService? get _service => _backend is DbQueryService ? _backend : null;
  DbSessionInfo? _session;
  final _tabs = <_QueryTab>[];
  int _active = 0, _nextId = 1, _generation = 0;
  bool _connecting = false, _failed = false;
  Object? _connectError;
  String _view = 'query';
  bool _aiOpen = false;
  double _editorFraction = .4;
  DbObjectRef? _selectedObject;
  DbJson? _lastPlan;
  Timer? _background;
  final _password = TextEditingController();

  /// Connections saved without a password ask for it on every open; the
  /// typed value is used once and never stored.
  bool get _asksPassword =>
      widget.connection.auth.passwordSecretId == null &&
      widget.connection.auth.keySecretId == null &&
      widget.connection.username.isNotEmpty;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));
  bool get _mobile => defaultTargetPlatform == TargetPlatform.android || defaultTargetPlatform == TargetPlatform.iOS;
  bool _tabCurrent(_QueryTab tab) => _current && _tabs.contains(tab) && _session?.id == tab.result.sessionId;
  _QueryTab? get _tab => _tabs.isEmpty ? null : _tabs[_active.clamp(0, _tabs.length - 1)];
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    if (!_asksPassword) unawaited(_connect());
  }

  Future<void> _connect() async {
    final service = _service;
    if (!_current || service == null || _connecting || _session != null) return;
    final generation = ++_generation;
    setState(() {
      _connecting = true;
      _failed = false;
    });
    try {
      final password = _asksPassword ? SecretText(_password.text) : null;
      _password.clear();
      final session = await service.connect(widget.connection.id, temporaryPassword: password);
      if (!_current || generation != _generation) {
        await service.disconnect(session.id);
        return;
      }
      setState(() {
        _session = session;
        _addTab();
      });
      widget.onSessionChanged?.call(session);
    } catch (error) {
      if (_current && generation == _generation) {
        setState(() {
          _failed = true;
          _connectError = error;
        });
      }
    } finally {
      if (_current && generation == _generation) setState(() => _connecting = false);
    }
  }

  void _addTab([String text = '']) {
    final session = _session, service = _service;
    if (!_current || session == null || service == null || _tabs.length >= 12) return;
    late _QueryTab tab;
    tab = _QueryTab(
      id: _nextId++,
      controller: DbEditorController(text: text),
      result: DbResultController(
        service: service,
        sessionId: session.id,
        memoryLimit: widget.connection.limits.resultMemoryBytes,
        isCurrent: () => _tabCurrent(tab),
      ),
    );
    tab.result.addListener(_changed);
    _tabs.add(tab);
    _active = _tabs.length - 1;
  }

  void _changed() {
    if (_current) setState(() {});
  }

  void _draft(String text) {
    if (!_current) return;
    setState(() {
      _addTab(text);
      _view = 'query';
    });
  }

  Future<void> _closeTab(_QueryTab tab) async {
    if (tab.result.running || tab.result.results.any((r) => r.hasMore)) {
      final l = context.l10n;
      final approved = await showConfirmDialog(
        context,
        title: l.commonClose,
        message: l.dbCloseRunning,
        confirmLabel: l.commonClose,
        secure: true,
      );
      if (!_tabCurrent(tab) || !approved) return;
    }
    setState(() {
      _tabs.remove(tab);
      tab.dispose();
      if (_tabs.isEmpty) _addTab();
      _active = _active.clamp(0, _tabs.length - 1);
    });
  }

  void _closeSession() {
    _generation++;
    _background?.cancel();
    final session = _session;
    _session = null;
    for (final tab in _tabs) {
      tab.dispose();
    }
    _tabs.clear();
    _lastPlan = null;
    _selectedObject = null;
    if (session != null && _service != null) unawaited(_service!.disconnect(session.id).catchError((Object _) {}));
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      _background?.cancel();
      _background = null;
      return;
    }
    if (defaultTargetPlatform == TargetPlatform.iOS &&
        (state == AppLifecycleState.paused || state == AppLifecycleState.hidden)) {
      _background ??= Timer(const Duration(seconds: 30), () {
        for (final tab in _tabs) {
          if (tab.result.running || tab.result.loadingMore) unawaited(tab.result.cancel());
        }
      });
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _closeSession();
    _password
      ..clear()
      ..dispose();
    super.dispose();
  }

  void _error() {
    if (mounted && _current) showSnack(context, context.l10n.dbOperationFailed, error: true);
  }

  Future<DbQueryRequest?> _prepare(_QueryTab tab, DbRunMode mode) async {
    final selection = tab.controller.selection;
    final prepared = await _service!.prepareQuery(
      _session!.id,
      tab.controller.text,
      mode: mode,
      selectionStart: selection.isValid ? selection.start : 0,
      selectionEnd: selection.isValid ? selection.end : 0,
    );
    if (!_tabCurrent(tab) || prepared.text.trim().isEmpty) return null;
    List<DbParameter> parameters = [];
    if (prepared.parameterNames.isNotEmpty) {
      if (prepared.parameterNames.length > 100 || !mounted) return null;
      final values = await showAppDialog<List<DbParameter>>(
        context,
        secure: true,
        builder: (_) => DbVariablesDialog(names: prepared.parameterNames),
      );
      if (!_tabCurrent(tab) || values == null) return null;
      parameters = values;
    }
    return DbQueryRequest(text: prepared.text, parameters: parameters);
  }

  Future<void> _discardReview(String session, String ticket) async {
    final service = _backend;
    if (service is DbQueryReviewService) {
      try {
        await service.discardQueryReview(session, ticket);
      } catch (_) {}
    }
  }

  Future<void> _run(DbRunMode mode) async {
    final tab = _tab, session = _session;
    if (tab == null || session == null || tab.preparing || tab.result.running || !_current) return;
    setState(() => tab.preparing = true);
    String? reviewTicket;
    try {
      final request = await _prepare(tab, mode);
      if (!_tabCurrent(tab) || request == null) return;
      final review = await _service!.reviewQuery(session.id, request);
      reviewTicket = review.ticket;
      if (!_tabCurrent(tab)) return;
      if (review.risk == DbQueryRisk.blocked ||
          ((session.production || session.readOnly) && review.risk != DbQueryRisk.read)) {
        final suggestion = review.suggestion;
        if (suggestion != null && mounted) {
          final l = context.l10n;
          final open = await showConfirmDialog(
            context,
            title: l.dbQueryBlockedTitle,
            message: l.dbScanInsteadOfKeys,
            confirmLabel: l.dbOpenSuggestion,
            secure: true,
            extra: SelectableText(suggestion, style: const TextStyle(fontFamily: 'monospace')),
          );
          if (open && _tabCurrent(tab)) setState(() => _addTab(suggestion));
        } else if (mounted) {
          showSnack(context, context.l10n.dbQueryBlocked, error: true);
        }
        return;
      }
      if (review.risk != DbQueryRisk.read) {
        if (!mounted) return;
        final approved = await showAppDialog<bool>(
          context,
          secure: true,
          builder: (_) =>
              DbQueryReviewDialog(connectionName: widget.connection.name, query: request.text, review: review),
        );
        if (!_tabCurrent(tab) || approved != true) return;
      }
      if (!DateTime.now().isBefore(review.expiresAt)) {
        if (mounted) showSnack(context, context.l10n.dbReviewExpired, error: true);
        return;
      }
      tab.edits.clear();
      tab.selectedRows.clear();
      tab.resultIndex = 0;
      tab.stale = false;
      await tab.result.execute(
        DbQueryRequest(text: request.text, parameters: request.parameters, approvalTicket: review.ticket),
      );
    } catch (_) {
      if (_tabCurrent(tab)) _error();
    } finally {
      if (reviewTicket != null) unawaited(_discardReview(session.id, reviewTicket));
      if (_tabCurrent(tab)) setState(() => tab.preparing = false);
    }
  }

  Future<void> _privileges() async {
    final session = _session;
    if (session == null || !_current) return;
    try {
      final report = await _service!.checkPrivileges(session.id);
      if (!mounted || !_current || _session?.id != session.id) return;
      final l = context.l10n;
      await showAppDialog<void>(
        context,
        secure: true,
        builder: (context) => GlassDialog(
          title: l.dbCheckPrivileges,
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                report.serverReadOnly == DbServerReadOnly.unverified ? l.dbUnverifiedReadOnly : l.dbVerifiedReadOnly,
              ),
              if (report.hasWritePrivileges == true) Text(l.dbRoleCanWrite),
            ],
          ),
          secondaryActions: [GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context))],
        ),
      );
    } catch (_) {
      _error();
    }
  }

  Future<void> _writeMode() async {
    final session = _session;
    if (!_current || session == null || session.production) return;
    try {
      if (session.readOnly) {
        final l = context.l10n;
        final yes = await showConfirmDialog(
          context,
          title: l.dbAllowWrites,
          message: l.dbAllowWritesHelp,
          confirmLabel: l.dbAllowWrites,
          secure: true,
          destructive: true,
        );
        if (!_current || _session?.id != session.id || !yes) return;
      }
      final updated = session.readOnly
          ? await _service!.allowSessionWrite(session.id)
          : await _service!.revokeSessionWrite(session.id);
      if (_current && _session?.id == session.id && updated.id == session.id) setState(() => _session = updated);
    } catch (_) {
      _error();
    }
  }

  Future<void> _explain(bool analyze) async {
    final tab = _tab, session = _session;
    if (tab == null || session == null || tab.preparing || (analyze && session.production)) return;
    setState(() => tab.preparing = true);
    try {
      if (analyze) {
        final l = context.l10n;
        final yes = await showConfirmDialog(
          context,
          title: l.dbExplainAnalyze,
          message: l.dbExplainAnalyzeHelp,
          confirmLabel: l.dbRun,
          secure: true,
        );
        if (!_tabCurrent(tab) || !yes) return;
      }
      final request = await _prepare(tab, DbRunMode.current);
      if (request == null || !_tabCurrent(tab)) return;
      final plan = await _service!.explain(session.id, request, analyze: analyze);
      if (!mounted || !_tabCurrent(tab)) return;
      _lastPlan = plan.document;
      await showAppDialog<void>(context, secure: true, builder: (_) => _PlanDialog(plan: plan));
    } catch (_) {
      if (_tabCurrent(tab)) _error();
    } finally {
      if (_tabCurrent(tab)) setState(() => tab.preparing = false);
    }
  }

  Future<void> _export(_QueryTab tab, DbResultSet result, [DbResultSelection? selection]) async {
    final service = _backend, session = _session, query = tab.result.queryId;
    if (!_tabCurrent(tab) || session == null || query == null || service is! DbExportService) return;
    await showAppDialog<void>(
      context,
      secure: true,
      builder: (_) => DbExportDialog(
        service: service,
        session: session,
        queryId: query,
        resultIndex: result.index,
        selection: selection,
      ),
    );
  }

  Future<void> _editCell(_QueryTab tab, DbResultSet result, int row, int column) async {
    final session = _session;
    if (!_tabCurrent(tab) ||
        session == null ||
        session.production ||
        session.readOnly ||
        _mobile ||
        result.editableTable == null ||
        !result.columns.any((c) => c.primaryKey)) {
      return;
    }
    final values = await showAppDialog<List<DbParameter>>(
      context,
      secure: true,
      builder: (_) => DbVariablesDialog(names: [result.columns[column].name]),
    );
    if (!_tabCurrent(tab) || values == null || values.length != 1) return;
    setState(
      () => tab.edits.add(
        DbCellEdit(
          originalPrimaryKey: {
            for (var i = 0; i < result.columns.length; i++)
              if (result.columns[i].primaryKey) result.columns[i].name: result.rows[row][i],
          },
          column: result.columns[column].name,
          value: values.single.value,
        ),
      ),
    );
  }

  Future<void> _applyEdits(_QueryTab tab, DbResultSet result) async {
    final service = _backend, session = _session;
    if (!_tabCurrent(tab) ||
        session == null ||
        session.production ||
        session.readOnly ||
        _mobile ||
        service is! DbCellEditService ||
        result.editableTable == null ||
        tab.edits.isEmpty) {
      return;
    }
    DbCellEditReview? review;
    try {
      review = await service.reviewCellEdits(session.id, result.editableTable!, List.of(tab.edits));
      if (!mounted || !_tabCurrent(tab)) return;
      final approved = await showAppDialog<bool>(
        context,
        secure: true,
        builder: (_) => DbQueryReviewDialog(
          connectionName: widget.connection.name,
          query: review!.sql.expose(),
          review: DbQueryReview(ticket: review.ticket, risk: DbQueryRisk.write, expiresAt: review.expiresAt),
        ),
      );
      if (!_tabCurrent(tab) || approved != true || !DateTime.now().isBefore(review.expiresAt)) return;
      await service.commitCellEdits(session.id, review.ticket);
      if (_tabCurrent(tab)) {
        setState(() {
          tab.edits.clear();
          tab.stale = true;
        });
      }
    } catch (_) {
      if (_tabCurrent(tab)) _error();
    } finally {
      review?.sql.wipe();
      if (review != null) {
        try {
          await service.discardCellEdits(session.id, review.ticket);
        } catch (_) {}
      }
    }
  }

  Widget _results(_QueryTab tab, DbSessionInfo session) {
    final l = context.l10n, results = tab.result.results;
    final result = results.where((r) => r.index == tab.resultIndex).firstOrNull ?? results.firstOrNull;
    return Column(
      children: [
        SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: Row(
            children: [
              for (final item in results)
                Padding(
                  padding: const EdgeInsets.only(right: 4),
                  child: ChoiceChip(
                    label: Text(
                      '${l.dbResults} ${item.index + 1} · ${item.rows.length} · ${l.dbElapsed(item.elapsedMs)}',
                    ),
                    selected: result == item,
                    onSelected: (_) => setState(() {
                      tab.resultIndex = item.index;
                      tab.selectedRows.clear();
                      tab.edits.clear();
                    }),
                  ),
                ),
              if (result != null && _backend is DbExportService)
                IconButton(
                  tooltip: l.dbExport,
                  icon: const Icon(Icons.download),
                  onPressed: () => _export(tab, result),
                ),
              if (tab.result.notices.isNotEmpty)
                IconButton(
                  tooltip: l.dbMessages,
                  icon: const Icon(Icons.message_outlined),
                  onPressed: () async {
                    final value = SecretText(tab.result.notices.join('\n'));
                    try {
                      await showAppDialog<void>(
                        context,
                        secure: true,
                        builder: (_) => DbProtectedTextDialog(title: l.dbMessages, value: value),
                      );
                    } finally {
                      value.wipe();
                    }
                  },
                ),
            ],
          ),
        ),
        if (tab.preparing || tab.result.running) const LinearProgressIndicator(),
        if (tab.result.errorCode != null) Text(l.dbOperationFailed),
        if (tab.result.cancelled) Text(l.dbCancelled),
        if (tab.stale) Text(l.dbResultStale),
        if (tab.edits.isNotEmpty)
          Wrap(
            spacing: 8,
            children: [
              GlassButton(
                label: '${l.dbApplyEdits} (${tab.edits.length})',
                onPressed: result == null ? null : () => _applyEdits(tab, result),
              ),
              GlassButton(label: l.dbDiscardEdits, onPressed: () => setState(tab.edits.clear)),
            ],
          ),
        Expanded(
          child: result == null
              ? Center(child: Text(l.dbNoResults))
              : DbResultGrid(
                  key: ValueKey('${tab.id}:${result.index}'),
                  result: result,
                  production: session.production,
                  onCopy: _backend is DbExportService ? (selection) => _export(tab, result, selection) : null,
                  onEdit:
                      !_mobile &&
                          !session.readOnly &&
                          !session.production &&
                          result.editableTable != null &&
                          result.columns.any((c) => c.primaryKey) &&
                          _backend is DbCellEditService
                      ? (row, col) => _editCell(tab, result, row, col)
                      : null,
                  onSelection: (rows) => setState(() => tab.selectedRows = rows),
                  onMore: result.hasMore ? () => tab.result.more() : null,
                  loadingMore: tab.result.loadingMore,
                ),
        ),
      ],
    );
  }

  Widget _queryPane(_QueryTab tab, DbSessionInfo session) => Column(
    children: [
      SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            for (var i = 0; i < _tabs.length; i++)
              InputChip(
                label: Text('${context.l10n.dbEditor} ${_tabs[i].id}'),
                selected: _active == i,
                onPressed: () => setState(() => _active = i),
                onDeleted: () => _closeTab(_tabs[i]),
              ),
            IconButton(
              tooltip: context.l10n.dbNewQuery,
              icon: const Icon(Icons.add),
              onPressed: _tabs.length < 12 ? () => setState(() => _addTab()) : null,
            ),
          ],
        ),
      ),
      Expanded(
        child: LayoutBuilder(
          builder: (context, bounds) {
            final height = bounds.maxHeight * _editorFraction;
            return Column(
              children: [
                SizedBox(
                  height: height,
                  child: DbQueryEditor(
                    key: ValueKey(tab.id),
                    controller: tab.controller,
                    session: session,
                    service: _service!,
                    isCurrent: () => _tabCurrent(tab),
                    running: tab.preparing || tab.result.running,
                    onRun: _run,
                    onCancel: tab.result.cancel,
                    onExplain: _explain,
                    onHistory: _backend is DbHistoryService ? () => setState(() => _view = 'history') : null,
                    onAi: _backend is DbAiService
                        ? () => setState(() {
                            _aiOpen = true;
                            if (MediaQuery.sizeOf(context).width < 1100) _view = 'ai';
                          })
                        : null,
                  ),
                ),
                GestureDetector(
                  behavior: HitTestBehavior.opaque,
                  onVerticalDragUpdate: (d) => setState(
                    () => _editorFraction = (_editorFraction + d.delta.dy / bounds.maxHeight).clamp(.2, .75),
                  ),
                  child: const SizedBox(
                    height: 12,
                    child: Center(child: SizedBox(width: 48, child: Divider(thickness: 3))),
                  ),
                ),
                Expanded(child: _results(tab, session)),
              ],
            );
          },
        ),
      ),
    ],
  );
  Widget _ai(DbSessionInfo session) => DbAiPanel(
    key: ValueKey('ai-${session.id}'),
    service: _backend! as DbAiService,
    session: session,
    query: () => _tab?.controller.text ?? '',
    onDraft: _draft,
    object: _selectedObject,
    errorCode: _tab?.result.errorCode,
    queryId: _tab?.result.queryId,
    resultIndex: _tab?.resultIndex,
    selectedRows: _tab?.selectedRows ?? const [],
    plan: _lastPlan,
  );
  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) _closeSession();
    });
    ref.watch(dbScopeProvider);
    final l = context.l10n, session = _session, tab = _tab;
    if (!_current) return Center(child: Text(l.dbLocked));
    if (_service == null) return Center(child: Text(l.dbUnavailable));
    if (session == null || tab == null) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(widget.connection.name),
            if (_connecting) const CircularProgressIndicator(),
            if (_failed)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 8),
                child: Text(
                  _connectError == null ? l.dbOperationFailed : dbFailureText(_connectError!, l),
                  textAlign: TextAlign.center,
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ),
            if (_asksPassword && !_connecting)
              SizedBox(
                width: 320,
                child: SecretField(
                  key: const ValueKey('db-connect-password'),
                  controller: _password,
                  label: l.dbPassword,
                  autofocus: true,
                  onSubmitted: (_) => _connect(),
                ),
              ),
            if (!_connecting)
              GlassButton(label: _asksPassword || !_failed ? l.commonConnect : l.commonRetry, onPressed: _connect),
            GlassButton(label: l.commonClose, onPressed: widget.onClose),
          ],
        ),
      );
    }
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(8),
          child: Wrap(
            spacing: 8,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Text(widget.connection.name, style: Theme.of(context).textTheme.titleMedium),
              Text('${session.engine.label} ${session.serverVersion}'),
              if (session.production) Chip(label: Text(l.dbProduction)),
              Chip(label: Text(session.readOnly ? l.dbReadOnly : l.dbWriteAllowed)),
              IconButton(
                tooltip: l.dbCheckPrivileges,
                icon: const Icon(Icons.verified_user_outlined),
                onPressed: _privileges,
              ),
              if (!session.production)
                IconButton(
                  tooltip: session.readOnly ? l.dbAllowWrites : l.dbReadOnly,
                  icon: Icon(session.readOnly ? Icons.lock : Icons.lock_open),
                  onPressed: _writeMode,
                ),
              IconButton(tooltip: l.commonClose, icon: const Icon(Icons.close), onPressed: widget.onClose),
            ],
          ),
        ),
        if (session.production && session.serverReadOnly == DbServerReadOnly.unverified) Text(l.dbUnverifiedReadOnly),
        Expanded(
          child: LayoutBuilder(
            builder: (context, bounds) {
              final wide = bounds.maxWidth >= 850, full = bounds.maxWidth >= 1100;
              final tree = DbObjectTree(
                key: ValueKey(session.id),
                session: session,
                service: _service!,
                onDraft: _draft,
                onInsert: (value) {
                  if (_current) _tab?.controller.insert(value);
                },
                onSelect: (object) => setState(() => _selectedObject = object),
              );
              final navigation = SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  children: [
                    if (!wide)
                      ChoiceChip(
                        label: Text(l.dbTree),
                        selected: _view == 'tree',
                        onSelected: (_) => setState(() => _view = 'tree'),
                      ),
                    ChoiceChip(
                      label: Text(l.dbEditor),
                      selected: _view == 'query',
                      onSelected: (_) => setState(() => _view = 'query'),
                    ),
                    if (_backend is DbHistoryService)
                      ChoiceChip(
                        label: Text(l.dbHistory),
                        selected: _view == 'history',
                        onSelected: (_) => setState(() => _view = 'history'),
                      ),
                    if (_backend is DbAiService)
                      ChoiceChip(
                        label: Text(l.dbAi),
                        selected: full ? _aiOpen : _view == 'ai',
                        onSelected: (v) => setState(() {
                          if (full) {
                            _aiOpen = v;
                          } else {
                            _view = 'ai';
                          }
                        }),
                      ),
                  ],
                ),
              );
              final Widget content = switch (_view) {
                'tree' when !wide => tree,
                'history' when _backend is DbHistoryService => DbHistoryPanel(
                  connection: widget.connection,
                  service: _backend,
                  onDraft: _draft,
                ),
                'ai' when !full && _backend is DbAiService => _ai(session),
                _ => _queryPane(tab, session),
              };
              return Row(
                children: [
                  if (wide) SizedBox(width: 240, child: tree),
                  Expanded(
                    child: Column(
                      children: [
                        navigation,
                        Expanded(child: content),
                      ],
                    ),
                  ),
                  if (full && _aiOpen && _backend is DbAiService) SizedBox(width: 330, child: _ai(session)),
                ],
              );
            },
          ),
        ),
      ],
    );
  }
}

class _PlanDialog extends ConsumerStatefulWidget {
  const _PlanDialog({required this.plan});
  final DbExplainPlan plan;
  @override
  ConsumerState<_PlanDialog> createState() => _PlanDialogState();
}

class _PlanDialogState extends ConsumerState<_PlanDialog> {
  late final _scope = ref.read(dbScopeProvider);
  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider))) return const SizedBox.shrink();
    return GlassDialog(
      title: '${context.l10n.dbExplain} · ${widget.plan.engine.label}',
      content: SizedBox(
        width: 900,
        height: MediaQuery.sizeOf(context).height * .65,
        child: DbPlanView(document: widget.plan.document),
      ),
      secondaryActions: [GlassButton(label: context.l10n.commonClose, onPressed: () => closeDialog<void>(context))],
    );
  }
}
