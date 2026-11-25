import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/monitoring_preferences.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/host_metrics_service.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/monitoring/monitoring_card.dart';
import 'package:consolecrypt/monitoring/monitoring_inventory.dart';
import 'package:consolecrypt/monitoring/monitoring_picker.dart';
import 'package:consolecrypt/monitoring/monitoring_poller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final dashboardMetricsProvider = Provider<DashboardMetricsReader>(
  (ref) => ref.watch(terminalServiceProvider) as DashboardMetricsReader,
);

class MonitoringScreen extends ConsumerStatefulWidget {
  const MonitoringScreen({super.key});
  @override
  ConsumerState<MonitoringScreen> createState() => _MonitoringScreenState();
}

class _MonitoringScreenState extends ConsumerState<MonitoringScreen> with WidgetsBindingObserver {
  bool get foreground {
    final lifecycle = WidgetsBinding.instance.lifecycleState;
    return lifecycle == null || lifecycle == AppLifecycleState.resumed || lifecycle == AppLifecycleState.inactive;
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) => setState(() {});

  Future<void> save(String profile, MonitoringPreferences Function(MonitoringPreferences) change) async {
    if (!mounted ||
        ref.read(activeProfileProvider)?.id.value != profile ||
        ref.read(appStageProvider) != AppStage.unlocked) {
      return;
    }
    final service = ref.read(settingsServiceProvider), current = service.currentLocal;
    final selection = change(current.monitoringByProfile[profile] ?? const MonitoringPreferences());
    try {
      await service.updateLocal(
        current.copyWith(monitoringByProfile: {...current.monitoringByProfile, profile: selection}),
      );
    } catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(errorMessage(context.l10n, error))));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final profile = ref.watch(activeProfileProvider)?.id.value;
    if (!foreground ||
        !TickerMode.valuesOf(context).enabled ||
        ref.watch(appStageProvider) != AppStage.unlocked ||
        profile == null) {
      return const SizedBox.shrink();
    }
    final hosts = ref.watch(hostsProvider),
        groups = ref.watch(groupsProvider),
        local = ref.watch(localSettingsProvider);
    final l = context.l10n;
    final failure = hosts.error ?? groups.error ?? local.error;
    if (failure != null) return PageScaffold(title: l.hostMetricsTitle, body: Text(errorMessage(l, failure)));
    if (!hosts.hasValue || !groups.hasValue || !local.hasValue) {
      return PageScaffold(
        title: l.hostMetricsTitle,
        body: const Center(child: CircularProgressIndicator()),
      );
    }
    final selection = local.requireValue.monitoringByProfile[profile] ?? const MonitoringPreferences();
    final inventory = MonitoringInventory(hosts.requireValue, groups.requireValue);
    return _Dashboard(
      key: ValueKey(profile),
      inventory: inventory,
      selection: selection,
      reader: ref.watch(dashboardMetricsProvider),
      onInterval: (v) => save(profile, (p) => p.copyWith(seconds: v)),
      onMinimal: (v) => save(profile, (p) => p.copyWith(minimal: v)),
      onPercent: (v) => save(profile, (p) => p.copyWith(percent: v)),
      choose: () async {
        final changed = await showAppDialog<MonitoringPreferences>(
          context,
          builder: (_) => MonitoringPicker(inventory: inventory, selection: selection),
        );
        if (changed != null) await save(profile, (p) => p.copyWith(hosts: changed.hosts, groups: changed.groups));
      },
    );
  }
}

class _Dashboard extends StatefulWidget {
  const _Dashboard({
    super.key,
    required this.inventory,
    required this.selection,
    required this.reader,
    required this.choose,
    required this.onInterval,
    required this.onPercent,
    required this.onMinimal,
  });
  final MonitoringInventory inventory;
  final MonitoringPreferences selection;
  final DashboardMetricsReader reader;
  final VoidCallback choose;
  final ValueChanged<int> onInterval;
  final ValueChanged<bool> onPercent, onMinimal;
  @override
  State<_Dashboard> createState() => _DashboardState();
}

class _DashboardState extends State<_Dashboard> {
  late final poller = MonitoringPoller(widget.reader);
  final collapsed = <ObjectId?>{};
  late List<Host> hosts;
  void configure() {
    hosts = widget.inventory.resolve(widget.selection);
    poller.configure(hosts, widget.selection.seconds);
  }

  @override
  void initState() {
    super.initState();
    configure();
  }

  @override
  void didUpdateWidget(_Dashboard old) {
    super.didUpdateWidget(old);
    configure();
  }

  @override
  void dispose() {
    poller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, prefs = widget.selection, tokens = GlassTokens.of(context);
    final sections = <ObjectId?, List<Host>>{};
    for (final host in hosts) {
      (sections[host.groupId] ??= []).add(host);
    }
    String title(ObjectId? id) {
      final group = widget.inventory.groups[id];
      return group == null ? l.inventoryUngrouped : widget.inventory.path(group);
    }

    final ordered = sections.keys.toList()..sort((a, b) => title(a).compareTo(title(b)));
    return ListenableBuilder(
      listenable: poller,
      builder: (context, _) => PageScaffold(
        title: l.hostMetricsTitle,
        actions: [
          Tooltip(
            message: l.dbAutoRefresh,
            child: DropdownButton<int>(
              value: prefs.seconds,
              items: [
                for (final sec in MonitoringPreferences.intervals)
                  DropdownMenuItem(value: sec, child: Text(sec == 0 ? l.monitorManual : l.monitorInterval(sec))),
              ],
              onChanged: (v) {
                if (v != null) widget.onInterval(v);
              },
            ),
          ),
          if (!prefs.minimal)
            FilterChip(
              label: const Text('%'),
              tooltip: l.monitorPercent,
              selected: prefs.percent,
              onSelected: widget.onPercent,
            ),
          GlassIconButton(
            key: const ValueKey('monitor-minimal'),
            icon: Icons.density_small_rounded,
            selected: prefs.minimal,
            tooltip: prefs.minimal ? l.monitorDetailed : l.monitorMinimal,
            onPressed: () => widget.onMinimal(!prefs.minimal),
          ),
          GlassIconButton(
            icon: Icons.refresh_rounded,
            tooltip: l.dockerRefresh,
            onPressed: hosts.isEmpty || poller.busy ? null : poller.refresh,
          ),
          GlassButton(
            key: const ValueKey('monitor-hosts'),
            label: l.navHosts,
            icon: Icons.add_rounded,
            style: GlassButtonStyle.prominent,
            onPressed: widget.choose,
          ),
        ],
        body: hosts.isEmpty
            ? EmptyState(icon: Icons.monitor_heart_outlined, title: l.monitorEmpty, message: l.monitorEmptyHint)
            : ListView(
                children: [
                  for (final group in ordered) ...[
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      title: Wrap(
                        spacing: 10,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        children: [
                          Text(title(group), style: tokens.typography.bodyEmph),
                          Text(
                            '${sections[group]!.length}',
                            style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                          ),
                        ],
                      ),
                      leading: Icon(
                        collapsed.contains(group) ? Icons.chevron_right_rounded : Icons.expand_more_rounded,
                      ),
                      onTap: () => setState(() {
                        if (!collapsed.remove(group)) collapsed.add(group);
                      }),
                    ),
                    if (!collapsed.contains(group))
                      LayoutBuilder(
                        builder: (context, box) {
                          const spacing = 8.0;
                          final minWidth =
                              MediaQuery.textScalerOf(context).scale(14) / 14 * (prefs.minimal ? 308 : 248);
                          final columns = ((box.maxWidth + spacing) / (minWidth + spacing)).floor().clamp(1, 100);
                          final width = (box.maxWidth - spacing * (columns - 1)) / columns;
                          return Wrap(
                            spacing: spacing,
                            runSpacing: spacing,
                            children: [
                              for (final host in sections[group]!)
                                SizedBox(
                                  width: width,
                                  child: MonitoringCard(
                                    key: ValueKey(host.id),
                                    host: host,
                                    reading: poller.readings[host.id],
                                    percent: prefs.percent,
                                    minimal: prefs.minimal,
                                  ),
                                ),
                            ],
                          );
                        },
                      ),
                    const SizedBox(height: 12),
                  ],
                ],
              ),
      ),
    );
  }
}
