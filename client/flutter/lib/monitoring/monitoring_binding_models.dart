import 'package:consolecrypt/monitoring/monitoring_models.dart';

enum MonitoringPanel { hostOverview, cpu, memory, filesystem, diskIo, network, docker, postgreSql, redis, nginx }

enum MonitoringUnit {
  percent,
  bytes,
  bytesPerSecond,
  cpuCores,
  connections,
  transactionsPerSecond,
  operationsPerSecond,
  requestsPerSecond,
}

/// Exact static Native metadata. Labels and remote strings cannot add a panel,
/// reinterpret units or supply an executable query through this model.
enum MonitoringMetric {
  cpuBusy(MonitoringUnit.percent, 'cpu.busyPercent.averageLogicalCpus.idleRate5m'),
  memoryUsedPercent(MonitoringUnit.percent, 'memory.usedPercent.totalMinusAvailable'),
  memoryAvailable(MonitoringUnit.bytes, 'memory.availableBytes'),
  filesystemUnavailablePercent(
    MonitoringUnit.percent,
    'filesystem.unavailableToUnprivilegedPercent.includesReserved.perMount',
  ),
  diskRead(MonitoringUnit.bytesPerSecond, 'disk.readBytesPerSecond.resetAware.perDevice'),
  diskWrite(MonitoringUnit.bytesPerSecond, 'disk.writeBytesPerSecond.resetAware.perDevice'),
  networkReceive(
    MonitoringUnit.bytesPerSecond,
    'network.receiveBytesPerSecond.resetAware.perInterface.includesLoopback',
  ),
  networkTransmit(
    MonitoringUnit.bytesPerSecond,
    'network.transmitBytesPerSecond.resetAware.perInterface.includesLoopback',
  ),
  containerCpu(MonitoringUnit.cpuCores, 'container.cpuCores.notHostPercent.resetAware.nonemptyImage.preserveIdentity'),
  containerMemory(MonitoringUnit.bytes, 'container.workingSetBytes.nonemptyImage.preserveIdentity'),
  postgresConnections(MonitoringUnit.connections, 'postgres.currentBackends.perDatabase'),
  postgresCommits(MonitoringUnit.transactionsPerSecond, 'postgres.commitsPerSecond.resetAware.perDatabase'),
  redisMemory(MonitoringUnit.bytes, 'redis.usedMemoryBytes'),
  redisClients(MonitoringUnit.connections, 'redis.connectedClients'),
  redisCommands(MonitoringUnit.operationsPerSecond, 'redis.commandsPerSecond.resetAware'),
  nginxConnections(MonitoringUnit.connections, 'nginx.activeConnections'),
  nginxRequests(MonitoringUnit.requestsPerSecond, 'nginx.requestsPerSecond.resetAware');

  const MonitoringMetric(this.unit, this.semantics);
  final MonitoringUnit unit;
  final String semantics;
  static MonitoringMetric fromMetadata(Json row) {
    final metric = values.byName(row['id'] as String);
    if (row['unit'] != metric.unit.name || row['semantics'] != metric.semantics) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
    }
    return metric;
  }
}

extension MonitoringPanelMetrics on MonitoringPanel {
  Set<MonitoringMetric> get metrics => switch (this) {
    MonitoringPanel.hostOverview => {
      MonitoringMetric.cpuBusy,
      MonitoringMetric.memoryUsedPercent,
      MonitoringMetric.memoryAvailable,
    },
    MonitoringPanel.cpu => {MonitoringMetric.cpuBusy},
    MonitoringPanel.memory => {MonitoringMetric.memoryUsedPercent, MonitoringMetric.memoryAvailable},
    MonitoringPanel.filesystem => {MonitoringMetric.filesystemUnavailablePercent},
    MonitoringPanel.diskIo => {MonitoringMetric.diskRead, MonitoringMetric.diskWrite},
    MonitoringPanel.network => {MonitoringMetric.networkReceive, MonitoringMetric.networkTransmit},
    MonitoringPanel.docker => {MonitoringMetric.containerCpu, MonitoringMetric.containerMemory},
    MonitoringPanel.postgreSql => {MonitoringMetric.postgresConnections, MonitoringMetric.postgresCommits},
    MonitoringPanel.redis => {
      MonitoringMetric.redisMemory,
      MonitoringMetric.redisClients,
      MonitoringMetric.redisCommands,
    },
    MonitoringPanel.nginx => {MonitoringMetric.nginxConnections, MonitoringMetric.nginxRequests},
  };
}

enum BindingReadState { resolved, missing, ambiguous, unavailable, ready, partial, noData }

enum BindingReadReason { discoveryFailed, discoveryInvalid, metricProbeFailed }

enum BindingPreparationReason { missingMetrics, probeFailed, selectorConflict, queryBudget }

class BindingPanelQuery {
  const BindingPanelQuery({required this.metric, this.outcome, this.error});
  final MonitoringMetric metric;
  final QueryOutcome? outcome;
  final MonitoringFailure? error;
  factory BindingPanelQuery.fromJson(Json json) => BindingPanelQuery(
    metric: MonitoringMetric.fromMetadata(json),
    outcome: json['outcome'] == null ? null : QueryOutcome.fromJson(object(json['outcome'])),
    error: json['error'] == null ? null : MonitoringFailure.fromJson(object(json['error'])),
  );
}

class BindingReadResult {
  const BindingReadResult({
    required this.bindingId,
    required this.bindingRevision,
    required this.state,
    required this.kind,
    this.panel,
    this.reason,
    this.preparationReason,
    this.discovery,
    this.probe,
    this.error,
    this.preparationError,
    this.limited = false,
    this.candidateIndices = const [],
    this.missingMetrics = const [],
    this.queries = const [],
  });
  final String bindingId, kind;
  final int bindingRevision;
  final BindingReadState state;
  final MonitoringPanel? panel;
  final BindingReadReason? reason;
  final BindingPreparationReason? preparationReason;
  final QueryOutcome? discovery, probe;
  final MonitoringFailure? error, preparationError;
  final bool limited;
  final List<int> candidateIndices;
  final List<String> missingMetrics;
  final List<BindingPanelQuery> queries;

  factory BindingReadResult.fromJson(Json json) {
    try {
      if (json.keys.toSet().difference({
            'kind',
            'bindingId',
            'bindingRevision',
            'state',
            'panel',
            'reason',
            'resolution',
            'discovery',
            'preparation',
            'error',
            'queries',
            'limited',
          }).isNotEmpty ||
          !['bindingDiscovery', 'bindingPanel'].contains(json['kind'])) {
        throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
      }
      final state = BindingReadState.values.byName(json['state'] as String);
      final isPanel = json['kind'] == 'bindingPanel';
      if ((!isPanel &&
              ![
                BindingReadState.resolved,
                BindingReadState.missing,
                BindingReadState.ambiguous,
                BindingReadState.unavailable,
              ].contains(state)) ||
          (isPanel && state == BindingReadState.resolved)) {
        throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
      }
      final resolution = json['resolution'] == null ? null : object(json['resolution']);
      final indices = switch (resolution?['state']) {
        'resolved' => [resolution!['candidateIndex'] as int],
        'ambiguous' => (resolution!['candidateIndices'] as List).cast<int>(),
        'missing' || null => <int>[],
        _ => throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse),
      };
      final resolved = [
        BindingReadState.resolved,
        BindingReadState.ready,
        BindingReadState.partial,
        BindingReadState.noData,
      ].contains(state);
      if ((resolved && (resolution?['state'] != 'resolved' || indices.length != 1)) ||
          state == BindingReadState.missing && resolution?['state'] != 'missing' ||
          state == BindingReadState.ambiguous && (resolution?['state'] != 'ambiguous' || indices.length < 2) ||
          indices.length > 1000 ||
          indices.any((i) => i < 0) ||
          indices.toSet().length != indices.length) {
        throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
      }
      final preparation = json['preparation'] == null ? null : object(json['preparation']);
      final planned = <MonitoringMetric>{};
      if (preparation != null) {
        if (!['ready', 'unavailable'].contains(preparation['state']) ||
            preparation.keys.toSet().difference({
              'state',
              'plan',
              'probe',
              'panel',
              'reason',
              'missingMetrics',
              'error',
            }).isNotEmpty) {
          throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
        }
        if (preparation['state'] == 'ready') {
          final plan = object(preparation['plan']);
          if (plan['panel'] != json['panel'] || plan['bindingRevision'] != json['bindingRevision']) {
            throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
          }
          final rows = plan['queries'] as List;
          if (rows.isEmpty || rows.length > 3) throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
          for (final row in rows) {
            planned.add(MonitoringMetric.fromMetadata(object(row)));
          }
          final expected = MonitoringPanel.values.byName(json['panel'] as String).metrics;
          if (planned.length != rows.length || planned.length != expected.length || !planned.containsAll(expected)) {
            throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
          }
        }
      }
      final queries = (json['queries'] as List? ?? []).map((r) => BindingPanelQuery.fromJson(object(r))).toList();
      if (queries.length > 3 ||
          queries.map((q) => q.metric).toSet().length != queries.length ||
          queries.any((q) => !planned.contains(q.metric) || q.outcome == null && q.error == null)) {
        throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
      }
      final discovery = json['discovery'] == null ? null : QueryOutcome.fromJson(object(json['discovery']));
      if (indices.isNotEmpty && (discovery?.data == null || indices.any((i) => i >= discovery!.data!.series.length))) {
        throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
      }
      return BindingReadResult(
        kind: json['kind'] as String,
        bindingId: json['bindingId'] as String,
        bindingRevision: json['bindingRevision'] as int,
        state: state,
        panel: isPanel ? MonitoringPanel.values.byName(json['panel'] as String) : null,
        reason: json['reason'] == null ? null : BindingReadReason.values.byName(json['reason'] as String),
        preparationReason: preparation?['reason'] == null
            ? null
            : BindingPreparationReason.values.byName(preparation!['reason'] as String),
        discovery: discovery,
        probe: preparation?['probe'] == null ? null : QueryOutcome.fromJson(object(preparation!['probe'])),
        error: json['error'] == null ? null : MonitoringFailure.fromJson(object(json['error'])),
        preparationError: preparation?['error'] == null
            ? null
            : MonitoringFailure.fromJson(object(preparation!['error'])),
        missingMetrics: List.unmodifiable(strings(preparation?['missingMetrics'])),
        candidateIndices: List.unmodifiable(indices),
        queries: List.unmodifiable(queries),
        limited: json['limited'] == true,
      );
    } catch (_) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
    }
  }
}
