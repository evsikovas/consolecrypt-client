import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_view_preferences_service.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Builds only visible rows, including when a watch cache contains 10,000 items.
class KubeResourceTableView extends ConsumerStatefulWidget {
  const KubeResourceTableView({
    required this.rows,
    required this.kind,
    required this.columns,
    required this.selected,
    required this.canSelect,
    required this.onSelection,
    required this.onOpen,
    required this.hasMore,
    required this.loadingMore,
    required this.onMore,
    this.contextId,
    this.onVisibleRows,
    this.onMobileAction,
    this.mobileTrailingAction,
    this.activeName,
    this.loading = false,
    this.emptyText,
    this.rowTrailing,
    super.key,
  });
  final List<KubeResourceRow> rows;
  final KubeResourceKind kind;
  final List<KubeTableColumn> columns;
  final Set<String> selected;
  final bool canSelect, hasMore, loadingMore;
  final ValueChanged<KubeResourceRow> onSelection, onOpen;
  final VoidCallback onMore;
  final String? contextId;
  final ValueChanged<Set<String>>? onVisibleRows;
  final Future<void> Function(KubeResourceRow, KubeResourceOpenAction)? onMobileAction;
  final KubeResourceOpenAction? mobileTrailingAction;

  /// Object shown in the desktop inspector; its row stays highlighted.
  final KubeResourceRef? activeName;
  final bool loading;

  /// Shown instead of rows when the list is empty.
  final String? emptyText;

  /// Optional per-row action (e.g. a GitOps action menu) shown before the name,
  /// so it stays visible however wide the table scrolls.
  final Widget? Function(KubeResourceRow row)? rowTrailing;
  @override
  ConsumerState<KubeResourceTableView> createState() => _KubeResourceTableViewState();
}

class _KubeResourceTableViewState extends ConsumerState<KubeResourceTableView> {
  final _horizontal = ScrollController(), _vertical = ScrollController();
  final _widths = <String, double>{}, _hidden = <String>{};
  String _sort = 'name';
  bool _ascending = true;
  List<KubeResourceRow> _rows = const [];
  Timer? _saveTimer;
  int _generation = 0;
  bool _dirty = false;
  KubeScope? _scope;
  final _keyboardFocus = FocusNode(
    debugLabel: /* l10n-ignore: Developer-only focus-node diagnostic label. */ 'Kubernetes resource table',
  ); // l10n-ignore: Developer-only focus-node diagnostic label.
  String? _keyboardId;
  double? _rowExtent;
  Set<String> _lastVisible = {};
  bool _visibleScheduled = false;
  @override
  void initState() {
    super.initState();
    _sortRows();
    _vertical.addListener(_scheduleVisibleRows);
    unawaited(_loadPreferences());
  }

  @override
  void didUpdateWidget(KubeResourceTableView old) {
    super.didUpdateWidget(old);
    if (old.kind.id != widget.kind.id || old.contextId != widget.contextId) {
      _generation++;
      _saveTimer?.cancel();
      _dirty = false;
      _sort = 'name';
      _ascending = true;
      _hidden.clear();
      _widths.clear();
      unawaited(_loadPreferences());
    }
    if (!identical(old.rows, widget.rows) || !identical(old.columns, widget.columns)) _sortRows();
  }

  @override
  void dispose() {
    _generation++;
    _saveTimer?.cancel();
    _horizontal.dispose();
    _vertical.dispose();
    _keyboardFocus.dispose();
    super.dispose();
  }

  Future<void> _loadPreferences() async {
    final id = widget.contextId;
    if (id == null) return;
    final scope = ref.read(kubeScopeProvider);
    _scope = scope;
    final service = ref.read(kubeServiceProvider);
    if (!scope.unlocked || service is! KubeViewPreferencesService) return;
    final generation = _generation, kind = widget.kind.id;
    try {
      final settings = await (service as KubeViewPreferencesService).readKubeViewPreferences(id);
      if (!mounted || generation != _generation || _dirty || !identical(scope, ref.read(kubeScopeProvider))) return;
      final table = (settings['tables'] as Map?)?[kind] as Map?;
      if (table == null) return;
      setState(() {
        _hidden.addAll((table['hidden'] as List? ?? const []).whereType<String>().where((id) => id != 'name'));
        for (final entry in ((table['widths'] as Map?) ?? {}).entries) {
          if (entry.key is String && entry.value is num) {
            _widths[entry.key as String] = (entry.value as num).toDouble().clamp(80, 600);
          }
        }
        _sort = table['sort'] as String? ?? 'name';
        _ascending = table['ascending'] as bool? ?? true;
        _sortRows();
      });
    } catch (_) {
      /* Presentation defaults remain usable if storage is unavailable. */
    }
  }

  void _savePreferences() {
    _dirty = true;
    _saveTimer?.cancel();
    final id = widget.contextId, scope = _scope;
    if (id == null || scope == null) return;
    final patch = <String, dynamic>{
      'kind': 'table',
      'resource_kind': widget.kind.id,
      'preferences': {'widths': Map.of(_widths), 'hidden': _hidden.toList(), 'sort': _sort, 'ascending': _ascending},
    };
    _saveTimer = Timer(const Duration(milliseconds: 250), () async {
      if (!mounted || !identical(scope, ref.read(kubeScopeProvider)) || !scope.unlocked) return;
      final service = ref.read(kubeServiceProvider);
      if (service is KubeViewPreferencesService) {
        try {
          await (service as KubeViewPreferencesService).updateKubeViewPreferences(id, patch: patch);
        } catch (_) {}
      }
    });
  }

  List<_Column> get _columns => [
    const _Column('name', -1),
    if (widget.kind.namespaced) const _Column('namespace', -1),
    const _Column('status', -1),
    const _Column('age', -1),
    for (final (index, column) in widget.columns.indexed)
      if (!{'name', 'namespace', 'status', 'age'}.contains(column.name.toLowerCase()))
        _Column('native:$index:${column.name}', index),
  ];
  String _label(_Column column) => switch (column.id) {
    'name' => context.l10n.kubeName,
    'namespace' => context.l10n.kubeNamespace,
    'status' => context.l10n.kubeStatus,
    'age' => context.l10n.kubeAge,
    _ => widget.columns[column.index].name,
  };
  String _value(KubeResourceRow row, _Column column) => switch (column.id) {
    'name' => row.name,
    'namespace' => row.namespace ?? '—',
    'status' => row.status.isEmpty ? '—' : row.status,
    'age' => row.createdAt == null ? '—' : _age(DateTime.now().difference(row.createdAt!)),
    _ => column.index < row.cells.length ? row.cells[column.index] : '—',
  };
  String _age(Duration age) {
    if (age.isNegative) return '0s'; // l10n-ignore: compact Kubernetes duration notation.
    if (age.inDays > 0) return '${age.inDays}d'; // l10n-ignore: Kubernetes duration.
    if (age.inHours > 0) return '${age.inHours}h'; // l10n-ignore: Kubernetes duration.
    if (age.inMinutes > 0) return '${age.inMinutes}m'; // l10n-ignore: Kubernetes duration.
    return '${age.inSeconds}s'; // l10n-ignore: Kubernetes duration.
  }

  void _sortRows() {
    final column = _columns.where((column) => column.id == _sort).firstOrNull ?? const _Column('name', -1);
    _rows = List.of(widget.rows)
      ..sort((a, b) {
        int result;
        if (column.id == 'age') {
          result = (a.createdAt?.millisecondsSinceEpoch ?? 0).compareTo(b.createdAt?.millisecondsSinceEpoch ?? 0);
        } else {
          final left = _value(a, column), right = _value(b, column);
          final numeric = column.index >= 0 && {'integer', 'number'}.contains(widget.columns[column.index].type);
          result = numeric && num.tryParse(left) != null && num.tryParse(right) != null
              ? num.parse(left).compareTo(num.parse(right))
              : left.toLowerCase().compareTo(right.toLowerCase());
        }
        if (result == 0) result = a.id.compareTo(b.id);
        return _ascending ? result : -result;
      });
    if (!_rows.any((row) => row.id == _keyboardId)) _keyboardId = null;
    _scheduleVisibleRows();
  }

  void _scheduleVisibleRows() {
    if (_visibleScheduled || widget.onVisibleRows == null) return;
    _visibleScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _visibleScheduled = false;
      if (!mounted || !_vertical.hasClients) return;
      final extent = _rowExtent ?? 80.0;
      final first = (_vertical.offset / extent).floor().clamp(0, _rows.length);
      final end = (first + (_vertical.position.viewportDimension / extent).ceil() + 2).clamp(0, _rows.length);
      final visible = _rows.sublist(first, end).map((row) => row.id).toSet();
      if (visible.length == _lastVisible.length && visible.containsAll(_lastVisible)) return;
      _lastVisible = visible;
      widget.onVisibleRows?.call(visible);
    });
  }

  void _changeSort(String id) => setState(() {
    if (_sort == id) {
      _ascending = !_ascending;
    } else {
      _sort = id;
      _ascending = true;
    }
    _sortRows();
    _savePreferences();
  });

  KeyEventResult _navigate(FocusNode node, KeyEvent event) {
    if (event is KeyUpEvent || _rows.isEmpty) return KeyEventResult.ignored;
    final keyboard = HardwareKeyboard.instance;
    if (keyboard.isControlPressed || keyboard.isMetaPressed || keyboard.isAltPressed) return KeyEventResult.ignored;
    final current = _rows.indexWhere((row) => row.id == _keyboardId);
    final key = event.logicalKey;
    if (key == LogicalKeyboardKey.enter || key == LogicalKeyboardKey.space) {
      if (current < 0) return KeyEventResult.ignored;
      if (key == LogicalKeyboardKey.enter && widget.kind.canRead) widget.onOpen(_rows[current]);
      if (key == LogicalKeyboardKey.space && widget.canSelect) widget.onSelection(_rows[current]);
      return KeyEventResult.handled;
    }
    final page = _vertical.hasClients ? (_vertical.position.viewportDimension / (_rowExtent ?? 48)).floor() : 10;
    final target = switch (key) {
      LogicalKeyboardKey.arrowDown => current + 1,
      LogicalKeyboardKey.arrowUp => current < 0 ? 0 : current - 1,
      LogicalKeyboardKey.home => 0,
      LogicalKeyboardKey.end => _rows.length - 1,
      LogicalKeyboardKey.pageDown => math.max(0, current) + page,
      LogicalKeyboardKey.pageUp => math.max(0, current) - page,
      _ => null,
    };
    if (target == null) return KeyEventResult.ignored;
    final index = target.clamp(0, _rows.length - 1);
    _keyboardFocus.requestFocus();
    setState(() => _keyboardId = _rows[index].id);
    if (_vertical.hasClients && _rowExtent != null) {
      final top = index * _rowExtent!, bottom = (index + 1) * _rowExtent!;
      final position = _vertical.position;
      final offset = top < position.pixels
          ? top
          : bottom > position.pixels + position.viewportDimension
          ? bottom - position.viewportDimension
          : position.pixels;
      _vertical.jumpTo(offset.clamp(0, position.maxScrollExtent));
    }
    return KeyEventResult.handled;
  }

  double _width(_Column column) =>
      _widths[column.id] ??
      switch (column.id) {
        'name' => 260,
        'namespace' => 170,
        'status' => 180,
        'age' => 100,
        _ => 150,
      };
  Widget _select(KubeResourceRow row) => Checkbox(
    key: ValueKey('kube-select-${row.id}'),
    value: widget.selected.contains(row.id),
    onChanged: (_) => widget.onSelection(row),
    semanticLabel: row.name,
  );
  Widget _more() => Center(
    child: Padding(
      padding: const EdgeInsets.all(8),
      child: GlassButton(
        key: const ValueKey('kube-load-more'),
        label: context.l10n.kubeLoadMore,
        onPressed: widget.loadingMore ? null : widget.onMore,
      ),
    ),
  );
  Widget _mobile() => ListView.builder(
    key: const ValueKey('kube-resource-list'),
    controller: _vertical,
    itemCount: _rows.length + (widget.hasMore ? 1 : 0),
    itemBuilder: (context, index) {
      if (index == _rows.length) return _more();
      final row = _rows[index];
      final tile = ListTile(
        key: ValueKey('kube-resource-${row.id}'),
        selected: widget.selected.contains(row.id),
        leading: widget.canSelect ? _select(row) : null,
        title: Text(row.name, maxLines: 2, overflow: TextOverflow.ellipsis),
        subtitle: Text(
          [
            if (row.namespace != null) row.namespace!,
            if (row.status.isNotEmpty) row.status,
            for (final column in _columns.where((c) => c.index >= 0).take(3))
              '${_label(column)}: ${_value(row, column)}',
          ].join(' · '),
          maxLines: 3,
          overflow: TextOverflow.ellipsis,
        ),
        trailing: widget.kind.canRead ? const Icon(Icons.chevron_right) : null,
        onTap: widget.kind.canRead ? () => widget.onOpen(row) : null,
      );
      final action = widget.onMobileAction;
      if (action == null || !widget.kind.canRead) return tile;
      Widget background(KubeResourceOpenAction operation, Alignment alignment) => ColoredBox(
        color: Theme.of(context).colorScheme.secondaryContainer,
        child: Align(
          alignment: alignment,
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Text(switch (operation) {
              KubeResourceOpenAction.logs => context.l10n.kubeLogs,
              KubeResourceOpenAction.shell => context.l10n.kubePodTerminal,
              KubeResourceOpenAction.restart => context.l10n.kubeRestart,
            }),
          ),
        ),
      );
      return Dismissible(
        key: ValueKey('kube-resource-swipe-${row.id}'),
        direction: widget.mobileTrailingAction == null ? DismissDirection.startToEnd : DismissDirection.horizontal,
        background: background(KubeResourceOpenAction.logs, Alignment.centerLeft),
        secondaryBackground: widget.mobileTrailingAction == null
            ? null
            : background(widget.mobileTrailingAction!, Alignment.centerRight),
        confirmDismiss: (direction) async {
          if (!mounted) return false;
          await action(
            row,
            direction == DismissDirection.startToEnd ? KubeResourceOpenAction.logs : widget.mobileTrailingAction!,
          );
          return false;
        },
        child: tile,
      );
    },
  );
  @override
  Widget build(BuildContext context) => ContentSurface(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: LayoutBuilder(
      builder: (context, constraints) {
        if (AppPlatform.isMobile && constraints.maxWidth < 720 || constraints.maxWidth < 420) {
          _rowExtent = null;
          _scheduleVisibleRows();
          return Column(
            children: [
              Align(
                alignment: Alignment.centerRight,
                child: PopupMenuButton<String>(
                  key: const ValueKey('kube-mobile-sort'),
                  tooltip: context.l10n.kubeTableSort,
                  onSelected: _changeSort,
                  itemBuilder: (_) => [
                    for (final column in _columns)
                      CheckedPopupMenuItem(
                        value: column.id,
                        checked: _sort == column.id,
                        child: Text(
                          /* l10n-ignore: Localized column name and sort-arrow symbols. */ '${_label(column)}${_sort == column.id ? (_ascending ? ' ↑' : ' ↓') : ''}',
                        ), // l10n-ignore: Localized column name and sort-arrow symbols.
                      ),
                  ],
                  child: Padding(
                    padding: const EdgeInsets.all(12),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        const Icon(Icons.sort),
                        const SizedBox(width: 8),
                        Flexible(
                          child: Text(
                            _label(_columns.where((column) => column.id == _sort).firstOrNull ?? _columns.first),
                          ),
                        ),
                        Icon(_ascending ? Icons.arrow_upward : Icons.arrow_downward, size: 16),
                      ],
                    ),
                  ),
                ),
              ),
              Expanded(child: _mobile()),
            ],
          );
        }
        final columns = _columns.where((c) => !_hidden.contains(c.id)).toList();
        final tokens = GlassTokens.of(context);
        final p = tokens.palette;
        final lead = widget.canSelect ? 40.0 : 12.0;
        final trailing = widget.rowTrailing == null ? 0.0 : 44.0;
        final width = math.max(constraints.maxWidth, columns.fold(lead + 40 + trailing, (sum, c) => sum + _width(c)));
        final extent = math.max(34.0, MediaQuery.textScalerOf(context).scale(15) + 18);
        _rowExtent = extent;
        _scheduleVisibleRows();
        final header = tokens.typography.caption.copyWith(
          color: tokens.secondaryLabel,
          fontWeight: FontWeight.w600,
          letterSpacing: 0.3,
        );
        return Stack(
          children: [
            Scrollbar(
              controller: _horizontal,
              thumbVisibility: width > constraints.maxWidth,
              notificationPredicate: (n) => n.depth == 0,
              child: SingleChildScrollView(
                controller: _horizontal,
                scrollDirection: Axis.horizontal,
                child: SizedBox(
                  width: width,
                  child: Column(
                    children: [
                      SizedBox(
                        height: 32,
                        child: Row(
                          children: [
                            SizedBox(width: lead + trailing),
                            for (final column in columns)
                              SizedBox(
                                width: _width(column),
                                child: Stack(
                                  children: [
                                    Positioned.fill(
                                      child: GlassInteractive(
                                        key: ValueKey('kube-sort-${column.id}'),
                                        semanticLabel: _label(column),
                                        onPressed: () => _changeSort(column.id),
                                        builder: (context, state) => Padding(
                                          padding: const EdgeInsets.symmetric(horizontal: 10),
                                          child: Row(
                                            children: [
                                              Flexible(
                                                child: Text(
                                                  _label(column).toUpperCase(),
                                                  maxLines: 1,
                                                  overflow: TextOverflow.ellipsis,
                                                  style: header.copyWith(
                                                    color: _sort == column.id || state.hovered ? p.label : null,
                                                  ),
                                                ),
                                              ),
                                              if (_sort == column.id)
                                                Icon(
                                                  _ascending ? Icons.arrow_drop_up : Icons.arrow_drop_down,
                                                  size: 16,
                                                  color: p.label,
                                                ),
                                            ],
                                          ),
                                        ),
                                      ),
                                    ),
                                    Positioned(
                                      right: 0,
                                      top: 6,
                                      bottom: 6,
                                      width: 8,
                                      child: MouseRegion(
                                        cursor: SystemMouseCursors.resizeColumn,
                                        child: GestureDetector(
                                          behavior: HitTestBehavior.opaque,
                                          onHorizontalDragUpdate: (event) => setState(() {
                                            _widths[column.id] = (_width(column) + event.delta.dx).clamp(80, 600);
                                            _savePreferences();
                                          }),
                                          child: Center(child: Container(width: 1, color: tokens.surfaces.separator)),
                                        ),
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                          ],
                        ),
                      ),
                      Divider(height: 1, color: tokens.surfaces.separator),
                      if (widget.loading && _rows.isEmpty) const LinearProgressIndicator(minHeight: 2),
                      Expanded(
                        child: _rows.isEmpty && widget.emptyText != null
                            ? Align(
                                alignment: const Alignment(-1, -0.9),
                                child: SizedBox(
                                  width: constraints.maxWidth,
                                  child: EmptyStateText(text: widget.emptyText!),
                                ),
                              )
                            : Focus(
                                key: const ValueKey('kube-table-keyboard'),
                                focusNode: _keyboardFocus,
                                onKeyEvent: _navigate,
                                onFocusChange: (_) => setState(() {}),
                                child: ListView.builder(
                                  key: const ValueKey('kube-resource-list'),
                                  controller: _vertical,
                                  itemExtent: extent,
                                  itemCount: _rows.length + (widget.hasMore ? 1 : 0),
                                  itemBuilder: (context, index) {
                                    if (index == _rows.length) return _more();
                                    final row = _rows[index];
                                    final active =
                                        widget.activeName?.name == row.name &&
                                        widget.activeName?.namespace == row.namespace;
                                    final keyboard = _keyboardFocus.hasFocus && row.id == _keyboardId;
                                    return Semantics(
                                      focused: keyboard,
                                      selected: widget.selected.contains(row.id) || active,
                                      child: GlassInteractive(
                                        key: ValueKey('kube-resource-${row.id}'),
                                        canRequestFocus: false,
                                        semanticLabel: row.name,
                                        onPressed: widget.kind.canRead ? () => widget.onOpen(row) : null,
                                        builder: (context, state) => DecoratedBox(
                                          decoration: BoxDecoration(
                                            color: active || widget.selected.contains(row.id)
                                                ? tokens.rowSelection
                                                : keyboard
                                                ? tokens.surfaces.fillPressed
                                                : state.hovered
                                                ? tokens.surfaces.fillHover
                                                : Colors.transparent,
                                            border: active ? Border(left: BorderSide(color: p.accent, width: 2)) : null,
                                          ),
                                          child: SizedBox(
                                            height: extent,
                                            child: Row(
                                              children: [
                                                SizedBox(width: lead, child: widget.canSelect ? _select(row) : null),
                                                if (widget.rowTrailing != null)
                                                  SizedBox(width: trailing, child: widget.rowTrailing!(row)),
                                                for (final column in columns)
                                                  SizedBox(
                                                    width: _width(column),
                                                    child: Padding(
                                                      padding: const EdgeInsets.symmetric(horizontal: 10),
                                                      child: column.id == 'status'
                                                          ? KubeStatusLabel(status: _value(row, column))
                                                          : Text(
                                                              _value(row, column),
                                                              maxLines: 1,
                                                              overflow: TextOverflow.ellipsis,
                                                              style:
                                                                  (column.id == 'name'
                                                                          ? tokens.typography.body
                                                                          : tokens.typography.callout)
                                                                      .copyWith(
                                                                        color: column.id == 'name'
                                                                            ? p.label
                                                                            : tokens.secondaryLabel,
                                                                        fontWeight: column.id == 'name'
                                                                            ? FontWeight.w500
                                                                            : null,
                                                                      ),
                                                            ),
                                                    ),
                                                  ),
                                              ],
                                            ),
                                          ),
                                        ),
                                      ),
                                    );
                                  },
                                ),
                              ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
            Positioned(
              right: 4,
              top: 0,
              height: 32,
              child: GlassMenuButton<String>(
                key: const ValueKey('kube-table-columns'),
                entries: [
                  for (final column in _columns.where((column) => column.id != 'name'))
                    GlassMenuItem<String>(
                      key: ValueKey('kube-column-${column.id}'),
                      value: column.id,
                      label: _label(column),
                      checked: !_hidden.contains(column.id),
                    ),
                ],
                onSelected: (id) => setState(() {
                  if (!_hidden.remove(id)) _hidden.add(id);
                  _savePreferences();
                }),
                builder: (context, open) => GlassIconButton(
                  icon: Icons.view_column_outlined,
                  tooltip: context.l10n.kubeTableColumns,
                  onPressed: open,
                ),
              ),
            ),
          ],
        );
      },
    ),
  );
}

/// Status text with a role-coloured dot (Running → success, Pending →
/// warning, CrashLoopBackOff → danger).
class KubeStatusLabel extends StatelessWidget {
  const KubeStatusLabel({required this.status, super.key});
  final String status;

  static GlassTone tone(String status) {
    final value = status.toLowerCase();
    // Workload readiness "ready/desired".
    final ratio = RegExp(r'^(\d+)/(\d+)$').firstMatch(value);
    if (ratio != null) {
      final ready = int.parse(ratio[1]!), desired = int.parse(ratio[2]!);
      if (desired == 0) return GlassTone.neutral;
      return ready >= desired ? GlassTone.success : (ready == 0 ? GlassTone.danger : GlassTone.warning);
    }
    if (const {
      'running',
      'ready',
      'active',
      'bound',
      'available',
      'healthy',
      'synced',
      'deployed',
      'true',
    }.contains(value)) {
      return GlassTone.success;
    }
    if (const {'succeeded', 'completed', 'complete'}.contains(value)) return GlassTone.neutral;
    if (value.contains('crashloop') ||
        value.contains('error') ||
        value.contains('backoff') ||
        value.contains('fail') ||
        value.contains('evicted') ||
        value.contains('oomkilled') ||
        const {'notready', 'lost', 'degraded', 'false', 'unhealthy'}.contains(value)) {
      return GlassTone.danger;
    }
    if (value.contains('pending') ||
        value.contains('creating') ||
        value.contains('terminating') ||
        value.contains('progress') ||
        value.contains('init') ||
        value == 'unknown') {
      return GlassTone.warning;
    }
    return GlassTone.neutral;
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final glassTone = tone(status);
    final color = glassTone == GlassTone.neutral ? tokens.palette.tertiary : tokens.palette.tone(glassTone);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          width: 7,
          height: 7,
          decoration: BoxDecoration(color: color, shape: BoxShape.circle),
        ),
        const SizedBox(width: 7),
        Flexible(
          child: Text(
            status,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: tokens.typography.callout.copyWith(
              color: glassTone == GlassTone.neutral ? tokens.secondaryLabel : color,
              fontWeight: FontWeight.w500,
            ),
          ),
        ),
      ],
    );
  }
}

/// Quiet one-line message inside an empty table.
class EmptyStateText extends StatelessWidget {
  const EmptyStateText({required this.text, super.key});
  final String text;
  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Text(
        text,
        textAlign: TextAlign.center,
        style: tokens.typography.body.copyWith(color: tokens.secondaryLabel),
      ),
    );
  }
}

final class _Column {
  const _Column(this.id, this.index);
  final String id;
  final int index;
}
