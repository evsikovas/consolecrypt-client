import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/containers/container_actions.dart';
import 'package:consolecrypt/wslc/features/containers/env_panel.dart';
import 'package:consolecrypt/wslc/features/containers/files_panel.dart';
import 'package:consolecrypt/wslc/features/containers/run_command_dialog.dart';
import 'package:consolecrypt/wslc/features/containers/terminal_panel.dart';
import 'package:consolecrypt/wslc/features/inventory/resource_dialogs.dart';
import 'package:consolecrypt/wslc/features/logs/log_panel.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/ports.dart';
import 'package:consolecrypt/wslc/shared/usage.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

const _overview = 0, _logs = 1, _terminal = 2, _files = 3, _mounts = 4, _env = 5, _inspect = 6;

class ContainerDetailsPage extends StatefulWidget {
  const ContainerDetailsPage({super.key, required this.controller, required this.containerId});
  final DeskController controller;
  final String containerId;
  @override
  State<ContainerDetailsPage> createState() => _ContainerDetailsPageState();
}

class _ContainerDetailsPageState extends State<ContainerDetailsPage> {
  late final api = WslcApiScope.of(context);

  ContainerDetails? details;
  String? error;
  int tab = _overview;
  int _request = 0;
  ContainerSummary? _seen;

  /// Directory the Files tab opens next; bumping [_filesKey] restarts the browser there.
  String filesPath = '/';
  int _filesKey = 0;

  ContainerSummary? get summary {
    for (final r in widget.controller.containers) {
      if (r.id == widget.containerId) return r;
    }
    return null;
  }

  @override
  void initState() {
    super.initState();
    _seen = summary;
    load();
  }

  @override
  void didUpdateWidget(covariant ContainerDetailsPage old) {
    super.didUpdateWidget(old);
    // Re-read Inspect only when the polled list reports a different state for this container.
    final now = summary;
    if (now != null && (now.state != _seen?.state || now.statusDisplay != _seen?.statusDisplay)) {
      load();
    }
    _seen = now;
  }

  Future<void> load() async {
    final request = ++_request;
    try {
      final result = await api.inspectContainer(containerId: widget.containerId, revealEnvironment: false);
      if (mounted && request == _request) {
        setState(() {
          details = result;
          error = null;
        });
      }
    } catch (e) {
      if (mounted && request == _request) {
        setState(() => error = errorText(e));
      }
    }
  }

  Future<void> act(ContainerSummary row, String action) async {
    final result = await runContainerAction(context, widget.controller, row, action, known: details);
    final current = result?.current;
    if (!mounted || result == null) return;
    // The operation already re-read Inspect after the command; reuse it.
    if (current != null) {
      _request++;
      setState(() => details = current);
    } else {
      await load();
    }
  }

  void browse(String path) => setState(() {
    filesPath = path;
    _filesKey++;
    tab = _files;
  });

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    final p = DeskPalette.of(context);
    final row = summary;
    final d = details;
    final state = row?.state ?? d?.state ?? ContainerState.unknown;
    final running = state == ContainerState.running;
    final color = stateColor(p, state);
    final busy = c.busyIds.contains(widget.containerId);
    final enabled = row != null && c.canMutate && !busy;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            IconButton(
              tooltip: context.l10n.wslcBackToList,
              onPressed: c.closeContainer,
              icon: const Icon(Icons.arrow_back_rounded),
            ),
            const SizedBox(width: 6),
            GradientIcon(Icons.view_in_ar_rounded, color: color, size: 46),
            const SizedBox(width: 14),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Flexible(
                        child: Text(
                          row?.name ?? d?.name ?? shortId(widget.containerId),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(fontSize: 21, fontWeight: FontWeight.w700, letterSpacing: -.4),
                        ),
                      ),
                      const SizedBox(width: 10),
                      Pill(stateText(context.l10n, state, row?.rawState ?? d?.rawState), color: color),
                    ],
                  ),
                  const SizedBox(height: 4),
                  Row(
                    children: [
                      CopyId(widget.containerId),
                      if ((row?.image ?? d?.imageName) != null) ...[
                        Text('  ·  ', style: TextStyle(color: p.faint)),
                        Flexible(
                          child: Text(
                            row?.image ?? d!.imageName!,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(color: p.muted, fontSize: 12.5),
                          ),
                        ),
                      ],
                    ],
                  ),
                ],
              ),
            ),
            const SizedBox(width: 12),
            Container(
              padding: const EdgeInsets.all(3),
              decoration: BoxDecoration(
                color: p.surface,
                borderRadius: BorderRadius.circular(12),
                border: Border.all(color: p.border),
              ),
              child: busy
                  ? const Padding(
                      padding: EdgeInsets.all(11),
                      child: SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2)),
                    )
                  : Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        IconButton(
                          tooltip: context.l10n.dockerActionStart,
                          color: p.success,
                          onPressed: enabled && canStart(state) ? () => act(row, 'start') : null,
                          icon: const Icon(Icons.play_arrow_rounded),
                        ),
                        IconButton(
                          tooltip: context.l10n.dockerActionStop,
                          color: p.danger,
                          onPressed: enabled && canStop(state) ? () => act(row, 'stop') : null,
                          icon: const Icon(Icons.stop_rounded),
                        ),
                        IconButton(
                          tooltip: context.l10n.dockerActionRestart,
                          onPressed: enabled && canStop(state) ? () => act(row, 'restart') : null,
                          icon: const Icon(Icons.restart_alt_rounded),
                        ),
                        Container(
                          width: 1,
                          height: 22,
                          margin: const EdgeInsets.symmetric(horizontal: 4),
                          color: p.border,
                        ),
                        IconButton(
                          tooltip: context.l10n.wslcStartupCommandContainerDetailsPage,
                          onPressed: () => showRunCommand(context, c, widget.containerId),
                          icon: const Icon(Icons.code_rounded),
                        ),
                        IconButton(
                          tooltip: context.l10n.terminalLocalOpen,
                          onPressed: running ? () => openContainerTerminal(context, widget.containerId) : null,
                          icon: const Icon(Icons.terminal_rounded),
                        ),
                        IconButton(
                          tooltip: context.l10n.wslcRemoveContainer,
                          color: p.danger,
                          onPressed: enabled ? () => removeContainerFlow(context, c, row) : null,
                          icon: const Icon(Icons.delete_outline_rounded),
                        ),
                      ],
                    ),
            ),
          ],
        ),
        const SizedBox(height: 18),
        Align(
          alignment: Alignment.centerLeft,
          child: FilterTabs<int>(
            value: tab,
            onChanged: (v) => setState(() => tab = v),
            items: [
              (_overview, context.l10n.kubeOverview, null),
              (_logs, context.l10n.kubeLogs, null),
              (_terminal, context.l10n.navTerminal, null),
              (_files, context.l10n.dockerComposeFiles, null),
              (_mounts, context.l10n.snippetDbMongodbConnections, d?.mounts?.length),
              (_env, context.l10n.kubeExecEnvironment, null),
              (_inspect, 'Inspect', null),
            ],
          ),
        ),
        const SizedBox(height: 14),
        Expanded(
          child: row == null && c.loaded.contains(DeskSection.containers)
              ? Panel(
                  child: EmptyState(
                    icon: Icons.search_off_rounded,
                    title: context.l10n.wslcContainerNoLongerFound,
                    hint: context.l10n.wslcTheRuntimeMayHaveRemovedItWhenIt,
                  ),
                )
              : tab == _logs
              ? LogPanel(key: ValueKey(widget.containerId), containerId: widget.containerId, active: true)
              : tab == _terminal
              ? TerminalPanel(
                  key: ValueKey('terminal-${widget.containerId}'),
                  containerId: widget.containerId,
                  running: running,
                )
              : tab == _env
              ? EnvPanel(key: ValueKey('env-${widget.containerId}'), containerId: widget.containerId)
              : tab == _files
              ? FilesPanel(
                  key: ValueKey('files-$_filesKey'),
                  containerId: widget.containerId,
                  running: running,
                  initialPath: filesPath,
                )
              : d == null
              ? Panel(
                  child: EmptyState(
                    icon: Icons.error_outline_rounded,
                    loading: error == null,
                    title: error == null
                        ? context.l10n.wslcLoadingInspectData
                        : context.l10n.wslcCouldNotLoadInspectData,
                    hint: error,
                  ),
                )
              : switch (tab) {
                  _overview => _Overview(details: d, summary: row, controller: c),
                  _mounts => _Mounts(
                    mounts: d.mounts,
                    containerId: widget.containerId,
                    running: running,
                    onBrowse: browse,
                  ),
                  _ => CodeView(d.inspectJson),
                },
        ),
      ],
    );
  }
}

class _SectionTitle extends StatelessWidget {
  const _SectionTitle(this.text);
  final String text;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(2, 18, 0, 10),
    child: Text(text, style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w700)),
  );
}

class _Overview extends StatelessWidget {
  const _Overview({required this.details, required this.summary, required this.controller});
  final ContainerDetails details;
  final ContainerSummary? summary;
  final DeskController controller;

  @override
  Widget build(BuildContext context) {
    final d = details;
    final p = DeskPalette.of(context);
    final running = d.state == ContainerState.running;
    final ports = d.ports.isNotEmpty
        ? d.ports.map(PublishedPort.fromBinding).toList()
        : parsePorts(summary?.portsDisplay);
    String or(Object? v) => v?.toString() ?? context.l10n.automationResultMissing;
    return ListView(
      padding: const EdgeInsets.only(bottom: 16),
      children: [
        _SectionTitle(context.l10n.syncScreenState),
        TileGrid(
          minTileWidth: 180,
          children: [
            InfoTile(
              label: context.l10n.syncScreenState,
              value: summary?.statusDisplay ?? stateText(context.l10n, d.state, d.rawState),
            ),
            InfoTile(label: context.l10n.wslcExitCode, value: or(d.exitCode)),
            InfoTile(label: context.l10n.dockerHealth, value: or(d.healthStatus)),
            InfoTile(
              label: context.l10n.dockerCreated,
              value: formatTimestamp(d.createdAt, l10n: context.l10n),
            ),
            InfoTile(
              label: context.l10n.dockerStarted,
              value: formatTimestamp(d.startedAt, l10n: context.l10n),
            ),
            if (!running)
              InfoTile(
                label: context.l10n.wslcFinished,
                value: formatTimestamp(d.finishedAt, l10n: context.l10n),
              ),
            InfoTile(
              label: context.l10n.wslcRemoveAfterStoppingRm,
              value: d.autoRemove == null
                  ? context.l10n.wslcUnknownContainerDetailsPage
                  : d.autoRemove!
                  ? context.l10n.wslcEnabled
                  : context.l10n.wslcDisabled,
            ),
            InfoTile(
              label: context.l10n.wslcStopTimeout,
              value: d.stopTimeoutSeconds == null
                  ? context.l10n.dbDesignDefault
                  : context.l10n.wslcSContainerDetailsPage(d.stopTimeoutSeconds!),
            ),
          ],
        ),
        if (running) ...[
          _SectionTitle(context.l10n.kubeResources),
          ValueListenableBuilder(
            valueListenable: controller.stats,
            builder: (context, stats, _) {
              final s = stats[d.id];
              final h = controller.history[d.id];
              String v(Object? x) => x?.toString() ?? '…';
              final rx = h?.rx.isNotEmpty == true ? h!.rx.last : null;
              final tx = h?.tx.isNotEmpty == true ? h!.tx.last : null;
              return Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  TileGrid(
                    minTileWidth: 300,
                    children: [
                      UsageChart(
                        title: context.l10n.kubeOverviewCpu,
                        value: s?.cpuPercent == null ? '…' : '${s!.cpuPercent!.toStringAsFixed(2)}%',
                        values: h?.cpu.toList() ?? const [],
                        color: p.success,
                      ),
                      UsageChart(
                        title: context.l10n.kubeOverviewMemory,
                        value: s?.memoryUsage == null
                            ? '…'
                            : '${s!.memoryUsage}'
                                  '${s.memoryPercent == null ? '' : ' · ${s.memoryPercent}%'}',
                        values: h?.memory.toList() ?? const [],
                        color: p.accent,
                      ),
                      UsageChart(
                        title: context.l10n.hostMetricsNetwork,
                        value: rx == null || tx == null
                            ? v(s?.netIo)
                            : '↓ ${formatBytes(rx, l10n: context.l10n, perSecond: true)}  ↑ ${formatBytes(tx, l10n: context.l10n, perSecond: true)}',
                        values: h?.rx.toList() ?? const [],
                        second: h?.tx.toList(),
                        color: p.accentEnd,
                        secondColor: p.warning,
                        legend: context.l10n.wslcReceiveInBlueSendInYellow,
                      ),
                    ],
                  ),
                  const SizedBox(height: 10),
                  TileGrid(
                    minTileWidth: 300,
                    children: [
                      InfoTile(label: context.l10n.wslcDiskReadWrite, value: v(s?.blockIo)),
                      InfoTile(label: context.l10n.wslcProcesses, value: v(s?.pids)),
                    ],
                  ),
                ],
              );
            },
          ),
        ],
        _SectionTitle(context.l10n.kubePorts),
        if (ports.isEmpty)
          Text(context.l10n.wslcNoPublishedPorts, style: TextStyle(color: p.muted))
        else
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [for (final port in ports) PortChip(port: port, addresses: controller.hostAddresses)],
          ),
        if (ports.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(context.l10n.wslcSelectAPortToOpenItInA, style: TextStyle(color: p.faint, fontSize: 12)),
          ),
        _SectionTitle(context.l10n.wslcStartup),
        TileGrid(
          minTileWidth: 380,
          children: [
            InfoTile(label: context.l10n.dockerImage, value: or(d.imageName)),
            InfoTile(label: context.l10n.wslcImageID, value: or(d.imageId), mono: true),
          ],
        ),
        const SizedBox(height: 10),
        InfoTile(label: context.l10n.sharingCommand, value: or(d.command), mono: true),
        Row(
          children: [
            Expanded(child: _SectionTitle(context.l10n.hostMetricsNetwork)),
            TextButton.icon(
              onPressed: controller.canMutate ? () => connectToNetwork(context, controller, containerId: d.id) : null,
              icon: const Icon(Icons.add_link_rounded, size: 18),
              label: Text(context.l10n.wslcConnectToNetwork),
            ),
          ],
        ),
        if (d.networks.isEmpty)
          Text(context.l10n.wslcTheRuntimeReturnedNoNetworkInformation, style: TextStyle(color: p.muted))
        else
          TileGrid(
            minTileWidth: 240,
            children: [
              for (final n in d.networks)
                InfoTile(
                  label: context.l10n.wslcNetwork(n.name),
                  trailing: IconButton(
                    tooltip: context.l10n.wslcDisconnectFromNetwork,
                    onPressed: controller.canMutate
                        ? () => disconnectFromNetwork(context, controller, n.name, d.id)
                        : null,
                    icon: const Icon(Icons.link_off_rounded, size: 18),
                  ),
                  value: [
                    'IP ${n.ipAddress ?? '—'}',
                    if (n.gateway != null) context.l10n.wslcGateway(n.gateway!),
                    if (n.macAddress != null) 'MAC ${n.macAddress}',
                  ].join('\n'),
                ),
            ],
          ),
      ],
    );
  }
}

class _Mounts extends StatelessWidget {
  const _Mounts({required this.mounts, required this.containerId, required this.running, required this.onBrowse});
  final List<MountInfo>? mounts;
  final String containerId;
  final bool running;
  final ValueChanged<String> onBrowse;

  Future<void> openFolder(BuildContext context, String path) async {
    final notify = WslcApiScope.notifierOf(context);
    try {
      await WslcApiScope.of(context).openHostFolder(path: path);
    } catch (e) {
      notify(errorText(e), error: true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final items = mounts;
    if (items == null || items.isEmpty) {
      return Panel(
        child: EmptyState(
          icon: Icons.folder_off_outlined,
          title: items == null
              ? context.l10n.wslcTheRuntimeReturnedNoMountInformation
              : context.l10n.wslcNoMountedVolumesOrDirectories,
        ),
      );
    }
    return ListView.separated(
      itemCount: items.length,
      separatorBuilder: (_, _) => const SizedBox(height: 10),
      itemBuilder: (context, i) {
        final m = items[i];
        final rw = m.readWrite;
        final bind = m.kind == 'bind';
        final source = bind ? m.source : (m.name ?? m.source);
        final destination = m.destination;
        return Panel(
          padding: const EdgeInsets.fromLTRB(16, 14, 10, 14),
          child: Row(
            children: [
              GradientIcon(
                bind ? Icons.folder_rounded : Icons.storage_rounded,
                color: bind ? p.warning : p.accentEnd,
                size: 38,
              ),
              const SizedBox(width: 14),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Flexible(
                          child: Text(
                            destination ?? context.l10n.automationResultMissing,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: const TextStyle(
                              fontWeight: FontWeight.w600,
                              fontFamily: monoFont,
                              fontFamilyFallback: monoFallback,
                            ),
                          ),
                        ),
                        const SizedBox(width: 8),
                        Pill(
                          bind ? context.l10n.wslcWindowsFolder : (m.kind ?? context.l10n.wslcUnknownType),
                          color: bind ? p.warning : p.accent,
                        ),
                        const SizedBox(width: 6),
                        Pill(
                          rw == null
                              ? context.l10n.automationResultMissing
                              : rw
                              ? 'RW'
                              : 'RO',
                          color: rw == true ? p.success : p.muted,
                        ),
                      ],
                    ),
                    const SizedBox(height: 4),
                    WslcSelectableText(
                      '← ${source ?? context.l10n.automationResultMissing}',
                      maxLines: 2,
                      minLines: 1,
                      style: TextStyle(
                        color: p.muted,
                        fontSize: 12,
                        fontFamily: monoFont,
                        fontFamilyFallback: monoFallback,
                      ),
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 8),
              if (bind && source != null)
                OutlinedButton.icon(
                  onPressed: () => openFolder(context, source),
                  icon: const Icon(Icons.folder_open_rounded, size: 18),
                  label: Text(context.l10n.wslcShowInFileExplorer),
                ),
              if (!bind && destination != null)
                Tooltip(
                  message: context.l10n.wslcThisVolumeIsStoredInsideTheWSLcVM,
                  child: OutlinedButton.icon(
                    onPressed: running ? () => onBrowse(destination) : null,
                    icon: const Icon(Icons.folder_copy_outlined, size: 18),
                    label: Text(context.l10n.wslcShowFiles),
                  ),
                ),
              PopupMenuButton<VoidCallback>(
                tooltip: context.l10n.glassMoreActions,
                icon: const Icon(Icons.more_vert_rounded),
                position: PopupMenuPosition.under,
                onSelected: (action) => action(),
                itemBuilder: (_) => [
                  if (source != null)
                    PopupMenuItem(
                      value: () => copyText(context, source),
                      child: Text(bind ? context.l10n.wslcCopyWindowsPath : context.l10n.wslcCopyVolumeName),
                    ),
                  if (destination != null)
                    PopupMenuItem(
                      value: () => copyText(context, destination),
                      child: Text(context.l10n.wslcCopyPathInsideContainer),
                    ),
                  if (destination != null)
                    PopupMenuItem(
                      enabled: running,
                      value: () => onBrowse(destination),
                      child: Text(context.l10n.wslcShowFilesInContainer),
                    ),
                  if (destination != null)
                    PopupMenuItem(
                      enabled: running,
                      value: () => openContainerTerminal(context, containerId, workdir: destination),
                      child: Text(context.l10n.wslcOpenTerminalHere),
                    ),
                ],
              ),
            ],
          ),
        );
      },
    );
  }
}
