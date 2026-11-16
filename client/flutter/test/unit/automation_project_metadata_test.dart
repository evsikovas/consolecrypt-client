import 'dart:async';

import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/automation/automation_project_metadata_controller.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/project_metadata_fixture.dart';

void main() {
  test('Native context keeps opaque nonce and undeclared floor cannot publish', () {
    final value = ProjectMetadataContext.parse(context(), profileId: profile, nonce: nonce, project: project);
    expect(value.frame.nonce, nonce);
    expect(value.catalog.releasedFloor, isNull);
    expect(value.canRequestPublish, isFalse);
    final bad = context();
    (bad['frame'] as AutomationJson)['notification_scope_id'] = '$nonce\n';
    expect(
      () => ProjectMetadataContext.parse(bad, profileId: profile, nonce: nonce, project: project),
      throwsFormatException,
    );
  });
  test('future catalog remains empty readonly and no implicit migration enables publication', () {
    final raw = context();
    (raw['frame'] as AutomationJson)['catalog_revision'] = null;
    raw['catalog'] = {'version': 1, 'read_only': true, 'records': <AutomationJson>[], 'files_transferred': false};
    final value = ProjectMetadataContext.parse(raw, profileId: profile, nonce: nonce, project: project);
    expect(value.catalog.schemaSupported, isFalse);
    expect(value.catalog.records, isEmpty);
    expect(value.canRequestPublish, isFalse);
  });
  test('remote rows reject local roots runnable flags raw paths and fractional schema', () {
    for (final key in ['read_only', 'local_root_bound', 'runnable']) {
      final raw = record();
      raw[key] = key == 'read_only' ? false : true;
      expect(() => ProjectMetadataRecord.parse(raw), throwsFormatException);
    }
    final raw = record();
    (raw['metadata'] as AutomationJson)['path'] = '/home/user/private';
    expect(() => ProjectMetadataRecord.parse(raw), throwsFormatException);
    final fractional = record();
    ((fractional['metadata'] as AutomationJson)['inventory'] as AutomationJson)['version'] = 2.0;
    expect(() => ProjectMetadataRecord.parse(fractional), throwsFormatException);
    final valid = ProjectMetadataRecord.parse(record());
    expect(valid.runnable, isFalse);
    expect(valid.localRootBound, isFalse);
    expect(valid.readOnly, isTrue);
  });
  test('catalog preserves known reconciliation state and refuses unbounded or foreign refs', () {
    final raw = catalog(withRecord: true);
    raw['read_only'] = true;
    raw['sync_state'] = 'reconciliation_required';
    raw['reason'] = 'settings_catalog_conflict_requires_reconciliation';
    expect(ProjectMetadataCatalog.parse(raw).reconciliationRequired, isTrue);
    final foreign = record();
    foreign['unresolved_references'] = [settingsId];
    expect(() => ProjectMetadataRecord.parse(foreign), throwsFormatException);
    final large = catalog();
    large['raw'] = 'x' * 524289;
    expect(() => ProjectMetadataCatalog.parse(large), throwsFormatException);
  });
  test('read and publish responses require exact original subject and project echo', () {
    expect(checkedProjectMetadataReply(reply(), profileId: profile, nonce: nonce).records, isEmpty);
    final bad = reply();
    bad['profile_id'] = settingsId;
    expect(() => checkedProjectMetadataReply(bad, profileId: profile, nonce: nonce), throwsFormatException);
    final published = reply(published: true);
    published['project_id'] = settingsId;
    expect(
      () => checkedProjectMetadataReply(published, profileId: profile, nonce: nonce, projectId: projectId),
      throwsFormatException,
    );
  });
  test('controller constructor and undeclared-floor publish cause no upload', () async {
    final fake = MetadataFake();
    final controller = ProjectMetadataController(
      service: fake,
      profileId: profile,
      nonce: nonce,
      project: project,
      originalCurrent: () => true,
    );
    expect(fake.reads, 0);
    expect(fake.publishes, 0);
    await controller.loadContext();
    expect(controller.issue, ProjectMetadataIssue.floorUndeclared);
    expect(await controller.publish(), isFalse);
    expect(fake.publishes, 0);
    controller.dispose();
  });
  test('explicit publish sends only original frame once and keeps returned rows inert', () async {
    final fake = MetadataFake()..candidate = true;
    final controller = ProjectMetadataController(
      service: fake,
      profileId: profile,
      nonce: nonce,
      project: project,
      originalCurrent: () => true,
    );
    await controller.loadContext();
    expect(fake.publishes, 0);
    expect(await controller.publish(), isTrue);
    expect(fake.request, {...(context(candidate: true)['frame'] as AutomationJson), 'expected_reference_revision': 0});
    expect(controller.catalog!.records.single.runnable, isFalse);
    expect(await controller.publish(), isFalse);
    expect(fake.publishes, 1);
    controller.dispose();
  });
  test('scope retirement drops pending response without adopting a new owner', () async {
    var current = true;
    final fake = MetadataFake()..pending = Completer<AutomationJson>();
    final controller = ProjectMetadataController(
      service: fake,
      profileId: profile,
      nonce: nonce,
      project: project,
      originalCurrent: () => current,
    );
    final reading = controller.loadContext();
    current = false;
    fake.pending!.complete(context(candidate: true));
    await reading;
    expect(controller.phase, ProjectMetadataPhase.retired);
    expect(controller.catalog, isNull);
    expect(fake.publishes, 0);
    controller.dispose();
  });
  test('uncertain publish never retries and standalone read never refreshes consumed frame', () async {
    final fake = MetadataFake()
      ..candidate = true
      ..published = {'version': 99};
    final controller = ProjectMetadataController(
      service: fake,
      profileId: profile,
      nonce: nonce,
      project: project,
      originalCurrent: () => true,
    );
    await controller.loadContext();
    expect(await controller.publish(), isFalse);
    expect(controller.phase, ProjectMetadataPhase.publicationUnknown);
    expect(await controller.publish(), isFalse);
    expect(fake.publishes, 1);
    await controller.readCatalog();
    expect(controller.context, isNull);
    expect(controller.canRequestPublish, isFalse);
    controller.dispose();
  });
}
