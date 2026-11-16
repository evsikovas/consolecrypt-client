import 'dart:convert';

import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111';
const nonce = '22222222-2222-4222-8222-222222222222';
const connectionId = '33333333-3333-4333-8333-333333333333';
const credentialId = '44444444-4444-4444-8444-444444444444';
Map<String, Object?> configuration() => {
  'id': connectionId,
  'name': 'Synthetic AWX',
  'endpoint': 'https://awx.example.test/controller',
  'route': {'kind': 'direct'},
  'tls': {'customCaPem': null},
  'auth': {'kind': 'bearerRef', 'credentialId': credentialId},
};
AwxConnection connection({int revision = 3}) => AwxConnection.fromJson({
  'id': connectionId,
  'revision': revision,
  'configuration': configuration(),
  'readOnly': false,
});
AwxCatalog catalog({int revision = 7}) => AwxCatalog.fromJson({
  'version': 1,
  'catalogRevision': revision,
  'readOnly': false,
  'connections': [
    {
      'id': connectionId,
      'revision': 3,
      'configuration': configuration(),
      'readOnly': false,
    },
  ],
});
Map<String, Object?> templateJson() => {
  'id': 9,
  'name': 'Synthetic template',
  'modified': '2026-11-12T11:00:00.000Z',
};
Map<String, Object?> jobJson() => {
  'id': 21,
  'name': 'Synthetic job',
  'status': 'running',
  'started': '2026-11-12T11:00:00Z',
  'finished': null,
};
final now = DateTime.utc(2026, 11, 12, 11);
AwxPreparedReview review({
  AwxLaunchParameters? parameters,
  String ticket = 'synthetic-one-use',
}) {
  final p = parameters ?? AwxLaunchParameters();
  return AwxPreparedReview.fromJson(
    {
      'reviewTicket': ticket,
      'template': templateJson(),
      'parameters': p.toJson(),
      'expiresUtc': '2026-11-12T11:02:00Z',
      'connectionId': connectionId,
      'connectionRevision': 3,
    },
    connection: connection(),
    templateId: 9,
    parameters: p,
    now: now,
  );
}

void main() {
  test(
    'fixed Native snake projections retain exact IDs and UTC timestamps',
    () {
      final page = AwxTemplatePage.fromJson({
        'templates': [templateJson()],
        'has_next': true,
        'page': 1,
      }, expectedPage: 1);
      expect(page.templates.single.modified, '2026-11-12T11:00:00.000Z');
      expect(page.templates.single.modifiedUtc, now);
      expect(
        AwxJob.fromJson(jobJson(), expectedId: 21).status,
        AwxJobStatus.running,
      );
    },
  );
  test('unexpected keys, float IDs and unknown states are not projected', () {
    expect(
      () =>
          AwxTemplate.fromJson({...templateJson(), 'token': 'must not appear'}),
      throwsA(isA<AwxModelException>()),
    );
    expect(
      () => AwxTemplate.fromJson({...templateJson(), 'id': 9.0}),
      throwsA(isA<AwxModelException>()),
    );
    expect(
      () => AwxJob.fromJson({...jobJson(), 'status': 'future'}),
      throwsA(isA<AwxModelException>()),
    );
    expect(
      () => AwxJob.fromJson(jobJson(), expectedId: 22),
      throwsA(isA<AwxModelException>()),
    );
  });
  test('invalid calendar timestamps cannot silently roll to another date', () {
    for (final value in [
      '2026-02-31T11:00:00Z',
      '2026-11-12T25:00:00Z',
      '2026-11-12T11:61:00Z',
    ]) {
      expect(
        () => AwxTemplate.fromJson({...templateJson(), 'modified': value}),
        throwsA(isA<AwxModelException>()),
      );
    }
  });
  test('pages have exact requested page and bounded unique template rows', () {
    expect(
      () => AwxTemplatePage.fromJson({
        'templates': [templateJson(), templateJson()],
        'has_next': false,
        'page': 1,
      }, expectedPage: 1),
      throwsA(isA<AwxModelException>()),
    );
    expect(
      () => AwxTemplatePage.fromJson({
        'templates': <Object?>[],
        'has_next': false,
        'page': 2,
      }, expectedPage: 1),
      throwsA(isA<AwxModelException>()),
    );
  });
  test(
    'parameters distinguish explicit empty from absent and freeze nested input',
    () {
      final vars = <String, Object?>{
        'nested': <String, Object?>{'x': 1},
      };
      final p = AwxLaunchParameters(limit: '', extraVars: vars);
      (vars['nested'] as Map)['x'] = 2;
      expect(p.toJson(), {
        'limit': '',
        'extra_vars': {
          'nested': {'x': 1},
        },
      });
      expect(p.sameAs(AwxLaunchParameters()), false);
      expect(
        () => (p.toJson()['extra_vars'] as Map)['new'] = 1,
        throwsUnsupportedError,
      );
      expect(jsonDecode(p.canonicalJson), p.toJson());
    },
  );
  test('parameters only accepted Native allowlist and UTF8 budget', () {
    for (final json in [
      <String, Object?>{
        'credentials': [1],
      },
      {'job_type': 'execute'},
      {'limit': 'x\ny'},
      {'extra_vars': <Object?>[]},
      {
        'extra_vars': {'x': double.nan},
      },
      {'limit': List.filled(257, 'я').join()},
    ]) {
      expect(
        () => AwxLaunchParameters.fromJson(json),
        throwsA(isA<AwxModelException>()),
      );
    }
    expect(
      () =>
          AwxLaunchParameters(extraVars: {'x': List.filled(32768, 'x').join()}),
      throwsA(isA<AwxModelException>()),
    );
  });
  test(
    'catalog future state keeps null revision readonly without writable zero',
    () {
      final c = AwxCatalog.fromJson({
        'version': 1,
        'catalogRevision': null,
        'readOnly': true,
        'connections': [
          {'id': connectionId, 'readOnly': true},
        ],
      });
      expect(c.revision, isNull);
      expect(c.connections.single.configuration, isNull);
      expect(
        () => AwxCatalog.fromJson({
          'version': 1,
          'catalogRevision': null,
          'readOnly': false,
          'connections': <Object?>[],
        }),
        throwsA(isA<AwxModelException>()),
      );
    },
  );
  test(
    'AWX configuration reuses SourceConfig refs and refuses HTTP/none/tunnels',
    () {
      for (final changed in [
        <String, Object?>{'endpoint': 'http://awx.example.test'},
        {
          'auth': {'kind': 'none', 'credentialId': credentialId},
        },
        {
          'route': {'kind': 'sshTunnel', 'routeId': profile},
        },
      ]) {
        expect(
          () => AwxConnection.fromJson({
            'id': connectionId,
            'revision': 3,
            'readOnly': false,
            'configuration': {...configuration(), ...changed},
          }),
          throwsA(anything),
        );
      }
      expect(connection().configuration!.toJson(), configuration());
    },
  );
  test(
    'review binds exact connection revision, template and canonical parameters',
    () {
      expect(review().connectionRevision, 3);
      final j = {
        'reviewTicket': 'ticket',
        'template': templateJson(),
        'parameters': <String, Object?>{},
        'expiresUtc': '2026-11-12T11:02:00Z',
        'connectionId': connectionId,
        'connectionRevision': 4,
      };
      expect(
        () => AwxPreparedReview.fromJson(
          j,
          connection: connection(),
          templateId: 9,
          parameters: AwxLaunchParameters(),
          now: now,
        ),
        throwsA(isA<AwxModelException>()),
      );
      j['connectionRevision'] = 3;
      j['parameters'] = {'limit': 'different'};
      expect(
        () => AwxPreparedReview.fromJson(
          j,
          connection: connection(),
          templateId: 9,
          parameters: AwxLaunchParameters(),
          now: now,
        ),
        throwsA(isA<AwxModelException>()),
      );
    },
  );
  test(
    'review expiry cannot exceed actual Native120s budget or already expire',
    () {
      for (final expires in ['2026-11-12T11:00:00Z', '2026-11-12T11:02:01Z']) {
        expect(
          () => AwxPreparedReview.fromJson(
            {
              'reviewTicket': 't',
              'template': templateJson(),
              'parameters': <String, Object?>{},
              'expiresUtc': expires,
              'connectionId': connectionId,
              'connectionRevision': 3,
            },
            connection: connection(),
            templateId: 9,
            parameters: AwxLaunchParameters(),
            now: now,
          ),
          throwsA(isA<AwxModelException>()),
        );
      }
    },
  );
  test('launch Unknown requires consumed true and has no retry payload', () {
    expect(
      AwxLaunchOutcome.fromJson({
        'status': 'unknown',
        'reviewConsumed': true,
      }, connection: connection()),
      isA<AwxLaunchUnknown>(),
    );
    expect(
      () => AwxLaunchOutcome.fromJson({
        'status': 'unknown',
        'reviewConsumed': false,
      }, connection: connection()),
      throwsA(isA<AwxModelException>()),
    );
  });
  test(
    'started link is exact original AWX resource, not arbitrary endpoint',
    () {
      final j = {
        'status': 'started',
        'job': jobJson(),
        'parametersIgnored': true,
        'originalJobUrl': 'https://awx.example.test/controller/api/v2/jobs/21/',
      };
      expect(
        (AwxLaunchOutcome.fromJson(
          j,
          connection: connection(),
        ) as AwxLaunchStarted).parametersIgnored,
        true,
      );
      for (final link in [
        'https://other.example.test/controller/api/v2/jobs/21/',
        'https://awx.example.test/controller/api/v2/jobs/22/',
        'https://user@awx.example.test/controller/api/v2/jobs/21/',
      ]) {
        expect(
          () => AwxLaunchOutcome.fromJson({
            ...j,
            'originalJobUrl': link,
          }, connection: connection()),
          throwsA(isA<AwxModelException>()),
        );
      }
    },
  );
  test('event counter cursor strict including empty page and float after', () {
    Map<String, Object?> event(int n) => {
      'counter': n,
      'event': 'runner_on_ok',
      'created': null,
      'stdout': 'ok',
      'truncated': false,
    };
    expect(
      AwxEventPage.fromJson({
        'events': [event(3)],
        'has_next': false,
        'after': 3,
      }, requestedAfter: 2).after,
      3,
    );
    for (final j in [
      <String, Object?>{
        'events': [event(2)],
        'has_next': false,
        'after': 2,
      },
      {'events': <Object?>[], 'has_next': false, 'after': 3},
      {'events': <Object?>[], 'has_next': false, 'after': 2.0},
    ]) {
      expect(
        () => AwxEventPage.fromJson(j, requestedAfter: 2),
        throwsA(isA<AwxModelException>()),
      );
    }
  });
  test('event output is bounded plain data with controls removed', () {
    final e = AwxEvent.fromJson({
      'counter': 1,
      'event': 'runner_on_failed',
      'created': null,
      'stdout': 'failed\u001b]52;clipboard\u0007\u202e\nnext',
      'truncated': false,
    });
    expect(e.stdout.contains('\u001b'), false);
    expect(e.stdout.contains('\u0007'), false);
    expect(e.stdout.contains('\u202e'), false);
    expect(e.stdout.contains('\nnext'), true);
    expect(
      () => AwxEvent.fromJson({
        'counter': 1,
        'event': 'runner_on_ok',
        'created': null,
        'stdout': List.filled(2049, 'x').join(),
        'truncated': false,
      }),
      throwsA(isA<AwxModelException>()),
    );
  });

  test(
    'manual extra vars reject duplicate escaped keys, deep JSON and syntax',
    () {
      for (final raw in [
        r'{"x":1,"\u0078":2}',
        r'{"x":{"k":1,"k":2}}',
        '[]',
        'not JSON',
        '${List.filled(34, '[').join()}0${List.filled(34, ']').join()}',
      ]) {
        expect(() => parseAwxExtraVars(raw), throwsA(isA<AwxModelException>()));
      }
      expect(parseAwxExtraVars(r'{"literal":"{{ not evaluated }}","x":null}'), {
        'literal': '{{ not evaluated }}',
        'x': null,
      });
    },
  );
  test('scope and facade version require integer v1 and exact fields', () {
    expect(
      AwxNativeScope.fromJson({
        'version': 1,
        'profileId': profile,
        'notificationScopeId': nonce,
      }).profileId,
      profile,
    );
    expect(
      () => AwxNativeScope.fromJson({
        'version': 1.0,
        'profileId': profile,
        'notificationScopeId': nonce,
      }),
      throwsA(isA<AwxModelException>()),
    );
    expect(
      awxEnvelopeData({
        'version': 1,
        'kind': 'templates',
        'data': <String, Object?>{},
      }, 'templates'),
      isEmpty,
    );
    expect(
      () => awxEnvelopeData({
        'version': 1,
        'kind': 'job',
        'data': <String, Object?>{},
      }, 'templates'),
      throwsA(isA<AwxModelException>()),
    );
  });
}
