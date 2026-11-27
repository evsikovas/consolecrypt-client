import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

typedef EditorSftpFile = ({Host host, String path});

/// Requests may arrive before the editor branch mounts or while it is restoring a draft.
final editorSftpRequestsProvider = NotifierProvider<EditorSftpRequests, List<EditorSftpFile>>(EditorSftpRequests.new);

class EditorSftpRequests extends Notifier<List<EditorSftpFile>> {
  @override
  List<EditorSftpFile> build() {
    ref.watch(activeProfileProvider.select((p) => p?.id));
    ref.watch(appStageProvider);
    return const [];
  }

  void open(Host host, String path) => state = [...state, (host: host, path: path)];

  EditorSftpFile takeFirst() {
    final first = state.first;
    state = state.sublist(1);
    return first;
  }
}
