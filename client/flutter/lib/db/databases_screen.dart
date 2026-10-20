import 'package:consolecrypt/db/db_connections_screen.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_workspace.dart';
import 'package:material_ui/material_ui.dart';

/// Databases section: the saved connections, or the workspace of the one
/// being opened. The workspace owns its session and closes it on exit.
class DatabasesScreen extends StatefulWidget {
  const DatabasesScreen({super.key});
  @override
  State<DatabasesScreen> createState() => _DatabasesScreenState();
}

class _DatabasesScreenState extends State<DatabasesScreen> {
  DbConnection? _open;

  @override
  Widget build(BuildContext context) {
    final open = _open;
    if (open == null) return DbConnectionsScreen(onOpen: (connection) => setState(() => _open = connection));
    return DbWorkspace(key: ValueKey(open.id), connection: open, onClose: () => setState(() => _open = null));
  }
}
