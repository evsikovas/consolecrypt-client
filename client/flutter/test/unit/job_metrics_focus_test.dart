import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_context_project_guard.dart';
import 'package:consolecrypt/monitoring/job_context_section_adapter.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:flutter_test/flutter_test.dart';

import 'job_context_navigation_test.dart' as old;
import 'job_metric_correlation_test.dart' as f;

JobContextMetadata metadata() => JobContextMetadata.parse({
  'version': 1,
  'profileId': f.profile,
  'notificationScopeId': f.uuid(900),
  'hostId': f.host,
  'hostRevision': 7,
  'protocol': 'ssh',
  'sourceId': f.source,
  'sourceRevision': 3,
  'bindingId': f.binding,
  'bindingRevision': 4,
})!;
JobContextFocus focus({JobCorrelationScope? scope}) =>
    JobContextFocus.forMonitoring(
      scope: scope ?? JobCorrelationScope(f.profile),
      metadata: metadata(),
      jobId: f.uuid(100),
      jobRevision: 3,
      projectId: f.project,
      manifestDigest: f.digest,
    )!;
Map<String, dynamic> job() => f.row(revision: 3, state: 'failed');

void main() {
  test('reverse focus has no fabricated UTC window and fixed monitoring route only', () {
    final packet = focus();
    expect(packet.originalContext, isNull);
    expect(packet.metadataOnly, isTrue);
    expect(packet.project, isNull);
    expect(packet.selection.projectRevision, isNull);
    expect(packet.originManifestDigest, f.digest);
    expect(jobContextSectionRoute(packet.selection), AppRoutes.monitoring);
    expect(jobContextMonitoringJobMatches(packet, f.catalog([job()])), isTrue);
  });
  test('metadata-only packet cannot become SSH/Docker/project navigation or a project read', () {
    for (final destination in [
      JobContextDestination.sshHost,
      JobContextDestination.dockerHost,
      JobContextDestination.ansibleProject,
    ]) {
      final packet = focus();
      expect(packet.claim(packet.scope, destination), isNull);
      expect(
        jobContextProjectFromCurrentMetadata(
          packet,
          const AutomationProjectCatalog(
            version: 1,
            readOnly: false,
            deviceLocal: true,
            projects: [],
          ),
          f.catalog([job()]),
          f.at,
        ),
        isNull,
      );
    }
    final context = f.context(), frame = old.frame(f.context());
    final owner = JobContextNavigation()..bind(frame);
    expect(
      owner.propose(frame, JobContextDestination.monitoringBinding).intent,
      isNull,
    );
    expect(
      context.startUtc.isUtc,
      isTrue,
    ); // old graph context remains unchanged
  });
  test('source page hide permits original session handoff and admission consumes exactly once', () async {
    final packet = focus();
    var producerVisible = true, calls = 0;
    producerVisible = false;
    expect(producerVisible, isFalse);
    final ready = await admitJobContextFocus(
      packet,
      destination: JobContextDestination.monitoringBinding,
      originalSessionScope: packet.scope,
      currentSessionScope: () => packet.scope,
      targetCurrent: () => true,
      recheck: (original) async {
        calls++;
        expect(original, same(packet.metadata));
        return original;
      },
    );
    expect(ready, same(packet));
    expect(
      await admitJobContextFocus(
        packet,
        destination: JobContextDestination.monitoringBinding,
        originalSessionScope: packet.scope,
        currentSessionScope: () => packet.scope,
        targetCurrent: () => true,
        recheck: (original) async {
          calls++;
          return original;
        },
      ),
      isNull,
    );
    expect(calls, 1);
  });
  test('same-profile replacement scope and target hide reject pending admission without adopting latest nonce', () async {
    final packet = focus(), pending = Completer<JobContextMetadata>();
    var current = packet.scope, visible = true;
    final result = admitJobContextFocus(
      packet,
      destination: JobContextDestination.monitoringBinding,
      originalSessionScope: packet.scope,
      currentSessionScope: () => current,
      targetCurrent: () => visible,
      recheck: (_) => pending.future,
    );
    current = JobCorrelationScope(f.profile);
    visible = false;
    pending.complete(packet.metadata);
    expect(await result, isNull);
    expect(packet.originalContext, isNull);
  });
  test('fresh job matching rejects changed revision/digest/tenant/targets rather than silently adopting them', () {
    final packet = focus();
    final variants = [
      job()..['notification_revision'] = 4,
      job()..['manifest_digest'] = 'b' * 64,
      job()..['host_ids'] = [f.uuid(99)],
      job()
        ..['manifest'] = {...job()['manifest'] as Map, 'tenant_id': f.uuid(99)},
      job()
        ..['manifest'] = {
          ...job()['manifest'] as Map,
          'targets': [
            {'host_id': f.uuid(99)},
          ],
        },
      job()..['manifest'] = {...job()['manifest'] as Map, 'version': 2},
    ];
    for (final row in variants) {
      expect(jobContextMonitoringJobMatches(packet, f.catalog([row])), isFalse);
    }
    expect(
      jobContextMonitoringJobMatches(packet, f.catalog([job(), job()])),
      isFalse,
    );
    expect(
      jobContextMonitoringJobMatches(packet, {
        'schema_version': 2,
        'read_only': true,
        'jobs': [job()],
      }),
      isFalse,
    );
  });
  test('missing legacy revision or malformed references never receive a synthetic original context', () {
    expect(
      JobContextFocus.forMonitoring(
        scope: JobCorrelationScope(f.profile),
        metadata: metadata(),
        jobId: f.uuid(100),
        jobRevision: 0,
        projectId: f.project,
        manifestDigest: f.digest,
      ),
      isNull,
    );
    expect(
      JobContextFocus.forMonitoring(
        scope: JobCorrelationScope(f.uuid(99)),
        metadata: metadata(),
        jobId: f.uuid(100),
        jobRevision: 3,
        projectId: f.project,
        manifestDigest: f.digest,
      ),
      isNull,
    );
    expect(
      jobContextApprovedHostIds(
        job()..remove('notification_revision'),
        profileId: f.profile,
        jobId: f.uuid(100),
        revision: 3,
        projectId: f.project,
        manifestDigest: f.digest,
      ),
      isNull,
    );
  });
}
