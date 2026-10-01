import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_button.dart';
import 'package:consolecrypt/core/glass/glass_interaction.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:consolecrypt/core/glass/glass_strings.dart';
import 'package:consolecrypt/core/glass/glass_surface.dart';
import 'package:flutter/gestures.dart';
import 'package:material_ui/material_ui.dart';

/// Connection state shown by a tab's status dot.
enum GlassTabState {
  /// No session state: the dot shows the host colour (or nothing).
  none,

  /// Green dot.
  connected,

  /// Amber dot, pulsing (static under Reduce Motion).
  reconnecting,

  /// Red outline dot.
  disconnected,
}

/// One terminal tab.
@immutable
final class GlassTab {
  const GlassTab({
    required this.title,
    this.subtitle,
    this.hostColor,
    this.state = GlassTabState.none,
    this.tooltip,
    this.key,
  });

  /// Hostname (bodyEmph).
  final String title;

  /// User (secondary).
  final String? subtitle;
  final Color? hostColor;
  final GlassTabState state;

  /// Carries the connection state as text (colour is never the only cue).
  final String? tooltip;
  final Key? key;
}

/// Terminal tab strip (LIQUID_GLASS_SPEC §4.9): a 34-high row *above* the
/// terminal card, never over it. Static `glass.thin` capsule track; tabs are
/// 28-high capsules 120–220 wide; the active tab is a `surface.content`
/// thumb. Overflow shows scroll arrows and 16 px edge fades; "+" is a
/// 28 px glass circle.
///
/// TODO(client): drag-to-reorder with `spring.snappy` — needs the terminal
/// tabs controller API; next: add `onReorder` in phase 2 with the screen.
class GlassTabStrip extends StatelessWidget {
  const GlassTabStrip({
    required this.tabs,
    required this.activeIndex,
    required this.onSelect,
    super.key,
    this.onClose,
    this.onAdd,
    this.trailing,
  });

  final List<GlassTab> tabs;
  final int? activeIndex;
  final ValueChanged<int> onSelect;
  final ValueChanged<int>? onClose;
  final VoidCallback? onAdd;

  /// Session toolbar group in the same row (SFTP, snippets, split, more).
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        // A narrow desktop workspace must keep space for its tabs and count.
        // Keep the track subtree stable when the session actions move below it.
        final stackActions = trailing != null && constraints.maxWidth < 600;
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            SizedBox(
              height: AppPlatform.isMobile ? 54 : GlassSizes.tabTrack,
              child: Row(
                children: [
                  Expanded(
                    child: GlassSurface(
                      variant: GlassVariant.thin,
                      shape: const StadiumBorder(),
                      child: Padding(
                        padding: const EdgeInsets.all(3),
                        child: _EdgeFadeScroll(
                          activeIndex: activeIndex,
                          children: [
                            for (final (i, tab) in tabs.indexed)
                              _TabChip(
                                key: tab.key,
                                tab: tab,
                                active: i == activeIndex,
                                onSelect: () => onSelect(i),
                                onClose: onClose == null ? null : () => onClose!(i),
                              ),
                          ],
                        ),
                      ),
                    ),
                  ),
                  if (onAdd != null) ...[
                    const SizedBox(width: GlassSpacing.s6),
                    GlassIconButton(
                      key: const ValueKey('glass-tab-add'),
                      icon: Icons.add_rounded,
                      tooltip: GlassStrings.of(context).newTab,
                      size: GlassSizes.tab,
                      iconSize: GlassSizes.iconRow,
                      onPressed: onAdd,
                    ),
                  ],
                  if (trailing != null && !stackActions) ...[
                    const SizedBox(width: GlassSpacing.toolbarGroupGap),
                    trailing!,
                  ],
                ],
              ),
            ),
            if (stackActions) ...[
              const SizedBox(height: GlassSpacing.s6),
              Align(alignment: AlignmentDirectional.centerEnd, child: trailing!),
            ],
          ],
        );
      },
    );
  }
}

class _TabChip extends StatelessWidget {
  const _TabChip({required this.tab, required this.active, required this.onSelect, super.key, this.onClose});

  final GlassTab tab;
  final bool active;
  final VoidCallback onSelect;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    const shape = StadiumBorder();
    Widget chip = GlassInteractive(
      onPressed: onSelect,
      selected: active,
      minHitSize: 0,
      semanticLabel: [
        tab.title,
        if (tab.subtitle != null) tab.subtitle!,
        if (tab.tooltip != null) tab.tooltip!,
      ].join(', '),
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
            child: ConstrainedBox(
              constraints: const BoxConstraints(minWidth: GlassSizes.tabMinWidth, maxWidth: GlassSizes.tabMaxWidth),
              child: SizedBox(
                height: AppPlatform.isMobile ? 48 : GlassSizes.tab,
                child: Padding(
                  padding: const EdgeInsetsDirectional.only(start: GlassSpacing.s12, end: GlassSpacing.s4),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      _StatusDot(state: tab.state, hostColor: tab.hostColor),
                      const SizedBox(width: GlassSpacing.s6),
                      Flexible(
                        child: Text.rich(
                          TextSpan(
                            children: [
                              TextSpan(
                                text: tab.title,
                                style: tokens.typography.bodyEmph.copyWith(color: p.label),
                              ),
                              if (tab.subtitle != null)
                                TextSpan(
                                  text: '  ${tab.subtitle}',
                                  style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                                ),
                            ],
                          ),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
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
                        )
                      else
                        const SizedBox(width: GlassSpacing.s8),
                    ],
                  ),
                ),
              ),
            ),
          ),
        );
      },
    );
    if (tab.tooltip != null) chip = Tooltip(message: tab.tooltip, child: chip);
    return Padding(padding: const EdgeInsets.only(right: 2), child: chip);
  }
}

class _StatusDot extends StatefulWidget {
  const _StatusDot({required this.state, this.hostColor});

  final GlassTabState state;
  final Color? hostColor;

  @override
  State<_StatusDot> createState() => _StatusDotState();
}

class _StatusDotState extends State<_StatusDot> with SingleTickerProviderStateMixin {
  AnimationController? _pulse;

  bool get _animate => widget.state == GlassTabState.reconnecting && !GlassScope.of(context).appearance.reduceMotion;

  void _sync() {
    if (_animate) {
      final pulse = _pulse ??= AnimationController(vsync: this, duration: const Duration(milliseconds: 600));
      if (!pulse.isAnimating) pulse.repeat(reverse: true);
    } else {
      _pulse?.stop();
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(_StatusDot oldWidget) {
    super.didUpdateWidget(oldWidget);
    _sync();
  }

  @override
  void dispose() {
    _pulse?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final p = GlassTokens.of(context).palette;
    final (Color color, bool outline) = switch (widget.state) {
      GlassTabState.none => (widget.hostColor ?? p.tertiary, false),
      GlassTabState.connected => (p.success, false),
      GlassTabState.reconnecting => (p.warning, false),
      GlassTabState.disconnected => (p.danger, true),
    };
    final Widget dot = SizedBox.square(
      dimension: 8,
      child: DecoratedBox(
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          color: outline ? null : color,
          border: outline ? Border.all(color: color, width: 1.5) : null,
        ),
      ),
    );
    final pulse = _pulse;
    if (pulse != null && _animate) {
      return FadeTransition(opacity: Tween(begin: 1.0, end: 0.3).animate(pulse), child: dot);
    }
    return dot;
  }
}

/// Visible navigation for overflowing tabs, with mouse-wheel scrolling and
/// automatic reveal when a tab is opened or activated from the keyboard.
class _EdgeFadeScroll extends StatefulWidget {
  const _EdgeFadeScroll({required this.children, required this.activeIndex});

  final List<Widget> children;
  final int? activeIndex;

  @override
  State<_EdgeFadeScroll> createState() => _EdgeFadeScrollState();
}

class _EdgeFadeScrollState extends State<_EdgeFadeScroll> {
  final _scroll = ScrollController();
  final _tabKeys = <GlobalKey>[];
  double _availableWidth = 0;
  double _viewportWidth = 0;
  bool _overflow = false;
  bool _fadeStart = false;
  bool _fadeEnd = false;
  bool _revealScheduled = false;

  @override
  void initState() {
    super.initState();
    _syncKeys();
    _scheduleReveal();
  }

  @override
  void didUpdateWidget(_EdgeFadeScroll oldWidget) {
    super.didUpdateWidget(oldWidget);
    _syncKeys();
    if (oldWidget.activeIndex != widget.activeIndex || oldWidget.children.length != widget.children.length) {
      _scheduleReveal();
    }
  }

  void _syncKeys() {
    while (_tabKeys.length < widget.children.length) {
      _tabKeys.add(GlobalKey());
    }
    if (_tabKeys.length > widget.children.length) _tabKeys.removeRange(widget.children.length, _tabKeys.length);
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  Duration get _duration =>
      GlassScope.of(context).appearance.reduceMotion ? Duration.zero : const Duration(milliseconds: 180);

  void _scheduleReveal() {
    if (_revealScheduled) return;
    _revealScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _revealScheduled = false;
      if (!mounted || !_scroll.hasClients) return;
      final index = widget.activeIndex;
      if (index == null || index < 0 || index >= _tabKeys.length) return;
      final target = _tabKeys[index].currentContext?.findRenderObject();
      if (target != null) {
        unawaited(_scroll.position.ensureVisible(target, alignment: 0.5, duration: _duration));
      }
    });
  }

  void _page(int direction) {
    if (!_scroll.hasClients) return;
    final position = _scroll.position;
    final target = (position.pixels + direction * position.viewportDimension * 0.8).clamp(
      position.minScrollExtent,
      position.maxScrollExtent,
    );
    if (_duration == Duration.zero) {
      _scroll.jumpTo(target);
    } else {
      unawaited(_scroll.animateTo(target, duration: _duration, curve: Curves.easeOut));
    }
  }

  void _onPointerSignal(PointerSignalEvent event) {
    if (event is! PointerScrollEvent || !_scroll.hasClients || !_overflow) return;
    final delta = event.scrollDelta.dx != 0 ? event.scrollDelta.dx : event.scrollDelta.dy;
    final position = _scroll.position;
    final target = (position.pixels + delta).clamp(position.minScrollExtent, position.maxScrollExtent);
    if (target == position.pixels) return;
    GestureBinding.instance.pointerSignalResolver.register(event, (_) => _scroll.jumpTo(target));
  }

  bool _onMetrics(ScrollMetrics m) {
    if (m.axis != Axis.horizontal) return false;
    // Compare against the whole track, before reserving space for arrows.
    // This also removes them when a resized window can fit all tabs again.
    final overflow = m.maxScrollExtent + m.viewportDimension > _availableWidth + 0.5;
    final start = m.extentBefore > 0.5;
    final end = m.extentAfter > 0.5;
    if (overflow != _overflow || start != _fadeStart || end != _fadeEnd) {
      setState(() {
        _overflow = overflow;
        _fadeStart = start;
        _fadeEnd = end;
      });
    }
    return false;
  }

  @override
  Widget build(BuildContext context) {
    final strings = GlassStrings.of(context);
    Widget scroll = NotificationListener<ScrollMetricsNotification>(
      onNotification: (n) {
        if (n.metrics.viewportDimension != _viewportWidth) {
          _viewportWidth = n.metrics.viewportDimension;
          _scheduleReveal();
        }
        return _onMetrics(n.metrics);
      },
      child: NotificationListener<ScrollNotification>(
        onNotification: (n) => _onMetrics(n.metrics),
        child: Listener(
          onPointerSignal: _onPointerSignal,
          child: SingleChildScrollView(
            key: const ValueKey('glass-tabs-scroll'),
            controller: _scroll,
            scrollDirection: Axis.horizontal,
            child: Row(
              children: [
                for (final (i, child) in widget.children.indexed) KeyedSubtree(key: _tabKeys[i], child: child),
              ],
            ),
          ),
        ),
      ),
    );
    // Always masked (with opaque stops when nothing overflows) so the scroll
    // view keeps its state when the fades toggle.
    scroll = ShaderMask(
      blendMode: BlendMode.dstIn,
      shaderCallback: (bounds) {
        final f = bounds.width <= 0 ? 0.0 : (16 / bounds.width).clamp(0.0, 0.5);
        return LinearGradient(
          colors: [
            _fadeStart ? const Color(0x00000000) : const Color(0xFF000000),
            const Color(0xFF000000),
            const Color(0xFF000000),
            _fadeEnd ? const Color(0x00000000) : const Color(0xFF000000),
          ],
          stops: [0, f, 1 - f, 1],
        ).createShader(bounds);
      },
      child: scroll,
    );
    return LayoutBuilder(
      builder: (context, constraints) {
        _availableWidth = constraints.maxWidth;
        return Row(
          children: [
            if (_overflow)
              GlassIconButton(
                key: const ValueKey('glass-tabs-scroll-left'),
                icon: Icons.chevron_left_rounded,
                tooltip: strings.scrollTabsLeft,
                style: GlassIconButtonStyle.plain,
                size: GlassSizes.tab,
                onPressed: _fadeStart ? () => _page(-1) : null,
              ),
            Expanded(key: const ValueKey('glass-tabs-viewport'), child: scroll),
            if (_overflow)
              GlassIconButton(
                key: const ValueKey('glass-tabs-scroll-right'),
                icon: Icons.chevron_right_rounded,
                tooltip: strings.scrollTabsRight,
                style: GlassIconButtonStyle.plain,
                size: GlassSizes.tab,
                onPressed: _fadeEnd ? () => _page(1) : null,
              ),
            if (_overflow)
              Tooltip(
                message: strings.openTerminalTabs(widget.children.length),
                excludeFromSemantics: true,
                child: Semantics(
                  label: strings.openTerminalTabs(widget.children.length),
                  child: Padding(
                    padding: const EdgeInsetsDirectional.only(start: 4, end: 6),
                    child: ExcludeSemantics(
                      child: Text(
                        '(${widget.children.length})',
                        key: const ValueKey('glass-tabs-count'),
                        style: GlassTokens.of(context).typography.caption
                            .copyWith(color: GlassTokens.of(context).secondaryLabel),
                      ),
                    ),
                  ),
                ),
              ),
          ],
        );
      },
    );
  }
}
