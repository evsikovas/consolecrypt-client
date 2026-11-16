import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/monitoring/job_context_focus.dart';
import 'package:consolecrypt/monitoring/job_context_navigation.dart';
import 'package:consolecrypt/monitoring/job_context_project_guard.dart';
import 'package:flutter_test/flutter_test.dart';

import 'job_context_navigation_test.dart' as navigation;
import 'job_metric_correlation_test.dart' as fixture;

JobContextFocus _focus() {
  final context = fixture.context(), jobs = navigation.jobs(fixture.context());
  // The projection must use the exact same original context identity.
  final currentJobs = fixture.read(fixture.reader(context), context, [fixture.row(revision: 3)]);
  expect(jobs.context, isNot(same(context)));
  final frame = navigation.frame(
    context,
    jobs: currentJobs,
    project: JobContextProject(fixture.project, 6, fixture.digest),
  );
  final owner = navigation.nav(frame);
  final intent = owner.propose(frame, JobContextDestination.ansibleProject, job: currentJobs.markers.single).intent!;
  return JobContextFocus.fromTaken(owner.take(intent)!, frame)!;
}

AutomationProjectCatalog _catalog({
  int version = 1,
  bool readOnly = false,
  int revision = 6,
  String? id,
  bool recordReadOnly = false,
  String? importState,
}) => AutomationProjectCatalog(
  version: version,
  readOnly: readOnly,
  deviceLocal: true,
  projects: [
    AutomationProject(
      id: id ?? fixture.project,
      name: 'Actual named project',
      revision: revision,
      readOnly: recordReadOnly,
      importState: importState,
    ),
  ],
);

void main() {
  test('exact original job revision and project metadata admit only that named project', () {
    final focus = _focus(), catalog = _catalog();
    expect(
      jobContextProjectFromCurrentMetadata(focus, catalog, fixture.catalog([fixture.row(revision: 3)]), fixture.at),
      same(catalog.projects.single),
    );
    expect(focus.originalContext!.scope, same(focus.scope));
  });
  test('newer metadata is not silently adopted as the selected project', () {
    final focus = _focus();
    for (final catalog in [
      _catalog(revision: 7),
      _catalog(id: fixture.uuid(999)),
      _catalog(recordReadOnly: true),
      _catalog(importState: 'importing'),
    ]) {
      expect(
        jobContextProjectFromCurrentMetadata(focus, catalog, fixture.catalog([fixture.row(revision: 3)]), fixture.at),
        isNull,
      );
    }
  });
  test('future or readonly catalog cannot create an editable focus', () {
    final focus = _focus();
    for (final catalog in [_catalog(version: 2), _catalog(readOnly: true)]) {
      expect(
        jobContextProjectFromCurrentMetadata(focus, catalog, fixture.catalog([fixture.row(revision: 3)]), fixture.at),
        isNull,
      );
    }
  });
  test('changed and legacy job revisions do not rebind the original handoff', () {
    final focus = _focus();
    for (final revision in [0, 2, 4]) {
      expect(
        jobContextProjectFromCurrentMetadata(
          focus,
          _catalog(),
          fixture.catalog([fixture.row(revision: revision)]),
          fixture.at,
        ),
        isNull,
      );
    }
  });
  test('approved host membership remains exact and never uses address or name', () {
    final focus = _focus();
    expect(
      jobContextProjectFromCurrentMetadata(
        focus,
        _catalog(),
        fixture.catalog([
          fixture.row(revision: 3, hosts: [fixture.uuid(700)]),
        ]),
        fixture.at,
      ),
      isNull,
    );
  });
  test('actual source files digest must match original selected job before editor replacement', () {
    final focus = _focus(), jobs = fixture.catalog([fixture.row(revision: 3)]);
    expect(
      jobContextProjectFromCurrentMetadata(
        focus,
        _catalog(),
        jobs,
        fixture.at,
        files: const AutomationProjectRevision(digest: 'changed', bytes: 0, entries: []),
      ),
      isNull,
    );
    expect(
      jobContextProjectFromCurrentMetadata(
        focus,
        _catalog(),
        jobs,
        fixture.at,
        files: AutomationProjectRevision(digest: fixture.digest, bytes: 0, entries: const []),
      ),
      isNotNull,
    );
  });
  test('duplicate rows and unknown schemas fail closed without a newest-row fallback', () {
    final focus = _focus();
    for (final jobs in [
      fixture.catalog([fixture.row(revision: 3), fixture.row(revision: 3)]),
      fixture.catalog([fixture.row(revision: 3)])..['schema_version'] = 2,
    ]) {
      expect(jobContextProjectFromCurrentMetadata(focus, _catalog(), jobs, fixture.at), isNull);
    }
  });
}
