import 'dart:convert';

import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_installer_fixture.dart' as f;

void main() {
  AgentInstallerFrame frame() =>
      AgentInstallerFrame(profileId: f.profileId, notificationScopeId: f.nonce);
  test('opaque original nonce and real i64 host revision survive exact source request', () {
    final c = AgentInstallerContext.parse(f.context(), frame());
    final r = c.request(
      name: 'Reviewed source',
      host: c.hosts.single,
      operation: AgentInstallerOperation.install,
      start: false,
      artifactPath: 'D:/public/release.zip',
      manifestPath: 'D:/public/release.json',
    );
    expect(r['expectedHostRevision'], '9007199254740993');
    expect(r['notificationScopeId'], f.nonce);
    expect(r.keys.toSet(), {
      'version',
      'profileId',
      'notificationScopeId',
      'name',
      'operation',
      'start',
      'hostId',
      'expectedHostRevision',
      'artifactPath',
      'manifestPath',
    });
    expect(r.containsKey('capabilities'), false);
    expect(r.containsKey('signer'), false);
    expect(r['start'], false);
  });
  test('foreign frames, invented revisions and copied host choices cannot become source selection', () {
    final c = AgentInstallerContext.parse(f.context(), frame()),
        copied = AgentInstallerContext.parse(f.context(), frame());
    expect(
      () => c.request(
        name: 'Source',
        host: copied.hosts.single,
        operation: AgentInstallerOperation.status,
        start: false,
      ),
      throwsFormatException,
    );
    for (final rev in [1, '0', '01', '9223372036854775808']) {
      final v = f.context();
      ((v['hosts'] as List).single as Map<String, dynamic>)['hostRevision'] =
          rev;
      expect(
        () => AgentInstallerContext.parse(v, frame()),
        throwsFormatException,
      );
    }
    final v = f.context()..['notificationScopeId'] = 'later';
    expect(
      () => AgentInstallerContext.parse(v, frame()),
      throwsFormatException,
    );
    expect(
      () =>
          AgentInstallerContext.parse(f.context()..['version'] = 1.0, frame()),
      throwsFormatException,
    );
  });
  test('source result cannot assert release verification, runtime version or another target', () {
    final c = AgentInstallerContext.parse(f.context(), frame());
    final r = c.request(
      name: 'Source',
      host: c.hosts.single,
      operation: AgentInstallerOperation.install,
      start: false,
      artifactPath: 'a.zip',
      manifestPath: 'a.json',
    );
    final good = AgentInstallerCreated.parse(f.created(r), c, r);
    expect(good.project.id, f.projectId);
    expect(good.artifactSha256, 'b' * 64);
    final variants = <Map<String, dynamic>>[
      f.created(r)..['actualRuntimeVersion'] = '1.2.3',
      f.created(r)..['hostId'] = f.projectId,
      f.created(r)..['Ready'] = true,
    ];
    final verified = f.created(r);
    (verified['claimedRelease'] as Map)['signatureVerified'] = true;
    variants.add(verified);
    for (final v in variants) {
      expect(() => AgentInstallerCreated.parse(v, c, r), throwsFormatException);
    }
  });
  test('nonrelease operations cannot smuggle artifact, start or an arbitrary target', () {
    final c = AgentInstallerContext.parse(f.context(), frame());
    expect(
      () => c.request(
        name: 'Source',
        host: c.hosts.single,
        operation: AgentInstallerOperation.remove,
        start: false,
        artifactPath: 'public.zip',
      ),
      throwsFormatException,
    );
    expect(
      () => c.request(
        name: 'Source',
        host: c.hosts.single,
        operation: AgentInstallerOperation.status,
        start: true,
      ),
      throwsFormatException,
    );
    final r = c.request(
      name: 'Status',
      host: c.hosts.single,
      operation: AgentInstallerOperation.status,
      start: false,
    );
    expect(r['artifactPath'], null);
    expect(r['manifestPath'], null);
    expect(jsonEncode(r), isNot(contains('secret')));
  });
}
