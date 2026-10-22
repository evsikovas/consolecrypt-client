import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/notes_panel.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('notes round-trip every target kind with the core wire names', () {
    for (final target in const <NoteTarget>[
      NoteObjectTarget(NoteObjectKind.dbConnection, 'c1'),
      NoteKubeTarget(
        contextId: 'k',
        group: 'apps',
        resource: 'deployments',
        kind: 'Deployment',
        namespace: 'prod',
        name: 'api',
      ),
      NoteDockerTarget(name: 'postgres'),
      NoteDockerTarget(hostId: 'h1', name: 'redis'),
    ]) {
      final note = Note(
        id: 'n',
        title: 'T',
        body: 'b',
        tags: const ['ops'],
        pinned: true,
        task: NoteTask(dueAt: DateTime.fromMillisecondsSinceEpoch(1000)),
        remindAt: DateTime.fromMillisecondsSinceEpoch(2000),
        target: target,
      );
      final back = Note.fromJson(note.toJson());
      expect(back.target, target);
      expect(back.task!.dueAt, note.task!.dueAt);
      expect(back.remindAt, note.remindAt);
      expect(back.pinned, isTrue);
    }
    expect(const NoteObjectTarget(NoteObjectKind.rdpHost, 'x').toJson()['object_kind'], 'rdp_host');
  });

  test('notes written by older cores decode; targets of later versions are kept verbatim', () {
    final note = Note.fromJson({
      'id': 'n',
      'title': 't',
      'body': 'b',
      'tags': <String>[],
      'created_at_ms': 1,
      'updated_at_ms': 2,
    });
    expect(note.task, isNull);
    expect(note.target, isNull);
    expect(note.targetLabel, isNull);
    for (final later in [
      {'kind': 'tunnel', 'id': 'x', 'port': 5432},
      {'kind': 'object', 'object_kind': 'sqlite_file', 'id': 'y'},
    ]) {
      final decoded = Note.fromJson({'id': 'n', 'title': 't', 'target': later, 'target_label': 'billing'});
      expect(decoded.target, isA<NoteUnknownTarget>());
      expect(decoded.targetLabel, 'billing');
      // An edit here writes the link back unchanged.
      final edited = decoded.copyWith(title: 'edited').toJson();
      expect(edited['target'], later);
      expect(edited['target_label'], 'billing');
      expect(NoteTarget.fromJson(edited['target']), decoded.target, reason: 'structural equality');
    }
    expect(Note.fromJson({'id': 'n', 'title': 't', 'target': 'host'}).target, isNull, reason: 'not a target');
    // A detached note forgets its old object's name.
    expect(note.copyWith(targetLabel: () => 'old').toJson()['target_label'], isNull);
  });

  test('due states, ordering and typed times', () {
    final now = DateTime(2026, 10, 22, 12);
    final late = Note(
      title: 'late',
      task: NoteTask(dueAt: now.subtract(const Duration(minutes: 1))),
    );
    final doneLate = Note(
      title: 'done',
      task: NoteTask(done: true, dueAt: now.subtract(const Duration(days: 1))),
    );
    final reminder = Note(title: 'r', remindAt: now);
    expect(late.overdue(now), isTrue);
    expect(doneLate.overdue(now), isFalse);
    expect(reminder.reminderDue(now), isTrue);
    expect(Note(title: 'future', remindAt: now.add(const Duration(seconds: 1))).reminderDue(now), isFalse);

    final soon = Note(
      title: 'soon',
      task: NoteTask(dueAt: now.add(const Duration(hours: 1))),
    );
    const open = Note(title: 'open', task: NoteTask());
    expect(([open, soon, late]..sort(compareTasks)).map((n) => n.title), ['late', 'soon', 'open']);
    final pinned = Note(title: 'pinned', pinned: true, updatedAt: DateTime(2000));
    final recent = Note(title: 'recent', updatedAt: DateTime(2026));
    expect(([recent, pinned]..sort(compareNotes)).first.title, 'pinned');

    expect(parseNoteTime('2026-10-23 09:30'), DateTime(2026, 10, 23, 9, 30));
    expect(parseNoteTime('2026-02-30 09:30'), isNull);
    expect(parseNoteTime('23.10.2026'), isNull);
    expect(parseNoteTime('2026-10-23 24:00'), isNull);
  });
}
