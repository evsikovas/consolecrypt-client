import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';

abstract interface class HostMetricsReader {
  Stream<HostMetricsSample> watchHost(Host host, RuntimeNoticeScope scope, String operationId);
  Future<void> cancelHostWatch(String operationId);
}

final class HostMetricsSample {
  HostMetricsSample.fromJson(Map<String, Object?> value)
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
