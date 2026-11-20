import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:material_ui/material_ui.dart';

Never _bad() => throw const FormatException('Recorded job audit unavailable');
AutomationJson _map(Object? v) => v is AutomationJson ? v : _bad();
AutomationJson _keys(Object? v, Set<String> keys) {
  final m = _map(v);
  if (m.length != keys.length || !m.keys.every(keys.contains)) _bad();
  return m;
}

String _id(Object? v) =>
    v is String &&
        RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(v) &&
        v != '00000000-0000-0000-0000-000000000000'
    ? v
    : _bad();
String _sha(Object? v) => v is String && RegExp(r'^[0-9a-f]{64}$').hasMatch(v) ? v : _bad();
String _choice(Object? v, Set<String> allowed) => v is String && allowed.contains(v) ? v : _bad();

final class AutomationAuditTarget {
  const AutomationAuditTarget(this.hostId, this.routeDigest);
  final String hostId, routeDigest;
}

/// Native recorded metadata, never permissions, an agreement or a signed log.
final class AutomationJobAdmissionAudit {
  const AutomationJobAdmissionAudit._({
    required this.status,
    this.reason,
    this.jobId,
    this.manifestDigest,
    this.signerId,
    this.projectId,
    this.revisionDigest,
    this.runnerId,
    this.issuedAt,
    this.production,
    this.ownerEvidenceDigest,
    this.targets = const [],
  });
  final String status;
  final String? reason, jobId, manifestDigest, signerId, projectId, revisionDigest, runnerId, ownerEvidenceDigest;
  final int? issuedAt;
  final bool? production;
  final List<AutomationAuditTarget> targets;
}

AutomationJobAdmissionAudit? parseAutomationJobAdmissionAudit(AutomationJson job) {
  if (!job.containsKey('admission_audit')) return null;
  final raw = _map(job['admission_audit']);
  if (raw['version'] is! int || raw['version'] != 1) _bad();
  final status = _choice(raw['status'], {'recorded', 'unrecorded', 'unavailable'});
  if (status != 'recorded') {
    _keys(raw, {'version', 'status', 'reason'});
    final reason = _choice(
      raw['reason'],
      status == 'unrecorded' ? {'legacy_native_provenance', 'runner_unrecorded', 'not_observed'} : {'binding_mismatch'},
    );
    return AutomationJobAdmissionAudit._(status: status, reason: reason);
  }
  _keys(raw, {
    'version',
    'status',
    'jobId',
    'manifestDigest',
    'signerId',
    'scope',
    'projectId',
    'revisionDigest',
    'runnerId',
    'issuedAt',
    'production',
    'ownerEvidenceDigest',
    'targets',
  });
  final jobId = _id(raw['jobId']),
      manifestDigest = _sha(raw['manifestDigest']),
      signer = _id(raw['signerId']),
      project = _id(raw['projectId']),
      revision = _sha(raw['revisionDigest']),
      runner = _id(raw['runnerId']);
  final issued = raw['issuedAt'],
      production = raw['production'],
      owner = raw['ownerEvidenceDigest'],
      targets = raw['targets'];
  if (raw['scope'] != 'jobs.execute' ||
      issued is! int ||
      issued < 1 ||
      issued > 253402300799 ||
      production is! bool ||
      (owner != null) != production ||
      targets is! List ||
      targets.isEmpty ||
      targets.length > 512) {
    _bad();
  }
  final ownerDigest = owner == null ? null : _sha(owner), seen = <String>{};
  final kept = <AutomationAuditTarget>[];
  for (final value in targets) {
    final t = _keys(value, {'hostId', 'routeDigest'}), id = _id(t['hostId']);
    if (!seen.add(id)) _bad();
    kept.add(AutomationAuditTarget(id, _sha(t['routeDigest'])));
  }
  // Encode only AFTER the closed bounded structure/leaves have been checked.
  if (utf8.encode(jsonEncode(raw)).length > 65536) _bad();
  final manifest = _map(job['manifest']),
      provenance = _map(job['correlation_provenance']),
      hosts = job['host_ids'],
      approved = manifest['targets'];
  if (_id(job['job_id']) != jobId ||
      _sha(job['manifest_digest']) != manifestDigest ||
      _id(job['project_id']) != project ||
      _id(job['runner_id']) != runner ||
      manifest['version'] is! int ||
      manifest['version'] != 1 ||
      manifest['job_id'] != jobId ||
      manifest['project_id'] != project ||
      manifest['runner_id'] != runner ||
      manifest['revision_digest'] != revision ||
      manifest['issued_at'] != issued ||
      _map(manifest['policy'])['production'] != production ||
      provenance['version'] is! int ||
      provenance['version'] != 1 ||
      provenance['jobId'] != jobId ||
      provenance['projectId'] != project ||
      provenance['runnerId'] != runner ||
      provenance['revisionDigest'] != revision ||
      provenance['deviceId'] != signer ||
      hosts is! List ||
      approved is! List ||
      hosts.length != kept.length ||
      approved.length != kept.length) {
    _bad();
  }
  for (var i = 0; i < kept.length; i++) {
    final t = _map(approved[i]);
    if (hosts[i] != kept[i].hostId || t['host_id'] != kept[i].hostId || t['route_digest'] != kept[i].routeDigest) {
      _bad();
    }
  }
  return AutomationJobAdmissionAudit._(
    status: status,
    jobId: jobId,
    manifestDigest: manifestDigest,
    signerId: signer,
    projectId: project,
    revisionDigest: revision,
    runnerId: runner,
    issuedAt: issued,
    production: production,
    ownerEvidenceDigest: ownerDigest,
    targets: List.unmodifiable(kept),
  );
}

/// A closed, bounded read-only section over the already fetched ORIGINAL row.
/// It has no service/port, network, execution, export or clipboard side effects.
class AutomationJobAdmissionAuditView extends StatelessWidget {
  const AutomationJobAdmissionAuditView({
    super.key,
    required this.job,
    required this.expanded,
    required this.page,
    required this.onExpanded,
    required this.onPage,
  });
  final AutomationJson job;
  final bool expanded;
  final int page;
  final ValueChanged<bool> onExpanded;
  final ValueChanged<int> onPage;
  Widget fact(String label, String value) => Padding(
    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [Text(label), SelectableText(value)]),
  );
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    AutomationJobAdmissionAudit? audit;
    var invalid = false;
    try {
      audit = parseAutomationJobAdmissionAudit(job);
    } on FormatException {
      invalid = true;
    }
    if (audit == null && !invalid) return const SizedBox.shrink();
    final data = audit,
        last = data == null || data.targets.isEmpty ? 0 : (data.targets.length - 1) ~/ 25,
        selected = page.clamp(0, last);
    return ExpansionTile(
      key: ValueKey('automation-audit-${job['job_id']}'),
      initiallyExpanded: expanded,
      onExpansionChanged: onExpanded,
      title: Text(l.automationAuditTitle),
      children: [
        InfoBanner(tone: BannerTone.info, message: l.automationAuditDisclosure),
        if (invalid || data?.status == 'unavailable')
          Text(l.automationAuditUnavailable)
        else if (data?.status == 'unrecorded')
          Text(switch (data?.reason) {
            'legacy_native_provenance' => l.automationAuditLegacy,
            'runner_unrecorded' => l.automationAuditRunnerUnrecorded,
            _ => l.automationAuditNotObserved,
          })
        else if (data != null) ...[
          fact(l.automationAuditInitiatingDevice, data.signerId!),
          fact(l.automationAuditJobId, data.jobId!),
          fact(l.automationAuditProjectId, data.projectId!),
          fact(l.automationAuditRevision, data.revisionDigest!),
          fact(l.automationAuditManifest, data.manifestDigest!),
          fact(l.automationAuditRunnerId, data.runnerId!),
          fact(l.automationAuditPurpose, 'jobs.execute'),
          fact(
            l.automationAuditIssuedUtc,
            DateTime.fromMillisecondsSinceEpoch(data.issuedAt! * 1000, isUtc: true).toIso8601String(),
          ),
          if (data.ownerEvidenceDigest != null) fact(l.automationAuditOwnerEvidence, data.ownerEvidenceDigest!),
          Text(l.automationAuditTargets(data.targets.length.toString())),
          for (final target in data.targets.skip(selected * 25).take(25))
            Padding(
              key: ValueKey('audit-target-${target.hostId}'),
              padding: const EdgeInsets.all(8),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  fact(l.automationAuditTargetId, target.hostId),
                  fact(l.automationAuditRouteDigest, target.routeDigest),
                ],
              ),
            ),
          if (data.targets.length > 25)
            Wrap(
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                TextButton(
                  key: ValueKey('audit-previous-${job['job_id']}'),
                  onPressed: selected > 0 ? () => onPage(selected - 1) : null,
                  child: Text(l.automationInventoryPrevious),
                ),
                Text('${selected + 1} / ${last + 1}'),
                TextButton(
                  key: ValueKey('audit-next-${job['job_id']}'),
                  onPressed: selected < last ? () => onPage(selected + 1) : null,
                  child: Text(l.automationInventoryNext),
                ),
              ],
            ),
        ],
      ],
    );
  }
}
