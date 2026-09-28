import 'package:material_ui/material_ui.dart';

/// Original, consistent 24-point outline symbols for the navigation.
enum AppSymbol {
  hosts,
  groups,
  credentials,
  knownHosts,
  terminal,
  sftp,
  tunnels,
  snippets,
  ai,
  devices,
  sync,
  backups,
  settings,
}

class AppSymbolIcon extends StatelessWidget {
  const AppSymbolIcon(this.symbol, {super.key, this.size = 20, this.color});
  final AppSymbol symbol;
  final double size;
  final Color? color;

  @override
  Widget build(BuildContext context) => ExcludeSemantics(
    child: CustomPaint(
      size: Size.square(size),
      painter: _SymbolPainter(symbol, color ?? IconTheme.of(context).color ?? const Color(0xff777777)),
    ),
  );
}

class _SymbolPainter extends CustomPainter {
  const _SymbolPainter(this.symbol, this.color);
  final AppSymbol symbol;
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    canvas.save();
    canvas.scale(size.width / 24, size.height / 24);
    final ink = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.6
      ..strokeJoin = StrokeJoin.round
      ..strokeCap = StrokeCap.round;
    void line(double x, double y, double a, double b) => canvas.drawLine(Offset(x, y), Offset(a, b), ink);
    void box(double x, double y, double w, double h, [double r = 2]) =>
        canvas.drawRRect(RRect.fromRectAndRadius(Rect.fromLTWH(x, y, w, h), Radius.circular(r)), ink);
    void path(List<Offset> points, {bool close = false}) => canvas.drawPath(Path()..addPolygon(points, close), ink);
    switch (symbol) {
      case AppSymbol.hosts:
        box(3, 3, 18, 7);
        box(3, 14, 18, 7);
        line(7, 6.5, 7.1, 6.5);
        line(7, 17.5, 7.1, 17.5);
        line(15, 6.5, 17, 6.5);
        line(15, 17.5, 17, 17.5);
      case AppSymbol.groups:
        box(9, 2, 6, 5, 1.5);
        box(2, 17, 6, 5, 1.5);
        box(16, 17, 6, 5, 1.5);
        line(12, 7, 12, 12);
        path(const [Offset(5, 17), Offset(5, 12), Offset(19, 12), Offset(19, 17)]);
      case AppSymbol.credentials:
        canvas.drawCircle(const Offset(8, 8), 5, ink);
        path(const [Offset(11.5, 11.5), Offset(21, 21), Offset(21, 17), Offset(18, 17), Offset(18, 14)]);
        line(6.5, 6.5, 6.6, 6.5);
      case AppSymbol.knownHosts:
        canvas.drawPath(
          Path()
            ..moveTo(12, 2)
            ..lineTo(21, 6)
            ..lineTo(21, 11)
            ..cubicTo(21, 16, 17, 20, 12, 22)
            ..cubicTo(7, 20, 3, 16, 3, 11)
            ..lineTo(3, 6)
            ..close(),
          ink,
        );
        path(const [Offset(8, 12), Offset(11, 15), Offset(16, 9)]);
      case AppSymbol.terminal:
        box(2, 3, 20, 18, 3);
        path(const [Offset(6, 8), Offset(10, 12), Offset(6, 16)]);
        line(13, 16, 18, 16);
      case AppSymbol.sftp:
        canvas.drawPath(
          Path()
            ..moveTo(3, 20)
            ..quadraticBezierTo(2, 20, 2, 18)
            ..lineTo(2, 6)
            ..quadraticBezierTo(2, 4, 4, 4)
            ..lineTo(9, 4)
            ..lineTo(12, 7)
            ..lineTo(20, 7)
            ..quadraticBezierTo(22, 7, 22, 9)
            ..lineTo(22, 18)
            ..quadraticBezierTo(22, 20, 20, 20)
            ..close(),
          ink,
        );
        line(8, 13, 16, 13);
        path(const [Offset(13, 10), Offset(16, 13), Offset(13, 16)]);
      case AppSymbol.tunnels:
        path(const [Offset(3, 7), Offset(21, 7), Offset(17, 3)]);
        path(const [Offset(21, 17), Offset(3, 17), Offset(7, 21)]);
        line(21, 7, 17, 11);
        line(3, 17, 7, 13);
      case AppSymbol.snippets:
        path(const [Offset(7, 6), Offset(2, 12), Offset(7, 18)]);
        path(const [Offset(17, 6), Offset(22, 12), Offset(17, 18)]);
        line(14, 3, 10, 21);
      case AppSymbol.ai:
        path(const [
          Offset(12, 2),
          Offset(15, 9),
          Offset(22, 12),
          Offset(15, 15),
          Offset(12, 22),
          Offset(9, 15),
          Offset(2, 12),
          Offset(9, 9),
        ], close: true);
      case AppSymbol.devices:
        box(2, 3, 15, 12);
        line(9, 15, 9, 20);
        line(5, 20, 12, 20);
        box(16, 10, 6, 12, 1.5);
        line(18.5, 19, 19.5, 19);
      case AppSymbol.sync:
        canvas.drawArc(const Rect.fromLTWH(4, 4, 16, 16), -2.7, 2.8, false, ink);
        canvas.drawArc(const Rect.fromLTWH(4, 4, 16, 16), .44, 2.8, false, ink);
        path(const [Offset(16, 10), Offset(20, 13), Offset(22, 8)]);
        path(const [Offset(8, 14), Offset(4, 11), Offset(2, 16)]);
      case AppSymbol.backups:
        box(3, 3, 18, 5, 1.5);
        path(const [Offset(5, 8), Offset(5, 21), Offset(19, 21), Offset(19, 8)]);
        line(9, 12, 15, 12);
      case AppSymbol.settings:
        line(5, 3, 5, 6);
        line(5, 12, 5, 21);
        canvas.drawCircle(const Offset(5, 9), 3, ink);
        line(12, 3, 12, 13);
        line(12, 19, 12, 21);
        canvas.drawCircle(const Offset(12, 16), 3, ink);
        line(19, 3, 19, 6);
        line(19, 12, 19, 21);
        canvas.drawCircle(const Offset(19, 9), 3, ink);
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(_SymbolPainter oldDelegate) => oldDelegate.symbol != symbol || oldDelegate.color != color;
}
