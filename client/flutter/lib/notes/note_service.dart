import 'package:consolecrypt/notes/note_models.dart';

/// Notes, tasks and reminders: vault objects, encrypted on this device and
/// synced end-to-end encrypted. Every call is scoped to the unlocked profile.
abstract interface class NoteService {
  /// Fires when notes may have changed (this device, sync, lock or unlock).
  Stream<void> changes();

  Future<List<Note>> notes();

  /// Creates (empty id) or updates; returns the stored note.
  Future<Note> save(Note note);

  Future<void> delete(String id);
}

/// Notes travelling with shares, for members of the same sync server. The
/// core verifies the owner's key and every author's signature.
abstract interface class SharedNoteService {
  /// Local objects (`host:<id>`, `db_connection:<id>`…) linked to shares.
  Future<Map<String, String>> shareLinks();

  Future<void> linkShare(String object, String shareId);

  Future<SharedNotes> sharedNotes(String shareId);

  /// Creates (empty [id]) or updates this user's note.
  Future<SharedNote> saveShared(
    String shareId, {
    required String title,
    String id = '',
    String body = '',
    List<String> tags = const [],
    Map<String, dynamic>? scope,
  });

  Future<void> deleteShared(String shareId, String noteId);
}
