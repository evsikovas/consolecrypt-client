import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/note_service.dart';
import 'package:consolecrypt/src/rust/api/inventory.dart' as rs_inv;
import 'package:consolecrypt/src/rust/api/sharing.dart' as rs_share;

/// Notes through the native vault (`notes_*` bridge calls), and notes
/// travelling with shares (`share_note*`).
final class RustNoteService implements NoteService, SharedNoteService {
  RustNoteService(this._hub);
  final RustBackend _hub;
  final _local = StreamController<void>.broadcast();

  @override
  Stream<void> changes() => Stream<void>.multi((controller) {
    void changed(Json event) {
      final type = event['type'];
      if ((type == 'objects_changed' && event['kind'] == 'note') ||
          const {'conflict_resolved', 'lagged', 'vault_unlocked', 'vault_locked', 'profiles_changed'}.contains(type)) {
        controller.add(null);
      }
    }

    _hub.eventListeners.add(changed);
    final local = _local.stream.listen((_) => controller.add(null));
    controller.add(null);
    controller.onCancel = () {
      _hub.eventListeners.remove(changed);
      unawaited(local.cancel());
    };
  }, isBroadcast: true);

  @override
  Future<List<Note>> notes() async {
    final list = jsonDecode(await guard(rs_inv.notesList)) as List;
    return List.unmodifiable(list.cast<NoteJson>().map(Note.fromJson));
  }

  @override
  Future<Note> save(Note note) async {
    final saved = Note.fromJson(
      jsonDecode(await guard(() => rs_inv.notesSave(noteJson: jsonEncode(note.toJson())))) as NoteJson,
    );
    _local.add(null);
    return saved;
  }

  @override
  Future<void> delete(String id) async {
    await guard(() => rs_inv.notesDelete(id: id));
    _local.add(null);
  }

  @override
  Future<Map<String, String>> shareLinks() async {
    final list = jsonDecode(await guard(rs_share.shareNoteLinks)) as List;
    return {for (final link in list.cast<Json>()) link['object']! as String: link['share_id']! as String};
  }

  @override
  Future<void> linkShare(String object, String shareId) async {
    await guard(() => rs_share.shareNoteLink(object: object, shareId: shareId));
    _local.add(null);
  }

  @override
  Future<SharedNotes> sharedNotes(String shareId) async =>
      SharedNotes.fromJson(jsonDecode(await guard(() => rs_share.shareNotes(shareId: shareId))) as NoteJson);

  @override
  Future<SharedNote> saveShared(
    String shareId, {
    required String title,
    String id = '',
    String body = '',
    List<String> tags = const [],
    Map<String, dynamic>? scope,
  }) async {
    final json = jsonEncode({'id': id, 'title': title, 'body': body, 'tags': tags, 'scope': scope});
    return SharedNote.fromJson(
      jsonDecode(await guard(() => rs_share.shareNoteSave(shareId: shareId, noteJson: json))) as NoteJson,
    );
  }

  @override
  Future<void> deleteShared(String shareId, String noteId) =>
      guard(() => rs_share.shareNoteDelete(shareId: shareId, noteId: noteId));
}
