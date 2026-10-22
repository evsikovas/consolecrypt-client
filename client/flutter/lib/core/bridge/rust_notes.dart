import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/note_service.dart';
import 'package:consolecrypt/src/rust/api/inventory.dart' as rs_inv;

/// Notes through the native vault (`notes_*` bridge calls).
final class RustNoteService implements NoteService {
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
}
