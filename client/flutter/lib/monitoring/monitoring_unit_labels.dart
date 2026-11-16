import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';

String monitoringUnitLabel(AppLocalizations l, MonitoringUnit unit) => switch (unit) {
  MonitoringUnit.percent => l.monitoringUnitPercent,
  MonitoringUnit.bytes => l.monitoringUnitBytes,
  MonitoringUnit.bytesPerSecond => l.monitoringUnitBytesPerSecond,
  MonitoringUnit.cpuCores => l.monitoringUnitCpuCores,
  MonitoringUnit.connections => l.monitoringUnitConnections,
  MonitoringUnit.transactionsPerSecond => l.monitoringUnitTransactionsPerSecond,
  MonitoringUnit.operationsPerSecond => l.monitoringUnitOperationsPerSecond,
  MonitoringUnit.requestsPerSecond => l.monitoringUnitRequestsPerSecond,
};
