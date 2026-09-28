import 'package:consolecrypt/ai/ai_chat_screen.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/symbols.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/snippets/snippets_screen.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Small permanent rail on the right: helpers never replace the workspace.
class WorkspaceToolRail extends ConsumerWidget {
  const WorkspaceToolRail({super.key, this.integrated = false});
  final bool integrated;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final selected = ref.watch(workspaceToolsProvider).selected;
    final l = context.l10n;
    return Padding(
      padding: integrated ? EdgeInsets.zero : const EdgeInsets.fromLTRB(8, 8, 0, 8),
      child: GlassSurface(
        key: const ValueKey('workspace-tools-rail'),
        shape: integrated ? GlassRadii.shape(0) : null,
        tint: integrated ? GlassTokens.of(context).surfaces.contentSolid : null,
        shadows: !integrated,
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 10),
        child: Column(
          children: [
            for (final tool in WorkspaceTool.values) ...[
              GlassSidebarItem(
                key: ValueKey('nav-${tool.name}'),
                icon: tool == WorkspaceTool.ai ? Icons.auto_awesome_rounded : Icons.code_rounded,
                leading: AppSymbolIcon(tool == WorkspaceTool.ai ? AppSymbol.ai : AppSymbol.snippets),
                label: tool == WorkspaceTool.ai ? l.navAiChat : l.navSnippets,
                compact: true,
                selected: tool == selected,
                onPressed: () => ref.read(workspaceToolsProvider.notifier).toggle(tool),
              ),
              const SizedBox(height: 12),
            ],
          ],
        ),
      ),
    );
  }
}

class WorkspaceToolPanel extends ConsumerStatefulWidget {
  const WorkspaceToolPanel({super.key, this.integrated = false});
  final bool integrated;

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
    if (selected != null) _visited.add(selected);
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
                                  GlassSegment(
                                    key: const ValueKey('nav-snippets'),
                                    value: WorkspaceTool.snippets,
                                    label: l.navSnippets,
                                  ),
                                  GlassSegment(
                                    key: const ValueKey('nav-ai'),
                                    value: WorkspaceTool.ai,
                                    label: l.navAiChat,
                                  ),
                                ],
                                selected: selected ?? WorkspaceTool.snippets,
                                onChanged: (tool) => ref.read(workspaceToolsProvider.notifier).open(tool),
                              ),
                            )
                          else ...[
                            AppSymbolIcon(selected == WorkspaceTool.ai ? AppSymbol.ai : AppSymbol.snippets),
                            const SizedBox(width: 10),
                            Expanded(
                              child: Text(
                                selected == WorkspaceTool.ai ? l.navAiChat : l.navSnippets,
                                style: tokens.typography.title3.copyWith(color: tokens.palette.label),
                              ),
                            ),
                          ],
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
                            for (final tool in WorkspaceTool.values)
                              Offstage(
                                offstage: selected != tool,
                                child: TickerMode(
                                  enabled: selected == tool,
                                  child: ExcludeFocus(
                                    excluding: selected != tool,
                                    child: !_visited.contains(tool)
                                        ? const SizedBox.shrink()
                                        : tool == WorkspaceTool.ai
                                        ? const AiChatScreen(embedded: true)
                                        : const SnippetsScreen(embedded: true),
                                  ),
                                ),
                              ),
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
