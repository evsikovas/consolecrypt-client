import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/docker/docker_database_connect.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:material_ui/material_ui.dart';

/// What a container row, its menu and the drawer can do; the workspace
/// implements it.
final class DockerContainerActions {
  const DockerContainerActions({
    required this.run,
    required this.logs,
    required this.details,
    required this.terminal,
    required this.forward,
    required this.copyId,
    required this.busy,
    this.database,
  });
  final Future<void> Function(DockerContainer container, DockerAction action) run;
  final void Function(DockerContainer container) logs, details;
  final Future<void> Function(DockerContainer container) terminal, forward, copyId;
  final bool Function(DockerContainer container) busy;

  /// Opens the container's database in the Databases section; null when the
  /// Databases section is unavailable.
  final Future<void> Function(DockerContainer container, DockerDbTarget target)? database;

  /// The database [container] runs when it can be opened from here.
  DockerDbTarget? databaseTarget(DockerContainer container, [DockerJson? inspect]) =>
      database == null ? null : dockerDatabaseTarget(container, inspect);
}

/// Menu values beyond the engine actions.
enum DockerMenuCommand { details, logs, terminal, forward, database, copyId }

/// Every action of one container, for the row menu, the context menu and
/// the drawer. [database] adds "Connect to database".
List<GlassMenuEntry<Object>> dockerContainerMenu(
  AppLocalizations l,
  DockerContainer container, {
  DockerDbTarget? database,
}) {
  final running = container.state == DockerContainerState.running;
  GlassMenuItem<Object> engine(DockerAction action, IconData icon, {bool destructive = false}) => GlassMenuItem<Object>(
    key: ValueKey('docker-menu-${action.name}'),
    value: action,
    label: dockerActionLabel(l, action),
    icon: icon,
    destructive: destructive,
    enabled: dockerActionAllowed(action, container.state),
  );
  return [
    GlassMenuItem<Object>(
      key: const ValueKey('docker-menu-logs'),
      value: DockerMenuCommand.logs,
      label: l.dockerLogs,
      icon: Icons.article_outlined,
    ),
    GlassMenuItem<Object>(
      key: const ValueKey('docker-menu-terminal'),
      value: DockerMenuCommand.terminal,
      label: l.dockerTerminal,
      icon: Icons.terminal_rounded,
      enabled: running,
    ),
    GlassMenuItem<Object>(
      key: const ValueKey('docker-menu-forward'),
      value: DockerMenuCommand.forward,
      label: l.dockerForwardPort,
      icon: Icons.swap_horiz_rounded,
      enabled: running,
    ),
    if (database != null)
      GlassMenuItem<Object>(
        key: const ValueKey('docker-menu-database'),
        value: DockerMenuCommand.database,
        label: l.kubeConnectDatabase,
        icon: Icons.storage_rounded,
        enabled: running,
      ),
    GlassMenuItem<Object>(
      key: const ValueKey('docker-menu-details'),
      value: DockerMenuCommand.details,
      label: l.dockerDetails,
      icon: Icons.info_outline_rounded,
    ),
    const GlassMenuDivider(),
    engine(DockerAction.start, Icons.play_arrow_rounded),
    engine(DockerAction.stop, Icons.stop_rounded),
    engine(DockerAction.restart, Icons.restart_alt_rounded),
    if (container.state == DockerContainerState.paused)
      engine(DockerAction.unpause, Icons.play_circle_outline_rounded)
    else
      engine(DockerAction.pause, Icons.pause_rounded),
    const GlassMenuDivider(),
    GlassMenuItem<Object>(
      key: const ValueKey('docker-menu-copy-id'),
      value: DockerMenuCommand.copyId,
      label: l.dockerCopyId,
      icon: Icons.content_copy_rounded,
    ),
    engine(DockerAction.kill, Icons.flash_on_rounded, destructive: true),
    engine(DockerAction.remove, Icons.delete_outline_rounded, destructive: true),
  ];
}

/// Runs a value picked from [dockerContainerMenu].
void runDockerMenuValue(
  DockerContainerActions actions,
  DockerContainer container,
  Object value, {
  DockerDbTarget? database,
}) {
  switch (value) {
    case final DockerAction action:
      unawaited(actions.run(container, action));
    case DockerMenuCommand.details:
      actions.details(container);
    case DockerMenuCommand.logs:
      actions.logs(container);
    case DockerMenuCommand.terminal:
      unawaited(actions.terminal(container));
    case DockerMenuCommand.forward:
      unawaited(actions.forward(container));
    case DockerMenuCommand.database:
      final target = database ?? actions.databaseTarget(container);
      if (target != null) unawaited(actions.database?.call(container, target));
    case DockerMenuCommand.copyId:
      unawaited(actions.copyId(container));
  }
}

/// What the menu of a compose project header does.
enum DockerComposeCommand { startAll, stopAll, restartAll, viewYaml, editYaml }

/// The compose project header menu (right click, long press or "…").
List<GlassMenuEntry<DockerComposeCommand>> dockerComposeMenu(AppLocalizations l, DockerContainerGroup group) {
  GlassMenuItem<DockerComposeCommand> item(
    DockerComposeCommand command,
    String label,
    IconData icon, {
    bool enabled = true,
  }) => GlassMenuItem<DockerComposeCommand>(
    key: ValueKey('docker-compose-menu-${command.name}'),
    value: command,
    label: label,
    icon: icon,
    enabled: enabled,
  );
  return [
    item(DockerComposeCommand.viewYaml, l.dockerComposeViewYaml, Icons.description_outlined),
    item(DockerComposeCommand.editYaml, l.dockerComposeEditYaml, Icons.edit_outlined),
    const GlassMenuDivider(),
    item(
      DockerComposeCommand.startAll,
      l.dockerComposeStartAll,
      Icons.play_arrow_rounded,
      enabled: group.running < group.total,
    ),
    item(DockerComposeCommand.stopAll, l.dockerComposeStopAll, Icons.stop_rounded, enabled: group.running > 0),
    item(
      DockerComposeCommand.restartAll,
      l.dockerComposeRestartAll,
      Icons.restart_alt_rounded,
      enabled: group.total > 0,
    ),
  ];
}

/// Containers grouped by compose project; groups collapse.
class DockerContainerList extends StatelessWidget {
  const DockerContainerList({
    required this.groups,
    required this.collapsed,
    required this.actions,
    required this.onToggleGroup,
    this.selectedId,
    this.onCompose,
    super.key,
  });
  final List<DockerContainerGroup> groups;
  final Set<String> collapsed;
  final DockerContainerActions actions;
  final ValueChanged<String> onToggleGroup;
  final String? selectedId;

  /// Runs a compose project menu command; null hides the menu.
  final void Function(DockerContainerGroup group, DockerComposeCommand command)? onCompose;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    // Each compose project (and the loose containers) is its own block.
    return RepaintBoundary(
      child: ScrollEdgeEffect(
        bottom: true,
        extent: 12,
        child: LayoutBuilder(
          builder: (context, constraints) => ListView.builder(
            key: const ValueKey('docker-containers'),
            padding: const EdgeInsets.only(bottom: GlassSpacing.s12),
            itemCount: groups.length,
            itemBuilder: (context, index) {
              final group = groups[index];
              final open = !collapsed.contains(group.key);
              final active = group.running > 0;
              return Padding(
                padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
                child: DecoratedBox(
                  key: ValueKey('docker-block-${group.key}'),
                  decoration: ShapeDecoration(
                    color: tokens.surfaces.fillField,
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(tokens.radii.card),
                      side: BorderSide(
                        color: active ? tokens.palette.success.withValues(alpha: .35) : tokens.surfaces.hairlineCard,
                      ),
                    ),
                  ),
                  child: Padding(
                    padding: const EdgeInsets.all(GlassSpacing.s4),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        _GroupHeader(
                          group: group,
                          collapsed: !open,
                          onToggle: () => onToggleGroup(group.key),
                          onCompose: group.project == null || onCompose == null
                              ? null
                              : (command) => onCompose!(group, command),
                        ),
                        if (open)
                          for (final container in group.containers)
                            DockerContainerRow(
                              container: container,
                              compact: constraints.maxWidth < 560,
                              selected: container.id == selectedId,
                              busy: actions.busy(container),
                              actions: actions,
                            ),
                      ],
                    ),
                  ),
                ),
              );
            },
          ),
        ),
      ),
    );
  }
}

class _GroupHeader extends StatelessWidget {
  const _GroupHeader({required this.group, required this.collapsed, required this.onToggle, this.onCompose});
  final DockerContainerGroup group;
  final bool collapsed;
  final VoidCallback onToggle;
  final ValueChanged<DockerComposeCommand>? onCompose;

  Future<void> _menuAt(BuildContext context, Offset position) async {
    final run = onCompose;
    if (run == null) return;
    final value = await showGlassMenu<DockerComposeCommand>(
      context: context,
      anchor: Rect.fromLTWH(position.dx, position.dy, 1, 1),
      entries: dockerComposeMenu(context.l10n, group),
    );
    if (value != null && context.mounted) run(value);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final compose = group.project != null;
    final run = onCompose;
    final header = GlassInteractive(
      key: ValueKey('docker-group-${group.key}'),
      semanticLabel: group.project ?? l.dockerStandalone,
      onPressed: onToggle,
      builder: (context, state) => DecoratedBox(
        decoration: ShapeDecoration(
          color: state.hovered ? tokens.surfaces.fillHover : Colors.transparent,
          shape: GlassRadii.shape(tokens.radii.row),
        ),
        child: Padding(
          padding: run == null ? const EdgeInsets.fromLTRB(6, 10, 12, 6) : const EdgeInsets.fromLTRB(6, 6, 6, 2),
          child: Row(
            children: [
              Icon(
                collapsed ? Icons.chevron_right_rounded : Icons.expand_more_rounded,
                size: 18,
                color: tokens.secondaryLabel,
              ),
              const SizedBox(width: 4),
              Icon(compose ? Icons.layers_outlined : Icons.view_in_ar_outlined, size: 16, color: tokens.secondaryLabel),
              const SizedBox(width: 8),
              Expanded(
                child: Row(
                  children: [
                    Flexible(
                      child: Text(
                        group.project ?? l.dockerStandalone,
                        key: ValueKey('docker-group-title-${group.key}'),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                      ),
                    ),
                    if (compose && MediaQuery.sizeOf(context).width >= 600) ...[
                      const SizedBox(width: 8),
                      Text(l.dockerCompose, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                    ],
                  ],
                ),
              ),
              const SizedBox(width: 8),
              Text(
                l.dockerGroupRunning(group.running, group.total),
                key: ValueKey('docker-group-count-${group.key}'),
                style: tokens.typography.caption.copyWith(
                  color: group.running == group.total ? tokens.palette.success : tokens.secondaryLabel,
                ),
              ),
              if (run != null) ...[
                const SizedBox(width: 4),
                GlassMenuButton<DockerComposeCommand>(
                  key: ValueKey('docker-compose-menu-${group.key}'),
                  entries: dockerComposeMenu(l, group),
                  onSelected: run,
                  builder: (context, open) => GlassIconButton(
                    key: ValueKey('docker-compose-menu-button-${group.key}'),
                    icon: Icons.more_horiz_rounded,
                    tooltip: l.dockerComposeActions,
                    size: 24,
                    iconSize: 16,
                    style: GlassIconButtonStyle.plain,
                    onPressed: open,
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
    if (run == null) return header;
    return GestureDetector(
      onSecondaryTapUp: (details) => unawaited(_menuAt(context, details.globalPosition)),
      onLongPressStart: (details) => unawaited(_menuAt(context, details.globalPosition)),
      child: header,
    );
  }
}

/// One container: state, name and compose service, image, health, status,
/// published ports and short id, with quick actions and a context menu.
class DockerContainerRow extends StatelessWidget {
  const DockerContainerRow({
    required this.container,
    required this.actions,
    this.selected = false,
    this.busy = false,
    this.compact = false,
    super.key,
  });
  final DockerContainer container;
  final DockerContainerActions actions;
  final bool selected, busy;

  /// Narrow list: no quick actions (the menu has them all).
  final bool compact;

  Future<void> _menuAt(BuildContext context, Offset position) async {
    final value = await showGlassMenu<Object>(
      context: context,
      anchor: Rect.fromLTWH(position.dx, position.dy, 1, 1),
      entries: dockerContainerMenu(context.l10n, container, database: actions.databaseTarget(container)),
    );
    if (value != null && context.mounted) runDockerMenuValue(actions, container, value);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final running = container.state == DockerContainerState.running;
    final active = dockerIsActive(container.state);
    final health = dockerHealth(l, container.health);
    final ports = formatDockerPorts(container.ports);
    final database = actions.databaseTarget(container);
    final caption = tokens.typography.caption.copyWith(color: tokens.secondaryLabel);
    Widget quick(String key, IconData icon, String tooltip, VoidCallback? onPressed, {Color? color}) => GlassIconButton(
      key: ValueKey('docker-$key-${container.id}'),
      icon: icon,
      tooltip: tooltip,
      size: 28,
      iconSize: 16,
      color: color,
      style: GlassIconButtonStyle.plain,
      onPressed: busy ? null : onPressed,
    );
    final row = GlassInteractive(
      key: ValueKey('docker-container-${container.id}'),
      semanticLabel: container.name,
      selected: selected,
      onPressed: () => actions.details(container),
      builder: (context, state) => DecoratedBox(
        decoration: ShapeDecoration(
          color: selected
              ? p.accent.withValues(alpha: .12)
              : state.hovered
              ? tokens.surfaces.fillHover
              : active
              ? p.success.withValues(alpha: .07)
              : Colors.transparent,
          shape: GlassRadii.shape(tokens.radii.row),
        ),
        // Stopped containers recede; running ones stay at full strength.
        child: Opacity(
          opacity: active || selected || state.hovered ? 1 : .55,
          child: Padding(
            padding: EdgeInsets.fromLTRB(compact ? 10 : 30, 8, 6, 8),
            child: Row(
              children: [
                StatusDot(color: p.tone(dockerStateTone(container.state)), size: 9),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Flexible(
                            flex: 3,
                            child: Text(
                              container.name,
                              key: ValueKey('docker-name-${container.id}'),
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: tokens.typography.bodyEmph.copyWith(
                                color: active ? p.label : tokens.secondaryLabel,
                              ),
                            ),
                          ),
                          if (container.composeService case final service?) ...[
                            const SizedBox(width: 6),
                            Flexible(
                              child: Text('($service)', maxLines: 1, overflow: TextOverflow.ellipsis, style: caption),
                            ),
                          ],
                        ],
                      ),
                      const SizedBox(height: 2),
                      Text(
                        compact ? container.image : '${container.image} · ${container.shortId}',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: tokens.typography.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel),
                      ),
                      const SizedBox(height: 4),
                      Wrap(
                        spacing: 6,
                        runSpacing: 4,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        children: [
                          GlassBadge(
                            key: ValueKey('docker-state-${container.id}'),
                            label: dockerStateLabel(l, container.state),
                            tone: dockerStateTone(container.state),
                            dense: true,
                          ),
                          if (health != null)
                            GlassBadge(
                              key: ValueKey('docker-health-${container.id}'),
                              label: health.$1,
                              tone: health.$2,
                              icon: Icons.monitor_heart_outlined,
                              dense: true,
                            ),
                          if (container.status.isNotEmpty)
                            ConstrainedBox(
                              constraints: BoxConstraints(maxWidth: compact ? 180 : 260),
                              child: Text(
                                container.status,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: caption,
                              ),
                            ),
                          for (final port in ports)
                            Text(
                              port,
                              key: ValueKey('docker-port-${container.id}-$port'),
                              style: tokens.typography.mono.copyWith(fontSize: 11, color: p.accent),
                            ),
                        ],
                      ),
                    ],
                  ),
                ),
                if (busy)
                  const Padding(
                    padding: EdgeInsets.symmetric(horizontal: 8),
                    child: SizedBox.square(dimension: 14, child: CircularProgressIndicator(strokeWidth: 2)),
                  ),
                if (!compact) ...[
                  if (dockerActionAllowed(DockerAction.start, container.state))
                    quick(
                      'start',
                      Icons.play_arrow_rounded,
                      l.dockerActionStart,
                      () => unawaited(actions.run(container, DockerAction.start)),
                      color: p.success,
                    )
                  else if (container.state == DockerContainerState.paused)
                    quick(
                      'unpause',
                      Icons.play_circle_outline_rounded,
                      l.dockerActionUnpause,
                      () => unawaited(actions.run(container, DockerAction.unpause)),
                    )
                  else
                    quick(
                      'stop',
                      Icons.stop_rounded,
                      l.dockerActionStop,
                      dockerActionAllowed(DockerAction.stop, container.state)
                          ? () => unawaited(actions.run(container, DockerAction.stop))
                          : null,
                    ),
                  quick(
                    'restart',
                    Icons.restart_alt_rounded,
                    l.dockerActionRestart,
                    dockerActionAllowed(DockerAction.restart, container.state)
                        ? () => unawaited(actions.run(container, DockerAction.restart))
                        : null,
                  ),
                  quick('logs', Icons.article_outlined, l.dockerLogs, () => actions.logs(container)),
                  quick(
                    'terminal',
                    Icons.terminal_rounded,
                    l.dockerTerminal,
                    running ? () => unawaited(actions.terminal(container)) : null,
                  ),
                  if (database != null)
                    quick(
                      'database',
                      Icons.storage_rounded,
                      l.kubeConnectDatabase,
                      running ? () => unawaited(actions.database!(container, database)) : null,
                    ),
                ],
                GlassMenuButton<Object>(
                  key: ValueKey('docker-menu-${container.id}'),
                  entries: dockerContainerMenu(l, container, database: database),
                  onSelected: (value) => runDockerMenuValue(actions, container, value, database: database),
                  builder: (context, open) => GlassIconButton(
                    key: ValueKey('docker-menu-button-${container.id}'),
                    icon: Icons.more_horiz_rounded,
                    tooltip: MaterialLocalizations.of(context).moreButtonTooltip,
                    size: 28,
                    iconSize: 16,
                    style: GlassIconButtonStyle.plain,
                    onPressed: open,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
    return GestureDetector(
      onSecondaryTapUp: (details) => _menuAt(context, details.globalPosition),
      onLongPressStart: (details) => _menuAt(context, details.globalPosition),
      child: row,
    );
  }
}
