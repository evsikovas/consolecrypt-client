import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/db/db_connections_screen.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_workspace.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/notes_providers.dart';
import 'package:consolecrypt/sharing/sharing_database.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:consolecrypt/sharing/sharing_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Databases section: the saved connections, or the workspace of the one
/// being opened. The workspace owns its session and closes it on exit.
class DatabasesScreen extends ConsumerStatefulWidget {
  const DatabasesScreen({super.key});
  @override
  ConsumerState<DatabasesScreen> createState() => _DatabasesScreenState();
}

class _DatabasesScreenState extends ConsumerState<DatabasesScreen> with NoteContextPublisher {
  @override
  ShellBranch get noteBranch => ShellBranch.databases;

  DbConnection? _open;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _openRequested(ref.read(dbOpenRequestProvider)));
  }

  /// Opens a connection another section asked for (Kubernetes "connect").
  Future<void> _openRequested(String? id) async {
    if (id == null || !mounted) return;
    ref.read(dbOpenRequestProvider.notifier).clear();
    final service = ref.read(dbServiceProvider);
    final connection = (await service?.connections())?.where((c) => c.id == id).firstOrNull;
    if (connection != null && mounted) setState(() => _open = connection);
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(dbOpenRequestProvider, (_, id) => _openRequested(id));
    final open = _open;
    publishNoteContext(
      open == null
          ? null
          : NoteContext(
              target: NoteObjectTarget(NoteObjectKind.dbConnection, open.id),
              label: open.name,
              icon: Icons.storage_rounded,
            ),
    );
    if (open == null) {
      // Offered only when the server advertises database connection sharing;
      // older servers keep both the action and the section hidden.
      final status = ref.watch(sharingStatusProvider).value;
      final sharing = status != null && status.enabled && !status.locked && status.supportsDbConnections;
      final shared = sharing
          ? (ref.watch(sharingItemsProvider).value ?? const <SharingItem>[])
                .where((item) => item.kind == SharingKind.dbConnection && item.trust != SharingTrust.deleted)
                .toList(growable: false)
          : const <SharingItem>[];
      return DbConnectionsScreen(
        onOpen: (connection) => setState(() => _open = connection),
        onShare: sharing ? (connection) => showSharingDbConnection(context, connectionId: connection.id) : null,
        shared: shared,
        onOpenShared: (item) => showSharingItemDetails(context, ref, item),
      );
    }
    return DbWorkspace(key: ValueKey(open.id), connection: open, onClose: () => setState(() => _open = null));
  }
}
