import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/symbols.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/view_mode_toggle.dart';
import 'package:consolecrypt/core/widgets/workspace_sections.dart';
import 'package:consolecrypt/groups/group_dialogs.dart';
import 'package:consolecrypt/groups/group_tree.dart';
import 'package:consolecrypt/hosts/inventory_navigation.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/notes_providers.dart';
import 'package:consolecrypt/rdp/rdp_launcher.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sharing/sharing_collections.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_management.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:consolecrypt/sharing/sharing_rdp.dart';
import 'package:consolecrypt/teams/team_models.dart';
import 'package:consolecrypt/teams/team_providers.dart';
import 'package:consolecrypt/teams/teams_settings.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

export 'package:consolecrypt/groups/group_tree.dart' show groupPathName;

/// Pure filter used by the hosts list (unit-tested): free text over name,
/// address, username, tags and group path; all selected tags must match.
List<Host> filterHosts(
  List<Host> hosts, {
  required String query,
  required Set<String> tags,
  Map<ObjectId, Group> groups = const {},
}) {
  final q = query.trim().toLowerCase();
  final result = hosts.where((h) {
    if (!tags.every(h.tags.contains)) return false;
    if (q.isEmpty) return true;
    final haystack = [
      h.name,
      h.address,
      h.username ?? '',
      ...h.tags,
      groupPathName(groups, h.groupId),
    ].join(' ').toLowerCase();
    return q.split(RegExp(r'\s+')).every(haystack.contains);
  }).toList();
  result.sort((a, b) {
    final ga = groupPathName(groups, a.groupId);
    final gb = groupPathName(groups, b.groupId);
    final c = ga.compareTo(gb);
    return c != 0 ? c : a.name.toLowerCase().compareTo(b.name.toLowerCase());
  });
  return result;
}

Future<void> connectToHost(BuildContext context, WidgetRef ref, Host host) async {
  if (host.isRdp) {
    final tab = await RdpLauncher.openSavedHost(context, ref, host);
    if (tab != null && context.mounted) context.go(AppRoutes.rdp);
    return;
  }
  final tab = await runWithFeedback(context, () => ref.read(terminalTabsProvider.notifier).open(host));
  if (tab != null && context.mounted) context.go(AppRoutes.terminal);
}

class HostsScreen extends ConsumerWidget {
  const HostsScreen({super.key, this.initialGroups = false});
  final bool initialGroups;

  @override
  Widget build(BuildContext context, WidgetRef ref) =>
      _InventoryBrowser(key: ValueKey(ref.watch(activeProfileProvider)?.id), initialGroups: initialGroups);
}

class _InventoryBrowser extends ConsumerStatefulWidget {
  const _InventoryBrowser({super.key, required this.initialGroups});
  final bool initialGroups;
  @override
  ConsumerState<_InventoryBrowser> createState() => _InventoryBrowserState();
}

class _InventoryBrowserState extends ConsumerState<_InventoryBrowser> {
  final _search = TextEditingController();
  final _scroll = ScrollController();
  final Set<String> _tags = {};
  final Set<ObjectId> _collapsed = {};
  late String _location = widget.initialGroups ? inventoryGroups : inventoryAll;
  late bool _cards = AppPlatform.isMobile;
  ObjectId? _inspectedHost;
  bool _descending = false;

  @override
  void dispose() {
    _search.dispose();
    _scroll.dispose();
    super.dispose();
  }

  void _browse(String location) {
    setState(() {
      _location = location;
      _inspectedHost = null;
      _search.clear();
      _tags.clear();
    });
    if (_scroll.hasClients) _scroll.jumpTo(0);
  }

  void _addHost(Group? group) => context.go(
    group == null ? AppRoutes.newHost : '${AppRoutes.newHost}?group=${Uri.encodeQueryComponent(group.id.value)}',
  );

  Future<Group?> _addTeamGroup(Team team) async {
    final name = await showTextInputDialog(
      context,
      title: context.l10n.teamsNewGroup,
      label: context.l10n.commonName,
      confirmLabel: context.l10n.commonCreate,
    );
    if (name == null || name.trim().isEmpty || !mounted) return null;
    final id = await runWithFeedback(
      context,
      () => ref.read(teamServiceProvider).createGroup(team.vaultId, name.trim()),
    );
    if (id == null || !mounted) return null;
    return ref.read(groupByIdProvider)[ObjectId(id)];
  }

  /// A new team host goes into a team group (hosts follow their group).
  Future<void> _addTeamHost(Team team, Map<ObjectId, Group> groups, String? Function(ObjectId) vaultOf) async {
    final roots = groups.values.where((g) => vaultOf(g.id) == team.vaultId && !groups.containsKey(g.parentId)).toList()
      ..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
    final target = roots.firstOrNull ?? await _addTeamGroup(team);
    if (target != null && mounted) _addHost(target);
  }

  Future<void> _delete(Host host) async {
    final l10n = context.l10n;
    final ok = await showConfirmDialog(
      context,
      title: l10n.hostsDeleteTitle(host.name),
      message: l10n.hostsDeleteMessage,
      confirmLabel: l10n.commonDelete,
      destructive: true,
    );
    if (ok && mounted) {
      await runWithFeedback(
        context,
        () => ref.read(inventoryServiceProvider).deleteHost(host.id),
        success: l10n.hostsDeleted,
      );
    }
  }

  Future<void> _deleteGroup(Group group) async {
    final l = context.l10n;
    final ok = await showConfirmDialog(
      context,
      title: l.groupsDeleteTitle(group.name),
      message: l.groupsDeleteMessage,
      confirmLabel: l.commonDelete,
      destructive: true,
    );
    if (ok && mounted) {
      await runWithFeedback(context, () => ref.read(inventoryServiceProvider).deleteGroup(group.id));
      if (mounted && !ref.read(groupByIdProvider).containsKey(group.id)) {
        _browse(group.parentId?.value ?? inventoryGroups);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final hostsAsync = ref.watch(hostsProvider);
    final groupsAsync = ref.watch(groupsProvider);
    final groups = ref.watch(groupByIdProvider);
    final allHosts = hostsAsync.value ?? const <Host>[];
    final counts = groupHostCounts(groups, allHosts);
    final teams = ref.watch(teamsProvider).value?.where((t) => t.open).toList() ?? const <Team>[];
    final origins = ref.watch(objectTeamsProvider).value ?? const <String, String>{};
    String? vaultOf(ObjectId id) => origins[id.value];
    final teamEntries = [
      for (final t in teams)
        InventoryTeamEntry(
          vaultId: t.vaultId,
          name: t.name,
          color: teamColor(t.color),
          count: allHosts.where((h) => vaultOf(h.id) == t.vaultId).length,
        ),
    ];
    final teamLocation = _location.startsWith(inventoryTeamPrefix)
        ? teams.where((t) => '$inventoryTeamPrefix${t.vaultId}' == _location).firstOrNull
        : null;
    final group = groups.values.where((g) => g.id.value == _location).firstOrNull;
    final location =
        group != null ||
            teamLocation != null ||
            (_location == inventoryPersonal && teams.isNotEmpty) ||
            [inventoryAll, inventoryGroups, inventoryUngrouped].contains(_location)
        ? _location
        : inventoryAll;
    // Vault filter of a team section (`null` = no filter, '' = personal).
    final vaultFilter = location == inventoryPersonal ? '' : teamLocation?.vaultId;
    bool inVault(ObjectId id) => vaultFilter == null || (vaultOf(id) ?? '') == vaultFilter;
    final subtree = group == null ? <ObjectId>{} : groupSubtree(groups, group.id);
    final scoped = allHosts
        .where(
          (h) => group != null
              ? subtree.contains(h.groupId)
              : location == inventoryUngrouped
              ? !groups.containsKey(h.groupId)
              : inVault(h.id),
        )
        .toList();
    final allTags = {for (final h in scoped) ...h.tags}.toList()..sort();
    final hosts = filterHosts(scoped, query: _search.text, tags: _tags, groups: groups);
    if (_descending) hosts.sort((a, b) => b.name.toLowerCase().compareTo(a.name.toLowerCase()));
    final searching = _search.text.trim().isNotEmpty || _tags.isNotEmpty;
    final query = _search.text.trim().toLowerCase();
    final visibleGroups = groups.values.where((g) {
      if (location == inventoryUngrouped || _tags.isNotEmpty) return false;
      if (vaultFilter != null) return inVault(g.id) && !(groups.containsKey(g.parentId) && inVault(g.parentId!));
      if (searching) {
        return (group == null || subtree.contains(g.id) && g.id != group.id) &&
            query
                .split(RegExp(r'\s+'))
                .every('${groupPathName(groups, g.id)} ${g.tags.join(' ')}'.toLowerCase().contains);
      }
      return group != null ? g.parentId == group.id : !groups.containsKey(g.parentId);
    }).toList()..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
    final showHosts = location != inventoryGroups || searching;
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final title =
        group?.name ??
        (teamLocation != null
            ? (teamLocation.name.isEmpty ? l.teamsUnnamed : teamLocation.name)
            : location == inventoryPersonal
            ? l.teamsFilterPersonal
            : location == inventoryGroups
            ? l.groupsTitle
            : location == inventoryUngrouped
            ? l.inventoryUngrouped
            : l.inventoryAllHosts);
    return PageScaffold(
      title: widget.initialGroups ? l.groupsTitle : l.hostsTitle,
      subtitle: l.inventoryOverview(allHosts.length, groups.length),
      actions: [
        GlassButton(
          key: const ValueKey('add-group'),
          icon: Icons.create_new_folder_rounded,
          label: teamLocation != null && group == null ? l.teamsNewGroup : l.groupsNewGroup,
          onPressed: teamLocation != null && group == null
              ? (teamLocation.canEdit ? () => _addTeamGroup(teamLocation) : null)
              : () => showGroupEditor(context, parentId: group?.id),
        ),
        GlassButton.prominent(
          key: const ValueKey('add-host'),
          icon: Icons.add_rounded,
          label: l.hostsNewHost,
          onPressed: teamLocation != null && group == null
              ? (teamLocation.canEdit ? () => _addTeamHost(teamLocation, groups, vaultOf) : null)
              : () => _addHost(group),
        ),
      ],
      body: LayoutBuilder(
        builder: (context, constraints) {
          final wide = constraints.maxWidth >= 760;
          final inspect =
              !AppPlatform.isMobile && !_cards && showHosts && hosts.isNotEmpty && constraints.maxWidth >= 980;
          final inspected = inspect ? hosts.where((host) => host.id == _inspectedHost).firstOrNull : null;
          final navigation = InventoryNavigation(
            groups: groups,
            hosts: allHosts,
            teams: teamEntries,
            personalCount: allHosts.where((h) => vaultOf(h.id) == null).length,
            location: location,
            onSelect: _browse,
            collapsed: _collapsed,
            compact: !wide,
            onToggle: (id) => setState(() => _collapsed.contains(id) ? _collapsed.remove(id) : _collapsed.add(id)),
          );
          final content = _InventoryViewport(
            children: [
              if (!wide) ...[navigation, const SizedBox(height: 12)],
              Row(
                children: [
                  Expanded(
                    child: GlassField(
                      key: const ValueKey('hosts-search'),
                      controller: _search,
                      search: true,
                      leadingIcon: Icons.search_rounded,
                      placeholder: l.inventorySearch,
                      onChanged: (_) => setState(() {}),
                    ),
                  ),
                  const SizedBox(width: 8),
                  GlassIconButton(
                    icon: _descending ? Icons.arrow_upward : Icons.arrow_downward,
                    tooltip: l.commonName,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => setState(() => _descending = !_descending),
                  ),
                  ViewModeToggle(
                    keyPrefix: 'inventory-view',
                    cards: _cards,
                    onChanged: (cards) => setState(() {
                      _cards = cards;
                      _inspectedHost = null;
                    }),
                  ),
                ],
              ),
              if (allTags.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 10),
                  child: SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: Wrap(
                      spacing: 6,
                      runSpacing: 6,
                      children: [
                        for (final tag in allTags)
                          FilterChip(
                            label: Text(tag),
                            selected: _tags.contains(tag),
                            onSelected: (yes) => setState(() => yes ? _tags.add(tag) : _tags.remove(tag)),
                          ),
                      ],
                    ),
                  ),
                ),
              const SizedBox(height: 14),
              if (group != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: _Breadcrumbs(group: group, groups: groups, onSelect: _browse),
                ),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: tokens.typography.title3.copyWith(color: tokens.palette.label),
                    ),
                  ),
                  if (group != null) ...[
                    if (ref.watch(activeProfileProvider)?.isSynced == true)
                      GlassIconButton(
                        key: const ValueKey('inventory-share-group'),
                        tooltip: l.sharingPublish,
                        icon: Icons.share_outlined,
                        style: GlassIconButtonStyle.plain,
                        onPressed: () => showSharingGroupPublish(context, groupId: group.id.value),
                      ),
                    GlassIconButton(
                      key: const ValueKey('inventory-group-settings'),
                      tooltip: l.inventoryGroupSettings,
                      icon: Icons.tune_rounded,
                      style: GlassIconButtonStyle.plain,
                      onPressed: () => showGroupEditor(context, group: group),
                    ),
                    GlassIconButton(
                      key: const ValueKey('inventory-delete-group'),
                      tooltip: l.commonDelete,
                      icon: Icons.delete_outline_rounded,
                      style: GlassIconButtonStyle.plain,
                      onPressed: () => _deleteGroup(group),
                    ),
                  ],
                  GlassIconButton(
                    key: const ValueKey('inventory-jump-profiles'),
                    tooltip: l.groupsJumpProfilesSection,
                    icon: Icons.alt_route_rounded,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => showJumpProfiles(context),
                  ),
                ],
              ),
              if (teamLocation != null && group == null)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Text(
                    l.teamsNewGroupHint,
                    style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                  ),
                ),
              if (group != null)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Text(
                    l.inventoryIncludesSubgroups,
                    style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                  ),
                ),
              const SizedBox(height: 12),
              Expanded(
                child: AsyncValueView(
                  value: groupsAsync,
                  data: (_) => AsyncValueView(
                    value: hostsAsync,
                    data: (_) {
                      if (visibleGroups.isEmpty && (!showHosts || hosts.isEmpty)) {
                        return EmptyState(
                          icon: searching ? Icons.search_off_rounded : Icons.folder_open_rounded,
                          title: searching
                              ? l.hostsNoMatches
                              : group != null
                              ? l.groupsNoHosts
                              : location == inventoryGroups
                              ? l.groupsEmpty
                              : l.hostsEmptyTitle,
                          message: searching ? l.inventorySearchHelp : l.inventoryEmptyHelp,
                          action: searching
                              ? GlassButton(label: l.inventoryClearFilters, onPressed: () => _browse(location))
                              : GlassButton(
                                  label: location == inventoryGroups ? l.groupsNewGroup : l.hostsNewHost,
                                  icon: Icons.add_rounded,
                                  onPressed: () =>
                                      location == inventoryGroups ? showGroupEditor(context) : _addHost(group),
                                ),
                        );
                      }
                      return LayoutBuilder(
                        builder: (context, box) {
                          final scale = MediaQuery.textScalerOf(context).scale(14) / 14;
                          final columns = (box.maxWidth / (290 * scale.clamp(1, 1.4))).floor().clamp(1, 4);
                          final cardWidth = (box.maxWidth - 12 * (columns - 1)) / columns;
                          final listController = MediaQuery.sizeOf(context).shortestSide < 600
                              ? PrimaryScrollController.maybeOf(context)
                              : _scroll;
                          return Scrollbar(
                            controller: listController,
                            thickness: 4,
                            child: CustomScrollView(
                              controller: listController,
                              slivers: [
                                if (visibleGroups.isNotEmpty && (!inspect || group != null || searching))
                                  SliverToBoxAdapter(
                                    child: Padding(
                                      padding: const EdgeInsets.only(bottom: 20),
                                      child: Wrap(
                                        spacing: 12,
                                        runSpacing: 12,
                                        children: [
                                          for (final g in visibleGroups)
                                            SizedBox(
                                              width: cardWidth,
                                              child: _GroupCard(
                                                group: g,
                                                path: searching ? groupPathName(groups, g.parentId) : '',
                                                count: counts[g.id] ?? 0,
                                                onOpen: () => _browse(g.id.value),
                                              ),
                                            ),
                                        ],
                                      ),
                                    ),
                                  ),
                                if (showHosts)
                                  SliverToBoxAdapter(
                                    child: Padding(
                                      padding: const EdgeInsets.only(bottom: 10),
                                      child: Text(
                                        l.groupsHostCount(hosts.length),
                                        style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                                      ),
                                    ),
                                  ),
                                if (inspect)
                                  SliverToBoxAdapter(
                                    child: WorkspaceColumns(
                                      actionWidth: 172,
                                      flex: const [3, 4, 2, 2],
                                      cells: [
                                        Text(l.uiHost),
                                        Text(l.hostEditorAddressLabel),
                                        Text(l.navGroups),
                                        Text(l.syncScreenState),
                                      ],
                                    ),
                                  ),
                                if (showHosts && hosts.isEmpty)
                                  SliverToBoxAdapter(
                                    child: Padding(
                                      padding: const EdgeInsets.all(24),
                                      child: Text(searching ? l.hostsNoMatches : l.groupsNoHosts),
                                    ),
                                  ),
                                if (showHosts)
                                  SliverList.builder(
                                    itemCount: _cards ? (hosts.length / columns).ceil() : hosts.length,
                                    itemBuilder: (context, index) {
                                      Widget hostTile(Host host) => _HostTile(
                                        host: host,
                                        groupName: groupPathName(groups, host.groupId),
                                        cards: _cards,
                                        inspected: inspected?.id == host.id,
                                        onInspect: inspect
                                            ? () => setState(() {
                                                _inspectedHost = _inspectedHost == host.id ? null : host.id;
                                              })
                                            : null,
                                        onDelete: () => _delete(host),
                                      );
                                      return Padding(
                                        padding: EdgeInsets.only(bottom: _cards ? GlassSpacing.s12 : GlassSpacing.s4),
                                        child: _cards
                                            ? Row(
                                                crossAxisAlignment: CrossAxisAlignment.start,
                                                children: [
                                                  for (var col = 0; col < columns; col++) ...[
                                                    if (col > 0) const SizedBox(width: 12),
                                                    Expanded(
                                                      child: index * columns + col < hosts.length
                                                          ? hostTile(hosts[index * columns + col])
                                                          : const SizedBox.shrink(),
                                                    ),
                                                  ],
                                                ],
                                              )
                                            : hostTile(hosts[index]),
                                      );
                                    },
                                  ),
                                const SliverToBoxAdapter(child: SizedBox(height: 12)),
                              ],
                            ),
                          );
                        },
                      );
                    },
                  ),
                ),
              ),
            ],
          );
          return Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (wide) ...[SizedBox(width: 184, child: navigation), const SizedBox(width: GlassSpacing.s12)],
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Expanded(
                      child: ContentSurface(padding: const EdgeInsets.all(GlassSpacing.s12), child: content),
                    ),
                    if (inspected != null) ...[
                      const SizedBox(height: GlassSpacing.s12),
                      ConstrainedBox(
                        constraints: BoxConstraints(maxHeight: constraints.maxHeight * .46),
                        child: _HostInspector(
                          host: inspected,
                          groupName: groupPathName(groups, inspected.groupId),
                          onClose: () => setState(() => _inspectedHost = null),
                        ),
                      ),
                    ],
                  ],
                ),
              ),
            ],
          );
        },
      ),
    );
  }
}

/// Header filters can scroll away on a phone, including with the IME open.
class _InventoryViewport extends StatelessWidget {
  const _InventoryViewport({required this.children});
  final List<Widget> children;
  @override
  Widget build(BuildContext context) {
    if (MediaQuery.sizeOf(context).shortestSide >= 600) {
      return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: children);
    }
    return NestedScrollView(
      headerSliverBuilder: (context, innerScrolled) => [
        SliverToBoxAdapter(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: children.take(children.length - 1).toList(),
          ),
        ),
      ],
      body: (children.last as Expanded).child,
    );
  }
}

class _Breadcrumbs extends StatelessWidget {
  const _Breadcrumbs({required this.group, required this.groups, required this.onSelect});
  final Group group;
  final Map<ObjectId, Group> groups;
  final ValueChanged<String> onSelect;
  @override
  Widget build(BuildContext context) {
    final parents = <Group>[];
    final seen = <ObjectId>{group.id};
    var cursor = groups[group.parentId];
    while (cursor != null && seen.add(cursor.id)) {
      parents.insert(0, cursor);
      cursor = groups[cursor.parentId];
    }
    final tokens = GlassTokens.of(context);
    return Wrap(
      crossAxisAlignment: WrapCrossAlignment.center,
      spacing: 4,
      children: [
        GlassButton(
          label: context.l10n.inventoryAllHosts,
          style: GlassButtonStyle.plain,
          onPressed: () => onSelect(inventoryAll),
        ),
        for (final parent in parents) ...[
          Icon(Icons.chevron_right_rounded, size: 16, color: tokens.secondaryLabel),
          GlassButton(label: parent.name, style: GlassButtonStyle.plain, onPressed: () => onSelect(parent.id.value)),
        ],
        Icon(Icons.chevron_right_rounded, size: 16, color: tokens.secondaryLabel),
        Text(group.name, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
      ],
    );
  }
}

class _GroupCard extends StatelessWidget {
  const _GroupCard({required this.group, required this.path, required this.count, required this.onOpen});
  final Group group;
  final String path;
  final int count;
  final VoidCallback onOpen;
  @override
  Widget build(BuildContext context) {
    final t = GlassTokens.of(context);
    return GlassInteractive(
      key: ValueKey('inventory-group-${group.name}'),
      onPressed: onOpen,
      semanticLabel: '${group.name}, ${context.l10n.groupsHostCount(count)}',
      builder: (context, state) => GlassFocusRing(
        visible: state.focusVisible,
        shape: GlassRadii.shape(t.radii.card),
        child: ContentSurface(
          color: state.hovered ? t.surfaces.fillHover : null,
          child: Row(
            children: [
              Icon(Icons.folder_rounded, color: t.palette.accent, size: 30),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      group.name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: t.typography.bodyEmph.copyWith(color: t.palette.label),
                    ),
                    const SizedBox(height: 4),
                    Text(
                      path.isEmpty ? context.l10n.groupsHostCount(count) : path,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: t.typography.callout.copyWith(color: t.secondaryLabel),
                    ),
                  ],
                ),
              ),
              Icon(Icons.chevron_right_rounded, size: 18, color: t.secondaryLabel),
            ],
          ),
        ),
      ),
    );
  }
}

enum _HostAction {
  connect,
  notes,
  group,
  sftp,
  edit,
  share,
  refreshShared,
  detachShared,
  copyToTeam,
  moveToTeam,
  moveToPersonal,
  delete,
}

class _HostInspector extends ConsumerWidget {
  const _HostInspector({required this.host, required this.groupName, required this.onClose});

  final Host host;
  final String groupName;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final icon = host.isRdp ? Icons.desktop_windows_rounded : Icons.terminal_rounded;
    final port = host.port ?? (host.isRdp ? defaultRdpPort : defaultSshPort);
    final details = [
      (l.hostEditorAddressLabel, host.address),
      if (host.username case final name?) (l.hostEditorUsernameLabel, name),
      (l.hostEditorPortLabel, '$port'),
      if (groupName.isNotEmpty) (l.navGroups, groupName),
    ];
    return SingleChildScrollView(
      primary: false,
      child: ContentSurface(
        key: const ValueKey('host-inspector'),
        padding: EdgeInsets.zero,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12, vertical: GlassSpacing.s8),
              child: OverflowBar(
                alignment: MainAxisAlignment.spaceBetween,
                spacing: GlassSpacing.s12,
                overflowSpacing: GlassSpacing.s8,
                children: [
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    spacing: GlassSpacing.s8,
                    children: [
                      Icon(icon, size: GlassSizes.iconToolbar, color: tokens.secondaryLabel),
                      Flexible(child: Text(host.name, style: tokens.typography.title3)),
                    ],
                  ),
                  Wrap(
                    spacing: GlassSpacing.s8,
                    runSpacing: GlassSpacing.s8,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      GlassButton.prominent(
                        key: const ValueKey('host-inspector-connect'),
                        label: l.commonConnect,
                        icon: icon,
                        onPressed: () => connectToHost(context, ref, host),
                      ),
                      if (!host.isRdp)
                        GlassButton(
                          key: const ValueKey('host-inspector-sftp'),
                          label: l.navSftp,
                          icon: Icons.folder_copy_rounded,
                          onPressed: () async {
                            await ref.read(sftpControllerProvider.notifier).connect(host);
                            if (context.mounted) context.go(AppRoutes.sftp);
                          },
                        ),
                      GlassButton(
                        key: const ValueKey('host-inspector-edit'),
                        label: l.commonEdit,
                        icon: Icons.edit_outlined,
                        onPressed: () => context.go(AppRoutes.editHost(host.id)),
                      ),
                      GlassIconButton(
                        key: const ValueKey('host-inspector-close'),
                        icon: Icons.close_rounded,
                        tooltip: l.commonClose,
                        style: GlassIconButtonStyle.plain,
                        onPressed: onClose,
                      ),
                    ],
                  ),
                ],
              ),
            ),
            const Divider(height: 1),
            Padding(
              padding: const EdgeInsets.all(GlassSpacing.s12),
              child: LayoutBuilder(
                builder: (context, bounds) {
                  const gap = GlassSpacing.s12;
                  final minWidth = MediaQuery.textScalerOf(context).scale(180);
                  final columns = ((bounds.maxWidth + gap) / (minWidth + gap)).floor().clamp(1, details.length);
                  final width = (bounds.maxWidth - gap * (columns - 1)) / columns;
                  return Wrap(
                    spacing: gap,
                    runSpacing: gap,
                    children: [
                      for (final (label, value) in details)
                        SizedBox(
                          width: width,
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            spacing: GlassSpacing.s4,
                            children: [
                              Text(label, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                              SelectableText(value, style: tokens.typography.callout),
                            ],
                          ),
                        ),
                    ],
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _HostTile extends ConsumerWidget {
  const _HostTile({
    required this.host,
    required this.groupName,
    required this.onDelete,
    this.cards = false,
    this.inspected = false,
    this.onInspect,
  });

  final bool cards;
  final bool inspected;
  final VoidCallback? onInspect;
  final Host host;
  final String groupName;
  final VoidCallback onDelete;

  Future<void> _chooseGroup(BuildContext context, WidgetRef ref) async {
    final profileId = ref.read(activeProfileProvider)?.id;
    final choice = await showAppDialog<({ObjectId? groupId})>(
      context,
      builder: (_) => _HostGroupPicker(hostId: host.id),
    );
    if (choice == null || !context.mounted || ref.read(activeProfileProvider)?.id != profileId) return;
    // Use the latest host: sync may have updated other fields while the picker was open.
    final current = ref.read(hostsProvider).value?.where((h) => h.id == host.id).firstOrNull;
    if (current == null || current.groupId == choice.groupId) return;
    if (choice.groupId != null && !ref.read(groupByIdProvider).containsKey(choice.groupId)) return;
    await runWithFeedback(
      context,
      () => ref.read(inventoryServiceProvider).saveHost(current.withGroup(choice.groupId)),
    );
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    final hasGroups = ref.watch(groupByIdProvider).isNotEmpty;
    final userPart = host.username == null ? '' : '${host.username}@';
    final portPart = host.port == null || host.port == (host.isRdp ? defaultRdpPort : defaultSshPort)
        ? ''
        : ':${host.port}';
    final connected = ref.watch(terminalTabsProvider).tabs.any((tab) => tab.host?.id == host.id && tab.isConnected);
    final team = teamOf(ref, host.id);
    final teams = ref.watch(teamsProvider).value ?? const <Team>[];
    final writableTeams = teams.where((x) => x.open && x.canEdit && x.vaultId != team?.vaultId).toList();
    final heading = Row(
      children: [
        Flexible(
          child: Text(
            host.name,
            overflow: TextOverflow.ellipsis,
            style: t.bodyEmph.copyWith(color: tokens.palette.label),
          ),
        ),
        if (team != null) ...[
          const SizedBox(width: GlassSpacing.s6),
          TeamChip(key: ValueKey('host-team-${host.name}'), team: team),
        ],
        if (host.jumpChain.isNotEmpty || host.jumpProfileId != null) ...[
          const SizedBox(width: GlassSpacing.s6),
          Tooltip(
            message: l10n.hostsViaJumpHostsTooltip,
            child: Icon(Icons.alt_route_rounded, size: 16, color: tokens.secondaryLabel),
          ),
        ],
        if (host.backend == SshBackend.openSsh) ...[
          const SizedBox(width: GlassSpacing.s6),
          Tooltip(
            message: l10n.hostsOpenSshTooltip,
            child: Icon(Icons.extension_rounded, size: 16, color: tokens.secondaryLabel),
          ),
        ],
      ],
    );
    final noteTarget = hostNoteContext(host).target;
    final noteCount = (ref.watch(notesProvider).value ?? const <Note>[]).where((n) => n.target == noteTarget).length;
    final actions = Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (noteCount > 0) ...[
          GlassButton(
            key: ValueKey('host-notes-${host.name}'),
            label: '$noteCount',
            icon: Icons.sticky_note_2_outlined,
            style: GlassButtonStyle.plain,
            size: GlassControlSize.sm,
            tooltip: l10n.hostsNotesCount(noteCount),
            onPressed: () => showHostNotes(ref, host),
          ),
          const SizedBox(width: GlassSpacing.s4),
        ],
        if (onInspect != null) ...[
          GlassIconButton(
            key: ValueKey('inspect-${host.name}'),
            icon: Icons.info_outline_rounded,
            tooltip: l10n.hostInspector,
            style: GlassIconButtonStyle.plain,
            selected: inspected,
            onPressed: onInspect,
          ),
          const SizedBox(width: GlassSpacing.s4),
        ],
        GlassIconButton(
          key: ValueKey('connect-${host.name}'),
          onPressed: () => connectToHost(context, ref, host),
          icon: host.isRdp ? Icons.desktop_windows_rounded : Icons.terminal_rounded,
          tooltip: l10n.commonConnect,
          style: GlassIconButtonStyle.plain,
        ),
        const SizedBox(width: GlassSpacing.s4),
        GlassMenuButton<_HostAction>(
          entries: [
            GlassMenuItem(
              key: ValueKey('host-connect-menu-${host.name}'),
              value: _HostAction.connect,
              label: l10n.commonConnect,
              icon: host.isRdp ? Icons.desktop_windows_rounded : Icons.terminal_rounded,
            ),
            GlassMenuItem(
              key: ValueKey('host-notes-menu-${host.name}'),
              value: _HostAction.notes,
              label: l10n.navNotes,
              icon: Icons.sticky_note_2_outlined,
            ),
            if (hasGroups)
              GlassMenuItem(
                key: ValueKey('host-group-menu-${host.name}'),
                value: _HostAction.group,
                label: l10n.hostsAssignGroup,
                icon: Icons.drive_file_move_rounded,
              ),
            const GlassMenuDivider(),
            if (!host.isRdp)
              GlassMenuItem(value: _HostAction.sftp, label: l10n.hostsOpenSftp, icon: Icons.folder_copy_rounded),
            if (ref.watch(activeProfileProvider)?.isSynced == true)
              GlassMenuItem(value: _HostAction.share, label: l10n.sharingPublish, icon: Icons.share_outlined),
            GlassMenuItem(
              key: ValueKey('host-edit-menu-${host.name}'),
              value: _HostAction.edit,
              label: l10n.commonEdit,
              icon: Icons.edit_rounded,
            ),
            if (host.metadata.containsKey('cc.shared.share') || host.metadata.containsKey('cc.shared.instance')) ...[
              GlassMenuItem(
                value: _HostAction.refreshShared,
                label: l10n.sharingRefreshHost,
                icon: Icons.refresh_rounded,
              ),
              GlassMenuItem(
                value: _HostAction.detachShared,
                label: l10n.sharingDetachHost,
                icon: Icons.link_off_rounded,
              ),
            ],
            if (writableTeams.isNotEmpty || team != null) ...[
              const GlassMenuDivider(),
              if (writableTeams.isNotEmpty) ...[
                GlassMenuItem(
                  key: ValueKey('host-copy-team-${host.name}'),
                  value: _HostAction.copyToTeam,
                  label: l10n.teamsCopyTo,
                  icon: Icons.copy_all_rounded,
                ),
                // Only the owner takes a host out of a team.
                if (team == null || team.isOwner)
                  GlassMenuItem(
                    key: ValueKey('host-move-team-${host.name}'),
                    value: _HostAction.moveToTeam,
                    label: l10n.teamsMoveTo,
                    icon: Icons.drive_file_move_rounded,
                  ),
              ],
              if (team != null && team.isOwner)
                GlassMenuItem(
                  key: ValueKey('host-move-personal-${host.name}'),
                  value: _HostAction.moveToPersonal,
                  label: l10n.teamsMoveToPersonal,
                  icon: Icons.person_rounded,
                ),
            ],
            const GlassMenuDivider(),
            GlassMenuItem(
              value: _HostAction.delete,
              label: l10n.commonDelete,
              icon: Icons.delete_rounded,
              destructive: true,
            ),
          ],
          onSelected: (v) async {
            if (!context.mounted) return;
            switch (v) {
              case _HostAction.connect:
                await connectToHost(context, ref, host);
              case _HostAction.notes:
                showHostNotes(ref, host);
              case _HostAction.share:
                if (host.isRdp) {
                  await showSharingRdpHost(context, hostId: host.id.value);
                } else {
                  await showSharingPublish(context, kind: SharingKind.host, objectId: host.id.value);
                }
              case _HostAction.group:
                await _chooseGroup(context, ref);
              case _HostAction.sftp:
                await ref.read(sftpControllerProvider.notifier).connect(host);
                if (context.mounted) context.go(AppRoutes.sftp);
              case _HostAction.edit:
                context.go(AppRoutes.editHost(host.id));
              case _HostAction.refreshShared:
                await showSharingBoundHost(context, host);
              case _HostAction.detachShared:
                final profile = ref.read(sharingSessionScopeProvider);
                final ok = await showConfirmDialog(
                  context,
                  title: l10n.sharingDetachHost,
                  message: l10n.sharingDetachHostHelp,
                  confirmLabel: l10n.sharingDetachHost,
                );
                if (ok && context.mounted && sharingSessionCurrent(ref, profile)) {
                  await runWithFeedback(context, () => ref.read(sharingServiceProvider).detachHost(host.id.value));
                }
              case _HostAction.copyToTeam || _HostAction.moveToTeam:
                final target = await pickTeam(context, writableTeams);
                if (target == null || !context.mounted) return;
                final move = v == _HostAction.moveToTeam;
                final service = ref.read(teamServiceProvider);
                final moved = ref.read(movedObjectsProvider.notifier);
                await runWithFeedback(
                  context,
                  () async => move
                      ? moved.record([host.id.value], await service.moveObjects([host.id.value], target.vaultId))
                      : await service.copyObjects([host.id.value], target.vaultId),
                  success: move ? l10n.teamsMoved(target.name) : l10n.teamsCopied(target.name),
                );
              case _HostAction.moveToPersonal:
                final moved = ref.read(movedObjectsProvider.notifier);
                await runWithFeedback(
                  context,
                  () async => moved.record([
                    host.id.value,
                  ], await ref.read(teamServiceProvider).moveObjects([host.id.value], null)),
                  success: l10n.teamsMoved(l10n.teamsFilterPersonal),
                );
              case _HostAction.delete:
                onDelete();
            }
          },
          builder: (context, open) => GlassIconButton(
            key: ValueKey('host-menu-${host.name}'),
            icon: Icons.more_horiz_rounded,
            tooltip: l10n.hostsMoreTooltip,
            style: GlassIconButtonStyle.plain,
            onPressed: open,
          ),
        ),
      ],
    );
    final endpoint = '$userPart${host.address}$portPart';
    final detail = groupName.isEmpty ? l10n.inventoryUngrouped : groupName;
    if (!cards && onInspect != null) {
      return Material(
        color: inspected ? tokens.sidebarSelection : Colors.transparent,
        child: InkWell(
          key: ValueKey('host-${host.name}'),
          onTap: () => connectToHost(context, ref, host),
          child: WorkspaceColumns(
            actionWidth: 172,
            actions: Align(alignment: Alignment.centerRight, child: actions),
            flex: const [3, 4, 2, 2],
            cells: [
              Row(
                children: [
                  Icon(host.isRdp ? Icons.desktop_windows_outlined : Icons.dns_outlined, size: 16),
                  const SizedBox(width: 6),
                  Expanded(child: heading),
                ],
              ),
              Text(
                endpoint,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: t.mono.copyWith(fontSize: t.callout.fontSize),
              ),
              Text(detail, maxLines: 2, overflow: TextOverflow.ellipsis),
              Text(
                connected
                    ? l10n.inventoryConnected
                    : host.isRdp
                    ? 'RDP'
                    : 'SSH',
              ),
            ],
          ),
        ),
      );
    }
    if (!cards) {
      return ContentSurface(
        padding: EdgeInsets.zero,
        border: inspected,
        color: inspected ? tokens.sidebarSelection : Colors.transparent,
        child: ListTile(
          key: ValueKey('host-${host.name}'),
          contentPadding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8, vertical: GlassSpacing.s4),
          leading: HostAvatar(host: host, size: 40),
          title: heading,
          subtitle: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SelectableText(
                endpoint,
                maxLines: 1,
                style: t.mono.copyWith(fontSize: t.callout.fontSize, color: tokens.secondaryLabel),
              ),
              Text(
                detail,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: t.callout.copyWith(color: tokens.secondaryLabel),
              ),
            ],
          ),
          trailing: actions,
          onTap: () => connectToHost(context, ref, host),
        ),
      );
    }
    return GlassInteractive(
      key: ValueKey('host-${host.name}'),
      descendantsAreFocusable: true,
      semanticLabel: '${host.name}, $endpoint, ${l10n.commonConnect}',
      onPressed: () => connectToHost(context, ref, host),
      builder: (context, state) => GlassFocusRing(
        visible: state.focusVisible,
        shape: GlassRadii.shape(tokens.radii.card),
        child: ContentSurface(
          color: state.hovered ? Color.alphaBlend(tokens.surfaces.fillHover, tokens.surfaces.content) : null,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  HostAvatar(host: host, size: 36),
                  const SizedBox(width: 10),
                  Expanded(child: heading),
                  if (connected)
                    Padding(
                      padding: const EdgeInsets.only(left: 6),
                      child: Tooltip(
                        message: l10n.inventoryConnected,
                        child: Icon(Icons.circle, size: 8, color: tokens.palette.success),
                      ),
                    ),
                ],
              ),
              const SizedBox(height: 12),
              SelectableText(
                endpoint,
                maxLines: 1,
                style: t.mono.copyWith(fontSize: t.callout.fontSize, color: tokens.palette.label),
              ),
              const SizedBox(height: 6),
              Row(
                children: [
                  Icon(Icons.folder_outlined, size: 14, color: tokens.secondaryLabel),
                  const SizedBox(width: 6),
                  Expanded(
                    child: Text(
                      detail,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: t.callout.copyWith(color: tokens.secondaryLabel),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      host.tags.isEmpty ? (host.isRdp ? 'RDP' : 'SSH') : host.tags.join(' · '),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: t.caption.copyWith(color: tokens.secondaryLabel),
                    ),
                  ),
                  const SizedBox(width: 8),
                  actions,
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _HostGroupPicker extends ConsumerStatefulWidget {
  const _HostGroupPicker({required this.hostId});

  final ObjectId hostId;

  @override
  ConsumerState<_HostGroupPicker> createState() => _HostGroupPickerState();
}

class _HostGroupPickerState extends ConsumerState<_HostGroupPicker> {
  String _query = '';

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final groups = ref.watch(groupByIdProvider);
    final host = ref.watch(hostsProvider).value?.where((h) => h.id == widget.hostId).firstOrNull;
    final sortedGroups = groups.keys.map((id) => (groupId: id, name: groupPathName(groups, id))).toList()
      ..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
    final choices = [
      (groupId: null, name: l10n.hostEditorNoGroup),
      ...sortedGroups,
    ].where((choice) => choice.name.toLowerCase().contains(_query.trim().toLowerCase())).toList();
    return GlassDialog(
      key: const ValueKey('host-group-picker'),
      title: l10n.hostsAssignGroup,
      width: 520,
      content: SizedBox(
        height: 360,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(l10n.hostsGroupHelp),
            const SizedBox(height: GlassSpacing.s12),
            GlassField(
              key: const ValueKey('host-group-search'),
              search: true,
              autofocus: true,
              leadingIcon: Icons.search_rounded,
              placeholder: l10n.hostsGroupSearchHint,
              onChanged: (value) => setState(() => _query = value),
            ),
            const SizedBox(height: GlassSpacing.s8),
            Expanded(
              child: choices.isEmpty
                  ? EmptyState(icon: Icons.folder_open_rounded, title: l10n.hostsGroupNoMatches, compact: true)
                  : ListView.builder(
                      itemCount: choices.length,
                      itemBuilder: (context, i) {
                        final choice = choices[i];
                        final selected = host?.groupId == choice.groupId;
                        return ListTile(
                          key: ValueKey('host-group-choice-${choice.groupId?.value ?? 'none'}'),
                          leading: Icon(choice.groupId == null ? Icons.folder_off_rounded : Icons.folder_rounded),
                          title: Text(choice.name),
                          selected: selected,
                          trailing: selected ? const Icon(Icons.check_rounded) : null,
                          onTap: host == null ? null : () => closeDialog(context, (groupId: choice.groupId)),
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<({ObjectId? groupId})>(context)),
      ],
    );
  }
}

/// Host glyph in a soft rounded tile (list rows, pickers).
class HostAvatar extends StatelessWidget {
  const HostAvatar({required this.host, super.key, this.size = 32});

  final Host host;
  final double size;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return SizedBox.square(
      dimension: size,
      child: DecoratedBox(
        decoration: ShapeDecoration(
          color: tokens.surfaces.inset,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(size * .28)),
        ),
        child: Center(
          child: AppSymbolIcon(AppSymbol.hosts, size: size * .55, color: tokens.secondaryLabel),
        ),
      ),
    );
  }
}
