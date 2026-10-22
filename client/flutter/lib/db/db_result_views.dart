import 'dart:math' as math;

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_result_controller.dart';
import 'package:consolecrypt/db/db_result_edits.dart';
import 'package:consolecrypt/db/db_result_grid.dart';
import 'package:flutter/services.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

/// Rows the text view writes out; the rest is summarised in its last line.
const dbTextViewRows = 2000;

/// Widest column of the text view, in characters.
const dbTextViewWidth = 60;

/// How one cell reads in the text and record views: masked columns hide the
/// value, NULL is [nullText], dates use the locale and long text is cut.
String dbCellDisplay(
  DbCell? cell,
  DbColumn column, {
  required String nullText,
  required String placeholder,
  required String locale,
  int limit = 300,
}) {
  if (cell == null) return placeholder;
  if (column.sensitive && cell.kind != DbCellKind.nullValue) return '••••';
  if (cell.kind == DbCellKind.nullValue) return nullText;
  if (cell.kind == DbCellKind.timestamp || cell.kind == DbCellKind.date) {
    final date = DateTime.tryParse(cell.text);
    if (date != null) {
      return cell.kind == DbCellKind.date
          ? DateFormat.yMd(locale).format(date)
          : DateFormat.yMd(locale).add_Hms().format(date.toLocal());
    }
  }
  final text = cell.text.replaceAll(RegExp(r'[\r\n\t]+'), ' ');
  return text.length > limit ? '${text.substring(0, limit)}…' : text;
}

/// The result as an aligned plain-text table (pending values included),
/// at most [dbTextViewRows] rows.
String dbResultText(
  DbResultSet result,
  DbResultEdits? edits, {
  required String nullText,
  required String placeholder,
  required String locale,
  String Function(int more)? moreRows,
}) {
  final columns = result.columns;
  final total = result.rows.length + (edits?.added.length ?? 0);
  final shown = math.min(total, dbTextViewRows);
  String value(int row, int column) {
    final cell = edits != null
        ? edits.value(result, row, column)
        : (row < result.rows.length ? result.rows[row][column] : null);
    return dbCellDisplay(
      cell,
      columns[column],
      nullText: nullText,
      placeholder: placeholder,
      locale: locale,
      limit: dbTextViewWidth,
    );
  }

  final cells = [
    for (var row = 0; row < shown; row++) [for (var c = 0; c < columns.length; c++) value(row, c)],
  ];
  final widths = [
    for (var c = 0; c < columns.length; c++)
      math.min(
        dbTextViewWidth + 1,
        cells.fold<int>(columns[c].name.length, (width, row) => math.max(width, row[c].length)),
      ),
  ];
  bool numeric(int column) =>
      dbColumnKind(columns[column]).index <= DbCellKind.float.index &&
      dbColumnKind(columns[column]).index >= DbCellKind.intValue.index;
  String pad(String text, int column) =>
      numeric(column) && text != nullText ? text.padLeft(widths[column]) : text.padRight(widths[column]);
  final rule = [for (final width in widths) '-' * (width + 2)].join('+');
  final line = '+$rule+';
  final out = StringBuffer()
    ..writeln(line)
    ..writeln('| ${[for (var c = 0; c < columns.length; c++) columns[c].name.padRight(widths[c])].join(' | ')} |')
    ..writeln(line);
  for (final row in cells) {
    out.writeln('| ${[for (var c = 0; c < columns.length; c++) pad(row[c], c)].join(' | ')} |');
  }
  out.write(line);
  if (total > shown && moreRows != null) out.write('\n${moreRows(total - shown)}');
  return out.toString();
}

/// Plain-text table view of a result: monospace, selectable and copyable.
class DbResultTextView extends StatelessWidget {
  const DbResultTextView({required this.result, required this.onCopy, this.edits, super.key});
  final DbResultSet result;
  final DbResultEdits? edits;
  final ValueChanged<String> onCopy;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final text = dbResultText(
      result,
      edits,
      nullText: l.dbNullCell,
      placeholder: l.dbDefaultCell,
      locale: Localizations.localeOf(context).toLanguageTag(),
      moreRows: l.dbTextMoreRows,
    );
    return Stack(
      children: [
        Positioned.fill(
          child: Scrollbar(
            child: SingleChildScrollView(
              key: const ValueKey('db-result-text'),
              padding: const EdgeInsets.all(GlassSpacing.s8),
              child: SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: SelectableText(
                  text,
                  key: const ValueKey('db-result-text-body'),
                  style: tokens.typography.callout.copyWith(
                    fontFamily: 'monospace',
                    fontFamilyFallback: const ['SF Mono', 'Cascadia Mono', 'DejaVu Sans Mono'],
                    color: tokens.palette.label,
                    height: 1.35,
                  ),
                ),
              ),
            ),
          ),
        ),
        Positioned(
          top: GlassSpacing.s6,
          right: GlassSpacing.s12,
          child: GlassIconButton(
            key: const ValueKey('db-result-text-copy'),
            icon: Icons.content_copy_rounded,
            tooltip: l.commonCopy,
            size: 28,
            iconSize: 15,
            onPressed: () => onCopy(text),
          ),
        ),
      ],
    );
  }
}

/// One row as a form: field name, type and value. When editing is allowed
/// a click on a value edits it (Enter keeps it pending, Esc cancels).
class DbRecordView extends StatefulWidget {
  const DbRecordView({
    required this.result,
    required this.row,
    this.edits,
    this.onCellEdit,
    this.onResetCell,
    super.key,
  });
  final DbResultSet result;
  final DbResultEdits? edits;

  /// The shown row (loaded or new); null when the result has no rows.
  final int? row;
  final void Function(int row, int column, DbCell value)? onCellEdit;
  final void Function(int row, int column)? onResetCell;

  @override
  State<DbRecordView> createState() => _DbRecordViewState();
}

class _DbRecordViewState extends State<DbRecordView> {
  final _editor = TextEditingController();
  int? _editing;
  bool _null = false, _invalid = false;
  FocusNode _focus = FocusNode(debugLabel: 'db-record-editor');

  @override
  void didUpdateWidget(DbRecordView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.row != widget.row || !identical(oldWidget.result, widget.result)) _cancel();
  }

  @override
  void dispose() {
    _editor
      ..clear()
      ..dispose();
    _focus.dispose();
    super.dispose();
  }

  DbCell? _value(int row, int column) {
    final edits = widget.edits;
    if (edits != null) return edits.value(widget.result, row, column);
    return row < widget.result.rows.length ? widget.result.rows[row][column] : null;
  }

  void _start(int column) {
    final row = widget.row;
    if (row == null || widget.onCellEdit == null || (widget.edits?.isDeleted(row) ?? false)) return;
    final info = widget.result.columns[column], current = _value(row, column);
    final previous = _focus;
    _focus = FocusNode(debugLabel: 'db-record-editor');
    WidgetsBinding.instance.addPostFrameCallback((_) => previous.dispose());
    setState(() {
      _editing = column;
      _null = current?.kind == DbCellKind.nullValue;
      _invalid = false;
      _editor.text = info.sensitive || current == null || _null ? '' : current.text;
    });
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && _editing == column) _focus.requestFocus();
    });
  }

  void _cancel() {
    if (_editing == null) return;
    setState(() {
      _editing = null;
      _invalid = false;
      _editor.clear();
    });
  }

  void _commit() {
    final row = widget.row, column = _editing, onCellEdit = widget.onCellEdit;
    if (row == null || column == null || onCellEdit == null) return;
    final sample = row < widget.result.rows.length ? widget.result.rows[row][column] : null;
    final kind = dbColumnKind(widget.result.columns[column], sample: sample);
    final value = _null ? const DbCell.nullValue() : dbCellFromInput(kind, _editor.text);
    if (value == null) {
      setState(() => _invalid = true);
      return;
    }
    setState(() {
      _editing = null;
      _editor.clear();
    });
    onCellEdit(row, column, value);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final row = widget.row, result = widget.result;
    if (row == null) {
      return Center(
        child: Text(l.dbNoRows, style: tokens.typography.caption.copyWith(color: tokens.palette.tertiary)),
      );
    }
    final added = row >= result.rows.length, deleted = widget.edits?.isDeleted(row) ?? false;
    final missing = widget.edits?.missing(result, row) ?? const [];
    final locale = Localizations.localeOf(context).toLanguageTag();
    String placeholder(int column) {
      final meta = widget.edits?.columns?.where((c) => c.name == result.columns[column].name).firstOrNull;
      return meta != null && (meta.hasDefault || meta.generated) ? l.dbDefaultCell : l.dbNullCell;
    }

    return ListView(
      key: const ValueKey('db-result-record'),
      padding: const EdgeInsets.all(GlassSpacing.s8),
      children: [
        Padding(
          padding: const EdgeInsets.only(bottom: GlassSpacing.s6),
          child: Row(
            children: [
              Text(
                added ? l.dbNewRow : l.dbRecordNumber(row + 1, result.rows.length),
                key: const ValueKey('db-record-title'),
                style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
              ),
              const SizedBox(width: GlassSpacing.s8),
              if (added) GlassBadge(label: l.dbRowAddedHint, tone: GlassTone.success, dense: true),
              if (deleted) GlassBadge(label: l.dbRowDeletedHint, tone: GlassTone.danger, dense: true),
            ],
          ),
        ),
        for (var column = 0; column < result.columns.length; column++)
          _field(context, row, column, missing.contains(result.columns[column].name), deleted, locale, placeholder),
      ],
    );
  }

  Widget _field(
    BuildContext context,
    int row,
    int column,
    bool missing,
    bool deleted,
    String locale,
    String Function(int) placeholder,
  ) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final info = widget.result.columns[column], value = _value(row, column);
    final changed = widget.edits?.changed(widget.result, row, column) ?? false;
    final editable = widget.onCellEdit != null && !deleted;
    final empty = value == null || value.kind == DbCellKind.nullValue;
    final label = SizedBox(
      width: 180,
      child: Row(
        children: [
          if (info.primaryKey) ...[
            Icon(Icons.key_rounded, size: 12, color: tokens.palette.warning),
            const SizedBox(width: 3),
          ],
          DbTypeIcon(column: info),
          const SizedBox(width: 5),
          Flexible(
            child: Text(
              info.name,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel, fontWeight: FontWeight.w600),
            ),
          ),
        ],
      ),
    );
    final Widget body;
    if (_editing == column) {
      body = CallbackShortcuts(
        bindings: {const SingleActivator(LogicalKeyboardKey.escape): _cancel},
        child: Row(
          children: [
            Expanded(
              child: GlassField(
                key: ValueKey('db-record-editor-$column'),
                controller: _editor,
                focusNode: _focus,
                placeholder: _null ? l.dbNullCell : null,
                errorText: _invalid ? l.dbInvalidValue : null,
                onChanged: (text) {
                  if (_invalid || (_null && text.isNotEmpty)) {
                    setState(() {
                      _invalid = false;
                      if (text.isNotEmpty) _null = false;
                    });
                  }
                },
                onSubmitted: (_) => _commit(),
              ),
            ),
            if (info.nullable) ...[
              const SizedBox(width: GlassSpacing.s4),
              GlassButton(
                key: ValueKey('db-record-null-$column'),
                label: l.dbNull,
                size: GlassControlSize.sm,
                style: _null ? GlassButtonStyle.prominent : GlassButtonStyle.plain,
                onPressed: () => setState(() {
                  _null = !_null;
                  _invalid = false;
                  if (_null) _editor.clear();
                }),
              ),
            ],
            GlassIconButton(
              icon: Icons.check_rounded,
              tooltip: l.dbKeepValue,
              size: 28,
              iconSize: 15,
              style: GlassIconButtonStyle.plain,
              onPressed: _commit,
            ),
          ],
        ),
      );
    } else {
      body = GestureDetector(
        key: ValueKey('db-record-value-$column'),
        behavior: HitTestBehavior.opaque,
        onTap: editable ? () => _start(column) : null,
        child: Container(
          constraints: const BoxConstraints(minHeight: 26),
          alignment: Alignment.centerLeft,
          padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s6, vertical: 3),
          decoration: BoxDecoration(
            color: changed ? tokens.palette.warning.withValues(alpha: .16) : tokens.surfaces.fillField,
            border: missing ? Border.all(color: tokens.palette.danger.withValues(alpha: .7)) : null,
            borderRadius: BorderRadius.circular(6),
          ),
          child: Text(
            dbCellDisplay(
              value,
              info,
              nullText: l.dbNullCell,
              placeholder: placeholder(column),
              locale: locale,
              limit: 2000,
            ),
            maxLines: 6,
            overflow: TextOverflow.ellipsis,
            style: tokens.typography.callout.copyWith(
              color: empty ? tokens.palette.tertiary : tokens.palette.label,
              fontStyle: empty ? FontStyle.italic : FontStyle.normal,
              decoration: deleted ? TextDecoration.lineThrough : null,
            ),
          ),
        ),
      );
    }
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(padding: const EdgeInsets.only(top: 5), child: label),
          const SizedBox(width: GlassSpacing.s8),
          Expanded(
            child: switch (missing
                ? l.dbRequiredValue
                : editable && _editing != column
                ? l.dbRecordEditHint
                : null) {
              final String tip => Tooltip(message: tip, child: body),
              null => body,
            },
          ),
          if (row >= widget.result.rows.length && value != null && widget.onResetCell != null && _editing != column)
            GlassIconButton(
              key: ValueKey('db-record-reset-$column'),
              icon: Icons.undo_rounded,
              tooltip: l.dbUseDefault,
              size: 26,
              iconSize: 14,
              style: GlassIconButtonStyle.plain,
              onPressed: () => widget.onResetCell!(row, column),
            ),
        ],
      ),
    );
  }
}
