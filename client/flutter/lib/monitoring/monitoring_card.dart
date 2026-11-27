import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/monitoring/monitoring_poller.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

class MonitoringCard extends StatelessWidget {
  const MonitoringCard({
    super.key,
    required this.host,
    required this.reading,
    required this.percent,
    this.minimal = false,
    required this.onConnect,
  });
  final Host host;
  final VoidCallback onConnect;
  final MonitorReading? reading;
  final bool percent, minimal;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context), colors = Theme.of(context).colorScheme;
    final sample = reading?.sample, error = reading?.error;
    String percentage(double? value) =>
        value == null ? '—' : NumberFormat.percentPattern(l.localeName).format(value / 100);
    String usage(int? used, int? total, double? ratio) => percent
        ? percentage(ratio)
        : used == null || total == null
        ? '—'
        : '${formatBytes(l, used)} / ${formatBytes(l, total)}';
    final state = error != null
        ? l.sessionUsageUnavailable
        : sample == null
        ? l.hostMetricsConnecting
        : l.monitorAvailable;
    final statusDot = Tooltip(
      message: state,
      child: Icon(
        Icons.circle,
        size: 8,
        color: error != null
            ? colors.error
            : sample == null
            ? tokens.secondaryLabel
            : tokens.palette.success,
      ),
    );
    final connect = GlassIconButton(
      key: ValueKey('monitor-connect-${host.id.value}'),
      tooltip: l.monitorConnectSsh(host.name),
      style: GlassIconButtonStyle.plain,
      icon: Icons.terminal_rounded,
      size: 28,
      onPressed: onConnect,
    );
    final diagnostic = error == null
        ? null
        : switch (error) {
            AppException(code: AppErrorCode.hostKeyRejected) => l.monitorTrustHint,
            AppException(code: AppErrorCode.authFailed || AppErrorCode.cancelled) => l.monitorAuthHint,
            _ => hostMetricsErrorMessage(l, error),
          };
    if (minimal) {
      return GlassPanel(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Wrap(
              spacing: 12,
              runSpacing: 4,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    statusDot,
                    const SizedBox(width: 8),
                    Flexible(
                      child: Tooltip(
                        message: host.address,
                        child: Text(host.name, overflow: TextOverflow.ellipsis, style: tokens.typography.bodyEmph),
                      ),
                    ),
                  ],
                ),
                if (sample != null)
                  for (final (label, description, ratio, value) in [
                    ('CPU', l.hostMetricsCpu, sample.cpu, percentage(sample.cpu)),
                    ('RAM', l.hostMetricsRam, sample.memoryPercent, percentage(sample.memoryPercent)),
                    (l.monitorDiskShort, l.hostMetricsDisk, sample.diskPercent, percentage(sample.diskPercent)),
                  ])
                    Tooltip(
                      message: description,
                      child: Text.rich(
                        TextSpan(
                          children: [
                            TextSpan(
                              text: '$label ',
                              style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                            ),
                            TextSpan(
                              text: value,
                              style: tokens.typography.callout.copyWith(color: _usageColor(tokens, ratio)),
                            ),
                          ],
                        ),
                      ),
                    ),
                connect,
                if (sample == null && diagnostic == null) Text(state, style: tokens.typography.caption),
              ],
            ),
            if (diagnostic != null) Text(diagnostic, style: tokens.typography.caption.copyWith(color: colors.error)),
          ],
        ),
      );
    }
    return GlassPanel(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Padding(padding: const EdgeInsets.only(top: 4, right: 9), child: statusDot),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(host.name, style: tokens.typography.bodyEmph),
                    Text(host.address, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                  ],
                ),
              ),
              connect,
            ],
          ),
          const SizedBox(height: 6),
          if (error != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: Text(diagnostic!, style: tokens.typography.caption.copyWith(color: colors.error)),
            ),
          if (sample == null && error == null) Text(l.hostMetricsConnecting, style: tokens.typography.caption),
          if (sample != null) ...[
            _Metric(label: 'CPU', description: l.hostMetricsCpu, value: percentage(sample.cpu), ratio: sample.cpu),
            _Metric(
              label: l.monitorLoad,
              description: l.hostMetricsLoadAverage,
              value: [
                sample.loadAverage?.replaceAll(' / ', '  ') ?? '—',
                if (sample.logicalCpus != null) '/ ${sample.logicalCpus}',
              ].join(' '),
            ),
            _Metric(
              label: 'RAM',
              description: l.hostMetricsRam,
              value: usage(sample.memoryUsed, sample.memoryTotal, sample.memoryPercent),
              ratio: sample.memoryPercent,
            ),
            _Metric(
              label: '/',
              description: l.hostMetricsDisk,
              value: usage(sample.diskUsed, sample.diskTotal, sample.diskPercent),
              ratio: sample.diskPercent,
            ),
            const SizedBox(height: 6),
            DefaultTextStyle(
              style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
              child: Wrap(
                alignment: WrapAlignment.spaceBetween,
                spacing: 12,
                runSpacing: 4,
                children: [
                  Tooltip(
                    message: l.hostMetricsUptime,
                    child: Text(
                      l.hostMetricsUptimeValue(
                        sample.uptime.inDays,
                        sample.uptime.inHours % 24,
                        sample.uptime.inMinutes % 60,
                      ),
                    ),
                  ),
                  Tooltip(
                    message: l.hostMetricsLastSample,
                    child: Text(DateFormat.Hms(l.localeName).format(sample.time.toLocal())),
                  ),
                ],
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class _Metric extends StatelessWidget {
  const _Metric({required this.label, required this.description, required this.value, this.ratio});
  final String label, description, value;
  final double? ratio;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 1),
      child: Semantics(
        label: '$description $value',
        excludeSemantics: true,
        child: Row(
          children: [
            ConstrainedBox(
              constraints: BoxConstraints(minWidth: MediaQuery.textScalerOf(context).scale(26)),
              child: Tooltip(
                message: description,
                child: Text(label, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
              ),
            ),
            const SizedBox(width: 8),
            if (ratio case final usage?) ...[
              Expanded(
                flex: 2,
                child: LinearProgressIndicator(
                  value: (usage / 100).clamp(0, 1),
                  minHeight: 4,
                  borderRadius: BorderRadius.circular(2),
                  color: _usageColor(tokens, ratio),
                ),
              ),
              const SizedBox(width: 8),
            ],
            Expanded(
              flex: 5,
              child: Text(
                value,
                textAlign: TextAlign.end,
                style: tokens.typography.callout.copyWith(color: _usageColor(tokens, ratio)),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

Color _usageColor(GlassTokens tokens, double? percent) => switch (percent) {
  null => tokens.secondaryLabel,
  <= 40 => tokens.palette.success,
  <= 90 => tokens.palette.warning,
  _ => tokens.palette.danger,
};
