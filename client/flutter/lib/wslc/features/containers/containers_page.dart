import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/compose/compose_dialog.dart';
import 'package:consolecrypt/wslc/features/containers/container_actions.dart';
import 'package:consolecrypt/wslc/features/containers/run_wizard.dart';
import 'package:consolecrypt/wslc/shared/ports.dart';
import 'package:consolecrypt/wslc/shared/usage.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

enum ContainerFilter { all, running, stopped }

class ContainersPage extends StatefulWidget {
  const ContainersPage({super.key, required this.controller});
  final DeskController controller;
  @override
  State<ContainersPage> createState() => _ContainersPageState();
}

class _ContainersPageState extends State<ContainersPage> {
  final search = TextEditingController();
  ContainerFilter filter = ContainerFilter.all;

  /// Full IDs of checked rows; pruned when containers disappear.
  final Set<String> selected = {};

  @override
  void dispose() {
    search.dispose();
    super.dispose();
  }

  bool matches(ContainerSummary r) {
    final passes = switch (filter) {
      ContainerFilter.all => true,
      ContainerFilter.running => r.state == ContainerState.running,
      ContainerFilter.stopped => r.state == ContainerState.exited || r.state == ContainerState.created,
    };
    if (!passes) return false;
    final query = search.text.trim().toLowerCase();
    return query.isEmpty ||
        '${r.name ?? ''} ${r.id} ${r.image ?? ''} ${r.portsDisplay ?? ''}'.toLowerCase().contains(query);
  }

  static bool eligible(ContainerSummary r, String action) => switch (action) {
    'start' => canStart(r.state),
    'stop' || 'restart' => canStop(r.state),
    _ => true,
  };

  /// Confirms (except start), runs the bulk action and reports a summary.
  Future<void> runBulk(BuildContext context, List<ContainerSummary> rows, String action) async {
    final targets = rows.where((r) => eligible(r, action)).toList();
    if (targets.isEmpty) return;
    final names = targets.map((r) => '• ${r.name ?? r.id.substring(0, 12)}');
    final l10n = context.l10n;
    if (action != 'start') {
      final running = targets.any((r) => r.state == ContainerState.running);
      final warning = action == 'remove'
          ? l10n.wslcRemovalCannotBeUndoneNamedVolumesWillBe(
              running ? l10n.wslcRunningContainersWillBeForciblyStopped : '',
            )
          : l10n.wslcTheRuntimeMayRemoveContainersWithAutoRemove;
      final ok = await confirmDanger(
        context,
        icon: switch (action) {
          'stop' => Icons.stop_rounded,
          'restart' => Icons.restart_alt_rounded,
          _ => Icons.delete_outline_rounded,
        },
        title: switch (action) {
          'stop' => context.l10n.wslcStopTheSelectedContainers(targets.length),
          'restart' => context.l10n.wslcRestartTheSelectedContainers(targets.length),
          _ => context.l10n.wslcRemoveTheSelectedContainers(targets.length),
        },
        message: [...names, '', warning].join('\n'),
        action: switch (action) {
          'stop' => context.l10n.dockerActionStop,
          'restart' => context.l10n.dockerActionRestart,
          _ => context.l10n.commonDelete,
        },
      );
      if (!ok || !context.mounted) return;
    }
    final notify = WslcApiScope.notifierOf(context);
    final (done, failures) = await widget.controller.bulk(targets, action);
    if (mounted) setState(() => selected.clear());
    notify(
      l10n.wslcDoneOf(done, targets.length, failures.isEmpty ? '' : '\n${failures.join('\n')}'),
      error: failures.isNotEmpty,
    );
  }

  Future<void> createContainer(BuildContext context) async {
    final id = await showRunWizard(context, widget.controller);
    if (id != null) widget.controller.openContainer(id);
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    final p = DeskPalette.of(context);
    final all = c.containers;
    final running = all.where((r) => r.state == ContainerState.running).length;
    final stopped = all.where((r) => r.state == ContainerState.exited || r.state == ContainerState.created).length;
    final rows = all.where(matches).toList(growable: false);
    selected.retainWhere((id) => all.any((r) => r.id == id));
    final chosen = all.where((r) => selected.contains(r.id)).toList();
    final visibleIds = rows.map((r) => r.id).toSet();
    final checkedVisible = visibleIds.where(selected.contains).length;
    final allState = checkedVisible == 0
        ? false
        : checkedVisible == visibleIds.length
        ? true
        : null;
    String count(int n) => c.ready ? '$n' : '—';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: StatCard(
                label: context.l10n.snippetCatalogContainers,
                value: count(all.length),
                icon: Icons.view_in_ar_rounded,
                color: p.accent,
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: StatCard(
                label: context.l10n.wslcRunningContainersPage,
                value: count(running),
                icon: Icons.play_arrow_rounded,
                color: p.success,
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: StatCard(
                label: context.l10n.noticesRuntimeStopped,
                value: count(stopped),
                icon: Icons.pause_rounded,
                color: p.warning,
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: StatCard(
                label: context.l10n.kubeImages,
                value: count(c.images.length),
                icon: Icons.layers_rounded,
                color: p.accentEnd,
              ),
            ),
          ],
        ),
        const SizedBox(height: 18),
        Row(
          children: [
            Expanded(
              child: SearchField(
                controller: search,
                hint: context.l10n.wslcSearchByNameIDImageOrPort,
                onChanged: (_) => setState(() {}),
              ),
            ),
            const SizedBox(width: 12),
            Flexible(
              child: SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: FilterTabs<ContainerFilter>(
                  value: filter,
                  onChanged: (v) => setState(() => filter = v),
                  items: [
                    (ContainerFilter.all, context.l10n.notesAll, all.length),
                    (ContainerFilter.running, context.l10n.wslcRunningContainersPageOther, running),
                    (ContainerFilter.stopped, context.l10n.wslcStopped, stopped),
                  ],
                ),
              ),
            ),
            const SizedBox(width: 12),
            // Narrow windows keep the search usable with an icon-only button.
            MediaQuery.sizeOf(context).width >= 1180
                ? FilledButton.icon(
                    key: const ValueKey('wslc-new-container'),
                    onPressed: c.canMutate ? () => createContainer(context) : null,
                    icon: const Icon(Icons.add_rounded, size: 18),
                    label: Text(context.l10n.dbDesignNew),
                  )
                : IconButton.filled(
                    key: const ValueKey('wslc-new-container'),
                    tooltip: context.l10n.wslcNewContainer,
                    onPressed: c.canMutate ? () => createContainer(context) : null,
                    icon: const Icon(Icons.add_rounded),
                  ),
            const SizedBox(width: 8),
            IconButton.outlined(
              key: const ValueKey('wslc-open-compose'),
              tooltip: context.l10n.wslcDockerComposeStartFromYAML,
              onPressed: c.canMutate ? () => showComposeDialog(context, c) : null,
              icon: const Icon(Icons.dynamic_feed_rounded),
            ),
            PopupMenuButton<String>(
              tooltip: context.l10n.wslcBulkActions,
              icon: const Icon(Icons.more_horiz_rounded),
              enabled: c.canMutate,
              position: PopupMenuPosition.under,
              onSelected: (action) => runBulk(context, all, action),
              itemBuilder: (_) => [
                PopupMenuItem(
                  value: 'stop',
                  enabled: running > 0,
                  child: Text(context.l10n.wslcStopAllRunningContainers(running)),
                ),
                PopupMenuItem(
                  value: 'start',
                  enabled: stopped > 0,
                  child: Text(context.l10n.wslcStartAllStoppedContainers(stopped)),
                ),
              ],
            ),
          ],
        ),
        if (chosen.isNotEmpty) ...[
          const SizedBox(height: 10),
          _BulkBar(
            count: chosen.length,
            enabled: c.canMutate,
            counts: {
              for (final a in ['start', 'stop', 'restart', 'remove']) a: chosen.where((r) => eligible(r, a)).length,
            },
            onAction: (action) => runBulk(context, chosen, action),
            onClear: () => setState(selected.clear),
          ),
        ],
        const SizedBox(height: 14),
        Expanded(
          child: Panel(
            child: rows.isEmpty
                ? EmptyState(
                    icon: Icons.view_in_ar_rounded,
                    loading: c.ready && !c.loaded.contains(DeskSection.containers) && c.error == null,
                    title: !c.ready
                        ? context.l10n.wslcNotConnectedToWSLc
                        : all.isEmpty
                        ? context.l10n.wslcNoContainersYet
                        : context.l10n.notesNothingFound,
                    hint: !c.ready
                        ? context.l10n.wslcCheckTheConnectionInSettings
                        : all.isEmpty
                        ? context.l10n.wslcSelectNewToCreateYourFirstContainer
                        : context.l10n.wslcChangeTheSearchTermOrFilter,
                  )
                : LayoutBuilder(
                    builder: (context, constraints) {
                      // Usage needs room; narrow windows keep the essential columns.
                      final showStats = constraints.maxWidth >= 860;
                      return Column(
                        children: [
                          _HeaderRow(
                            showStats: showStats,
                            checked: allState,
                            onChecked: (v) => setState(() {
                              if (v == true) {
                                selected.addAll(visibleIds);
                              } else {
                                selected.removeAll(visibleIds);
                              }
                            }),
                          ),
                          Divider(height: 1, color: p.border),
                          Expanded(
                            child: ListView.builder(
                              key: const ValueKey('container-list'),
                              padding: const EdgeInsets.symmetric(vertical: 4),
                              itemCount: rows.length,
                              itemBuilder: (context, i) => _ContainerRow(
                                row: rows[i],
                                controller: c,
                                busy: c.busyIds.contains(rows[i].id),
                                showStats: showStats,
                                selected: selected.contains(rows[i].id),
                                onSelected: (v) =>
                                    setState(() => v ? selected.add(rows[i].id) : selected.remove(rows[i].id)),
                              ),
                            ),
                          ),
                        ],
                      );
                    },
                  ),
          ),
        ),
      ],
    );
  }
}

const _actionsWidth = 168.0;

class _BulkBar extends StatelessWidget {
  const _BulkBar({
    required this.count,
    required this.enabled,
    required this.counts,
    required this.onAction,
    required this.onClear,
  });
  final int count;
  final bool enabled;
  final Map<String, int> counts;
  final ValueChanged<String> onAction;
  final VoidCallback onClear;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    Widget button(String action, IconData icon, String label, Color color) {
      final n = counts[action] ?? 0;
      return TextButton.icon(
        style: TextButton.styleFrom(foregroundColor: color),
        onPressed: enabled && n > 0 ? () => onAction(action) : null,
        icon: Icon(icon, size: 18),
        label: Text('$label ($n)'),
      );
    }

    return Container(
      padding: const EdgeInsets.fromLTRB(14, 4, 6, 4),
      decoration: BoxDecoration(
        color: p.accent.withValues(alpha: .1),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: p.accent.withValues(alpha: .3)),
      ),
      child: Row(
        children: [
          Text(context.l10n.wslcSelected(count), style: const TextStyle(fontWeight: FontWeight.w700)),
          const SizedBox(width: 12),
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: Row(
                children: [
                  button('start', Icons.play_arrow_rounded, context.l10n.dockerActionStart, p.success),
                  button('stop', Icons.stop_rounded, context.l10n.dockerActionStop, p.danger),
                  button('restart', Icons.restart_alt_rounded, context.l10n.dockerActionRestart, p.text),
                  button('remove', Icons.delete_outline_rounded, context.l10n.commonDelete, p.danger),
                ],
              ),
            ),
          ),
          IconButton(
            tooltip: context.l10n.terminalClearSelection,
            onPressed: onClear,
            icon: const Icon(Icons.close_rounded),
          ),
        ],
      ),
    );
  }
}

class _HeaderRow extends StatelessWidget {
  const _HeaderRow({required this.showStats, required this.checked, required this.onChecked});
  final bool showStats;
  final bool? checked;
  final ValueChanged<bool?> onChecked;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    Widget label(String text, int flex) => Expanded(
      flex: flex,
      child: Text(
        text.toUpperCase(),
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: TextStyle(color: p.faint, fontSize: 10.5, fontWeight: FontWeight.w700, letterSpacing: .7),
      ),
    );
    return Padding(
      padding: const EdgeInsets.fromLTRB(6, 4, 16, 2),
      child: Row(
        children: [
          SizedBox(
            width: 34,
            child: Checkbox(tristate: true, value: checked, onChanged: (_) => onChecked(checked != true)),
          ),
          const SizedBox(width: 26),
          label(context.l10n.kubeContainer, 3),
          label(context.l10n.dockerImage, 3),
          label(context.l10n.syncScreenState, 2),
          label(context.l10n.kubePorts, 2),
          if (showStats) label('CPU · RAM', 2),
          const SizedBox(width: _actionsWidth),
        ],
      ),
    );
  }
}

class _ContainerRow extends StatelessWidget {
  const _ContainerRow({
    required this.row,
    required this.controller,
    required this.busy,
    required this.showStats,
    required this.selected,
    required this.onSelected,
  });
  final ContainerSummary row;
  final DeskController controller;
  final bool busy;
  final bool showStats;
  final bool selected;
  final ValueChanged<bool> onSelected;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final color = stateColor(p, row.state);
    final enabled = controller.canMutate && !busy;
    final ports = parsePorts(row.portsDisplay);
    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: () => controller.openContainer(row.id),
        hoverColor: p.raised.withValues(alpha: .7),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(6, 10, 16, 10),
          child: Row(
            children: [
              SizedBox(
                width: 34,
                child: Checkbox(value: selected, onChanged: (v) => onSelected(v ?? false)),
              ),
              const SizedBox(width: 6),
              StatusDot(color: color, glow: row.state == ContainerState.running),
              const SizedBox(width: 12),
              Expanded(
                flex: 3,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      row.name ?? context.l10n.automationResultMissing,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
                    ),
                    const SizedBox(height: 2),
                    CopyId(row.id),
                  ],
                ),
              ),
              Expanded(
                flex: 3,
                child: Padding(
                  padding: const EdgeInsets.only(right: 12),
                  child: Tooltip(
                    message: row.image ?? context.l10n.automationResultMissing,
                    child: Text(
                      row.image ?? context.l10n.automationResultMissing,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(color: p.muted, fontSize: 13),
                    ),
                  ),
                ),
              ),
              Expanded(
                flex: 2,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Pill(stateText(context.l10n, row.state, row.rawState), color: color),
                    if (row.statusDisplay != null) ...[
                      const SizedBox(height: 3),
                      Text(
                        row.statusDisplay!,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(color: p.faint, fontSize: 11),
                      ),
                    ],
                  ],
                ),
              ),
              Expanded(
                flex: 2,
                child: Padding(
                  padding: const EdgeInsets.only(right: 8),
                  child: ports.isNotEmpty
                      ? PortChips(ports: ports, addresses: controller.hostAddresses)
                      : Text(
                          row.portsDisplay == null ? context.l10n.automationResultMissing : '—',
                          style: TextStyle(color: p.faint, fontSize: 12),
                        ),
                ),
              ),
              if (showStats)
                Expanded(
                  flex: 2,
                  child: _UsageCell(controller: controller, row: row),
                ),
              SizedBox(
                width: _actionsWidth,
                child: busy
                    ? const Align(
                        alignment: Alignment.centerRight,
                        child: Padding(
                          padding: EdgeInsets.only(right: 12),
                          child: SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2)),
                        ),
                      )
                    : Row(
                        mainAxisAlignment: MainAxisAlignment.end,
                        children: [
                          IconButton(
                            tooltip: context.l10n.dockerActionStart,
                            color: p.success,
                            onPressed: enabled && canStart(row.state)
                                ? () => runContainerAction(context, controller, row, 'start')
                                : null,
                            icon: const Icon(Icons.play_arrow_rounded),
                          ),
                          IconButton(
                            tooltip: context.l10n.dockerActionStop,
                            color: p.danger,
                            onPressed: enabled && canStop(row.state)
                                ? () => runContainerAction(context, controller, row, 'stop')
                                : null,
                            icon: const Icon(Icons.stop_rounded),
                          ),
                          IconButton(
                            tooltip: context.l10n.dockerActionRestart,
                            onPressed: enabled && canStop(row.state)
                                ? () => runContainerAction(context, controller, row, 'restart')
                                : null,
                            icon: const Icon(Icons.restart_alt_rounded),
                          ),
                          ContainerMenu(row: row, controller: controller),
                        ],
                      ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Rebuilds alone when a new stats sample arrives.
class _UsageCell extends StatelessWidget {
  const _UsageCell({required this.controller, required this.row});
  final DeskController controller;
  final ContainerSummary row;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    if (row.state != ContainerState.running) {
      return Text('—', style: TextStyle(color: p.faint, fontSize: 12));
    }
    return ValueListenableBuilder(
      valueListenable: controller.stats,
      builder: (context, stats, _) {
        final s = stats[row.id];
        if (s == null) {
          return Text('…', style: TextStyle(color: p.faint, fontSize: 12));
        }
        final cpu = s.cpuPercent;
        final trend = controller.history[row.id]?.cpu.toList() ?? const [];
        return Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    cpu == null ? 'CPU —' : 'CPU ${cpu.toStringAsFixed(1)}%',
                    maxLines: 1,
                    style: TextStyle(
                      fontSize: 12,
                      fontFamily: monoFont,
                      fontFamilyFallback: monoFallback,
                      color: (cpu ?? 0) >= 80 ? p.warning : p.text,
                    ),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    memoryShort(s.memoryUsage),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 11.5,
                      color: p.muted,
                      fontFamily: monoFont,
                      fontFamilyFallback: monoFallback,
                    ),
                  ),
                ],
              ),
            ),
            if (trend.length > 1)
              SizedBox(
                width: 46,
                height: 24,
                child: Sparkline(values: trend, color: p.success),
              ),
            const SizedBox(width: 8),
          ],
        );
      },
    );
  }
}

/// "29.09MiB / 11.49GiB" → "29.09MiB".
String memoryShort(String? usage) => usage == null ? 'RAM —' : usage.split('/').first.trim();
