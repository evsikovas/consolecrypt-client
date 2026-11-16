import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_models.dart';

/// User display metadata only. Never attach this to metric-proven units or
/// convert numeric samples based on this value.
enum SavedPanelUnitHint { unknown, percent, bytes, bytesPerSecond, seconds, count, perSecond }

enum SavedPanelState { current, staleSource, missingSource }

Never _invalid() => throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
void _keys(Json row, Set<String> keys) {
  if (row.length != keys.length || !row.keys.toSet().containsAll(keys)) _invalid();
}

bool _id(String id) =>
    RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(id) &&
    id != '00000000-0000-0000-0000-000000000000';

class SavedPanelDefinition {
  const SavedPanelDefinition({
    required this.title,
    required this.sourceId,
    required this.sourceRevision,
    required this.query,
    required this.unitHint,
  });
  final String title, sourceId, query;
  final int sourceRevision;
  final SavedPanelUnitHint unitHint;
  factory SavedPanelDefinition.fromJson(Json json) {
    _keys(json, {'title', 'sourceId', 'sourceRevision', 'query', 'unitHint'});
    final value = SavedPanelDefinition(
      title: json['title'] as String,
      sourceId: json['sourceId'] as String,
      sourceRevision: json['sourceRevision'] as int,
      query: json['query'] as String,
      unitHint: SavedPanelUnitHint.values.byName(json['unitHint'] as String),
    );
    value.validate();
    return value;
  }
  void validate() {
    bool control(int r) => r < 32 || r >= 127 && r <= 159;
    if (!_id(sourceId) ||
        sourceRevision < 1 ||
        title.isEmpty ||
        title.trim() != title ||
        title.runes.length > 120 ||
        title.runes.any(control) ||
        query.trim().isEmpty ||
        utf8.encode(query).length > 16384 ||
        query.runes.any((r) => control(r) && r != 9 && r != 10 && r != 13)) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidQuery);
    }
  }

  Json toJson() => {
    'title': title,
    'sourceId': sourceId,
    'sourceRevision': sourceRevision,
    'query': query,
    'unitHint': unitHint.name,
  };
}

class SavedMonitoringPanel {
  const SavedMonitoringPanel({required this.id, required this.revision, required this.definition, required this.state});
  final String id;
  final int revision;
  final SavedPanelDefinition definition;
  final SavedPanelState state;
  factory SavedMonitoringPanel.fromJson(Json json) {
    _keys(json, {'id', 'revision', 'definition', 'state'});
    final id = json['id'] as String, revision = json['revision'] as int;
    if (!_id(id) || revision < 1) _invalid();
    return SavedMonitoringPanel(
      id: id,
      revision: revision,
      definition: SavedPanelDefinition.fromJson(object(json['definition'])),
      state: SavedPanelState.values.byName(json['state'] as String),
    );
  }
  bool currentFor(MonitoringSource source) =>
      state == SavedPanelState.current &&
      definition.sourceId == source.id &&
      definition.sourceRevision == source.revision;
}

class SavedPanelRange {
  const SavedPanelRange({required this.start, required this.end, required this.stepSeconds});
  final double start, end, stepSeconds;
  Json toJson() {
    if (!start.isFinite ||
        !end.isFinite ||
        start.abs() > 253402300799 ||
        end.abs() > 253402300799 ||
        end < start ||
        !stepSeconds.isFinite ||
        stepSeconds <= 0) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidQuery);
    }
    return {'start': start, 'end': end, 'stepSeconds': stepSeconds};
  }
}

class SavedPanelRead {
  const SavedPanelRead(this.outcome, this.unitHint);
  final QueryOutcome outcome;
  final SavedPanelUnitHint unitHint;
  factory SavedPanelRead.fromJson(Json json, SavedMonitoringPanel panel) {
    _keys(json, {'kind', 'panelId', 'panelRevision', 'unitHint', 'outcome'});
    if (json['kind'] != 'savedPanel' ||
        json['panelId'] != panel.id ||
        json['panelRevision'] != panel.revision ||
        json['unitHint'] != panel.definition.unitHint.name) {
      _invalid();
    }
    final outcome = object(json['outcome']);
    if (outcome['error'] != null && outcome['status'] == null) {
      throw MonitoringFailure.fromJson(object(outcome['error']));
    }
    return SavedPanelRead(QueryOutcome.fromJson(outcome), panel.definition.unitHint);
  }
}


/// Diagnostic metadata only. Deliberately has no Ready or write-permission field.
enum PanelSyncStorage { local, synced }
enum PanelSyncReadiness { localOnly, floorUndeclared, rosterUnavailable, rosterInvalid, devicesUnsupported, serverAdmissionUnverified }
enum PanelSyncServer { notChecked, available, unsupported, unavailable, timedOut, authenticationRequired, deviceRevoked, upgradeRequired, quarantined, proofRequired, invalid, refused }
enum PanelSyncAccount { off, enabled, quarantined }
enum PanelSyncBlockerReason { unknownVersion, invalidVersion, prereleaseVersion, olderVersion }
final class PanelSyncDeviceBlocker {
  const PanelSyncDeviceBlocker(this.deviceId,this.reason);
  final String deviceId;
  final PanelSyncBlockerReason reason;
}

final class MonitoringPanelSyncSnapshot {
  const MonitoringPanelSyncSnapshot._(this.profileId, this.notificationScopeId, this.storage, this.releasedFloor,
    this.readiness, this.checkedDevices, this.blockedDevices, this.server, this.account, this.accountFloor, this.deviceBlockers);
  final String profileId, notificationScopeId;
  final PanelSyncStorage storage;
  final String? releasedFloor, accountFloor;
  final PanelSyncReadiness readiness;
  final int checkedDevices, blockedDevices;
  final List<PanelSyncDeviceBlocker> deviceBlockers;
  final PanelSyncServer server;
  final PanelSyncAccount? account;
  bool matches(String profile, String nonce) => profileId == profile && notificationScopeId == nonce;
  factory MonitoringPanelSyncSnapshot.fromJson(Json json) {
    _keys(json, {'version','profileId','notificationScopeId','storage','releasedMinimumClientVersion','readiness','server','account'});
    if (json['version'] is! int || json['version'] != 1 || json['profileId'] is! String || json['notificationScopeId'] is! String ||
        (json['profileId'] as String).length != 36 ||
        !_id(json['profileId'] as String) || (json['notificationScopeId'] as String).isEmpty ||
        utf8.encode(json['notificationScopeId'] as String).length>256 ||
        (json['notificationScopeId'] as String).runes.any((r)=>r<32||r>=127&&r<=159)) {
      _invalid();
    }
    String? floor(Object? raw) {
      if (raw == null) return null;
      if (raw is! String || raw.length > 64 || raw.trim() != raw || !RegExp(r'^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$').hasMatch(raw)) _invalid();
      return raw;
    }
    final storage = PanelSyncStorage.values.byName(json['storage'] as String);
    final released = floor(json['releasedMinimumClientVersion']);
    final readiness = object(json['readiness']);
    _keys(readiness, {'state','checkedActiveDevices','blockedDevices'});
    final state = PanelSyncReadiness.values.byName(readiness['state'] as String);
    final checked = readiness['checkedActiveDevices'], blocked = readiness['blockedDevices'];
    if (checked is! int || checked < 0 || checked > 256 || blocked is! List || blocked.length > checked) _invalid();
    final ids = <String>{};
    final reasons=<PanelSyncDeviceBlocker>[];
    for (final raw in blocked) {
      final row=object(raw); _keys(row, {'deviceId','reason'});
      if (row['deviceId'] is! String || !_id(row['deviceId'] as String) || !ids.add(row['deviceId'] as String) ||
          !{'unknownVersion','invalidVersion','prereleaseVersion','olderVersion'}.contains(row['reason'])) {
        _invalid();
      }
      reasons.add(PanelSyncDeviceBlocker(row['deviceId'] as String,PanelSyncBlockerReason.values.byName(row['reason'] as String)));
    }
    final server = PanelSyncServer.values.byName(json['server'] as String);
    PanelSyncAccount? account;
    String? minimum;
    if (json['account'] != null) {
      final raw=object(json['account']); _keys(raw, {'state','minimumClientVersion'});
      account=PanelSyncAccount.values.byName(raw['state'] as String); minimum=floor(raw['minimumClientVersion']);
      if ((account == PanelSyncAccount.off) != (minimum == null)) _invalid();
    }
    if ((server == PanelSyncServer.available) != (account != null) ||
        storage == PanelSyncStorage.local && (state != PanelSyncReadiness.localOnly || server != PanelSyncServer.notChecked || account != null) ||
        storage == PanelSyncStorage.synced && (state == PanelSyncReadiness.localOnly ||
          (released == null) != (state == PanelSyncReadiness.floorUndeclared))) {
      _invalid();
    }
    return MonitoringPanelSyncSnapshot._(json['profileId'] as String,json['notificationScopeId'] as String,storage,released,
      state,checked,blocked.length,server,account,minimum,List.unmodifiable(reasons));
  }
}
