import 'package:consolecrypt/ai/ai_chat_screen.dart';
import 'package:consolecrypt/app/features.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/symbols.dart';
import 'package:consolecrypt/app/tool_icons.dart';
import 'package:consolecrypt/app/workspace_forwards.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/kubernetes/kube_ai_host.dart';
import 'package:consolecrypt/kubernetes/kube_ai_panel.dart';
import 'package:consolecrypt/kubernetes/kube_forward_panel.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/notes/notes_panel.dart';
import 'package:consolecrypt/notes/notes_providers.dart';
import 'package:consolecrypt/snippets/snippets_screen.dart';
import 'package:consolecrypt/terminal/host_metrics_panel.dart';
import 'package:consolecrypt/terminal/session_tools.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:consolecrypt/terminal/tmux_panel.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

List<WorkspaceTool> get _generalTools => [
  for (final tool in WorkspaceTool.values)
    if (!const {WorkspaceTool.server, WorkspaceTool.docker, WorkspaceTool.tmux, WorkspaceTool.history}.contains(tool) &&
        (tool != WorkspaceTool.forwards || kubernetesEnabled || dockerEnabled))
      tool,
];

String workspaceToolLabel(AppLocalizations l, WorkspaceTool tool) => switch (tool) {
  WorkspaceTool.snippets => l.navSnippets,
  WorkspaceTool.notes => l.navNotes,
  WorkspaceTool.ai => l.navAiChat,
  WorkspaceTool.forwards => l.navPortForwards,
  WorkspaceTool.server => l.hostMetricsTitle,
  WorkspaceTool.history => l.settingsCommandHistoryLabel,
  WorkspaceTool.docker => l.dockerTitle,
  WorkspaceTool.tmux => l.tmuxTitle,
};

Widget _toolIcon(WorkspaceTool tool) => switch (tool) {
  WorkspaceTool.snippets => const AppSymbolIcon(AppSymbol.snippets),
  WorkspaceTool.notes => const AppSymbolIcon(AppSymbol.notes),
  WorkspaceTool.ai => const AppSymbolIcon(AppSymbol.ai),
  WorkspaceTool.forwards => const AppSymbolIcon(AppSymbol.tunnels),
  WorkspaceTool.history => const Icon(Icons.history_rounded, size: 18),
  WorkspaceTool.tmux => const Icon(Icons.layers_outlined, size: 18),
  WorkspaceTool.docker => const Icon(ToolIcons.docker, size: 18),
  WorkspaceTool.server => const Icon(Icons.show_chart_rounded, size: GlassSizes.iconToolbar),
};

/// Small permanent rail on the right: helpers never replace the workspace.
class WorkspaceToolRail extends ConsumerWidget {
  const WorkspaceToolRail({super.key, required this.branch, this.integrated = false});
  final ShellBranch branch;
  final bool integrated;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selected = ref.watch(workspaceToolsProvider).selected;
    final l = context.l10n;
    final scope = ref.watch(kubeScopeProvider);
    final docker = dockerEnabled && ref.watch(dockerServiceProvider) != null
        ? (ref.watch(dockerForwardsProvider).value ?? const [])
              .where((f) => f.state == DockerForwardState.running)
              .length
        : 0;
    final forwards =
        docker +
        (kubernetesEnabled && scope.unlocked
            ? (ref.watch(kubeForwardsProvider(scope)).value ?? const []).where((f) => f.state == 'running').length
            : 0);
    final notes = ref.watch(notesAttentionProvider);
    final tab = branch == ShellBranch.terminal ? ref.watch(activeSshTerminalProvider) : null;
    final history = branch == ShellBranch.terminal && ref.watch(terminalTabsProvider).active?.hasCommandHistory == true;
    final dockerHere = tab != null && dockerEnabled && ref.watch(sessionHasDockerProvider(tab)).value == true;
    return Padding(
      padding: integrated ? EdgeInsets.zero : const EdgeInsets.fromLTRB(4, 8, 0, 8),
      child: GlassSurface(
        key: const ValueKey('workspace-tools-rail'),
        shape: integrated ? GlassRadii.shape(0) : null,
        tint: integrated ? GlassTokens.of(context).surfaces.contentSolid : null,
        shadows: !integrated,
        padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s2, vertical: GlassSpacing.s8),
        child: Column(
          children: [
            for (final tool in [
              ..._generalTools,
              if (history) WorkspaceTool.history,
              if (dockerHere) WorkspaceTool.docker,
              if (tab != null && ref.watch(tmuxVersionProvider(tab.tmuxParent ?? tab)).value != null)
                WorkspaceTool.tmux,
              if (tab != null) WorkspaceTool.server,
            ]) ...[
              GlassSidebarItem(
                key: ValueKey('nav-${tool.name}'),
                icon: Icons.code_rounded,
                leading: _toolIcon(tool),
                label: workspaceToolLabel(l, tool),
                badge: switch (tool) {
                  WorkspaceTool.forwards when forwards > 0 => '$forwards',
                  WorkspaceTool.notes when notes > 0 => '$notes',
                  _ => null,
                },
                badgeTone: tool == WorkspaceTool.notes ? GlassTone.warning : GlassTone.success,
                compact: true,
                selected: tool == selected,
                onPressed: () => ref.read(workspaceToolsProvider.notifier).toggle(tool),
              ),
              const SizedBox(height: GlassSpacing.s8),
            ],
          ],
        ),
      ),
    );
  }
}

class WorkspaceToolPanel extends ConsumerStatefulWidget {
  const WorkspaceToolPanel({super.key, this.integrated = false, this.branch = ShellBranch.hosts});
  final bool integrated;

  /// The section on screen: notes follow its open object.
  final ShellBranch branch;

  @override
  ConsumerState<WorkspaceToolPanel> createState() => _WorkspaceToolPanelState();
}

class _WorkspaceToolPanelState extends ConsumerState<WorkspaceToolPanel> {
  final _visited = <WorkspaceTool>{};
  final _focus = FocusScopeNode(debugLabel: 'workspace tools'); // l10n-ignore: focus debug label

  @override
  void dispose() {
    _focus.dispose();
    super.dispose();
  }

  void _close() {
    _focus.unfocus();
    ref.read(workspaceToolsProvider.notifier).close();
  }

  @override
  Widget build(BuildContext context) {
    final selected = ref.watch(workspaceToolsProvider).selected;
    ref.listen(workspaceToolsProvider.select((s) => s.selected), (_, next) {
      if (next != null) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) _focus.requestFocus();
        });
      }
    });
    if (selected != null && _generalTools.contains(selected)) _visited.add(selected);
    final tab = widget.branch == ShellBranch.terminal ? ref.watch(activeSshTerminalProvider) : null;
    final active = widget.branch == ShellBranch.terminal ? ref.watch(terminalTabsProvider).active : null;
    final tools = [
      if (active?.hasCommandHistory == true) WorkspaceTool.history,
      if (tab != null && dockerEnabled && ref.watch(sessionHasDockerProvider(tab)).value == true) WorkspaceTool.docker,
      if (tab != null && ref.watch(tmuxVersionProvider(tab.tmuxParent ?? tab)).value != null) WorkspaceTool.tmux,
      if (tab != null) WorkspaceTool.server,
    ];
    if (selected != null && !_generalTools.contains(selected) && !tools.contains(selected)) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && ref.read(workspaceToolsProvider).selected == selected) _close();
      });
    }
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return Offstage(
      offstage: selected == null,
      child: ExcludeFocus(
        excluding: selected == null,
        child: CallbackShortcuts(
          bindings: {const SingleActivator(LogicalKeyboardKey.escape): _close},
          child: FocusScope(
            node: _focus,
            child: Padding(
              padding: widget.integrated ? EdgeInsets.zero : const EdgeInsets.only(top: 8),
              child: GlassSurface(
                key: const ValueKey('workspace-tools-panel'),
                // Keep the same clip widget type when switching styles so drafts survive.
                shape: widget.integrated ? GlassRadii.shape(0) : null,
                tint: widget.integrated ? tokens.surfaces.contentSolid : null,
                shadows: !widget.integrated,
                child: Column(
                  children: [
                    Padding(
                      padding: const EdgeInsets.fromLTRB(14, 10, 8, 10),
                      child: Row(
                        children: [
                          if (widget.integrated)
                            Expanded(
                              child: GlassSegmented<WorkspaceTool>(
                                key: const ValueKey('workspace-tools-tabs'),
                                inChrome: false,
                                expand: true,
                                semanticLabel: l.settingsWorkspacePanel,
                                segments: [
                                  for (final tool in _generalTools)
                                    GlassSegment(
                                      key: ValueKey('nav-${tool.name}'),
                                      value: tool,
                                      label: workspaceToolLabel(l, tool),
                                    ),
                                ],
                                selected: selected ?? WorkspaceTool.snippets,
                                onChanged: (tool) => ref.read(workspaceToolsProvider.notifier).open(tool),
                              ),
                            )
                          else ...[
                            _toolIcon(selected ?? WorkspaceTool.snippets),
                            const SizedBox(width: 10),
                            Expanded(
                              child: Text(
                                workspaceToolLabel(l, selected ?? WorkspaceTool.snippets),
                                style: tokens.typography.title3.copyWith(color: tokens.palette.label),
                              ),
                            ),
                          ],
                          if (widget.integrated)
                            for (final tool in tools)
                              GlassIconButton(
                                key: ValueKey('nav-${tool.name}'),
                                selected: selected == tool,
                                icon: switch (tool) {
                                  WorkspaceTool.docker => ToolIcons.docker,
                                  WorkspaceTool.tmux => Icons.layers_outlined,
                                  WorkspaceTool.history => Icons.history_rounded,
                                  _ => Icons.show_chart_rounded,
                                },
                                tooltip: workspaceToolLabel(l, tool),
                                size: 28,
                                iconSize: 16,
                                style: GlassIconButtonStyle.plain,
                                onPressed: () => ref.read(workspaceToolsProvider.notifier).toggle(tool),
                              ),
                          GlassIconButton(
                            key: const ValueKey('workspace-tools-close'),
                            icon: widget.integrated && !AppPlatform.isMobile
                                ? Icons.chevron_right_rounded
                                : Icons.close_rounded,
                            tooltip: widget.integrated ? l.workspaceToolsCollapse : l.workspaceToolsClose,
                            style: GlassIconButtonStyle.plain,
                            onPressed: _close,
                          ),
                        ],
                      ),
                    ),
                    Expanded(
                      child: ColoredBox(
                        color: tokens.surfaces.contentSolid,
                        child: Stack(
                          fit: StackFit.expand,
                          children: [
                            for (final tool in _generalTools)
                              Offstage(
                                offstage: selected != tool,
                                child: TickerMode(
                                  enabled: selected == tool,
                                  child: ExcludeFocus(
                                    excluding: selected != tool,
                                    child: !_visited.contains(tool)
                                        ? const SizedBox.shrink()
                                        : switch (tool) {
                                            WorkspaceTool.ai => const _AiTool(),
                                            WorkspaceTool.snippets => const SnippetsScreen(embedded: true),
                                            WorkspaceTool.notes => NotesPanel(branch: widget.branch),
                                            WorkspaceTool.forwards => const WorkspaceForwardsPanel(),
                                            WorkspaceTool.server ||
                                            WorkspaceTool.history ||
                                            WorkspaceTool.tmux ||
                                            WorkspaceTool.docker => const SizedBox.shrink(),
                                          },
                                  ),
                                ),
                              ),
                            if (selected == WorkspaceTool.server && tab != null)
                              HostMetricsPanel(
                                key: ValueKey(tab.sessionId),
                                hostName: tab.host?.name,
                                address: tab.host?.address,
                                description: tab.host?.notes,
                                history: tab.hostMetrics,
                              ),
                            if (selected == WorkspaceTool.history && active != null && tools.contains(selected))
                              SessionHistoryPanel(key: ValueKey(active.sessionId), tab: active.tmuxParent ?? active),
                            if (selected == WorkspaceTool.tmux && tab != null && tools.contains(selected))
                              TmuxPanel(
                                key: ValueKey((tab.tmuxParent ?? tab).sessionId),
                                parent: tab.tmuxParent ?? tab,
                              ),
                            if (selected == WorkspaceTool.docker && tab != null && tools.contains(selected))
                              SessionDockerPanel(key: ValueKey(tab.sessionId), tab: tab),
                          ],
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class WorkspaceToolResizeHandle extends ConsumerWidget {
  const WorkspaceToolResizeHandle({required this.width, super.key});
  final double width;

  @override
  Widget build(BuildContext context, WidgetRef ref) => Semantics(
    label: context.l10n.workspaceToolsResize,
    onIncrease: () => ref.read(workspaceToolsProvider.notifier).resize(width + 20),
    onDecrease: () => ref.read(workspaceToolsProvider.notifier).resize(width - 20),
    child: MouseRegion(
      cursor: SystemMouseCursors.resizeColumn,
      child: GestureDetector(
        key: const ValueKey('workspace-tools-resize'),
        behavior: HitTestBehavior.opaque,
        onHorizontalDragStart: (_) => ref.read(workspaceToolsProvider.notifier).resize(width),
        onHorizontalDragUpdate: (details) =>
            ref.read(workspaceToolsProvider.notifier).resize(ref.read(workspaceToolsProvider).width - details.delta.dx),
        child: Center(
          child: Container(
            width: 2,
            height: 40,
            decoration: BoxDecoration(
              color: GlassTokens.of(context).surfaces.separator,
              borderRadius: BorderRadius.circular(2),
            ),
          ),
        ),
      ),
    ),
  );
}

/// General AI chat, or the connected cluster's assistant while a Kubernetes
/// workspace is open (switchable). Both stay mounted so drafts survive.
class _AiTool extends ConsumerWidget {
  const _AiTool();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final host = ref.watch(kubeAiHostProvider);
    final cluster = host != null && ref.watch(kubeAiClusterModeProvider);
    final l = context.l10n;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (host != null)
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
            child: GlassSegmented<bool>(
              key: const ValueKey('workspace-ai-mode'),
              inChrome: false,
              expand: true,
              semanticLabel: l.navAiChat,
              segments: [
                GlassSegment(
                  key: const ValueKey('workspace-ai-cluster'),
                  value: true,
                  label: l.kubeAiClusterChat(host.contextName),
                ),
                GlassSegment(key: const ValueKey('workspace-ai-general'), value: false, label: l.kubeAiGeneralChat),
              ],
              selected: cluster,
              onChanged: (value) => ref.read(kubeAiClusterModeProvider.notifier).set(value),
            ),
          ),
        Expanded(
          child: Stack(
            fit: StackFit.expand,
            children: [
              Offstage(offstage: cluster, child: const AiChatScreen(embedded: true)),
              if (host != null)
                Offstage(
                  offstage: !cluster,
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(12, 4, 12, 12),
                    child: KubeAiPanel(
                      key: ValueKey(('kube-ai', host.session.id, host.request)),
                      session: host.session,
                      contextName: host.contextName,
                      service: host.service,
                      selected: host.selected,
                      initialQuestion: host.question,
                      namespace: host.namespace,
                      onReviewedCommand: host.onReviewedCommand,
                    ),
                  ),
                ),
            ],
          ),
        ),
      ],
    );
  }
}
