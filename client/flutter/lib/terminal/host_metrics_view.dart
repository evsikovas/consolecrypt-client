import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:material_ui/material_ui.dart';

/// The view owns the watch and keeps only recent, in-memory samples.
class HostMetricsView extends StatefulWidget {
  const HostMetricsView({
    super.key,
    required this.host,
    required this.reader,
    required this.scope,
    required this.sessionId,
    required this.scopePending,
    required this.isCurrent,
  });

  final Host host;
  final HostMetricsReader? reader;
  final RuntimeNoticeScope? scope;
  final TerminalSessionId sessionId;
  final bool scopePending;
  final bool Function() isCurrent;

  @override
  State<HostMetricsView> createState() => _HostMetricsViewState();
}

class _HostMetricsViewState extends State<HostMetricsView> {
  final _samples = <HostMetricsSample>[];
  // _stop cancels this when switching tabs, retrying, or disposing the panel.
  // ignore: cancel_subscriptions
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
  void didUpdateWidget(HostMetricsView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.sessionId != widget.sessionId ||
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
    if (!_supported || !widget.isCurrent() || _operationId != null) return;
    final id = _operationId = generateUuidV4();
    final generation = ++_generation;
    bool current() => mounted && generation == _generation && widget.isCurrent();
    _subscription = widget.reader!
        .watchHost(widget.host, widget.sessionId, widget.scope!, id)
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

  void _stop(HostMetricsReader? reader) {
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
    if (error is AppException && error.args['field'] == 'host_metrics') {
      return switch (error.args['rule']) {
        'host_metrics_linux_only' => l.hostMetricsLinuxOnly,
        'host_metrics_ssh_backend' => l.hostMetricsNativeOnly,
        'host_metrics_host_changed' => l.hostMetricsChanged,
        'host_metrics_session_closed' => l.hostMetricsConnectSsh,
        _ => l.hostMetricsReadFailed,
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
    final latestUptime = latest?.uptime;
    final cpuDetails = [
      ?latest?.cpuModel,
      ?latest?.architecture,
      if (latest?.logicalCpus case final count?) l.hostMetricsLogicalCpus(count),
    ];
    final theme = Theme.of(context);
    return ListView(
      key: const ValueKey('host-metrics-live'),
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 16),
      children: [
        ContentSurface(
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  widget.host.name,
                  style: theme.textTheme.titleMedium,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                ),
                Text(
                  widget.host.address,
                  style: theme.textTheme.bodySmall,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                ),
                if (widget.host.notes.trim().isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Text(widget.host.notes, style: theme.textTheme.bodyMedium),
                ],
                if (latest != null) ...[
                  const SizedBox(height: 8),
                  _detail(l.hostMetricsOs, latest.os),
                  _detail(l.hostMetricsKernel, latest.kernel),
                  _detail(l.hostMetricsProcessor, cpuDetails.isEmpty ? null : cpuDetails.join(' · ')),
                  if (latestUptime != null)
                    _detail(
                      l.hostMetricsUptime,
                      l.hostMetricsUptimeValue(
                        latestUptime.inDays,
                        latestUptime.inHours % 24,
                        latestUptime.inMinutes % 60,
                      ),
                    ),
                  _detail(l.hostMetricsLoadAverage, latest.loadAverage),
                ],
              ],
            ),
          ),
        ),
        if (!_supported && !(supported && widget.reader != null && widget.scopePending))
          Padding(
            padding: const EdgeInsets.only(top: 12),
            child: Text(supported ? l.hostMetricsUnavailable : l.hostMetricsNativeOnly),
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
              children: [const LinearProgressIndicator(), Text(l.hostMetricsConnecting)],
            ),
          ),
        if (latest != null) ...[
          const SizedBox(height: 12),
          Wrap(
            spacing: 12,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Text('${l.hostMetricsLastSample} ${clock(latest.time)}'),
              ChoiceChip(
                label: Text(l.hostMetricsGraphs),
                selected: !_values,
                onSelected: (_) => setState(() => _values = false),
              ),
              ChoiceChip(
                label: Text(l.hostMetricsValues),
                selected: _values,
                onSelected: (_) => setState(() => _values = true),
              ),
            ],
          ),
          const SizedBox(height: 12),
          if (_values)
            for (final sample in _samples.reversed)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 6),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(clock(sample.time), style: theme.textTheme.titleSmall),
                    SelectableText(
                      'CPU ${percent(sample.cpu)}  ·  ${l.hostMetricsRam} ${amount(sample.memoryUsed, sample.memoryTotal)}  ·  '
                      '${l.hostMetricsDisk} ${amount(sample.diskUsed, sample.diskTotal)}  ·  '
                      '↓ ${speed(sample.received)}  ↑ ${speed(sample.sent)}',
                    ),
                  ],
                ),
              )
          else ...[
            _card(l.hostMetricsCpu, percent(latest.cpu), (sample) => sample.cpu, scale: 100),
            _card(
              l.hostMetricsRam,
              amount(latest.memoryUsed, latest.memoryTotal),
              (sample) => sample.memoryPercent,
              scale: 100,
            ),
            _card(
              l.hostMetricsNetwork,
              '↓ ${speed(latest.received)}  ↑ ${speed(latest.sent)}',
              (sample) => sample.received,
              second: (sample) => sample.sent,
            ),
            _card(
              l.hostMetricsDisk,
              amount(latest.diskUsed, latest.diskTotal),
              (sample) => sample.diskPercent,
              scale: 100,
            ),
          ],
        ],
      ],
    );
  }

  Widget _detail(String label, String? value) {
    if (value == null) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(flex: 2, child: Text(label, style: Theme.of(context).textTheme.bodySmall)),
          const SizedBox(width: 8),
          Expanded(flex: 3, child: Text(value, style: Theme.of(context).textTheme.bodySmall)),
        ],
      ),
    );
  }

  Widget _card(
    String title,
    String value,
    double? Function(HostMetricsSample) select, {
    double? scale,
    double? Function(HostMetricsSample)? second,
  }) {
    final scheme = Theme.of(context).colorScheme;
    final data = _samples.map(select).toList(), other = second == null ? null : _samples.map(second).toList();
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: ContentSurface(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Wrap(
                spacing: 8,
                alignment: WrapAlignment.spaceBetween,
                crossAxisAlignment: WrapCrossAlignment.center,
                children: [
                  Text(title, style: Theme.of(context).textTheme.titleSmall),
                  Text(value, style: Theme.of(context).textTheme.bodySmall),
                ],
              ),
              const SizedBox(height: 8),
              SizedBox(height: 44, child: CustomPaint(painter: _HostTrend(data, other, scale, scheme))),
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
