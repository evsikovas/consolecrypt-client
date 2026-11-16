import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';

export 'package:consolecrypt/automation/automation_service_delegation_service.dart';

final _uuid = RegExp(
  r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$',
);
final _sha = RegExp(r'^[0-9a-f]{64}$');
bool _id(Object? value) =>
    value is String &&
    _uuid.hasMatch(value) &&
    value != '00000000-0000-0000-0000-000000000000';
bool _revision(Object? value) =>
    value is int && value > 0 && value <= 9007199254740991;
Never _invalid() =>
    throw const FormatException('Service context unavailable or changed');
AutomationJson _map(Object? value) =>
    value is Map<String, dynamic> ? value : _invalid();
bool _keys(AutomationJson value, Set<String> keys) =>
    value.length == keys.length && value.keys.every(keys.contains);
String _text(Object? value, int limit) =>
    value is String &&
        value.isNotEmpty &&
        value.length <= limit &&
        !value.runes.any((r) => r < 32 || r == 127)
    ? value
    : _invalid();
DateTime _utc(Object? value) {
  if (value is! String || value.length > 32) _invalid();
  final match = RegExp(
    r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?Z$',
  ).firstMatch(value);
  if (match == null) _invalid();
  final parsed = DateTime.tryParse(value);
  if (parsed == null ||
      !parsed.isUtc ||
      parsed.year != int.parse(match.group(1)!) ||
      parsed.month != int.parse(match.group(2)!) ||
      parsed.day != int.parse(match.group(3)!) ||
      parsed.hour != int.parse(match.group(4)!) ||
      parsed.minute != int.parse(match.group(5)!) ||
      parsed.second != int.parse(match.group(6)!)) {
    _invalid();
  }
  return parsed;
}

dynamic _freeze(dynamic value) => switch (value) {
  Map<String, dynamic>() => Map<String, dynamic>.unmodifiable(
    value.map((key, child) => MapEntry(key, _freeze(child))),
  ),
  List() => List<dynamic>.unmodifiable(value.map(_freeze)),
  _ => value,
};
AutomationJson _snapshot(AutomationJson value) {
  final encoded = jsonEncode(value);
  if (utf8.encode(encoded).length > 1024 * 1024) _invalid();
  return _freeze(jsonDecode(encoded)) as AutomationJson;
}

bool _same(Object? a, Object? b) {
  if (a is Map && b is Map) {
    return a.length == b.length &&
        a.keys.every((key) => b.containsKey(key) && _same(a[key], b[key]));
  }
  if (a is List && b is List) {
    return a.length == b.length &&
        List.generate(a.length, (i) => i).every((i) => _same(a[i], b[i]));
  }
  return a == b;
}

/// Captured from the actual existing Hub proof before any asynchronous work.
/// It is request data; Native independently checks the original unlock.
final class ServiceDelegationScope {
  ServiceDelegationScope(this.profileId, this.notificationScopeId) {
    if (!_id(profileId) ||
        notificationScopeId.isEmpty ||
        utf8.encode(notificationScopeId).length > 256 ||
        notificationScopeId.runes.any(
          (value) => value < 32 || value >= 127 && value <= 159,
        )) {
      _invalid();
    }
  }
  final String profileId, notificationScopeId;
  AutomationJson get frame => {
    'version': 1,
    'profileId': profileId,
    'notificationScopeId': notificationScopeId,
  };
  bool matches(AutomationJson value) =>
      value['version'] == 1 &&
      value['profileId'] == profileId &&
      value['notificationScopeId'] == notificationScopeId;
}

/// An immutable UI intent, distinct from a prepared Native service consent.
final class ServiceDelegationIntent {
  ServiceDelegationIntent({
    required this.scope,
    required this.runnerId,
    required this.identityId,
    required this.runnerRevision,
    required this.projectRevision,
    required AutomationJson run,
  }) : run = _snapshot(run) {
    if (!_id(runnerId) ||
        !_id(identityId) ||
        !_revision(runnerRevision) ||
        !_revision(projectRevision) ||
        run['runner_id'] != runnerId ||
        !_id(run['project_id']) ||
        run['cancel_on_lock'] != false ||
        !{'check', 'apply'}.contains(run['mode']) ||
        run.containsKey('retry_of') ||
        run.containsKey('vault_credential_id') ||
        run.containsKey('vault_selection') ||
        _map(run['parameters']).containsKey('vault')) {
      _invalid();
    }
    final hosts = run['host_ids'];
    if (hosts is! List ||
        hosts.isEmpty ||
        hosts.length > 32 ||
        hosts.any((v) => !_id(v)) ||
        hosts.toSet().length != hosts.length) {
      _invalid();
    }
  }
  final ServiceDelegationScope scope;
  final String runnerId, identityId;
  final int runnerRevision, projectRevision;
  final AutomationJson run;
  AutomationJson prepare({
    required String zone,
    required int hour,
    required int minute,
    required List<int> weekdays,
    required int validitySeconds,
    required int maxJobs,
    required int maxPerUtcDay,
  }) => {
    ...scope.frame,
    'expectedRunnerRevision': runnerRevision,
    'expectedProjectRevision': projectRevision,
    'run': run,
    'schedule': {
      'zone': zone,
      'hour': hour,
      'minute': minute,
      'weekdays': List<int>.of(weekdays)..sort(),
    },
    'validitySeconds': validitySeconds,
    'maxJobs': maxJobs,
    'maxPerUtcDay': maxPerUtcDay,
  };
  AutomationJson get statusRequest => {
    ...scope.frame,
    'runnerId': runnerId,
    'expectedRunnerRevision': runnerRevision,
  };
}

final class ServiceDelegationPreview {
  ServiceDelegationPreview._(
    this.intent,
    this.review,
    this.policy,
    this.service,
    this.schedule,
    this.expiresUtc,
    this.consentRemainingMs,
  );
  factory ServiceDelegationPreview.parse(
    AutomationJson raw,
    ServiceDelegationIntent intent,
    AutomationJson selected,
  ) {
    final value = _snapshot(raw);
    if (!_keys(value, {
          'version',
          'profileId',
          'notificationScopeId',
          'review',
          'policy',
          'service',
          'schedule',
          'expiresUtc',
          'consentRemainingMs',
        }) ||
        !intent.scope.matches(value)) {
      _invalid();
    }
    final review = _map(value['review']),
        policy = _map(value['policy']),
        service = _map(value['service']),
        schedule = _map(value['schedule']);
    final remaining = value['consentRemainingMs'];
    if (remaining is! int ||
        remaining < 1 ||
        remaining > 60000 ||
        !_id(review['review_id']) ||
        review['project_revision'] != intent.projectRevision ||
        review['runner_id'] != intent.runnerId ||
        review['playbook'] != intent.run['playbook'] ||
        review['mode'] != intent.run['mode'] ||
        !_same(review['parameters'], intent.run['parameters']) ||
        _map(review['validation'])['valid'] != true ||
        _map(review['policy'])['production'] != false ||
        !_keys(policy, {
          'version',
          'id',
          'owner_id',
          'owner_epoch',
          'tenant',
          'service_id',
          'signer',
          'recipient',
          'project_id',
          'project_revision',
          'template_digest',
          'playbook',
          'schedule_id',
          'schedule_revision',
          'definition_digest',
          'inventory_digest',
          'parameters_digest',
          'environment_digest',
          'hosts',
          'credentials',
          'route_inputs',
          'modes',
          'quotas',
          'policy_revision',
          'revocation_epoch',
          'issued_at',
          'expires_at',
          'desktop_lock_policy',
          'access_policy',
          'execution_origin',
        }) ||
        policy['version'] != 4 ||
        !_id(policy['id']) ||
        !_id(policy['owner_id']) ||
        !_revision(policy['owner_epoch']) ||
        !_revision(policy['policy_revision']) ||
        !_revision(policy['revocation_epoch']) ||
        policy['service_id'] != intent.runnerId ||
        policy['project_id'] != intent.run['project_id'] ||
        policy['project_revision'] != intent.projectRevision ||
        policy['playbook'] != intent.run['playbook'] ||
        policy['desktop_lock_policy'] != 'independentDelegatedService') {
      _invalid();
    }
    final access = _map(policy['access_policy']);
    if (access['mode'] != 'boundedOffline' ||
        access['offline_not_after'] != policy['expires_at'] ||
        policy['issued_at'] is! int ||
        policy['expires_at'] is! int ||
        (policy['expires_at'] as int) <= (policy['issued_at'] as int) ||
        (policy['expires_at'] as int) - (policy['issued_at'] as int) > 86400) {
      _invalid();
    }
    final tenant = _map(policy['tenant']),
        origin = _map(policy['execution_origin']);
    if (!_keys(tenant, {'kind', 'id'}) ||
        !{'localProfile', 'personalAccount'}.contains(tenant['kind']) ||
        !_id(tenant['id']) ||
        tenant['kind'] == 'localProfile' &&
            tenant['id'] != intent.scope.profileId ||
        origin['service_id'] != intent.runnerId ||
        origin['runner_revision'] != intent.runnerRevision ||
        origin['backend'] != 'linuxNativeSsh' ||
        origin['network'] != 'serviceLocal' ||
        !_same(policy['modes'], [intent.run['mode']])) {
      _invalid();
    }
    for (final key in ['signer', 'recipient']) {
      final public = _map(policy[key]);
      if (public['kind'] != (key == 'signer' ? 'ed25519' : 'x25519') ||
          public['key_epoch'] != 1 ||
          !_revision(public['certificate_epoch']) ||
          public['public_key'] is! String ||
          !_sha.hasMatch(public['public_key'] as String)) {
        _invalid();
      }
    }
    final expires = _utc(value['expiresUtc']);
    if (expires.millisecondsSinceEpoch ~/ 1000 != policy['expires_at']) {
      _invalid();
    }
    final validation = _map(review['validation']);
    for (final entry in {
      'template_digest': _map(review['revision'])['digest'],
      'inventory_digest': review['inventory_digest'],
      'parameters_digest': validation['parameters_digest'],
      'environment_digest': validation['environment_digest'],
    }.entries) {
      if (policy[entry.key] is! String ||
          !_sha.hasMatch(policy[entry.key] as String) ||
          policy[entry.key] != entry.value) {
        _invalid();
      }
    }
    if (!_keys(service, {
          'runnerId',
          'runnerRevision',
          'identityId',
          'namespace',
          'signingFingerprint',
          'recipientFingerprint',
          'keyEpoch',
          'serviceState',
        }) ||
        service['runnerId'] != intent.runnerId ||
        service['runnerRevision'] != intent.runnerRevision ||
        service['identityId'] != intent.identityId ||
        service['serviceState'] != 'pending' ||
        service['keyEpoch'] != 1 ||
        service['namespace'] is! String ||
        !RegExp(r'^cc-service-local-v1-[0-9a-f]{64}$')
            .hasMatch(service['namespace'] as String) ||
        service['signingFingerprint'] is! String ||
        !_sha.hasMatch(service['signingFingerprint'] as String) ||
        service['recipientFingerprint'] is! String ||
        !_sha.hasMatch(service['recipientFingerprint'] as String)) {
      _invalid();
    }
    final targets = review['targets'],
        hosts = policy['hosts'],
        credentials = policy['credentials'];
    if (targets is! List ||
        targets.isEmpty ||
        targets.length > 32 ||
        hosts is! List ||
        hosts.length != targets.length ||
        credentials is! List ||
        credentials.length > 64) {
      _invalid();
    }
    final actualHosts = targets.map((v) => _map(v)['host_id']).toSet();
    if (actualHosts.length != targets.length ||
        !actualHosts.containsAll(intent.run['host_ids'] as List) ||
        actualHosts.length != (intent.run['host_ids'] as List).length ||
        hosts.map((v) => _map(v)['id']).toSet().length != hosts.length ||
        hosts.any((v) => !actualHosts.contains(_map(v)['id']))) {
      _invalid();
    }
    for (final value in credentials) {
      final row = _map(value);
      if (!_keys(row, {'id', 'revision'}) ||
          !_id(row['id']) ||
          !_revision(row['revision'])) {
        _invalid();
      }
    }
    if (!_keys(schedule, {
          'version',
          'zone',
          'hour',
          'minute',
          'weekdays',
          'graceSeconds',
          'gapPolicy',
          'foldPolicy',
          'missedPolicy',
        }) ||
        schedule['version'] != 1 ||
        schedule['graceSeconds'] != 60 ||
        schedule['gapPolicy'] != 'skip' ||
        schedule['foldPolicy'] != 'first' ||
        schedule['missedPolicy'] != 'skip') {
      _invalid();
    }
    _text(schedule['zone'], 128);
    final days = schedule['weekdays'];
    if (schedule['hour'] is! int ||
        (schedule['hour'] as int) < 0 ||
        (schedule['hour'] as int) > 23 ||
        schedule['minute'] is! int ||
        (schedule['minute'] as int) < 0 ||
        (schedule['minute'] as int) > 59 ||
        days is! List ||
        days.isEmpty ||
        days.length > 7 ||
        days.toSet().length != days.length ||
        days.any((v) => v is! int || v < 0 || v > 6)) {
      _invalid();
    }
    final chosen = _map(selected['schedule']);
    for (final key in ['zone', 'hour', 'minute', 'weekdays']) {
      if (!_same(schedule[key], chosen[key])) _invalid();
    }
    final quotas = _map(policy['quotas']);
    if (quotas['max_concurrent'] != 1 ||
        quotas['max_job_seconds'] != intent.run['timeout_seconds'] ||
        quotas['max_forks'] != intent.run['forks'] ||
        !_revision(quotas['max_jobs']) ||
        !_revision(quotas['max_per_utc_day']) ||
        selected['maxJobs'] is! int ||
        selected['maxPerUtcDay'] is! int ||
        selected['validitySeconds'] is! int ||
        (quotas['max_jobs'] as int) > (selected['maxJobs'] as int) ||
        (quotas['max_per_utc_day'] as int) >
            (selected['maxPerUtcDay'] as int) ||
        (policy['expires_at'] as int) - (policy['issued_at'] as int) >
            (selected['validitySeconds'] as int)) {
      _invalid();
    }
    return ServiceDelegationPreview._(
      intent,
      review,
      policy,
      service,
      schedule,
      expires,
      remaining,
    );
  }
  final ServiceDelegationIntent intent;
  final AutomationJson review, policy, service, schedule;
  final DateTime expiresUtc;
  final int consentRemainingMs;
  String get reviewId => review['review_id'] as String;
  String get policyId => policy['id'] as String;
  AutomationJson get confirmation => {
    ...intent.scope.frame,
    'reviewId': reviewId,
    'policyId': policyId,
  };
  AutomationJson get cancellation => {
    ...intent.scope.frame,
    'reviewId': reviewId,
  };
}

final class ServiceDelegationStatus {
  ServiceDelegationStatus._(
    this.localState,
    this.remoteState,
    this.policyRevision,
    this.revocationEpoch,
    this.expiresUtc,
  );
  factory ServiceDelegationStatus.parse(
    AutomationJson raw,
    ServiceDelegationScope scope,
    String runnerId,
  ) {
    final value = _snapshot(raw);
    if (!_keys(value, {
          'version',
          'profileId',
          'notificationScopeId',
          'runnerId',
          'serviceId',
          'localState',
          'remoteState',
          'policyRevision',
          'revocationEpoch',
          'expiresUtc',
        }) ||
        !scope.matches(value) ||
        value['runnerId'] != runnerId ||
        value['serviceId'] != runnerId ||
        !{
          'absent',
          'pendingEnrollment',
          'deliveryUnknown',
          'acknowledged',
          'localBlocked',
        }.contains(value['localState']) ||
        !{
          'unknown',
          'unbound',
          'pending',
          'enrolled',
          'restored',
          'revocationRecorded',
        }.contains(value['remoteState']) ||
        value['policyRevision'] != null &&
            !_revision(value['policyRevision']) ||
        value['revocationEpoch'] != null &&
            !_revision(value['revocationEpoch'])) {
      _invalid();
    }
    return ServiceDelegationStatus._(
      value['localState'] as String,
      value['remoteState'] as String,
      value['policyRevision'] as int?,
      value['revocationEpoch'] as int?,
      value['expiresUtc'] == null ? null : _utc(value['expiresUtc']),
    );
  }
  final String localState, remoteState;
  final int? policyRevision, revocationEpoch;
  final DateTime? expiresUtc;
  bool get enrollmentAcknowledged =>
      localState == 'acknowledged' &&
      remoteState == 'enrolled' &&
      policyRevision != null &&
      revocationEpoch != null &&
      expiresUtc != null;
}
