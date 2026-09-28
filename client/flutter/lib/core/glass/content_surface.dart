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
/// over [extent] px at the chosen edges; [ScrollEdgeEffect.hard] pins
/// [header] on an opaque band with a separator. One style per edge.
class ScrollEdgeEffect extends StatelessWidget {
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
  Widget build(BuildContext context) {
    if (style == ScrollEdgeStyle.hard) {
      final s = GlassTokens.of(context).surfaces;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          DecoratedBox(
            decoration: BoxDecoration(
              color: s.contentSolid,
              border: Border(bottom: BorderSide(color: s.separator)),
            ),
            child: header,
          ),
          Expanded(child: child),
        ],
      );
    }
    return ShaderMask(
      blendMode: BlendMode.dstIn,
      shaderCallback: (bounds) {
        final f = bounds.height <= 0 ? 0.0 : (extent / bounds.height).clamp(0.0, 0.5);
        return LinearGradient(
          begin: Alignment.topCenter,
          end: Alignment.bottomCenter,
          colors: [
            top ? const Color(0x00000000) : const Color(0xFF000000),
            const Color(0xFF000000),
            const Color(0xFF000000),
            bottom ? const Color(0x00000000) : const Color(0xFF000000),
          ],
          stops: [0, f, 1 - f, 1],
        ).createShader(bounds);
      },
      child: child,
    );
  }
}
