import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:flutter_test/flutter_test.dart';

import 'job_metric_correlation_test.dart' as f;

final nonce = f.uuid(900);
Map<String, Object?> wire(JobCorrelationContext c) => {
  'version': 1,
  'profileId': c.scope.profileId,
  'notificationScopeId': nonce,
  'hostId': c.hostId,
  'hostRevision': c.hostRevision,
  'protocol': 'ssh',
  'sourceId': c.sourceId,
  'sourceRevision': c.sourceRevision,
  'bindingId': c.bindingId,
  'bindingRevision': c.bindingRevision,
};
JobContextNavigationFrame frame(
  JobCorrelationContext c, {
  JobContextMetadata? metadata,
  String? scope,
  JobMetricSnapshot? jobs,
  JobContextProject? project,
}) => JobContextNavigationFrame(
  context: c,
  notificationScopeId: scope ?? nonce,
  metadata: metadata ?? JobContextMetadata.parse(wire(c)),
  jobs: jobs,
  project: project,
);
JobMetricSnapshot jobs(JobCorrelationContext c, {int revision = 1}) =>
    f.read(f.reader(c), c, [f.row(revision: revision)]);
JobContextNavigation nav(JobContextNavigationFrame frame) => JobContextNavigation()..bind(frame);

void main() {
  test('strict Native context rejects missing/future/extra authority metadata', () {
    final c = f.context();
    f.check(JobContextMetadata.parse(wire(c)) != null);
    for (final key in wire(c).keys) {
      final value = wire(c)..remove(key);
      f.check(JobContextMetadata.parse(value) == null);
    }
    for (final field in ['endpoint', 'credentialRef', 'address', 'execute', 'isOwner', 'approved']) {
      f.check(JobContextMetadata.parse(wire(c)..[field] = true) == null);
    }
    for (final version in [2, 1.0, null]) {
      f.check(JobContextMetadata.parse(wire(c)..['version'] = version) == null);
    }
  });
  test('no Host.updatedAt or UI counter substitute for Native host revision', () {
    final c = f.context();
    for (final value in [null, '2026-11-12T12:00:00Z', 0, 1.0, -1, 9007199254740992]) {
      f.check(JobContextMetadata.parse(wire(c)..['hostRevision'] = value) == null);
    }
    final missing = JobContextNavigationFrame(context: c, notificationScopeId: nonce, metadata: null);
    final n = nav(missing);
    f.check(
      n.propose(missing, JobContextDestination.sshHost).failure == JobContextNavigationFailure.metadataUnavailable,
    );
  });
  test('canonical context IDs and flat bounded field shape only', () {
    final c = f.context();
    for (final key in ['profileId', 'notificationScopeId', 'hostId', 'sourceId', 'bindingId']) {
      for (final value in [
        f.uuid(0),
        '$nonce\n',
        {'value': nonce},
        'localhost',
      ]) {
        f.check(JobContextMetadata.parse(wire(c)..[key] = value) == null);
      }
    }
  });
  test('SSH selection is original saved ID and never a connection/session', () {
    final c = f.context(), value = frame(c), n = nav(value);
    final intent = n.propose(value, JobContextDestination.sshHost).intent!;
    final selection = n.take(intent)!;
    f.check(selection.destination == JobContextDestination.sshHost && selection.hostId == c.hostId);
    f.check(selection.hostRevision == 7 && selection.profileId == f.profile);
    f.check(selection.sourceId == c.sourceId && selection.bindingId == c.bindingId);
    f.check(selection.jobId == null && selection.projectId == null);
    f.check(n.take(intent) == null);
  });
  test('Docker selection is exact saved SSH host, no local/first-host fallback', () {
    final c = f.context(), value = frame(c), n = nav(value);
    final selection = n.take(n.propose(value, JobContextDestination.dockerHost).intent!)!;
    f.check(selection.destination == JobContextDestination.dockerHost && selection.hostId == f.host);
    f.check(selection.hostId != 'local');
  });
  test('unknown/RDP host protocol cannot become SSH or Docker implicit route', () {
    for (final protocol in ['rdp', 'new-backend', null]) {
      final c = f.context(), raw = wire(c)..['protocol'] = protocol;
      final value = frame(c, metadata: JobContextMetadata.parse(raw)), n = nav(value);
      for (final destination in JobContextDestination.values) {
        f.check(n.propose(value, destination).failure == JobContextNavigationFailure.unsupportedProtocol);
      }
    }
  });
  test('metadata from another profile or native unlock nonce is denied', () {
    for (final key in ['profileId', 'notificationScopeId']) {
      final c = f.context(), raw = wire(c)..[key] = f.uuid(999);
      final value = frame(c, metadata: JobContextMetadata.parse(raw)), n = nav(value);
      final reason = key == 'profileId'
          ? JobContextNavigationFailure.profileChanged
          : JobContextNavigationFailure.scopeChanged;
      f.check(n.propose(value, JobContextDestination.sshHost).failure == reason);
    }
  });
  test('host/source/binding ID or revision mismatch denies before navigation', () {
    for (final key in ['hostId', 'hostRevision', 'sourceId', 'sourceRevision', 'bindingId', 'bindingRevision']) {
      final c = f.context(), raw = wire(c)..[key] = key.endsWith('Id') ? f.uuid(999) : 999;
      final value = frame(c, metadata: JobContextMetadata.parse(raw)), n = nav(value);
      final reason = key.startsWith('host')
          ? JobContextNavigationFailure.hostChanged
          : key.startsWith('source')
          ? JobContextNavigationFailure.sourceChanged
          : JobContextNavigationFailure.bindingChanged;
      f.check(n.propose(value, JobContextDestination.sshHost).failure == reason);
    }
  });
  test('Ansible project requires exact selected host job and actual matching source digest', () {
    final c = f.context(), snapshot = jobs(c), job = snapshot.markers.single;
    final value = frame(c, jobs: snapshot, project: JobContextProject(f.project, 9, f.digest)), n = nav(value);
    final selection = n.take(n.propose(value, JobContextDestination.ansibleProject, job: job).intent!)!;
    f.check(selection.projectId == f.project && selection.projectRevision == 9);
    f.check(selection.jobId == job.jobId && selection.jobRevision == 1 && selection.hostId == f.host);
  });
  test('no latest/first project inferred from a host panel', () {
    final c = f.context(), value = frame(c, project: JobContextProject(f.project, 1, f.digest)), n = nav(value);
    f.check(n.propose(value, JobContextDestination.ansibleProject).failure == JobContextNavigationFailure.missingJob);
  });
  test('missing project and wrong project/content revision are denied', () {
    final c = f.context(), snapshot = jobs(c), marker = snapshot.markers.single;
    for (final project in [
      null,
      JobContextProject(f.uuid(999), 1, f.digest),
      JobContextProject(f.project, 1, List.filled(64, 'b').join()),
    ]) {
      final value = frame(c, jobs: snapshot, project: project), n = nav(value);
      f.check(
        n.propose(value, JobContextDestination.ansibleProject, job: marker).failure ==
            (project == null ? JobContextNavigationFailure.missingProject : JobContextNavigationFailure.projectChanged),
      );
    }
  });
  test('no job created from caller ID; only exact captured current marker admitted', () {
    final c = f.context(), current = jobs(c, revision: 2), older = jobs(c, revision: 1);
    final value = frame(c, jobs: current), n = nav(value);
    f.check(
      n.propose(value, JobContextDestination.sshHost, job: older.markers.single).failure ==
          JobContextNavigationFailure.jobChanged,
    );
    final foreign = jobs(f.context());
    f.check(
      n.propose(value, JobContextDestination.sshHost, job: foreign.markers.single).failure ==
          JobContextNavigationFailure.jobChanged,
    );
  });
  test('legacy unknown job revision cannot be used as current job context', () {
    final c = f.context(), snapshot = jobs(c, revision: 0);
    final value = frame(c, jobs: snapshot), n = nav(value);
    f.check(
      n.propose(value, JobContextDestination.sshHost, job: snapshot.markers.single).failure ==
          JobContextNavigationFailure.jobChanged,
    );
  });
  test('original job revision update invalidates deferred intent without retarget', () {
    final c = f.context(), snapshot = jobs(c), before = frame(c, jobs: snapshot), n = nav(before);
    final intent = n.propose(before, JobContextDestination.dockerHost, job: snapshot.markers.single).intent!;
    n.bind(frame(c, jobs: jobs(c, revision: 2)));
    f.check(n.take(intent) == null);
  });
  test('same profile reopened nonce invalidates selection; no UUID-only lifetime match', () {
    final c = f.context(), before = frame(c), n = nav(before);
    final intent = n.propose(before, JobContextDestination.sshHost).intent!;
    final newNonce = f.uuid(998),
        current = frame(
          c,
          scope: newNonce,
          metadata: JobContextMetadata.parse(wire(c)..['notificationScopeId'] = newNonce),
        );
    n.bind(current);
    f.check(n.take(intent) == null);
    f.check(n.propose(before, JobContextDestination.sshHost).failure == JobContextNavigationFailure.contextUnavailable);
    f.check(n.take(n.propose(current, JobContextDestination.sshHost).intent!) != null);
  });
  test('source/host/range change clears intent; new selection never borrows old target', () {
    for (final next in [
      f.context(sourceRevision: 4),
      f.context(hostRevision: 8),
      f.context(selectedHost: f.uuid(90)),
      f.context(start: f.at),
    ]) {
      final before = frame(f.context()), n = nav(before);
      final intent = n.propose(before, JobContextDestination.sshHost).intent!;
      n.bind(frame(next));
      f.check(n.take(intent) == null);
    }
  });
  test('foreign projection cannot consume or replace original pending intent', () {
    final value = frame(f.context()), original = nav(value), foreign = nav(value);
    final intent = original.propose(value, JobContextDestination.sshHost).intent!;
    f.check(foreign.take(intent) == null);
    f.check(original.take(intent) != null);
  });
  test('latest explicit choice supersedes older pending intent', () {
    final value = frame(f.context()), n = nav(value);
    final old = n.propose(value, JobContextDestination.sshHost).intent!;
    final current = n.propose(value, JobContextDestination.dockerHost).intent!;
    f.check(n.take(old) == null && n.take(current)!.destination == JobContextDestination.dockerHost);
  });
  test('lock and dispose suppress old and future actions immediately', () {
    final value = frame(f.context()), n = nav(value);
    final old = n.propose(value, JobContextDestination.sshHost).intent!;
    n.bind(null);
    f.check(n.take(old) == null);
    n.bind(value);
    final current = n.propose(value, JobContextDestination.sshHost).intent!;
    n.dispose();
    n.bind(value);
    f.check(n.take(current) == null && n.propose(value, JobContextDestination.sshHost).intent == null);
  });
  test('project metadata is strict data, cannot carry paths or guessed revision', () {
    f.rejects(() => JobContextProject('/tmp/project', 1, f.digest));
    f.rejects(() => JobContextProject(f.project, 0, f.digest));
    f.rejects(() => JobContextProject(f.project, 1, 'revision-id'));
  });
}
