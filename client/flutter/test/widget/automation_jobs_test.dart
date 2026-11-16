import 'dart:async';
import 'dart:io' as io;

import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/ansible_code_editor.dart';
import 'package:consolecrypt/automation/automation_execution_console.dart';
import 'package:consolecrypt/automation/automation_job_result.dart';
import 'package:consolecrypt/automation/automation_project_credentials.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_runners_dialog.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation_dialog.dart';
import 'package:consolecrypt/automation/automation_service_delegation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/inventory_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';
import '../support/project_environment_fixture.dart' as environment;


// Synthetic UI contract only; real admission/SSH execution is verified by the
// separate native acceptance fixture, not by this mock.
class _Api implements AutomationService, AutomationRunnerService, AutomationJobService {
  final host = Host.create(name: 'Target fixture', address: 'target.example.test');
  AutomationJson? prepared;
  Completer<AutomationJson>? pending;
  int confirmations = 0;
  String? confirmedId, discardedId;
  bool valid = true;
  final cancelledJobs = <String>[];
  static const project = AutomationProject(id: 'project', name: 'Project fixture', readOnly: false, revision: 1);
  @override
  Future<AutomationProjectCatalog> projects() async =>
      const AutomationProjectCatalog(version: 1, readOnly: false, deviceLocal: true, projects: [project]);
  @override
  Future<AutomationProject> createProject({required String name, String? directory}) async => project;
  @override
  Future<AutomationProject> renameProject(String id, {required int revision, required String name}) async => project;
  @override
  Future<AutomationProjectRevision> files(String projectId) async => const AutomationProjectRevision(
    digest: 'fixture-digest',
    bytes: 24,
    entries: [AutomationFileEntry(path: 'playbook.yml', directory: false)],
  );
  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async =>
      AutomationProjectFile(path: path, text: '---\n- hosts: all\n', sha256: 'fixture-file', lineEndings: 'lf');
  @override
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async => AutomationProjectFile(path: path, text: text, sha256: 'saved-fixture', lineEndings: 'lf');
  @override
  Future<AutomationRunnerCatalog> runners() async => const AutomationRunnerCatalog(
    readOnly: false,
    runners: [AutomationRunnerProfile(id: 'runner', name: 'Runner fixture', readOnly: false)],
  );
  @override
  Future<AutomationRunnerProfile> createRunner({required String name, required AutomationJson configuration}) async =>
      throw UnimplementedError();
  @override
  Future<AutomationJson> connectRunner(String id) async => {};
  @override
  Future<void> disconnectRunner(String id) async {}
  @override
  Future<void> forgetRunner(String id, {required int revision}) async {}
  @override
  Future<AutomationJson> prepareRun(AutomationJson request) async {
    prepared = request;
    return pending != null ? pending!.future : review();
  }

  AutomationJson review() => {
    'review_id': 'opaque-native-review',
    'project_id': 'project',
    'runner_id': 'runner',
    'project_revision': 17,
    'inventory_digest': 'fixture-inventory-proof',
    'privilege_review': {'escalation': 'not_verified'},
    'origin': {'kind': 'wsl', 'distribution': 'Ubuntu 24.04', 'user': 'runner'},
    'routes': [
      {
        'name': 'Target fixture',
        'host_id': host.id.value,
        'hops': [
          {'name': 'Bastion fixture', 'endpoint': 'jump@bastion.example.test:2222'},
          {'name': 'Target fixture', 'endpoint': 'runner@target.example.test:22'},
        ],
      },
    ],
    'playbook': 'playbook.yml',
    'mode': 'check',
    'parameters': {'extra_vars': <String, dynamic>{}, 'tags': <String>[], 'skip_tags': <String>[], 'diff': false},
    'policy': {
      'strict_isolation': true,
      'cancel_on_lock': true,
      'timeout_seconds': 300,
      'forks': 2,
      'production': false,
    },
    'revision': {'digest': 'fixture-revision-proof'},
    'expires_at': DateTime.now().millisecondsSinceEpoch ~/ 1000 + 600,
    'targets': [
      {
        'alias': 'cc_fixture',
        'host_id': host.id.value,
        'address': 'target.example.test',
        'port': 22,
        'username': 'runner',
        'route_digest': 'fixture-route-proof',
      },
    ],
    'validation': {
      'valid': valid,
      'environment_digest': 'fixture-environment-proof',
      'checks': [
        {
          'name': 'syntax',
          'exit_code': valid ? 0 : 1,
          'output': valid ? 'fixture check passed' : 'fixture syntax failure',
        },
      ],
    },
  };
  @override
  Future<void> discardReview(String id) async {
    discardedId = id;
  }

  @override
  Future<AutomationJson> confirmRun(String reviewId) async {
    confirmations++;
    confirmedId = reviewId;
    return {'job_id': 'job-fixture'};
  }

  @override
  Future<AutomationJson> jobs() async => {'schema_version': 1, 'read_only': false, 'jobs': <AutomationJson>[]};
  @override
  Future<void> cancelJob(String id) async {
    cancelledJobs.add(id);
  }

  @override
  Future<void> acknowledgeUnknown(String id, {required bool acceptPartialChanges}) async {}
}

class _InheritanceInventory extends Fake implements InventoryService {
  int previews = 0, reveals = 0;
  Completer<EffectiveHostConfig>? pending;
  EffectiveHostConfig result() => const EffectiveHostConfig(
    username: Resolved('inherited-user', ValueSource.group, sourceName: 'Operations folder'),
    port: Resolved(2222, ValueSource.group, sourceName: 'Operations folder'),
    credentialId: Resolved.unset(), credentialName: null,
    groupPath: ['Operations folder'],
    route: [RouteHop(hostId: ObjectId('00000000-0000-4000-8000-000000000099'), label: 'Explicit bastion route')],
    routeSource: Resolved('Shared jump', ValueSource.jumpProfile, sourceName: 'Shared jump'));
  @override Future<EffectiveHostConfig> resolveEffective(Host host) async {
    previews++; return pending?.future ?? result();
  }
  @override Future<Never> revealCredentialSecret(ObjectId id) async { reveals++; throw StateError('not permitted'); }
  @override Stream<List<Group>> watchGroups() => Stream.value([]);
  @override Stream<List<Credential>> watchCredentials() => Stream.value([]);
  @override Stream<List<JumpProfile>> watchJumpProfiles() => Stream.value([]);
  @override Stream<List<KnownHost>> watchKnownHosts() => Stream.value([]);
}
class _InheritanceApi extends _Api {
  final inventory = _InheritanceInventory();
  int connects = 0;
  @override Future<AutomationJson> connectRunner(String id) async { connects++; return {}; }
}

class _Events extends _Api implements AutomationEventService {
  String state = 'running';
  Completer<AutomationProjectRevision>? pendingFiles;
  @override
  Future<AutomationProjectRevision> files(String projectId) => pendingFiles?.future ?? super.files(projectId);
  final cursors = <int>[];
  Completer<AutomationJson>? pendingEvents;
  @override
  Future<AutomationJson> jobs() async => {
    'jobs': [
      {
        'job_id': 'job-fixture',
        'runner_id': 'runner',
        'project_id': 'project',
        'host_ids': [host.id.value],
        'accepted_at': 1,
        'state': state,
        'manifest': {
          'playbook': 'playbook.yml',
          'mode': 'check',
          'targets': [
            {'alias': 'cc_fixture', 'address': host.address},
          ],
        },
        'inventory_groups': {
          'web': {
            'hosts': ['cc_fixture'],
            'children': <String>[],
            'vars': <String, dynamic>{},
          },
        },
      },
    ],
  };
  AutomationJson page(int after) => {
    'events': after == 0
        ? [
            {
              'sequence': 1,
              'type': 'runner_on_start',
              'timestamp': '2026-11-10T12:00:00Z',
              'payload': {'task': 'fixture-event-one'},
            },
            {
              'sequence': 2,
              'type': 'runner_on_ok',
              'timestamp': '2026-11-10T12:00:01Z',
              'payload': {
                'task': 'fixture-event-two',
                'host': 'cc_fixture',
                'res': {'changed': true, 'stdout': 'DO_NOT_SHOW_RAW_STDOUT'},
              },
            },
          ]
        : <AutomationJson>[],
    'cursor': after == 0 ? 2 : after,
    'output_truncated': false,
    'state': 'running',
  };
  @override
  Future<AutomationJson> jobEvents(String id, {required int after, int limit = 50}) async {
    cursors.add(after);
    return pendingEvents != null ? pendingEvents!.future : page(after);
  }
}

class _PolledResults extends _Events {
  int polls = 0;
  @override
  Future<AutomationJson> jobs() async {
    polls++;
    final value = await super.jobs();
    final job = (value['jobs'] as List).single as AutomationJson;
    job['manifest'] = {
      'playbook': 'playbook.yml',
      'mode': 'check',
      'targets': [
        for (var i = 0; i < 25; i++) {'alias': 'approved-node-$i'},
      ],
    };
    job['runner_result'] = {
      'result': {
        'exit_code': 0,
        'summary': {
          'stats': {
            'ok': {for (var i = 0; i < 25; i++) 'approved-node-$i': 1},
          },
        },
      },
    };
    if (polls > 1) {
      (value['jobs'] as List).insert(0, {...job, 'job_id': 'newer-job', 'accepted_at': 2, 'runner_result': null});
    }
    return value;
  }
}

class _DesktopHistoryProof extends _Events {
  @override
  Future<AutomationJson> jobs() async => {
    'jobs': [
      for (final (index, entry) in [
        ('deploy.yml', 'succeeded', 0),
        ('update.yml', 'failed', 2),
        ('backup.yml', 'cancelled', null),
      ].indexed)
        {
          'job_id': 'history-proof-$index',
          'runner_id': 'runner',
          'project_id': 'project',
          'host_ids': [host.id.value],
          'accepted_at': 3 - index,
          'state': entry.$2,
          'manifest': {
            'playbook': entry.$1,
            'mode': index == 0 ? 'apply' : 'check',
            'targets': [
              {'alias': 'cc_fixture', 'address': host.address},
            ],
          },
          if (entry.$3 != null)
            'runner_result': {
              'result': {'exit_code': entry.$3},
            },
        },
    ],
  };
}

class _InventoryJobs extends _Api implements AutomationSourceInventoryService {
  final unbound = Host.create(name: 'Unbound saved host', address: 'unbound.example.test');
  bool mixed = false;
  Completer<AutomationJson>? pendingConfiguration;
  AutomationJson configuration() => {
    'version': 1,
    'project_id': 'project',
    'project_revision': 1,
    'configuration': {
      'version': 1,
      'mode': mixed ? 'mixed' : 'external_only',
      'sources': [
        {'path': 'hosts.ini', 'format': 'ini'},
      ],
      'bindings': {
        'external_target': {
          'host_id': host.id.value,
          'resolution': 'match',
          'source': {'address': host.address, 'port': 22, 'username': null},
        },
      },
    },
  };
  @override
  Future<AutomationJson> inventoryConfiguration(String projectId) async =>
      pendingConfiguration?.future ?? configuration();
  @override
  Future<AutomationJson> startInventory(String projectId, String runnerId, List<AutomationJson> sources) =>
      throw UnimplementedError();
  @override
  Future<AutomationJson> inventoryPage(String id, {String query = '', int offset = 0}) => throw UnimplementedError();
  @override
  Future<void> discardInventory(String id) => throw UnimplementedError();
  @override
  Future<AutomationProject> commitInventory(String id, AutomationJson configuration) => throw UnimplementedError();
  @override
  Future<AutomationProject> clearInventory(String projectId, int revision) => throw UnimplementedError();
  @override
  Future<AutomationJson> checkInventory(String id, AutomationJson configuration) => throw UnimplementedError();
}

class _DiffJobs extends _Api implements AutomationDiffService {
  Completer<AutomationJson>? pendingDiff;
  bool failDiff = false;
  final offsets = <int>[];
  final digests = <String>[];
  @override
  AutomationJson review() =>
      super.review()..['git_source'] = {'reference': 'main', 'commit': 'fixture-commit', 'snapshot_digest': 'original'};
  AutomationJson diffPage(int offset) => {
    'current_digest': 'fixture-revision-proof',
    'baseline_digest': 'original',
    'offset': offset,
    'total': 51,
    'changes': [
      for (var i = offset; i < (offset + 50).clamp(0, 51); i++) {'path': 'file-$i.yml', 'kind': 'modified'},
    ],
  };
  @override
  Future<AutomationJson> projectDiff(String projectId, {required String currentDigest, int offset = 0}) async {
    offsets.add(offset);
    digests.add(currentDigest);
    if (failDiff) throw const FormatException('stale revision');
    return pendingDiff?.future ?? diffPage(offset);
  }

  @override
  Future<AutomationJson> projectFileDiff(String projectId, String path, {required String currentDigest}) async {
    digests.add(currentDigest);
    return {
      'path': path,
      'current_digest': currentDigest,
      'preview': 'text',
      'before': 'original YAML',
      'after': 'edited YAML',
    };
  }
}

class _FailureEvents extends _Events {
  @override
  AutomationJson page(int after) => {
    'events': after == 0
        ? [
            {
              'sequence': 1,
              'type': 'runner_on_failed',
              'timestamp': 'fixture',
              'payload': {
                'task': 'Install package',
                'host': 'cc_fixture',
                'res': {
                  'msg': 'Fixture package is unavailable',
                  'stdout': 'Fixture module diagnostic',
                  'unknown_project_data': {'value': 7},
                },
              },
            },
            {
              'sequence': 2,
              'type': 'runner_on_failed',
              'timestamp': 'fixture',
              'payload': {
                'task': 'Hidden task',
                'host': 'cc_fixture',
                'res': {'censored': true, 'msg': 'PRIVATE_NOLOG_CONTENT', 'stdout': 'PRIVATE_NOLOG_OUTPUT'},
              },
            },
          ]
        : <AutomationJson>[],
    'cursor': after == 0 ? 2 : after,
    'output_truncated': false,
    'state': 'failed',
  };
}

class _GrowingEvents extends _Events {
  @override
  AutomationJson page(int after) {
    final sequences = after == 0 ? List.generate(50, (i) => i + 1) : [after + 1];
    return {
      'events': [
        for (final n in sequences)
          {
            'sequence': n,
            'type': 'playbook_on_task_start',
            'timestamp': 'fixture',
            'payload': {'task': 'Task $n'},
          },
      ],
      'cursor': sequences.last,
      'output_truncated': false,
      'state': 'running',
    };
  }
}

class _TabbedEvents extends _Events {
  @override
  AutomationJson page(int after) => {
    'events': after == 0
        ? [
            for (var i = 1; i <= 30; i++)
              {
                'sequence': i,
                'type': 'runner_on_failed',
                'timestamp': 'fixture',
                'payload': {
                  'host': 'cc_fixture',
                  'task': 'Task $i',
                  'res': {
                    'msg': 'Module reason $i',
                    'extra': {'number': i},
                  },
                },
              },
          ]
        : <AutomationJson>[],
    'cursor': after == 0 ? 30 : after,
    'output_truncated': false,
    'state': 'failed',
  };
}

class _ContextEvents extends _Events {
  @override
  AutomationJson page(int after) => {
    'events': after == 0
        ? [
            {
              'sequence': 1,
              'type': 'playbook_on_play_start',
              'payload': {'play': 'Configure server'},
            },
            {
              'sequence': 2,
              'type': 'playbook_on_task_start',
              'payload': {'task': 'Install package'},
            },
            {
              'sequence': 3,
              'type': 'runner_on_ok',
              'payload': {
                'host': 'cc_fixture',
                'task': 'Already installed',
                'res': {'changed': false},
              },
            },
            {
              'sequence': 4,
              'type': 'runner_on_ok',
              'payload': {
                'host': 'cc_fixture',
                'task': 'Configuration updated',
                'res': {'changed': true},
              },
            },
            {
              'sequence': 5,
              'type': 'runner_on_skipped',
              'payload': {'host': 'cc_fixture', 'task': 'Optional step'},
            },
            {
              'sequence': 6,
              'type': 'runner_on_failed',
              'payload': {
                'host': 'cc_fixture',
                'task': 'Package unavailable',
                'res': {'msg': 'Fixture diagnostic'},
              },
            },
            {
              'sequence': 7,
              'type': 'runner_on_unreachable',
              'payload': {'host': 'cc_fixture', 'task': 'Remote offline'},
            },
            {
              'sequence': 8,
              'type': 'runner_summary',
              'payload': {
                'stats': {
                  'ok': {'cc_fixture': 2},
                  'changed': {'cc_fixture': 1},
                  'failures': {'cc_fixture': 1},
                  'dark': {'cc_fixture': 1},
                  'skipped': {'cc_fixture': 1},
                },
              },
            },
          ]
        : <AutomationJson>[],
    'cursor': after == 0 ? 8 : after,
    'output_truncated': false,
    'state': 'failed',
  };
}

class _ReviewLifecycle extends _Api {
  Completer<AutomationJson>? connecting;
  int preparations = 0;
  bool rejectConfirmation = false;
  final discarded = <String>[];
  @override
  Future<AutomationJson> connectRunner(String id) async => connecting?.future ?? {};
  @override
  Future<AutomationJson> prepareRun(AutomationJson request) async {
    preparations++;
    return super.prepareRun(request);
  }
  @override
  AutomationJson review() => super.review()..['review_id'] = 'review-$preparations';
  @override
  Future<void> discardReview(String id) async {
    discarded.add(id);
    await super.discardReview(id);
  }
  @override
  Future<AutomationJson> confirmRun(String reviewId) async {
    if (rejectConfirmation) {
      confirmations++;
      confirmedId = reviewId;
      throw const FormatException('Synthetic source changed before admission');
    }
    return super.confirmRun(reviewId);
  }
}

class _ReviewFacts extends _Api {
  final expires = DateTime.now().millisecondsSinceEpoch ~/ 1000 + 600;
  @override
  AutomationJson review() => super.review()
    ..['mode'] = 'apply'
    ..['expires_at'] = expires
    ..['policy'] = {'timeout_seconds': 617, 'forks': 4, 'cancel_on_lock': false};
}

class _UpdatableLegacyRunner extends _Api implements AutomationRunnerEditingService, AutomationRunnerSetupService {
  int catalogs = 0, reconnects = 0;
  final setupCalls = <bool>[];
  final requests = <AutomationJson>[];
  @override
  Future<AutomationRunnerCatalog> runners() async {
    catalogs++;
    return const AutomationRunnerCatalog(
      readOnly: false,
      runners: [
        AutomationRunnerProfile(
          id: 'runner',
          name: 'Legacy Runner fixture',
          readOnly: false,
          revision: 7,
          configuration: {
            'kind': 'local',
            'launch': {
              'kind': 'native',
              'python': '/home/fixture/legacy/bin/python',
              'module_dir': '/home/fixture/legacy/src',
              'root': '/home/fixture/legacy/state',
            },
          },
        ),
      ],
    );
  }

  @override
  Future<AutomationJson> connectRunner(String id) async {
    reconnects++;
    return {};
  }

  @override
  Future<AutomationJson> prepareRun(AutomationJson request) async {
    requests.add(request);
    throw const AutomationRunnerUpdateRequired();
  }

  @override
  Future<AutomationRunnerProfile> updateRunner(
    String id, {
    required int revision,
    required String name,
    required AutomationJson configuration,
  }) async => throw UnimplementedError();
  @override
  Future<AutomationJson> setupRunner(AutomationJson target, {required String operationId, bool install = false}) async {
    setupCalls.add(install);
    throw StateError('Setup requires an explicit user action in this fixture');
  }

  @override
  Future<void> cancelRunnerSetup(String operationId) async {}
}

class _DirectUpdateRunner extends _UpdatableLegacyRunner {
  _DirectUpdateRunner(this.scannerMarksOutdated);
  final bool scannerMarksOutdated;
  String? updatedId;
  int? updatedRevision;
  AutomationJson? updatedConfiguration;
  static const oldPaths = {
    'python': '/home/fixture/legacy/bin/python',
    'module_dir': '/home/fixture/legacy/src',
    'root': '/home/fixture/legacy/state',
  };
  static const newPaths = {
    'python': '/home/fixture/new/bin/python',
    'module_dir': '/home/fixture/new/src',
    'root': '/home/fixture/new/state',
  };
  @override
  Future<AutomationJson> setupRunner(AutomationJson target, {required String operationId, bool install = false}) async {
    setupCalls.add(install);
    return install
        ? {'paths': newPaths}
        : {
            'candidates': [oldPaths],
            'outdated_candidates': scannerMarksOutdated ? [oldPaths['python']] : <String>[],
            'install_supported': true,
            'install_parent': '/home/fixture',
            'required_packages': <String, dynamic>{},
          };
  }

  @override
  Future<AutomationRunnerProfile> updateRunner(
    String id, {
    required int revision,
    required String name,
    required AutomationJson configuration,
  }) async {
    updatedId = id;
    updatedRevision = revision;
    updatedConfiguration = configuration;
    return AutomationRunnerProfile(
      id: id,
      name: name,
      readOnly: false,
      revision: revision + 1,
      configuration: configuration,
    );
  }
}

class _OlderRunner extends _Api {
  @override
  Future<AutomationJson> prepareRun(AutomationJson request) async => throw const AutomationRunnerUpdateRequired();
}

class _EnvironmentJobs extends _Api implements AutomationProjectCredentialsService {
  final events=RuntimeNoticeEventAdapter();
  String profile='';
  bool configured=true;
  int contexts=0,connects=0;
  @override Future<AutomationProjectCatalog> projects() async =>const AutomationProjectCatalog(version:1,readOnly:false,
    deviceLocal:true,projects:[environment.project]);
  @override Future<AutomationJson> projectCredentialsContext(String id) async {
    expect(id,environment.projectId);contexts++;
    final value=environment.context(configured:configured,profile:profile);
    if(configured) value['policy']=environment.policy(host:host.id.value);
    return value;
  }
  @override Future<AutomationJson> saveProjectCredentials(AutomationJson value) async =>throw StateError('No automatic policy save');
  @override Future<AutomationJson> connectRunner(String id) async {connects++;return {};}
  @override AutomationJson review() {
    final row=super.review()..['project_id']=environment.projectId..['project_revision']=1;
    if(configured && prepared?['deployment_environment']!=null) { row['deployment_environment']={
      'id':environment.environmentId,'name':'Staging','production':(row['policy'] as Map)['production'],
      'policy_sha256':'a'*64,'credential_overrides':[{'host_id':host.id.value,'credential_id':environment.credentialId}]}; }
    return row;
  }
}

Future<ProviderContainer> _open(
  WidgetTester tester,
  _Api service, {
  Size size = const Size(1400, 900),
  bool selectTarget = true,
  bool openFile = false,
  AppLocale locale = AppLocale.en,
  RuntimeNoticeEventAdapter? serviceEvents,
  String projectId='project',
}) async {
  await loadAutomationProofFonts(tester);
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Automation jobs test');
  if (io.Platform.environment['CONSOLECRYPT_UI_PROOF_DIR'] != null) {
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  }
  await pumpApp(
    tester,
    backend,
    size: size,
    locale: locale,
    captureBoundary: io.Platform.environment['CONSOLECRYPT_UI_PROOF_DIR'] != null,
    overrides: [
      automationServiceProvider.overrideWithValue(service),
      if (serviceEvents != null) criticalRuntimeEventsProvider.overrideWithValue(serviceEvents),
      if (service is _InheritanceApi) inventoryServiceProvider.overrideWithValue(service.inventory),
      hostsProvider.overrideWith((ref) => Stream.value([service.host, if (service is _InventoryJobs) service.unbound])),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  if(service is _EnvironmentJobs) {
    service.profile=container.read(activeProfileProvider)!.id.value;
    service.events.acceptScope(originalEpoch:service.events.epoch,nativeProfile:{'id':service.profile,'active':true,
      'vault_state':'unlocked','notification_scope_id':environment.nonce});
  }
  container.read(routerProvider).go(AppRoutes.automation);
  await settle(tester);
  await tapKey(tester, 'automation-project-$projectId');
  if (openFile) await tapKey(tester, 'automation-file-playbook.yml');
  await tapKey(tester, 'automation-open-run');
  final selects = find.byType(GlassSelect<String?>);
  tester.widget<GlassSelect<String?>>(selects.first).onChanged!('runner');
  await tester.pump();
  if (selectTarget) {
    await tester.ensureVisible(find.byKey(ValueKey('automation-target-${service.host.id}')));
    await tester.tap(find.byKey(ValueKey('automation-target-${service.host.id}')));
  }
  await tester.pump();
  return container;
}


class _TaskResultEvents extends _Events {
  @override
  Future<AutomationJson> jobs() async {
    final result = await super.jobs();
    for (final record in result['jobs'] as List) {
      (record as AutomationJson)['manifest_digest'] = 'a' * 64;
    }
    return result;
  }
  @override
  AutomationJson page(int after) => {
    'job_id': 'job-fixture', 'manifest_digest': 'a' * 64,
    'events': after == 0 ? [{
      'version': 1, 'job_id': 'job-fixture', 'sequence': 1,
      'type': 'runner_on_failed', 'timestamp': '2026-11-13T12:00:00Z',
      'payload': {'host': 'cc_fixture', 'task': 'Task result integration',
        'task_uuid': '22222222-2222-4222-8222-222222222222',
        'res': {'changed': false, 'rc': 7, 'msg': 'Recorded task failure from existing event service'}},
    }] : <AutomationJson>[],
    'cursor': after == 0 ? 1 : after, 'output_truncated': false, 'state': 'running',
  };
}

class _PrivateReview extends _Api {
  late AutomationJson frozen;
  @override
  AutomationJson review() {
    frozen = super.review();
    frozen['parameters'] = {'extra_vars': {'native_frozen_key': 'PRIVATE_APPROVED_VALUE'},
      'tags': ['PRIVATE_TAG'], 'skip_tags': <String>[], 'diff': false,
      'vault': {'credentialId': 'PRIVATE_APPROVED_REF'}};
    frozen['privilege_review'] = {'escalation': 'not_verified',
      'accounts': [{'host_id': host.id.value, 'username': 'runner'}]};
    return frozen;
  }
}

class _ProductionReview extends _Api {
  bool missingChallenge = false;
  @override
  AutomationJson review() {
    final value = super.review();
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    value['review_id'] = '01010101-0101-4101-8101-010101010101';
    (value['policy'] as AutomationJson)['production'] = true;
    if (!missingChallenge) {
      value['owner_confirmation'] = {
      'policy': 'ownerConfirmationVaultPassphraseV1', 'kind': 'ownerConfirmation', 'required': true,
      'manifest_digest': 'ab' * 32, 'job_id': '02020202-0202-4202-8202-020202020202',
      'issued_at': now, 'expires_at': now + 300,
      };
    }
    return value;
  }
}

class _ProductionOwner extends _ProductionReview implements AutomationProductionOwnerService {
  final ownerIds = <String>[];
  final proofs = <StepUpProof>[];
  @override
  Future<AutomationJson> confirmOwnerRun(String reviewId, {required StepUpProof proof}) async {
    ownerIds.add(reviewId); proofs.add(proof);
    return {'job_id': 'job-fixture'};
  }
}


class _PublicServiceJobs extends _Api implements AutomationServiceDelegationService {
  int legacyConnects = 0, serviceCalls = 0;
  @override
  Future<AutomationRunnerCatalog> runners() async => const AutomationRunnerCatalog(readOnly: false,
    runners: [AutomationRunnerProfile(id: 'runner', name: 'Service connection', readOnly: false,
      revision: 1, configuration: {'kind':'service_remote','host_id':'owned-host',
      'identity_id':'11111111-1111-4111-8111-111111111111'})]);
  @override
  Future<AutomationJson> connectRunner(String id) async { legacyConnects++; throw StateError('No legacy service connect'); }
  Future<AutomationJson> unused() async { serviceCalls++; throw StateError('No service RPC on open/hide'); }
  @override Future<AutomationJson> prepareServiceDelegation(AutomationJson value) => unused();
  @override Future<AutomationJson> confirmServiceDelegation(AutomationJson value) => unused();
  @override Future<AutomationJson> cancelServiceDelegation(AutomationJson value) => unused();
  @override Future<AutomationJson> serviceDelegationStatus(AutomationJson value) => unused();
  @override Future<AutomationJson> revokeServiceDelegation(AutomationJson value) => unused();
}

void main() {

  testWidgets('short Russian workspace retains an editor above the execution console at 800x600',(tester)async{
    final api=_ContextEvents()..state='failed';
    await _open(tester,api,size:const Size(800,600),locale:AppLocale.ru,openFile:true);
    await tapKey(tester,'automation-validate-run');await tapKey(tester,'automation-confirm-run');await settle(tester);
    final editor=tester.getRect(find.byType(AnsibleCodeEditor));
    final console=tester.getRect(find.byKey(const ValueKey('automation-execution-panel')));
    expect(editor.height,greaterThanOrEqualTo(24));expect(editor.bottom,lessThan(console.top));
    expect(find.byKey(const ValueKey('automation-console-close')).hitTestable(),findsOneWidget);
    expect(find.byKey(const ValueKey('automation-console-technical-tab')).hitTestable(),findsOneWidget);
    expect(tester.takeException(),isNull);
  },variant:const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('SEC01 configured project requires an explicit environment and freezes it in ordinary Run review', (tester) async {
    final api=_EnvironmentJobs();addTearDown(api.events.dispose);
    await _open(tester,api,projectId:environment.projectId,serviceEvents:api.events);
    expect(api.contexts,1);expect(api.connects,0);expect(api.prepared,isNull);
    final prepare=find.byKey(const ValueKey('automation-validate-run'));
    expect(tester.widget<FilledButton>(prepare).onPressed,isNull);
    final form=tester.widget<ListView>(find.byKey(const ValueKey('automation-run-scroll'))).controller!;
    form.jumpTo(0);await tester.pump();
    expect(api.contexts,1); // Returning to the kept original selector must not recapture.
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('deployment-run-selection'))).onChanged!(environment.environmentId);
    await tester.pump();await tapKey(tester,'automation-validate-run');
    expect(api.connects,1);
    expect(api.prepared!['deployment_environment'],{'version':1,'frame':environment.frame(profile:api.profile),
      'environment_id':environment.environmentId});
    expect(find.textContaining('Staging'),findsOneWidget);
    expect(find.text('Native frozen host → credential references'),findsOneWidget);
    expect(api.confirmations,0);expect(tester.takeException(),isNull);
  });
  testWidgets('SEC01 legacy project keeps the exact omitted request field', (tester) async {
    final api=_EnvironmentJobs()..configured=false;addTearDown(api.events.dispose);
    await _open(tester,api,projectId:environment.projectId,serviceEvents:api.events);
    expect(api.contexts,1);expect(api.connects,0);expect(api.prepared,isNull);
    await tapKey(tester,'automation-validate-run');
    expect(api.prepared!.containsKey('deployment_environment'),false);
    expect(api.connects,1);expect(api.confirmations,0);expect(tester.takeException(),isNull);
  });
  testWidgets('SEC01 original boundary during preparation discards the late review without confirmation', (tester) async {
    final api=_EnvironmentJobs()..pending=Completer<AutomationJson>();addTearDown(api.events.dispose);
    await _open(tester,api,projectId:environment.projectId,serviceEvents:api.events);
    final form=tester.widget<ListView>(find.byKey(const ValueKey('automation-run-scroll'))).controller!;
    form.jumpTo(0);await tester.pump();
    expect(api.contexts,1); // Returning to the kept original selector must not recapture.
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('deployment-run-selection'))).onChanged!(environment.environmentId);
    await tester.pump();await tapKey(tester,'automation-validate-run');
    expect(api.prepared,isNotNull);api.events.boundary();await settle(tester);
    api.pending!.complete(api.review());await settle(tester);
    expect(api.discardedId,'opaque-native-review');expect(api.confirmations,0);
    expect(find.byKey(const ValueKey('automation-confirm-run')),findsNothing);
    expect(api.connects,1);expect(tester.takeException(),isNull);
  });

  testWidgets('public service action opens original scoped dialog without ordinary connect and route hide retires it', (tester) async {
    final service = _PublicServiceJobs(), events = RuntimeNoticeEventAdapter(); addTearDown(events.dispose);
    final container = await _open(tester, service, selectTarget:false, serviceEvents:events);
    final profile = container.read(activeProfileProvider)!;
    events.acceptScope(originalEpoch:events.epoch,nativeProfile:{'id':profile.id.value,'active':true,
      'vault_state':'unlocked','notification_scope_id':'22222222-2222-4222-8222-222222222222'});
    await settle(tester);
    expect(tester.widget<FilledButton>(find.byKey(const ValueKey('automation-validate-run'))).onPressed,isNull);
    await tapKey(tester,'automation-service-open');
    expect(find.byType(AutomationServiceDelegationDialog),findsOneWidget);
    expect(service.legacyConnects,0);expect(service.serviceCalls,0);
    container.read(routerProvider).go(AppRoutes.terminal);await settle(tester);
    expect(find.byType(AutomationServiceDelegationDialog),findsNothing);
    expect(service.legacyConnects,0);expect(service.serviceCalls,0);expect(tester.takeException(),isNull);
  });
  testWidgets('public service original hub retirement removes only owned dialog without service RPC', (tester) async {
    final service = _PublicServiceJobs(), events = RuntimeNoticeEventAdapter(); addTearDown(events.dispose);
    final container = await _open(tester,service,selectTarget:false,serviceEvents:events);
    final profile=container.read(activeProfileProvider)!;
    events.acceptScope(originalEpoch:events.epoch,nativeProfile:{'id':profile.id.value,'active':true,
      'vault_state':'unlocked','notification_scope_id':'22222222-2222-4222-8222-222222222222'});
    await settle(tester);await tapKey(tester,'automation-service-open');
    expect(find.byType(AutomationServiceDelegationDialog),findsOneWidget);
    events.boundary();await settle(tester);
    expect(find.byType(AutomationServiceDelegationDialog),findsNothing);
    expect(service.legacyConnects,0);expect(service.serviceCalls,0);expect(tester.takeException(),isNull);
  });

  testWidgets('production missing Native owner capability never falls back to ordinary confirm', (tester) async {
    final service = _ProductionReview(); await _open(tester, service);
    await tapKey(tester, 'automation-validate-run'); await tapKey(tester, 'automation-confirm-run');
    expect(service.confirmations, 0);
    expect(find.byKey(const ValueKey('automation-owner-confirmation-dialog')), findsNothing);
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('production malformed or missing original challenge cannot invoke owner or ordinary path', (tester) async {
    final service = _ProductionOwner()..missingChallenge = true; await _open(tester, service);
    await tapKey(tester, 'automation-validate-run'); await tapKey(tester, 'automation-confirm-run');
    expect(service.confirmations, 0); expect(service.ownerIds, isEmpty);
    expect(find.byKey(const ValueKey('automation-owner-confirmation-dialog')), findsNothing);
  });
  testWidgets('production main composer requires separate passphrase action and original review', (tester) async {
    final service = _ProductionOwner(); await _open(tester, service);
    await tapKey(tester, 'automation-validate-run'); await tapKey(tester, 'automation-confirm-run');
    expect(find.byKey(const ValueKey('automation-owner-confirmation-dialog')), findsOneWidget);
    expect(service.ownerIds, isEmpty); expect(service.confirmations, 0);
    await tester.enterText(find.descendant(of: find.byKey(const ValueKey('automation-owner-passphrase')),
      matching: find.byType(EditableText)), 'synthetic-passphrase'); await tester.pump();
    await tapKey(tester, 'automation-owner-confirm');
    expect(service.ownerIds, ['01010101-0101-4101-8101-010101010101']);
    expect(service.confirmations, 0);
    expect((service.proofs.single as PassphraseProof).passphrase.isWiped, isTrue);
    expect(tester.takeException(), isNull);
  });
  testWidgets('route retirement dismisses production owner dialog before any submission', (tester) async {
    final service = _ProductionOwner(); final container = await _open(tester, service);
    await tapKey(tester, 'automation-validate-run'); await tapKey(tester, 'automation-confirm-run');
    expect(find.byKey(const ValueKey('automation-owner-confirmation-dialog')), findsOneWidget);
    container.read(routerProvider).go(AppRoutes.hosts); await settle(tester);
    expect(service.ownerIds, isEmpty); expect(service.confirmations, 0);
    expect(find.byKey(const ValueKey('automation-owner-confirmation-dialog')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('RUN02 variables come from the owned prepared snapshot and raw values are never revealed', (tester) async {
    final api = _PrivateReview();
    await _open(tester, api);
    final l = tester.element(find.byKey(const ValueKey('automation-validate-run'))).l10n;
    final field = find.byWidgetPredicate((w) => w is TextField && w.decoration?.labelText == l.automationVariables);
    await tester.ensureVisible(field);
    await tester.enterText(field, '{"mutable_form_key":"PRIVATE_FORM_VALUE"}');
    await tapKey(tester, 'automation-validate-run');
    // A retained service/cache reference cannot alter the already prepared UI.
    (api.frozen['parameters'] as AutomationJson)['extra_vars'] = {'late_cache_key': 'PRIVATE_LATE_VALUE'};
    api.frozen['privilege_review'] = {'escalation': 'future_no_sudo', 'accounts': <Object?>[]};
    final scroll = find.descendant(of: find.byKey(const ValueKey('automation-run-scroll')), matching: find.byType(Scrollable)).first;
    await tester.scrollUntilVisible(find.byKey(const ValueKey('automation-review-variables')), 240, scrollable: scroll);
    await tapKey(tester, 'automation-review-variables');
    expect(find.text('native_frozen_key'), findsOneWidget);
    expect(find.textContaining('mutable_form_key'), findsNothing);
    expect(find.textContaining('late_cache_key'), findsNothing);
    for (final value in ['PRIVATE_APPROVED_VALUE', 'PRIVATE_APPROVED_REF', 'PRIVATE_TAG', 'PRIVATE_FORM_VALUE', 'PRIVATE_LATE_VALUE']) {
      expect(find.textContaining(value), findsNothing);
    }
    expect(find.text('Value hidden'), findsOneWidget);
    await tester.scrollUntilVisible(find.text('cc_fixture: runner'), -160, scrollable: scroll);
    expect(find.text('cc_fixture: runner'), findsOneWidget);
    expect(find.text('Privilege escalation: Not verified'), findsOneWidget);
    expect(api.confirmations, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('RUN02 editing during Runner connection cannot prepare a mixed or stale request', (tester) async {
    final service = _ReviewLifecycle()..connecting = Completer<AutomationJson>();
    await _open(tester, service);
    final l = tester.element(find.byKey(const ValueKey('automation-validate-run'))).l10n;
    final field = find.byWidgetPredicate((w) => w is TextField && w.decoration?.labelText == l.automationVariables);
    await tapKey(tester, 'automation-validate-run');
    await tester.ensureVisible(field);
    await tester.enterText(field, '{"updated":true}');
    service.connecting!.complete({});
    await settle(tester);
    expect(service.preparations, 0);
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(find.text(l.automationReviewChanged), findsOneWidget);
    await tapKey(tester, 'automation-validate-run');
    expect((service.prepared!['parameters'] as AutomationJson)['extra_vars'], {'updated': true});
    expect(service.confirmations, 0);
  });
  testWidgets('RUN02 a late review after parameter editing is discarded without confirmation', (tester) async {
    final service = _ReviewLifecycle()..pending = Completer<AutomationJson>();
    await _open(tester, service);
    final l = tester.element(find.byKey(const ValueKey('automation-validate-run'))).l10n;
    final field = find.byWidgetPredicate((w) => w is TextField && w.decoration?.labelText == l.automationTags);
    await tapKey(tester, 'automation-validate-run');
    expect((service.prepared!['parameters'] as AutomationJson)['tags'], isEmpty);
    await tester.ensureVisible(field);
    await tester.enterText(field, 'changed_after_request');
    service.pending!.complete(service.review());
    await settle(tester);
    expect(service.discarded, ['review-1']);
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(tester.widget<TextField>(field).controller!.text, 'changed_after_request');
    expect(service.confirmations, 0);
  });
  testWidgets('RUN02 Back retires the exact review before editing and requires explicit validation', (tester) async {
    final service = _ReviewLifecycle();
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-review-edit');
    expect(service.discarded, ['review-1']);
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(service.preparations, 1);
    final l = tester.element(find.byKey(const ValueKey('automation-validate-run'))).l10n;
    final field = find.byWidgetPredicate((w) => w is TextField && w.decoration?.labelText == l.automationVariables);
    await tester.ensureVisible(field);
    await tester.enterText(field, '{"reviewed_again":2}');
    await tapKey(tester, 'automation-validate-run');
    expect(service.preparations, 2);
    expect((service.prepared!['parameters'] as AutomationJson)['extra_vars'], {'reviewed_again': 2});
    expect(service.confirmations, 0);
    await tapKey(tester, 'automation-confirm-run');
    expect(service.confirmedId, 'review-2');
  });
  testWidgets('RUN02 rejected confirmation never reuses its consumed review and preserves the draft', (tester) async {
    final service = _ReviewLifecycle()..rejectConfirmation = true;
    await _open(tester, service);
    final l = tester.element(find.byKey(const ValueKey('automation-validate-run'))).l10n;
    final field = find.byWidgetPredicate((w) => w is TextField && w.decoration?.labelText == l.automationVariables);
    await tester.ensureVisible(field);
    await tester.enterText(field, '{"preserved":7}');
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    expect(service.confirmedId, 'review-1');
    expect(service.confirmations, 1);
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(tester.widget<TextField>(field).controller!.text, '{"preserved":7}');
    await settle(tester);
    expect(service.preparations, 1);
    expect(service.confirmations, 1);
    service.rejectConfirmation = false;
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    expect(service.confirmedId, 'review-2');
    expect(service.confirmations, 2);
  });

  testWidgets('target inheritance is explicit readonly preview with provenance and no selection/run', (tester) async {
    final service = _InheritanceApi();
    await _open(tester, service, selectTarget: false);
    expect(service.inventory.previews, 0);
    await tapKey(tester, 'automation-target-settings-${service.host.id.value}');
    expect(service.inventory.previews, 1);
    expect(find.text('inherited-user'), findsOneWidget);
    expect(find.text('2222'), findsOneWidget);
    expect(find.textContaining('inherited from group Operations folder'), findsWidgets);
    expect(find.text('Explicit bastion route'), findsOneWidget);
    expect(find.textContaining('selected Runner during the final run review'), findsOneWidget);
    expect(service.prepared, isNull); expect(service.confirmations, 0); expect(service.inventory.reveals, 0); expect(service.connects, 0);
    await tapKey(tester, 'automation-target-settings-close');
    expect(tester.widget<CheckboxListTile>(find.byKey(ValueKey('automation-target-${service.host.id}'))).value, false);
    expect(tester.takeException(), isNull);
  });
  testWidgets('pending inheritance preview is retired on hide and cannot reappear on return', (tester) async {
    final service = _InheritanceApi()..inventory.pending = Completer<EffectiveHostConfig>();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-target-settings-${service.host.id.value}');
    container.read(routerProvider).go(AppRoutes.terminal); await settle(tester);
    container.read(routerProvider).go(AppRoutes.automation); await settle(tester);
    service.inventory.pending!.complete(service.inventory.result()); await settle(tester);
    expect(find.byKey(const ValueKey('automation-target-settings-dialog')), findsNothing);
    expect(find.text('inherited-user'), findsNothing);
    expect(service.prepared, isNull); expect(service.confirmations, 0); expect(service.inventory.reveals, 0); expect(service.connects, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('target inheritance edit opens only existing exact host editor without connection or run', (tester) async {
    final service = _InheritanceApi(); final container = await _open(tester, service);
    await tapKey(tester, 'automation-target-settings-${service.host.id.value}');
    await tapKey(tester, 'automation-target-settings-edit');
    expect(container.read(routerProvider).routeInformationProvider.value.uri.path, AppRoutes.editHost(service.host.id));
    expect(service.prepared, isNull); expect(service.confirmations, 0); expect(service.inventory.reveals, 0); expect(service.connects, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('disposing only the run composer retires its pending inheritance dialog without touching another section', (tester) async {
    final service = _InheritanceApi()..inventory.pending = Completer<EffectiveHostConfig>();
    await _open(tester, service);
    await tapKey(tester, 'automation-target-settings-${service.host.id.value}');
    // Existing workspace action, as also reachable through navigation/focus;
    // no profile/route change is needed to dispose this run composer.
    tester.widget<GlassButton>(find.byKey(const ValueKey('automation-workspace-projects'), skipOffstage: false)).onPressed!();
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-target-settings-dialog')), findsNothing);
    service.inventory.pending!.complete(service.inventory.result()); await settle(tester);
    expect(find.text('inherited-user'), findsNothing);
    expect(service.prepared, isNull); expect(service.confirmations, 0); expect(service.connects, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('frozen review shows Native alias and approved username/port without deriving them from display name', (tester) async {
    final service = _Api(); await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    final scroll = find.descendant(of: find.byKey(const ValueKey('automation-run-scroll')), matching: find.byType(Scrollable)).first;
    await tester.scrollUntilVisible(find.text('Ansible alias: cc_fixture'), 160, scrollable: scroll);
    expect(find.text('Ansible alias: cc_fixture'), findsOneWidget);
    expect(find.text('Username: runner'), findsOneWidget);
    expect(find.text('Port: 22'), findsOneWidget);
    expect(service.confirmations, 0);
    expect(find.byKey(ValueKey('automation-target-settings-${service.host.id.value}')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('existing history opens output task results and preserves them through polling without another execution', (tester) async {
    final api = _TaskResultEvents();
    await _open(tester, api);
    await tapKey(tester, 'automation-workspace-jobs');
    await tapKey(tester, 'automation-events-job-fixture');
    await tapKey(tester, 'automation-console-technical-tab');
    final initialPages = api.cursors.length;
    await tapKey(tester, 'automation-task-results-job-fixture');
    expect(api.cursors.length, initialPages + 1);
    expect(api.cursors.last, 0);
    await tapKey(tester, 'automation-task-result-1');
    expect(find.text('Recorded task failure from existing event service'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
    await settle(tester);
    expect(find.text('Recorded task failure from existing event service'), findsOneWidget);
    expect(api.confirmations, 0);
    expect(api.cancelledJobs, isEmpty);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('Russian narrow bottom console keeps two compact rows and accessible controls', (tester) async {
    await _open(
      tester,
      _ContextEvents()..state = 'failed',
      openFile: true,
      locale: AppLocale.ru,
      size: const Size(700, 1000),
    );
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    await tester.drag(find.byKey(const ValueKey('automation-console-resize')), const Offset(0, -150));
    await settle(tester);
    expect(tester.getSize(find.byKey(const ValueKey('automation-console-header'))).height, lessThanOrEqualTo(64));
    expect(find.byKey(const ValueKey('automation-console-technical-tab')).hitTestable(), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-console-close')).hitTestable(), findsOneWidget);
    await captureAutomationWorkspace(tester, 'console-compact-narrow-ru');
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('desktop Russian history keeps three terminal jobs compact with direct output and retry actions', (
    tester,
  ) async {
    await _open(tester, _DesktopHistoryProof(), locale: AppLocale.ru, size: const Size(1500, 1000));
    await tapKey(tester, 'automation-workspace-jobs');
    for (var i = 0; i < 3; i++) {
      final card = find.byKey(ValueKey('automation-history-job-history-proof-$i'));
      expect(tester.getSize(card).height, lessThan(160));
      expect(find.byKey(ValueKey('automation-events-history-proof-$i')).hitTestable(), findsOneWidget);
      expect(find.byKey(ValueKey('automation-retry-history-proof-$i')).hitTestable(), findsOneWidget);
      expect(find.text('history-proof-$i'), findsNothing);
    }
    expect(find.text('deploy.yml'), findsOneWidget);
    expect(find.text('update.yml'), findsOneWidget);
    expect(find.text('backup.yml'), findsOneWidget);
    await captureAutomationWorkspace(tester, 'jobs-compact-desktop-ru');
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('Russian framed Output shows readable native context colors and clean tab separation', (tester) async {
    final service = _ContextEvents()..state = 'failed';
    await _open(tester, service, openFile: true, locale: AppLocale.ru);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    await tester.drag(find.byKey(const ValueKey('automation-console-resize')), const Offset(0, -190));
    await settle(tester);
    final output = find.byKey(const ValueKey('automation-console-scroll-job-fixture'));
    tester
        .state<ScrollableState>(find.descendant(of: output, matching: find.byType(Scrollable)).first)
        .position
        .jumpTo(0);
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-console-output-frame')), findsOneWidget);
    expect(find.text('PLAY RECAP'), findsOneWidget);
    expect(find.textContaining('ok=2'), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-event-diagnostics-6')), findsNothing);
    await captureAutomationWorkspace(tester, 'console-context-framed-ru');
    await tapKey(tester, 'automation-console-technical-tab');
    expect(find.byKey(const ValueKey('automation-console-output-frame')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('console tabs retain independent scroll expansion and selection across polling collapse and navigation', (
    tester,
  ) async {
    final service = _TabbedEvents();
    final container = await _open(tester, service, openFile: true);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    expect(find.text('Output'), findsOneWidget);
    expect(find.text('Technical information'), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-event-diagnostics-1')), findsNothing);
    final output = find.byKey(const ValueKey('automation-console-scroll-job-fixture'));
    await tester.drag(output, const Offset(0, 120));
    await settle(tester);
    final outputPosition = tester
        .state<ScrollableState>(find.descendant(of: output, matching: find.byType(Scrollable)).first)
        .position;
    final outputOffset = outputPosition.pixels;
    await tapKey(tester, 'automation-console-technical-tab');
    final technical = find.byKey(const ValueKey('automation-console-technical-scroll-job-fixture'));
    final technicalPosition = tester
        .state<ScrollableState>(find.descendant(of: technical, matching: find.byType(Scrollable)).first)
        .position;
    technicalPosition.jumpTo(0);
    await settle(tester);
    await tapKey(tester, 'automation-event-diagnostics-1');
    expect(find.textContaining('Module reason 1'), findsOneWidget);
    technicalPosition.jumpTo(18);
    await settle(tester);
    final technicalOffset = technicalPosition.pixels;
    await tester.pump(const Duration(seconds: 5));
    await settle(tester);
    expect(find.textContaining('Module reason 1'), findsOneWidget);
    expect(technicalPosition.pixels, closeTo(technicalOffset, 1));
    await tapKey(tester, 'automation-console-collapse');
    await tapKey(tester, 'automation-console-collapse');
    expect(find.byKey(const ValueKey('automation-event-diagnostics-1')), findsOneWidget);
    expect(find.textContaining('Module reason 1'), findsOneWidget);
    final restoredTechnical = tester
        .state<ScrollableState>(
          find
              .descendant(
                of: find.byKey(const ValueKey('automation-console-technical-scroll-job-fixture')),
                matching: find.byType(Scrollable),
              )
              .first,
        )
        .position;
    expect(restoredTechnical.pixels, closeTo(technicalOffset, 1));
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    expect(find.textContaining('Module reason 1'), findsOneWidget);
    await tapKey(tester, 'automation-console-output-tab');
    final restoredOutput = tester
        .state<ScrollableState>(
          find
              .descendant(
                of: find.byKey(const ValueKey('automation-console-scroll-job-fixture')),
                matching: find.byType(Scrollable),
              )
              .first,
        )
        .position;
    expect(restoredOutput.pixels, closeTo(outputOffset, 1));
    expect(find.textContaining('Module reason 1'), findsNothing);
    expect(find.text('Technical event details'), findsNothing);
    expect(service.cancelledJobs, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Russian execution console has exact Output and Technical information tabs', (tester) async {
    final service = _FailureEvents();
    await _open(tester, service, openFile: true, locale: AppLocale.ru);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    expect(find.text('Вывод'), findsOneWidget);
    expect(find.text('Техническая информация'), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-event-diagnostics-6')), findsNothing);
    await captureAutomationWorkspace(tester, 'console-output-ru');
    await tapKey(tester, 'automation-console-technical-tab');
    await tapKey(tester, 'automation-event-diagnostics-1');
    expect(find.textContaining('Fixture package is unavailable'), findsOneWidget);
    expect(find.textContaining('PRIVATE_NOLOG'), findsNothing);
    await captureAutomationWorkspace(tester, 'console-technical-ru');
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('legacy Helper error opens selected Runner settings and preserves the unsaved run draft on return', (
    tester,
  ) async {
    final service = _UpdatableLegacyRunner();
    await _open(tester, service);
    final l = tester.element(find.byKey(const ValueKey('automation-validate-run'))).l10n;
    Finder field(String label) => find.byWidgetPredicate((w) => w is TextField && w.decoration?.labelText == label);
    await tester.ensureVisible(field(l.automationVariables));
    await tester.pump();
    await tester.enterText(field(l.automationVariables), '{"fixture_var":"retained"}');
    await tester.ensureVisible(field(l.automationTags));
    await tester.pump();
    await tester.enterText(field(l.automationTags), 'fixture_tag');
    await tester.ensureVisible(field(l.automationTimeout));
    await tester.pump();
    await tester.enterText(field(l.automationTimeout), '617');
    await tapKey(tester, 'automation-validate-run');
    expect(service.requests.length, 1);
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    await tapKey(tester, 'automation-configure-runner');
    expect(find.widgetWithText(TextField, 'Legacy Runner fixture'), findsOneWidget);
    expect(find.widgetWithText(TextField, '/home/fixture/legacy/bin/python'), findsOneWidget);
    expect(service.setupCalls, isEmpty);
    expect(service.reconnects, 1);
    expect(service.confirmations, 0);
    await tester.tap(find.text('Close').last);
    await settle(tester);
    expect(tester.widget<TextField>(field(l.automationVariables)).controller!.text, '{"fixture_var":"retained"}');
    expect(tester.widget<TextField>(field(l.automationTags)).controller!.text, 'fixture_tag');
    expect(tester.widget<TextField>(field(l.automationTimeout)).controller!.text, '617');
    expect(tester.widget<CheckboxListTile>(find.byKey(ValueKey('automation-target-${service.host.id}'))).value, true);
    final selectors = tester.widgetList<GlassSelect<String?>>(find.byType(GlassSelect<String?>)).toList();
    expect(selectors[0].value, 'runner');
    expect(selectors[1].value, 'playbook.yml');
    expect(service.catalogs, greaterThanOrEqualTo(2));
    expect(service.setupCalls, isEmpty);
    expect(service.requests.length, 1);
    await tapKey(tester, 'automation-validate-run');
    expect(service.reconnects, 2);
    expect(service.requests.length, 2);
    expect(service.requests.last, service.requests.first);
    expect(service.setupCalls, isEmpty);
    expect(service.confirmations, 0);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.linux}));

  for (final marked in [true, false]) {
    testWidgets('direct saved Runner update preserves ID and revision with outdated marker $marked', (tester) async {
      final service = _DirectUpdateRunner(marked);
      final container = await _open(tester, service);
      final scope = container.read(automationScopeProvider);
      final context = tester.element(find.byKey(const ValueKey('automation-validate-run')));
      showDialog<void>(
        context: context,
        builder: (_) => AutomationRunnersDialog(scope: scope, service: service),
      );
      await settle(tester);
      await tapKey(tester, 'automation-update-runner-runner');
      expect(service.setupCalls, [false]);
      final l = tester.element(find.byKey(const ValueKey('automation-install-runner'))).l10n;
      expect(find.widgetWithText(GlassButton, l.automationUpdateRunner), findsOneWidget);
      expect(service.updatedId, isNull);
      await tapKey(tester, 'automation-install-runner');
      expect(find.text(l.automationRunnerUpdatePlanHint), findsOneWidget);
      expect(service.setupCalls, [false]);
      await tester.tap(find.widgetWithText(GlassButton, l.automationUpdateRunner).last);
      await settle(tester);
      expect(service.setupCalls, [false, true]);
      expect(find.widgetWithText(TextField, _DirectUpdateRunner.newPaths['python']!), findsOneWidget);
      await tester.tap(find.widgetWithText(GlassButton, l.commonSave));
      await settle(tester);
      expect(service.updatedId, 'runner');
      expect(service.updatedRevision, 7);
      expect(
        (service.updatedConfiguration!['launch'] as AutomationJson)['python'],
        _DirectUpdateRunner.newPaths['python'],
      );
      expect(service.confirmations, 0);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.linux}));
  }
  testWidgets('workspace surfaced selection switches while Jobs preserves the run draft', (tester) async {
    await _open(tester, _Events());
    GlassButton button(String id) => tester.widget<GlassButton>(find.byKey(ValueKey('automation-workspace-$id')));
    expect(button('run').style, GlassButtonStyle.prominent);
    final l = tester.element(find.byKey(const ValueKey('automation-validate-run'))).l10n;
    final variables = find.byWidgetPredicate((w) => w is TextField && w.decoration?.labelText == l.automationVariables);
    await tester.ensureVisible(variables);
    await tester.pump();
    await tester.enterText(variables, '{"retained":true}');
    await tapKey(tester, 'automation-workspace-jobs');
    expect(button('jobs').style, GlassButtonStyle.prominent);
    expect(button('run').style, GlassButtonStyle.glass);
    await tapKey(tester, 'automation-workspace-run');
    expect(button('run').style, GlassButtonStyle.prominent);
    expect(tester.widget<TextField>(variables).controller!.text, '{"retained":true}');
    await tapKey(tester, 'automation-workspace-projects');
    expect(button('projects').style, GlassButtonStyle.prominent);
    expect(button('jobs').style, GlassButtonStyle.glass);
    expect(tester.takeException(), isNull);
  });

  testWidgets('manual console scroll is retained while five-second polling appends new events', (tester) async {
    final service = _GrowingEvents();
    await _open(tester, service, openFile: true);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    final list = find.byKey(const ValueKey('automation-console-scroll-job-fixture'));
    await tester.drag(list, const Offset(0, 150));
    await settle(tester);
    final position = tester
        .state<ScrollableState>(find.descendant(of: list, matching: find.byType(Scrollable)).first)
        .position;
    final offset = position.pixels;
    expect(position.maxScrollExtent - offset, greaterThan(24));
    await tester.pump(const Duration(seconds: 5));
    await settle(tester);
    expect(position.pixels, closeTo(offset, 1));
    expect(service.cursors.length, greaterThan(1));
    expect(tester.takeException(), isNull);
  });

  testWidgets('failure diagnostics require explicit expansion and no_log content is never displayed', (tester) async {
    final service = _FailureEvents();
    await _open(tester, service, openFile: true);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    expect(find.textContaining('Fixture package is unavailable'), findsNothing);
    expect(find.textContaining('PRIVATE_NOLOG'), findsNothing);
    expect(find.byKey(const ValueKey('automation-event-diagnostics-1')), findsNothing);
    expect(find.text('Technical event details'), findsNothing);
    await tapKey(tester, 'automation-console-technical-tab');
    await tapKey(tester, 'automation-event-diagnostics-1');
    expect(find.textContaining('Fixture package is unavailable'), findsOneWidget);
    expect(find.textContaining('Fixture module diagnostic'), findsOneWidget);
    expect(find.textContaining('unknown_project_data'), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-event-diagnostics-2')), findsNothing);
    expect(find.textContaining('PRIVATE_NOLOG'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('accepted jobs open a resizable readonly bottom console with the editor retained', (tester) async {
    final service = _Events();
    final container = await _open(tester, service, openFile: true);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    expect(find.byType(AutomationExecutionConsole), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-editor')).hitTestable(), findsOneWidget);
    expect(find.textContaining('TASK [fixture-event-one]'), findsOneWidget);
    expect(find.textContaining('changed [target.example.test]'), findsOneWidget);
    expect(find.textContaining('DO_NOT_SHOW_RAW_STDOUT'), findsNothing);
    final panel = find.byKey(const ValueKey('automation-execution-panel'));
    final before = tester.getSize(panel).height;
    await tester.drag(find.byKey(const ValueKey('automation-console-resize')), const Offset(0, -45));
    await settle(tester);
    expect(tester.getSize(panel).height, greaterThan(before));
    await tapKey(tester, 'automation-console-collapse');
    expect(tester.getSize(panel).height, AutomationExecutionConsole.collapsedHeight);
    final calls = service.cursors.length;
    await tester.pump(const Duration(seconds: 5));
    await settle(tester);
    expect(service.cursors.length, calls);
    await tapKey(tester, 'automation-console-collapse');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    final hiddenCalls = service.cursors.length;
    await tester.pump(const Duration(seconds: 5));
    await settle(tester);
    expect(service.cursors.length, hiddenCalls);
    expect(service.cancelledJobs, isEmpty);
    expect(service.confirmations, 1);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-editor')), findsOneWidget);
    await captureAutomationWorkspace(tester, 'execution-bottom-console');
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  for (final wide in [true, false]) {
    testWidgets('native review facts use responsive cards ${wide ? 'wide' : 'narrow'} and distinct Apply badge', (
      tester,
    ) async {
      final service = _ReviewFacts();
      await _open(tester, service, size: wide ? const Size(1500, 1000) : const Size(560, 1000));
      await tapKey(tester, 'automation-validate-run');
      final playbook = tester.getRect(find.byKey(const ValueKey('automation-review-playbook-card')));
      final runner = tester.getRect(find.byKey(const ValueKey('automation-review-runner-card')));
      if (wide) {
        expect(runner.left - playbook.right, greaterThanOrEqualTo(16));
        expect(runner.top, playbook.top);
        expect(runner.height, closeTo(playbook.height, .1));
        final targets = tester.getRect(find.byKey(const ValueKey('automation-review-targets-card')));
        final policy = tester.getRect(find.byKey(const ValueKey('automation-review-limits-card')));
        expect(targets.top, policy.top);
        expect(targets.height, closeTo(policy.height, .1));
      } else {
        expect(runner.top - playbook.bottom, greaterThanOrEqualTo(16));
      }
      expect(
        tester.widget<SelectableText>(find.byKey(const ValueKey('automation-review-playbook'))).data,
        'playbook.yml',
      );
      expect(
        tester.widget<SelectableText>(find.byKey(const ValueKey('automation-review-runner'))).data,
        'Runner fixture',
      );
      expect(tester.widget<SelectableText>(find.byKey(const ValueKey('automation-review-project-revision'))).data, '17');
      expect(tester.widget<SelectableText>(find.byKey(const ValueKey('automation-review-project-digest'))).data, 'fixture-revision-proof');
      expect(tester.widget<SelectableText>(find.byKey(const ValueKey('automation-review-inventory-digest'))).data, 'fixture-inventory-proof');
      expect(tester.widget<SelectableText>(find.byKey(const ValueKey('automation-review-runner-id'))).data, 'runner');
      expect(find.textContaining('Privilege escalation required by playbooks'), findsOneWidget);
      expect(tester.widget<SelectableText>(find.byKey(const ValueKey('automation-review-timeout'))).data, '617');
      expect(tester.widget<SelectableText>(find.byKey(const ValueKey('automation-review-forks'))).data, '4');
      expect(
        tester.widget<SelectableText>(find.byKey(const ValueKey('automation-review-expiry'))).data,
        DateFormat.yMd('en').add_Hms().format(DateTime.fromMillisecondsSinceEpoch(service.expires * 1000).toLocal()),
      );
      expect(find.text('WSL · Ubuntu 24.04 · runner'), findsOneWidget);
      expect(find.textContaining('jump@bastion.example.test:2222'), findsOneWidget);
      expect(find.text('Selected: 1'), findsOneWidget);
      expect(find.text('On lock, an already running job continues until its access expires.'), findsOneWidget);
      expect(
        tester.widget<GlassBadge>(find.byKey(const ValueKey('automation-review-mode-badge'))).tone,
        GlassTone.warning,
      );
      expect(find.text('fixture-revision-proof'), findsOneWidget);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
  testWidgets('Russian dark review cards are readable with collapsed technical metadata', (tester) async {
    await _open(tester, _Api(), locale: AppLocale.ru, size: const Size(1500, 1000));
    await tapKey(tester, 'automation-validate-run');
    expect(find.text('Плейбук и режим'), findsOneWidget);
    expect(find.text('Параметры запуска'), findsOneWidget);
    expect(tester.widget<GlassBadge>(find.byKey(const ValueKey('automation-review-mode-badge'))).tone, GlassTone.info);
    await captureAutomationWorkspace(tester, 'review-cards-ru');
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets('polling retains approved-host expansion and scroll through multiple five-second refreshes', (
    tester,
  ) async {
    final service = _PolledResults();
    await _open(tester, service);
    await tapKey(tester, 'automation-workspace-jobs');
    await tapKey(tester, 'automation-job-details-job-fixture');
    await tester.ensureVisible(find.text('Results for approved hosts'));
    await tester.pump();
    await tester.tap(find.text('Results for approved hosts'));
    await settle(tester);
    await tester.ensureVisible(find.text('approved-node-20'));
    await settle(tester);
    final result = tester.state(find.byType(AutomationJobResult));
    final scroll = tester.state<ScrollableState>(
      find
          .descendant(of: find.byKey(const ValueKey('automation-jobs-scroll')), matching: find.byType(Scrollable))
          .first,
    );
    final offset = scroll.position.pixels;
    for (var i = 0; i < 3; i++) {
      await tester.pump(const Duration(seconds: 5));
      await settle(tester);
    }
    expect(service.polls, greaterThan(3));
    expect(tester.state(find.byType(AutomationJobResult)), same(result));
    expect(find.text('approved-node-20'), findsOneWidget);
    expect(scroll.position.pixels, closeTo(offset, 1));
    expect(tester.takeException(), isNull);
  });

  testWidgets('validation stays inline and does not steal terminal or command palette focus', (tester) async {
    final service = _Api()..pending = Completer<AutomationJson>();
    final container = await _open(tester, service);
    expect(find.byType(AlertDialog), findsNothing);
    await captureAutomationWorkspace(tester, 'run-composer-inline');
    await tapKey(tester, 'automation-validate-run');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    service.pending!.complete(service.review());
    await settle(tester);
    expect(find.text('Review job'), findsNothing);
    expect(service.confirmations, 0);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    expect(find.text('Review job'), findsOneWidget);
    expect(find.byType(AlertDialog), findsNothing);
    await tapKey(tester, 'automation-workspace-projects');
    expect(service.discardedId, 'opaque-native-review');
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('accepted job and editor survive terminal switching without resubmission', (tester) async {
    final service = _Events();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    expect(find.byType(AlertDialog), findsNothing);
    await tapKey(tester, 'automation-workspace-jobs');
    await tapKey(tester, 'automation-events-job-fixture');
    await captureAutomationWorkspace(tester, 'jobs-inline');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    expect(find.textContaining('fixture-event-one'), findsOneWidget);
    expect(service.confirmations, 1);
    await tapKey(tester, 'automation-workspace-projects');
    await tapKey(tester, 'automation-file-playbook.yml');
    await enterKey(tester, 'automation-editor', '---\n# retained local draft\n');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.automation);
    await settle(tester);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('automation-editor'))).controller!.text,
      contains('retained local draft'),
    );
    expect(service.confirmations, 1);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('leaving pending validation discards its late native review', (tester) async {
    final service = _Api()..pending = Completer<AutomationJson>();
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-workspace-projects');
    service.pending!.complete(service.review());
    await settle(tester);
    expect(service.discardedId, 'opaque-native-review');
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(service.confirmations, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('returning to Projects discards a retry review before editing', (tester) async {
    final service = _Events()..state = 'failed';
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    await tapKey(tester, 'automation-workspace-jobs');
    await tapKey(tester, 'automation-retry-job-fixture');
    tester.widget<GlassSelect<String?>>(find.byType(GlassSelect<String?>).first).onChanged!('runner');
    await tester.pump();
    await tapKey(tester, 'automation-target-${service.host.id}');
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-workspace-projects');
    expect(service.discardedId, 'opaque-native-review');
    await tapKey(tester, 'automation-workspace-jobs');
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(service.confirmations, 1);
    expect(tester.takeException(), isNull);
  });
  testWidgets('an older Runner gives an actionable update message without confirmation', (tester) async {
    final service = _OlderRunner();
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tester.ensureVisible(find.textContaining('Update the selected Runner and reconnect'));
    expect(find.textContaining('Update the selected Runner and reconnect'), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(service.confirmations, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Git review requires digest-bound diff and supports paging and before-after text', (tester) async {
    final service = _DiffJobs();
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    expect(tester.widget<FilledButton>(find.byKey(const ValueKey('automation-confirm-run'))).onPressed, isNull);
    await tapKey(tester, 'automation-review-diff');
    await tester.tap(find.text('file-0.yml'));
    await settle(tester);
    expect(find.text('original YAML'), findsOneWidget);
    expect(find.text('edited YAML'), findsOneWidget);
    await tester.tap(find.text('Back').hitTestable());
    await settle(tester);
    await tester.tap(find.byTooltip('Next'));
    await settle(tester);
    expect(find.text('file-50.yml'), findsOneWidget);
    expect(service.offsets, [0, 50]);
    expect(service.digests, everyElement('fixture-revision-proof'));
    await tapKey(tester, 'automation-diff-reviewed');
    await tapKey(tester, 'automation-confirm-run');
    expect(service.confirmations, 1);
    expect(tester.takeException(), isNull);
  });
  testWidgets('stale Git diff cannot enable confirmation', (tester) async {
    final service = _DiffJobs()..failDiff = true;
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-review-diff');
    expect(tester.widget<FilledButton>(find.byKey(const ValueKey('automation-diff-reviewed'))).onPressed, isNull);
    expect(service.confirmations, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('a late Git diff is discarded after lock', (tester) async {
    final service = _DiffJobs()..pendingDiff = Completer<AutomationJson>();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-review-diff');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pendingDiff!.complete(service.diffPage(0));
    await settle(tester);
    expect(find.text('file-0.yml'), findsNothing);
    expect(service.confirmations, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('external-only runs show only explicitly bound saved targets', (tester) async {
    final service = _InventoryJobs();
    await _open(tester, service);
    expect(find.text('external_target · Target fixture'), findsOneWidget);
    expect(find.byKey(ValueKey('automation-target-${service.unbound.id}')), findsNothing);
    expect(tester.getSize(find.byKey(const ValueKey('automation-run-targets'))).height, lessThanOrEqualTo(100));
    await tapKey(tester, 'automation-validate-run');
    expect(service.prepared!['host_ids'], [service.host.id.value]);
    expect(service.confirmations, 0);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(tester.takeException(), isNull);
  });
  testWidgets('mixed inventory offers unbound saved hosts without selecting them', (tester) async {
    final service = _InventoryJobs()..mixed = true;
    await _open(tester, service);
    expect(
      tester.widget<CheckboxListTile>(find.byKey(ValueKey('automation-target-${service.unbound.id}'))).value,
      false,
    );
    await tapKey(tester, 'automation-validate-run');
    expect(service.prepared!['host_ids'], [service.host.id.value]);
    await tester.tap(find.text('Cancel').last);
    await settle(tester);
    expect(tester.takeException(), isNull);
  });
  testWidgets('pending inventory configuration cannot enable a run or restore it after lock', (tester) async {
    final service = _InventoryJobs()..pendingConfiguration = Completer<AutomationJson>();
    final container = await _open(tester, service, selectTarget: false);
    expect(isEnabled(tester, 'automation-validate-run'), false);
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pendingConfiguration!.complete(service.configuration());
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-validate-run')), findsNothing);
    expect(service.prepared, isNull);
    expect(tester.takeException(), isNull);
  });
  testWidgets('a pending retry cannot reopen project settings after locking', (tester) async {
    final service = _Events()..state = 'failed';
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    service.pendingFiles = Completer<AutomationProjectRevision>();
    await tapKey(tester, 'automation-workspace-jobs');
    await tester.tap(find.byKey(const ValueKey('automation-retry-job-fixture')));
    await settle(tester);
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pendingFiles!.complete(
      const AutomationProjectRevision(
        digest: 'late',
        bytes: 0,
        entries: [AutomationFileEntry(path: 'playbook.yml', directory: false)],
      ),
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-validate-run')), findsNothing);
    expect(service.confirmations, 1);
    expect(tester.takeException(), isNull);
  });
  testWidgets('retry opens fresh settings without submitting and preserves the previous job link', (tester) async {
    final service = _Events()..state = 'failed';
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    expect(service.confirmations, 1);
    service.prepared = null;
    await tapKey(tester, 'automation-workspace-jobs');
    await tapKey(tester, 'automation-retry-job-fixture');
    expect(service.prepared, isNull);
    expect(service.confirmations, 1);
    expect(find.textContaining('This is a new job.'), findsOneWidget);
    final selects = find.byType(GlassSelect<String?>);
    tester.widget<GlassSelect<String?>>(selects.first).onChanged!('runner');
    await tester.pump();
    final target = find.byKey(ValueKey('automation-target-${service.host.id}'));
    await Scrollable.ensureVisible(tester.element(target), alignment: .5);
    await settle(tester);
    expect(target.hitTestable(), findsOneWidget);
    await tester.tap(target);
    await tester.pump();
    await tapKey(tester, 'automation-validate-run');
    expect(service.prepared?['retry_of'], 'job-fixture');
    expect(service.prepared?['mode'], 'check');
    expect(service.confirmations, 1);
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('history follows the verified cursor without duplicating tasks', (tester) async {
    final service = _Events();
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    await tapKey(tester, 'automation-workspace-jobs');
    expect(find.text('Ansible groups · web (1)'), findsNothing);
    expect(find.text('job-fixture'), findsNothing);
    final card = find.byKey(const ValueKey('automation-history-job-job-fixture'));
    expect(tester.getSize(card).height, lessThan(200));
    await captureAutomationWorkspace(tester, 'jobs-compact-history');
    await tapKey(tester, 'automation-job-details-job-fixture');
    expect(find.text('Ansible groups · web (1)'), findsOneWidget);
    await tapKey(tester, 'automation-workspace-jobs');
    await tapKey(tester, 'automation-events-job-fixture');
    expect(service.cursors.first, 0);
    await settle(tester, steps: 25);
    expect(service.cursors.skip(1), everyElement(2));
    expect(find.textContaining('fixture-event-one'), findsOneWidget);
    expect(find.textContaining('fixture-event-two'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('late journal pages never restore plaintext after locking', (tester) async {
    final service = _Events()..pendingEvents = Completer<AutomationJson>();
    final container = await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    await tapKey(tester, 'automation-confirm-run');
    await tapKey(tester, 'automation-workspace-jobs');
    await tester.tap(find.byKey(const ValueKey('automation-events-job-fixture')));
    await tester.pump();
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.pendingEvents!.complete(service.page(0));
    await settle(tester);
    expect(find.textContaining('fixture-event-one'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('validation never executes and confirmation sends only the native review ID', (tester) async {
    final service = _Api();
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    expect(service.prepared?['host_ids'], [service.host.id.value]);
    expect(service.prepared?['mode'], 'check');
    expect(service.confirmations, 0);
    expect(find.text('Review job'), findsOneWidget);
    expect(find.text('WSL · Ubuntu 24.04 · runner'), findsOneWidget);
    await tester.ensureVisible(find.textContaining('jump@bastion.example.test:2222'));
    expect(find.textContaining('jump@bastion.example.test:2222 → runner@target.example.test:22'), findsOneWidget);
    expect(find.text('fixture-revision-proof'), findsOneWidget);
    await tester.scrollUntilVisible(find.byKey(const ValueKey('automation-review-technical')), 280,
      scrollable: find.descendant(of: find.byKey(const ValueKey('automation-run-scroll')),
        matching: find.byType(Scrollable)).first);
    await tester.tap(find.text('Snapshot technical details'));
    await settle(tester);
    await tester.ensureVisible(find.text('fixture-environment-proof'));
    expect(find.text('fixture-environment-proof'), findsOneWidget);
    await tapKey(tester, 'automation-confirm-run');
    expect(service.confirmations, 1);
    expect(service.confirmedId, 'opaque-native-review');
    expect(find.text('Jobs'), findsWidgets);
    expect(tester.takeException(), isNull);
  });
  testWidgets('failed checks expose diagnostics without an execution button', (tester) async {
    final service = _Api()..valid = false;
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(service.confirmations, 0);
    expect(find.text('fixture syntax failure').hitTestable(), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-validation-check-syntax')).hitTestable(), findsOneWidget);
    await captureAutomationWorkspace(tester, 'failed-validation-inline');
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('failed validation exposes timeout cancellation and bounded-output flags beside diagnostics', (
    tester,
  ) async {
    final service = _Api()..pending = Completer<AutomationJson>();
    await _open(tester, service);
    await tapKey(tester, 'automation-validate-run');
    final review = service.review();
    review['validation'] = {
      'valid': false,
      'checks': [
        {'name': 'lint', 'exit_code': -1, 'output': '', 'timed_out': true, 'cancelled': true, 'truncated': true},
      ],
    };
    service.pending!.complete(review);
    await settle(tester);
    expect(find.text('The check exceeded its time limit.').hitTestable(), findsOneWidget);
    expect(find.text('The check returned no diagnostic output.').hitTestable(), findsOneWidget);
    expect(find.text('Cancelled'), findsOneWidget);
    expect(find.textContaining('Output is truncated'), findsWidgets);
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(service.confirmations, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('lock removes an in-flight review and a late response cannot display targets', (tester) async {
    final service = _Api()..pending = Completer<AutomationJson>();
    final container = await _open(tester, service);
    await tester.tap(find.byKey(const ValueKey('automation-validate-run')));
    await tester.pump();
    await container.read(appServicesProvider).vault.lock();
    await settle(tester);
    service.pending!.complete(service.review());
    await settle(tester);
    expect(find.text('Review job'), findsNothing);
    expect(find.text('fixture-route-proof'), findsNothing);
    expect(service.confirmations, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('run settings remain scrollable on a narrow desktop window', (tester) async {
    final service = _Api();
    await _open(tester, service, size: const Size(500, 850));
    final targetRect = tester.getRect(find.byKey(ValueKey('automation-target-${service.host.id}')));
    final modeRect = tester.getRect(find.byType(GlassSelect<bool>));
    expect(tester.getSize(find.byKey(const ValueKey('automation-run-targets'))).height, lessThanOrEqualTo(100));
    expect(modeRect.top - targetRect.bottom, inInclusiveRange(16, 100));
    await captureAutomationWorkspace(tester, 'run-composer-narrow');
    await tapKey(tester, 'automation-validate-run');
    expect(find.text('Review job'), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
}
