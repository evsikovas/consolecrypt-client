import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_groups_dialog.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart' show settle, tapKey;

const scope = AutomationScope('owned-folder-profile', true);

class _Folders implements AutomationInventoryService, AutomationFolderBindingService {
  final folder = Group.create('Duplicate folder');
  final duplicate = Group.create('Duplicate folder');
  late final member = Host.create(name: 'Owned SSH member', address: '192.0.2.1').withGroup(folder.id);
  final calls =
      <
        ({
          String project,
          String profile,
          String nonce,
          int revision,
          String name,
          AutomationFolderBindingAction action,
          String? folder,
        })
      >[];
  Completer<AutomationJson>? pending;
  AutomationJson? saved;
  String profile = 'owned-folder-profile';
  int revision = 1;
  Map<String, dynamic> bindings = {};
  Map<String, dynamic> groups = {'docker_hosts': <String>[], 'manual': <String>[]};
  @override
  Future<AutomationJson> inventory(String projectId) async => {
    'version': 1,
    'project_id': projectId,
    'project_revision': revision,
    'profile_id': profile,
    'notification_scope_id': '11111111-1111-4111-8111-111111111111',
    'inventory': {'version': 1, 'groups': groups},
    'folder_mapping': {
      'bindings': {'version': 1, 'groups': bindings},
      'status': 'available',
    },
  };
  AutomationJson reply(String project, String name, AutomationFolderBindingAction action, String? id) {
    revision++;
    if (action == AutomationFolderBindingAction.linkDirect) {
      bindings[name] = {'folder_id': id, 'mode': 'direct'};
      groups[name] = <String>[];
    } else if (action == AutomationFolderBindingAction.snapshotDirect) {
      bindings.remove(name);
      groups[name] = id == folder.id.value ? [member.id.value] : <String>[];
    } else {
      bindings.remove(name);
    }
    return {
      'version': 1,
      'project': {'id': project, 'revision': revision, 'read_only': false},
      'inventory': {'version': 1, 'groups': groups},
      'folder_bindings': {'version': 1, 'groups': bindings},
      'composition_status': 'available',
    };
  }

  @override
  Future<AutomationJson> folderBinding(
    String projectId, {
    required String profileId,
    required String notificationScopeId,
    required int revision,
    required String groupName,
    required AutomationFolderBindingAction action,
    String? folderId,
  }) async {
    calls.add((
      project: projectId,
      profile: profileId,
      nonce: notificationScopeId,
      revision: revision,
      name: groupName,
      action: action,
      folder: folderId,
    ));
    if (pending != null) return await pending!.future;
    return reply(projectId, groupName, action, folderId);
  }

  @override
  Future<AutomationProject> saveInventory(
    String projectId, {
    required int revision,
    required AutomationJson inventory,
  }) async {
    saved = inventory;
    return AutomationProject(id: projectId, name: 'Owned project', revision: revision + 1, readOnly: false);
  }
}

Future<ProviderContainer> _open(
  WidgetTester tester,
  _Folders service, {
  Size size = const Size(1200, 1100),
  Locale locale = const Locale('en'),
  double textScale = 1,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        automationScopeProvider.overrideWithValue(scope),
        groupsProvider.overrideWith((_) => Stream.value([service.folder, service.duplicate])),
        hostsProvider.overrideWith((_) => Stream.value([service.member])),
      ],
      child: MaterialApp(
        theme: AppTheme.dark(platform: TargetPlatform.windows),
        locale: locale,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(textScale)),
          child: child!,
        ),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              key: const ValueKey('open-owned-folder-dialog'),
              onPressed: () => showDialog<void>(
                context: context,
                builder: (_) => AutomationGroupsDialog(scope: scope, projectId: 'owned-project', service: service),
              ),
              child: const Text('Fixture'),
            ),
          ),
        ),
      ),
    ),
  );
  await tapKey(tester, 'open-owned-folder-dialog');
  await settle(tester);
  return ProviderScope.containerOf(tester.element(find.byType(AutomationGroupsDialog)));
}

Future<void> choose(WidgetTester tester, String folder) async {
  await tester.ensureVisible(find.byKey(const ValueKey('automation-group-folder')));
  await tapKey(tester, 'automation-group-folder');
  await tapKey(tester, 'automation-folder-option-$folder');
}

void main() {
  testWidgets('folder selection uses stable ID despite duplicate labels and explicit link CAS', (tester) async {
    final service = _Folders();
    await _open(tester, service);
    expect(service.calls, isEmpty);
    await choose(tester, service.duplicate.id.value);
    await tapKey(tester, 'automation-folder-link');
    expect(service.calls.single, (
      project: 'owned-project',
      profile: 'owned-folder-profile',
      nonce: '11111111-1111-4111-8111-111111111111',
      revision: 1,
      name: 'docker_hosts',
      action: AutomationFolderBindingAction.linkDirect,
      folder: service.duplicate.id.value,
    ));
    expect(service.saved, isNull); // Link is metadata, never a run target selection.
    expect(tester.widget<TextButton>(find.byKey(const ValueKey('automation-group-delete'))).onPressed, isNull);
    expect(
      tester.widget<CheckboxListTile>(find.byKey(ValueKey('automation-group-host-${service.member.id}'))).onChanged,
      isNull,
    );
  });
  testWidgets('snapshot and unlink are distinct explicit actions with advanced revision', (tester) async {
    final service = _Folders();
    await _open(tester, service);
    await choose(tester, service.folder.id.value);
    await tapKey(tester, 'automation-folder-link');
    await tapKey(tester, 'automation-folder-snapshot');
    expect(service.calls.map((c) => c.action), [
      AutomationFolderBindingAction.linkDirect,
      AutomationFolderBindingAction.snapshotDirect,
    ]);
    expect(service.calls.last.revision, 2);
    expect(
      tester.widget<CheckboxListTile>(find.byKey(ValueKey('automation-group-host-${service.member.id}'))).value,
      isTrue,
    );
    expect(
      tester.widget<CheckboxListTile>(find.byKey(ValueKey('automation-group-host-${service.member.id}'))).onChanged,
      isNotNull,
    );
    await tapKey(tester, 'automation-folder-link');
    await tapKey(tester, 'automation-folder-unlink');
    expect(service.calls.last.action, AutomationFolderBindingAction.unlink);
    expect(service.calls.last.revision, 4);
  });
  testWidgets('lock while mutation is pending hides composition and discards late result', (tester) async {
    final service = _Folders()..pending = Completer<AutomationJson>();
    final container = await _open(tester, service);
    await choose(tester, service.folder.id.value);
    await tapKey(tester, 'automation-folder-link');
    container.updateOverrides([
      automationScopeProvider.overrideWithValue(const AutomationScope('owned-folder-profile', false)),
      groupsProvider.overrideWith((_) => Stream.value([service.folder, service.duplicate])),
      hostsProvider.overrideWith((_) => Stream.value([service.member])),
    ]);
    await settle(tester);
    service.pending!.complete(
      service.reply('owned-project', 'docker_hosts', AutomationFolderBindingAction.linkDirect, service.folder.id.value),
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-folder-unlink')), findsNothing);
    expect(service.saved, isNull);
    expect(tester.takeException(), isNull);
  });
  testWidgets('future mapping cannot become an editable manual fallback', (tester) async {
    final service = _Folders()
      ..bindings = {
        'docker_hosts': {'folder_id': 'not-a-folder', 'mode': 'future'},
      };
    await _open(tester, service);
    expect(service.calls, isEmpty);
    expect(tester.widget<FilledButton>(find.byKey(const ValueKey('automation-groups-save'))).onPressed, isNull);
  });
  testWidgets('link merges only its named group and preserves another unsaved group until Save', (tester) async {
    final service = _Folders();
    await _open(tester, service);
    await tester.enterText(find.byKey(const ValueKey('automation-group-name')), 'draft_group');
    await tapKey(tester, 'automation-group-add');
    await tapKey(tester, 'automation-ansible-group');
    await tapKey(tester, 'automation-ansible-group-docker_hosts');
    await choose(tester, service.folder.id.value);
    await tapKey(tester, 'automation-folder-link');
    await tapKey(tester, 'automation-groups-save');
    final savedGroups = service.saved!['groups'] as Map<String, dynamic>;
    expect(savedGroups['draft_group'], isEmpty);
    expect(savedGroups['docker_hosts'], isEmpty); // Live link persists no stale copied membership.
    expect(service.calls.single.revision, 1);
  });
  testWidgets('reusing dialog state across profile scope epoch discards an old matching-project reply', (tester) async {
    final service = _Folders()..pending = Completer<AutomationJson>();
    final profile = ValueNotifier<AutomationScope>(scope);
    addTearDown(profile.dispose);
    tester.view.physicalSize = const Size(1200, 1100);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      ValueListenableBuilder<AutomationScope>(
        valueListenable: profile,
        builder: (_, current, child) => ProviderScope(
          overrides: [
            automationScopeProvider.overrideWithValue(current),
            groupsProvider.overrideWith((_) => Stream.value([service.folder, service.duplicate])),
            hostsProvider.overrideWith((_) => Stream.value([service.member])),
          ],
          child: MaterialApp(
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            supportedLocales: AppLocalizations.supportedLocales,
            home: Scaffold(
              body: AutomationGroupsDialog(scope: current, projectId: 'owned-project', service: service),
            ),
          ),
        ),
      ),
    );
    await settle(tester);
    await choose(tester, service.folder.id.value);
    await tapKey(tester, 'automation-folder-link');
    service.profile = 'other-profile-with-same-project-id';
    profile.value = const AutomationScope('other-profile-with-same-project-id', true);
    await settle(tester);
    service.pending!.complete(
      service.reply('owned-project', 'docker_hosts', AutomationFolderBindingAction.linkDirect, service.folder.id.value),
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('automation-folder-unlink')), findsNothing);
    expect(
      tester.widget<CheckboxListTile>(find.byKey(ValueKey('automation-group-host-${service.member.id}'))).onChanged,
      isNotNull,
    );
    expect(service.calls, hasLength(1));
    expect(tester.takeException(), isNull);
  });
  for (final size in [const Size(580, 900), const Size(1400, 1000)]) {
    testWidgets('Russian folder mapping controls remain usable at ${size.width}px with larger text', (tester) async {
      final service = _Folders();
      await _open(tester, service, size: size, locale: const Locale('ru'), textScale: 1.4);
      await choose(tester, service.folder.id.value);
      final link = find.byKey(const ValueKey('automation-folder-link'));
      await tester.ensureVisible(link);
      await settle(tester);
      expect(link.hitTestable(), findsOneWidget);
      expect(find.text('1 SSH-хост в папке'), findsOneWidget);
      if (size.width > 1000) {
        // Measure unwanted blank space, not total height: Ahem and real UI
        // fonts legitimately wrap the same text into different line counts.
        final lastHost = find.byKey(ValueKey('automation-group-host-${service.member.id}'));
        final footer = find.byKey(const ValueKey('automation-groups-save'));
        final emptySpace = tester.getRect(footer).top - tester.getRect(lastHost).bottom;
        expect(emptySpace, inInclusiveRange(0, 96));
      }
      expect(
        tester.getRect(link).overlaps(tester.getRect(find.byKey(const ValueKey('automation-groups-save')))),
        isFalse,
      );
      expect(service.calls, isEmpty);
      expect(service.saved, isNull);
      expect(tester.takeException(), isNull);
      await captureAutomationDialog(tester, 'folder-mapping-ru-${size.width < 600 ? 'narrow' : 'wide'}');
    });
  }
}
