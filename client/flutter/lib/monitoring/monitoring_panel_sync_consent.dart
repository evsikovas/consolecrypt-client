import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:flutter/foundation.dart';

/// The Native held review is the only action owner. No floors/keys/roster input.
abstract interface class MonitoringPanelSyncConsentHandle {
  Future<String> details();
  Future<String> confirm();
  Future<String> recheckCheckpoint();
  void cancel();
}

abstract interface class MonitoringPanelSyncConsentPort {
  Future<MonitoringPanelSyncConsentHandle> prepare(String requestJson);
}

abstract interface class MonitoringPanelSyncConsentCapability {
  MonitoringPanelSyncConsentPort? get panelSyncConsentPort;
}

Never _invalid() => throw const FormatException('Invalid held panel policy metadata');
void _keys(Json value, Set<String> keys) {
  if (value.length != keys.length || !value.keys.toSet().containsAll(keys)) _invalid();
}

Json _decode(String raw) {
  if (utf8.encode(raw).length > 131072) _invalid();
  return object(jsonDecode(raw));
}

final _uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
String _id(Object? value) {
  if (value is! String || !_uuid.hasMatch(value) || value == '00000000-0000-0000-0000-000000000000') _invalid();
  return value;
}

String _decimal(Object? value, {bool signed = false}) {
  if (value is! String || value.length > 20 || !RegExp(r'^(0|[1-9][0-9]*)$').hasMatch(value)) _invalid();
  final limit = BigInt.parse(signed ? '9223372036854775807' : '18446744073709551615');
  if (BigInt.parse(value) > limit) _invalid();
  return value;
}

String? _version(Object? value) {
  if (value == null) return null;
  if (value is! String ||
      value.length > 64 ||
      !RegExp(r'^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$').hasMatch(value)) {
    _invalid();
  }
  if (value.split('.').any((part) => BigInt.parse(part) > BigInt.parse('18446744073709551615'))) _invalid();
  return value;
}

void _scope(Json value, RuntimeNoticeScope original) {
  if (value['version'] is! int ||
      value['version'] != 1 ||
      value['profileId'] != original.profileId ||
      value['notificationScopeId'] != original.nonce ||
      value['scope'] != 'allPersonalVaults') {
    _invalid();
  }
}

typedef PanelSyncCheckpoint = ({String vaultId, String epoch, String sequence});
typedef PanelSyncCatalog = ({String settingsId, String objectRevision, int schema, String revision});
PanelSyncCheckpoint _checkpoint(Object? raw) {
  final value = object(raw);
  _keys(value, {'vaultId', 'vaultEpoch', 'vaultSequence'});
  return (
    vaultId: _id(value['vaultId']),
    epoch: _id(value['vaultEpoch']),
    sequence: _decimal(value['vaultSequence'], signed: true),
  );
}

PanelSyncCatalog _catalog(Object? raw) {
  final value = object(raw);
  _keys(value, {'settingsId', 'objectRevision', 'schema', 'revision'});
  final schema = value['schema'];
  if (schema is! int || schema < 1 || schema > 3) _invalid();
  if (schema > 1 && value['revision'] == '0') _invalid();
  return (
    settingsId: _id(value['settingsId']),
    objectRevision: _decimal(value['objectRevision'], signed: true),
    schema: schema,
    revision: _decimal(value['revision']),
  );
}

enum PanelSyncConsentKind { blocked, ownerConsent, checkpointRecapture }

final class PanelSyncConsentReview {
  const PanelSyncConsentReview._(
    this.scope,
    this.kind,
    this.accountId,
    this.policyRevision,
    this.rosterDigest,
    this.minimum,
    this.offered,
    this.snapshot,
    this.checkpoint,
    this.catalog,
    this.remainingMs,
    this.canConfirm,
    this.canRecheck,
  );
  final RuntimeNoticeScope scope;
  final PanelSyncConsentKind kind;
  final String accountId, policyRevision, rosterDigest;
  final String? minimum, offered;
  final MonitoringPanelSyncSnapshot snapshot;
  final PanelSyncCheckpoint checkpoint;
  final PanelSyncCatalog catalog;
  final int remainingMs;
  final bool canConfirm, canRecheck;
  factory PanelSyncConsentReview.parse(String raw, RuntimeNoticeScope original) {
    final value = _decode(raw);
    _keys(value, {
      'version',
      'profileId',
      'notificationScopeId',
      'kind',
      'scope',
      'accountId',
      'accountState',
      'policyRevision',
      'rosterDigest',
      'releasedMinimumClientVersion',
      'offeredMinimumClientVersion',
      'minimumClientVersion',
      'checkpoint',
      'catalog',
      'readiness',
      'remainingMs',
      'canConfirm',
      'canRecheck',
    });
    _scope(value, original);
    final digest = value['rosterDigest'];
    if (digest is! String || !RegExp(r'^[0-9a-f]{64}$').hasMatch(digest)) _invalid();
    final kind = PanelSyncConsentKind.values.byName(value['kind'] as String);
    final remaining = value['remainingMs'], confirm = value['canConfirm'], recheck = value['canRecheck'];
    if (remaining is! int || remaining <= 0 || remaining > 300000 || confirm is! bool || recheck is! bool) _invalid();
    final minimum = _version(value['minimumClientVersion']), offered = _version(value['offeredMinimumClientVersion']);
    final released = _version(value['releasedMinimumClientVersion']);
    final policyRevision = _decimal(value['policyRevision'], signed: true);
    final snapshot = MonitoringPanelSyncSnapshot.fromJson({
      'version': 1,
      'profileId': original.profileId,
      'notificationScopeId': original.nonce,
      'storage': 'synced',
      'releasedMinimumClientVersion': released,
      'readiness': value['readiness'],
      'server': 'available',
      'account': {'state': value['accountState'], 'minimumClientVersion': minimum},
    });
    final off = snapshot.account == PanelSyncAccount.off;
    final expectedConfirm =
        released != null && snapshot.readiness == PanelSyncReadiness.serverAdmissionUnverified && off;
    final emptyRoster = {
      PanelSyncReadiness.floorUndeclared,
      PanelSyncReadiness.rosterUnavailable,
      PanelSyncReadiness.rosterInvalid,
    }.contains(snapshot.readiness);
    if ((off && policyRevision != '0') ||
        (!off && policyRevision == '0') ||
        emptyRoster && (snapshot.checkedDevices != 0 || snapshot.blockedDevices != 0) ||
        snapshot.readiness == PanelSyncReadiness.devicesUnsupported &&
            (snapshot.checkedDevices == 0 || snapshot.blockedDevices == 0) ||
        snapshot.readiness == PanelSyncReadiness.serverAdmissionUnverified &&
            (snapshot.checkedDevices == 0 || snapshot.blockedDevices != 0) ||
        confirm != expectedConfirm ||
        recheck != (snapshot.account == PanelSyncAccount.enabled) ||
        confirm && recheck ||
        kind !=
            (confirm
                ? PanelSyncConsentKind.ownerConsent
                : recheck
                ? PanelSyncConsentKind.checkpointRecapture
                : PanelSyncConsentKind.blocked)) {
      _invalid();
    }
    return PanelSyncConsentReview._(
      original,
      kind,
      _id(value['accountId']),
      policyRevision,
      digest,
      minimum,
      offered,
      snapshot,
      _checkpoint(value['checkpoint']),
      _catalog(value['catalog']),
      remaining,
      confirm,
      recheck,
    );
  }
}

final class PanelSyncConsentResult {
  const PanelSyncConsentResult(this.rechecked, this.policyRevision, this.checkpoint, this.catalog);
  final bool rechecked;
  final String policyRevision;
  final PanelSyncCheckpoint? checkpoint;
  final PanelSyncCatalog? catalog;
  factory PanelSyncConsentResult.parse(String raw, PanelSyncConsentReview review, bool recheck) {
    final value = _decode(raw);
    _keys(value, {
      'version',
      'profileId',
      'notificationScopeId',
      'scope',
      'action',
      'accountId',
      'policyRevision',
      if (recheck) ...{'checkpoint', 'catalog'},
    });
    _scope(value, review.scope);
    final revision = _decimal(value['policyRevision'], signed: true);
    if (value['accountId'] != review.accountId ||
        value['action'] != (recheck ? 'checkpointRecaptured' : 'accountPolicyConfirmed') ||
        BigInt.parse(revision) != BigInt.parse(review.policyRevision) + (recheck ? BigInt.zero : BigInt.one)) {
      _invalid();
    }
    final checkpoint = recheck ? _checkpoint(value['checkpoint']) : null;
    if (recheck && (!review.canRecheck || checkpoint != review.checkpoint) || !recheck && !review.canConfirm) {
      _invalid();
    }
    return PanelSyncConsentResult(recheck, revision, checkpoint, recheck ? _catalog(value['catalog']) : null);
  }
}

enum PanelSyncConsentFailure { unavailable, invalid, expired, retired, outcomeUnknown }

/// One local owner of one Native review; observations never grant panel writes.
final class PanelSyncConsentController extends ChangeNotifier {
  PanelSyncConsentController({
    required this.port,
    required this.scope,
    required this.isOriginal,
    this.elapsedForReview,
  });
  final MonitoringPanelSyncConsentPort port;
  final RuntimeNoticeScope scope;
  final bool Function() isOriginal;
  final Duration Function()? elapsedForReview;
  MonitoringPanelSyncConsentHandle? _handle;
  Stopwatch? _clock;
  Timer? _timer;
  int _generation = 0;
  bool _disposed = false, _retired = false;
  bool busy = false, spent = false;
  PanelSyncConsentReview? review;
  PanelSyncConsentResult? result;
  PanelSyncConsentFailure? failure;
  bool get current => !_disposed && !_retired && isOriginal();
  int get remainingSeconds {
    final remaining = review?.remainingMs ?? 0;
    final elapsed = (elapsedForReview?.call() ?? _clock?.elapsed ?? Duration.zero).inMilliseconds;
    return ((remaining - elapsed).clamp(0, remaining) + 999) ~/ 1000;
  }

  bool get canConfirm => current && !busy && !spent && remainingSeconds > 0 && review?.canConfirm == true;
  bool get canRecheck => current && !busy && !spent && remainingSeconds > 0 && review?.canRecheck == true;
  void _emit() {
    if (!_disposed) notifyListeners();
  }

  void _cancel(MonitoringPanelSyncConsentHandle? handle) {
    try {
      handle?.cancel();
    } catch (_) {}
  }

  void _stop() {
    _timer?.cancel();
    _timer = null;
  }

  void _tick() {
    _stop();
    if (!current) {
      retire();
      return;
    }
    if (review == null || spent) return;
    if (remainingSeconds == 0) {
      _cancel(_handle);
      _handle = null;
      review = null;
      failure = PanelSyncConsentFailure.expired;
      _emit();
      return;
    }
    _timer = Timer(const Duration(seconds: 1), () {
      _emit();
      _tick();
    });
  }

  bool _same(int generation) {
    if (!current) {
      retire();
      return false;
    }
    return generation == _generation;
  }

  Future<void> prepare() async {
    if (!current || busy || spent || review != null) return;
    final generation = ++_generation;
    busy = true;
    failure = null;
    _emit();
    MonitoringPanelSyncConsentHandle? issued;
    var retained = false;
    void releaseIssued() {
      if (!retained) {
        _cancel(issued);
      } else if (identical(_handle, issued)) {
        _handle = null;
        _cancel(issued);
      }
    }

    try {
      issued = await port.prepare(
        jsonEncode({'version': 1, 'profileId': scope.profileId, 'notificationScopeId': scope.nonce}),
      );
      if (!_same(generation)) {
        releaseIssued();
        return;
      }
      // Retain the original ACK before async details so close/retire can cancel it now.
      _handle = issued;
      retained = true;
      final raw = await issued.details();
      if (!_same(generation)) {
        releaseIssued();
        return;
      }
      final parsed = PanelSyncConsentReview.parse(raw, scope);
      _handle = issued;
      review = parsed;
      _clock = Stopwatch()..start();
      _tick();
    } catch (error) {
      releaseIssued();
      if (_same(generation)) {
        failure = error is FormatException ? PanelSyncConsentFailure.invalid : PanelSyncConsentFailure.unavailable;
      }
    } finally {
      if (_same(generation)) {
        busy = false;
        _emit();
      }
    }
  }

  Future<void> act({required bool recheck}) async {
    if (!(recheck ? canRecheck : canConfirm)) return;
    final original = _handle!, accepted = review!, generation = _generation;
    spent = true;
    busy = true;
    _stop();
    _emit();
    try {
      final raw = await (recheck ? original.recheckCheckpoint() : original.confirm());
      if (_same(generation)) result = PanelSyncConsentResult.parse(raw, accepted, recheck);
    } catch (_) {
      if (_same(generation)) failure = PanelSyncConsentFailure.outcomeUnknown;
    } finally {
      if (_same(generation)) {
        busy = false;
        _emit();
      }
    }
  }

  void retire({bool notify = true}) {
    if (_retired) return;
    _retired = true;
    _generation++;
    _stop();
    _cancel(_handle);
    _handle = null;
    review = null;
    result = null;
    busy = false;
    failure = PanelSyncConsentFailure.retired;
    if (notify) _emit();
  }

  @override
  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _generation++;
    _stop();
    _cancel(_handle);
    _handle = null;
    review = null;
    result = null;
    super.dispose();
  }
}
