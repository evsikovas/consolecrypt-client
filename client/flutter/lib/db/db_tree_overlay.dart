import 'package:consolecrypt/db/db_models.dart';
import 'package:material_ui/material_ui.dart';

/// How a draft object differs from the server.
enum DbDraftState {
  /// Exists only in an open designer (a new table or column).
  added,

  /// Exists on the server and is changed in an open designer.
  changed,
}

/// A structure node that exists only in an open designer.
final class DbDraftNode {
  const DbDraftNode({required this.node, required this.state});
  final DbTreeNode node;
  final DbDraftState state;
}

/// Unsaved designer drafts shown in the structure tree: new tables and
/// columns appear under their parent, changed ones are marked. The tree
/// asks this overlay while building its rows and rebuilds when it notifies.
abstract interface class DbTreeOverlay implements Listenable {
  /// Draft children of [parent] (null: the top level), after the loaded ones.
  List<DbDraftNode> childrenOf(DbObjectRef? parent);

  /// The draft state of a loaded object, or null when it is unchanged.
  DbDraftState? stateOf(DbObjectRef object);

  /// Whether [object] exists only as a draft (never loaded from the server).
  bool isDraft(DbObjectRef object);

  /// Brings the designer that owns [object] to the front.
  void focus(DbObjectRef object);

  /// Parents whose children changed on the server (a saved design); the
  /// tree reloads them once.
  List<DbObjectRef> takeReloads();

  /// Objects to expand so a new draft is visible; taken once.
  List<DbObjectRef> takeReveals();
}
