import 'package:consolecrypt/app/theme/glass_materials.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_palette.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:consolecrypt/core/glass/glass_badges.dart';
import 'package:consolecrypt/core/glass/glass_interaction.dart';
import 'package:consolecrypt/core/glass/glass_surface.dart';
import 'package:material_ui/material_ui.dart';

/// Sidebar placement (LIQUID_GLASS_SPEC §2.5): the one switch between the
/// owner's floating inset sidebar and the macOS 27 edge-to-edge sidebar.
enum GlassSidebarStyle {
  /// `glass.regular` panel, `r.panel`, inset `shellInset` (8) from the
  /// window's left, top and bottom edges.
  floating,

  /// `shellInset` = 0: square outer corners, a separator on the inner edge,
  /// same `glass.regular` tint.
  edgeToEdge,
}

/// Floating glass sidebar (§4.1): static `glass.regular` (never live — it
/// sits over the painted ambient backdrop), width 240 (compact 64). On macOS
/// the top 52 pt are reserved for the traffic lights.
class GlassSidebar extends StatelessWidget {
  const GlassSidebar({
    required this.children,
    super.key,
    this.header,
    this.footer,
    this.style = GlassSidebarStyle.floating,
    this.compact = false,
    this.reserveTrafficLights,
  });

  /// Nav items ([GlassSidebarItem]s, section labels).
  final List<Widget> children;

  /// Profile switcher etc. (below the traffic lights).
  final Widget? header;

  /// Lock button, compact sync pill. Never put critical actions only here.
  final Widget? footer;
  final GlassSidebarStyle style;
  final bool compact;

  /// Defaults to `true` on macOS.
  final bool? reserveTrafficLights;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final floating = style == GlassSidebarStyle.floating;
    final inset = floating ? tokens.radii.shellInset : 0.0;
    final reserve = reserveTrafficLights ?? tokens.platform == TargetPlatform.macOS;
    final width = compact ? GlassSizes.sidebarCompactWidth : GlassSizes.sidebarWidth;

    Widget panel = GlassSurface(
      key: const ValueKey('glass-sidebar'),
      variant: GlassVariant.regular,
      shape: GlassRadii.shape(floating ? tokens.radii.panel : 0),
      shadows: floating,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SizedBox(height: reserve ? GlassSizes.trafficLightsTop : GlassSpacing.s12),
          if (header != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, 0, GlassSpacing.s8, GlassSpacing.s8),
              child: header,
            ),
          Expanded(
            child: ListView(
              padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
              children: children,
            ),
          ),
          if (footer != null) Padding(padding: const EdgeInsets.all(GlassSpacing.s8), child: footer),
        ],
      ),
    );
    if (!floating) {
      panel = DecoratedBox(
        position: DecorationPosition.foreground,
        decoration: BoxDecoration(
          border: BorderDirectional(end: BorderSide(color: tokens.surfaces.separator)),
        ),
        child: panel,
      );
    }
    return Padding(
      padding: EdgeInsetsDirectional.fromSTEB(inset, inset, 0, inset),
      child: SizedBox(width: width, child: panel),
    );
  }
}

/// Sidebar navigation item (§4.1): height 32, radius `r.row` (concentric
/// with the panel), icon 18, body label. Selected: accent α .16 fill, accent
/// icon (the only coloured icon), weight 600. Hover: `fill.hover`. Compact:
/// icon only with a tooltip.
class GlassSidebarItem extends StatelessWidget {
  const GlassSidebarItem({
    required this.icon,
    required this.label,
    required this.onPressed,
    super.key,
    this.selectedIcon,
    this.leading,
    this.selected = false,
    this.badge,
    this.badgeTone = GlassTone.neutral,
    this.compact = false,
  });

  final IconData icon;
  final IconData? selectedIcon;

  /// Optional vector glyph; inherits the selection colour through IconTheme.
  final Widget? leading;
  final String label;
  final VoidCallback? onPressed;
  final bool selected;

  /// Dense count badge (terminal tabs, pending approvals → warning).
  final String? badge;
  final GlassTone badgeTone;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final shape = GlassRadii.shape(tokens.radii.row);
    Widget item = GlassInteractive(
      onPressed: onPressed,
      selected: selected,
      semanticLabel: badge == null ? label : '$label, $badge',
      builder: (context, state) {
        final bg = selected
            ? tokens.sidebarSelection
            : state.pressed
            ? tokens.surfaces.fillPressed
            : state.hovered
            ? tokens.surfaces.fillHover
            : const Color(0x00000000);
        final iconColor = (selected ? p.accent : tokens.secondaryLabel).withValues(alpha: state.enabled ? 1 : 0.38);
        final glyph = IconTheme(
          data: IconThemeData(size: GlassSizes.iconRow, color: iconColor),
          child: leading ?? Icon(selected ? (selectedIcon ?? icon) : icon),
        );
        final badgeWidget = badge == null ? null : GlassBadge(label: badge!, tone: badgeTone, dense: true);
        return GlassFocusRing(
          visible: state.focusVisible,
          shape: shape,
          child: DecoratedBox(
            decoration: ShapeDecoration(color: bg, shape: shape),
            child: SizedBox(
              height: GlassSizes.sidebarRow + 4,
              child: compact
                  ? Center(
                      child: badgeWidget == null
                          ? glyph
                          : Badge(
                              label: Text(badge!),
                              backgroundColor: switch (badgeTone) {
                                GlassTone.warning => p.warningFill,
                                GlassTone.danger => p.dangerFill,
                                _ => p.accentFill,
                              },
                              textColor: badgeTone == GlassTone.danger ? p.onDangerFill : p.onAccent,
                              child: glyph,
                            ),
                    )
                  : Padding(
                      padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
                      child: Row(
                        children: [
                          glyph,
                          const SizedBox(width: GlassSpacing.s8),
                          Expanded(
                            child: Text(
                              label,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: tokens.typography.body.copyWith(
                                color: p.label.withValues(alpha: state.enabled ? 1 : 0.38),
                                fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
                              ),
                            ),
                          ),
                          ?badgeWidget,
                        ],
                      ),
                    ),
            ),
          ),
        );
      },
    );
    if (compact) item = Tooltip(message: label, child: item);
    return Padding(padding: const EdgeInsets.symmetric(vertical: 1), child: item);
  }
}
