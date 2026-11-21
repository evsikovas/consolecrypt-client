import 'dart:collection';
import 'dart:math' as math;

import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:material_ui/material_ui.dart';


final _size = RegExp(r'^\s*([\d.]+)\s*([a-zA-Z]*)\s*$');

/// "29.09MiB", "1.2MB", "340kB", "0B" → bytes. Binary and decimal units as Docker prints them.
double? parseSize(String? text) {
  final m = _size.firstMatch(text ?? '');
  if (m == null) return null;
  final value = double.tryParse(m.group(1)!);
  if (value == null) return null;
  final unit = m.group(2)!.toLowerCase();
  final factor = switch (unit) {
    '' || 'b' => 1.0,
    'kb' => 1e3,
    'kib' => 1024.0,
    'mb' => 1e6,
    'mib' => 1024.0 * 1024,
    'gb' => 1e9,
    'gib' => 1024.0 * 1024 * 1024,
    'tb' => 1e12,
    'tib' => 1024.0 * 1024 * 1024 * 1024,
    _ => null,
  };
  return factor == null ? null : value * factor;
}

/// "1.2MB / 340kB" → (1200000, 340000).
(double, double)? parsePair(String? text) {
  final parts = text?.split('/');
  if (parts == null || parts.length != 2) return null;
  final a = parseSize(parts[0]), b = parseSize(parts[1]);
  return a == null || b == null ? null : (a, b);
}

/// [decimal] matches sizes the runtime prints for images (`108MB` = 108 000 000 bytes).
String formatBytes(double bytes, {bool perSecond = false, bool decimal = false}) {
  const units = ['Б', 'КБ', 'МБ', 'ГБ', 'ТБ'];
  final base = decimal ? 1000 : 1024;
  var v = bytes;
  var i = 0;
  while (v >= base && i < units.length - 1) {
    v /= base;
    i++;
  }
  final text = v >= 100 || i == 0 ? v.toStringAsFixed(0) : v.toStringAsFixed(1);
  return '$text ${units[i]}${perSecond ? '/с' : ''}';
}

/// Rolling samples of one container; network counters become per-second rates.
class UsageHistory {
  UsageHistory([this.capacity = 60]);
  final int capacity;
  final cpu = ListQueue<double>();
  final memory = ListQueue<double>();
  final rx = ListQueue<double>();
  final tx = ListQueue<double>();
  DateTime? _at;
  (double, double)? _net;

  void _push(ListQueue<double> q, double v) {
    q.add(v);
    while (q.length > capacity) {
      q.removeFirst();
    }
  }

  void add(ContainerStats s, DateTime at) {
    _push(cpu, s.cpuPercent ?? 0);
    _push(memory, parseSize(s.memoryUsage?.split('/').first) ?? 0);
    final net = parsePair(s.netIo);
    final last = _net, lastAt = _at;
    if (net != null && last != null && lastAt != null) {
      final dt = at.difference(lastAt).inMilliseconds / 1000;
      if (dt > 0) {
        // Counters reset when the container restarts; a negative delta is not traffic.
        _push(rx, math.max(0, (net.$1 - last.$1) / dt));
        _push(tx, math.max(0, (net.$2 - last.$2) / dt));
      }
    }
    _net = net;
    _at = at;
  }
}

/// Line chart with a soft gradient fill. Draws nothing until two points exist.
class Sparkline extends StatelessWidget {
  const Sparkline({
    super.key,
    required this.values,
    required this.color,
    this.second,
    this.secondColor,
    this.max,
    this.strokeWidth = 1.6,
  });
  final List<double> values;
  final Color color;
  final List<double>? second;
  final Color? secondColor;

  /// Fixed upper bound (e.g. 100 for percent); otherwise the data maximum.
  final double? max;
  final double strokeWidth;

  @override
  Widget build(BuildContext context) => RepaintBoundary(
    child: CustomPaint(
      painter: _SparkPainter(values, color, second, secondColor ?? DeskPalette.of(context).accent, max, strokeWidth),
      size: Size.infinite,
    ),
  );
}

class _SparkPainter extends CustomPainter {
  _SparkPainter(this.values, this.color, this.second, this.secondColor, this.max, this.strokeWidth);
  final List<double> values;
  final Color color;
  final List<double>? second;
  final Color secondColor;
  final double? max;
  final double strokeWidth;

  @override
  void paint(Canvas canvas, Size size) {
    final all = [...values, ...?second];
    if (all.isEmpty) return;
    final top = math.max(max ?? all.reduce(math.max), 1e-9) * (max == null ? 1.15 : 1);
    void line(List<double> data, Color c, {required bool fill}) {
      if (data.length < 2) return;
      final step = size.width / (data.length - 1);
      final path = Path();
      for (var i = 0; i < data.length; i++) {
        final x = i * step;
        final y = size.height - (data[i] / top).clamp(0, 1) * size.height;
        i == 0 ? path.moveTo(x, y) : path.lineTo(x, y);
      }
      if (fill) {
        final area = Path.from(path)
          ..lineTo(size.width, size.height)
          ..lineTo(0, size.height)
          ..close();
        canvas.drawPath(
          area,
          Paint()
            ..shader = LinearGradient(
              begin: Alignment.topCenter,
              end: Alignment.bottomCenter,
              colors: [c.withValues(alpha: .28), c.withValues(alpha: 0)],
            ).createShader(Offset.zero & size),
        );
      }
      canvas.drawPath(
        path,
        Paint()
          ..color = c
          ..style = PaintingStyle.stroke
          ..strokeWidth = strokeWidth
          ..strokeJoin = StrokeJoin.round
          ..strokeCap = StrokeCap.round,
      );
    }

    line(values, color, fill: true);
    if (second != null) line(second!, secondColor, fill: false);
  }

  @override
  bool shouldRepaint(_SparkPainter old) => true;
}

/// Card with a title, the current value and a chart of recent samples.
class UsageChart extends StatelessWidget {
  const UsageChart({
    super.key,
    required this.title,
    required this.value,
    required this.values,
    required this.color,
    this.second,
    this.secondColor,
    this.legend,
    this.max,
  });
  final String title;
  final String value;
  final List<double> values;
  final Color color;
  final List<double>? second;
  final Color? secondColor;
  final String? legend;
  final double? max;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return Container(
      padding: const EdgeInsets.fromLTRB(14, 12, 14, 10),
      decoration: BoxDecoration(
        color: p.raised.withValues(alpha: .55),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: p.border.withValues(alpha: .7)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            title.toUpperCase(),
            style: TextStyle(color: p.faint, fontSize: 10.5, fontWeight: FontWeight.w700, letterSpacing: .6),
          ),
          const SizedBox(height: 4),
          Text(
            value,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
          ),
          const SizedBox(height: 8),
          SizedBox(
            height: 64,
            child: values.length < 2
                ? Center(
                    child: Text('Сбор данных…', style: TextStyle(color: p.faint, fontSize: 12)),
                  )
                : Sparkline(
                    values: values,
                    color: color,
                    second: second,
                    secondColor: secondColor,
                    max: max,
                    strokeWidth: 2,
                  ),
          ),
          if (legend != null) ...[
            const SizedBox(height: 6),
            Text(legend!, style: TextStyle(color: p.faint, fontSize: 11)),
          ],
        ],
      ),
    );
  }
}
