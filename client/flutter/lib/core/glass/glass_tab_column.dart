part of 'glass_tab_strip.dart';

/// Terminal tabs as a column at the left edge of the terminal (the vertical
/// form of [GlassTabStrip]): a static `glass.thin` panel with one row per tab
/// (status dot, title, subtitle, close) and "+" with the fold toggle at the
/// bottom. The 8 px gap on its right edge resizes the column; dragging it far
/// to the left, a double click on it, or the toggle fold the column to
/// numbered icons with the status dot and the title in a tooltip.
///
/// Keyboard: ↑/↓ and Home/End move the focus between tabs, Enter/Space switch
/// to the focused tab, Shift+F10 or the Menu key open its context menu.
class GlassTabColumn extends StatefulWidget {
  const GlassTabColumn({
    required this.tabs,
    required this.activeIndex,
    required this.onSelect,
    super.key,
    this.width = defaultWidth,
    this.minWidth = defaultMinWidth,
    this.maxWidth = defaultMaxWidth,
    this.collapsed = false,
    this.onClose,
    this.onContextMenu,
    this.onAdd,
    this.onResize,
    this.onCollapsedChanged,
    this.onReorder,
  });

  static const double defaultWidth = 232;
  static const double defaultMinWidth = 160;
  static const double defaultMaxWidth = 420;

  /// Panel width when folded to icons.
  static const double collapsedWidth = 48;

  /// The resize gap right of the panel (part of the widget's width).
  static const double handleWidth = GlassSpacing.s8;

  final List<GlassTab> tabs;
  final int? activeIndex;
  final ValueChanged<int> onSelect;
  final ValueChanged<int>? onClose;

  /// Secondary click / long press / keyboard menu on a tab, with the global
  /// anchor position.
  final void Function(int index, Offset position)? onContextMenu;
  final VoidCallback? onAdd;

  /// Moves the tab at `from` to `to`: drag and drop, or Alt+↑/↓ (desktop).
  final void Function(int from, int to)? onReorder;

  /// Panel width when expanded, kept within [minWidth]–[maxWidth]. A
  /// [maxWidth] below [minWidth] (no room beside the terminal) shows the
  /// column folded without a way to expand it.
  final double width;
  final double minWidth;
  final double maxWidth;
  final bool collapsed;

  /// The new expanded width after a drag or an accessibility increment.
  final ValueChanged<double>? onResize;

  /// Folded or expanded by the toggle, a drag or a double click on the edge.
  final ValueChanged<bool>? onCollapsedChanged;

  @override
  State<GlassTabColumn> createState() => _GlassTabColumnState();
}

typedef _ColumnLayout = ({double width, bool collapsed});

class _GlassTabColumnState extends State<GlassTabColumn> {
  static const _step = 20.0;

  final _scroll = ScrollController();
  final _rowKeys = <GlobalKey>[];
  final _focusNodes = <FocusNode>[];

  /// Panel width under the pointer while the edge is dragged.
  double? _dragWidth;

  /// The last change, shown until the owner passes the new values back.
  _ColumnLayout? _local;
  bool _handleHovered = false;

  bool get _noRoom => widget.maxWidth < widget.minWidth;

  double get _maxWidth => _noRoom ? widget.minWidth : widget.maxWidth;

  double get _expandedWidth => (_local?.width ?? widget.width).clamp(widget.minWidth, _maxWidth);

  bool get _collapsed => _noRoom || (_local?.collapsed ?? widget.collapsed);

  @override
  void initState() {
    super.initState();
    _syncChildren();
    _reveal(widget.activeIndex);
  }

  @override
  void didUpdateWidget(GlassTabColumn oldWidget) {
    super.didUpdateWidget(oldWidget);
    _syncChildren();
    if (oldWidget.width != widget.width || oldWidget.collapsed != widget.collapsed) _local = null;
    if (oldWidget.activeIndex != widget.activeIndex || oldWidget.tabs.length != widget.tabs.length) {
      _reveal(widget.activeIndex);
    }
  }

  void _syncChildren() {
    while (_rowKeys.length < widget.tabs.length) {
      _rowKeys.add(GlobalKey());
      _focusNodes.add(FocusNode());
    }
    while (_rowKeys.length > widget.tabs.length) {
      _rowKeys.removeLast();
      // The row using it unmounts in this frame.
      final node = _focusNodes.removeLast();
      WidgetsBinding.instance.addPostFrameCallback((_) => node.dispose());
    }
  }

  @override
  void dispose() {
    _scroll.dispose();
    for (final node in _focusNodes) {
      node.dispose();
    }
    super.dispose();
  }

  /// Scrolls the row into view (only as far as needed).
  void _reveal(int? index) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || index == null || index < 0 || index >= _rowKeys.length) return;
      final row = _rowKeys[index].currentContext;
      if (row == null || !row.mounted) return;
      unawaited(Scrollable.ensureVisible(row, alignmentPolicy: ScrollPositionAlignmentPolicy.keepVisibleAtEnd));
      unawaited(Scrollable.ensureVisible(row, alignmentPolicy: ScrollPositionAlignmentPolicy.keepVisibleAtStart));
    });
  }

  void _focus(int index) {
    if (widget.tabs.isEmpty) return;
    final i = index.clamp(0, widget.tabs.length - 1);
    _focusNodes[i].requestFocus();
    _reveal(i);
  }

  void _menuFor(int index) {
    final box = _rowKeys[index].currentContext?.findRenderObject();
    if (box is! RenderBox || !box.hasSize) return;
    widget.onContextMenu?.call(index, box.localToGlobal(box.size.bottomCenter(Offset.zero)));
  }

  /// Dragging below the middle between the folded and the minimum width folds.
  _ColumnLayout _preview(double width) => width < (GlassTabColumn.collapsedWidth + widget.minWidth) / 2
      ? (width: _expandedWidth, collapsed: true)
      : (width: width.clamp(widget.minWidth, _maxWidth), collapsed: false);

  void _commit(_ColumnLayout next) {
    final width = _expandedWidth;
    final collapsed = _collapsed;
    setState(() => _local = next);
    if (!next.collapsed && next.width != width) widget.onResize?.call(next.width);
    if (next.collapsed != collapsed) widget.onCollapsedChanged?.call(next.collapsed);
  }

  void _toggle() => _commit((width: _expandedWidth, collapsed: !_collapsed));

  void _dragStart(DragStartDetails _) =>
      setState(() => _dragWidth = _collapsed ? GlassTabColumn.collapsedWidth : _expandedWidth);

  void _dragUpdate(DragUpdateDetails details) {
    final width = _dragWidth;
    if (width != null) setState(() => _dragWidth = width + details.delta.dx);
  }

  void _dragEnd(DragEndDetails _) {
    final width = _dragWidth;
    if (width == null) return;
    _dragWidth = null;
    _commit(_preview(width));
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final strings = GlassStrings.of(context);
    final view = switch (_dragWidth) {
      final width? => _preview(width),
      null => (width: _expandedWidth, collapsed: _collapsed),
    };
    final folded = view.collapsed;
    final twoLines = widget.tabs.any((tab) => tab.subtitle != null);
    final menu = widget.onContextMenu;
    final rows = [
      for (final (i, tab) in widget.tabs.indexed)
        KeyedSubtree(
          key: _rowKeys[i],
          child: GestureDetector(
            key: tab.key,
            behavior: HitTestBehavior.translucent,
            onSecondaryTapUp: menu == null ? null : (details) => menu(i, details.globalPosition),
            onLongPressStart: menu == null ? null : (details) => menu(i, details.globalPosition),
            child: CallbackShortcuts(
              bindings: {
                const SingleActivator(LogicalKeyboardKey.arrowDown): () => _focus(i + 1),
                const SingleActivator(LogicalKeyboardKey.arrowUp): () => _focus(i - 1),
                const SingleActivator(LogicalKeyboardKey.home): () => _focus(0),
                const SingleActivator(LogicalKeyboardKey.end): () => _focus(widget.tabs.length - 1),
                if (menu != null) ...{
                  const SingleActivator(LogicalKeyboardKey.f10, shift: true): () => _menuFor(i),
                  const SingleActivator(LogicalKeyboardKey.contextMenu): () => _menuFor(i),
                },
                if (widget.onReorder case final reorder?) ...{
                  const SingleActivator(LogicalKeyboardKey.arrowUp, alt: true): () {
                    if (i > 0) reorder(i, i - 1);
                  },
                  const SingleActivator(LogicalKeyboardKey.arrowDown, alt: true): () {
                    if (i < widget.tabs.length - 1) reorder(i, i + 1);
                  },
                },
              },
              child: GlassTabDrag(
                index: i,
                axis: Axis.vertical,
                onReorder: widget.onReorder,
                child: _ColumnTab(
                  tab: tab,
                  number: i + 1,
                  active: i == widget.activeIndex,
                  folded: folded,
                  twoLines: twoLines,
                  focusNode: _focusNodes[i],
                  onSelect: () => widget.onSelect(i),
                  onClose: widget.onClose == null ? null : () => widget.onClose!(i),
                ),
              ),
            ),
          ),
        ),
    ];
    final Widget? add = widget.onAdd == null
        ? null
        : GlassIconButton(
            key: const ValueKey('glass-tab-add'),
            icon: Icons.add_rounded,
            tooltip: strings.newTab,
            size: GlassSizes.tab,
            iconSize: GlassSizes.iconRow,
            onPressed: widget.onAdd,
          );
    final Widget? toggle = widget.onCollapsedChanged == null || _noRoom
        ? null
        : GlassIconButton(
            key: const ValueKey('glass-tab-column-toggle'),
            icon: folded ? Icons.keyboard_double_arrow_right_rounded : Icons.keyboard_double_arrow_left_rounded,
            tooltip: folded ? strings.tabColumnExpand : strings.tabColumnCollapse,
            style: GlassIconButtonStyle.plain,
            size: GlassSizes.tab,
            iconSize: GlassSizes.iconRow,
            onPressed: _toggle,
          );
    final footer = folded
        ? Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              ?add,
              if (add != null && toggle != null) const SizedBox(height: GlassSpacing.s4),
              ?toggle,
            ],
          )
        : Row(children: [?add, const Spacer(), ?toggle]);
    final panel = GlassSurface(
      key: const ValueKey('glass-tab-column'),
      variant: GlassVariant.thin,
      shape: GlassRadii.shape(tokens.radii.card),
      child: Semantics(
        container: true,
        explicitChildNodes: true,
        label: strings.openTerminalTabs(widget.tabs.length),
        child: Padding(
          padding: const EdgeInsets.all(GlassSpacing.s4),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Expanded(
                child: FocusTraversalGroup(
                  child: SingleChildScrollView(
                    key: const ValueKey('glass-tab-column-scroll'),
                    controller: _scroll,
                    child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: rows),
                  ),
                ),
              ),
              if (add != null || toggle != null)
                Padding(
                  padding: const EdgeInsets.only(top: GlassSpacing.s4),
                  child: footer,
                ),
            ],
          ),
        ),
      ),
    );
    final resizable = !_noRoom;
    final handle = Semantics(
      label: strings.tabColumnResize,
      onIncrease: resizable
          ? () => _commit((
              width: folded ? _expandedWidth : (_expandedWidth + _step).clamp(widget.minWidth, _maxWidth),
              collapsed: false,
            ))
          : null,
      onDecrease: resizable && !folded
          ? () => _commit(_preview(_expandedWidth - _step < widget.minWidth ? 0 : _expandedWidth - _step))
          : null,
      child: MouseRegion(
        cursor: resizable ? SystemMouseCursors.resizeColumn : MouseCursor.defer,
        onEnter: (_) => setState(() => _handleHovered = true),
        onExit: (_) => setState(() => _handleHovered = false),
        child: GestureDetector(
          key: const ValueKey('glass-tab-column-resize'),
          behavior: HitTestBehavior.opaque,
          // The edge follows the pointer from the press, slop included.
          dragStartBehavior: DragStartBehavior.down,
          onHorizontalDragStart: resizable ? _dragStart : null,
          onHorizontalDragUpdate: resizable ? _dragUpdate : null,
          onHorizontalDragEnd: resizable ? _dragEnd : null,
          onHorizontalDragCancel: resizable ? () => setState(() => _dragWidth = null) : null,
          onDoubleTap: resizable && widget.onCollapsedChanged != null ? _toggle : null,
          child: Center(
            child: Container(
              width: 2,
              height: 40,
              decoration: BoxDecoration(
                color: resizable && (_handleHovered || _dragWidth != null)
                    ? tokens.surfaces.separator
                    : const Color(0x00000000),
                borderRadius: BorderRadius.circular(2),
              ),
            ),
          ),
        ),
      ),
    );
    return SizedBox(
      width: (folded ? GlassTabColumn.collapsedWidth : view.width) + GlassTabColumn.handleWidth,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Expanded(child: panel),
          SizedBox(width: GlassTabColumn.handleWidth, child: handle),
        ],
      ),
    );
  }
}

/// One row of [GlassTabColumn]: status dot, title and subtitle, close; folded
/// to its number with the dot (the title moves into the tooltip).
class _ColumnTab extends StatelessWidget {
  const _ColumnTab({
    required this.tab,
    required this.number,
    required this.active,
    required this.folded,
    required this.twoLines,
    required this.focusNode,
    required this.onSelect,
    this.onClose,
  });

  final GlassTab tab;
  final int number;
  final bool active;
  final bool folded;
  final bool twoLines;
  final FocusNode focusNode;
  final VoidCallback onSelect;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final shape = GlassRadii.shape(tokens.radii.row);
    final teamBar = tab.teamColor == null
        ? null
        : Container(
            width: 3,
            height: 16,
            decoration: BoxDecoration(color: tab.teamColor, borderRadius: BorderRadius.circular(2)),
          );
    final title = Text(
      tab.title,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: tokens.typography.bodyEmph.copyWith(color: p.label),
    );
    final Widget content = folded
        ? Stack(
            children: [
              Center(
                child: Text('$number', style: tokens.typography.bodyEmph.copyWith(color: p.label)),
              ),
              PositionedDirectional(
                top: GlassSpacing.s6,
                end: GlassSpacing.s6,
                child: _StatusDot(state: tab.state, hostColor: tab.hostColor),
              ),
              if (teamBar != null) PositionedDirectional(start: 3, top: 10, child: teamBar),
            ],
          )
        : Padding(
            padding: const EdgeInsetsDirectional.only(start: GlassSpacing.s8, end: GlassSpacing.s4),
            child: Row(
              children: [
                if (teamBar != null) ...[teamBar, const SizedBox(width: GlassSpacing.s4)],
                _StatusDot(state: tab.state, hostColor: tab.hostColor),
                const SizedBox(width: GlassSpacing.s8),
                Expanded(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      title,
                      if (tab.subtitle != null)
                        Text(
                          tab.subtitle!,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                        ),
                    ],
                  ),
                ),
                if (onClose != null)
                  GlassIconButton(
                    icon: Icons.close_rounded,
                    tooltip: GlassStrings.of(context).closeTab,
                    style: GlassIconButtonStyle.plain,
                    size: 20,
                    iconSize: 14,
                    onPressed: onClose,
                  ),
              ],
            ),
          );
    Widget row = GlassInteractive(
      focusNode: focusNode,
      onPressed: onSelect,
      selected: active,
      minHitSize: 0,
      semanticLabel: [tab.title, ?tab.subtitle, ?tab.tooltip].join(', '),
      builder: (context, state) {
        final bg = active
            ? tokens.surfaces.segmentThumb
            : state.hovered
            ? tokens.surfaces.fillHover
            : const Color(0x00000000);
        return GlassFocusRing(
          visible: state.focusVisible,
          shape: shape,
          child: DecoratedBox(
            decoration: ShapeDecoration(
              color: bg,
              shape: shape,
              shadows: active ? const [BoxShadow(color: Color(0x1F000000), offset: Offset(0, 1), blurRadius: 3)] : null,
            ),
            child: SizedBox(
              width: double.infinity,
              height: folded ? 36 : (twoLines ? 44 : GlassSizes.sidebarRow),
              child: content,
            ),
          ),
        );
      },
    );
    // Folded rows carry the title in the tooltip; expanded rows the state.
    final tooltip = folded ? [tab.title, ?tab.subtitle, ?tab.tooltip].join('\n') : tab.tooltip;
    if (tooltip != null) row = Tooltip(message: tooltip, child: row);
    if (tab.attention && !active) row = _AttentionPulse(shape: shape, child: row);
    return Padding(padding: const EdgeInsets.symmetric(vertical: 1), child: row);
  }
}
