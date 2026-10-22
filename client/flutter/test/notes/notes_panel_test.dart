import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_notes.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/inventory_service.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/notes_panel.dart';
import 'package:consolecrypt/notes/notes_providers.dart';
import 'package:consolecrypt/notes/notes_reminders.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

final _host = Host(
  id: const ObjectId('h1'),
  name: 'db-prod',
  address: '10.0.0.5',
  createdAt: DateTime(2026),
  updatedAt: DateTime(2026),
  notes: 'Restart only after 22:00.',
);

const _hostContext = NoteContext(target: NoteObjectTarget(NoteObjectKind.host, 'h1'), label: 'db-prod');

class _Inventory implements InventoryService {
  final saved = <Host>[];
  @override
  Future<Host> saveHost(Host host) async {
    saved.add(host);
    return host;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<ProviderContainer> _pump(
  WidgetTester tester,
  MockNoteService service, {
  NoteContext? current = _hostContext,
  _Inventory? inventory,
  String language = 'en',
  Size size = const Size(420, 800),
  double scale = 1,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        noteServiceProvider.overrideWithValue(service),
        notesUnlockedProvider.overrideWithValue(true),
        noteContextProvider.overrideWith((ref, _) => current),
        hostsProvider.overrideWith((ref) => Stream.value([_host])),
        inventoryServiceProvider.overrideWithValue(inventory ?? _Inventory()),
      ],
      child: MaterialApp(
        locale: Locale(language),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
          child: child!,
        ),
        home: const Scaffold(
          body: NotesReminderWatcher(child: NotesPanel(branch: ShellBranch.terminal)),
        ),
      ),
    ),
  );
  await settle(tester);
  return ProviderScope.containerOf(tester.element(find.byType(NotesPanel)));
}

AppLocalizations _l(WidgetTester tester) => tester.element(find.byType(NotesPanel)).l10n;

void main() {
  testWidgets('a new note in "This object" is attached to it and saved after a pause', (tester) async {
    final service = MockNoteService(seed: const []);
    await _pump(tester, service);
    final l = _l(tester);
    expect(find.text(l.notesEmptyObject('db-prod')), findsNothing, reason: 'the host card notes are shown');
    expect(find.byKey(const ValueKey('notes-host-card')), findsOneWidget);
    await tapKey(tester, 'notes-new');
    await enterKey(tester, 'note-title', 'Rotate keys');
    await tester.pump(const Duration(seconds: 1));
    await settle(tester);
    final saved = await service.notes();
    expect(saved.single.title, 'Rotate keys');
    expect(saved.single.target, _hostContext.target);
    await tapKey(tester, 'note-back');
    expect(find.text('Rotate keys'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('tasks: overdue first, check marks done and moves to completed', (tester) async {
    final now = DateTime.now();
    final service = MockNoteService(
      seed: [
        Note(
          id: 'a',
          title: 'Later',
          task: NoteTask(dueAt: now.add(const Duration(days: 3))),
          updatedAt: now,
        ),
        Note(
          id: 'b',
          title: 'Late',
          task: NoteTask(dueAt: now.subtract(const Duration(hours: 1))),
          updatedAt: now,
        ),
        Note(id: 'c', title: 'Plain note', updatedAt: now),
      ],
    );
    final container = await _pump(tester, service, current: null);
    final l = _l(tester);
    expect(container.read(notesAttentionProvider), 1);
    await tapKey(tester, 'notes-mode-tasks');
    expect(find.text('Plain note'), findsNothing);
    expect(tester.getTopLeft(find.text('Late')).dy, lessThan(tester.getTopLeft(find.text('Later')).dy));
    expect(find.textContaining(l.notesOverdueAt('').trim()), findsOneWidget);
    await tapKey(tester, 'note-done-b');
    expect((await service.notes()).firstWhere((n) => n.id == 'b').task!.done, isTrue);
    expect(find.text(l.notesCompleted(1)), findsOneWidget);
    expect(container.read(notesAttentionProvider), 0);
  });

  testWidgets('search narrows the list; "All" shows where a note belongs', (tester) async {
    final service = MockNoteService(
      seed: [
        const Note(
          id: 'a',
          title: 'Backups',
          body: 'nightly',
          target: NoteDockerTarget(name: 'postgres'),
        ),
        const Note(id: 'b', title: 'Certificates', tags: ['tls']),
      ],
    );
    await _pump(tester, service, current: null);
    expect(find.text('postgres'), findsOneWidget);
    await enterKey(tester, 'notes-search', 'tls');
    expect(find.text('Backups'), findsNothing);
    expect(find.text('Certificates'), findsOneWidget);
  });

  testWidgets('host card notes move into a note before the field is cleared', (tester) async {
    final service = MockNoteService(seed: const []);
    final inventory = _Inventory();
    await _pump(tester, service, inventory: inventory);
    await tapKey(tester, 'notes-host-move');
    final note = (await service.notes()).single;
    expect(note.body, _host.notes);
    expect(note.target, _hostContext.target);
    expect(inventory.saved.single.notes, isEmpty);
    expect(inventory.saved.single.address, _host.address);
  });

  testWidgets('a reminder that went off shows a toast once and is cleared', (tester) async {
    final service = MockNoteService(
      seed: [Note(id: 'r', title: 'Call the vendor', remindAt: DateTime.now().subtract(const Duration(minutes: 1)))],
    );
    await _pump(tester, service, current: null);
    final l = _l(tester);
    expect(find.text(l.notesReminderToast('Call the vendor')), findsOneWidget);
    expect((await service.notes()).single.remindAt, isNull);
    // The toast leaves after 30 s and the cleared reminder does not come back.
    await tester.pump(const Duration(seconds: 31));
    await settle(tester);
    await tester.pump(const Duration(seconds: 31));
    await settle(tester);
    expect(find.text(l.notesReminderToast('Call the vendor')), findsNothing);
  });

  testWidgets('shared notes of a linked object: list, scope, write, read-only others', (tester) async {
    final service = MockNoteService(seed: const [])
      ..sharedLinks['host:h1'] = 'share-1'
      ..shared['share-1'] = [
        const SharedNote(id: 's1', title: 'Firewall rules', authorUserId: 'colleague', body: 'port 5432 only'),
        const SharedNote(
          id: 's2',
          title: 'Container note',
          authorUserId: 'colleague',
          scope: {'docker_container': 'pg'},
        ),
      ];
    await _pump(tester, service);
    final l = _l(tester);
    expect(find.text(l.notesShared), findsOneWidget);
    expect(find.text('Firewall rules'), findsOneWidget);
    // A note about one container of the host stays with that container.
    expect(find.text('Container note'), findsNothing);
    expect(find.text(l.notesAuthorMember), findsOneWidget);
    await tester.tap(find.text('Firewall rules'));
    await settle(tester);
    expect(find.text(l.notesSharedReadOnly), findsOneWidget);
    expect(find.byKey(const ValueKey('shared-note-save')), findsNothing);
    await tester.tap(find.text(l.commonClose));
    await settle(tester);
    await tapKey(tester, 'shared-notes-new');
    await enterKey(tester, 'shared-note-title', 'Maintenance window');
    await tapKey(tester, 'shared-note-save');
    final saved = service.shared['share-1']!.firstWhere((n) => n.title == 'Maintenance window');
    expect(saved.scope, isNull);
    expect(find.text('Maintenance window'), findsOneWidget);
    expect(find.text(l.notesAuthorYou), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a member waits until the owner opens shared notes', (tester) async {
    final service = MockNoteService(seed: const [])
      ..sharedLinks['host:h1'] = 'share-1'
      ..waitingForOwner = true;
    await _pump(tester, service);
    expect(find.text(_l(tester).notesSharedWaiting), findsOneWidget);
    expect(tester.widget<GlassIconButton>(find.byKey(const ValueKey('shared-notes-new'))).onPressed, isNull);
  });

  test('links and scopes name the shared object, not local ids', () {
    expect(noteLinkKey(const NoteDockerTarget(hostId: 'h', name: 'pg')), 'host:h');
    expect(noteLinkKey(const NoteDockerTarget(name: 'pg')), isNull);
    expect(
      noteLinkKey(const NoteKubeTarget(contextId: 'c', resource: 'pods', kind: 'Pod', name: 'p')),
      'kube_context:c',
    );
    expect(noteScope(const NoteDockerTarget(hostId: 'h', name: 'pg')), {'docker_container': 'pg'});
    expect(
      sameNoteScope({
        'kube_resource': {'group': '', 'resource': 'pods', 'namespace': 'a', 'name': 'p'},
      }, noteScope(const NoteKubeTarget(contextId: 'other', resource: 'pods', kind: 'Pod', namespace: 'a', name: 'p'))),
      isTrue,
    );
  });

  for (final language in ['en', 'ru']) {
    testWidgets('notes panel and editor fit a phone at 150% in $language', (tester) async {
      final service = MockNoteService(
        seed: [
          Note(
            id: 't',
            title: 'Renew certificates',
            task: NoteTask(dueAt: DateTime.now()),
            tags: const ['ops'],
          ),
        ],
      );
      await _pump(tester, service, language: language, size: const Size(412, 900), scale: 1.5);
      await tapKey(tester, 'notes-mode-all');
      await tapKey(tester, 'notes-new');
      await tapKey(tester, 'note-task');
      expect(find.byKey(const ValueKey('note-due')), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }
}
