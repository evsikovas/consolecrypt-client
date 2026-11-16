import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class AutomationGroupsDialog extends ConsumerStatefulWidget {
  const AutomationGroupsDialog({super.key, required this.scope, required this.projectId, required this.service});
  final AutomationScope scope;
  final String projectId;
  final AutomationInventoryService service;
  @override
  ConsumerState<AutomationGroupsDialog> createState() => _AutomationGroupsDialogState();
}

class _AutomationGroupsDialogState extends ConsumerState<AutomationGroupsDialog> {
  final name = TextEditingController();
  final groups = <String, Set<String>>{};
  String? selected;
  String? selectedFolder;
  Map<String, String> folderBindings = {};
  bool folderCompositionUnavailable = false;
  int? revision;
  List<String>? selectedHosts;
  String? nativeProfileId;
  String? nativeNotificationScopeId;
  int sourceEpoch = 0;
  bool busy = true;
  Object? error;
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  @override
  void initState() {
    super.initState();
    load();
  }

  @override
  void didUpdateWidget(covariant AutomationGroupsDialog oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.scope, widget.scope) ||
        oldWidget.projectId != widget.projectId ||
        !identical(oldWidget.service, widget.service)) {
      sourceEpoch++;
      groups.clear();
      folderBindings = {};
      selected = null;
      selectedFolder = null;
      revision = null;
      selectedHosts = null;
      nativeProfileId = null;
      nativeNotificationScopeId = null;
      folderCompositionUnavailable = false;
      busy = true;
      error = null;
      name.clear();
      load();
    }
  }

  @override
  void dispose() {
    name.dispose();
    super.dispose();
  }

  Future<void> load() async {
    final epoch = sourceEpoch;
    try {
      final value = await widget.service.inventory(widget.projectId);
      if (!mounted || !current || epoch != sourceEpoch) return;
      final inventory = value['inventory'] as AutomationJson;
      if (value['version'] != 1 || value['project_id'] != widget.projectId || ![1, 2].contains(inventory['version'])) {
        throw FormatException(context.l10n.automationGroupsInvalid);
      }
      final loaded = inventory['groups'] as Map<String, dynamic>;
      if (widget.service is AutomationFolderBindingService &&
          (value['profile_id'] is! String ||
              value['profile_id'] != widget.scope.profileId?.toString() ||
              value['notification_scope_id'] is! String ||
              !RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
                  .hasMatch(value['notification_scope_id'] as String))) {
        throw FormatException(context.l10n.automationGroupsInvalid);
      }
      final mapping = value['folder_mapping'];
      final bindings = mapping == null
          ? <String, String>{}
          : readBindings((mapping as AutomationJson)['bindings'], loaded.keys.toSet());
      setState(() {
        nativeProfileId = value['profile_id'] as String?;
        nativeNotificationScopeId = value['notification_scope_id'] as String?;
        folderBindings = bindings;
        folderCompositionUnavailable = mapping is Map && mapping['status'] == 'unavailable';
        revision = value['project_revision'] as int;
        selectedHosts = (inventory['selected_hosts'] as List?)?.cast<String>();
        groups.addAll({for (final entry in loaded.entries) entry.key: (entry.value as List).cast<String>().toSet()});
        selected = groups.keys.firstOrNull;
        selectedFolder = folderBindings[selected];
      });
    } catch (e) {
      if (current && epoch == sourceEpoch) setState(() => error = e);
    } finally {
      if (current && epoch == sourceEpoch) setState(() => busy = false);
    }
  }

  void add() {
    final value = name.text.trim();
    if (!RegExp(r'^[A-Za-z_][A-Za-z0-9_-]{0,95}$').hasMatch(value) ||
        ['all', 'ungrouped', '_meta'].contains(value) ||
        groups.containsKey(value) ||
        groups.length >= 64) {
      setState(() => error = FormatException(context.l10n.automationGroupsInvalid));
      return;
    }
    setState(() {
      groups[value] = {};
      selected = value;
      selectedFolder = null;
      name.clear();
      error = null;
    });
  }

  Future<void> save() async {
    final epoch = sourceEpoch;
    if (!current || busy || revision == null) return;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final updated = await widget.service.saveInventory(
        widget.projectId,
        revision: revision!,
        inventory: {
          'version': selectedHosts == null ? 1 : 2,
          if (selectedHosts != null) 'selected_hosts': selectedHosts,
          'groups': {for (final entry in groups.entries) entry.key: entry.value.toList()..sort()},
        },
      );
      if (mounted && current && epoch == sourceEpoch) Navigator.pop(context, updated);
    } catch (e) {
      if (current && epoch == sourceEpoch) setState(() => error = e);
    } finally {
      if (current && epoch == sourceEpoch) setState(() => busy = false);
    }
  }

  Map<String, String> readBindings(dynamic raw, Set<String> names) {
    if (raw is! Map || raw['version'] != 1 || raw['groups'] is! Map || (raw['groups'] as Map).length > 64) {
      throw FormatException(context.l10n.automationGroupsInvalid);
    }
    final result = <String, String>{};
    final uuid = RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$');
    for (final entry in (raw['groups'] as Map).entries) {
      final value = entry.value;
      if (entry.key is! String ||
          !names.contains(entry.key) ||
          value is! Map ||
          value['mode'] != 'direct' ||
          value['folder_id'] is! String ||
          !uuid.hasMatch(value['folder_id'] as String) ||
          value['folder_id'] == '00000000-0000-0000-0000-000000000000') {
        throw FormatException(context.l10n.automationGroupsInvalid);
      }
      result[entry.key as String] = value['folder_id'] as String;
    }
    return result;
  }

  Future<void> changeFolder(AutomationFolderBindingAction action) async {
    final epoch = sourceEpoch;
    final service = widget.service;
    final group = selected;
    final folder = selectedFolder;
    final expected = revision;
    final profileId = nativeProfileId;
    final nonce = nativeNotificationScopeId;
    if (!current ||
        busy ||
        expected == null ||
        profileId == null ||
        nonce == null ||
        group == null ||
        service is! AutomationFolderBindingService ||
        (action != AutomationFolderBindingAction.unlink && folder == null)) {
      return;
    }
    setState(() {
      busy = true;
      error = null;
    });
    try {
      final result = await (service as AutomationFolderBindingService).folderBinding(
        widget.projectId,
        profileId: profileId,
        notificationScopeId: nonce,
        revision: expected,
        groupName: group,
        action: action,
        folderId: folder,
      );
      if (!mounted || !current || epoch != sourceEpoch) return;
      final project = AutomationProject.fromJson(result['project'] as AutomationJson);
      final inventory = result['inventory'] as AutomationJson;
      final members = ((inventory['groups'] as Map)[group] as List).cast<String>().toSet();
      final bindings = readBindings(result['folder_bindings'], {...groups.keys, group});
      if (![1, 2].contains(inventory['version']) ||
          !['available', 'unavailable'].contains(result['composition_status']) ||
          result['version'] != 1 ||
          project.id != widget.projectId ||
          project.revision != expected + 1 ||
          project.readOnly ||
          (action == AutomationFolderBindingAction.linkDirect
              ? bindings[group] != folder
              : bindings.containsKey(group))) {
        throw FormatException(context.l10n.automationGroupsInvalid);
      }
      setState(() {
        revision = project.revision;
        // Merge ONLY this explicit group. Other unsaved checkbox/group edits
        // remain until Save; a metadata action never selects run host_ids.
        groups[group] = members;
        folderBindings = bindings;
        selectedFolder = folderBindings[group] ?? folder;
        folderCompositionUnavailable = result['composition_status'] == 'unavailable';
      });
    } catch (e) {
      if (current && epoch == sourceEpoch) setState(() => error = e);
    } finally {
      if (current && epoch == sourceEpoch) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final scope = ref.watch(automationScopeProvider);
    if (!identical(scope, widget.scope)) return const SizedBox.shrink();
    final l = context.l10n;
    final folders = ref.watch(groupsProvider);
    final availableFolders = folders.value ?? const <Group>[];
    final linked = folderBindings[selected];
    final folderKnown = availableFolders.any((f) => f.id.value == selectedFolder);
    final hosts = ref.watch(hostsProvider);
    final available = [
      for (final host in hosts.value ?? <Host>[])
        if (host.protocol == HostProtocol.ssh) host,
    ];
    final members = groups[selected];
    final chosenCount = available.where((host) => host.groupId?.value == selectedFolder).length;
    final linkedFolder = availableFolders.where((folder) => folder.id.value == linked).firstOrNull;
    final proposed = linked == null
        ? <String>{}
        : available.where((host) => host.groupId?.value == linked).map((host) => host.id.value).toSet();
    final missing = members?.where((id) => !available.any((host) => host.id.value == id)).toList() ?? <String>[];
    return AlertDialog(
      title: Text(l.automationGroups),
      content: SizedBox(
        width: 640,
        child: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
          child: ListView(
            shrinkWrap: true,
            children: [
              Text(l.automationGroupsHint),
              const SizedBox(height: 12),
              if (busy) const LinearProgressIndicator(),
              if (error != null)
                Text(
                  error is AutomationFolderBindingChanged
                      ? l.automationFolderChanged
                      : error is FormatException
                      ? (error as FormatException).message
                      : errorMessage(l, error!),
                ),
              TextField(
                key: const ValueKey('automation-group-name'),
                controller: name,
                enabled: !busy && revision != null,
                decoration: InputDecoration(labelText: l.automationGroupName),
                autocorrect: false,
              ),
              Wrap(
                spacing: 8,
                children: [
                  TextButton(
                    key: const ValueKey('automation-group-add'),
                    onPressed: busy || revision == null ? null : add,
                    child: Text(l.commonAdd),
                  ),
                  if (selected != null)
                    TextButton(
                      key: const ValueKey('automation-group-delete'),
                      onPressed: busy || folderBindings.containsKey(selected)
                          ? null
                          : () => setState(() {
                              groups.remove(selected);
                              selected = groups.keys.firstOrNull;
                              selectedFolder = folderBindings[selected];
                            }),
                      child: Text(l.commonDelete),
                    ),
                ],
              ),
              GlassSelect<String?>(
                key: const ValueKey('automation-ansible-group'),
                value: selected,
                expand: true,
                semanticLabel: l.automationGroups,
                onChanged: busy
                    ? null
                    : (value) => setState(() {
                        selected = value;
                        selectedFolder = folderBindings[value];
                      }),
                items: [
                  GlassSelectItem(value: null, label: l.automationGroups),
                  for (final group in groups.keys)
                    GlassSelectItem(value: group, label: group, key: ValueKey('automation-ansible-group-$group')),
                ],
              ),
              if (selected != null && widget.service is AutomationFolderBindingService) ...[
                const SizedBox(height: 12),
                Text(l.automationFolderDirectHint),
                if (folderCompositionUnavailable) Text(l.automationFolderUnavailable),
                if (folders.isLoading) const LinearProgressIndicator(),
                if (folders.hasError) Text(errorMessage(l, folders.error!)),
                GlassSelect<String?>(
                  key: const ValueKey('automation-group-folder'),
                  value: selectedFolder,
                  expand: true,
                  semanticLabel: l.automationFolderSelect,
                  onChanged: busy ? null : (value) => setState(() => selectedFolder = value),
                  items: [
                    GlassSelectItem(value: null, label: l.automationFolderSelect),
                    for (final folder in availableFolders)
                      GlassSelectItem(
                        value: folder.id.value,
                        label: folder.name,
                        key: ValueKey('automation-folder-option-${folder.id.value}'),
                      ),
                    if (selectedFolder != null && !folderKnown)
                      GlassSelectItem(value: selectedFolder, label: l.automationFolderMissing),
                  ],
                ),
                if (selectedFolder != null && folderKnown) Text(l.automationFolderMemberCount(chosenCount)),
                if (linked != null) Text(l.automationFolderLinkedName(linkedFolder?.name ?? l.automationFolderMissing)),
                Wrap(
                  spacing: 8,
                  children: [
                    TextButton(
                      key: const ValueKey('automation-folder-link'),
                      onPressed: busy || !folderKnown
                          ? null
                          : () => changeFolder(AutomationFolderBindingAction.linkDirect),
                      child: Text(l.automationFolderLink),
                    ),
                    TextButton(
                      key: const ValueKey('automation-folder-snapshot'),
                      onPressed: busy || !folderKnown
                          ? null
                          : () => changeFolder(AutomationFolderBindingAction.snapshotDirect),
                      child: Text(l.automationFolderSnapshot),
                    ),
                    if (linked != null)
                      TextButton(
                        key: const ValueKey('automation-folder-unlink'),
                        onPressed: busy ? null : () => changeFolder(AutomationFolderBindingAction.unlink),
                        child: Text(l.automationFolderUnlink),
                      ),
                  ],
                ),
              ],
              if (members != null) ...[
                if (hosts.isLoading) const LinearProgressIndicator(),
                if (hosts.hasError) Text(errorMessage(l, hosts.error!)),
                for (final host in available)
                  CheckboxListTile(
                    key: ValueKey('automation-group-host-${host.id}'),
                    dense: true,
                    value: linked == null ? members.contains(host.id.value) : proposed.contains(host.id.value),
                    title: Text(host.name),
                    subtitle: Text(host.address),
                    onChanged: busy || linked != null
                        ? null
                        : (value) => setState(() {
                            if (value == true) {
                              members.add(host.id.value);
                            } else {
                              members.remove(host.id.value);
                            }
                          }),
                  ),
                for (final id in missing)
                  CheckboxListTile(
                    dense: true,
                    value: true,
                    title: Text(l.automationGroupMissing),
                    subtitle: Text(id),
                    onChanged: busy ? null : (_) => setState(() => members.remove(id)),
                  ),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: busy && revision != null ? null : () => Navigator.pop(context),
          child: Text(l.commonCancel),
        ),
        FilledButton(
          key: const ValueKey('automation-groups-save'),
          onPressed: busy || revision == null ? null : save,
          child: Text(l.commonSave),
        ),
      ],
    );
  }
}
