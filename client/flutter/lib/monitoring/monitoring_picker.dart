import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/monitoring_preferences.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/monitoring/monitoring_inventory.dart';
import 'package:material_ui/material_ui.dart';

class MonitoringPicker extends StatefulWidget {
  const MonitoringPicker({super.key, required this.inventory, required this.selection});
  final MonitoringInventory inventory;
  final MonitoringPreferences selection;

  @override
  State<MonitoringPicker> createState() => _MonitoringPickerState();
}

class _MonitoringPickerState extends State<MonitoringPicker> {
  late final hosts = {...widget.selection.hosts}, groups = {...widget.selection.groups};
  bool showGroups = false;
  String query = '';

  void toggle(Set<String> items, String id, bool? value) => setState(() {
    if (value == true) {
      items.add(id);
    } else {
      items.remove(id);
    }
  });

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, inventory = widget.inventory;
    final folders = inventory.groups.values.where((g) => inventory.path(g).toLowerCase().contains(query)).toList()
      ..sort((a, b) => inventory.path(a).compareTo(inventory.path(b)));
    final filtered = inventory.hosts
        .where((h) => h.name.toLowerCase().contains(query) || h.address.toLowerCase().contains(query))
        .toList();
    final count = showGroups ? folders.length : filtered.length;
    return GlassDialog(
      title: l.monitorChoose,
      width: 520,
      content: SizedBox(
        height: 380,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Wrap(
              spacing: 8,
              children: [
                ChoiceChip(
                  label: Text(l.navHosts),
                  selected: !showGroups,
                  onSelected: (_) => setState(() => showGroups = false),
                ),
                ChoiceChip(
                  label: Text(l.navGroups),
                  selected: showGroups,
                  onSelected: (_) => setState(() => showGroups = true),
                ),
              ],
            ),
            const SizedBox(height: 8),
            GlassField(
              search: true,
              leadingIcon: Icons.search_rounded,
              placeholder: l.monitorSearch,
              onChanged: (v) => setState(() => query = v.trim().toLowerCase()),
            ),
            const SizedBox(height: 8),
            Expanded(
              child: count == 0
                  ? Center(child: Text(l.hostPickerEmpty))
                  : ListView.builder(
                      itemCount: count,
                      itemBuilder: (context, i) {
                        if (showGroups) {
                          final group = folders[i];
                          final inherited = inventory
                              .ancestors(group.parentId)
                              .any((g) => g.id != group.id && groups.contains(g.id.value));
                          return CheckboxListTile(
                            title: Text(inventory.path(group)),
                            value: inherited || groups.contains(group.id.value),
                            onChanged: inherited ? null : (v) => toggle(groups, group.id.value, v),
                          );
                        }
                        final host = filtered[i], inherited = inventory.includedByGroup(host, groups);
                        return CheckboxListTile(
                          title: Text(host.name),
                          subtitle: Text(host.address),
                          value: inherited || hosts.contains(host.id.value),
                          onChanged: inherited ? null : (v) => toggle(hosts, host.id.value, v),
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(label: l.commonCancel, onPressed: () => closeDialog<MonitoringPreferences>(context)),
      ],
      primaryAction: GlassButton(
        label: l.commonSave,
        onPressed: () => closeDialog(context, widget.selection.copyWith(hosts: hosts, groups: groups)),
      ),
    );
  }
}
