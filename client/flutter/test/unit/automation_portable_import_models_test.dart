import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/portable_import_fixture.dart' as f;

PortableImportIntent intent() => PortableImportIntent.capture(
  scope: f.scope(),
  source: '/owned/selected.ccproject',
  requestedName: ' Fresh import ',
);
void main() {
  test('Ready keeps exact raw name echo separately from displayed Native normalized name', () {
    final captured = intent(), json = f.readyJson(intent());
    final ready = PortableImportReady.fromJson(json, intent: captured);
    expect(ready.requestedName, ' Fresh import ');
    expect(ready.name, 'Fresh import');
    expect(ready.projectId, f.projectId);
    expect(ready.remainingMillis, 300000);
    expect(ready.summary.files, 2);
    expect(ready.publicationSupported, isTrue);
  });
  test('unknown version, extra approval or caller target fields fail closed', () {
    for (final mutation in ['version', 'approved', 'targetPath', 'projectRevision']) {
      final captured = intent(), json = f.readyJson(intent());
      json[mutation] = mutation == 'version' ? 2 : true;
      expect(() => PortableImportReady.fromJson(json, intent: captured), throwsA(isA<PortableImportFailure>()));
    }
  });
  test('capability observation has closed reason vocabulary and is never approval', () {
    for (final reason in [
      'publication_platform_unsupported',
      'publication_location_unavailable',
      'package_tree_unsupported',
      'import_capacity_reached',
    ]) {
      final captured = intent(),
          json = f.readyJson(intent())
            ..['publicationSupported'] = false
            ..['publicationReason'] = reason;
      expect(PortableImportReady.fromJson(json, intent: captured).publicationReason, reason);
    }
    for (final pair in [(true, 'future'), (false, null), (false, 'future')]) {
      final captured = intent(),
          json = f.readyJson(intent())
            ..['publicationSupported'] = pair.$1
            ..['publicationReason'] = pair.$2;
      expect(() => PortableImportReady.fromJson(json, intent: captured), throwsA(isA<PortableImportFailure>()));
    }
  });
  test('raw input edits cannot pass as same normalized-name intent', () {
    final captured = intent(), json = f.readyJson(intent())..['requestedName'] = 'Fresh import';
    expect(() => PortableImportReady.fromJson(json, intent: captured), throwsA(isA<PortableImportFailure>()));
  });
  test('reply numeric and UTF8 budgets reject fake expiry or unbounded metadata', () {
    for (final value in [0, -1, 300001, 1.5, true]) {
      final captured = intent(), json = f.readyJson(intent())..['remainingMillis'] = value;
      expect(() => PortableImportReady.fromJson(json, intent: captured), throwsA(isA<PortableImportFailure>()));
    }
    expect(() => portableImportDecode('x' * 16385), throwsA(isA<PortableImportFailure>()));
    expect(() => portableImportDecode('[]'), throwsA(isA<PortableImportFailure>()));
    expect(() => portableImportDecode('{'), throwsA(isA<PortableImportFailure>()));
  });
  test('fresh IDs, hashes and Native normalized display name remain strictly bounded', () {
    for (final key in ['reviewId', 'reviewDigest', 'projectId', 'name', 'encryptedSha256']) {
      final captured = intent(), json = f.readyJson(intent());
      json[key] = key == 'name' ? 'x' * 121 : 'INVALID';
      expect(() => PortableImportReady.fromJson(json, intent: captured), throwsA(isA<PortableImportFailure>()));
    }
    final captured = intent(), json = f.readyJson(intent())..['name'] = 'bad\nname';
    expect(() => PortableImportReady.fromJson(json, intent: captured), throwsA(isA<PortableImportFailure>()));
  });
  test('summary is exact Native projection with no paths, inventory, Runner or secret refs', () {
    final captured = intent(), json = f.readyJson(intent());
    (json['summary'] as Map<String, dynamic>)['runnerId'] = 'not permitted';
    expect(() => PortableImportReady.fromJson(json, intent: captured), throwsA(isA<PortableImportFailure>()));
  });
  test('only same installed refs/rev3/digest from accepted owner can form result data', () {
    final captured = intent(), ready = PortableImportReady.fromJson(f.readyJson(intent()), intent: captured);
    final result = PortableInstalledProject.fromJson(f.installedJson(ready), reviewed: ready);
    expect(result.projectRevision, 3);
    expect(result.projectId, ready.projectId);
    for (final key in [
      'scope',
      'reviewId',
      'projectId',
      'name',
      'projectRevision',
      'revisionDigest',
      'encryptedSha256',
    ]) {
      final json = f.installedJson(ready);
      json[key] = key == 'projectRevision' ? 4 : 'INVALID';
      expect(() => PortableInstalledProject.fromJson(json, reviewed: ready), throwsA(isA<PortableImportFailure>()));
    }
  });
  test('success boolean or preview cannot be relabelled as installed project', () {
    final captured = intent(), ready = PortableImportReady.fromJson(f.readyJson(intent()), intent: captured);
    expect(
      () => PortableInstalledProject.fromJson({'success': true}, reviewed: ready),
      throwsA(isA<PortableImportFailure>()),
    );
    expect(
      () => PortableInstalledProject.fromJson(f.readyJson(intent()), reviewed: ready),
      throwsA(isA<PortableImportFailure>()),
    );
  });
  test('intent has no generated project/destination ID and preserves exact raw value without trim imitation', () {
    final captured = PortableImportIntent.capture(
      scope: f.scope(),
      source: '/owned/selected.ccproject',
      requestedName: '\ufeff Fresh import ',
    );
    expect(captured.requestedName, '\ufeff Fresh import ');
    expect(
      () => PortableImportIntent.capture(scope: f.scope(), source: '/owned/selected.zip', requestedName: 'Fresh'),
      throwsA(isA<PortableImportFailure>()),
    );
    expect(
      () => PortableImportIntent.capture(
        scope: f.scope(),
        source: '/owned/selected.ccproject',
        requestedName: 'x' * 1025,
      ),
      throwsA(isA<PortableImportFailure>()),
    );
  });
}
