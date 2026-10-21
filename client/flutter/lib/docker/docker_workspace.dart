import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_container_details.dart';
import 'package:consolecrypt/docker/docker_container_list.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_forwards.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/docker/docker_terminal.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Content width from which details and forwards sit beside the list.
const _sideBySide = 860.0;

/// Content width from which details and forwards fit next to each other.
const _bothPanels = 1240.0;

/// One connected engine: header, grouped containers, the container drawer
/// and the Docker forwards panel. Refreshes every 5 s while visible and the
/// app is in the foreground.
class DockerWorkspace extends ConsumerStatefulWidget {
  const DockerWorkspace({
    required this.service,
    required this.session,
    required this.onDisconnect,
    this.host,
    this.refreshInterval = const Duration(seconds: 5),
    super.key,
  });
  final DockerService service;
  final DockerSessionInfo session;
  final DockerHost? host;
  final VoidCallback onDisconnect;
  final Duration refreshInterval;
  @override
  ConsumerState<DockerWorkspace> createState() => _DockerWorkspaceState();
}

class _DockerWorkspaceState extends ConsumerState<DockerWorkspace> with WidgetsBindingObserver {
  final _containers = ValueNotifier<List<DockerContainer>>(const []);
  final _collapsed = <String>{};
  final _busy = <String>{};
  Timer? _timer;
  bool _foreground = true, _loading = false, _loaded = false, _forwardsOpen = false;
  DockerStateFilter _filter = DockerStateFilter.all;
  Object? _error;
  String _query = '';
  String? _selected;
  DockerDetailsTab _tab = DockerDetailsTab.overview;
  int _tabRequest = 0, _generation = 0;

  String get _hostName => widget.session.hostName.isNotEmpty ? widget.session.hostName : (widget.host?.name ?? '');

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    unawaited(_refresh());
    _timer = Timer.periodic(widget.refreshInterval, (_) {
      if (mounted && _foreground && !_loading && TickerMode.valuesOf(context).enabled) unawaited(_refresh());
    });
  }

  @override
  void dispose() {
    _generation++;
    _timer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    _containers.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    final foreground = state == AppLifecycleState.resumed;
    if (foreground && !_foreground && mounted) unawaited(_refresh());
    _foreground = foreground;
  }

  Future<void> _refresh() async {
    final generation = ++_generation;
    setState(() => _loading = true);
    try {
      final containers = await widget.service.containers(widget.session.id);
      if (!mounted || generation != _generation) return;
      _containers.value = containers;
      setState(() {
        _error = null;
        _loaded = true;
        if (_selected != null && !containers.any((c) => c.id == _selected)) _selected = null;
      });
    } catch (error) {
      if (mounted && generation == _generation) setState(() => _error = error);
    } finally {
      if (mounted && generation == _generation) setState(() => _loading = false);
    }
  }

  /// From the last layout: the workspace is narrower than the window when
  /// the sidebar is shown.
  bool _wide = false, _both = false;

  void _open(DockerContainer container, [DockerDetailsTab tab = DockerDetailsTab.overview]) {
    if (!_wide) {
      unawaited(
        Navigator.of(context).push(
          MaterialPageRoute<void>(
            builder: (_) => _PushedPage(
              child: _DetailsHost(
                containers: _containers,
                id: container.id,
                builder: (current, close) => DockerContainerDetails(
                  key: ValueKey(('docker-details', current.id)),
                  service: widget.service,
                  session: widget.session,
                  container: current,
                  initialTab: tab,
                  onClose: close,
                  actions: _actions,
                ),
              ),
            ),
          ),
        ),
      );
      return;
    }
    setState(() {
      _selected = container.id;
      _tab = tab;
      _tabRequest++;
      if (!_both) _forwardsOpen = false;
    });
  }

  void _toggleForwards() {
    if (!_wide) {
      unawaited(
        Navigator.of(context).push(
          MaterialPageRoute<void>(
            builder: (context) => _PushedPage(
              child: DockerForwardsPanel(service: widget.service, onClose: () => Navigator.of(context).maybePop()),
            ),
          ),
        ),
      );
      return;
    }
    setState(() => _forwardsOpen = !_forwardsOpen);
  }

  DockerContainerActions get _actions => DockerContainerActions(
    run: _act,
    logs: (c) => _open(c, DockerDetailsTab.logs),
    details: _open,
    terminal: _terminal,
    forward: _forward,
    copyId: (c) => copyPlainWithNotice(context, ref, c.id, what: context.l10n.dockerContainerId),
    busy: (c) => _busy.contains(c.id),
  );

  Future<void> _act(DockerContainer container, DockerAction action) async {
    if (_busy.contains(container.id)) return;
    final l = context.l10n;
    if (action == DockerAction.kill || action == DockerAction.remove) {
      final approved = await showConfirmDialog(
        context,
        title: action == DockerAction.kill ? l.dockerKillTitle(container.name) : l.dockerRemoveTitle(container.name),
        message: action == DockerAction.kill ? l.dockerKillHelp : l.dockerRemoveHelp,
        confirmLabel: dockerActionLabel(l, action),
        destructive: true,
      );
      if (!approved || !mounted) return;
    }
    setState(() => _busy.add(container.id));
    try {
      await widget.service.action(widget.session.id, container.id, action);
      if (!mounted) return;
      if (action == DockerAction.remove && _selected == container.id) setState(() => _selected = null);
    } catch (error) {
      if (mounted) showSnack(context, dockerErrorText(context.l10n, error), error: true);
    } finally {
      if (mounted) setState(() => _busy.remove(container.id));
    }
    if (mounted) await _refresh();
  }

  Future<void> _terminal(DockerContainer container) =>
      openDockerTerminal(context, session: widget.session, hostName: _hostName, container: container);

  Future<void> _forward(DockerContainer container) async {
    final created = await showDockerForwardDialog(
      context,
      service: widget.service,
      session: widget.session,
      container: container,
    );
    if (created == true && mounted && _wide && !_forwardsOpen) _toggleForwards();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final containers = _containers.value;
    final groups = groupDockerContainers(containers, query: _query, filter: _filter);
    final active = containers.where((c) => dockerIsActive(c.state)).length;
    final selected = containers.where((c) => c.id == _selected).firstOrNull;
    final forwards = ref.watch(dockerForwardsProvider).value?.where((f) => f.state == DockerForwardState.running);
    final pad = pagePadding(context);
    final list = _error != null && !_loaded
        ? Center(
            child: InfoBanner(
              key: const ValueKey('docker-containers-error'),
              tone: BannerTone.danger,
              message: dockerErrorText(l, _error!),
              action: GlassButton(label: l.dockerRetry, onPressed: _refresh),
            ),
          )
        : !_loaded
        ? const SizedBox.shrink()
        : groups.isEmpty
        ? EmptyState(
            key: const ValueKey('docker-no-containers'),
            icon: Icons.view_in_ar_outlined,
            title: containers.isEmpty ? l.dockerNoContainers : l.dockerNoMatches,
            message: containers.isEmpty ? l.dockerNoContainersHelp : null,
          )
        : DockerContainerList(
            groups: groups,
            collapsed: _collapsed,
            selectedId: _selected,
            actions: _actions,
            onToggleGroup: (key) =>
                setState(() => _collapsed.contains(key) ? _collapsed.remove(key) : _collapsed.add(key)),
          );
    return Material(
      type: MaterialType.transparency,
      child: Padding(
        padding: EdgeInsets.fromLTRB(pad, GlassSpacing.s16, pad, pad),
        child: LayoutBuilder(
          builder: (context, constraints) {
            final wide = _wide = constraints.maxWidth >= _sideBySide;
            final both = _both = constraints.maxWidth >= _bothPanels;
            final showForwards = wide && _forwardsOpen;
            final showDetails = wide && selected != null && (!showForwards || both);
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _EngineHeader(
                  session: widget.session,
                  hostName: _hostName,
                  host: widget.host,
                  running: _loaded ? containers.where((c) => c.state == DockerContainerState.running).length : null,
                  total: _loaded ? containers.length : null,
                  loading: _loading,
                  forwards: forwards?.length ?? 0,
                  forwardsOpen: _forwardsOpen,
                  onRefresh: _refresh,
                  onForwards: _toggleForwards,
                  onDisconnect: widget.onDisconnect,
                ),
                const SizedBox(height: GlassSpacing.s12),
                Wrap(
                  spacing: GlassSpacing.s8,
                  runSpacing: GlassSpacing.s8,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    SizedBox(
                      width: constraints.maxWidth < 600 ? constraints.maxWidth : 320,
                      child: GlassField(
                        key: const ValueKey('docker-filter'),
                        fieldKey: const ValueKey('docker-filter-input'),
                        search: true,
                        placeholder: l.dockerFilterContainers,
                        onChanged: (value) => setState(() => _query = value),
                      ),
                    ),
                    GlassSegmented<DockerStateFilter>(
                      key: const ValueKey('docker-state-filter'),
                      inChrome: false,
                      semanticLabel: l.dockerStateFilter,
                      selected: _filter,
                      onChanged: (value) => setState(() => _filter = value),
                      segments: [
                        GlassSegment(
                          key: const ValueKey('docker-filter-all'),
                          value: DockerStateFilter.all,
                          label: l.dockerFilterAll(containers.length),
                        ),
                        GlassSegment(
                          key: const ValueKey('docker-filter-running'),
                          value: DockerStateFilter.running,
                          label: l.dockerFilterRunning(active),
                        ),
                        GlassSegment(
                          key: const ValueKey('docker-filter-stopped'),
                          value: DockerStateFilter.stopped,
                          label: l.dockerFilterStopped(containers.length - active),
                        ),
                      ],
                    ),
                  ],
                ),
                if (_error != null && _loaded)
                  Padding(
                    padding: const EdgeInsets.only(top: GlassSpacing.s8),
                    child: Text(
                      dockerErrorText(l, _error!),
                      key: const ValueKey('docker-refresh-error'),
                      style: GlassTokens.of(context).typography.callout
                          .copyWith(color: GlassTokens.of(context).palette.danger),
                    ),
                  ),
                const SizedBox(height: GlassSpacing.s8),
                SizedBox(height: 2, child: _loading && !_loaded ? const LinearProgressIndicator(minHeight: 2) : null),
                Expanded(
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Expanded(child: list),
                      if (showDetails) ...[
                        const SizedBox(width: GlassSpacing.s12),
                        SizedBox(
                          width: both && showForwards ? 440 : 480,
                          child: DockerContainerDetails(
                            key: ValueKey(('docker-details', selected.id)),
                            service: widget.service,
                            session: widget.session,
                            container: selected,
                            initialTab: _tab,
                            tabRequest: _tabRequest,
                            onClose: () => setState(() => _selected = null),
                            actions: _actions,
                          ),
                        ),
                      ],
                      if (showForwards) ...[
                        const SizedBox(width: GlassSpacing.s12),
                        SizedBox(
                          width: 340,
                          child: DockerForwardsPanel(
                            service: widget.service,
                            onClose: () => setState(() => _forwardsOpen = false),
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
      ),
    );
  }
}

class _EngineHeader extends StatelessWidget {
  const _EngineHeader({
    required this.session,
    required this.hostName,
    required this.host,
    required this.running,
    required this.total,
    required this.loading,
    required this.forwards,
    required this.forwardsOpen,
    required this.onRefresh,
    required this.onForwards,
    required this.onDisconnect,
  });
  final DockerSessionInfo session;
  final String hostName;
  final DockerHost? host;
  final int? running, total;
  final bool loading, forwardsOpen;
  final int forwards;
  final VoidCallback onRefresh, onForwards, onDisconnect;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final local = host?.kind == DockerHostKind.local || session.hostId == DockerHost.localId;
    final platform = [session.os, session.arch].where((s) => s.isNotEmpty).join('/');
    final facts = [
      if (session.engineVersion.isNotEmpty) l.dockerEngineVersion(session.engineVersion),
      if (platform.isNotEmpty) platform,
      if (running != null && total != null) l.dockerRunningOfTotal(running!, total!),
    ];
    return ContentSurface(
      key: const ValueKey('docker-engine-header'),
      padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s8, GlassSpacing.s12, GlassSpacing.s8),
      child: Row(
        children: [
          GlassIconButton(
            key: const ValueKey('docker-disconnect'),
            icon: Icons.arrow_back_rounded,
            tooltip: l.dockerDisconnect,
            style: GlassIconButtonStyle.plain,
            onPressed: onDisconnect,
          ),
          const SizedBox(width: GlassSpacing.s6),
          Icon(local ? Icons.laptop_mac_rounded : Icons.dns_outlined, size: 22, color: tokens.palette.accent),
          const SizedBox(width: GlassSpacing.s12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  local ? l.dockerLocalHost : hostName,
                  key: const ValueKey('docker-engine-host'),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: tokens.typography.title3.copyWith(color: tokens.palette.label),
                ),
                if (facts.isNotEmpty)
                  Text(
                    facts.join(' · '),
                    key: const ValueKey('docker-engine-facts'),
                    maxLines: 3,
                    overflow: TextOverflow.ellipsis,
                    style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                  ),
              ],
            ),
          ),
          const SizedBox(width: GlassSpacing.s8),
          Stack(
            clipBehavior: Clip.none,
            children: [
              GlassIconButton(
                key: const ValueKey('docker-forwards-toggle'),
                icon: Icons.swap_horiz_rounded,
                tooltip: l.dockerForwards,
                selected: forwardsOpen,
                onPressed: onForwards,
              ),
              if (forwards > 0)
                Positioned(
                  right: -6,
                  top: -6,
                  child: IgnorePointer(
                    child: GlassBadge(label: '$forwards', tone: GlassTone.success, dense: true),
                  ),
                ),
            ],
          ),
          const SizedBox(width: GlassSpacing.s6),
          GlassIconButton(
            key: const ValueKey('docker-refresh'),
            icon: Icons.refresh,
            tooltip: l.dockerRefresh,
            onPressed: loading ? null : onRefresh,
          ),
        ],
      ),
    );
  }
}

/// Keeps a pushed details page on the live container list; closes it when
/// the container is gone.
class _DetailsHost extends StatelessWidget {
  const _DetailsHost({required this.containers, required this.id, required this.builder});
  final ValueListenable<List<DockerContainer>> containers;
  final String id;
  final Widget Function(DockerContainer container, VoidCallback close) builder;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder<List<DockerContainer>>(
    valueListenable: containers,
    builder: (context, list, _) {
      final container = list.where((c) => c.id == id).firstOrNull;
      if (container == null) {
        return EmptyState(
          icon: Icons.view_in_ar_outlined,
          title: context.l10n.dockerContainerGone,
          compact: true,
          action: GlassButton(label: context.l10n.commonClose, onPressed: () => Navigator.of(context).maybePop()),
        );
      }
      return builder(container, () => Navigator.of(context).maybePop());
    },
  );
}

/// Full-screen page for narrow windows (list → details push).
class _PushedPage extends StatelessWidget {
  const _PushedPage({required this.child});
  final Widget child;

  @override
  Widget build(BuildContext context) => Scaffold(
    backgroundColor: GlassTokens.of(context).surfaces.contentSolid,
    body: SafeArea(
      child: Padding(padding: const EdgeInsets.all(GlassSpacing.s8), child: child),
    ),
  );
}
