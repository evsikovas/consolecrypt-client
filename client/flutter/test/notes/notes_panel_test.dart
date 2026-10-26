import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/mock/mock_notes.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/inventory_service.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/note_service.dart';
import 'package:consolecrypt/notes/notes_panel.dart';
import 'package:consolecrypt/notes/notes_providers.dart';
import 'package:consolecrypt/notes/notes_reminders.dart';
import 'package:consolecrypt/notes/system_reminders.dart';
import 'package:flutter/services.dart';
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
  NoteService service, {
  NoteContext? current = _hostContext,
  _Inventory? inventory,
  String language = 'en',
  Size size = const Size(420, 800),
  double scale = 1,
  ReminderScheduler? scheduler,
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
        reminderSchedulerProvider.overrideWithValue(scheduler ?? _Scheduler()),
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

/// Records what the watcher hands to the system.
class _Scheduler implements ReminderScheduler {
  final replaced = <List<DateTime>>[];
  String? title, body;
  int cleared = 0;
  @override
  Future<void> replace(List<DateTime> at, {required String title, required String body}) async {
    replaced.add(at);
    this.title = title;
    this.body = body;
  }

  @override
  Future<void> clear() async => cleared++;
}

/// A vault that refuses every save (locked meanwhile, disk full…).
class _FailingNotes implements NoteService {
  @override
  Stream<void> changes() => Stream.value(null);
  @override
  Future<List<Note>> notes() async => const [];
  @override
  Future<Note> save(Note note) async => throw StateError('vault is locked');
  @override
  Future<void> delete(String id) async {}
}

String _status(WidgetTester tester) => tester.widget<Text>(find.byKey(const ValueKey('note-status'))).data!;

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

  testWidgets('future reminders go to the system with neutral text, past ones do not', (tester) async {
    final soon = DateTime.now().add(const Duration(hours: 1));
    final later = DateTime.now().add(const Duration(days: 2));
    final scheduler = _Scheduler();
    final service = MockNoteService(
      seed: [
        Note(id: 'b', title: 'Rotate the database password', remindAt: later),
        Note(id: 'a', title: 'Call the vendor', remindAt: soon),
        Note(id: 'c', title: 'Old', remindAt: DateTime.now().subtract(const Duration(minutes: 5))),
      ],
    );
    await _pump(tester, service, current: null, scheduler: scheduler);
    expect(scheduler.replaced.last, [soon.toUtc(), later.toUtc()]);
    expect(scheduler.title, 'ConsoleCrypt');
    expect(scheduler.body, _l(tester).notesSystemReminder);
    expect(scheduler.body, isNot(contains('vendor')), reason: 'the note text never reaches the system');
    // Unchanged reminders are not handed over again.
    final count = scheduler.replaced.length;
    await tester.pump(const Duration(seconds: 31));
    await settle(tester);
    expect(scheduler.replaced.length, count);
  });

  test('the system gets the nearest future reminders, each instant once', () {
    final now = DateTime.utc(2026, 10, 26, 12);
    final times = systemReminderTimes([
      now.add(const Duration(minutes: 2)),
      null,
      now.subtract(const Duration(minutes: 1)),
      now.add(const Duration(minutes: 1)),
      now.add(const Duration(minutes: 1)),
      for (var i = 0; i < 100; i++) now.add(Duration(days: 1, minutes: i)),
    ], now);
    expect(times.length, maxSystemReminders);
    expect(times.take(2), [now.add(const Duration(minutes: 1)), now.add(const Duration(minutes: 2))]);
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

  testWidgets('the editor has Save and a status; ⌘S and Ctrl+S save at once', (tester) async {
    final service = MockNoteService(seed: const []);
    await _pump(tester, service);
    final l = _l(tester);
    await tapKey(tester, 'notes-new');
    expect(isEnabled(tester, 'note-save'), isFalse, reason: 'nothing to save yet');
    await tester.enterText(find.byKey(const ValueKey('note-title')), 'Rotate keys');
    await tester.pump();
    expect(_status(tester), l.notesUnsaved);
    await tapKey(tester, 'note-save');
    expect((await service.notes()).single.title, 'Rotate keys');
    expect(_status(tester), l.notesSaved);
    expect(isEnabled(tester, 'note-save'), isFalse);

    await tester.enterText(find.byKey(const ValueKey('note-title')), 'Rotate keys today');
    await tester.pump();
    expect(_status(tester), l.notesUnsaved);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyS);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
    await settle(tester);
    expect((await service.notes()).single.title, 'Rotate keys today');
    expect(_status(tester), l.notesSaved);

    await tester.enterText(find.byKey(const ValueKey('note-title')), 'Rotate keys tonight');
    await tester.pump();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyS);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect((await service.notes()).single.title, 'Rotate keys tonight');
    expect(service.notes(), completion(hasLength(1)), reason: 'saves update one note');
    expect(tester.takeException(), isNull);
  });

  testWidgets('leaving with changes that could not be saved asks first', (tester) async {
    await _pump(tester, _FailingNotes());
    final l = _l(tester);
    await tapKey(tester, 'notes-new');
    await tester.enterText(find.byKey(const ValueKey('note-title')), 'Draft');
    await tapKey(tester, 'note-save');
    expect(_status(tester), l.notesSaveFailed);
    await tapKey(tester, 'note-back');
    expect(find.text(l.notesUnsavedTitle), findsOneWidget);
    await tester.tap(find.text(l.commonCancel).last);
    await settle(tester);
    expect(find.byKey(const ValueKey('note-title')), findsOneWidget, reason: 'stays in the editor');
    await tapKey(tester, 'note-back');
    await tester.tap(find.text(l.notesDiscardChanges).last);
    await settle(tester);
    expect(find.byKey(const ValueKey('note-title')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('an empty new note closes without asking', (tester) async {
    await _pump(tester, MockNoteService(seed: const []));
    await tapKey(tester, 'notes-new');
    await tapKey(tester, 'note-back');
    expect(find.text(_l(tester).notesUnsavedTitle), findsNothing);
    expect(
      find.byKey(const ValueKey('notes-list')).evaluate().isNotEmpty || find.byType(EmptyState).evaluate().isNotEmpty,
      isTrue,
    );
  });

  testWidgets('a host chosen for the Object tab replaces the open object until cleared', (tester) async {
    final other = Host(
      id: const ObjectId('h2'),
      name: 'web-1',
      address: '10.0.0.6',
      createdAt: DateTime(2026),
      updatedAt: DateTime(2026),
    );
    final service = MockNoteService(
      seed: const [Note(id: 'n2', title: 'Nginx reload only', target: NoteObjectTarget(NoteObjectKind.host, 'h2'))],
    );
    final container = await _pump(tester, service, current: null);
    final l = _l(tester);
    // Nothing open: All is shown, the Object tab offers to choose a host.
    expect(find.text('Nginx reload only'), findsOneWidget);
    await tapKey(tester, 'notes-mode-object');
    expect(find.text(l.notesNoObjectTitle), findsOneWidget);
    expect(find.byKey(const ValueKey('notes-empty-choose-host')), findsOneWidget);

    container.read(notesChosenObjectProvider.notifier).choose(hostNoteContext(other));
    await settle(tester);
    expect(tester.widget<Text>(find.byKey(const ValueKey('notes-context'))).data, 'web-1');
    expect(find.text('Nginx reload only'), findsOneWidget);
    // A new note from here is attached to the chosen host.
    await tapKey(tester, 'notes-new');
    await enterKey(tester, 'note-title', 'Check TLS');
    await tapKey(tester, 'note-save');
    await tapKey(tester, 'note-back');
    final created = (await service.notes()).firstWhere((n) => n.title == 'Check TLS');
    expect(created.target, const NoteObjectTarget(NoteObjectKind.host, 'h2'));

    await tapKey(tester, 'notes-object-clear');
    expect(container.read(notesChosenObjectProvider), isNull);
    expect(find.text(l.notesNoObjectTitle), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('choosing a host opens the Object tab even from All', (tester) async {
    final container = await _pump(tester, MockNoteService(seed: const []));
    await tapKey(tester, 'notes-mode-all');
    container.read(notesChosenObjectProvider.notifier).choose(hostNoteContext(_host));
    await settle(tester);
    expect(find.byKey(const ValueKey('notes-context')), findsOneWidget);
    expect(find.byKey(const ValueKey('notes-host-card')), findsOneWidget);
  });

  testWidgets('a note whose object is gone stays, marked not linked, and can be attached again', (tester) async {
    final service = MockNoteService(
      seed: [
        const Note(
          id: 'gone',
          title: 'Vacuum weekly',
          target: NoteObjectTarget(NoteObjectKind.host, 'deleted-host'),
          targetLabel: 'old-db',
        ),
        Note(id: 'later', title: 'Tunnel to billing', target: NoteUnknownTarget({'kind': 'tunnel', 'id': 't'})),
        const Note(id: 'here', title: 'Restart window', target: NoteObjectTarget(NoteObjectKind.host, 'h1')),
        const Note(id: 'free', title: 'Buy coffee'),
      ],
    );
    await _pump(tester, service, current: null);
    final l = _l(tester);
    expect(find.text('Vacuum weekly'), findsOneWidget, reason: 'never deleted with its object');
    expect(find.byKey(const ValueKey('note-not-linked-gone')), findsOneWidget);
    expect(find.text('old-db'), findsOneWidget, reason: 'the name it had');
    expect(find.byKey(const ValueKey('note-not-linked-later')), findsOneWidget, reason: 'a later version link');
    expect(find.byKey(const ValueKey('note-not-linked-here')), findsNothing);
    expect(find.byKey(const ValueKey('note-not-linked-free')), findsNothing, reason: 'never attached');

    await tapKey(tester, 'notes-filter');
    await tester.tap(find.text(l.notesFilterNotLinked(2)).last);
    await settle(tester);
    expect(find.text('Vacuum weekly'), findsOneWidget);
    expect(find.text('Tunnel to billing'), findsOneWidget);
    expect(find.text('Restart window'), findsNothing);
    expect(find.text('Buy coffee'), findsNothing);

    await tapKey(tester, 'note-menu-gone');
    await tapKey(tester, 'note-rebind-gone');
    await pickHost(tester, 'db-prod');
    final rebound = (await service.notes()).firstWhere((n) => n.id == 'gone');
    expect(rebound.target, const NoteObjectTarget(NoteObjectKind.host, 'h1'));
    expect(rebound.targetLabel, 'db-prod');
    expect(find.byKey(const ValueKey('note-not-linked-gone')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('editing a note of a later version keeps its link', (tester) async {
    final later = NoteUnknownTarget({'kind': 'tunnel', 'id': 't'});
    final service = MockNoteService(
      seed: [Note(id: 'later', title: 'Tunnel to billing', target: later, targetLabel: 'billing')],
    );
    await _pump(tester, service, current: null);
    final l = _l(tester);
    await tester.tap(find.text('Tunnel to billing'));
    await settle(tester);
    expect(find.text(l.notesNotLinkedWas('billing')), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('note-title')), 'Tunnel to billing (prod)');
    await tapKey(tester, 'note-save');
    final saved = (await service.notes()).single;
    expect(saved.target, later);
    expect(saved.targetLabel, 'billing');
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
