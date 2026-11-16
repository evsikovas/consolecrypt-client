import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/monitoring/agent_reader_models.dart';

/// Display metadata from an already consumed authenticated read, never readiness
/// or installation evidence. No transport, grant, persistence or refresh exists.
final class AgentHeartbeatObservation {
  AgentHeartbeatObservation._(
    this.selection,
    this.association,
    this.dataset,
    this.receivedAt,
    this.host,
    this.scopeOwner,
    this.epoch,
  );
  final AgentReaderSelection selection;
  final AgentReaderLocator association;
  final AgentReaderDataset dataset;
  final DateTime receivedAt;
  final Host host;
  final Object scopeOwner;
  final Object? epoch;
}

abstract interface class AgentHeartbeatObservationCapability {
  AgentHeartbeatObservations get agentHeartbeatObservations;
}

/// Owned only by the original profile's MonitoringService. A changed frame,
/// service, Host snapshot or scope owner cannot reuse an earlier observation.
final class AgentHeartbeatObservations {
  final _byHost = <String, AgentHeartbeatObservation>{};
  void record({
    required AgentReaderSelection selection,
    required AgentReaderLocator association,
    required AgentReaderDataset dataset,
    required Host host,
    required Object scopeOwner,
    required Object? epoch,
    required DateTime receivedAt,
  }) {
    if (dataset.kind != 'heartbeatV1' ||
        dataset.agentVersion == null ||
        dataset.agentId == null ||
        dataset.hostId != selection.hostId ||
        host.id.value != selection.hostId) {
      return;
    }
    if (!_byHost.containsKey(selection.hostId) && _byHost.length >= 64) _byHost.remove(_byHost.keys.first);
    _byHost[selection.hostId] = AgentHeartbeatObservation._(
      selection,
      association,
      dataset,
      receivedAt.toUtc(),
      host,
      scopeOwner,
      epoch,
    );
  }

  AgentHeartbeatObservation? read({
    required Host host,
    required String profileId,
    required String notificationScopeId,
    required Object scopeOwner,
    required Object? epoch,
  }) {
    final value = _byHost[host.id.value];
    if (value == null) return null;
    if (!identical(value.host, host) ||
        !identical(value.scopeOwner, scopeOwner) ||
        value.epoch != epoch ||
        value.selection.profileId != profileId ||
        value.selection.notificationScopeId != notificationScopeId) {
      _byHost.remove(host.id.value);
      return null;
    }
    return value;
  }

  void clear() => _byHost.clear();
}
