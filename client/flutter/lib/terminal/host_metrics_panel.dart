import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

/// Displays the active SSH tab's in-memory measurements. The tab owns collection.
class HostMetricsPanel extends StatelessWidget {
  const HostMetricsPanel({super.key, required this.history, this.hostName, this.address, this.description});
  final String? hostName, address, description;
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
            if (sample == null && hostName != null) Text(hostName!, style: tokens.typography.bodyEmph),
            if (error != null) InfoBanner(message: hostMetricsErrorMessage(l, error), tone: BannerTone.warning),
            if (sample == null && error == null) ...[const LinearProgressIndicator(), Text(l.hostMetricsConnecting)],
            if (sample != null) ...[
              _systemDetails(context, l, sample),
              Text(
                '${l.hostMetricsLastSample} ${DateFormat.Hms(l.localeName).format(sample.time.toLocal())}',
                style: tokens.typography.caption.copyWith(color: tokens.palette.secondary),
              ),
              _charts(context, l, snapshot.samples, sample),
            ],
          ],
        ),
      );
    },
  );


  Widget _charts(BuildContext context, AppLocalizations l, List<HostMetricsSample> history, HostMetricsSample latest) {
    final colors = Theme.of(context).colorScheme;
    final percent = NumberFormat.decimalPercentPattern(locale: l.localeName, decimalDigits: 1);
    String percentage(double? value) => value == null || !value.isFinite ? '—' : percent.format(value / 100);
    String bytes(int? value) => value == null ? '—' : formatBytes(l, value);
    String speed(double? value) =>
        value == null || !value.isFinite ? '—' : l.sftpTransferSpeed(formatBytes(l, value.round()));

    final tokens = GlassTokens.of(context);
    final end = latest.time, start = end.subtract(const Duration(minutes: 5));
    final points = history.where((sample) => !sample.time.isBefore(start)).toList();
    final time = DateFormat.Hm(l.localeName);
    Widget meter(String label, int? used, int? total) {
      final pct = used == null || total == null || total == 0 ? null : 100 * used / total;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(child: Text(label, style: tokens.typography.bodyEmph)),
              Text(percentage(pct)),
            ],
          ),
          if (pct != null)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: LinearProgressIndicator(value: (pct / 100).clamp(0, 1), minHeight: 3),
            ),
          Text([bytes(used), bytes(total)].join(' / '), style: tokens.typography.caption),
        ],
      );
    }

    final cores = latest.cores.keys.toList()
      ..sort((a, b) => int.parse(a.substring(3)).compareTo(int.parse(b.substring(3))));
    return ContentSurface(
      padding: const EdgeInsets.all(12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        spacing: 8,
        children: [
          Wrap(
            alignment: WrapAlignment.spaceBetween,
            spacing: 8,
            children: [
              Text(l.hostMetricsCpu, style: tokens.typography.bodyEmph),
              Text(l.uiFiveMinutes, style: tokens.typography.caption),
            ],
          ),
          Text(percentage(latest.cpu), style: tokens.typography.title1.copyWith(color: colors.primary)),
          SizedBox(
            height: 94,
            child: CustomPaint(painter: _CpuChart(points, start, end, colors.primary, tokens.surfaces.separator)),
          ),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              for (final t in [start, start.add(const Duration(seconds: 150)), end])
                Text(time.format(t.toLocal()), style: tokens.typography.caption),
            ],
          ),
          const Divider(height: 1),
          if (cores.isNotEmpty)
            LayoutBuilder(
              builder: (context, constraints) {
                const gap = 4.0;
                final maxColumns = ((constraints.maxWidth + gap) / 24).floor().clamp(1, cores.length);
                final rows = (cores.length / maxColumns).ceil();
                final columns = (cores.length / rows).ceil();
                final width = ((constraints.maxWidth - gap * (columns - 1)) / columns).floorToDouble();
                return Wrap(
                  spacing: gap,
                  runSpacing: gap,
                  children: [
                    for (final name in cores)
                      Tooltip(
                        message: '$name · ${percentage((latest.cores[name] as num?)?.toDouble())}',
                        child: ClipRRect(
                          borderRadius: BorderRadius.circular(4),
                          child: SizedBox(
                            width: width,
                            height: 28,
                            child: ColoredBox(
                              color: tokens.surfaces.inset,
                              child: Align(
                                alignment: Alignment.bottomCenter,
                                child: FractionallySizedBox(
                                  widthFactor: 1,
                                  heightFactor: (((latest.cores[name] as num?)?.toDouble() ?? 0) / 100).clamp(0, 1),
                                  child: ColoredBox(color: colors.primary),
                                ),
                              ),
                            ),
                          ),
                        ),
                      ),
                  ],
                );
              },
            ),
          meter(l.hostMetricsRam, latest.memoryUsed, latest.memoryTotal),
          if ((latest.swapTotal ?? 0) > 0) meter(l.sessionSwap, latest.swapUsed, latest.swapTotal),
          meter(l.hostMetricsDisk, latest.diskUsed, latest.diskTotal),
          Text(l.hostMetricsNetwork, style: tokens.typography.bodyEmph),
          Wrap(
            spacing: 12,
            children: [
              Text('${l.hostMetricsReceived} ↓ ${speed(latest.received)}'),
              Text('${l.hostMetricsSent} ↑ ${speed(latest.sent)}'),
            ],
          ),
          for (final nic in latest.interfaces)
            ContentSurface(
              kind: ContentSurfaceKind.inset,
              radius: 6,
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
              child: Wrap(
                alignment: WrapAlignment.spaceBetween,
                crossAxisAlignment: WrapCrossAlignment.center,
                spacing: 8,
                runSpacing: 4,
                children: [
                  Text(nic['name'] as String, style: tokens.typography.mono.copyWith(fontSize: 11)),
                  Wrap(
                    spacing: 12,
                    children: [
                      for (final (arrow, key, label) in [
                        ('↓', 'rx', l.hostMetricsReceived),
                        ('↑', 'tx', l.hostMetricsSent),
                      ])
                        Tooltip(
                          message: label,
                          child: Text(
                            '$arrow ${speed((nic[key] as num?)?.toDouble())}',
                            style: tokens.typography.caption.copyWith(
                              color: ((nic[key] as num?) ?? 0) > 0 ? tokens.palette.label : tokens.secondaryLabel,
                            ),
                          ),
                        ),
                    ],
                  ),
                ],
              ),
            ),
        ],
      ),
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
      if (address?.isNotEmpty == true) (l.hostEditorAddressLabel, address!),
      if (sample.os case final os?) (l.hostMetricsOs, os),
      if (sample.logins case final count?) (l.sessionLogins, count.toString()),
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
          if (hostName != null) Text(hostName!, style: tokens.typography.bodyEmph),
          if (description?.trim().isNotEmpty == true)
            Text(description!, maxLines: 3, overflow: TextOverflow.ellipsis, style: tokens.typography.caption),
          for (final (label, value) in rows) ...[
            const Divider(height: 1),
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
                  child: Text(
                    value,
                    textAlign: TextAlign.end,
                    style: tokens.typography.callout.copyWith(color: tokens.palette.label),
                  ),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }
}

class _CpuChart extends CustomPainter {
  const _CpuChart(this.samples, this.start, this.end, this.color, this.grid);
  final List<HostMetricsSample> samples;
  final DateTime start, end;
  final Color color, grid;
  @override
  void paint(Canvas canvas, Size size) {
    final area = (Offset.zero & size).deflate(2);
    if (area.isEmpty) return;
    final guide = Paint()..color = grid;
    for (final fraction in [0.0, 1 / 3, 2 / 3, 1.0]) {
      final y = area.top + fraction * area.height;
      canvas.drawLine(Offset(area.left, y), Offset(area.right, y), guide);
    }
    final line = Paint()
      ..color = color
      ..strokeWidth = 2
      ..style = PaintingStyle.stroke;
    final fill = Paint()..color = color.withValues(alpha: .12);
    final points = <Offset>[];
    void draw() {
      if (points.isEmpty) return;
      final path = Path()..moveTo(points.first.dx, points.first.dy);
      for (final point in points.skip(1)) {
        path.lineTo(point.dx, point.dy);
      }
      final shade = Path.from(path)
        ..lineTo(points.last.dx, area.bottom)
        ..lineTo(points.first.dx, area.bottom)
        ..close();
      canvas.drawPath(shade, fill);
      canvas.drawPath(path, line);
      canvas.drawCircle(points.last, 2.5, Paint()..color = color);
      points.clear();
    }

    for (final sample in samples) {
      final cpu = sample.cpu;
      if (cpu == null || !cpu.isFinite) {
        draw();
        continue;
      }
      final x = (sample.time.difference(start).inMilliseconds / end.difference(start).inMilliseconds).clamp(0.0, 1.0);
      points.add(Offset(area.left + x * area.width, area.bottom - area.height * (cpu / 100).clamp(0.0, 1.0)));
    }
    draw();
  }

  @override
  bool shouldRepaint(_CpuChart old) =>
      old.samples != samples || old.color != color || old.grid != grid || old.end != end;
}
