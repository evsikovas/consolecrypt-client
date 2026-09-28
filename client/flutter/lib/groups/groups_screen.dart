import 'package:consolecrypt/hosts/hosts_screen.dart';
import 'package:material_ui/material_ui.dart';

export 'group_tree.dart' show GroupNode, flattenGroupTree;

/// Groups and hosts share the same inventory browser and actions.
class GroupsScreen extends StatelessWidget {
  const GroupsScreen({super.key});

  @override
  Widget build(BuildContext context) => const HostsScreen(initialGroups: true);
}
