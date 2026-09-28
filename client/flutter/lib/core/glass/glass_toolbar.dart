import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_motion.dart';
import 'package:consolecrypt/app/theme/glass_palette.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_interaction.dart';
import 'package:consolecrypt/core/glass/glass_scope.dart';
import 'package:consolecrypt/core/glass/glass_surface.dart';
import 'package:macos_window_utils/widgets/macos_toolbar_passthrough.dart';
import 'package:material_ui/material_ui.dart';

/// The 52-high toolbar band (LIQUID_GLASS_SPEC §4.1): no background of its
/// own, up to three capsule groups (leading · centre · trailing), 12 px
/// between groups. Content columns start below it (52 top inset), so at rest
/// nothing sits under the capsules.
///
/// When the macOS unified title bar is active, every group is wrapped in
/// `MacosToolbarPassthrough` so clicks reach Flutter and empty areas still
/// drag the window.
class GlassToolbar extends StatelessWidget {
  const GlassToolbar({
    super.key,
    this.leading = const [],
    this.center,
    this.trailing = const [],
    this.leadingInset = 0,
    this.padding = const EdgeInsets.symmetric(horizontal: GlassSpacing.s12),
  });

  final List<Widget> leading;
  final Widget? center;
  final List<Widget> trailing;

  /// Extra leading space (78 when the traffic lights move into the band).
  final double leadingInset;
  final EdgeInsetsGeometry padding;

  @override
  Widget build(BuildContext context) {
    final passthrough = GlassScope.of(context).unifiedTitlebar;
    Widget wrap(Widget w) => passthrough ? MacosToolbarPassthrough(child: w) : w;
    List<Widget> spaced(List<Widget> items, {bool flexible = false}) => [
      for (final (i, w) in items.indexed) ...[
        if (i > 0) const SizedBox(width: GlassSpacing.toolbarGroupGap),
        // Leading groups may shrink (the title ellipsizes); trailing actions
        // keep their size so the primary action stays visible.
        if (flexible) Flexible(child: wrap(w)) else wrap(w),
      ],
    ];
    Widget band = SizedBox(
      height: GlassSizes.toolbarBand,
      child: Padding(
        padding: padding.add(EdgeInsetsDirectional.only(start: leadingInset)),
        child: Row(
          children: [
            ...spaced(leading, flexible: true),
            const SizedBox(width: GlassSpacing.toolbarGroupGap),
            Expanded(child: center == null ? const SizedBox.shrink() : Center(child: wrap(center!))),
            const SizedBox(width: GlassSpacing.toolbarGroupGap),
            ...spaced(trailing),
          ],
        ),
      ),
    );
    if (passthrough) {
      // Groups can move without their own constraints changing (a longer
      // title, the sidebar collapsing): re-send every native passthrough
      // rect after each rebuild so clicks never land on a stale region.
      band = MacosToolbarPassthroughScope(child: _PassthroughRefresh(child: band));
    }
    return band;
  }
}

class _PassthroughRefresh extends StatefulWidget {
  const _PassthroughRefresh({required this.child});

  final Widget child;

  @override
  State<_PassthroughRefresh> createState() => _PassthroughRefreshState();
}

class _PassthroughRefreshState extends State<_PassthroughRefresh> {
  @override
  Widget build(BuildContext context) {
    final notify = MacosToolbarPassthroughScope.maybeNotifyChangesOf(context);
    if (notify != null) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) notify();
      });
    }
    return widget.child;
  }
}

/// Toolbar items grouped onto one `glass.thin` capsule (height 32, 6 px gap).
/// Items inside should be plain (`GlassIconButtonStyle.plain`, text) — the
/// group is the glass. Never mix symbols and text labels in one group.
///
/// [backdrop]: only the toolbar group on list routes may be live (§3 rule 1).
class GlassToolbarGroup extends StatelessWidget {
  const GlassToolbarGroup({
    required this.children,
    super.key,
    this.backdrop = BackdropMode.static,
    this.padding = const EdgeInsets.symmetric(horizontal: GlassSpacing.s4),
  });

  final List<Widget> children;
  final BackdropMode backdrop;
  final EdgeInsetsGeometry padding;

  @override
  Widget build(BuildContext context) => GlassCapsule(
    backdrop: backdrop,
    padding: padding,
    child: Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (final (i, w) in children.indexed) ...[
          if (i > 0) const SizedBox(width: GlassSpacing.toolbarItemGap),
          Flexible(child: w),
        ],
      ],
    ),
  );
}

/// Toolbar page title (title3, left-aligned), for the leading group.
class GlassToolbarTitle extends StatelessWidget {
  const GlassToolbarTitle(this.title, {super.key});

  final String title;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
      child: Semantics(
        header: true,
        child: Text(
          title,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: tokens.typography.title3.copyWith(color: tokens.palette.label),
        ),
      ),
    );
  }
}

/// Sync / local-only status pill (§4.1): `glass.thin` capsule, 24 high, an
/// 8 px status dot (or [icon]) plus a caption/600 label. [pulsing] animates
/// the dot at 1 Hz ("Syncing…"), static under Reduce Motion. Local-only
/// uses [GlassTone.neutral] and a lock icon — never green.
class GlassStatusPill extends StatefulWidget {
  const GlassStatusPill({
    required this.label,
    required this.tone,
    super.key,
    this.icon,
    this.pulsing = false,
    this.onPressed,
    this.tooltip,
  });

  final String label;
  final GlassTone tone;
  final IconData? icon;
  final bool pulsing;
  final VoidCallback? onPressed;
  final String? tooltip;

  @override
  State<GlassStatusPill> createState() => _GlassStatusPillState();
}

class _GlassStatusPillState extends State<GlassStatusPill> with SingleTickerProviderStateMixin {
  AnimationController? _pulse;

  void _syncPulse(bool animate) {
    if (animate) {
      final pulse = _pulse ??= AnimationController(vsync: this, duration: const Duration(seconds: 1));
      if (!pulse.isAnimating) pulse.repeat(reverse: true);
    } else {
      _pulse?.stop();
    }
  }

  bool get _animate => widget.pulsing && !GlassScope.of(context).appearance.reduceMotion;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _syncPulse(_animate);
  }

  @override
  void didUpdateWidget(GlassStatusPill oldWidget) {
    super.didUpdateWidget(oldWidget);
    _syncPulse(_animate);
  }

  @override
  void dispose() {
    _pulse?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final motion = GlassMotion(reduceMotion: GlassScope.of(context).appearance.reduceMotion);
    final color = tokens.palette.tone(widget.tone);
    final Widget indicator = widget.icon != null
        ? Icon(widget.icon, size: 12, color: color)
        : _pulse != null && widget.pulsing && motion.allowsMovement
        ? FadeTransition(
            opacity: Tween(begin: 1.0, end: 0.35).animate(_pulse!),
            child: _Dot(color: color),
          )
        : _Dot(color: color);
    final capsule = GlassCapsule(
      height: GlassSizes.statusPill,
      padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          indicator,
          const SizedBox(width: GlassSpacing.s6),
          Text(
            widget.label,
            style: tokens.typography.caption.copyWith(color: tokens.palette.label, fontWeight: FontWeight.w600),
          ),
        ],
      ),
    );
    // Build from the immutable capsule: a closure over a reassigned `pill`
    // would make the interactive pill contain itself.
    final Widget pill = widget.onPressed != null
        ? GlassInteractive(
            onPressed: widget.onPressed,
            semanticLabel: widget.label,
            builder: (context, state) =>
                GlassFocusRing(visible: state.focusVisible, shape: const StadiumBorder(), child: capsule),
          )
        : Semantics(
            label: widget.label,
            child: ExcludeSemantics(child: capsule),
          );
    return widget.tooltip == null ? pill : Tooltip(message: widget.tooltip, child: pill);
  }
}

class _Dot extends StatelessWidget {
  const _Dot({required this.color});

  final Color color;

  @override
  Widget build(BuildContext context) => SizedBox.square(
    dimension: 8,
    child: DecoratedBox(
      decoration: BoxDecoration(color: color, shape: BoxShape.circle),
    ),
  );
}
