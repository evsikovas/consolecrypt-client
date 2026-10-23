import 'dart:async';

import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/note_service.dart';

/// In-memory notes for the demo backend and widget tests.
final class MockNoteService implements NoteService, SharedNoteService {
  MockNoteService({List<Note>? seed, DateTime Function()? clock}) : _clock = clock ?? DateTime.now {
    for (final note in seed ?? _demo(_clock())) {
      _notes[note.id] = note;
    }
  }

  final DateTime Function() _clock;
  final _notes = <String, Note>{};

  /// Links and shared notes, by share id (no encryption in the mock).
  final sharedLinks = <String, String>{};
  final shared = <String, List<SharedNote>>{};
  bool sharedEnabled = true, waitingForOwner = false;
  final _changes = StreamController<void>.broadcast();
  var _next = 0;

  @override
  Stream<void> changes() => Stream<void>.multi((controller) {
    controller.add(null);
    final sub = _changes.stream.listen((_) => controller.add(null));
    controller.onCancel = sub.cancel;
  }, isBroadcast: true);

  @override
  Future<List<Note>> notes() async => List.unmodifiable(_notes.values);

  @override
  Future<Note> save(Note note) async {
    if (note.title.trim().isEmpty) throw ArgumentError.value(note.title, 'title');
    final now = _clock();
    final existing = _notes[note.id];
    final id = existing == null ? 'note-${++_next}' : note.id;
    final task = note.task;
    final saved = Note(
      id: id,
      title: note.title.trim(),
      body: note.body,
      tags: [
        for (final tag in note.tags)
          if (tag.trim().isNotEmpty) tag.trim(),
      ],
      pinned: note.pinned,
      task: task == null
          ? null
          : NoteTask(done: task.done, dueAt: task.dueAt, doneAt: task.done ? (existing?.task?.doneAt ?? now) : null),
      remindAt: note.remindAt,
      target: note.target,
      targetLabel: note.target == null ? null : note.targetLabel,
      createdAt: existing?.createdAt ?? now,
      updatedAt: now,
    );
    _notes[id] = saved;
    _changes.add(null);
    return saved;
  }

  @override
  Future<void> delete(String id) async {
    _notes.remove(id);
    _changes.add(null);
  }

  static List<Note> _demo(DateTime now) => [
    Note(
      id: 'demo-note-1',
      title: 'Renew TLS certificates',
      body: 'Staging first, then **production**.',
      tags: const ['ops'],
      task: NoteTask(dueAt: now.add(const Duration(days: 2))),
      createdAt: now,
      updatedAt: now,
    ),
    Note(
      id: 'demo-note-2',
      title: 'Backup window',
      body: 'Nightly dumps run 02:00–03:00 UTC; avoid restarts.',
      pinned: true,
      createdAt: now,
      updatedAt: now,
    ),
  ];

  @override
  Future<Map<String, String>> shareLinks() async => Map.unmodifiable(sharedLinks);

  @override
  Future<void> linkShare(String object, String shareId) async {
    sharedLinks[object] = shareId;
    _changes.add(null);
  }

  @override
  Future<SharedNotes> sharedNotes(String shareId) async => SharedNotes(
    shareId: shareId,
    enabled: sharedEnabled,
    owner: true,
    ownerUserId: 'me',
    waitingForOwner: waitingForOwner,
    notes: List.unmodifiable(shared[shareId] ?? const []),
  );

  @override
  Future<SharedNote> saveShared(
    String shareId, {
    required String title,
    String id = '',
    String body = '',
    List<String> tags = const [],
    Map<String, dynamic>? scope,
  }) async {
    final list = shared.putIfAbsent(shareId, () => []);
    final existing = list.where((n) => n.id == id).firstOrNull;
    final now = _clock();
    final note = SharedNote(
      id: existing?.id ?? 'shared-${++_next}',
      title: title,
      body: body,
      tags: tags,
      scope: scope,
      authorUserId: 'me',
      mine: true,
      canDelete: true,
      revision: (existing?.revision ?? 0) + 1,
      createdAt: existing?.createdAt ?? now,
      updatedAt: now,
    );
    list
      ..removeWhere((n) => n.id == note.id)
      ..add(note);
    return note;
  }

  @override
  Future<void> deleteShared(String shareId, String noteId) async => shared[shareId]?.removeWhere((n) => n.id == noteId);
}
