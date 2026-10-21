import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
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
  });
  final Future<void> Function(DockerContainer container, DockerAction action) run;
  final void Function(DockerContainer container) logs, details;
  final Future<void> Function(DockerContainer container) terminal, forward, copyId;
  final bool Function(DockerContainer container) busy;
}

/// Menu values beyond the engine actions.
enum DockerMenuCommand { details, logs, terminal, forward, copyId }

/// Every action of one container, for the row menu, the context menu and
/// the drawer.
List<GlassMenuEntry<Object>> dockerContainerMenu(AppLocalizations l, DockerContainer container) {
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
void runDockerMenuValue(DockerContainerActions actions, DockerContainer container, Object value) {
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
    case DockerMenuCommand.copyId:
      unawaited(actions.copyId(container));
  }
}

/// Containers grouped by compose project; groups collapse.
class DockerContainerList extends StatelessWidget {
  const DockerContainerList({
    required this.groups,
    required this.collapsed,
    required this.actions,
    required this.onToggleGroup,
    this.selectedId,
    super.key,
  });
  final List<DockerContainerGroup> groups;
  final Set<String> collapsed;
  final DockerContainerActions actions;
  final ValueChanged<String> onToggleGroup;
  final String? selectedId;

  @override
  Widget build(BuildContext context) {
    final rows = <Object>[
      for (final group in groups) ...[group, if (!collapsed.contains(group.key)) ...group.containers],
    ];
    return ContentSurface(
      padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s4),
      child: RepaintBoundary(
        child: ScrollEdgeEffect(
          bottom: true,
          extent: 12,
          child: LayoutBuilder(
            builder: (context, constraints) => ListView.builder(
              key: const ValueKey('docker-containers'),
              padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s4),
              itemCount: rows.length,
              itemBuilder: (context, index) => switch (rows[index]) {
                final DockerContainerGroup group => _GroupHeader(
                  group: group,
                  collapsed: collapsed.contains(group.key),
                  onToggle: () => onToggleGroup(group.key),
                ),
                final DockerContainer container => DockerContainerRow(
                  container: container,
                  compact: constraints.maxWidth < 560,
                  selected: container.id == selectedId,
                  busy: actions.busy(container),
                  actions: actions,
                ),
                _ => const SizedBox.shrink(),
              },
            ),
          ),
        ),
      ),
    );
  }
}

class _GroupHeader extends StatelessWidget {
  const _GroupHeader({required this.group, required this.collapsed, required this.onToggle});
  final DockerContainerGroup group;
  final bool collapsed;
  final VoidCallback onToggle;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final compose = group.project != null;
    return GlassInteractive(
      key: ValueKey('docker-group-${group.key}'),
      semanticLabel: group.project ?? l.dockerStandalone,
      onPressed: onToggle,
      builder: (context, state) => DecoratedBox(
        decoration: ShapeDecoration(
          color: state.hovered ? tokens.surfaces.fillHover : Colors.transparent,
          shape: GlassRadii.shape(tokens.radii.row),
        ),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(6, 10, 12, 6),
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
            ],
          ),
        ),
      ),
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
      entries: dockerContainerMenu(context.l10n, container),
    );
    if (value != null && context.mounted) runDockerMenuValue(actions, container, value);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final running = container.state == DockerContainerState.running;
    final health = dockerHealth(l, container.health);
    final ports = formatDockerPorts(container.ports);
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
              : Colors.transparent,
          shape: GlassRadii.shape(tokens.radii.row),
        ),
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
                            style: tokens.typography.bodyEmph.copyWith(color: p.label),
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
                            child: Text(container.status, maxLines: 1, overflow: TextOverflow.ellipsis, style: caption),
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
              ],
              GlassMenuButton<Object>(
                key: ValueKey('docker-menu-${container.id}'),
                entries: dockerContainerMenu(l, container),
                onSelected: (value) => runDockerMenuValue(actions, container, value),
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
    );
    return GestureDetector(
      onSecondaryTapUp: (details) => _menuAt(context, details.globalPosition),
      onLongPressStart: (details) => _menuAt(context, details.globalPosition),
      child: row,
    );
  }
}
