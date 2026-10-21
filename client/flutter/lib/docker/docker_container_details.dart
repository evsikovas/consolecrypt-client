import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_document_view.dart';
import 'package:consolecrypt/docker/docker_container_list.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_logs_view.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

enum DockerDetailsTab { overview, logs, json }

/// Container drawer: overview from `inspect`, live logs and the raw JSON.
class DockerContainerDetails extends ConsumerStatefulWidget {
  const DockerContainerDetails({
    required this.service,
    required this.session,
    required this.container,
    required this.onClose,
    required this.actions,
    this.initialTab = DockerDetailsTab.overview,
    this.tabRequest = 0,
    super.key,
  });
  final DockerService service;
  final DockerSessionInfo session;
  final DockerContainer container;
  final VoidCallback onClose;
  final DockerContainerActions actions;
  final DockerDetailsTab initialTab;

  /// Changes when the workspace asks for [initialTab] again (e.g. "Logs" on
  /// the selected row).
  final int tabRequest;
  @override
  ConsumerState<DockerContainerDetails> createState() => _DockerContainerDetailsState();
}

class _DockerContainerDetailsState extends ConsumerState<DockerContainerDetails> with SingleTickerProviderStateMixin {
  late final TabController _tabs = TabController(
    length: DockerDetailsTab.values.length,
    initialIndex: widget.initialTab.index,
    vsync: this,
  )..addListener(() => mounted ? setState(() {}) : null);
  DockerContainerDetail? _detail;
  bool _loading = true;
  Object? _error;
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  @override
  void didUpdateWidget(covariant DockerContainerDetails oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.tabRequest != widget.tabRequest) _tabs.animateTo(widget.initialTab.index);
    if (oldWidget.container.state != widget.container.state) unawaited(_load());
  }

  @override
  void dispose() {
    _generation++;
    _tabs.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    final generation = ++_generation;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final detail = await widget.service.inspect(widget.session.id, widget.container.id);
      if (mounted && generation == _generation) setState(() => _detail = detail);
    } catch (error) {
      if (mounted && generation == _generation) setState(() => _error = error);
    } finally {
      if (mounted && generation == _generation) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final container = widget.container;
    final running = container.state == DockerContainerState.running;
    final busy = widget.actions.busy(container);
    Widget action(String key, String label, IconData icon, VoidCallback? onPressed) => GlassButton(
      key: ValueKey('docker-details-$key'),
      label: label,
      icon: icon,
      size: GlassControlSize.sm,
      onPressed: busy ? null : onPressed,
    );
    final json = _detail == null ? '' : const JsonEncoder.withIndent('  ').convert(_detail!.inspect);
    return ContentSurface(
      key: const ValueKey('docker-container-details'),
      kind: ContentSurfaceKind.solid,
      padding: EdgeInsets.zero,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s12, GlassSpacing.s8, 0),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        container.name,
                        key: const ValueKey('docker-details-name'),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: tokens.typography.title3.copyWith(color: p.label),
                      ),
                      const SizedBox(height: 2),
                      Row(
                        children: [
                          GlassBadge(
                            label: dockerStateLabel(l, container.state),
                            tone: dockerStateTone(container.state),
                            dense: true,
                          ),
                          const SizedBox(width: 6),
                          Flexible(
                            child: Text(
                              container.shortId,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: tokens.typography.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel),
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
                ),
                GlassIconButton(
                  key: const ValueKey('docker-details-refresh'),
                  icon: Icons.refresh,
                  tooltip: l.dockerRefresh,
                  style: GlassIconButtonStyle.plain,
                  onPressed: _loading ? null : _load,
                ),
                GlassMenuButton<Object>(
                  key: const ValueKey('docker-details-menu'),
                  entries: dockerContainerMenu(l, container),
                  onSelected: (value) => runDockerMenuValue(widget.actions, container, value),
                  builder: (context, open) => GlassIconButton(
                    icon: Icons.more_horiz_rounded,
                    tooltip: MaterialLocalizations.of(context).moreButtonTooltip,
                    style: GlassIconButtonStyle.plain,
                    onPressed: open,
                  ),
                ),
                GlassIconButton(
                  key: const ValueKey('docker-details-close'),
                  icon: Icons.close_rounded,
                  tooltip: l.commonClose,
                  style: GlassIconButtonStyle.plain,
                  onPressed: widget.onClose,
                ),
              ],
            ),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s8, GlassSpacing.s16, 0),
            child: Wrap(
              spacing: GlassSpacing.s6,
              runSpacing: GlassSpacing.s6,
              children: [
                if (dockerActionAllowed(DockerAction.start, container.state))
                  action(
                    'start',
                    l.dockerActionStart,
                    Icons.play_arrow_rounded,
                    () => unawaited(widget.actions.run(container, DockerAction.start)),
                  )
                else
                  action(
                    'stop',
                    l.dockerActionStop,
                    Icons.stop_rounded,
                    dockerActionAllowed(DockerAction.stop, container.state)
                        ? () => unawaited(widget.actions.run(container, DockerAction.stop))
                        : null,
                  ),
                action(
                  'restart',
                  l.dockerActionRestart,
                  Icons.restart_alt_rounded,
                  dockerActionAllowed(DockerAction.restart, container.state)
                      ? () => unawaited(widget.actions.run(container, DockerAction.restart))
                      : null,
                ),
                action(
                  'terminal',
                  l.dockerTerminal,
                  Icons.terminal_rounded,
                  running ? () => unawaited(widget.actions.terminal(container)) : null,
                ),
                action(
                  'forward',
                  l.dockerForwardPort,
                  Icons.swap_horiz_rounded,
                  running ? () => unawaited(widget.actions.forward(container)) : null,
                ),
              ],
            ),
          ),
          if (_loading) const Padding(padding: EdgeInsets.only(top: 6), child: LinearProgressIndicator(minHeight: 2)),
          if (_error != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s16, GlassSpacing.s8, GlassSpacing.s16, 0),
              child: Text(
                dockerErrorText(l, _error!),
                key: const ValueKey('docker-details-error'),
                style: tokens.typography.callout.copyWith(color: p.danger),
              ),
            ),
          const SizedBox(height: GlassSpacing.s8),
          TabBar(
            controller: _tabs,
            isScrollable: true,
            tabAlignment: TabAlignment.start,
            labelColor: p.accent,
            unselectedLabelColor: tokens.secondaryLabel,
            labelStyle: tokens.typography.bodyEmph,
            unselectedLabelStyle: tokens.typography.body,
            indicatorColor: p.accent,
            indicatorSize: TabBarIndicatorSize.label,
            dividerColor: tokens.surfaces.separator,
            labelPadding: const EdgeInsets.symmetric(horizontal: 12),
            padding: const EdgeInsets.symmetric(horizontal: 4),
            tabs: [
              Tab(height: 36, key: const ValueKey('docker-tab-overview'), text: l.dockerOverview),
              Tab(height: 36, key: const ValueKey('docker-tab-logs'), text: l.dockerLogs),
              Tab(height: 36, key: const ValueKey('docker-tab-json'), text: l.dockerInspectJson),
            ],
          ),
          Expanded(
            child: TabBarView(
              controller: _tabs,
              children: [
                _DockerOverview(container: container, detail: _detail),
                Padding(
                  padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s8, GlassSpacing.s12, 0),
                  child: DockerLogsView(
                    service: widget.service,
                    sessionId: widget.session.id,
                    containerId: container.id,
                    active: _tabs.index == DockerDetailsTab.logs.index,
                  ),
                ),
                _detail == null
                    ? const SizedBox.shrink()
                    : Padding(
                        key: const ValueKey('docker-inspect-json'),
                        padding: const EdgeInsets.fromLTRB(GlassSpacing.s12, GlassSpacing.s4, GlassSpacing.s12, 0),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Align(
                              alignment: Alignment.centerLeft,
                              child: GlassButton.plain(
                                key: const ValueKey('docker-json-copy'),
                                label: l.dockerCopyJson,
                                icon: Icons.content_copy_rounded,
                                onPressed: () => copyPlainWithNotice(context, ref, json),
                              ),
                            ),
                            Expanded(
                              child: DbDocumentView(document: _detail!.inspect, rawText: json),
                            ),
                          ],
                        ),
                      ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _DockerOverview extends StatelessWidget {
  const _DockerOverview({required this.container, required this.detail});
  final DockerContainer container;
  final DockerContainerDetail? detail;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final inspect = detail?.inspect ?? const <String, dynamic>{};
    final config = inspect['Config'] as Map? ?? const {};
    final state = inspect['State'] as Map? ?? const {};
    String text(Object? value) => value is String || value is num || value is bool ? value.toString() : '';
    final created = DateTime.tryParse(text(inspect['Created']))?.toLocal() ?? container.createdAt?.toLocal();
    final command = [
      if (inspect['Path'] case final String path when path.isNotEmpty) path,
      for (final arg in inspect['Args'] as List? ?? const []) text(arg),
    ].join(' ');
    final mounts = (inspect['Mounts'] as List? ?? const []).whereType<Map<Object?, Object?>>().toList();
    final networkSettings = (inspect['NetworkSettings'] as Map?)?['Networks'];
    final networks = <String, String>{
      ...container.networks,
      if (networkSettings is Map)
        for (final entry in networkSettings.entries)
          entry.key.toString(): text((entry.value as Map?)?['IPAddress']).isEmpty
              ? (container.networks[entry.key.toString()] ?? '')
              : text((entry.value as Map?)?['IPAddress']),
    };
    final labels = <String, String>{
      for (final entry in (config['Labels'] as Map? ?? const {}).entries) entry.key.toString(): text(entry.value),
    };
    final env = [for (final value in config['Env'] as List? ?? const []) text(value)];
    final ports = formatDockerPorts(container.ports);
    final health = dockerHealth(l, container.health);
    final info = <(String, String)>[
      (l.dockerImage, text(config['Image']).isEmpty ? container.image : text(config['Image'])),
      (l.dockerStatus, container.status.isEmpty ? dockerStateLabel(l, container.state) : container.status),
      if (health != null) (l.dockerHealth, health.$1),
      if (created != null) (l.dockerCreated, formatDateTime(l, created)),
      if (command.isNotEmpty) (l.dockerCommand, command),
      if (inspect['RestartCount'] is num) (l.dockerRestartCount, text(inspect['RestartCount'])),
      if (text(state['StartedAt']).isNotEmpty && container.state == DockerContainerState.running)
        if (DateTime.tryParse(text(state['StartedAt'])) case final started?)
          (l.dockerStarted, formatDateTime(l, started.toLocal())),
      if (container.composeProject != null) (l.dockerComposeProject, container.composeProject!),
      if (container.composeService != null) (l.dockerComposeService, container.composeService!),
      (l.dockerContainerId, container.id),
    ];
    Widget mono(String value, {Color? color}) => SelectableText(
      value,
      style: tokens.typography.mono.copyWith(fontSize: 12, color: color ?? tokens.palette.label),
    );
    return ListView(
      key: const ValueKey('docker-overview'),
      padding: const EdgeInsets.all(GlassSpacing.s16),
      children: [
        _Section(
          title: l.dockerDetails,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [for (final (label, value) in info) _Fact(label: label, value: value)],
          ),
        ),
        if (ports.isNotEmpty)
          _Section(
            title: l.dockerPorts,
            child: Wrap(spacing: 8, runSpacing: 4, children: [for (final port in ports) mono(port)]),
          ),
        if (networks.isNotEmpty)
          _Section(
            title: l.dockerNetworks,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final entry in networks.entries)
                  _Fact(label: entry.key, value: entry.value.isEmpty ? '—' : entry.value, mono: true),
              ],
            ),
          ),
        if (mounts.isNotEmpty)
          _Section(
            title: l.dockerMounts,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final mount in mounts)
                  Padding(padding: const EdgeInsets.symmetric(vertical: 3), child: mono(_mountText(mount, text))),
              ],
            ),
          ),
        if (labels.isNotEmpty)
          _Section(
            title: l.dockerLabels,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [for (final entry in labels.entries) _Fact(label: entry.key, value: entry.value, mono: true)],
            ),
          ),
        if (env.isNotEmpty)
          _Section(
            title: l.dockerEnvironment,
            child: Column(
              key: const ValueKey('docker-env'),
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                for (final value in env) Padding(padding: const EdgeInsets.symmetric(vertical: 2), child: mono(value)),
              ],
            ),
          ),
      ],
    );
  }
}

/// `source → destination (type, ro)` of one mount.
String _mountText(Map<Object?, Object?> mount, String Function(Object?) text) {
  final source = text(mount['Source']).isEmpty ? text(mount['Name']) : text(mount['Source']);
  final type = text(mount['Type']);
  final mode = mount['RW'] == false ? ', ro' : ''; // l10n-ignore: mount mode token
  return '$source → ${text(mount['Destination'])}${type.isEmpty ? '' : ' ($type$mode)'}';
}

class _Section extends StatelessWidget {
  const _Section({required this.title, required this.child});
  final String title;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(title, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
          const SizedBox(height: GlassSpacing.s6),
          child,
        ],
      ),
    );
  }
}

class _Fact extends StatelessWidget {
  const _Fact({required this.label, required this.value, this.mono = false});
  final String label, value;
  final bool mono;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final style = (mono ? tokens.typography.mono.copyWith(fontSize: 12) : tokens.typography.callout).copyWith(
      color: tokens.palette.label,
    );
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final labelText = Text(label, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel));
          if (constraints.maxWidth < 360) {
            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                labelText,
                SelectableText(value, style: style),
              ],
            );
          }
          return Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(width: 140, child: labelText),
              const SizedBox(width: 8),
              Expanded(child: SelectableText(value, style: style)),
            ],
          );
        },
      ),
    );
  }
}
