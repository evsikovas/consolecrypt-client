import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_association_models.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';
import 'package:consolecrypt/monitoring/agent_setup_wire.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_association_fixture.dart' as fixture;

const _planId = '60000000-0000-4000-8000-000000000006';
const _address = '10.0.0.8';

Map<String, dynamic> _host() => {
  'id': fixture.hostId,
  'revision': fixture.selectedHost.revision,
  'name': fixture.selectedHost.name,
  'address': _address,
};

Map<String, dynamic> _context() => {
  'version': 1,
  'kind': 'agentSetupContext',
  'profileId': fixture.scope.profileId,
  'notificationScopeId': fixture.scope.nonce,
  'hosts': [_host()],
  'profiles': [
    {'id': 'linux_basic', 'requiresCollection': true, 'pairingPurpose': 'metrics', 'available': true},
  ],
  'releases': <Object>[],
  'destinations': <Object>[],
  'releaseState': 'unavailable',
  'destinationState': 'unconfigured',
};

AgentSetupSelection _selection() {
  final context = parseAgentSetupContext(jsonEncode(_context()), fixture.scope);
  return AgentSetupSelection(host: context.hosts.single, profile: context.profiles.single);
}

Map<String, dynamic> _plan() => {
  'version': 1,
  'kind': 'agentSetupPlan',
  'profileId': fixture.scope.profileId,
  'notificationScopeId': fixture.scope.nonce,
  'id': _planId,
  'host': _host(),
  'monitoringProfileId': 'linux_basic',
  'destinationRef': null,
  'release': null,
  'canProceed': false,
  'requiresInstall': false,
  'requiresPairing': false,
  'requiresCollection': true,
  'certificateLifetimeSeconds': 0,
  'actions': <Object>[],
  'blocker': 'releaseUnavailable',
  'blockers': ['releaseUnavailable', 'destinationUnavailable'],
  'observations': {
    'software': 'notObserved',
    'node': 'notObserved',
    'collectorRuntime': 'notObserved',
    'graphData': 'notObserved',
  },
  'remoteMutationAttempted': false,
};

Map<String, dynamic> _cleanup() => {
  'version': 1,
  'kind': 'agentSetupCleanup',
  'closedObserved': true,
  'remoteMutationAttempted': false,
};

final _rejected = throwsA(anyOf(isA<AgentSetupFailure>(), isA<FormatException>()));

void main() {
  group('original Agent setup context', () {
    test('preserves original scope and exact revision above the JS integer limit', () {
      final result = parseAgentSetupContext(jsonEncode(_context()), fixture.scope);
      expect(result.scope, same(fixture.scope));
      expect(result.hosts.single.revision, '9007199254740993');
      expect(result.profiles.single.pairingPurpose, AgentAssociationPurpose.metrics);
      expect(result.profiles.single.requiresCollection, isTrue);
      expect(result.releases, isEmpty);
      expect(result.destinations, isEmpty);
    });

    for (final field in ['profileId', 'notificationScopeId']) {
      test('rejects a different original $field', () {
        final row = _context()..[field] = 'a different original scope';
        expect(() => parseAgentSetupContext(jsonEncode(row), fixture.scope), _rejected);
      });
    }

    test('rejects duplicate saved hosts', () {
      final row = _context()..['hosts'] = [_host(), _host()];
      expect(() => parseAgentSetupContext(jsonEncode(row), fixture.scope), _rejected);
    });

    for (final id in ['not-a-uuid', '00000000-0000-0000-0000-000000000000', 'ABCDEFAB-0000-4000-8000-000000000002']) {
      test('rejects noncanonical saved host id $id', () {
        final row = _context()..['hosts'] = [_host()..['id'] = id];
        expect(() => parseAgentSetupContext(jsonEncode(row), fixture.scope), _rejected);
      });
    }

    for (final invalid in ['0', '01', '-1', '9007199254740993.0', '9223372036854775808']) {
      test('rejects invalid exact host revision $invalid', () {
        final row = _context()..['hosts'] = [_host()..['revision'] = invalid];
        expect(() => parseAgentSetupContext(jsonEncode(row), fixture.scope), _rejected);
      });
    }

    test('rejects numeric host revision instead of accepting rounded data', () {
      final row = _context()..['hosts'] = [_host()..['revision'] = 9007199254740992];
      expect(() => parseAgentSetupContext(jsonEncode(row), fixture.scope), _rejected);
    });

    for (final field in ['name', 'address']) {
      for (final value in ['', 'x' * (field == 'name' ? 1025 : 4097)]) {
        test('rejects ${value.isEmpty ? 'empty' : 'oversized'} host $field', () {
          final row = _context()..['hosts'] = [_host()..[field] = value];
          expect(() => parseAgentSetupContext(jsonEncode(row), fixture.scope), _rejected);
        });
      }
    }

    test('rejects more than the host-count budget before admitting any hosts', () {
      final row = _context()..['hosts'] = List.generate(10001, (_) => _host());
      expect(() => parseAgentSetupContext(jsonEncode(row), fixture.scope), _rejected);
    });

    for (final field in ['releases', 'destinations']) {
      test('rejects an unreviewed new $field catalog', () {
        final row = _context()
          ..[field] = [
            {'id': 'caller-selected-authority'},
          ];
        expect(() => parseAgentSetupContext(jsonEncode(row), fixture.scope), _rejected);
      });
    }

    test('rejects a new profile with broader enrollment purpose', () {
      final row = _context();
      ((row['profiles'] as List).single as Map<String, dynamic>)['pairingPurpose'] = 'exporters';
      expect(() => parseAgentSetupContext(jsonEncode(row), fixture.scope), _rejected);
    });

    test('rejects unknown top-level or nested host authority fields', () {
      final top = _context()..['approved'] = true;
      final nested = _context()..['hosts'] = [_host()..['command'] = 'unexpected'];
      expect(() => parseAgentSetupContext(jsonEncode(top), fixture.scope), _rejected);
      expect(() => parseAgentSetupContext(jsonEncode(nested), fixture.scope), _rejected);
    });

    test('rejects empty, malformed, nonobject and over-budget response bodies', () {
      for (final raw in ['', '{', '[]', 'null', ' ' * (2 * 1024 * 1024 + 1)]) {
        expect(() => parseAgentSetupContext(raw, fixture.scope), _rejected);
      }
    });

    test('uses UTF-8 bytes for the raw response budget', () {
      final raw = jsonEncode({'padding': '🔐' * (512 * 1024)});
      expect(raw.length, lessThan(2 * 1024 * 1024));
      expect(utf8.encode(raw).length, greaterThan(2 * 1024 * 1024));
      expect(() => parseAgentSetupContext(raw, fixture.scope), _rejected);
    });
  });

  group('sealed Agent setup plan', () {
    test('preserves only the original selection and a truthful blocked plan', () {
      final selection = _selection();
      final result = parseAgentSetupPlan(jsonEncode(_plan()), fixture.scope, selection);
      expect(result.selection, same(selection));
      expect(result.canProceed, isFalse);
      expect(result.requiresInstall, isFalse);
      expect(result.requiresPairing, isFalse);
      expect(result.requiresCollection, isTrue);
      expect(result.release, isNull);
      expect(result.actions, isEmpty);
      expect(result.blocker, AgentSetupIssue.releaseUnavailable);
    });

    for (final field in ['profileId', 'notificationScopeId']) {
      test('rejects a plan from another original $field', () {
        final row = _plan()..[field] = 'another scope';
        expect(() => parseAgentSetupPlan(jsonEncode(row), fixture.scope, _selection()), _rejected);
      });
    }

    for (final field in ['id', 'revision', 'name', 'address']) {
      test('rejects changed original host $field', () {
        final row = _plan();
        (row['host'] as Map)[field] = switch (field) {
          'id' => fixture.nodeId,
          'revision' => '9007199254740992',
          _ => 'another value',
        };
        expect(() => parseAgentSetupPlan(jsonEncode(row), fixture.scope, _selection()), _rejected);
      });
    }

    for (final field in ['canProceed', 'requiresInstall', 'requiresPairing', 'remoteMutationAttempted']) {
      test('rejects forged $field authorization or mutation evidence', () {
        final row = _plan()..[field] = true;
        expect(() => parseAgentSetupPlan(jsonEncode(row), fixture.scope, _selection()), _rejected);
      });
    }

    for (final field in ['software', 'node', 'collectorRuntime', 'graphData']) {
      test('rejects forged observed $field evidence', () {
        final row = _plan();
        (row['observations'] as Map)[field] = 'ready';
        expect(() => parseAgentSetupPlan(jsonEncode(row), fixture.scope, _selection()), _rejected);
      });
    }

    for (final field in ['release', 'destinationRef']) {
      test('rejects an unapproved plan $field reference', () {
        final row = _plan()..[field] = field == 'release' ? {'id': 'unreviewed'} : fixture.nodeId;
        expect(() => parseAgentSetupPlan(jsonEncode(row), fixture.scope, _selection()), _rejected);
      });
    }

    test('rejects even a locally selected destination absent from Native admission', () {
      final original = _selection();
      final selection = AgentSetupSelection(
        host: original.host,
        profile: original.profile,
        destination: const AgentSetupDestinationRef(id: 'unreviewed', name: 'Unreviewed', receiverApprovalId: 'local'),
      );
      expect(() => parseAgentSetupPlan(jsonEncode(_plan()), fixture.scope, selection), _rejected);
    });

    test('rejects proposed actions and broadened certificate lifetime', () {
      for (final row in [
        _plan()..['actions'] = ['installAgent'],
        _plan()..['certificateLifetimeSeconds'] = 1,
        _plan()..['certificateLifetimeSeconds'] = 0.0,
      ]) {
        expect(() => parseAgentSetupPlan(jsonEncode(row), fixture.scope, _selection()), _rejected);
      }
    });

    test('rejects removed blocker or malformed operation identity', () {
      for (final row in [
        _plan()..['blocker'] = null,
        _plan()..['blockers'] = ['releaseUnavailable'],
        _plan()..['id'] = 'not-a-uuid',
        _plan()..['observations'] = {'software': 'notObserved'},
      ]) {
        expect(() => parseAgentSetupPlan(jsonEncode(row), fixture.scope, _selection()), _rejected);
      }
    });
  });

  group('truthful Agent setup cleanup', () {
    test('accepts only observed closure of an operation that did not mutate remotely', () {
      expect(parseAgentSetupCleanup(jsonEncode(_cleanup())).closedObserved, isTrue);
    });

    test('never upgrades unknown closure, remote mutation or a rollback claim to success', () {
      for (final row in [
        _cleanup()..['closedObserved'] = false,
        _cleanup()..['closedObserved'] = null,
        _cleanup()..['remoteMutationAttempted'] = true,
        _cleanup()..['remoteMutationAttempted'] = null,
        _cleanup()..['rolledBack'] = true,
        _cleanup()..['version'] = 1.0,
        _cleanup()..['kind'] = 'anotherCleanup',
      ]) {
        expect(() => parseAgentSetupCleanup(jsonEncode(row)), _rejected);
      }
    });
  });
}
