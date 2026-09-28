import 'package:consolecrypt/app/app_info.dart';
import 'package:consolecrypt/app/theme/glass_tokens.dart';
import 'package:material_ui/material_ui.dart';

enum BrandArtwork { mark, wordmark, shield }

/// Resolution-independent shield and terminal prompt. The same geometry is
/// used by the interface and the app-icon generator in tool/render_brand.dart.
class BrandMark extends StatelessWidget {
  const BrandMark.mark({super.key, double size = 28}) : artwork = BrandArtwork.mark, extent = size;
  const BrandMark.wordmark({super.key, double height = 44}) : artwork = BrandArtwork.wordmark, extent = height;
  const BrandMark.shield({super.key, double height = 120}) : artwork = BrandArtwork.shield, extent = height;

  final BrandArtwork artwork;
  final double extent;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final mark = CustomPaint(
      painter: BrandSymbolPainter(color: tokens.palette.accent),
      size: Size.square(extent),
    );
    return Semantics(
      label: kAppName,
      image: true,
      excludeSemantics: true,
      child: SizedBox(
        key: ValueKey('brand-${artwork.name}'),
        height: extent,
        width: artwork == BrandArtwork.wordmark ? extent * 6.15 : extent,
        child: artwork == BrandArtwork.wordmark
            ? Row(
                children: [
                  mark,
                  SizedBox(width: extent * .22),
                  Expanded(
                    child: FittedBox(
                      fit: BoxFit.scaleDown,
                      alignment: AlignmentDirectional.centerStart,
                      child: Text.rich(
                        TextSpan(
                          children: [
                            TextSpan(text: kAppName.substring(0, 7)),
                            TextSpan(
                              text: kAppName.substring(7),
                              style: TextStyle(color: tokens.palette.accent),
                            ),
                          ],
                        ),
                        style: tokens.typography.title1.copyWith(
                          color: tokens.palette.label,
                          fontSize: extent * .66,
                          fontWeight: FontWeight.w600,
                          letterSpacing: -extent * .025,
                        ),
                      ),
                    ),
                  ),
                ],
              )
            : mark,
      ),
    );
  }
}

/// Coordinates use a 32-point grid and remain legible at small icon sizes.
class BrandSymbolPainter extends CustomPainter {
  const BrandSymbolPainter({required this.color});
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    canvas.save();
    canvas.scale(size.width / 32, size.height / 32);
    final shield = Path()
      ..moveTo(16, 2.5)
      ..lineTo(27, 7)
      ..lineTo(27, 15)
      ..cubicTo(27, 22.2, 22.5, 26.5, 16, 29.5)
      ..cubicTo(9.5, 26.5, 5, 22.2, 5, 15)
      ..lineTo(5, 7)
      ..close();
    canvas.drawPath(shield, Paint()..color = color.withValues(alpha: .09));
    final ink = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.round
      ..strokeJoin = StrokeJoin.round;
    canvas.drawPath(shield, ink);
    canvas.drawPath(
      Path()
        ..moveTo(10, 11)
        ..lineTo(14, 15)
        ..lineTo(10, 19),
      ink,
    );
    canvas.drawLine(const Offset(17, 19), const Offset(22, 19), ink);
    canvas.restore();
  }

  @override
  bool shouldRepaint(BrandSymbolPainter oldDelegate) => oldDelegate.color != color;
}
