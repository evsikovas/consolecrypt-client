import 'dart:async';
import 'dart:math' as math;

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
    super.key,
  });
  final DbResultSet result;
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

  Widget _cell(int row, int column, {double? width}) {
    final cell = widget.result.rows[row][column], info = widget.result.columns[column];
    return SizedBox(
      width: width,
      height: 40,
      child: InkWell(
        key: ValueKey('db-cell-$row-$column'),
        onDoubleTap: () => _show(row, column),
        onLongPress: () => _show(row, column),
        onTap: () => _select(row),
        onSecondaryTapDown: (position) async {
          final overlay = Overlay.of(context).context.findRenderObject()! as RenderBox;
          final l = context.l10n;
          final action = await showMenu<String>(
            context: context,
            position: RelativeRect.fromRect(
              Rect.fromLTWH(position.globalPosition.dx, position.globalPosition.dy, 1, 1),
              Offset.zero & overlay.size,
            ),
            items: [
              PopupMenuItem(value: 'value', child: Text(l.dbShowValue)),
              if (widget.onCopy != null) ...[
                PopupMenuItem(value: 'cell', child: Text(l.dbCopyCell)),
                PopupMenuItem(value: 'row', child: Text(l.dbCopyRow)),
              ],
              if (!widget.production && widget.onEdit != null) PopupMenuItem(value: 'edit', child: Text(l.dbEditCell)),
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
        },
        child: Container(
          alignment: cell.numeric ? Alignment.centerRight : Alignment.centerLeft,
          padding: const EdgeInsets.symmetric(horizontal: 10),
          decoration: BoxDecoration(
            border: Border(right: BorderSide(color: Theme.of(context).dividerColor)),
          ),
          child: Text(
            _text(cell, info),
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(fontStyle: cell.kind == DbCellKind.nullValue ? FontStyle.italic : FontStyle.normal),
          ),
        ),
      ),
    );
  }

  Widget _header(int column, double width) {
    final l = context.l10n;
    return SizedBox(
      width: width,
      height: 44,
      child: Stack(
        children: [
          Positioned.fill(
            child: Row(
              children: [
                Expanded(
                  child: InkWell(
                    onTap: () {
                      if (_sort == column) {
                        _ascending = !_ascending;
                      } else {
                        _sort = column;
                        _ascending = true;
                      }
                      _applyFilter();
                    },
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 10),
                      child: Text(widget.result.columns[column].name, maxLines: 1, overflow: TextOverflow.ellipsis),
                    ),
                  ),
                ),
                if (_sort == column) Icon(_ascending ? Icons.arrow_upward : Icons.arrow_downward, size: 14),
                PopupMenuButton<String>(
                  iconSize: 16,
                  tooltip: l.dbColumns,
                  itemBuilder: (_) => [
                    if (_pinned.contains(column) || _pinned.length < 3)
                      PopupMenuItem(
                        value: 'pin',
                        child: Text(_pinned.contains(column) ? l.dbUnpinColumn : l.dbPinColumn),
                      ),
                    PopupMenuItem(value: 'hide', child: Text(l.commonHide)),
                  ],
                  onSelected: (value) => setState(() {
                    if (value == 'pin') {
                      if (!_pinned.add(column)) _pinned.remove(column);
                    }
                    if (value == 'hide') {
                      _hidden.add(column);
                      _pinned.remove(column);
                    }
                  }),
                ),
              ],
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
                onHorizontalDragUpdate: (d) => setState(() => _widths[column] = (width + d.delta.dx).clamp(80, 600)),
              ),
            ),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
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
        double width(int column) => _widths[column] ?? 180;
        double pinWidth(int column) =>
            width(column).clamp(80, math.max(80, bounds.maxWidth * .4 / (pinned.isEmpty ? 1 : pinned.length)));
        final pinnedTotal = pinned.fold<double>(0, (sum, i) => sum + pinWidth(i));
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
        return ContentSurface(
          kind: ContentSurfaceKind.solid,
          padding: EdgeInsets.zero,
          child: Column(
            children: [
              Padding(
                padding: const EdgeInsets.all(8),
                child: Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    if (widget.production) Text(l.dbProd, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                    SizedBox(
                      width: bounds.maxWidth < 500 ? bounds.maxWidth - 32 : 240,
                      child: TextField(
                        key: const ValueKey('db-result-filter'),
                        decoration: InputDecoration(labelText: l.commonSearch),
                        onChanged: (v) {
                          _filter = v.toLowerCase();
                          _scheduleFilter();
                        },
                      ),
                    ),
                    IconButton(
                      tooltip: records ? l.dbGrid : l.dbRecords,
                      icon: Icon(records ? Icons.table_chart : Icons.view_agenda),
                      onPressed: () => setState(() => _records = !records),
                    ),
                    PopupMenuButton<int>(
                      tooltip: l.dbColumns,
                      icon: const Icon(Icons.view_column),
                      itemBuilder: (_) => [
                        for (var i = 0; i < widget.result.columns.length; i++)
                          CheckedPopupMenuItem(
                            value: i,
                            checked: !_hidden.contains(i),
                            child: Text(widget.result.columns[i].name),
                          ),
                      ],
                      onSelected: (i) => setState(() {
                        if (!_hidden.remove(i)) _hidden.add(i);
                      }),
                    ),
                    if (widget.onCopy != null)
                      IconButton(
                        tooltip: _selected.isEmpty ? l.dbCopyResult : l.dbCopySelection,
                        icon: const Icon(Icons.copy),
                        onPressed: () => widget.onCopy!(
                          DbResultSelection(rows: _selected.isEmpty ? null : (_selected.toList()..sort())),
                        ),
                      ),
                  ],
                ),
              ),
              if (widget.result.truncated) Text(l.dbTruncated),
              Expanded(
                child: records
                    ? ListView.builder(
                        key: const ValueKey('db-result-records'),
                        controller: _vertical,
                        itemCount: _count,
                        itemBuilder: (context, index) {
                          final row = _row(index);
                          return Card(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.stretch,
                              children: [
                                CheckboxListTile(
                                  title: Text('${row + 1}'),
                                  value: _selected.contains(row),
                                  onChanged: (_) => _select(row),
                                ), // l10n-ignore: Result row number.
                                for (final column in columns.take(_recordColumns[row] ?? 16))
                                  ListTile(
                                    title: Text(widget.result.columns[column].name),
                                    subtitle: Text(
                                      _text(widget.result.rows[row][column], widget.result.columns[column]),
                                      maxLines: 3,
                                      overflow: TextOverflow.ellipsis,
                                    ),
                                    onTap: () => _show(row, column),
                                  ),
                                if (columns.length > (_recordColumns[row] ?? 16))
                                  TextButton(
                                    onPressed: () =>
                                        setState(() => _recordColumns[row] = (_recordColumns[row] ?? 16) + 16),
                                    child: Text(l.dbLoadMore),
                                  ),
                              ],
                            ),
                          );
                        },
                      )
                    : Row(
                        children: [
                          if (pinned.isNotEmpty)
                            SizedBox(
                              width: pinnedTotal,
                              child: Column(
                                children: [
                                  Row(children: [for (final column in pinned) _header(column, pinWidth(column))]),
                                  Expanded(
                                    child: _PinnedRows(
                                      controller: _vertical,
                                      rowCount: _count,
                                      builder: (index) {
                                        final row = _row(index);
                                        return Row(
                                          children: [
                                            for (final column in pinned) _cell(row, column, width: pinWidth(column)),
                                          ],
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
                                      height: 44,
                                      child: Stack(
                                        children: [
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
                                        itemExtent: 40,
                                        itemCount: _count,
                                        itemBuilder: (context, index) {
                                          final row = _row(index);
                                          return ColoredBox(
                                            color: _selected.contains(row)
                                                ? Theme.of(context).colorScheme.primary.withValues(alpha: .15)
                                                : Colors.transparent,
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
                TextButton(
                  key: const ValueKey('db-result-more'),
                  onPressed: widget.loadingMore ? null : widget.onMore,
                  child: Text(l.dbLoadMore),
                ),
            ],
          ),
        );
      },
    );
  }
}

/// Pinned cells follow the one authoritative vertical viewport without sharing
/// a ScrollPosition (which would break offset reads and wheel/drag scrolling).
class _PinnedRows extends StatefulWidget {
  const _PinnedRows({required this.controller, required this.rowCount, required this.builder});
  final ScrollController controller;
  final int rowCount;
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
      itemExtent: 40,
      itemCount: widget.rowCount,
      itemBuilder: (_, i) => widget.builder(i),
    );
  }
}
