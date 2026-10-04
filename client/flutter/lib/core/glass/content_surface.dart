import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:material_ui/material_ui.dart';

/// Content surfaces of LIQUID_GLASS_SPEC §2.3 (not glass: no blur, no
/// refraction).
enum ContentSurfaceKind {
  /// Cards, lists, forms: `surface.content` (alpha only).
  content,

  /// SFTP panes, editors, the terminal card: opaque `surface.contentSolid`.
  solid,

  /// Recovery Kit words, verification codes: opaque `surface.paper`.
  paper,

  /// Code groups, inline code, fingerprints: opaque `surface.inset`.
  inset,
}

/// A content-layer card (§4.6): `r.card`, `hairline.card` border, padding
/// 16; dark mode adds a white α .04 top highlight line. No shadow.
class ContentSurface extends StatelessWidget {
  const ContentSurface({
    required this.child,
    super.key,
    this.kind = ContentSurfaceKind.content,
    this.radius,
    this.padding = const EdgeInsets.all(GlassSpacing.card),
    this.border = true,
    this.color,
  });

  final Widget child;
  final ContentSurfaceKind kind;

  /// Defaults to `r.card` (`r.md` for [ContentSurfaceKind.inset]).
  final double? radius;
  final EdgeInsetsGeometry padding;
  final bool border;

  /// Overrides the surface colour (e.g. the terminal theme background).
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final s = tokens.surfaces;
    final fill =
        color ??
        switch (kind) {
          ContentSurfaceKind.content => s.content,
          ContentSurfaceKind.solid => s.contentSolid,
          ContentSurfaceKind.paper => s.paper,
          ContentSurfaceKind.inset => s.inset,
        };
    final r = radius ?? (kind == ContentSurfaceKind.inset ? tokens.radii.md : tokens.radii.card);
    final shape = GlassRadii.shape(r);
    Widget result = DecoratedBox(
      decoration: ShapeDecoration(
        color: fill,
        shape: border ? shape.copyWith(side: BorderSide(color: s.hairlineCard)) : shape,
      ),
      // material_ui rows (hover highlights) paint on the nearest Material;
      // without one here they would be drawn under this fill.
      child: Material(
        type: MaterialType.transparency,
        child: Padding(padding: padding, child: child),
      ),
    );
    if (tokens.isDark && kind != ContentSurfaceKind.inset) {
      result = Stack(
        // Same size as without the highlight (stretches like the light card).
        fit: StackFit.passthrough,
        children: [
          result,
          Positioned(
            top: 0.5,
            left: r,
            right: r,
            height: 1,
            child: IgnorePointer(child: ColoredBox(color: s.cardTopHighlight)),
          ),
        ],
      );
    }
    return result;
  }
}

/// Scroll-edge styles (§1.6, §3 rule 2).
enum ScrollEdgeStyle {
  /// Progressive fade to transparent (a `ShaderMask`, not a blur).
  soft,

  /// Opaque `surface.contentSolid` band for pinned headers.
  hard,
}

/// Replaces bar backgrounds: [ScrollEdgeStyle.soft] fades scrolling content
/// over [extent] px at edges with more content outside the viewport. At the
/// start/end of a list its first/last row stays fully visible instead of
/// appearing underneath a header. [ScrollEdgeEffect.hard] pins [header] on
/// an opaque band with a separator. One style per edge.
class ScrollEdgeEffect extends StatefulWidget {
  const ScrollEdgeEffect({required this.child, super.key, this.top = true, this.bottom = false, this.extent = 24})
    : style = ScrollEdgeStyle.soft,
      header = null;

  const ScrollEdgeEffect.hard({required this.child, required Widget this.header, super.key})
    : style = ScrollEdgeStyle.hard,
      top = true,
      bottom = false,
      extent = 0;

  final Widget child;
  final ScrollEdgeStyle style;
  final bool top;
  final bool bottom;
  final double extent;
  final Widget? header;

  @override
  State<ScrollEdgeEffect> createState() => _ScrollEdgeEffectState();
}

class _ScrollEdgeEffectState extends State<ScrollEdgeEffect> {
  bool _above = false;
  bool _below = false;

  void _updateMetrics(ScrollMetrics metrics) {
    // A nested or horizontal list must not fade its containing page.
    if (metrics.axis != Axis.vertical || !metrics.hasContentDimensions) return;
    final reversed = metrics.axisDirection == AxisDirection.up;
    final above = (reversed ? metrics.extentAfter : metrics.extentBefore) > 0.5;
    final below = (reversed ? metrics.extentBefore : metrics.extentAfter) > 0.5;
    if (above == _above && below == _below) return;
    setState(() {
      _above = above;
      _below = below;
    });
  }

  @override
  Widget build(BuildContext context) {
    if (widget.style == ScrollEdgeStyle.hard) {
      final s = GlassTokens.of(context).surfaces;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          DecoratedBox(
            decoration: BoxDecoration(
              color: s.contentSolid,
              border: Border(bottom: BorderSide(color: s.separator)),
            ),
            child: widget.header,
          ),
          Expanded(child: widget.child),
        ],
      );
    }
    return NotificationListener<ScrollMetricsNotification>(
      onNotification: (notification) {
        if (notification.depth == 0) _updateMetrics(notification.metrics);
        return false;
      },
      child: NotificationListener<ScrollNotification>(
        onNotification: (notification) {
          if (notification.depth == 0) _updateMetrics(notification.metrics);
          return false;
        },
        // Keep the same mask/child tree while scrolling so text fields and
        // scroll positions retain their state when an edge becomes visible.
        child: ShaderMask(
          blendMode: BlendMode.dstIn,
          shaderCallback: (bounds) {
            final f = bounds.height <= 0 ? 0.0 : (widget.extent / bounds.height).clamp(0.0, 0.5);
            return LinearGradient(
              begin: Alignment.topCenter,
              end: Alignment.bottomCenter,
              colors: [
                widget.top && _above ? const Color(0x00000000) : const Color(0xFF000000),
                const Color(0xFF000000),
                const Color(0xFF000000),
                widget.bottom && _below ? const Color(0x00000000) : const Color(0xFF000000),
              ],
              stops: [0, f, 1 - f, 1],
            ).createShader(bounds);
          },
          child: widget.child,
        ),
      ),
    );
  }
}
