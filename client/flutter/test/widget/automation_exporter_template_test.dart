import 'dart:convert';
import 'dart:io' as io;

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/automation/agent_installer_project_focus.dart';
import 'package:consolecrypt/automation/automation_agent_installer.dart';
import 'package:consolecrypt/automation/automation_exporter_template.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_run_dialog.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:crypto/crypto.dart' as digest;
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class Projects implements AutomationService {
  final projectsCreated = <AutomationProject>[];
  final contents = <String, String>{'playbook.yml': '---\n[]\n'};
  final expected = <String, String?>{};
  String? readShaOverride;
  @override
  Future<AutomationProjectCatalog> projects() async =>
      AutomationProjectCatalog(version: 1, readOnly: false, deviceLocal: true, projects: projectsCreated);
  @override
  Future<AutomationProject> createProject({required String name, String? directory}) async {
    expect(directory, isNull);
    final created = AutomationProject(
      id: '00000000-0000-4000-8000-000000000001',
      name: name,
      readOnly: false,
      sourceKind: 'managed',
      revision: 1,
    );
    projectsCreated.add(created);
    return created;
  }

  @override
  Future<AutomationProjectRevision> files(String projectId) async => AutomationProjectRevision(
    digest: 'a' * 64,
    bytes: 100,
    entries: [for (final path in contents.keys) AutomationFileEntry(path: path, directory: false)],
  );
  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async =>
      AutomationProjectFile(path: path, text: contents[path]!, sha256: readShaOverride ?? 'b' * 64, lineEndings: 'lf');
  @override
  Future<AutomationProjectFile> saveFile(
    String projectId, {
    required String path,
    required String text,
    String? expectedSha256,
  }) async {
    expected[path] = expectedSha256;
    contents[path] = text;
    return AutomationProjectFile(path: path, text: text, sha256: 'c' * 64, lineEndings: 'lf');
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('no execution or credential capability');
}

/// Supports the actual install menu without granting a fake execution result.
class InstallProjects extends Projects implements AutomationJobService, AutomationRunnerService {
  int prepares = 0, confirms = 0, installationRequests = 0, connects = 0, otherWrites = 0;
  @override
  Future<AutomationProjectFile> readFile(String projectId, String path) async {
    final text = contents[path]!;
    return AutomationProjectFile(path: path, text: text,
      sha256: digest.sha256.convert(utf8.encode(text)).toString(), lineEndings: 'lf');
  }
  @override
  Future<AutomationJson> prepareRun(AutomationJson request) async {
    prepares++;
    if ((request['parameters'] as AutomationJson?)?['extra_vars'] case final Map<String, dynamic> vars) {
      if (vars['cc_exporter_mode'] == 'install') installationRequests++;
    }
    throw StateError('Native preparation requires an explicit user action');
  }
  @override
  Future<AutomationJson> confirmRun(String reviewId) async {
    confirms++;
    throw StateError('Native confirmation requires an explicit user action');
  }
  @override
  Future<AutomationJson> jobs() async => {'schema_version': 1, 'read_only': false, 'jobs': <AutomationJson>[]};
  @override
  Future<AutomationRunnerCatalog> runners() async => const AutomationRunnerCatalog(readOnly: false, runners: []);
  @override
  Future<AutomationJson> connectRunner(String id) async {
    connects++;
    throw StateError('No automatic Runner connection');
  }
  @override
  Future<void> discardReview(String id) async { otherWrites++; }
  @override
  Future<void> cancelJob(String id) async { otherWrites++; }
  @override
  Future<void> acknowledgeUnknown(String id, {required bool acceptPartialChanges}) async { otherWrites++; }
  @override
  Future<AutomationRunnerProfile> createRunner({required String name, required AutomationJson configuration}) async {
    otherWrites++;
    throw StateError('No automatic Runner setup');
  }
  @override
  Future<void> disconnectRunner(String id) async { otherWrites++; }
  @override
  Future<void> forgetRunner(String id, {required int revision}) async { otherWrites++; }
}

Future<ProviderContainer> open(
  WidgetTester tester,
  Projects service, {
  AppLocale locale = AppLocale.en,
  Size size = const Size(1600, 1000),
  bool withHost = false,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Owned template UI');
  if (withHost) {
    await backend.inventory.saveHost(Host.create(name: 'Owned cAdvisor target', address: '192.0.2.12'));
  }
  await pumpApp(
    tester,
    backend,
    locale: locale,
    size: size,
    overrides: [automationServiceProvider.overrideWithValue(service)],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.automation);
  await settle(tester);
  return container;
}

void main() {
  setUp(() {
    // Cached asset Futures retain their creating test's FakeAsync zone.
    // Each test must load the real shipped bytes in its own active zone.
    for (final asset in const [
      'assets/automation/exporters/cadvisor.yml',
      'assets/automation/exporters/node-exporter.yml',
      'assets/automation/exporters/node-exporter.service.j2',
    ]) {
      rootBundle.evict(asset);
    }
  });
  for (final fromHost in [false, true]) {
  testWidgets('cAdvisor ${fromHost ? 'original Host intent' : 'install menu'} fills ordinary Run without executing', (tester) async {
    final service = InstallProjects();
    final container = await open(tester, service, withHost: fromHost);
    AgentCadvisorComposerFocus? focus;
    var originalHost = true;
    if (fromHost) {
      final scope = container.read(automationScopeProvider);
      final host = container.read(hostsProvider).value!.firstWhere((h) => h.protocol == HostProtocol.ssh);
      final frame = AgentInstallerFrame(profileId: scope.profileId.toString(), notificationScopeId: 'original host test');
      final metadata = AgentInstallerContext.parse({...frame.toJson(), 'hosts': [
        {'hostId': host.id.value, 'hostRevision': '9007199254740993', 'name': host.name},
      ]}, frame);
      focus = AgentCadvisorComposerFocus(host: metadata.hosts.single, frame: frame,
        returnUri: Uri.parse(AppRoutes.hosts), scope: scope, service: service, original: () => originalHost);
      container.read(routerProvider).go(AppRoutes.automation, extra: focus);
      await settle(tester);
    } else {
      await tapKey(tester, 'automation-project-actions');
      await tapKey(tester, 'automation-cadvisor-install');
    }
    expect(find.textContaining('--privileged'), findsOneWidget);
    expect(service.projectsCreated, isEmpty);
    await tapKey(tester, 'automation-cadvisor-add-source');
    await tester.enterText(find.descendant(of: find.byType(AlertDialog), matching: find.byType(TextField)),
      'Reviewed cAdvisor installation');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await settle(tester, steps: 25);
    expect(find.byKey(const ValueKey('automation-cadvisor-install-options')), findsOneWidget);
    expect(tester.widget<TextButton>(find.byKey(const ValueKey('cadvisor-install-compose'))).onPressed, isNull);
    await enterKey(tester, 'cadvisor-install-image-digest', 'a' * 64);
    await enterKey(tester, 'cadvisor-install-image-id', 'sha256:${'b' * 64}');
    await enterKey(tester, 'cadvisor-install-port', '19102');
    await tapKey(tester, 'cadvisor-install-consent');
    await tapKey(tester, 'cadvisor-install-compose');
    final composer = find.byType(AutomationRunDialog);
    expect(composer, findsOneWidget);
    expect(tester.widget<AutomationRunDialog>(composer).cadvisorTemplate?.project.id,
      service.projectsCreated.single.id);
    if (fromHost) {
      final run = tester.widget<AutomationRunDialog>(composer);
      expect(identical(run.cadvisorFocus, focus), isTrue);
      expect(run.cadvisorOriginal?.call(), isTrue);
      final hostFocus = focus!;
      final selectedTarget = find.byKey(ValueKey('automation-target-${hostFocus.hostId}'));
      await tester.scrollUntilVisible(selectedTarget, 160, maxScrolls: 15, scrollable: find.descendant(
        of: find.byKey(const ValueKey('automation-run-scroll')), matching: find.byType(Scrollable)).first);
      final target = tester.widget<CheckboxListTile>(selectedTarget);
      expect(target.value, isTrue);
      expect(target.onChanged, isNull);
      expect(hostFocus.take(container.read(automationScopeProvider), service), isFalse);
      originalHost = false;
      expect(run.cadvisorOriginal?.call(), isFalse);
    }
    final prefilled = find.byWidgetPredicate((widget) => widget is TextField &&
      widget.controller?.text.contains('"cc_exporter_mode"') == true);
    await tester.scrollUntilVisible(prefilled, 240, maxScrolls: 15, scrollable: find.descendant(
      of: find.byKey(const ValueKey('automation-run-scroll')), matching: find.byType(Scrollable)).first);
    final values = jsonDecode(tester.widget<TextField>(prefilled).controller!.text);
    expect(values, {
      'cc_exporter_mode': 'install', 'cc_cadvisor_version': '0.60.6',
      'cc_cadvisor_image_sha256': 'a' * 64, 'cc_cadvisor_image_id': 'sha256:${'b' * 64}',
      'cc_cadvisor_owner': service.projectsCreated.single.id, 'cc_cadvisor_port': 19102,
      'cc_cadvisor_host_access': 'privileged-host-filesystems-and-kmsg',
    });
    expect(find.byKey(const ValueKey('automation-validate-run')), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-confirm-run')), findsNothing);
    expect(service.prepares, 0);
    expect(service.confirms, 0);
    expect(service.installationRequests, 0);
    expect(service.connects, 0);
    expect(service.otherWrites, 0);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
  test('typed cAdvisor draft requires consent and exact original template/project revision', () async {
    final service = Projects();
    final project = await service.createProject(name: 'Any name is not template identity');
    final source = await io.File('assets/automation/exporters/cadvisor.yml').readAsString();
    service.contents['playbook.yml'] = source;
    service.readShaOverride = automationCadvisorTemplateSha256;
    final binding = await AutomationCadvisorTemplateBinding.capture(service, project.id, isCurrent: () => true);
    final options = AutomationCadvisorInstallOptions(imageSha256: 'a' * 64, imageId: 'sha256:${'b' * 64}',
      owner: project.id, port: 19101, hostAccessReviewed: true);
    expect(options.extraVars, {
      'cc_exporter_mode': 'install', 'cc_cadvisor_version': '0.60.6',
      'cc_cadvisor_image_sha256': 'a' * 64, 'cc_cadvisor_image_id': 'sha256:${'b' * 64}',
      'cc_cadvisor_owner': project.id, 'cc_cadvisor_port': 19101,
      'cc_cadvisor_host_access': 'privileged-host-filesystems-and-kmsg',
    });
    expect(() => AutomationCadvisorInstallOptions(imageSha256: 'a' * 64, imageId: 'sha256:${'b' * 64}',
      owner: project.id, port: 19101, hostAccessReviewed: false), throwsFormatException);
    await binding.recheck(service, isCurrent: () => true);
    // A matching filename or borrowed digest cannot adopt modified contents.
    service.contents['playbook.yml'] = '$source\n# different project contents\n';
    await expectLater(binding.recheck(service, isCurrent: () => true), throwsFormatException);
    service.contents['playbook.yml'] = source;
    service.projectsCreated[0] = AutomationProject(id: project.id, name: project.name,
      readOnly: false, sourceKind: 'managed', revision: 2);
    await expectLater(binding.recheck(service, isCurrent: () => true), throwsFormatException);
    await expectLater(binding.recheck(service, isCurrent: () => false), throwsFormatException);
    expect(service.expected, isEmpty); // No writes, Runner connection, preparation or execution.
  });


  test('cAdvisor capture pins canonical bytes and rejects stale or self-consistent edited sources', () async {
    final service = Projects();
    final project = await service.createProject(name: 'Canonical pin regression');
    final source = await io.File('assets/automation/exporters/cadvisor.yml').readAsString();
    final actual = digest.sha256.convert(utf8.encode(source)).toString();
    expect(actual, automationCadvisorTemplateSha256);
    service.contents['playbook.yml'] = source;
    service.readShaOverride = actual;
    final binding = await AutomationCadvisorTemplateBinding.capture(service, project.id, isCurrent: () => true);
    expect(binding.project.id, project.id);
    // Current bytes do not make a stale peer-reported digest trustworthy.
    service.readShaOverride = 'a341f805b5b1db61431c267cd59763de227160a06a1b929a0d1a7e1d1f6e1453';
    await expectLater(AutomationCadvisorTemplateBinding.capture(service, project.id, isCurrent: () => true),
      throwsFormatException);
    // A matching hash of modified bytes still cannot replace the reviewed pin.
    final edited = '$source\n# unreviewed template edit\n';
    service.contents['playbook.yml'] = edited;
    service.readShaOverride = digest.sha256.convert(utf8.encode(edited)).toString();
    await expectLater(AutomationCadvisorTemplateBinding.capture(service, project.id, isCurrent: () => true),
      throwsFormatException);
    expect(service.expected, isEmpty);
  });
  test('cAdvisor lifecycle binds one Host and original container to a closed component manifest', () {
    const owner = '00000000-0000-4000-8000-000000000001';
    const host = '00000000-0000-4000-8000-000000000002';
    for (final operation in AutomationCadvisorOperation.values) {
      final options = AutomationCadvisorInstallOptions(imageSha256: 'a' * 64,
        imageId: 'sha256:${'b' * 64}', owner: owner, port: 19101, hostAccessReviewed: true,
        operation: operation, containerId: operation == AutomationCadvisorOperation.install ? null : 'c' * 64);
      final bound = bindCadvisorComponentManifest(options.extraVars, host);
      final manifest = bound['cc_cadvisor_component_manifest'] as AutomationJson;
      expect(manifest['managedBy'], 'reviewedAnsible');
      expect(manifest['hostId'], host);
      expect(manifest['owner'], owner);
      expect(manifest['bindIp'], '127.0.0.1');
      expect(bound['cc_exporter_mode'], operation.name);
      final keys = manifest.keys.toList()..sort();
      expect(bound['cc_cadvisor_manifest_sha256'], digest.sha256.convert(utf8.encode(
        jsonEncode({for (final key in keys) key: manifest[key]}))).toString());
      final otherHost = bindCadvisorComponentManifest(options.extraVars, owner);
      expect(otherHost['cc_cadvisor_manifest_sha256'], isNot(bound['cc_cadvisor_manifest_sha256']));
      if (operation != AutomationCadvisorOperation.install) {
        expect(bound['cc_cadvisor_container_id'], 'c' * 64);
        expect(() => AutomationCadvisorInstallOptions(imageSha256: 'a' * 64,
          imageId: 'sha256:${'b' * 64}', owner: owner, port: 19101, hostAccessReviewed: true,
          operation: operation), throwsFormatException);
      }
      expect(() => bindCadvisorComponentManifest({...options.extraVars, 'consolecrypt_actual': <String, dynamic>{}}, host),
        throwsFormatException);
      expect(() => bindCadvisorComponentManifest({...options.extraVars, 'cc_exporter_mode': 'restart'}, host),
        throwsFormatException);
    }
  });
  for (final template in ['node-exporter', 'cadvisor']) {
    testWidgets('$template project template writes reviewed inspect source without execution', (tester) async {
      final service = Projects();
      await open(tester, service);
      await tapKey(tester, 'automation-project-actions');
      await tapKey(tester, 'automation-$template-template');
      if (template == 'cadvisor') {
        expect(service.projectsCreated, isEmpty);
        expect(service.expected, isEmpty);
        await tapKey(tester, 'automation-cadvisor-add-source');
      }
      final field = find.descendant(of: find.byType(AlertDialog), matching: find.byType(TextField));
      await tester.enterText(field, 'Reviewed exporter');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await settle(tester, steps: 25);
      expect(service.projectsCreated.length, 1);
      expect(service.contents['playbook.yml'], contains('cc_exporter_mode: inspect'));
      expect(service.expected['playbook.yml'], 'b' * 64);
      if (template == 'node-exporter') {
        expect(service.contents.containsKey('node-exporter.service.j2'), isTrue);
        expect(service.expected['node-exporter.service.j2'], isNull);
        expect(service.contents['playbook.yml'], contains('use_netrc: false'));
      } else {
        expect(service.contents['playbook.yml'], contains('privileged-host-filesystems-and-kmsg'));
      }
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
  testWidgets('cancelling template project name creates no project or file', (tester) async {
    final service = Projects();
    await open(tester, service);
    await tapKey(tester, 'automation-project-actions');
    await tapKey(tester, 'automation-node-exporter-template');
    await tester.tap(find.descendant(of: find.byType(AlertDialog), matching: find.text('Cancel')));
    await settle(tester);
    expect(service.projectsCreated, isEmpty);
    expect(service.expected, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('cAdvisor explicit privilege review cancel produces zero project or file writes', (tester) async {
    final service = Projects();
    await open(tester, service);
    await tapKey(tester, 'automation-project-actions');
    await tapKey(tester, 'automation-cadvisor-template');
    expect(find.textContaining('--privileged'), findsOneWidget);
    expect(find.textContaining('--device=/dev/kmsg'), findsOneWidget);
    expect(find.textContaining('--volume=/var/run:/var/run:ro'), findsOneWidget);
    expect(find.textContaining('does not make the Docker API read-only'), findsOneWidget);
    expect(service.projectsCreated, isEmpty);
    expect(service.expected, isEmpty);
    await tapKey(tester, 'automation-cadvisor-cancel');
    expect(find.byKey(const ValueKey('automation-cadvisor-review')), findsNothing);
    expect(service.projectsCreated, isEmpty);
    expect(service.expected, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  for (final afterConsent in [false, true]) {
    testWidgets('cAdvisor original scope replacement before write afterConsent=$afterConsent', (tester) async {
      final service = Projects();
      final container = await open(tester, service);
      await tapKey(tester, 'automation-project-actions');
      await tapKey(tester, 'automation-cadvisor-template');
      if (afterConsent) await tapKey(tester, 'automation-cadvisor-add-source');
      final previous = container.read(automationScopeProvider);
      container.invalidate(automationScopeProvider);
      expect(identical(previous, container.read(automationScopeProvider)), false);
      await settle(tester);
      expect(find.byKey(const ValueKey('automation-cadvisor-review')), findsNothing);
      expect(find.byType(AlertDialog), findsNothing);
      expect(service.projectsCreated, isEmpty);
      expect(service.expected, isEmpty);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
  testWidgets('cAdvisor service replacement retires reviewed route without adoption', (tester) async {
    final first = Projects(), second = Projects();
    final container = await open(tester, first);
    await tapKey(tester, 'automation-project-actions');
    await tapKey(tester, 'automation-cadvisor-template');
    final app = container.read(appServicesProvider);
    container.updateOverrides([
      appServicesProvider.overrideWithValue(app),
      automationServiceProvider.overrideWithValue(second),
    ]);
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-cadvisor-review')), findsNothing);
    expect(first.projectsCreated, isEmpty);
    expect(second.projectsCreated, isEmpty);
    expect(first.expected, isEmpty);
    expect(second.expected, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('RU cAdvisor disclosure stays bounded and actionable in a narrow scaled viewport', (tester) async {
    final service = Projects();
    await open(tester, service, locale: AppLocale.ru);
    await tapKey(tester, 'automation-project-actions');
    await tapKey(tester, 'automation-cadvisor-template');
    // Resize the actual modal after opening from the full existing workspace.
    tester.view.physicalSize = const Size(500, 700);
    final context = tester.element(find.byKey(const ValueKey('automation-cadvisor-review')));
    final original = MediaQuery.of(context);
    // The full-app setting supplies scale via the platform dispatcher.
    tester.platformDispatcher.textScaleFactorTestValue = 1.5;
    addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
    await settle(tester);
    expect(original.size.width, greaterThan(0));
    expect(find.textContaining('не делает Docker API'), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-cadvisor-cancel')).hitTestable(), findsOneWidget);
    expect(find.byKey(const ValueKey('automation-cadvisor-add-source')).hitTestable(), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tapKey(tester, 'automation-cadvisor-cancel');
    expect(service.projectsCreated, isEmpty);
    expect(service.expected, isEmpty);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('reviewed disclosure literals match the actual shipped fixed cAdvisor asset', (tester) async {
    final source = (await tester.runAsync(() => io.File('assets/automation/exporters/cadvisor.yml').readAsString()))!;
    for (final flag in [
      '--privileged',
      '--device=/dev/kmsg',
      '--volume=/:/rootfs:ro',
      '--volume=/var/run:/var/run:ro',
      '--volume=/sys:/sys:ro',
      '--volume=/var/lib/docker:/var/lib/docker:ro',
      '--volume=/dev/disk:/dev/disk:ro',
      '--restart=unless-stopped',
      '--host=unix:///var/run/docker.sock',
      "cc_cadvisor_host_access == 'privileged-host-filesystems-and-kmsg'",
      'cc_exporter_mode: inspect',
      'become: true',
    ]) {
      expect(source, contains(flag));
    }
    expect(source, contains('--publish=127.0.0.1:{{ cc_cadvisor_port }}:8080'));
    expect(source, contains("['inspect', 'install', 'start', 'stop', 'remove']"));
    expect(source, contains('managedBy: reviewedAnsible'));
    expect(source, contains("hostId: '{{ consolecrypt_host_id }}'"));
    expect(source, contains('consolecrypt_actual.id == consolecrypt_expected_container_id'));
    expect(source, contains('consolecrypt_actual.host == consolecrypt_host_id'));
    expect(source, contains('consolecrypt_actual.manifestSha256 == consolecrypt_cadvisor_manifest_digest'));
    expect(source, contains('that: *cc_cadvisor_owned_constraints'));
    expect(source.indexOf('Deny foreign identity'), lessThan(source.indexOf('Start only the already verified')));
    for (final forbidden in ['--force', '--volumes', '--rm']) {
      expect(source, isNot(contains(forbidden)));
    }
  });
}
