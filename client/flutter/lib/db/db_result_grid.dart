import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:consolecrypt/db/db_result_controller.dart';
import 'package:consolecrypt/db/db_value_dialog.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

/// Exact numeric ordering without converting BigInt/Decimal values to double.
int compareDbCells(DbCell left, DbCell right) {
  if (left.kind == DbCellKind.nullValue) return right.kind == DbCellKind.nullValue ? 0 : -1;
  if (right.kind == DbCellKind.nullValue) return 1;
  if (left.numeric && right.numeric) {
    final a = _decimal(left.text), b = _decimal(right.text);
    if (a != null && b != null) {
      if (a.$1 != b.$1) return a.$1.compareTo(b.$1);
      if (a.$1 == 0) return 0;
      final exponent = a.$2.compareTo(b.$2);
      if (exponent != 0) return exponent * a.$1;
      final length = a.$3.length > b.$3.length ? a.$3.length : b.$3.length;
      return a.$3.padRight(length, '0').compareTo(b.$3.padRight(length, '0')) * a.$1;
    }
  }
  return left.text.compareTo(right.text);
}

(int, BigInt, String)? _decimal(String value) {
  final m = RegExp(r'^([+-]?)(\d*)(?:\.(\d*))?(?:[eE]([+-]?\d+))?$').firstMatch(value);
  if (m == null || '${m[2]}${m[3] ?? ''}'.isEmpty) return null;
  final exponent = BigInt.tryParse(m[4] ?? '0');
  if (exponent == null) return null;
  final whole = m[2]!, fraction = m[3] ?? '';
  final combined = '$whole$fraction';
  final digits = combined.replaceFirst(RegExp(r'^0+'), '');
  if (digits.isEmpty) return (0, BigInt.zero, '0');
  final leading = combined.length - digits.length;
  return (m[1] == '-' ? -1 : 1, BigInt.from(whole.length - leading) + exponent, digits);
}

class DbResultGrid extends StatefulWidget {
  const DbResultGrid({
    required this.result,
    this.production = true,
    this.onCopy,
    this.onEdit,
    this.onSelection,
    this.onMore,
    this.loadingMore = false,
    this.leading = const [],
    this.trailing = const [],
    super.key,
  });
  final DbResultSet result;

  /// Toolbar slots shared with the surrounding panel (title, result sets,
  /// export): the grid keeps a single compact header row.
  final List<Widget> leading, trailing;
  final bool production, loadingMore;
  final void Function(DbResultSelection selection)? onCopy;
  final void Function(int row, int column)? onEdit;
  final ValueChanged<List<int>>? onSelection;
  final VoidCallback? onMore;
  @override
  State<DbResultGrid> createState() => _DbResultGridState();
}

class _DbResultGridState extends State<DbResultGrid> {
  final _horizontal = ScrollController(), _vertical = ScrollController();
  final _selected = <int>{}, _pinned = <int>{}, _hidden = <int>{};
  final _widths = <int, double>{};
  final _auto = <int, (double, int)>{};
  final _recordColumns = <int, int>{};
  String _filter = '';
  int? _sort;
  bool _ascending = true;
  bool? _records;
  List<int>? _order;
  Timer? _filterTimer;
  late int _revision = widget.result.revision;
  @override
  void initState() {
    super.initState();
    _horizontal.addListener(_scroll);
  }

  void _scroll() {
    if (mounted) setState(() {});
  }

  @override
  void didUpdateWidget(DbResultGrid oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.result, widget.result)) {
      _selected.clear();
      _recordColumns.clear();
      _pinned.clear();
      _hidden.clear();
      _widths.clear();
      _auto.clear();
      _order = null;
      _sort = null;
    }
    if (_revision != widget.result.revision) {
      _revision = widget.result.revision;
      if (_filter.isNotEmpty || _sort != null) _scheduleFilter();
    }
  }

  void _scheduleFilter() {
    _filterTimer?.cancel();
    _filterTimer = Timer(const Duration(milliseconds: 250), () {
      if (!mounted) return;
      _applyFilter();
    });
  }

  void _applyFilter() {
    if (_filter.isEmpty && _sort == null) {
      setState(() => _order = null);
      return;
    }
    final rows = widget.result.rows;
    final ids = <int>[
      for (var i = 0; i < rows.length; i++)
        if (_filter.isEmpty || rows[i].any((c) => c.text.toLowerCase().contains(_filter))) i,
    ];
    final column = _sort;
    if (column != null) {
      ids.sort((a, b) {
        final compared = compareDbCells(rows[a][column], rows[b][column]);
        return compared == 0 ? a.compareTo(b) : compared * (_ascending ? 1 : -1);
      });
    }
    setState(() => _order = ids);
  }

  @override
  void dispose() {
    _filterTimer?.cancel();
    _horizontal.dispose();
    _vertical.dispose();
    super.dispose();
  }

  int get _count => _order?.length ?? widget.result.rows.length;
  int _row(int index) => _order?[index] ?? index;
  void _select(int row) {
    setState(() {
      if (!_selected.add(row)) _selected.remove(row);
    });
    widget.onSelection?.call(_selected.toList()..sort());
  }

  void _show(int row, int column) => showAppDialog<void>(
    context,
    secure: true,
    builder: (_) => DbValueDialog(column: widget.result.columns[column], cell: widget.result.rows[row][column]),
  );
  String _text(DbCell cell, DbColumn column) {
    if (column.sensitive && cell.kind != DbCellKind.nullValue) return '••••';
    if (cell.kind == DbCellKind.nullValue) return context.l10n.dbNull;
    if (cell.kind == DbCellKind.timestamp || cell.kind == DbCellKind.date) {
      final date = DateTime.tryParse(cell.text);
      if (date != null) {
        return cell.kind == DbCellKind.date
            ? DateFormat.yMd(Localizations.localeOf(context).toLanguageTag()).format(date)
            : DateFormat.yMd(Localizations.localeOf(context).toLanguageTag()).add_Hms().format(date.toLocal());
      }
    }
    final text = cell.text;
    return text.length > 300 ? '${text.substring(0, 300)}…' : text;
  }

  static double get _rowHeight => AppPlatform.isMobile ? 34 : 26;
  static const _headerHeight = 30.0;

  /// Column width from its header and the first rows, like the classic SQL
  /// clients; dragging the header edge overrides it.
  double _autoWidth(int column) {
    final rows = widget.result.rows, sample = math.min(rows.length, 50);
    final cached = _auto[column];
    if (cached != null && cached.$2 >= sample) return cached.$1;
    final info = widget.result.columns[column];
    final char = (GlassTokens.of(context).typography.callout.fontSize ?? 12) * .62;
    // Header: bold name, caption type, key icon and the menu arrow.
    var header = (info.name.length * 1.1 + info.typeName.length * .9) * char + (info.primaryKey ? 16 : 0) + 42;
    var cells = 0.0;
    for (var i = 0; i < sample; i++) {
      cells = math.max(cells, math.min(_text(rows[i][column], info).length, 44).toDouble());
    }
    header = math.min(header, 300);
    final width = math.max(header, cells * char + 20).clamp(64.0, 360.0);
    _auto[column] = (width, sample);
    return width;
  }

  Future<void> _cellMenu(int row, int column, Offset position) async {
    final l = context.l10n;
    final action = await showGlassMenu<String>(
      context: context,
      anchor: Rect.fromLTWH(position.dx, position.dy, 1, 1),
      entries: [
        GlassMenuItem(value: 'value', label: l.dbShowValue, icon: Icons.visibility_outlined),
        if (widget.onCopy != null) ...[
          GlassMenuItem(value: 'cell', label: l.dbCopyCell, icon: Icons.content_copy_rounded),
          GlassMenuItem(value: 'row', label: l.dbCopyRow, icon: Icons.table_rows_outlined),
        ],
        if (!widget.production && widget.onEdit != null)
          GlassMenuItem(value: 'edit', label: l.dbEditCell, icon: Icons.edit_outlined),
      ],
    );
    if (!mounted) return;
    switch (action) {
      case 'value':
        _show(row, column);
      case 'cell':
        widget.onCopy?.call(DbResultSelection(rows: [row], columns: [column]));
      case 'row':
        widget.onCopy?.call(DbResultSelection(rows: [row]));
      case 'edit':
        widget.onEdit?.call(row, column);
    }
  }

  BorderSide get _line => BorderSide(color: GlassTokens.of(context).surfaces.separator);

  Widget _cell(int row, int column, {double? width}) {
    final tokens = GlassTokens.of(context);
    final cell = widget.result.rows[row][column], info = widget.result.columns[column];
    final empty = cell.kind == DbCellKind.nullValue;
    return GestureDetector(
      key: ValueKey('db-cell-$row-$column'),
      behavior: HitTestBehavior.opaque,
      onTap: () => _select(row),
      onDoubleTap: () => _show(row, column),
      onLongPress: () => _show(row, column),
      onSecondaryTapUp: (details) => unawaited(_cellMenu(row, column, details.globalPosition)),
      child: Container(
        width: width,
        height: _rowHeight,
        alignment: cell.numeric ? Alignment.centerRight : Alignment.centerLeft,
        padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
        decoration: BoxDecoration(
          border: Border(right: _line, bottom: _line),
        ),
        child: Text(
          _text(cell, info),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: tokens.typography.callout.copyWith(
            color: empty ? tokens.palette.tertiary : tokens.palette.label,
            fontStyle: empty ? FontStyle.italic : FontStyle.normal,
            fontFeatures: const [FontFeature.tabularFigures()],
          ),
        ),
      ),
    );
  }

  /// Row-number gutter: click selects the row, like the cell itself.
  Widget _number(int row, double width) {
    final tokens = GlassTokens.of(context);
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () => _select(row),
      child: Container(
        width: width,
        height: _rowHeight,
        alignment: Alignment.centerRight,
        padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s6),
        decoration: BoxDecoration(
          color: tokens.surfaces.fillField,
          border: Border(right: _line, bottom: _line),
        ),
        child: Text(
          '${row + 1}', // l10n-ignore: Result row number.
          style: tokens.typography.caption.copyWith(
            color: _selected.contains(row) ? tokens.palette.accent : tokens.palette.tertiary,
            fontFeatures: const [FontFeature.tabularFigures()],
          ),
        ),
      ),
    );
  }

  List<GlassMenuEntry<String>> _columnMenu(int column) {
    final l = context.l10n;
    return [
      GlassMenuItem(value: 'asc', label: '↑ ${widget.result.columns[column].name}', icon: Icons.arrow_upward_rounded),
      GlassMenuItem(
        value: 'desc',
        label: '↓ ${widget.result.columns[column].name}',
        icon: Icons.arrow_downward_rounded,
      ),
      if (_pinned.contains(column) || _pinned.length < 3)
        GlassMenuItem(
          value: 'pin',
          label: _pinned.contains(column) ? l.dbUnpinColumn : l.dbPinColumn,
          icon: Icons.push_pin_outlined,
        ),
      GlassMenuItem(value: 'hide', label: l.commonHide, icon: Icons.visibility_off_outlined),
    ];
  }

  void _columnAction(int column, String value) {
    if (value == 'asc' || value == 'desc') {
      _sort = column;
      _ascending = value == 'asc';
      _applyFilter();
      return;
    }
    setState(() {
      if (value == 'pin' && !_pinned.add(column)) _pinned.remove(column);
      if (value == 'hide') {
        _hidden.add(column);
        _pinned.remove(column);
      }
    });
  }

  Widget _header(int column, double width) {
    final tokens = GlassTokens.of(context), info = widget.result.columns[column];
    return Container(
      width: width,
      height: _headerHeight,
      decoration: BoxDecoration(
        color: tokens.surfaces.fillField,
        border: Border(right: _line, bottom: _line),
      ),
      child: Stack(
        children: [
          Positioned.fill(
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: () {
                if (_sort == column) {
                  _ascending = !_ascending;
                } else {
                  _sort = column;
                  _ascending = true;
                }
                _applyFilter();
              },
              onSecondaryTapUp: (details) async {
                final value = await showGlassMenu<String>(
                  context: context,
                  anchor: Rect.fromLTWH(details.globalPosition.dx, details.globalPosition.dy, 1, 1),
                  entries: _columnMenu(column),
                );
                if (value != null && mounted) _columnAction(column, value);
              },
              child: Padding(
                padding: const EdgeInsets.only(left: GlassSpacing.s8, right: 2),
                child: Row(
                  children: [
                    if (info.primaryKey) ...[
                      Icon(Icons.key_rounded, size: 12, color: tokens.palette.warning),
                      const SizedBox(width: 4),
                    ],
                    Flexible(
                      flex: 4,
                      child: Text(
                        info.name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: tokens.typography.callout.copyWith(
                          color: tokens.palette.label,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ),
                    if (info.typeName.isNotEmpty) ...[
                      const SizedBox(width: 5),
                      Flexible(
                        child: Text(
                          info.typeName,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: tokens.typography.caption.copyWith(color: tokens.palette.tertiary),
                        ),
                      ),
                    ],
                    const Spacer(),
                    if (_sort == column)
                      Icon(
                        _ascending ? Icons.arrow_upward_rounded : Icons.arrow_downward_rounded,
                        size: 12,
                        color: tokens.palette.accent,
                      ),
                    GlassMenuButton<String>(
                      entries: _columnMenu(column),
                      onSelected: (value) => _columnAction(column, value),
                      builder: (context, open) => GlassIconButton(
                        icon: Icons.arrow_drop_down_rounded,
                        tooltip: context.l10n.dbColumns,
                        size: 20,
                        iconSize: 15,
                        style: GlassIconButtonStyle.plain,
                        onPressed: open,
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
          Positioned(
            right: 0,
            top: 0,
            bottom: 0,
            width: 6,
            child: MouseRegion(
              cursor: SystemMouseCursors.resizeColumn,
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onHorizontalDragUpdate: (d) => setState(() => _widths[column] = (width + d.delta.dx).clamp(56, 800)),
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _toolbar(bool records, double maxWidth) {
    final l = context.l10n;
    Widget tool(IconData icon, String tooltip, VoidCallback? onPressed, {Key? key}) => GlassIconButton(
      key: key,
      icon: icon,
      tooltip: tooltip,
      size: 28,
      iconSize: 16,
      style: GlassIconButtonStyle.plain,
      onPressed: onPressed,
    );
    return SizedBox(
      height: 38,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s6),
        child: Row(
          children: [
            Expanded(
              child: SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(children: widget.leading),
              ),
            ),
            Flexible(
              child: ConstrainedBox(
                constraints: BoxConstraints(maxWidth: maxWidth < 500 ? 160 : 220),
                child: GlassField(
                  key: const ValueKey('db-result-filter'),
                  search: true,
                  placeholder: l.commonSearch,
                  onChanged: (v) {
                    _filter = v.toLowerCase();
                    _scheduleFilter();
                  },
                ),
              ),
            ),
            const SizedBox(width: GlassSpacing.s4),
            tool(
              records ? Icons.grid_on_rounded : Icons.view_agenda_outlined,
              records ? l.dbGrid : l.dbRecords,
              () => setState(() => _records = !records),
              key: const ValueKey('db-result-mode'),
            ),
            GlassMenuButton<int>(
              entries: [
                for (var i = 0; i < math.min(widget.result.columns.length, 300); i++)
                  GlassMenuItem(value: i, label: widget.result.columns[i].name, checked: !_hidden.contains(i)),
              ],
              onSelected: (i) => setState(() {
                if (!_hidden.remove(i)) _hidden.add(i);
              }),
              builder: (context, open) => tool(Icons.view_column_outlined, l.dbColumns, open),
            ),
            if (widget.onCopy != null)
              tool(
                Icons.content_copy_rounded,
                _selected.isEmpty ? l.dbCopyResult : l.dbCopySelection,
                () => widget.onCopy!(DbResultSelection(rows: _selected.isEmpty ? null : (_selected.toList()..sort()))),
              ),
            ...widget.trailing,
          ],
        ),
      ),
    );
  }

  Widget _record(int row, List<int> columns) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final selected = _selected.contains(row);
    return Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
      child: ContentSurface(
        padding: EdgeInsets.zero,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            GlassInteractive(
              selected: selected,
              onPressed: () => _select(row),
              builder: (context, state) => Padding(
                padding: const EdgeInsets.fromLTRB(
                  GlassSpacing.s12,
                  GlassSpacing.s8,
                  GlassSpacing.s12,
                  GlassSpacing.s8,
                ),
                child: Row(
                  children: [
                    Icon(
                      selected ? Icons.check_box_rounded : Icons.check_box_outline_blank_rounded,
                      size: 18,
                      color: selected ? tokens.palette.accent : tokens.secondaryLabel,
                    ),
                    const SizedBox(width: GlassSpacing.s8),
                    Text(
                      '#${row + 1}', // l10n-ignore: Result row number.
                      style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                    ),
                  ],
                ),
              ),
            ),
            Container(height: 1, color: tokens.surfaces.separator),
            for (final column in columns.take(_recordColumns[row] ?? 16))
              GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: () => _show(row, column),
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(
                    GlassSpacing.s12,
                    GlassSpacing.s6,
                    GlassSpacing.s12,
                    GlassSpacing.s6,
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        widget.result.columns[column].name,
                        style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                      ),
                      Text(
                        _text(widget.result.rows[row][column], widget.result.columns[column]),
                        maxLines: 3,
                        overflow: TextOverflow.ellipsis,
                        style: tokens.typography.body.copyWith(color: tokens.palette.label),
                      ),
                    ],
                  ),
                ),
              ),
            if (columns.length > (_recordColumns[row] ?? 16))
              Align(
                alignment: Alignment.centerLeft,
                child: GlassButton.plain(
                  label: l.dbLoadMore,
                  onPressed: () => setState(() => _recordColumns[row] = (_recordColumns[row] ?? 16) + 16),
                ),
              ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    return LayoutBuilder(
      builder: (context, bounds) {
        final records =
            _records ?? (bounds.maxWidth < 700 && MediaQuery.orientationOf(context) == Orientation.portrait);
        final columns = List.generate(
          widget.result.columns.length,
          (i) => i,
        ).where((i) => !_hidden.contains(i)).toList();
        final pinned = columns.where(_pinned.contains).toList();
        final scrolling = columns.where((i) => !_pinned.contains(i)).toList();
        double width(int column) => _widths[column] ?? _autoWidth(column);
        double pinWidth(int column) =>
            width(column).clamp(56, math.max(56, bounds.maxWidth * .4 / (pinned.isEmpty ? 1 : pinned.length)));
        final numberWidth = (math.max(_count, 1).toString().length * 7.5 + 14).clamp(34.0, 90.0);
        final pinnedTotal = numberWidth + pinned.fold<double>(0, (sum, i) => sum + pinWidth(i));
        final available = (bounds.maxWidth - pinnedTotal).clamp(1, double.infinity);
        final offsets = <int, double>{};
        var total = 0.0;
        for (final column in scrolling) {
          offsets[column] = total;
          total += width(column);
        }
        final offset = _horizontal.hasClients ? _horizontal.offset : 0;
        final visible = scrolling
            .where((i) => offsets[i]! + width(i) >= offset && offsets[i]! <= offset + available)
            .toList();
        Color rowColor(int row, int index) => _selected.contains(row)
            ? tokens.palette.accent.withValues(alpha: .16)
            : index.isOdd
            ? tokens.surfaces.fillField.withValues(alpha: tokens.surfaces.fillField.a * .55)
            : Colors.transparent;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _toolbar(records, bounds.maxWidth),
            Container(height: 1, color: tokens.surfaces.separator),
            Expanded(
              child: records
                  ? ListView.builder(
                      key: const ValueKey('db-result-records'),
                      controller: _vertical,
                      padding: const EdgeInsets.all(GlassSpacing.s8),
                      itemCount: _count,
                      itemBuilder: (context, index) => _record(_row(index), columns),
                    )
                  : Row(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        SizedBox(
                          width: pinnedTotal,
                          child: Column(
                            children: [
                              Row(
                                children: [
                                  Container(
                                    width: numberWidth,
                                    height: _headerHeight,
                                    decoration: BoxDecoration(
                                      color: tokens.surfaces.fillField,
                                      border: Border(right: _line, bottom: _line),
                                    ),
                                  ),
                                  for (final column in pinned) _header(column, pinWidth(column)),
                                ],
                              ),
                              Expanded(
                                child: _PinnedRows(
                                  controller: _vertical,
                                  rowCount: _count,
                                  extent: _rowHeight,
                                  builder: (index) {
                                    final row = _row(index);
                                    return ColoredBox(
                                      color: rowColor(row, index),
                                      child: Row(
                                        children: [
                                          _number(row, numberWidth),
                                          for (final column in pinned) _cell(row, column, width: pinWidth(column)),
                                        ],
                                      ),
                                    );
                                  },
                                ),
                              ),
                            ],
                          ),
                        ),
                        Expanded(
                          child: SingleChildScrollView(
                            key: const ValueKey('db-result-horizontal'),
                            controller: _horizontal,
                            scrollDirection: Axis.horizontal,
                            child: SizedBox(
                              width: (total < available ? available : total).toDouble(),
                              child: Column(
                                children: [
                                  SizedBox(
                                    height: _headerHeight,
                                    child: Stack(
                                      children: [
                                        Positioned.fill(
                                          child: DecoratedBox(
                                            decoration: BoxDecoration(
                                              color: tokens.surfaces.fillField,
                                              border: Border(bottom: _line),
                                            ),
                                          ),
                                        ),
                                        for (final column in visible)
                                          Positioned(
                                            left: offsets[column],
                                            top: 0,
                                            width: width(column),
                                            child: _header(column, width(column)),
                                          ),
                                      ],
                                    ),
                                  ),
                                  Expanded(
                                    child: ListView.builder(
                                      key: const ValueKey('db-result-grid'),
                                      controller: _vertical,
                                      itemExtent: _rowHeight,
                                      itemCount: _count,
                                      itemBuilder: (context, index) {
                                        final row = _row(index);
                                        return ColoredBox(
                                          color: rowColor(row, index),
                                          child: Stack(
                                            children: [
                                              for (final column in visible)
                                                Positioned(
                                                  left: offsets[column],
                                                  width: width(column),
                                                  top: 0,
                                                  child: _cell(row, column),
                                                ),
                                            ],
                                          ),
                                        );
                                      },
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          ),
                        ),
                      ],
                    ),
            ),
            if (widget.result.hasMore)
              Align(
                alignment: Alignment.centerLeft,
                child: GlassButton.plain(
                  key: const ValueKey('db-result-more'),
                  label: l.dbLoadMore,
                  onPressed: widget.loadingMore ? null : widget.onMore,
                ),
              ),
          ],
        );
      },
    );
  }
}

/// Pinned cells follow the one authoritative vertical viewport without sharing
/// a ScrollPosition (which would break offset reads and wheel/drag scrolling).
class _PinnedRows extends StatefulWidget {
  const _PinnedRows({required this.controller, required this.rowCount, required this.extent, required this.builder});
  final ScrollController controller;
  final int rowCount;
  final double extent;
  final Widget Function(int) builder;
  @override
  State<_PinnedRows> createState() => _PinnedRowsState();
}

class _PinnedRowsState extends State<_PinnedRows> {
  final _controller = ScrollController();
  bool _syncing = false;
  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_follow);
    _controller.addListener(_lead);
  }

  void _follow() => _sync(widget.controller, _controller);
  void _lead() => _sync(_controller, widget.controller);
  void _sync(ScrollController source, ScrollController target) {
    if (_syncing || !source.hasClients || !target.hasClients) return;
    _syncing = true;
    target.jumpTo(source.offset.clamp(0, target.position.maxScrollExtent));
    _syncing = false;
  }

  @override
  void dispose() {
    widget.controller.removeListener(_follow);
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _follow();
    });
    return ListView.builder(
      controller: _controller,
      itemExtent: widget.extent,
      itemCount: widget.rowCount,
      itemBuilder: (_, i) => widget.builder(i),
    );
  }
}
