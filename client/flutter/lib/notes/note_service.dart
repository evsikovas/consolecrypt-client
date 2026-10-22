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
