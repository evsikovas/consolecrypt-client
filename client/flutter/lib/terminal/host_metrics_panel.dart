import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

/// Displays the active SSH tab's in-memory measurements. The tab owns collection.
class HostMetricsPanel extends StatelessWidget {
  const HostMetricsPanel({super.key, required this.history});
  final ValueListenable<HostMetricsHistory> history;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder<HostMetricsHistory>(
    valueListenable: history,
    builder: (context, snapshot, _) {
      final l = context.l10n;
      final tokens = GlassTokens.of(context);
      final sample = snapshot.latest;
      final error = snapshot.error;
      return SingleChildScrollView(
        key: const ValueKey('host-metrics-live'),
        padding: const EdgeInsets.all(GlassSpacing.s12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: GlassSpacing.s12,
          children: [
            if (error != null) InfoBanner(message: _readError(l, error), tone: BannerTone.warning),
            if (sample == null && error == null) ...[const LinearProgressIndicator(), Text(l.hostMetricsConnecting)],
            if (sample != null) ...[
              Text(
                '${l.hostMetricsLastSample} ${DateFormat.Hms(l.localeName).format(sample.time.toLocal())}',
                style: tokens.typography.caption.copyWith(color: tokens.palette.secondary),
              ),
              _charts(context, l, snapshot.samples, sample),
              _systemDetails(context, l, sample),
            ],
          ],
        ),
      );
    },
  );

  String _readError(AppLocalizations l, AppException error) {
    if (error.args['field'] != 'host_metrics') return errorMessage(l, error);
    return switch (error.args['rule']) {
      'host_metrics_linux_only' => l.hostMetricsLinuxOnly,
      'host_metrics_ssh_backend' => l.hostMetricsNativeOnly,
      'host_metrics_host_changed' => l.hostMetricsChanged,
      'host_metrics_session_closed' => l.hostMetricsConnectSsh,
      _ => l.hostMetricsReadFailed,
    };
  }

  Widget _charts(BuildContext context, AppLocalizations l, List<HostMetricsSample> history, HostMetricsSample latest) {
    final colors = Theme.of(context).colorScheme;
    final percent = NumberFormat.decimalPercentPattern(locale: l.localeName, decimalDigits: 1);
    String percentage(double? value) => value == null || !value.isFinite ? '—' : percent.format(value / 100);
    String bytes(int? value) => value == null ? '—' : formatBytes(l, value);
    String speed(double? value) =>
        value == null || !value.isFinite ? '—' : l.sftpTransferSpeed(formatBytes(l, value.round()));

    final charts = [
      _MetricCard(
        title: l.hostMetricsCpu,
        value: percentage(latest.cpu),
        icon: Icons.speed,
        color: colors.primary,
        samples: history,
        select: (s) => s.cpu,
      ),
      _MetricCard(
        title: l.hostMetricsRam,
        value: percentage(latest.memoryPercent),
        icon: Icons.memory_outlined,
        color: colors.primary,
        detail: '${bytes(latest.memoryUsed)} / ${bytes(latest.memoryTotal)}',
        samples: history,
        select: (s) => s.memoryPercent,
      ),
      _MetricCard(
        title: l.hostMetricsDisk,
        value: percentage(latest.diskPercent),
        icon: Icons.storage_outlined,
        color: colors.primary,
        detail: '${bytes(latest.diskUsed)} / ${bytes(latest.diskTotal)}',
        samples: history,
        select: (s) => s.diskPercent,
      ),
      _MetricCard(
        title: l.hostMetricsNetwork,
        value: '${l.hostMetricsReceived} ${speed(latest.received)}',
        detail: '${l.hostMetricsSent} ${speed(latest.sent)}',
        icon: Icons.swap_vert,
        color: colors.primary,
        samples: history,
        select: (s) => s.received,
        second: (s) => s.sent,
      ),
    ];
    return LayoutBuilder(
      builder: (context, bounds) {
        const gap = GlassSpacing.s8;
        final columns = bounds.maxWidth >= 920
            ? 4
            : bounds.maxWidth >= 460
            ? 2
            : 1;
        final width = (bounds.maxWidth - gap * (columns - 1)) / columns;
        return Wrap(
          spacing: gap,
          runSpacing: gap,
          children: [for (final chart in charts) SizedBox(width: width, child: chart)],
        );
      },
    );
  }

  Widget _systemDetails(BuildContext context, AppLocalizations l, HostMetricsSample sample) {
    final tokens = GlassTokens.of(context);
    final cpu = [
      ?sample.cpuModel,
      ?sample.architecture,
      if (sample.logicalCpus case final count?) l.hostMetricsLogicalCpus(count),
    ];
    final rows = [
      if (sample.os case final os?) (l.hostMetricsOs, os),
      if (sample.kernel case final kernel?) (l.hostMetricsKernel, kernel),
      if (cpu.isNotEmpty) (l.hostMetricsProcessor, cpu.join(' · ')),
      if (sample.loadAverage case final load?) (l.hostMetricsLoadAverage, load),
      (
        l.hostMetricsUptime,
        l.hostMetricsUptimeValue(sample.uptime.inDays, sample.uptime.inHours % 24, sample.uptime.inMinutes % 60),
      ),
    ];
    return ContentSurface(
      padding: const EdgeInsets.all(GlassSpacing.s12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        spacing: GlassSpacing.s6,
        children: [
          for (final (label, value) in rows)
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              spacing: GlassSpacing.s8,
              children: [
                Expanded(
                  flex: 2,
                  child: Text(label, style: tokens.typography.caption.copyWith(color: tokens.palette.secondary)),
                ),
                Expanded(
                  flex: 3,
                  child: Text(value, style: tokens.typography.callout.copyWith(color: tokens.palette.label)),
                ),
              ],
            ),
        ],
      ),
    );
  }
}

typedef _Reading = double? Function(HostMetricsSample);

class _MetricCard extends StatelessWidget {
  const _MetricCard({
    required this.title,
    required this.value,
    required this.icon,
    required this.color,
    required this.samples,
    required this.select,
    this.detail,
    this.second,
  });
  final String title, value;
  final String? detail;
  final IconData icon;
  final Color color;
  final List<HostMetricsSample> samples;
  final _Reading select;
  final _Reading? second;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final secondColor = tokens.palette.secondary;
    return ContentSurface(
      padding: const EdgeInsets.all(GlassSpacing.s12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        spacing: GlassSpacing.s4,
        children: [
          Row(
            spacing: GlassSpacing.s6,
            children: [
              Icon(icon, size: GlassSizes.iconMenu, color: tokens.palette.secondary),
              Expanded(
                child: Text(title, style: tokens.typography.callout.copyWith(color: tokens.palette.label)),
              ),
            ],
          ),
          Text(value, style: tokens.typography.title3.copyWith(color: second != null ? color : tokens.palette.label)),
          if (detail case final text?)
            Text(
              text,
              style: tokens.typography.caption.copyWith(color: second != null ? secondColor : tokens.palette.secondary),
            ),
          SizedBox(
            height: 38,
            child: RepaintBoundary(
              child: CustomPaint(
                painter: _TrendPainter(samples, select, second, color, secondColor, tokens.surfaces.separator),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _TrendPainter extends CustomPainter {
  const _TrendPainter(this.samples, this.select, this.second, this.color, this.secondColor, this.grid);
  final List<HostMetricsSample> samples;
  final _Reading select;
  final _Reading? second;
  final Color color, secondColor, grid;

  @override
  void paint(Canvas canvas, Size size) {
    if (samples.isEmpty || size.width < 4 || size.height < 4) return;
    final area = (Offset.zero & size).deflate(2);
    final guide = Paint()..color = grid;
    for (final y in [area.top, area.center.dy, area.bottom]) {
      canvas.drawLine(Offset(area.left, y), Offset(area.right, y), guide);
    }
    var ceiling = 100.0;
    if (second case final sent?) {
      ceiling = 1;
      for (final sample in samples) {
        for (final value in [select(sample), sent(sample)]) {
          if (value != null && value.isFinite && value > ceiling) ceiling = value;
        }
      }
    }
    final first = samples.first.time;
    final span = samples.last.time.difference(first).inMilliseconds;
    void draw(_Reading read, Color lineColor) {
      final ink = Paint()
        ..color = lineColor
        ..strokeWidth = 2
        ..strokeCap = StrokeCap.round;
      Offset? previous;
      for (final sample in samples) {
        final value = read(sample);
        if (value == null || !value.isFinite) {
          previous = null;
          continue;
        }
        final position = span <= 0 ? 1.0 : (sample.time.difference(first).inMilliseconds / span).clamp(0.0, 1.0);
        final point = Offset(
          area.left + area.width * position,
          area.bottom - area.height * (value / ceiling).clamp(0.0, 1.0),
        );
        if (previous == null) {
          canvas.drawCircle(point, 2, ink);
        } else {
          canvas.drawLine(previous, point, ink);
        }
        previous = point;
      }
    }

    draw(select, color);
    if (second case final sent?) draw(sent, secondColor);
  }

  @override
  bool shouldRepaint(covariant _TrendPainter old) =>
      samples != old.samples ||
      select != old.select ||
      second != old.second ||
      color != old.color ||
      secondColor != old.secondColor ||
      grid != old.grid;
}
