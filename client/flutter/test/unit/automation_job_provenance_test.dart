import 'package:consolecrypt/automation/automation_job_provenance.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/automation_job_fixture.dart' as f;

void main() {
  Set<String>? approved(Map<String, dynamic> row) => automationApprovedJobHostIds(
    row,
    profileId: f.profile,
    jobId: f.uuid(100),
    revision: 3,
    projectId: f.project,
    manifestDigest: f.digest,
  );
  test('AI repair accepts the exact original job and approved target set', () {
    final row = f.row(state: 'failed', revision: 3);
    expect(approved(row), {f.host});
    expect(() => approved(row)!.add(f.uuid(8)), throwsUnsupportedError);
  });
  test('another profile or changed manifest cannot authorize a repair context', () {
    final row = f.row(state: 'failed', revision: 3);
    (row['manifest'] as Map)['tenant_id'] = f.uuid(8);
    expect(approved(row), isNull);
    (row['manifest'] as Map)['tenant_id'] = f.profile;
    row['manifest_digest'] = 'b' * 64;
    expect(approved(row), isNull);
  });
  test('duplicate declared hosts and unmatched approved targets are rejected', () {
    final row = f.row(state: 'failed', revision: 3, hosts: [f.host, f.host]);
    expect(approved(row), isNull);
    final changed = f.row(state: 'failed', revision: 3);
    (((changed['manifest'] as Map)['targets'] as List).single as Map)['host_id'] = f.uuid(8);
    expect(approved(changed), isNull);
  });
}
