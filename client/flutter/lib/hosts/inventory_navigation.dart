import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/groups/group_tree.dart';
import 'package:material_ui/material_ui.dart';

const inventoryAll = 'all';
const inventoryGroups = 'groups';
const inventoryUngrouped = 'ungrouped';

/// Teams: personal-vault resources only.
const inventoryPersonal = 'personal';

/// Teams: one team's resources (`team:<vault id>`).
const inventoryTeamPrefix = 'team:';

/// A team section of the navigation (colour, name, host count).
final class InventoryTeamEntry {
  const InventoryTeamEntry({required this.vaultId, required this.name, required this.color, required this.count});
  final String vaultId;
  final String name;
  final Color? color;
  final int count;
}

/// The same locations appear in the desktop tree and the compact selector.
class InventoryNavigation extends StatelessWidget {
  const InventoryNavigation({
    super.key,
    required this.groups,
    required this.hosts,
    required this.location,
    required this.onSelect,
    required this.collapsed,
    required this.onToggle,
    this.compact = false,
    this.teams = const [],
    this.personalCount = 0,
  });

  final Map<ObjectId, Group> groups;
  final List<Host> hosts;
  final String location;
  final ValueChanged<String> onSelect;
  final Set<ObjectId> collapsed;
  final ValueChanged<ObjectId> onToggle;
  final bool compact;

  /// Team sections (empty: the profile has no teams).
  final List<InventoryTeamEntry> teams;
  final int personalCount;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final counts = groupHostCounts(groups, hosts);
    final nodes = flattenGroupTree(groups.values.toList());
    final base = [
      (value: inventoryAll, label: l.inventoryAllHosts, icon: Icons.dns_rounded, count: hosts.length),
      (value: inventoryGroups, label: l.groupsTitle, icon: Icons.folder_copy_rounded, count: groups.length),
      (
        value: inventoryUngrouped,
        label: l.inventoryUngrouped,
        icon: Icons.folder_off_rounded,
        count: hosts.where((h) => !groups.containsKey(h.groupId)).length,
      ),
    ];
    final teamItems = teams.isEmpty
        ? const <({String value, String label, IconData icon, int count, Color? color})>[]
        : [
            (
              value: inventoryPersonal,
              label: l.teamsFilterPersonal,
              icon: Icons.person_rounded,
              count: personalCount,
              color: null,
            ),
            for (final t in teams)
              (
                value: '$inventoryTeamPrefix${t.vaultId}',
                label: t.name.isEmpty ? l.teamsUnnamed : t.name,
                icon: Icons.groups_rounded,
                count: t.count,
                color: t.color,
              ),
          ];
    if (compact) {
      return GlassSelect<String>(
        key: const ValueKey('inventory-location'),
        value: location,
        expand: true,
        semanticLabel: l.inventoryLocation,
        items: [
          for (final item in base)
            GlassSelectItem(
              value: item.value,
              label: '${item.label} (${item.count})',
              icon: item.icon,
              key: ValueKey('inventory-location-${item.value}'),
            ),
          for (final item in teamItems)
            GlassSelectItem(
              value: item.value,
              label: '${item.label} (${item.count})',
              icon: item.icon,
              key: ValueKey('inventory-location-${item.value}'),
            ),
          for (final node in nodes)
            GlassSelectItem(
              value: node.group.id.value,
              label: groupPathName(groups, node.group.id),
              icon: Icons.folder_rounded,
              key: ValueKey('inventory-location-${node.group.name}'),
            ),
        ],
        onChanged: onSelect,
      );
    }
    bool hidden(Group group) {
      var cursor = group.parentId;
      final seen = <ObjectId>{group.id};
      while (cursor != null && seen.add(cursor)) {
        if (collapsed.contains(cursor)) return true;
        cursor = groups[cursor]?.parentId;
      }
      return false;
    }

    return ContentSurface(
      padding: const EdgeInsets.all(8),
      child: ListView(
        children: [
          for (final item in base)
            _LocationRow(
              key: ValueKey('inventory-nav-${item.value}'),
              label: item.label,
              icon: item.icon,
              count: item.count,
              selected: location == item.value,
              onTap: () => onSelect(item.value),
            ),
          if (teamItems.isNotEmpty) ...[
            const Padding(padding: EdgeInsets.symmetric(vertical: 12), child: Divider(height: 1)),
            Padding(
              padding: const EdgeInsets.fromLTRB(10, 0, 10, 8),
              child: Text(l.teamsTitle, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
            ),
            for (final item in teamItems)
              _LocationRow(
                key: ValueKey('inventory-nav-${item.value}'),
                label: item.label,
                icon: item.icon,
                count: item.count,
                selected: location == item.value,
                stripe: item.color,
                onTap: () => onSelect(item.value),
              ),
          ],
          const Padding(padding: EdgeInsets.symmetric(vertical: 12), child: Divider(height: 1)),
          Padding(
            padding: const EdgeInsets.fromLTRB(10, 0, 10, 8),
            child: Text(l.inventoryFolders, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
          ),
          if (nodes.isEmpty)
            Padding(
              padding: const EdgeInsets.all(10),
              child: Text(l.groupsEmpty, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
            ),
          for (final node in nodes)
            if (!hidden(node.group))
              Padding(
                padding: EdgeInsets.only(left: (node.depth * 12.0).clamp(0, 48)),
                child: _LocationRow(
                  key: ValueKey('group-${node.group.name}'),
                  label: node.group.name,
                  icon: location == node.group.id.value ? Icons.folder_open_rounded : Icons.folder_rounded,
                  selected: location == node.group.id.value,
                  count: counts[node.group.id] ?? 0,
                  onTap: () => onSelect(node.group.id.value),
                  toggle: groups.values.any((g) => g.parentId == node.group.id)
                      ? GlassIconButton(
                          key: ValueKey('inventory-expand-${node.group.name}'),
                          tooltip: collapsed.contains(node.group.id) ? l.inventoryExpand : l.inventoryCollapse,
                          icon: collapsed.contains(node.group.id)
                              ? Icons.chevron_right_rounded
                              : Icons.expand_more_rounded,
                          style: GlassIconButtonStyle.plain,
                          iconSize: 16,
                          onPressed: () => onToggle(node.group.id),
                        )
                      : null,
                ),
              ),
        ],
      ),
    );
  }
}

class _LocationRow extends StatelessWidget {
  const _LocationRow({
    super.key,
    required this.label,
    required this.icon,
    required this.count,
    required this.selected,
    required this.onTap,
    this.toggle,
    this.stripe,
  });
  final String label;
  final IconData icon;
  final int count;
  final bool selected;
  final VoidCallback onTap;
  final Widget? toggle;

  /// Team colour bar on the leading edge.
  final Color? stripe;

  @override
  Widget build(BuildContext context) {
    final t = GlassTokens.of(context);
    final fg = selected ? t.palette.accent : t.palette.label;
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: GlassInteractive(
        selected: selected,
        semanticLabel: '$label, $count',
        onPressed: onTap,
        builder: (context, state) => GlassFocusRing(
          visible: state.focusVisible,
          shape: GlassRadii.shape(10),
          child: Container(
            constraints: const BoxConstraints(minHeight: 42),
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(10),
              color: selected
                  ? t.palette.accent.withValues(alpha: .14)
                  : state.hovered
                  ? t.surfaces.fillHover
                  : null,
            ),
            child: Row(
              children: [
                if (stripe != null) ...[
                  Container(
                    width: 4,
                    height: 20,
                    decoration: BoxDecoration(color: stripe, borderRadius: BorderRadius.circular(2)),
                  ),
                  const SizedBox(width: 6),
                ],
                Icon(icon, size: 18, color: selected ? fg : stripe ?? t.secondaryLabel),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: t.typography.body.copyWith(color: fg),
                  ),
                ),
                const SizedBox(width: 6),
                Text('$count', style: t.typography.caption.copyWith(color: t.secondaryLabel)),
                ?toggle,
              ],
            ),
          ),
        ),
      ),
    );
  }
}
