import 'dart:async';

import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:flutter/foundation.dart';

abstract interface class HostMetricsReader {
  Stream<HostMetricsSample> watchHost(Host host, TerminalSessionId sessionId, String operationId);
  Future<void> cancelHostWatch(String operationId);
}

final class HostMetricsHistory {
  const HostMetricsHistory({this.samples = const [], this.error});

  final List<HostMetricsSample> samples;
  final AppException? error;
  HostMetricsSample? get latest => samples.lastOrNull;
}

/// Owned by one terminal tab, including when its panel is closed.
final class TerminalHostMetrics extends ValueNotifier<HostMetricsHistory> {
  TerminalHostMetrics() : super(const HostMetricsHistory());

  static const capacity = 120;
  String? _operation;
  HostMetricsReader? _reader;
  // Cancelled by stop/dispose or completed by the native reader.
  // ignore: cancel_subscriptions
  StreamSubscription<HostMetricsSample>? _subscription;
  Timer? _retry;
  bool _started = false;

  void start(Host host, TerminalSessionId sessionId, HostMetricsReader reader) {
    if (_started) return;
    _started = true;
    _reader = reader;
    if (host.backend != SshBackend.native) {
      value = const HostMetricsHistory(
        error: AppException(
          AppErrorCode.unsupported,
          'SSH backend', // l10n-ignore: diagnostic detail; host_metrics_ssh_backend selects the localized UI message
          args: {'field': 'host_metrics', 'rule': 'host_metrics_ssh_backend'},
        ),
      );
      return;
    }
    void subscribe() {
      final id = _operation = generateUuidV4();
      void ended(Object? cause) {
        if (_operation != id) return;
        _subscription = null;
        _operation = null;
        final error = cause is AppException
            ? cause
            : const AppException(AppErrorCode.serverUnreachable, 'SSH statistics unavailable');
        value = HostMetricsHistory(samples: value.samples, error: error);
        if (!_started ||
            const {
              'host_metrics_linux_only',
              'host_metrics_ssh_backend',
              'host_metrics_host_changed',
              'host_metrics_session_closed',
            }.contains(error.args['rule'])) {
          return;
        }
        _retry = Timer(const Duration(seconds: 15), subscribe);
      }

      _subscription = reader
          .watchHost(host, sessionId, id)
          .listen(
            (sample) {
              if (_operation != id) return;
              final previous = value.samples;
              value = HostMetricsHistory(
                samples: List.unmodifiable([...previous.skip(previous.length >= capacity ? 1 : 0), sample]),
              );
            },
            onError: ended,
            onDone: () => ended(null),
            cancelOnError: true,
          );
    }

    subscribe();
  }

  void stop() {
    if (!_started) return;
    _started = false;
    _retry?.cancel();
    final id = _operation, reader = _reader;
    _operation = null;
    _reader = null;
    if (id != null && reader != null) unawaited(reader.cancelHostWatch(id).catchError((Object _) {}));
    unawaited(_subscription?.cancel());
    _subscription = null;
    value = const HostMetricsHistory();
  }

  @override
  void dispose() {
    stop();
    super.dispose();
  }
}

final class HostMetricsSample {
  HostMetricsSample.fromJson(Map<String, Object?> value)
    : time = DateTime.fromMillisecondsSinceEpoch(value['timestampMs'] as int),
      cpu = (value['cpuPercent'] as num?)?.toDouble(),
      channelLatencyMs = value['channelLatencyMs'] as int?,
      memoryTotal = value['memoryTotal'] as int?,
      memoryUsed = value['memoryUsed'] as int?,
      diskTotal = value['diskTotal'] as int?,
      diskUsed = value['diskUsed'] as int?,
      received = (value['receivedPerSecond'] as num?)?.toDouble(),
      sent = (value['sentPerSecond'] as num?)?.toDouble(),
      uptime = Duration(seconds: (value['uptimeSeconds'] as num).round()),
      docker = value['docker'] == true,
      logins = value['logins'] as int?,
      swapTotal = value['swapTotal'] as int?,
      swapUsed = value['swapUsed'] as int?,
      cores = Map<String, dynamic>.from(value['cores'] as Map? ?? const {}),
      interfaces = (value['interfaces'] as List? ?? const [])
          .whereType<Map<String, Object?>>()
          .where((row) => row['name'] is String)
          .toList(),
      os = value['os'] as String?,
      kernel = value['kernel'] as String?,
      architecture = value['architecture'] as String?,
      cpuModel = value['cpuModel'] as String?,
      logicalCpus = value['logicalCpus'] as int?,
      loadAverage = value['loadAverage'] as String?;

  final bool docker;
  final int? logins, swapTotal, swapUsed;
  final Map<String, dynamic> cores;
  final List<Map<String, Object?>> interfaces;
  final DateTime time;
  final double? cpu, received, sent;
  final int? memoryTotal, memoryUsed, diskTotal, diskUsed;
  final Duration uptime;
  final String? os, kernel, architecture, cpuModel, loadAverage;
  final int? logicalCpus, channelLatencyMs;

  double? get memoryPercent => _percent(memoryUsed, memoryTotal);
  double? get diskPercent => _percent(diskUsed, diskTotal);
  static double? _percent(int? used, int? total) =>
      used == null || total == null || total == 0 ? null : 100 * used / total;
}
