import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
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
import 'package:consolecrypt/db/db_prod_unlock_dialog.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_dialogs.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_result_controller.dart';
import 'package:consolecrypt/db/db_result_grid.dart';
import 'package:consolecrypt/db/db_snippet_picker.dart';
import 'package:consolecrypt/db/db_transport_badge.dart';
import 'package:consolecrypt/db/db_tree.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

final class _QueryTab {
  _QueryTab({required this.id, required this.controller, required this.result});
  final int id;
  final DbEditorController controller;
  final DbResultController result;

  /// Values typed into cells of the shown result, by (row, column), until
  /// they are applied (reviewed as UPDATE statements) or discarded.
  final pending = <(int, int), DbCell>{};
  int resultIndex = 0;
  List<int> selectedRows = [];
  bool preparing = false, stale = false;

  /// The data preview tab: the object whose first rows it shows and the
  /// query it holds. The next preview replaces both; editing the text makes
  /// it an ordinary tab, so typed text is never overwritten.
  String? previewName, previewText;
  bool get preview => previewName != null;
  void dispose() {
    previewName = previewText = null;
    controller.clear();
    controller.dispose();
    result.dispose();
    pending.clear();
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

  /// The editor tab used last; generated text goes there while the data
  /// preview tab is shown.
  _QueryTab? _lastEditor;
  bool _connecting = false, _failed = false;
  Object? _connectError;
  String _view = 'query';
  String? _side;
  bool _navOpen = true;
  double _navWidth = 280;
  double _editorFraction = .4;
  DbObjectRef? _selectedObject;
  DbJson? _lastPlan;
  Timer? _background;

  /// Ticks the production write countdown; asks the core again at the end.
  Timer? _prodTimer;

  /// The privilege check after unlocking production: the role cannot write.
  bool _roleCannotWrite = false;
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
      _watchProductionWrite();
      ref.read(dbLastSecurityProvider.notifier).record(widget.connection.id, session.transportSecurity);
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
    tab.controller.addListener(() {
      if (mounted && tab.preview && tab.controller.text != tab.previewText) {
        tab
          ..previewName = null
          ..previewText = null;
        _lastEditor = tab;
        _changed();
      }
    });
    _tabs.add(tab);
    _select(_tabs.length - 1);
  }

  void _select(int index) {
    _active = index;
    final tab = _tabs[index];
    if (!tab.preview) _lastEditor = tab;
  }

  void _changed() {
    if (_current) setState(() {});
  }

  /// Generated text (DDL, history, AI, suggestions) goes below the text of
  /// the current editor tab without running. While the data preview tab is
  /// shown it goes to the editor tab used last; a tab is created only when
  /// there is none.
  void _draft(String text) {
    if (!_current || _session == null) return;
    setState(() {
      var tab = _tab;
      if (tab == null || tab.preview) {
        final last = _lastEditor;
        tab = last != null && _tabs.contains(last) && !last.preview ? last : _tabs.where((t) => !t.preview).lastOrNull;
      }
      if (tab == null) {
        _addTab();
        tab = _tabs.lastOrNull;
        if (tab == null || tab.preview) return;
      }
      _select(_tabs.indexOf(tab));
      tab.controller.appendStatement(text, sql: _session!.engine.sql);
      _view = 'query';
    });
  }

  /// First rows of [object] in the one reusable data preview tab, run at
  /// once (a read never needs a review dialog).
  Future<void> _preview(DbObjectRef object, String text) async {
    if (!_current || _session == null) return;
    var tab = _tabs.where((t) => t.preview).firstOrNull;
    if (tab == null) {
      if (_tabs.length >= 12) {
        // No room for the preview tab: the query goes to the editor instead.
        _draft(text);
        return;
      }
      setState(_addTab);
      tab = _tabs.lastOrNull;
      if (tab == null) return;
    }
    if (tab.result.running || tab.result.loadingMore) await tab.result.cancel();
    if (!_tabCurrent(tab) || tab.preparing) return;
    setState(() {
      tab!
        ..previewName = object.name
        ..previewText = text;
      tab.controller.value = TextEditingValue(text: text, selection: const TextSelection.collapsed(offset: 0));
      _active = _tabs.indexOf(tab);
      _view = 'query';
    });
    await _run(DbRunMode.script);
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
    _prodTimer?.cancel();
    _prodTimer = null;
    _roleCannotWrite = false;
    final session = _session;
    _session = null;
    for (final tab in _tabs) {
      tab.dispose();
    }
    _tabs.clear();
    _lastEditor = null;
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
      final live = _session ?? session;
      if (review.risk == DbQueryRisk.blocked || (!live.writable && review.risk != DbQueryRisk.read)) {
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
          if (open && _tabCurrent(tab)) _draft(suggestion);
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
          builder: (_) => DbQueryReviewDialog(query: request.text, review: review),
        );
        if (!_tabCurrent(tab) || approved != true) return;
      }
      if (!DateTime.now().isBefore(review.expiresAt)) {
        if (mounted) showSnack(context, context.l10n.dbReviewExpired, error: true);
        return;
      }
      tab.pending.clear();
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

  DbProductionWriteService? get _prodWrites => _backend is DbProductionWriteService ? _backend : null;

  void _setSession(DbSessionInfo updated) {
    if (!_current || _session?.id != updated.id) return;
    setState(() => _session = updated);
    _watchProductionWrite();
  }

  /// Keeps the countdown of an open production write window running and,
  /// when it ends, asks the core for the re-locked state.
  void _watchProductionWrite() {
    _prodTimer?.cancel();
    _prodTimer = null;
    final session = _session;
    if (session == null || !session.productionWriteOpen) return;
    _prodTimer = Timer.periodic(const Duration(seconds: 1), (_) {
      final current = _session;
      if (!_current || current == null) {
        _prodTimer?.cancel();
        return;
      }
      if (current.productionWriteOpen) {
        setState(() {});
        return;
      }
      _prodTimer?.cancel();
      _prodTimer = null;
      setState(() {});
      unawaited(_refreshSession(notify: true));
    });
  }

  Future<void> _refreshSession({bool notify = false}) async {
    final session = _session, service = _prodWrites;
    if (session == null || service == null) return;
    try {
      final updated = await service.sessionState(session.id);
      _setSession(updated);
    } catch (_) {}
    if (notify && _current && mounted) showSnack(context, context.l10n.dbProdWriteLocked);
  }

  Future<void> _unlockProduction() async {
    final session = _session, service = _prodWrites;
    if (!_current || session == null || service == null || !session.production || _mobile) return;
    final updated = await showAppDialog<DbSessionInfo>(
      context,
      secure: true,
      builder: (_) => DbProdUnlockDialog(service: service, session: session, connectionName: widget.connection.name),
    );
    if (updated == null || !_current || _session?.id != session.id) return;
    _roleCannotWrite = false;
    _setSession(updated);
    // A read-only role still refuses on the server: say so right away.
    try {
      final report = await _service!.checkPrivileges(session.id);
      if (_current && _session?.id == session.id) setState(() => _roleCannotWrite = report.hasWritePrivileges == false);
    } catch (_) {}
  }

  Future<void> _lockProduction() async {
    final session = _session, service = _prodWrites;
    if (!_current || session == null || service == null) return;
    try {
      _setSession(await service.lockProduction(session.id));
      if (_current) setState(() => _roleCannotWrite = false);
    } catch (_) {
      _error();
    }
  }

  /// Copies one result value: masked columns through the clipboard that
  /// clears itself.
  Future<void> _copyCell(DbResultSet result, int row, int column) async {
    if (!_current) return;
    final info = result.columns[column], cell = result.rows[row][column];
    final l = context.l10n;
    if (info.sensitive) {
      await copySecretWithNotice(context, ref, cell.text, what: l.copyWhatValue);
    } else {
      await copyPlainWithNotice(context, ref, cell.text, what: l.copyWhatValue);
    }
  }

  /// Cell edits are possible when writes are allowed (a non-production
  /// session or an open production window) on a single-table result with a
  /// primary key, on desktop.
  bool _canEdit(DbSessionInfo session, DbResultSet result) =>
      !_mobile &&
      session.writable &&
      result.editableTable != null &&
      result.columns.any((c) => c.primaryKey) &&
      _backend is DbCellEditService;

  /// Picks one of the user's snippets for this engine and puts it below the
  /// text of the editor (like any generated text); nothing runs.
  Future<void> _insertSnippet() async {
    final session = _session;
    if (!_current || session == null) return;
    final sql = await pickDbSnippet(context, ref, engine: session.engine);
    if (sql != null && _current && _session?.id == session.id) _draft(sql);
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

  void _editCell(_QueryTab tab, DbResultSet result, int row, int column, DbCell value) {
    final session = _session;
    if (!_tabCurrent(tab) || session == null || !_canEdit(session, result)) return;
    final original = result.rows[row][column];
    final same = original.kind == value.kind && original.text == value.text;
    setState(() {
      if (same) {
        tab.pending.remove((row, column));
      } else {
        tab.pending[(row, column)] = value;
      }
    });
  }

  Future<void> _applyEdits(_QueryTab tab, DbResultSet result) async {
    final service = _backend, session = _session;
    if (!_tabCurrent(tab) ||
        session == null ||
        !_canEdit(session, result) ||
        service is! DbCellEditService ||
        tab.pending.isEmpty) {
      return;
    }
    final edits = [
      for (final MapEntry(key: (row, column), :value) in tab.pending.entries)
        DbCellEdit(
          originalPrimaryKey: {
            for (var i = 0; i < result.columns.length; i++)
              if (result.columns[i].primaryKey) result.columns[i].name: result.rows[row][i],
          },
          column: result.columns[column].name,
          value: value,
        ),
    ];
    DbCellEditReview? review;
    try {
      review = await service.reviewCellEdits(session.id, result.editableTable!, edits);
      if (!mounted || !_tabCurrent(tab)) return;
      final approved = await showAppDialog<bool>(
        context,
        secure: true,
        builder: (_) => DbQueryReviewDialog(
          query: review!.sql.expose(),
          review: DbQueryReview(ticket: review.ticket, risk: DbQueryRisk.write, expiresAt: review.expiresAt),
        ),
      );
      if (!_tabCurrent(tab) || approved != true || !DateTime.now().isBefore(review.expiresAt)) return;
      await service.commitCellEdits(session.id, review.ticket);
      if (_tabCurrent(tab)) {
        setState(() {
          tab.pending.clear();
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

  bool get _hasHistory => _backend is DbHistoryService;
  bool get _hasAi => _backend is DbAiService;

  Color? get _connectionColor {
    final color = widget.connection.color;
    if (color == null || !RegExp(r'^#[a-fA-F0-9]{6}$').hasMatch(color)) return null;
    return Color(int.parse('ff${color.substring(1)}', radix: 16));
  }

  /// Opens a side panel (from the editor or a shortcut); on narrow layouts
  /// the same panel replaces the editor view.
  void _showSide(String side) => setState(() {
    _side = side;
    _view = side;
  });

  void _toggleSide(String side) => setState(() {
    _side = _side == side ? null : side;
    _view = _side ?? 'query';
  });

  /// Rounded panel that clips its content to the surface corners.
  Widget _panel(Key key, Widget child, {bool solid = false}) {
    final radius = GlassTokens.of(context).radii.card;
    return ContentSurface(
      key: key,
      kind: solid ? ContentSurfaceKind.solid : ContentSurfaceKind.content,
      radius: radius,
      padding: EdgeInsets.zero,
      child: ClipRRect(borderRadius: BorderRadius.circular(radius), child: child),
    );
  }

  Widget _separator() => Container(height: 1, color: GlassTokens.of(context).surfaces.separator);

  /// Title row of a docked panel: icon, caption and trailing actions.
  Widget _panelHeader(IconData icon, String title, List<Widget> actions) {
    final tokens = GlassTokens.of(context);
    return SizedBox(
      height: 36,
      child: Padding(
        padding: const EdgeInsets.only(left: GlassSpacing.s12, right: GlassSpacing.s4),
        child: Row(
          children: [
            Icon(icon, size: 15, color: tokens.secondaryLabel),
            const SizedBox(width: GlassSpacing.s6),
            Expanded(
              child: Text(
                title,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel, fontWeight: FontWeight.w600),
              ),
            ),
            ...actions,
          ],
        ),
      ),
    );
  }

  Widget _tool(String key, IconData icon, String tooltip, VoidCallback? onPressed, {bool selected = false}) =>
      GlassIconButton(
        key: ValueKey(key),
        icon: icon,
        tooltip: tooltip,
        size: 30,
        iconSize: 17,
        selected: selected,
        style: GlassIconButtonStyle.plain,
        onPressed: onPressed,
      );

  /// Production write window controls: the unlock button, or the "writes
  /// until" badge with its countdown and the lock button.
  List<Widget> _productionWrite(DbSessionInfo session, {required bool wide}) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    if (!session.production || _prodWrites == null) return const [];
    final until = session.productionWriteUntil;
    if (session.productionWriteOpen && until != null) {
      final left = until.difference(DateTime.now());
      final seconds = left.inSeconds.clamp(0, 24 * 3600);
      final countdown = '${seconds ~/ 60}:${(seconds % 60).toString().padLeft(2, '0')}'; // l10n-ignore: mm:ss countdown
      final time = DateFormat.Hm(Localizations.localeOf(context).toLanguageTag()).format(until);
      return [
        if (wide) ...[
          GlassBadge(
            key: const ValueKey('db-prod-write-badge'),
            label: l.dbProdWriteUntil(time),
            tone: GlassTone.danger,
            icon: Icons.lock_open_rounded,
            dense: true,
          ),
          const SizedBox(width: GlassSpacing.s6),
        ],
        Tooltip(
          message: l.dbProdWriteUntil(time),
          child: Text(
            wide ? l.dbProdWriteLeft(countdown) : countdown,
            key: const ValueKey('db-prod-write-countdown'),
            style: tokens.typography.caption.copyWith(
              color: tokens.palette.danger,
              fontWeight: FontWeight.w600,
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
        ),
        const SizedBox(width: GlassSpacing.s6),
        GlassButton(
          key: const ValueKey('db-prod-lock'),
          label: l.dbLockProduction,
          icon: Icons.lock_rounded,
          size: GlassControlSize.sm,
          onPressed: _lockProduction,
        ),
        const SizedBox(width: GlassSpacing.s6),
      ];
    }
    if (!session.canUnlockProduction || _mobile) return const [];
    return [
      if (wide)
        GlassButton(
          key: const ValueKey('db-prod-unlock'),
          label: l.dbUnlockProduction,
          icon: Icons.lock_open_rounded,
          size: GlassControlSize.sm,
          onPressed: _unlockProduction,
        )
      else
        _tool('db-prod-unlock', Icons.lock_open_rounded, l.dbUnlockProduction, _unlockProduction),
      const SizedBox(width: GlassSpacing.s6),
    ];
  }

  Widget _header(DbSessionInfo session, {required bool wide}) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final readOnly = !session.writable;
    return ContentSurface(
      key: const ValueKey('db-header'),
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s6, GlassSpacing.s6, GlassSpacing.s8, GlassSpacing.s6),
      child: Row(
        children: [
          _tool('db-close', Icons.arrow_back_rounded, l.commonClose, widget.onClose),
          const SizedBox(width: GlassSpacing.s6),
          Icon(Icons.storage_rounded, size: 20, color: _connectionColor ?? tokens.palette.accent),
          const SizedBox(width: GlassSpacing.s8),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  widget.connection.name,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                ),
                Text(
                  '${session.engine.label} ${session.serverVersion}',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                ),
              ],
            ),
          ),
          if (session.transportSecurity case final security?) ...[
            DbTransportBadge(security, compact: !wide, key: const ValueKey('db-transport-badge')),
            const SizedBox(width: GlassSpacing.s6),
          ],
          if (session.production) ...[
            GlassBadge(label: l.dbProd, tone: GlassTone.danger, icon: Icons.shield_outlined, dense: true),
            const SizedBox(width: GlassSpacing.s6),
          ],
          ..._productionWrite(session, wide: wide),
          if (wide && !session.productionWriteOpen) ...[
            GlassBadge(
              key: const ValueKey('db-access-badge'),
              label: readOnly ? l.dbReadOnly : l.dbWriteAllowed,
              tone: readOnly ? GlassTone.neutral : GlassTone.warning,
              icon: readOnly ? Icons.lock_outline_rounded : Icons.edit_outlined,
              dense: true,
            ),
            const SizedBox(width: GlassSpacing.s6),
          ],
          if (!session.production)
            _tool(
              'db-write-mode',
              session.readOnly ? Icons.lock_rounded : Icons.lock_open_rounded,
              session.readOnly ? l.dbAllowWrites : l.dbReadOnly,
              _writeMode,
            ),
          _tool('db-privileges', Icons.verified_user_outlined, l.dbCheckPrivileges, _privileges),
          if (wide) ...[
            Container(
              width: 1,
              height: 18,
              margin: const EdgeInsets.symmetric(horizontal: GlassSpacing.s6),
              color: tokens.surfaces.separator,
            ),
            _tool(
              'db-toggle-tree',
              Icons.account_tree_outlined,
              l.dbTree,
              () => setState(() => _navOpen = !_navOpen),
              selected: _navOpen,
            ),
            if (_hasHistory)
              _tool(
                'db-toggle-history',
                Icons.history_rounded,
                l.dbHistory,
                () => _toggleSide('history'),
                selected: _side == 'history',
              ),
            if (_hasAi)
              _tool(
                'db-toggle-ai',
                Icons.auto_awesome_outlined,
                l.dbAi,
                () => _toggleSide('ai'),
                selected: _side == 'ai',
              ),
          ],
        ],
      ),
    );
  }

  Widget _statusBar(_QueryTab tab, DbSessionInfo session, DbResultSet? result) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final busy = tab.preparing || tab.result.running;
    final failed = tab.result.errorCode != null;
    final style = tokens.typography.caption.copyWith(color: tokens.secondaryLabel);
    final facts = [
      if (result != null) l.dbRowCount('${result.rows.length}${result.hasMore ? '+' : ''}'),
      if (result != null && tab.selectedRows.isNotEmpty) l.dbSelectedRows(tab.selectedRows.length),
      if (result?.affectedRows != null && result!.columns.isEmpty) l.dbRowsAffected(result.affectedRows!),
      if (result != null) l.dbElapsed(result.elapsedMs),
    ];
    return Container(
      key: const ValueKey('db-status-bar'),
      height: 26,
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
                    : tab.result.cancelled
                    ? l.dbCancelled
                    : l.dbStatusReady,
                ...facts,
              ].join('  ·  '), // l10n-ignore: Visual separator between status facts.
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: style,
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

  Widget _results(_QueryTab tab, DbSessionInfo session) {
    final l = context.l10n, tokens = GlassTokens.of(context), results = tab.result.results;
    final result = results.where((r) => r.index == tab.resultIndex).firstOrNull ?? results.firstOrNull;
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
          onChanged: (index) => setState(() {
            tab.resultIndex = index;
            tab.selectedRows.clear();
            tab.pending.clear();
          }),
        ),
      ],
      const SizedBox(width: GlassSpacing.s8),
    ];
    final actions = [
      if (result != null && _backend is DbExportService)
        _tool(
          'db-export',
          Icons.file_download_outlined,
          l.dbExport,
          () => _export(
            tab,
            result,
            tab.selectedRows.isEmpty ? null : DbResultSelection(rows: List.of(tab.selectedRows)),
          ),
        ),
      if (tab.result.notices.isNotEmpty)
        _tool('db-messages', Icons.chat_bubble_outline_rounded, l.dbMessages, () async {
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
        }),
    ];
    return _panel(
      const ValueKey('db-results'),
      solid: true,
      Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (result == null) ...[_panelHeader(Icons.table_rows_outlined, l.dbResults, actions), _separator()],
          SizedBox(height: 2, child: tab.preparing || tab.result.running ? const LinearProgressIndicator() : null),
          if (tab.result.errorCode != null || tab.stale)
            Padding(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s6, GlassSpacing.s8, 0),
              child: InfoBanner(
                tone: tab.result.errorCode != null ? BannerTone.danger : BannerTone.warning,
                message: tab.result.errorCode != null ? l.dbOperationFailed : l.dbResultStale,
              ),
            ),
          if (tab.pending.isNotEmpty)
            Padding(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s6, GlassSpacing.s8, 0),
              child: Wrap(
                spacing: GlassSpacing.s8,
                runSpacing: GlassSpacing.s6,
                children: [
                  GlassButton(
                    key: const ValueKey('db-apply-edits'),
                    label: '${l.dbApplyEdits} (${tab.pending.length})',
                    style: GlassButtonStyle.prominent,
                    size: GlassControlSize.sm,
                    onPressed: result == null ? null : () => _applyEdits(tab, result),
                  ),
                  GlassButton(
                    label: l.dbDiscardEdits,
                    size: GlassControlSize.sm,
                    onPressed: () => setState(tab.pending.clear),
                  ),
                ],
              ),
            ),
          Expanded(
            child: result == null
                ? Center(
                    child: Text(
                      l.dbNoResults,
                      style: tokens.typography.caption.copyWith(color: tokens.palette.tertiary),
                    ),
                  )
                : DbResultGrid(
                    key: ValueKey('${tab.id}:${result.index}'),
                    result: result,
                    leading: title,
                    trailing: actions,
                    onCopy: _backend is DbExportService ? (selection) => _export(tab, result, selection) : null,
                    onCopyCell: (row, column) => unawaited(_copyCell(result, row, column)),
                    onCellEdit: _canEdit(session, result)
                        ? (row, column, value) => _editCell(tab, result, row, column, value)
                        : null,
                    pending: tab.pending,
                    onSelection: (rows) => setState(() => tab.selectedRows = rows),
                    onMore: result.hasMore ? () => tab.result.more() : null,
                    loadingMore: tab.result.loadingMore,
                  ),
          ),
          _statusBar(tab, session, result),
        ],
      ),
    );
  }

  Widget _queryPane(_QueryTab tab, DbSessionInfo session) {
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        GlassTabStrip(
          key: const ValueKey('db-query-tabs'),
          tabs: [
            for (final item in _tabs)
              GlassTab(
                key: ValueKey('db-query-tab-${item.id}'),
                title: item.preview ? l.dbPreviewTabTitle(item.previewName!) : '${l.dbEditor} ${item.id}',
                state: item.result.running ? GlassTabState.reconnecting : GlassTabState.none,
              ),
          ],
          activeIndex: _active,
          onSelect: (i) => setState(() => _select(i)),
          onClose: (i) => _closeTab(_tabs[i]),
          onAdd: _tabs.length < 12 ? () => setState(() => _addTab()) : null,
        ),
        const SizedBox(height: GlassSpacing.s8),
        Expanded(
          child: LayoutBuilder(
            builder: (context, bounds) => Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                SizedBox(
                  height: bounds.maxHeight * _editorFraction,
                  child: _panel(
                    const ValueKey('db-editor-panel'),
                    solid: true,
                    DbQueryEditor(
                      key: ValueKey(tab.id),
                      controller: tab.controller,
                      session: session,
                      service: _service!,
                      isCurrent: () => _tabCurrent(tab),
                      running: tab.preparing || tab.result.running,
                      onRun: _run,
                      onCancel: tab.result.cancel,
                      onExplain: _explain,
                      onHistory: _hasHistory ? () => _showSide('history') : null,
                      onAi: _hasAi ? () => _showSide('ai') : null,
                      onSnippets: _insertSnippet,
                    ),
                  ),
                ),
                _Splitter(
                  key: const ValueKey('db-editor-splitter'),
                  vertical: false,
                  onDrag: (dy) =>
                      setState(() => _editorFraction = (_editorFraction + dy / bounds.maxHeight).clamp(.2, .75)),
                ),
                Expanded(child: _results(tab, session)),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Widget _navigator(Widget tree) => _panel(
    const ValueKey('db-navigator'),
    Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _panelHeader(Icons.account_tree_outlined, context.l10n.dbTree, const []),
        _separator(),
        Expanded(child: tree),
      ],
    ),
  );

  Widget _sidePanel(String side, DbSessionInfo session) {
    final l = context.l10n, history = side == 'history';
    return _panel(
      ValueKey('db-side-$side'),
      Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _panelHeader(history ? Icons.history_rounded : Icons.auto_awesome_outlined, history ? l.dbHistory : l.dbAi, [
            _tool('db-side-close', Icons.close_rounded, l.commonClose, () => setState(() => _side = null)),
          ]),
          _separator(),
          Expanded(
            child: history
                ? DbHistoryPanel(connection: widget.connection, service: _backend! as DbHistoryService, onDraft: _draft)
                : _ai(session),
          ),
        ],
      ),
    );
  }

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

  Widget _connectPane() {
    final l = context.l10n, tokens = GlassTokens.of(context);
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 380),
        child: ContentSurface(
          key: const ValueKey('db-connect-pane'),
          padding: const EdgeInsets.all(GlassSpacing.s24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Icon(Icons.storage_rounded, size: 32, color: _connectionColor ?? tokens.palette.accent),
              const SizedBox(height: GlassSpacing.s8),
              Text(
                widget.connection.name,
                textAlign: TextAlign.center,
                style: tokens.typography.title3.copyWith(color: tokens.palette.label),
              ),
              Text(
                widget.connection.engine.label,
                textAlign: TextAlign.center,
                style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
              ),
              const SizedBox(height: GlassSpacing.s16),
              if (_connecting) const Center(child: CircularProgressIndicator()),
              if (_failed) ...[
                InfoBanner(
                  tone: BannerTone.danger,
                  message: _connectError == null ? l.dbOperationFailed : dbFailureText(_connectError!, l),
                ),
                const SizedBox(height: GlassSpacing.s12),
              ],
              if (_asksPassword && !_connecting) ...[
                SecretField(
                  key: const ValueKey('db-connect-password'),
                  controller: _password,
                  label: l.dbPassword,
                  autofocus: true,
                  onSubmitted: (_) => _connect(),
                ),
                const SizedBox(height: GlassSpacing.s12),
              ],
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  GlassButton(label: l.commonClose, onPressed: widget.onClose),
                  if (!_connecting) ...[
                    const SizedBox(width: GlassSpacing.s8),
                    GlassButton(
                      label: _asksPassword || !_failed ? l.commonConnect : l.commonRetry,
                      style: GlassButtonStyle.prominent,
                      onPressed: _connect,
                    ),
                  ],
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) _closeSession();
    });
    ref.watch(dbScopeProvider);
    final l = context.l10n, session = _session, tab = _tab;
    if (!_current) return Center(child: Text(l.dbLocked));
    if (_service == null) return Center(child: Text(l.dbUnavailable));
    if (session == null || tab == null) return _connectPane();
    final pad = _mobile ? GlassSpacing.s8 : GlassSpacing.s12;
    return Material(
      type: MaterialType.transparency,
      child: Padding(
        padding: EdgeInsets.all(pad),
        child: LayoutBuilder(
          builder: (context, bounds) {
            final wide = bounds.maxWidth >= 900;
            final tree = DbObjectTree(
              key: ValueKey(session.id),
              session: session,
              service: _service!,
              onDraft: _draft,
              onPreview: (object, text) => unawaited(_preview(object, text)),
              onInsert: (value) {
                if (_current) _tab?.controller.insert(value);
              },
              onSelect: (object) => setState(() => _selectedObject = object),
            );
            final side = switch (_side) {
              'history' when _hasHistory => 'history',
              'ai' when _hasAi => 'ai',
              _ => null,
            };
            final Widget body;
            if (wide) {
              // Navigator | editor + results | optional history/AI panel.
              final showNav = _navOpen && (side == null || bounds.maxWidth >= 1100);
              final navWidth = _navWidth.clamp(200.0, bounds.maxWidth * .35);
              body = Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (showNav) ...[
                    SizedBox(width: navWidth, child: _navigator(tree)),
                    _Splitter(
                      key: const ValueKey('db-navigator-splitter'),
                      vertical: true,
                      onDrag: (dx) => setState(() => _navWidth = (navWidth + dx).clamp(200.0, bounds.maxWidth * .35)),
                    ),
                  ],
                  Expanded(child: _queryPane(tab, session)),
                  if (side != null) ...[
                    const SizedBox(width: GlassSpacing.s8),
                    SizedBox(width: bounds.maxWidth >= 1300 ? 380 : 330, child: _sidePanel(side, session)),
                  ],
                ],
              );
            } else {
              final view = switch (_view) {
                'history' when _hasHistory => 'history',
                'ai' when _hasAi => 'ai',
                'tree' => 'tree',
                _ => 'query',
              };
              body = Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  GlassSegmented<String>(
                    key: const ValueKey('db-view'),
                    inChrome: false,
                    expand: true,
                    segments: [
                      GlassSegment(value: 'tree', label: l.dbTree, icon: Icons.account_tree_outlined),
                      GlassSegment(value: 'query', label: l.dbEditor, icon: Icons.code_rounded),
                      if (_hasHistory) GlassSegment(value: 'history', label: l.dbHistory, icon: Icons.history_rounded),
                      if (_hasAi) GlassSegment(value: 'ai', label: l.dbAi, icon: Icons.auto_awesome_outlined),
                    ],
                    selected: view,
                    onChanged: (value) => setState(() => _view = value),
                  ),
                  const SizedBox(height: GlassSpacing.s8),
                  Expanded(
                    child: switch (view) {
                      'tree' => _navigator(tree),
                      'history' || 'ai' => _sidePanel(view, session),
                      _ => _queryPane(tab, session),
                    },
                  ),
                ],
              );
            }
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _header(session, wide: wide),
                if (session.production &&
                    !session.productionWriteOpen &&
                    session.serverReadOnly == DbServerReadOnly.unverified)
                  Padding(
                    padding: const EdgeInsets.only(top: GlassSpacing.s8),
                    child: InfoBanner(tone: BannerTone.warning, message: l.dbUnverifiedReadOnly),
                  ),
                if (session.productionWriteOpen && _roleCannotWrite)
                  Padding(
                    padding: const EdgeInsets.only(top: GlassSpacing.s8),
                    child: InfoBanner(
                      key: const ValueKey('db-prod-role-read-only'),
                      tone: BannerTone.warning,
                      message: l.dbProdRoleReadOnly,
                    ),
                  ),
                const SizedBox(height: GlassSpacing.s8),
                Expanded(child: body),
              ],
            );
          },
        ),
      ),
    );
  }
}

/// Drag handle between two panes; [vertical] bars resize columns.
class _Splitter extends StatefulWidget {
  const _Splitter({required this.vertical, required this.onDrag, super.key});
  final bool vertical;
  final ValueChanged<double> onDrag;
  @override
  State<_Splitter> createState() => _SplitterState();
}

class _SplitterState extends State<_Splitter> {
  bool _hover = false, _dragging = false;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final active = _hover || _dragging;
    final vertical = widget.vertical;
    void start(DragStartDetails _) => setState(() => _dragging = true);
    void end(DragEndDetails _) => setState(() => _dragging = false);
    return MouseRegion(
      cursor: vertical ? SystemMouseCursors.resizeColumn : SystemMouseCursors.resizeRow,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onHorizontalDragStart: vertical ? start : null,
        onHorizontalDragUpdate: vertical ? (d) => widget.onDrag(d.delta.dx) : null,
        onHorizontalDragEnd: vertical ? end : null,
        onVerticalDragStart: vertical ? null : start,
        onVerticalDragUpdate: vertical ? null : (d) => widget.onDrag(d.delta.dy),
        onVerticalDragEnd: vertical ? null : end,
        child: SizedBox(
          width: vertical ? GlassSpacing.s8 : null,
          height: vertical ? null : GlassSpacing.s8,
          child: Center(
            child: AnimatedContainer(
              duration: const Duration(milliseconds: 120),
              width: vertical ? 3 : 40,
              height: vertical ? 40 : 3,
              decoration: ShapeDecoration(
                shape: const StadiumBorder(),
                color: active ? tokens.palette.accent : tokens.surfaces.separator,
              ),
            ),
          ),
        ),
      ),
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
