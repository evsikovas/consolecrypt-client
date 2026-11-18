import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart' show monitoringOperationId;
import 'package:material_ui/material_ui.dart';

abstract interface class MonitoringHostLiveReader {
  Stream<HostMetricsSample> watchHost(Host host, RuntimeNoticeScope scope, String operationId);
  Future<void> cancelHostWatch(String operationId);
}

final class HostMetricsSample {
  HostMetricsSample.fromJson(Json value)
    : time = DateTime.fromMillisecondsSinceEpoch(value['timestampMs'] as int),
      cpu = (value['cpuPercent'] as num?)?.toDouble(),
      memoryTotal = value['memoryTotal'] as int?,
      memoryUsed = value['memoryUsed'] as int?,
      diskTotal = value['diskTotal'] as int?,
      diskUsed = value['diskUsed'] as int?,
      received = (value['receivedPerSecond'] as num?)?.toDouble(),
      sent = (value['sentPerSecond'] as num?)?.toDouble(),
      uptime = Duration(seconds: (value['uptimeSeconds'] as num).round());

  final DateTime time;
  final double? cpu, received, sent;
  final int? memoryTotal, memoryUsed, diskTotal, diskUsed;
  final Duration uptime;

  double? get memoryPercent => _percent(memoryUsed, memoryTotal);
  double? get diskPercent => _percent(diskUsed, diskTotal);
  static double? _percent(int? used, int? total) =>
      used == null || total == null || total == 0 ? null : 100 * used / total;
}

/// The view owns the watch and keeps only recent, in-memory samples.
class MonitoringHostLive extends StatefulWidget {
  const MonitoringHostLive({
    super.key,
    required this.host,
    required this.reader,
    required this.scope,
    required this.active,
    required this.isCurrent,
  });

  final Host host;
  final MonitoringHostLiveReader? reader;
  final RuntimeNoticeScope? scope;
  final bool active;
  final bool Function() isCurrent;

  @override
  State<MonitoringHostLive> createState() => _MonitoringHostLiveState();
}

class _MonitoringHostLiveState extends State<MonitoringHostLive> {
  final _samples = <HostMetricsSample>[];
  StreamSubscription<HostMetricsSample>? _subscription;
  String? _operationId;
  Object? _failure;
  var _generation = 0;
  var _values = false;
  bool get _supported => widget.host.backend == SshBackend.native && widget.reader != null && widget.scope != null;

  @override
  void initState() {
    super.initState();
    _start();
  }

  @override
  void didUpdateWidget(MonitoringHostLive oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.active != widget.active ||
        oldWidget.host.id != widget.host.id ||
        oldWidget.host.updatedAt != widget.host.updatedAt ||
        oldWidget.reader != widget.reader ||
        oldWidget.scope != widget.scope) {
      _stop(oldWidget.reader);
      _samples.clear();
      _failure = null;
      _start();
    }
  }

  void _start() {
    if (!_supported || !widget.active || !widget.isCurrent() || _operationId != null) return;
    final id = _operationId = monitoringOperationId();
    final generation = ++_generation;
    bool current() => mounted && generation == _generation && widget.active && widget.isCurrent();
    _subscription = widget.reader!
        .watchHost(widget.host, widget.scope!, id)
        .listen(
          (sample) {
            if (!current()) return;
            setState(() {
              _samples.add(sample);
              if (_samples.length > 120) _samples.removeAt(0);
            });
          },
          onError: (Object error) {
            if (current()) setState(() => _failure = error);
          },
          onDone: () {
            if (!current()) return;
            setState(() {
              _operationId = null;
              _subscription = null;
              _failure ??= const AppException(AppErrorCode.serverUnreachable, 'The SSH metrics connection ended');
            });
          },
        );
  }

  void _stop(MonitoringHostLiveReader? reader) {
    ++_generation;
    final id = _operationId, subscription = _subscription;
    _operationId = null;
    _subscription = null;
    if (id != null && reader != null) {
      unawaited(reader.cancelHostWatch(id).catchError((Object _) {}));
    }
    unawaited(subscription?.cancel());
  }

  @override
  void dispose() {
    _stop(widget.reader);
    super.dispose();
  }

  String _error(AppLocalizations l, Object error) {
    if (error is AppException && error.args['field'] == 'monitoring') {
      return switch (error.args['rule']) {
        'host_metrics_linux_only' => l.monitoringLiveLinuxOnly,
        'host_metrics_ssh_backend' => l.monitoringLiveNativeOnly,
        'host_metrics_host_changed' => l.correlationContextChanged,
        _ => l.monitoringLiveReadFailed,
      };
    }
    return errorMessage(l, error);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final latest = _samples.lastOrNull;
    String clock(DateTime time) =>
        MaterialLocalizations.of(context).formatTimeOfDay(TimeOfDay.fromDateTime(time), alwaysUse24HourFormat: true);
    String percent(double? value) => value == null ? '—' : '${value.toStringAsFixed(1)} %';
    String amount(int? used, int? total) =>
        used == null || total == null ? '—' : '${formatBytes(l, used)} / ${formatBytes(l, total)}';
    String speed(double? value) => value == null ? '—' : '${formatBytes(l, value.round())}/s';
    final supported = widget.host.backend == SshBackend.native;
    return ListView(
      key: const ValueKey('monitoring-host-live'),
      padding: const EdgeInsets.all(20),
      children: [
        Text(l.monitoringLiveTitle, style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 4),
        Text(l.monitoringLiveHint),
        if (!_supported)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Text(supported ? l.monitoringUnavailable : l.monitoringLiveNativeOnly),
          )
        else if (_failure case final error?)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                InfoBanner(message: _error(l, error), tone: BannerTone.danger),
                OutlinedButton(
                  onPressed: () {
                    _stop(widget.reader);
                    setState(() {
                      _samples.clear();
                      _failure = null;
                    });
                    _start();
                  },
                  child: Text(l.commonRetry),
                ),
              ],
            ),
          )
        else if (latest == null)
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [const LinearProgressIndicator(), Text(l.monitoringLiveConnecting)],
            ),
          ),
        if (latest != null) ...[
          const SizedBox(height: 12),
          Wrap(
            spacing: 12,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Text('${l.monitoringLiveLastSample} ${clock(latest.time)}'),
              Text(
                '${l.monitoringLiveUptime} ${l.monitoringLiveUptimeValue(latest.uptime.inDays, latest.uptime.inHours % 24, latest.uptime.inMinutes % 60)}',
              ),
              ChoiceChip(
                label: Text(l.monitoringSetupGraphs),
                selected: !_values,
                onSelected: (_) => setState(() => _values = false),
              ),
              ChoiceChip(
                label: Text(l.monitoringLiveValues),
                selected: _values,
                onSelected: (_) => setState(() => _values = true),
              ),
            ],
          ),
          const SizedBox(height: 12),
          if (_values)
            for (final sample in _samples.reversed)
              ListTile(
                dense: true,
                title: Text(clock(sample.time)),
                subtitle: SelectableText(
                  'CPU ${percent(sample.cpu)}  ·  ${l.monitoringLiveRam} ${amount(sample.memoryUsed, sample.memoryTotal)}  ·  '
                  '${l.monitoringLiveDisk} ${amount(sample.diskUsed, sample.diskTotal)}  ·  '
                  '↓ ${speed(sample.received)}  ↑ ${speed(sample.sent)}',
                ),
              )
          else
            LayoutBuilder(
              builder: (context, constraints) {
                final width = constraints.maxWidth >= 620 ? (constraints.maxWidth - 12) / 2 : constraints.maxWidth;
                return Wrap(
                  spacing: 12,
                  runSpacing: 12,
                  children: [
                    _card(width, l.monitoringLiveCpu, percent(latest.cpu), (sample) => sample.cpu, scale: 100),
                    _card(
                      width,
                      l.monitoringLiveRam,
                      amount(latest.memoryUsed, latest.memoryTotal),
                      (sample) => sample.memoryPercent,
                      scale: 100,
                    ),
                    _card(
                      width,
                      l.monitoringLiveDisk,
                      amount(latest.diskUsed, latest.diskTotal),
                      (sample) => sample.diskPercent,
                      scale: 100,
                    ),
                    _card(
                      width,
                      l.monitoringLiveNetwork,
                      '↓ ${speed(latest.received)}  ↑ ${speed(latest.sent)}',
                      (sample) => sample.received,
                      second: (sample) => sample.sent,
                    ),
                  ],
                );
              },
            ),
        ],
      ],
    );
  }

  Widget _card(
    double width,
    String title,
    String value,
    double? Function(HostMetricsSample) select, {
    double? scale,
    double? Function(HostMetricsSample)? second,
  }) {
    final scheme = Theme.of(context).colorScheme;
    final data = _samples.map(select).toList(), other = second == null ? null : _samples.map(second).toList();
    return SizedBox(
      width: width,
      child: ContentSurface(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(title, style: Theme.of(context).textTheme.titleMedium),
              Text(value, style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 8),
              SizedBox(height: 56, child: CustomPaint(painter: _HostTrend(data, other, scale, scheme))),
            ],
          ),
        ),
      ),
    );
  }
}

class _HostTrend extends CustomPainter {
  _HostTrend(this.data, this.other, this.percentScale, this.colors);
  final List<double?> data;
  final List<double?>? other;
  final double? percentScale;
  final ColorScheme colors;

  @override
  void paint(Canvas canvas, Size size) {
    final grid = Paint()..color = colors.outlineVariant;
    for (final fraction in [0.0, 0.5, 1.0]) {
      final y = fraction * size.height;
      canvas.drawLine(Offset(0, y), Offset(size.width, y), grid);
    }
    final maximum =
        percentScale ??
        [...data, ...?other].whereType<double>().fold<double>(1.0, (max, value) => value > max ? value : max);
    void draw(List<double?> values, Color color) {
      final paint = Paint()
        ..color = color
        ..strokeWidth = 2;
      Offset? previous;
      for (var i = 0; i < values.length; i++) {
        final value = values[i];
        if (value == null) {
          previous = null;
          continue;
        }
        final x = values.length < 2 ? 0.0 : i * size.width / (values.length - 1);
        final point = Offset(x, size.height * (1 - (value / maximum).clamp(0, 1)));
        if (previous == null) {
          canvas.drawCircle(point, 2, paint);
        } else {
          canvas.drawLine(previous, point, paint);
        }
        previous = point;
      }
    }

    draw(data, colors.primary);
    if (other case final values?) draw(values, colors.tertiary);
  }

  @override
  bool shouldRepaint(covariant _HostTrend oldDelegate) => true;
}
