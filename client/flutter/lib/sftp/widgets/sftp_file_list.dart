import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sftp/file_kind.dart';
import 'package:consolecrypt/sftp/sftp_actions.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/sftp_rows.dart';
import 'package:consolecrypt/sftp/widgets/sftp_drag.dart';
import 'package:consolecrypt/sftp/widgets/sftp_drop_target.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

const sftpRowHeight = 26.0;
const _headerHeight = 28.0;
const _indent = 16.0;

/// The remote file list (content surface): sortable / resizable /
/// reorderable columns, inline folder tree, multi-select, keyboard
/// navigation, type-to-select, inline rename, context menu and drag & drop.
class SftpFileList extends ConsumerStatefulWidget {
  const SftpFileList({required this.focusNode, super.key});

  final FocusNode focusNode;

  @override
  ConsumerState<SftpFileList> createState() => _SftpFileListState();
}

class _SftpFileListState extends ConsumerState<SftpFileList> {
  final ScrollController _scroll = ScrollController();
  String _typeBuffer = '';
  DateTime _typeAt = DateTime.fromMillisecondsSinceEpoch(0);

  /// Folder under the pointer while files are dragged in from the OS.
  String? _osDropFolder;
  bool _osDragging = false;

  SftpController get _controller => ref.read(sftpControllerProvider.notifier);

  @override
  void initState() {
    super.initState();
    widget.focusNode.addListener(_onFocus);
  }

  @override
  void dispose() {
    widget.focusNode.removeListener(_onFocus);
    _scroll.dispose();
    super.dispose();
  }

  void _onFocus() => setState(() {});

  List<SftpRow> get _rows => ref.read(sftpRowsProvider);

  List<String> get _order => [for (final r in _rows) r.path];

  // Pointer ----------------------------------------------------------------------------

  void _onPointerDown(PointerDownEvent event, SftpRow row) {
    if (event.buttons != kPrimaryMouseButton && event.kind == PointerDeviceKind.mouse) return;
    widget.focusNode.requestFocus();
    final keys = HardwareKeyboard.instance;
    if (keys.isShiftPressed) {
      _controller.selectRange(row.path, _order);
    } else if (AppPlatform.usesMeta ? keys.isMetaPressed : keys.isControlPressed) {
      _controller.toggleSelection(row.path);
    } else if (!ref.read(sftpControllerProvider).selection.contains(row.path)) {
      _controller.selectOnly(row.path);
    }
  }

  void _onPointerUp(SftpRow row) {
    // A plain click on a row of a multi-selection selects just that row
    // (on up, so dragging the whole selection still works).
    final keys = HardwareKeyboard.instance;
    final state = ref.read(sftpControllerProvider);
    if (!keys.isShiftPressed &&
        !(AppPlatform.usesMeta ? keys.isMetaPressed : keys.isControlPressed) &&
        state.selection.length > 1 &&
        state.selection.contains(row.path)) {
      _controller.selectOnly(row.path);
    }
  }

  /// Glass context menu at the pointer for the row under it (selecting it
  /// first) or, on the empty area, for the current folder.
  Future<void> _openContextMenu(Offset globalPosition, {SftpRow? row}) async {
    final state = ref.read(sftpControllerProvider);
    if (row == null) {
      _controller.clearSelection();
    } else if (!state.selection.contains(row.path)) {
      _controller.selectOnly(row.path);
    }
    final actions = SftpActions(context, ref);
    final sel = ref.read(sftpControllerProvider).selectedEntries;
    final choice = await showGlassMenu<SftpAction>(
      context: context,
      anchor: Rect.fromLTWH(globalPosition.dx, globalPosition.dy, 0, 0),
      entries: actions.glassMenuEntries(sel),
    );
    if (mounted) actions.runChoice(choice, sel);
  }

  // Keyboard ---------------------------------------------------------------------------

  void _run(SftpAction action) {
    final sel = ref.read(sftpControllerProvider).selectedEntries;
    if (SftpActions.isEnabled(action, sel)) unawaited(SftpActions(context, ref).run(action, sel));
  }

  void _ensureVisible(int index) {
    if (!_scroll.hasClients) return;
    final pos = _scroll.position;
    final top = index * sftpRowHeight;
    final bottom = top + sftpRowHeight;
    if (top < pos.pixels) {
      _scroll.jumpTo(top);
    } else if (bottom > pos.pixels + pos.viewportDimension) {
      _scroll.jumpTo((bottom - pos.viewportDimension).clamp(0, pos.maxScrollExtent));
    }
  }

  void _moveCursor(int delta, {bool extend = false, bool absolute = false}) {
    final rows = _rows;
    if (rows.isEmpty) return;
    final state = ref.read(sftpControllerProvider);
    final current = rows.indexWhere((r) => r.path == state.cursor);
    final int next;
    if (absolute) {
      next = delta < 0 ? 0 : rows.length - 1;
    } else if (current < 0) {
      next = delta > 0 ? 0 : rows.length - 1;
    } else {
      next = (current + delta).clamp(0, rows.length - 1);
    }
    final path = rows[next].path;
    if (extend) {
      _controller.selectRange(path, _order);
    } else {
      _controller.selectOnly(path);
    }
    _ensureVisible(next);
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) return KeyEventResult.ignored;
    final state = ref.read(sftpControllerProvider);
    if (state.renaming != null) return KeyEventResult.ignored;
    final keys = HardwareKeyboard.instance;
    final primary = AppPlatform.usesMeta ? keys.isMetaPressed : keys.isControlPressed;
    final shift = keys.isShiftPressed;
    final key = event.logicalKey;
    final rows = _rows;
    final cursorRow = rows.where((r) => r.path == state.cursor).firstOrNull;

    bool handled(void Function() action) {
      action();
      return true;
    }

    final done = switch (key) {
      LogicalKeyboardKey.arrowUp when primary => handled(() => unawaited(_controller.goUp())),
      LogicalKeyboardKey.arrowDown when primary => handled(() => _run(SftpAction.open)),
      LogicalKeyboardKey.arrowUp => handled(() => _moveCursor(-1, extend: shift)),
      LogicalKeyboardKey.arrowDown => handled(() => _moveCursor(1, extend: shift)),
      LogicalKeyboardKey.home => handled(() => _moveCursor(-1, extend: shift, absolute: true)),
      LogicalKeyboardKey.end => handled(() => _moveCursor(1, extend: shift, absolute: true)),
      LogicalKeyboardKey.arrowRight when cursorRow != null && cursorRow.entry.isDirectory => handled(
        () => _controller.setExpanded(cursorRow.entry, expanded: true),
      ),
      LogicalKeyboardKey.arrowLeft when cursorRow != null => handled(() {
        if (cursorRow.expanded) {
          _controller.setExpanded(cursorRow.entry, expanded: false);
        } else if (cursorRow.depth > 0) {
          final parent = parentRemotePath(cursorRow.path);
          _controller.selectOnly(parent);
          _ensureVisible(_order.indexOf(parent));
        }
      }),
      LogicalKeyboardKey.enter || LogicalKeyboardKey.numpadEnter => handled(() => _run(SftpAction.open)),
      LogicalKeyboardKey.space when _typeBuffer.isEmpty || _typeExpired => handled(() => _run(SftpAction.quickLook)),
      LogicalKeyboardKey.delete || LogicalKeyboardKey.backspace => handled(() => _run(SftpAction.delete)),
      LogicalKeyboardKey.f2 => handled(() => _run(SftpAction.rename)),
      LogicalKeyboardKey.keyA when primary => handled(() => _controller.selectAll(_order)),
      LogicalKeyboardKey.keyI when primary => handled(() => _run(SftpAction.getInfo)),
      LogicalKeyboardKey.keyD when primary => handled(() => _run(SftpAction.duplicate)),
      LogicalKeyboardKey.keyR when primary => handled(() => _run(SftpAction.refresh)),
      LogicalKeyboardKey.keyN when primary && shift => handled(() => _run(SftpAction.newFolder)),
      LogicalKeyboardKey.escape when state.selection.isNotEmpty => handled(_controller.clearSelection),
      _ => false,
    };
    if (done) return KeyEventResult.handled;
    final char = event.character;
    if (!primary && !keys.isAltPressed && char != null && char.length == 1 && char.codeUnitAt(0) >= 32) {
      _typeToSelect(char);
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  bool get _typeExpired => DateTime.now().difference(_typeAt) > const Duration(milliseconds: 1000);

  void _typeToSelect(String char) {
    _typeBuffer = _typeExpired ? char : _typeBuffer + char;
    _typeAt = DateTime.now();
    final prefix = _typeBuffer.toLowerCase();
    final rows = _rows;
    final match = rows.indexWhere((r) => r.entry.name.toLowerCase().startsWith(prefix));
    if (match >= 0) {
      _controller.selectOnly(rows[match].path);
      _ensureVisible(match);
    }
  }

  // Drag & drop ------------------------------------------------------------------------

  String? _folderAt(Offset localPosition) {
    final offset = _scroll.hasClients ? _scroll.offset : 0.0;
    final index = ((localPosition.dy + offset) / sftpRowHeight).floor();
    final rows = _rows;
    if (index < 0 || index >= rows.length) return null;
    return rows[index].entry.isDirectory ? rows[index].path : null;
  }

  Future<void> _uploadInto(List<String> localPaths, String? folder) async {
    final target = folder ?? ref.read(sftpControllerProvider).remotePath;
    await uploadPaths(context, ref, localPaths, target);
  }

  Future<void> _moveInto(SftpDragData data, String folder) async {
    final l10n = context.l10n;
    final moved = await runWithFeedback(context, () => _controller.move(data.paths, folder));
    if (moved != null && moved > 0 && mounted) showSnack(context, l10n.sftpMoved(moved, folder));
  }

  // Inline rename ---------------------------------------------------------------------

  Future<void> _commitRename(String path, String name) async {
    final l10n = context.l10n;
    try {
      await _controller.commitRename(path, name);
    } on AppException catch (e) {
      if (!mounted) return;
      showSnack(context, e.code == AppErrorCode.validation ? l10n.sftpNameInvalid : errorMessage(l10n, e), error: true);
    }
    if (mounted) widget.focusNode.requestFocus();
  }

  void _cancelRename() {
    _controller.cancelRename();
    widget.focusNode.requestFocus();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final state = ref.watch(sftpControllerProvider);
    final rows = ref.watch(sftpRowsProvider);
    final focused = widget.focusNode.hasFocus;

    return LayoutBuilder(
      builder: (context, constraints) {
        final columns = fitColumns(state.prefs, constraints.maxWidth - 16);
        Widget body;
        if (state.isLoadingCurrent) {
          body = const Center(child: CircularProgressIndicator());
        } else if (rows.isEmpty) {
          body = Center(
            child: Text(
              state.filter.trim().isNotEmpty ? l10n.sftpNoMatches(state.filter.trim()) : l10n.sftpEmptyFolder,
              key: const ValueKey('sftp-list-empty'),
              textAlign: TextAlign.center,
              style: tokens.typography.body.copyWith(color: tokens.secondaryLabel),
            ),
          );
        } else {
          body = ListView.builder(
            controller: _scroll,
            itemExtent: sftpRowHeight,
            itemCount: rows.length,
            itemBuilder: (context, i) {
              final row = rows[i];
              final selected = state.selection.contains(row.path);
              final dragPaths = selected ? state.selection.toList() : [row.path];
              return _RowItem(
                key: ValueKey('sftp-row-${row.path}'),
                row: row,
                index: i,
                columns: columns,
                prefs: state.prefs,
                selected: selected,
                focused: focused,
                isCursor: state.cursor == row.path,
                renaming: state.renaming == row.path,
                osDropHighlight: _osDropFolder == row.path,
                dragPaths: dragPaths,
                onPointerDown: (e) => _onPointerDown(e, row),
                onPointerUp: () => _onPointerUp(row),
                onDoubleTap: () {
                  _controller.selectOnly(row.path);
                  _run(SftpAction.open);
                },
                onSecondaryTap: (global) => unawaited(_openContextMenu(global, row: row)),
                onToggle: () => _controller.toggleExpanded(row.entry),
                onRenameCommit: (name) => unawaited(_commitRename(row.path, name)),
                onRenameCancel: _cancelRename,
                onMoveInto: (data) => unawaited(_moveInto(data, row.path)),
                onUploadInto: (data) => unawaited(_uploadInto(data.paths, row.path)),
              );
            },
          );
        }

        final list = DragTarget<LocalDragData>(
          onWillAcceptWithDetails: (_) => true,
          onAcceptWithDetails: (d) => unawaited(_uploadInto(d.data.paths, null)),
          builder: (context, candidates, _) => SftpDropTarget(
            onHover: (p) => setState(() {
              _osDragging = true;
              _osDropFolder = _folderAt(p);
            }),
            onExit: () => setState(() {
              _osDragging = false;
              _osDropFolder = null;
            }),
            onDrop: (paths, p) => unawaited(_uploadInto(paths, _folderAt(p))),
            child: GestureDetector(
              behavior: HitTestBehavior.translucent,
              onSecondaryTapUp: (d) => unawaited(_openContextMenu(d.globalPosition)),
              onTapDown: (_) => widget.focusNode.requestFocus(),
              // Drop target (§4.10): accent border + accent α .06 fill.
              child: DecoratedBox(
                position: DecorationPosition.foreground,
                decoration: (candidates.isNotEmpty || (_osDragging && _osDropFolder == null))
                    ? BoxDecoration(
                        color: tokens.palette.accent.withValues(alpha: 0.06),
                        border: Border.all(color: tokens.palette.accent, width: 2),
                      )
                    : const BoxDecoration(),
                child: body,
              ),
            ),
          ),
        );

        // The pane card (sftp_screen) is the opaque content surface; the
        // pinned header uses the hard scroll edge (§3 rule 2).
        return Focus(
          focusNode: widget.focusNode,
          onKeyEvent: _onKey,
          child: ScrollEdgeEffect.hard(
            // Header cells use ink: give them a Material above the opaque band.
            header: Material(
              type: MaterialType.transparency,
              child: _Header(columns: columns, prefs: state.prefs),
            ),
            child: list,
          ),
        );
      },
    );
  }
}

// Header ---------------------------------------------------------------------------------

String columnLabel(AppLocalizations l, SftpListColumn c) => switch (c) {
  SftpListColumn.name => l.sftpColumnName,
  SftpListColumn.size => l.sftpColumnSize,
  SftpListColumn.kind => l.sftpColumnKind,
  SftpListColumn.modified => l.sftpColumnModified,
  SftpListColumn.permissions => l.sftpColumnPermissions,
  SftpListColumn.owner => l.sftpColumnOwner,
  SftpListColumn.group => l.sftpColumnGroup,
};

enum _HeaderChoice { foldersFirst, showHidden, reset }

class _Header extends ConsumerWidget {
  const _Header({required this.columns, required this.prefs});

  final List<SftpListColumn> columns;
  final SftpBrowserPreferences prefs;

  /// Column/visibility menu (right-click on the header): glass menu with
  /// check marks at the pointer.
  Future<void> _menu(BuildContext context, WidgetRef ref, Offset global) async {
    final l10n = context.l10n;
    final controller = ref.read(sftpControllerProvider.notifier);
    final choice = await showGlassMenu<Object>(
      context: context,
      anchor: Rect.fromLTWH(global.dx, global.dy, 0, 0),
      entries: [
        for (final c in SftpListColumn.values.skip(1))
          GlassMenuItem<Object>(
            key: ValueKey('sftp-column-toggle-${c.wireName}'),
            value: c,
            label: columnLabel(l10n, c),
            checked: prefs.visibleColumns.contains(c),
          ),
        const GlassMenuDivider<Object>(),
        GlassMenuItem<Object>(
          key: const ValueKey('sftp-folders-first'),
          value: _HeaderChoice.foldersFirst,
          label: l10n.sftpFoldersFirst,
          checked: prefs.foldersFirst,
        ),
        GlassMenuItem<Object>(
          key: const ValueKey('sftp-show-hidden'),
          value: _HeaderChoice.showHidden,
          label: l10n.sftpShowHidden,
          checked: prefs.showHidden,
        ),
        const GlassMenuDivider<Object>(),
        GlassMenuItem<Object>(
          key: const ValueKey('sftp-reset-columns'),
          value: _HeaderChoice.reset,
          label: l10n.sftpResetColumns,
          // Keeps the label aligned with the checkable items above.
          checked: false,
        ),
      ],
    );
    switch (choice) {
      case final SftpListColumn c:
        controller.toggleColumn(c);
      case _HeaderChoice.foldersFirst:
        controller.toggleFoldersFirst();
      case _HeaderChoice.showHidden:
        controller.toggleShowHidden();
      case _HeaderChoice.reset:
        controller.resetColumns();
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // The opaque band + separator come from ScrollEdgeEffect.hard.
    return GestureDetector(
      onSecondaryTapUp: (d) => unawaited(_menu(context, ref, d.globalPosition)),
      child: SizedBox(
        height: _headerHeight,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
          child: Row(
            children: [
              for (final c in columns)
                if (c == SftpListColumn.name)
                  Expanded(
                    child: _HeaderCell(column: c, prefs: prefs),
                  )
                else
                  SizedBox(
                    width: prefs.widthOf(c),
                    child: _HeaderCell(column: c, prefs: prefs),
                  ),
            ],
          ),
        ),
      ),
    );
  }
}

class _HeaderCell extends ConsumerWidget {
  const _HeaderCell({required this.column, required this.prefs});

  final SftpListColumn column;
  final SftpBrowserPreferences prefs;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final controller = ref.read(sftpControllerProvider.notifier);
    final active = prefs.sortColumn == column;
    final labelStyle = tokens.typography.callout.copyWith(
      fontWeight: active ? FontWeight.w600 : FontWeight.w500,
      color: active ? tokens.palette.label : tokens.secondaryLabel,
    );
    final label = Row(
      children: [
        Flexible(
          child: Text(columnLabel(l10n, column), maxLines: 1, overflow: TextOverflow.ellipsis, style: labelStyle),
        ),
        if (active)
          Icon(
            prefs.sortAscending ? Icons.arrow_drop_up_rounded : Icons.arrow_drop_down_rounded,
            key: ValueKey('sftp-sort-${prefs.sortAscending ? 'asc' : 'desc'}'),
            size: GlassSizes.iconRow,
            color: tokens.palette.label,
          ),
      ],
    );
    Widget cell = InkWell(
      key: ValueKey('sftp-col-${column.wireName}'),
      onTap: () => controller.setSort(column),
      child: Padding(
        padding: const EdgeInsets.only(left: 8, right: 4),
        child: Align(alignment: AlignmentDirectional.centerStart, child: label),
      ),
    );
    if (column != SftpListColumn.name) {
      cell = Draggable<SftpListColumn>(
        data: column,
        axis: Axis.horizontal,
        feedback: ContentSurface(
          kind: ContentSurfaceKind.solid,
          radius: tokens.radii.sm,
          padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8, vertical: GlassSpacing.s4),
          child: Text(columnLabel(l10n, column), style: labelStyle.copyWith(color: tokens.palette.label)),
        ),
        child: cell,
      );
    }
    return DragTarget<SftpListColumn>(
      onWillAcceptWithDetails: (d) => d.data != column,
      onAcceptWithDetails: (d) => controller.moveColumn(d.data, column),
      builder: (context, candidates, _) => Stack(
        children: [
          Positioned.fill(
            child: DecoratedBox(
              decoration: BoxDecoration(
                border: BorderDirectional(
                  start: candidates.isNotEmpty
                      ? BorderSide(color: tokens.palette.accent, width: 2)
                      : BorderSide(color: tokens.surfaces.separator),
                ),
              ),
              child: cell,
            ),
          ),
          if (column != SftpListColumn.name)
            PositionedDirectional(
              start: 0,
              top: 0,
              bottom: 0,
              width: 6,
              child: MouseRegion(
                cursor: SystemMouseCursors.resizeColumn,
                child: GestureDetector(
                  key: ValueKey('sftp-col-resize-${column.wireName}'),
                  behavior: HitTestBehavior.opaque,
                  onHorizontalDragUpdate: (d) => controller.setColumnWidth(column, prefs.widthOf(column) - d.delta.dx),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

// Rows -----------------------------------------------------------------------------------

class _RowItem extends StatelessWidget {
  const _RowItem({
    required this.row,
    required this.index,
    required this.columns,
    required this.prefs,
    required this.selected,
    required this.focused,
    required this.isCursor,
    required this.renaming,
    required this.osDropHighlight,
    required this.dragPaths,
    required this.onPointerDown,
    required this.onPointerUp,
    required this.onDoubleTap,
    required this.onSecondaryTap,
    required this.onToggle,
    required this.onRenameCommit,
    required this.onRenameCancel,
    required this.onMoveInto,
    required this.onUploadInto,
    super.key,
  });

  final SftpRow row;
  final int index;
  final List<SftpListColumn> columns;
  final SftpBrowserPreferences prefs;
  final bool selected;
  final bool focused;
  final bool isCursor;
  final bool renaming;
  final bool osDropHighlight;
  final List<String> dragPaths;
  final ValueChanged<PointerDownEvent> onPointerDown;
  final VoidCallback onPointerUp;
  final VoidCallback onDoubleTap;
  final ValueChanged<Offset> onSecondaryTap;
  final VoidCallback onToggle;
  final ValueChanged<String> onRenameCommit;
  final VoidCallback onRenameCancel;
  final ValueChanged<SftpDragData> onMoveInto;
  final ValueChanged<LocalDragData> onUploadInto;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final scheme = theme.colorScheme;
    final l10n = context.l10n;
    final e = row.entry;
    final kind = fileKindOf(e);
    final activeSelection = selected && focused;
    final fg = activeSelection ? scheme.onPrimary : scheme.onSurface;
    final secondary = activeSelection ? scheme.onPrimary.withValues(alpha: 0.85) : scheme.onSurfaceVariant;
    final dim = e.isHidden ? 0.55 : 1.0;
    final base = theme.textTheme.bodyMedium?.copyWith(fontSize: 13, color: fg, height: 1.2);
    final small = base?.copyWith(color: secondary, fontSize: 12.5);
    final mono = small?.copyWith(
      fontFamily: AppPlatform.monospaceFamily,
      fontFamilyFallback: AppPlatform.monospaceFallback,
    );

    Widget cell(SftpListColumn c) => switch (c) {
      SftpListColumn.name => _NameCell(
        row: row,
        kind: kind,
        style: base,
        iconColor: activeSelection ? scheme.onPrimary : kind.color(scheme),
        dim: dim,
        renaming: renaming,
        onToggle: onToggle,
        onRenameCommit: onRenameCommit,
        onRenameCancel: onRenameCancel,
      ),
      SftpListColumn.size => Text(
        e.isDirectory ? '--' : formatBytes(l10n, e.size),
        textAlign: TextAlign.end,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: small,
      ),
      SftpListColumn.kind => Text(fileKindLabel(l10n, e), maxLines: 1, overflow: TextOverflow.ellipsis, style: small),
      SftpListColumn.modified => Text(
        e.modifiedAt == null ? '--' : formatDateTime(l10n, e.modifiedAt!),
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: small,
      ),
      SftpListColumn.permissions => Text(e.modeString, maxLines: 1, overflow: TextOverflow.clip, style: mono),
      SftpListColumn.owner => Text(
        e.owner ?? '${e.uid ?? '--'}',
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: small,
      ),
      SftpListColumn.group => Text(
        e.group ?? '${e.gid ?? '--'}',
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: small,
      ),
    };

    Widget content(bool dropHover) {
      final Color? background;
      if (dropHover || osDropHighlight) {
        background = scheme.primary.withValues(alpha: 0.22);
      } else if (selected) {
        background = focused ? scheme.primary : scheme.onSurface.withValues(alpha: 0.12);
      } else {
        background = index.isOdd ? scheme.onSurface.withValues(alpha: 0.025) : null;
      }
      return DecoratedBox(
        decoration: BoxDecoration(
          color: background,
          border: isCursor && focused && !selected ? Border.all(color: scheme.primary.withValues(alpha: 0.6)) : null,
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8),
          child: Row(
            children: [
              for (final c in columns)
                if (c == SftpListColumn.name)
                  Expanded(child: cell(c))
                else
                  SizedBox(
                    width: prefs.widthOf(c),
                    child: Padding(padding: const EdgeInsets.symmetric(horizontal: 8), child: cell(c)),
                  ),
            ],
          ),
        ),
      );
    }

    Widget item = Listener(
      onPointerDown: onPointerDown,
      onPointerUp: (_) => onPointerUp(),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onDoubleTap: renaming ? null : onDoubleTap,
        onSecondaryTapUp: (d) => onSecondaryTap(d.globalPosition),
        child: e.isDirectory
            ? DragTarget<Object>(
                onWillAcceptWithDetails: (d) => switch (d.data) {
                  SftpDragData(:final paths) => !paths.contains(e.path),
                  LocalDragData() => true,
                  _ => false,
                },
                onAcceptWithDetails: (d) => switch (d.data) {
                  final SftpDragData data => onMoveInto(data),
                  final LocalDragData data => onUploadInto(data),
                  _ => null,
                },
                builder: (context, candidates, _) => content(candidates.isNotEmpty),
              )
            : content(false),
      ),
    );
    if (!renaming) {
      item = Draggable<SftpDragData>(
        data: SftpDragData(dragPaths),
        dragAnchorStrategy: pointerDragAnchorStrategy,
        feedback: DragCountChip(count: dragPaths.length, icon: kind.icon),
        child: item,
      );
    }
    return Semantics(selected: selected, label: e.name, child: item);
  }
}

class _NameCell extends StatelessWidget {
  const _NameCell({
    required this.row,
    required this.kind,
    required this.style,
    required this.iconColor,
    required this.dim,
    required this.renaming,
    required this.onToggle,
    required this.onRenameCommit,
    required this.onRenameCancel,
  });

  final SftpRow row;
  final FileKind kind;
  final TextStyle? style;
  final Color iconColor;
  final double dim;
  final bool renaming;
  final VoidCallback onToggle;
  final ValueChanged<String> onRenameCommit;
  final VoidCallback onRenameCancel;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final e = row.entry;
    final dangling = e.isSymlink && e.linkTargetKind == null;
    Widget icon = Icon(kind.icon, size: 17, color: iconColor);
    if (e.isSymlink) {
      icon = Tooltip(
        message: dangling ? l10n.sftpDanglingLink : l10n.sftpSymlinkTooltip(e.linkTarget ?? ''),
        child: Stack(
          clipBehavior: Clip.none,
          children: [
            icon,
            PositionedDirectional(
              start: -3,
              bottom: -3,
              child: DecoratedBox(
                decoration: BoxDecoration(color: Theme.of(context).colorScheme.surface, shape: BoxShape.circle),
                child: Icon(
                  dangling ? Icons.link_off : Icons.north_east,
                  key: ValueKey('sftp-symlink-${e.name}'),
                  size: 10,
                  color: dangling ? Theme.of(context).colorScheme.error : iconColor,
                ),
              ),
            ),
          ],
        ),
      );
    }
    return Padding(
      padding: EdgeInsetsDirectional.only(start: row.depth * _indent),
      child: Row(
        children: [
          SizedBox(
            width: 18,
            child: e.isDirectory
                ? GestureDetector(
                    key: ValueKey('sftp-disclosure-${e.path}'),
                    behavior: HitTestBehavior.opaque,
                    onTap: onToggle,
                    child: Semantics(
                      button: true,
                      label: row.expanded ? l10n.sftpCollapse : l10n.sftpExpand,
                      child: row.loading
                          ? const Padding(
                              padding: EdgeInsets.all(3),
                              child: CircularProgressIndicator(strokeWidth: 1.5),
                            )
                          : AnimatedRotation(
                              turns: row.expanded ? 0.25 : 0,
                              duration: const Duration(milliseconds: 120),
                              child: Icon(Icons.arrow_right, size: 18, color: style?.color),
                            ),
                    ),
                  )
                : null,
          ),
          const SizedBox(width: 2),
          Opacity(opacity: dim, child: icon),
          const SizedBox(width: 6),
          Expanded(
            child: renaming
                ? _RenameField(name: e.name, onCommit: onRenameCommit, onCancel: onRenameCancel)
                : Opacity(
                    opacity: dim,
                    child: Text(e.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: style),
                  ),
          ),
        ],
      ),
    );
  }
}

class _RenameField extends StatefulWidget {
  const _RenameField({required this.name, required this.onCommit, required this.onCancel});

  final String name;
  final ValueChanged<String> onCommit;
  final VoidCallback onCancel;

  @override
  State<_RenameField> createState() => _RenameFieldState();
}

class _RenameFieldState extends State<_RenameField> {
  late final TextEditingController _controller = TextEditingController(text: widget.name);
  final FocusNode _focus = FocusNode(debugLabel: 'sftp-rename');
  bool _done = false;

  @override
  void initState() {
    super.initState();
    final (stem, _) = splitFileName(widget.name);
    _controller.selection = TextSelection(baseOffset: 0, extentOffset: stem.length);
    _focus.addListener(() {
      if (!_focus.hasFocus) _commit();
    });
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _focus.requestFocus();
    });
  }

  @override
  void dispose() {
    _controller.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _commit() {
    if (_done) return;
    _done = true;
    widget.onCommit(_controller.text);
  }

  void _cancel() {
    if (_done) return;
    _done = true;
    widget.onCancel();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return CallbackShortcuts(
      bindings: {const SingleActivator(LogicalKeyboardKey.escape): _cancel},
      child: TextField(
        key: const ValueKey('sftp-rename-field'),
        controller: _controller,
        focusNode: _focus,
        // Keep the preselected filename stem when desktop focus arrives.
        selectAllOnFocus: false,
        autocorrect: false,
        enableSuggestions: false,
        style: theme.textTheme.bodyMedium?.copyWith(fontSize: 13, color: theme.colorScheme.onSurface),
        decoration: InputDecoration(
          isDense: true,
          filled: true,
          fillColor: theme.colorScheme.surface,
          contentPadding: const EdgeInsets.symmetric(horizontal: 4, vertical: 4),
          border: OutlineInputBorder(borderSide: BorderSide(color: theme.colorScheme.primary)),
        ),
        onSubmitted: (_) => _commit(),
      ),
    );
  }
}
