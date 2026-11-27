import 'dart:async';

import 'package:consolecrypt/app/features.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/tool_icons.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/docker/docker_compose_dialog.dart';
import 'package:consolecrypt/docker/docker_container_details.dart';
import 'package:consolecrypt/docker/docker_container_list.dart';
import 'package:consolecrypt/docker/docker_database_connect.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_forwards.dart';
import 'package:consolecrypt/docker/docker_image_list.dart';
import 'package:consolecrypt/docker/docker_network_list.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_removal_dialog.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/docker/docker_system_views.dart';
import 'package:consolecrypt/docker/docker_terminal.dart';
import 'package:consolecrypt/docker/docker_volume_list.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/notes_providers.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Content width from which details and forwards sit beside the list.
const _sideBySide = 860.0;

const _minDetailsWidth = 360.0;
const _minListWidth = 400.0;

/// One connected engine: header, grouped containers, the container drawer
/// and the Docker forwards panel. Refreshes every 5 s while visible and the
/// app is in the foreground.
class DockerWorkspace extends ConsumerStatefulWidget {
  const DockerWorkspace({
    required this.service,
    required this.session,
    required this.onDisconnect,
    this.host,
    this.sessionPanel = false,
    this.refreshInterval = const Duration(seconds: 5),
    super.key,
  });
  final bool sessionPanel;
  final DockerService service;
  final DockerSessionInfo session;
  final DockerHost? host;
  final VoidCallback onDisconnect;
  final Duration refreshInterval;
  @override
  ConsumerState<DockerWorkspace> createState() => _DockerWorkspaceState();
}

enum _DockerTab { containers, volumes, images, networks, disk, events }

class _DockerWorkspaceState extends ConsumerState<DockerWorkspace> with WidgetsBindingObserver, NoteContextPublisher {
  @override
  ShellBranch get noteBranch => ShellBranch.docker;

  final _containers = ValueNotifier<List<DockerContainer>>(const []);
  final _collapsed = <String>{};
  final _busy = <String>{};
  final _removingProjects = <String>{};
  Timer? _timer;
  bool _foreground = true, _loading = false, _loaded = false;
  DockerStateFilter _filter = DockerStateFilter.all;
  var _inventory = _DockerTab.containers;
  bool _images = false;
  DockerContainerState? _panelState;
  bool _sortByState = true;
  static const _stateOrder = [
    DockerContainerState.failed,
    DockerContainerState.dead,
    DockerContainerState.restarting,
    DockerContainerState.running,
    DockerContainerState.paused,
    DockerContainerState.exited,
    DockerContainerState.created,
    DockerContainerState.removing,
    DockerContainerState.unknown,
  ];
  final _diskView = GlobalKey<DockerDiskUsageState>();
  final _eventsView = GlobalKey<DockerEventJournalState>();
  final _networkList = GlobalKey<DockerNetworkListState>();
  final _imageList = GlobalKey<DockerImageListState>();
  DialogRoute<void>? _detailDialog;

  final _volumeList = GlobalKey<DockerVolumeListState>();
  Object? _error;
  String _query = '';
  String? _selected;
  double _detailsFraction = .43;
  DockerDetailsTab _tab = DockerDetailsTab.overview;
  int _tabRequest = 0, _generation = 0;

  String get _hostName => widget.session.hostName.isNotEmpty ? widget.session.hostName : (widget.host?.name ?? '');

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    unawaited(_refresh());
    _timer = Timer.periodic(widget.refreshInterval, (_) {
      if (!mounted || !_foreground || !TickerMode.valuesOf(context).enabled) return;
      if (!_loading) unawaited(_refresh());
      _eventsView.currentState?.refresh();
    });
  }

  @override
  void dispose() {
    _generation++;
    final route = _detailDialog;
    if (route != null && route.isActive) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (route.isActive) route.navigator?.removeRoute(route);
      });
    }
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
  bool _wide = false;

  void _open(DockerContainer container, [DockerDetailsTab tab = DockerDetailsTab.overview]) {
    if (widget.sessionPanel) {
      if (_detailDialog?.isActive == true) {
        return;
      }
      late final DialogRoute<void> route;
      route = DialogRoute<void>(
        context: context,
        builder: (ctx) => Dialog(
          child: SizedBox(
            width: 760,
            height: 620,
            child: _DetailsHost(
              containers: _containers,
              id: container.id,
              builder: (current, close) => DockerContainerDetails(
                key: ValueKey(current.id),
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
      );
      _detailDialog = route;
      unawaited(
        Navigator.of(context).push(route).whenComplete(() {
          if (identical(_detailDialog, route)) _detailDialog = null;
        }),
      );
      return;
    }
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
    });
  }

  /// Running forwards live in the shared right panel with the Kubernetes
  /// ones; narrow layouts push a page instead.
  void _toggleForwards() {
    if (!AppWindowLayout.isPhone(context) && _wide) {
      ref.read(workspaceToolsProvider.notifier).toggle(WorkspaceTool.forwards);
      return;
    }
    if (!_wide || AppWindowLayout.isPhone(context)) {
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
  }

  DockerContainerActions get _actions => DockerContainerActions(
    run: _act,
    logs: (c) => _open(c, DockerDetailsTab.logs),
    details: _open,
    terminal: _terminal,
    forward: _forward,
    copyId: (c) => copyPlainWithNotice(context, ref, c.id, what: context.l10n.dockerContainerId),
    busy: (c) => _busy.contains(c.id) || _removingProjects.contains(c.composeProject),
    database: databasesEnabled && ref.read(dbServiceProvider) != null ? _database : null,
  );

  Future<void> _act(DockerContainer container, DockerAction action) async {
    if (_busy.contains(container.id) || _removingProjects.contains(container.composeProject)) return;
    final l = context.l10n;
    if (action == DockerAction.kill) {
      final approved = await showConfirmDialog(
        context,
        title: l.dockerKillTitle(container.name),
        message: l.dockerKillHelp,
        confirmLabel: dockerActionLabel(l, action),
        destructive: true,
      );
      if (!approved || !mounted) return;
    }
    setState(() => _busy.add(container.id));
    try {
      if (action == DockerAction.remove) {
        await _removeContainer(container);
        return;
      }
      await widget.service.action(widget.session.id, container.id, action);
    } catch (error) {
      if (mounted) showSnack(context, dockerErrorText(context.l10n, error), error: true);
    } finally {
      if (mounted) setState(() => _busy.remove(container.id));
    }
    if (mounted) await _refresh();
  }

  Future<void> _removeContainer(DockerContainer container) async {
    final sessionId = widget.session.id;
    final plan = await widget.service.removalPlan(sessionId, container.id);
    if (!mounted || widget.session.id != sessionId) return;
    if (plan.containerId != container.id) throw const DockerException(DockerFailure.other);
    final local = widget.host?.kind == DockerHostKind.local || DockerHost.isLocalId(widget.session.hostId);
    final selection = await showDockerRemovalPlan(context, plan, local ? context.l10n.dockerLocalHost : _hostName);
    if (selection == null || !mounted || widget.session.id != sessionId) return;
    final result = await widget.service.removeWithCleanup(sessionId, container.id, selection);
    if (!mounted) return;
    await _refresh();
    if (!mounted) return;
    unawaited(_volumeList.currentState?.refresh());
    ref.invalidate(dockerForwardsProvider);
    if (!result.containerRemoved || result.error != null || result.resources.isNotEmpty) {
      await showDockerRemovalResult(context, result);
    } else {
      showSnack(context, context.l10n.dockerRemovalRemoved);
    }
  }

  Future<void> _removeProject(String project) async {
    final session = widget.session;
    final scope = ref.read(dockerScopeProvider);
    bool current() =>
        mounted &&
        widget.session.id == session.id &&
        ref.read(dockerScopeProvider).unlocked &&
        ref.read(dockerScopeProvider).profileId == scope.profileId;
    if (!current() || !_removingProjects.add(project)) return;
    setState(() {});
    final l = context.l10n;
    try {
      final all = await widget.service.containers(session.id);
      if (!mounted || !current()) return;
      final targets = all.where((c) => c.composeProject == project).toList();
      if (targets.isEmpty) {
        await _refresh();
        return;
      }
      final local = widget.host?.kind == DockerHostKind.local || DockerHost.isLocalId(session.hostId);
      final volumes = await confirmDockerProjectRemoval(
        context,
        local ? l.dockerLocalHost : _hostName,
        project,
        targets,
      );
      if (volumes == null || !current()) return;
      final issues = <String>[];
      var removed = 0;
      for (final target in targets) {
        if (!mounted || !current()) return;
        try {
          // Replan after each removal. A network shared within this project
          // becomes eligible only when its last confirmed container is removed.
          final plan = await widget.service.removalPlan(session.id, target.id);
          if (!mounted || !current()) return;
          if (plan.containerId != target.id || plan.name != target.name) {
            issues.add('${target.name} — ${l.dockerRemovalChanged}');
            continue;
          }
          final result = await widget.service.removeWithCleanup(
            session.id,
            target.id,
            DockerRemovalSelection(
              planId: plan.planId,
              confirmName: target.name,
              stopRunning: true,
              networks: [
                for (final r in plan.resources)
                  if (r.kind == DockerRemovalKind.network && r.eligible) r.id,
              ],
              volumes: [
                if (volumes)
                  for (final r in plan.resources)
                    if (r.kind == DockerRemovalKind.volume && r.eligible) r.id,
              ],
            ),
          );
          if (result.containerRemoved) removed++;
          if (!result.containerRemoved || result.error != null) {
            issues.add('${target.name} — ${dockerRemovalReason(l, result.error ?? 'unknown')}');
          }
          for (final r in result.resources) {
            if (r.status != DockerRemovalStatus.removed) {
              issues.add('${r.name} — ${dockerRemovalReason(l, r.reason)}');
            }
          }
        } catch (error) {
          issues.add('${target.name} — ${dockerErrorText(l, error)}');
        }
      }
      if (!mounted || !current()) return;
      await _refresh();
      if (!mounted || !current()) return;
      unawaited(_volumeList.currentState?.refresh());
      ref.invalidate(dockerForwardsProvider);
      final summary = l.dockerComposeRemoveSummary(project, removed, targets.length);
      if (issues.isEmpty) {
        showSnack(context, summary);
      } else {
        await showDockerProjectRemovalIssues(context, summary, issues);
      }
    } catch (error) {
      if (mounted && current()) showSnack(context, dockerErrorText(l, error), error: true);
    } finally {
      if (mounted) setState(() => _removingProjects.remove(project));
    }
  }

  /// Project-wide engine actions and the compose files dialog.
  Future<void> _compose(DockerContainerGroup group, DockerComposeCommand command) async {
    final project = group.project;
    if (project == null || _removingProjects.contains(project)) return;
    if (command == DockerComposeCommand.removeProject) {
      if (_containers.value.any((c) => c.composeProject == project && _busy.contains(c.id))) return;
      await _removeProject(project);
      return;
    }
    final action = switch (command) {
      DockerComposeCommand.startAll => DockerAction.start,
      DockerComposeCommand.stopAll => DockerAction.stop,
      DockerComposeCommand.restartAll => DockerAction.restart,
      DockerComposeCommand.viewYaml || DockerComposeCommand.editYaml || DockerComposeCommand.removeProject => null,
    };
    if (action == null) {
      await showDockerComposeDialog(
        context,
        service: widget.service,
        session: widget.session,
        project: project,
        edit: command == DockerComposeCommand.editYaml,
      );
      if (mounted) await _refresh();
      return;
    }
    final targets = [
      for (final c in _containers.value)
        if (c.composeProject == project && dockerActionAllowed(action, c.state) && !_busy.contains(c.id)) c,
    ];
    if (targets.isEmpty) return;
    final l = context.l10n;
    if (action != DockerAction.start) {
      final stop = action == DockerAction.stop;
      final approved = await showConfirmDialog(
        context,
        title: stop ? l.dockerComposeStopAllTitle(project) : l.dockerComposeRestartAllTitle(project),
        message: l.dockerComposeBulkHelp(targets.length),
        confirmLabel: stop ? l.dockerComposeStopAll : l.dockerComposeRestartAll,
        destructive: stop,
      );
      if (!approved || !mounted || _removingProjects.contains(project)) return;
    }
    setState(() => _busy.addAll(targets.map((c) => c.id)));
    var failed = 0;
    for (final container in targets) {
      try {
        await widget.service.action(widget.session.id, container.id, action);
      } catch (_) {
        failed++;
      } finally {
        if (mounted) setState(() => _busy.remove(container.id));
      }
      if (!mounted) return;
    }
    if (failed > 0 && mounted) showSnack(context, l.dockerComposeBulkFailed(failed), error: true);
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
    if (created == true && mounted && !AppWindowLayout.isPhone(context) && _wide) {
      ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.forwards);
    }
  }

  Future<void> _database(DockerContainer container, DockerDbTarget target) async {
    try {
      await connectDockerDatabase(
        context,
        ref,
        service: widget.service,
        session: widget.session,
        container: container,
        target: target,
      );
    } catch (error) {
      if (mounted) showSnack(context, dockerErrorText(context.l10n, error), error: true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final noted = _containers.value.where((c) => c.id == _selected).firstOrNull;
    if (!widget.sessionPanel) {
      publishNoteContext(
        noted == null
            ? null
            : NoteContext(
                // The Docker host id is `local` for this computer.
                target: NoteDockerTarget(
                  hostId: widget.session.hostId == 'local'
                      ? null
                      : widget.session.hostId, // l10n-ignore: Docker host id
                  name: noted.name,
                ),
                label: noted.name,
                icon: ToolIcons.docker,
              ),
      );
    }
    final l = context.l10n;
    final containers = _containers.value;
    if (widget.sessionPanel) {
      final observer = widget.service;
      final visible =
          containers
              .where(
                (c) =>
                    _panelState == null ||
                    c.displayState == _panelState ||
                    (_panelState == DockerContainerState.failed && c.displayState == DockerContainerState.dead),
              )
              .toList()
            ..sort((a, b) {
              if (_sortByState) {
                final rank = _stateOrder.indexOf(a.displayState).compareTo(_stateOrder.indexOf(b.displayState));
                if (rank != 0) return rank;
              }
              return a.name.compareTo(b.name);
            });
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 8, 4),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    _hostName,
                    overflow: TextOverflow.ellipsis,
                    style: GlassTokens.of(context).typography.bodyEmph,
                  ),
                ),
                GlassIconButton(
                  icon: Icons.refresh,
                  tooltip: l.dockerRefresh,
                  size: 28,
                  style: GlassIconButtonStyle.plain,
                  onPressed: () {
                    if (_images) {
                      _imageList.currentState?.refresh();
                    } else {
                      unawaited(_refresh());
                    }
                  },
                ),
              ],
            ),
          ),
          if (observer is DockerObservationService)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 8),
              child: GlassSegmented<bool>(
                semanticLabel: l.dockerView,
                selected: _images,
                segments: [
                  GlassSegment(value: false, label: l.dockerStandalone),
                  GlassSegment(value: true, label: l.kubeImages),
                ],
                onChanged: (v) => setState(() => _images = v),
              ),
            ),
          if (!_images)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
              child: Wrap(
                spacing: 8,
                runSpacing: 4,
                children: [
                  GlassSelect<DockerContainerState?>(
                    key: const ValueKey('docker-panel-filter'),
                    semanticLabel: l.dockerStateFilter,
                    value: _panelState,
                    items: [
                      GlassSelectItem(value: null, label: l.systemdAll),
                      for (final state in _stateOrder.where((s) => s != DockerContainerState.dead))
                        GlassSelectItem(value: state, label: dockerStateLabel(l, state)),
                    ],
                    onChanged: (state) => setState(() => _panelState = state),
                  ),
                  GlassSelect<bool>(
                    key: const ValueKey('docker-panel-sort'),
                    semanticLabel: l.dockerContainerSort,
                    value: _sortByState,
                    items: [
                      GlassSelectItem(value: true, label: l.systemdSortState),
                      GlassSelectItem(value: false, label: l.systemdSortName),
                    ],
                    onChanged: (value) => setState(() => _sortByState = value),
                  ),
                ],
              ),
            ),
          Expanded(
            child: _images
                ? DockerImageList(
                    key: _imageList,
                    kubernetes: ref.watch(kubeServiceProvider),
                    service: observer as DockerObservationService,
                    session: widget.session,
                  )
                : _error != null
                ? Center(child: Text(dockerErrorText(l, _error!)))
                : !_loaded
                ? const Center(child: CircularProgressIndicator())
                : visible.isEmpty
                ? Center(child: Text(containers.isEmpty ? l.dockerNoContainers : l.dockerNoMatches))
                : ListView.separated(
                    itemCount: visible.length,
                    separatorBuilder: (_, _) => const Divider(height: 1),
                    itemBuilder: (context, i) {
                      final c = visible[i], tokens = GlassTokens.of(context);
                      final color = tokens.palette.tone(dockerStateTone(c.displayState));
                      return ListTile(
                        dense: true,
                        visualDensity: VisualDensity.compact,
                        contentPadding: const EdgeInsets.symmetric(horizontal: 12),
                        leading: Icon(ToolIcons.docker, size: 18, color: color),
                        title: Text(c.name, maxLines: 1, overflow: TextOverflow.ellipsis),
                        subtitle: Text(c.image, maxLines: 1, overflow: TextOverflow.ellipsis),
                        trailing: Tooltip(
                          message: c.status,
                          child: Text(
                            dockerStateLabel(l, c.displayState),
                            style: tokens.typography.caption.copyWith(color: color),
                          ),
                        ),
                        onTap: () => _open(c),
                      );
                    },
                  ),
          ),
        ],
      );
    }
    final groups = groupDockerContainers(containers, query: _query, filter: _filter);
    final active = containers.where((c) => dockerIsActive(c.state)).length;
    final selected = containers.where((c) => c.id == _selected).firstOrNull;
    final forwards = ref.watch(dockerForwardsProvider).value?.where((f) => f.state == DockerForwardState.running);
    final pad = pagePadding(context);
    final observation = widget.service;
    final list = _inventory == _DockerTab.images && observation is DockerObservationService
        ? DockerImageList(
            key: _imageList,
            kubernetes: ref.watch(kubeServiceProvider),
            service: observation as DockerObservationService,
            session: widget.session,
            query: _query,
          )
        : _inventory == _DockerTab.networks && observation is DockerObservationService
        ? DockerNetworkList(
            key: _networkList,
            service: observation as DockerObservationService,
            session: widget.session,
            query: _query,
            containers: containers,
          )
        : _inventory == _DockerTab.disk && observation is DockerObservationService
        ? DockerDiskUsage(
            key: _diskView,
            service: observation as DockerObservationService,
            sessionId: widget.session.id,
            query: _query,
          )
        : _inventory == _DockerTab.events && observation is DockerObservationService
        ? DockerEventJournal(
            key: _eventsView,
            service: observation as DockerObservationService,
            sessionId: widget.session.id,
            query: _query,
          )
        : _inventory == _DockerTab.volumes
        ? DockerVolumeList(key: _volumeList, service: widget.service, session: widget.session, query: _query)
        : _error != null && !_loaded
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
            icon: ToolIcons.docker,
            title: containers.isEmpty ? l.dockerNoContainers : l.dockerNoMatches,
            message: containers.isEmpty ? l.dockerNoContainersHelp : null,
          )
        : DockerContainerList(
            groups: groups,
            collapsed: _collapsed,
            selectedId: _selected,
            actions: _actions,
            onCompose: (group, command) => unawaited(_compose(group, command)),
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
            final paneSpace = constraints.maxWidth - GlassSpacing.s12;
            final maxDetailsWidth = wide ? paneSpace - _minListWidth : _minDetailsWidth;
            final detailsWidth = (paneSpace * _detailsFraction).clamp(_minDetailsWidth, maxDetailsWidth);
            void resizeBy(double delta) => setState(() {
              final currentWidth = (_detailsFraction * paneSpace).clamp(_minDetailsWidth, maxDetailsWidth);
              _detailsFraction = (currentWidth + delta).clamp(_minDetailsWidth, maxDetailsWidth) / paneSpace;
            });
            final showDetails = _inventory == _DockerTab.containers && wide && selected != null;
            final sections = <_DockerTab, String>{
              _DockerTab.containers: l.dockerStandalone,
              _DockerTab.volumes: l.dockerVolumes,
              if (observation is DockerObservationService) ...{
                _DockerTab.images: l.kubeImages,
                _DockerTab.networks: l.dockerNetworks,
                _DockerTab.disk: l.dockerDiskUsage,
                _DockerTab.events: l.dockerEventJournal,
              },
            };
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
                  forwardsOpen: ref.watch(workspaceToolsProvider).selected == WorkspaceTool.forwards,
                  onRefresh: () {
                    unawaited(_refresh());
                    unawaited(_volumeList.currentState?.refresh());
                    _imageList.currentState?.refresh();
                    _networkList.currentState?.refresh();
                    _diskView.currentState?.refresh();
                    _eventsView.currentState?.refresh();
                  },
                  onForwards: _toggleForwards,
                  onDisconnect: widget.onDisconnect,
                ),
                const SizedBox(height: GlassSpacing.s12),
                ContentSurface(
                  padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8),
                  child: Wrap(
                    spacing: GlassSpacing.s8,
                    runSpacing: GlassSpacing.s8,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      DefaultTabController(
                        length: sections.length,
                        initialIndex: sections.keys.toList().indexOf(_inventory),
                        child: TabBar(
                          key: const ValueKey('docker-view'),
                          isScrollable: true,
                          tabAlignment: TabAlignment.start,
                          dividerHeight: 0,
                          labelPadding: const EdgeInsets.symmetric(horizontal: 12),
                          labelStyle: GlassTokens.of(context).typography.button,
                          onTap: (i) => setState(() => _inventory = sections.keys.elementAt(i)),
                          tabs: [
                            for (final section in sections.entries)
                              Tab(
                                key: ValueKey('docker-view-${section.key.name}'),
                                text: section.value,
                                height: MediaQuery.textScalerOf(context).scale(20) + 14,
                              ),
                          ],
                        ),
                      ),
                      ConstrainedBox(
                        constraints: const BoxConstraints(maxWidth: 320),
                        child: GlassField(
                          key: const ValueKey('docker-filter'),
                          fieldKey: const ValueKey('docker-filter-input'),
                          search: true,
                          placeholder: _inventory == _DockerTab.containers ? l.dockerFilterContainers : l.commonSearch,
                          onChanged: (value) => setState(() => _query = value),
                        ),
                      ),
                      if (_inventory == _DockerTab.containers)
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
                        Semantics(
                          label: l.workspaceToolsResize,
                          onIncrease: () => resizeBy(24),
                          onDecrease: () => resizeBy(-24),
                          child: MouseRegion(
                            cursor: SystemMouseCursors.resizeColumn,
                            child: GestureDetector(
                              key: const ValueKey('docker-details-divider'),
                              behavior: HitTestBehavior.opaque,
                              dragStartBehavior: DragStartBehavior.down,
                              onHorizontalDragUpdate: (event) => resizeBy(-event.delta.dx),
                              child: VerticalDivider(
                                width: GlassSpacing.s12,
                                thickness: 1,
                                color: GlassTokens.of(context).surfaces.separator,
                              ),
                            ),
                          ),
                        ),
                        SizedBox(
                          width: detailsWidth,
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
    final local = host?.kind == DockerHostKind.local || DockerHost.isLocalId(session.hostId);
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
          icon: ToolIcons.docker,
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
