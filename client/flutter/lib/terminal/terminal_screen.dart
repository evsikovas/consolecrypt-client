import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/ai/command_palette.dart';
import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/sidebar_state.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/teams/team_providers.dart';
import 'package:consolecrypt/terminal/terminal_context_menu.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:consolecrypt/terminal/terminal_target_picker.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

class TerminalScreen extends ConsumerStatefulWidget {
  const TerminalScreen({super.key});

  @override
  ConsumerState<TerminalScreen> createState() => _TerminalScreenState();
}

class _TerminalScreenState extends ConsumerState<TerminalScreen> {
  /// Keeps the terminal panes (focus, selection, scroll) when the tabs move
  /// between the row and the column.
  final _card = GlobalKey();
  late final SidebarCollapsed _sidebar;

  @override
  void initState() {
    super.initState();
    _sidebar = ref.read(sidebarCollapsedProvider.notifier);
  }

  @override
  void dispose() {
    final sidebar = _sidebar;
    scheduleMicrotask(() => sidebar.setBesideTabs(false));
    super.dispose();
  }

  Future<void> _newTab() async {
    final profile = ref.read(activeProfileProvider)?.id;
    final target = await showTerminalTargetPicker(context);
    if (target == null ||
        !mounted ||
        profile != ref.read(activeProfileProvider)?.id ||
        ref.read(appStageProvider) != AppStage.unlocked) {
      return;
    }
    await runWithFeedback(context, () => ref.read(terminalTabsProvider.notifier).openTarget(target));
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(terminalTabsProvider);
    // Offstage shell branches disable tickers: that is "not on screen" for
    // the unseen-activity markers.
    final visible = TickerMode.valuesOf(context).enabled;
    final placement = AppPlatform.isMobile
        ? TerminalTabPlacement.top
        : ref.watch(localSettingsProvider.select((s) => s.value?.terminalTabPlacement)) ?? TerminalTabPlacement.top;
    final column = placement == TerminalTabPlacement.left && state.tabs.isNotEmpty;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      ref.read(terminalTabsProvider.notifier).setViewing(visible);
      // The app sidebar folds next to the tab column (the user's choice wins).
      _sidebar.setBesideTabs(visible && column);
    });
    final l10n = context.l10n;
    final pad = AppPlatform.isMobile ? 6.0 : pagePadding(context);
    final Widget body;
    if (state.tabs.isEmpty) {
      body = Material(
        type: MaterialType.transparency,
        child: EmptyState(
          icon: Icons.terminal_rounded,
          title: l10n.terminalEmptyTitle,
          message: AppPlatform.isMobile
              ? l10n.terminalEmptyMessage(
                  commandFor(AppCommandId.newTerminalTab, ref.watch(appCommandsProvider)).shortcutLabel,
                )
              : l10n.terminalLocalEmptyMessage,
          action: GlassButton.prominent(
            key: const ValueKey('terminal-connect'),
            size: GlassControlSize.lg,
            onPressed: _newTab,
            icon: Icons.add_rounded,
            label: AppPlatform.isMobile ? l10n.terminalConnectToHost : l10n.terminalLocalOpen,
          ),
        ),
      );
    } else {
      final active = state.active!;
      final card = Expanded(
        child: _TerminalCard(key: _card, state: state, active: active),
      );
      // Tab strip above the card (or the tab column beside it), banners
      // between them, the terminal card fully opaque below (§4.9). Context
      // menus are transient user actions.
      body = Material(
        type: MaterialType.transparency,
        child: Padding(
          padding: EdgeInsets.fromLTRB(pad, GlassSpacing.s8, pad, pad),
          child: column
              ? Row(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _TerminalTabs(
                      state: state,
                      active: active,
                      onNewTab: _newTab,
                      placement: placement,
                      // The terminal keeps at least 360 px beside the column
                      // (the shell sizes MediaQuery to the content area).
                      maxColumnWidth: MediaQuery.sizeOf(context).width - 2 * pad - 360 - GlassTabColumn.handleWidth,
                    ),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          _SessionBar(tab: active),
                          const SizedBox(height: GlassSpacing.s8),
                          _SessionBanners(tab: active),
                          card,
                        ],
                      ),
                    ),
                  ],
                )
              : Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _TerminalTabs(state: state, active: active, onNewTab: _newTab, placement: placement),
                    const SizedBox(height: GlassSpacing.s8),
                    _SessionBanners(tab: active),
                    card,
                    if (AppPlatform.isMobile) _MobileTerminalKeys(tab: active),
                  ],
                ),
        ),
      );
    }
    // Streaming output repaints every frame: all chrome stays static while
    // the terminal is visible (§3 rule 1, §6.6).
    return GlassBlurSuppressor(budget: GlassScope.of(context).budget, child: body);
  }
}

/// The open terminals as the tab strip above the terminal or, on desktop, as
/// the resizable tab column at its left edge (Settings → Terminal).
class _TerminalTabs extends ConsumerWidget {
  const _TerminalTabs({
    required this.state,
    required this.active,
    required this.onNewTab,
    required this.placement,
    this.maxColumnWidth = LocalSettings.terminalTabColumnMax,
  });

  final TerminalTabsState state;
  final TerminalTab active;
  final VoidCallback onNewTab;
  final TerminalTabPlacement placement;
  final double maxColumnWidth;

  static GlassTabState _state(TerminalTab tab) => switch (tab.state) {
    SessionConnectionState.connected => GlassTabState.connected,
    SessionConnectionState.connecting ||
    SessionConnectionState.reconnecting ||
    SessionConnectionState.awaitingHostKey ||
    SessionConnectionState.awaitingPassword => GlassTabState.reconnecting,
    SessionConnectionState.disconnected => GlassTabState.disconnected,
  };

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final strings = GlassStrings.of(context);
    final controller = ref.read(terminalTabsProvider.notifier);
    final subtitleMode =
        ref.watch(localSettingsProvider.select((s) => s.value?.terminalTabSubtitle)) ?? TerminalTabSubtitle.name;
    final tabs = [
      for (final (i, tab) in state.tabs.indexed)
        GlassTab(
          key: ValueKey('tab-${tab.target.label}-$i'),
          title: tab.displayTitle,
          teamColor: switch (tab.host) {
            final host? => teamColor(teamOf(ref, host.id)?.color),
            null => null,
          },
          attention: tab.anyAttention,
          subtitle: _tabSubtitle(context, tab, subtitleMode),
          state: _state(tab),
          tooltip: switch (_state(tab)) {
            GlassTabState.connected => strings.tabConnected,
            GlassTabState.reconnecting => strings.tabReconnecting,
            GlassTabState.disconnected => strings.tabDisconnected,
            GlassTabState.none => null,
          },
        ),
    ];
    final activeIndex = state.activeIndex.clamp(0, state.tabs.length - 1);
    void menu(int i, Offset position) => unawaited(_tabMenu(context, ref, state.tabs[i], i, position));
    if (placement == TerminalTabPlacement.left) {
      final layout = ref.watch(
        localSettingsProvider.select((s) => (s.value?.terminalTabColumnWidth, s.value?.terminalTabColumnCollapsed)),
      );
      final settings = ref.read(settingsServiceProvider);
      return GlassTabColumn(
        key: const ValueKey('terminal-tab-column'),
        tabs: tabs,
        activeIndex: activeIndex,
        width: layout.$1 ?? LocalSettings.terminalTabColumnDefault,
        minWidth: LocalSettings.terminalTabColumnMin,
        maxWidth: maxColumnWidth.clamp(0, LocalSettings.terminalTabColumnMax),
        collapsed: layout.$2 ?? false,
        onSelect: controller.activate,
        onClose: (i) => controller.closeTab(state.tabs[i]),
        onContextMenu: menu,
        onAdd: onNewTab,
        onReorder: controller.moveTab,
        onResize: (width) => runWithFeedback(
          context,
          () => settings.updateLocal(settings.currentLocal.copyWith(terminalTabColumnWidth: width)),
        ),
        onCollapsedChanged: (collapsed) => runWithFeedback(
          context,
          () => settings.updateLocal(settings.currentLocal.copyWith(terminalTabColumnCollapsed: collapsed)),
        ),
      );
    }
    return GlassTabStrip(
      activeIndex: activeIndex,
      tabs: tabs,
      onSelect: controller.activate,
      onClose: (i) => controller.closeTab(state.tabs[i]),
      onContextMenu: menu,
      onAdd: onNewTab,
      onReorder: controller.moveTab,
      trailing: AppPlatform.isMobile ? null : _SessionToolbar(tab: active),
    );
  }
}

/// Second line of a terminal tab: the host's list name (or its address when
/// it has none), its address / this computer's network name, or nothing.
String? _tabSubtitle(BuildContext context, TerminalTab tab, TerminalTabSubtitle mode) {
  final host = tab.host;
  String? text = switch (mode) {
    TerminalTabSubtitle.off => null,
    TerminalTabSubtitle.name when tab.isLocal => context.l10n.terminalLocalComputer,
    TerminalTabSubtitle.address when tab.isLocal => Platform.localHostname,
    TerminalTabSubtitle.name => host == null ? null : (host.name.trim().isEmpty ? host.address : host.name),
    TerminalTabSubtitle.address => host?.address,
  };
  text = text?.trim();
  return text == null || text.isEmpty || text == tab.displayTitle ? null : text;
}

/// Thin bar above the terminal next to the tab column: the active tab's
/// title and subtitle (also while the column is folded to icons) and the
/// session tools of the tab-strip row.
class _SessionBar extends ConsumerWidget {
  const _SessionBar({required this.tab});

  final TerminalTab tab;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = GlassTokens.of(context);
    final subtitleMode =
        ref.watch(localSettingsProvider.select((s) => s.value?.terminalTabSubtitle)) ?? TerminalTabSubtitle.name;
    final subtitle = _tabSubtitle(context, tab, subtitleMode);
    return SizedBox(
      key: const ValueKey('terminal-session-bar'),
      height: GlassSizes.tabTrack,
      child: Row(
        children: [
          const SizedBox(width: GlassSpacing.s4),
          Expanded(
            child: Text.rich(
              TextSpan(
                children: [
                  TextSpan(
                    text: tab.displayTitle,
                    style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                  ),
                  if (subtitle != null)
                    TextSpan(
                      text: '  $subtitle',
                      style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                    ),
                ],
              ),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
          const SizedBox(width: GlassSpacing.toolbarGroupGap),
          _SessionToolbar(tab: tab),
        ],
      ),
    );
  }
}

Future<void> _tabMenu(BuildContext context, WidgetRef ref, TerminalTab tab, int index, Offset position) async {
  final l10n = context.l10n;
  final controller = ref.read(terminalTabsProvider.notifier);
  final count = ref.read(terminalTabsProvider).tabs.length;
  final action = await showGlassMenu<String>(
    context: context,
    anchor: position & const Size(1, 1),
    entries: [
      GlassMenuItem(
        key: const ValueKey('terminal-tab-rename'),
        value: 'rename',
        label: l10n.terminalTabRename,
        icon: Icons.drive_file_rename_outline,
      ),
      if (tab.customTitle != null)
        GlassMenuItem(value: 'reset', label: l10n.terminalTabResetName, icon: Icons.undo_rounded),
      GlassMenuItem(
        key: const ValueKey('terminal-tab-duplicate'),
        value: 'duplicate',
        label: l10n.terminalTabDuplicate,
        icon: Icons.copy_all_outlined,
      ),
      const GlassMenuDivider(),
      GlassMenuItem(
        key: const ValueKey('terminal-tab-close'),
        value: 'close',
        label: l10n.terminalTabClose,
        icon: Icons.close_rounded,
      ),
      GlassMenuItem(value: 'others', label: l10n.terminalTabCloseOthers, enabled: count > 1),
      GlassMenuItem(value: 'right', label: l10n.terminalTabCloseRight, enabled: index < count - 1),
    ],
  );
  if (!context.mounted || action == null) return;
  switch (action) {
    case 'rename':
      final name = await showTextInputDialog(context, title: l10n.terminalTabRenameTitle, initial: tab.displayTitle);
      if (name != null) controller.rename(tab, name);
    case 'reset':
      controller.rename(tab, null);
    case 'duplicate':
      await controller.duplicate(tab);
    case 'close':
      await controller.closeTab(tab);
    case 'others':
      await controller.closeOthers(tab);
    case 'right':
      await controller.closeToRight(tab);
  }
}

/// Session actions: one static glass capsule group in the tab-strip row.
class _SessionToolbar extends ConsumerWidget {
  const _SessionToolbar({required this.tab});

  final TerminalTab tab;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    return GlassToolbarGroup(
      children: [
        GlassIconButton(
          key: const ValueKey('terminal-snippets'),
          icon: Icons.code_rounded,
          tooltip: l10n.terminalSnippets,
          style: GlassIconButtonStyle.plain,
          size: GlassSizes.tab,
          iconSize: GlassSizes.iconRow,
          onPressed: () => ref.read(workspaceToolsProvider.notifier).toggle(WorkspaceTool.snippets),
        ),
        GlassIconButton(
          key: const ValueKey('terminal-ask-ai'),
          icon: Icons.auto_awesome_rounded,
          tooltip: l10n.terminalAskAi,
          style: GlassIconButtonStyle.plain,
          size: GlassSizes.tab,
          iconSize: GlassSizes.iconRow,
          onPressed: () => showCommandPalette(
            context,
            selectedText: tab.selectedText,
            hostId: tab.host?.id,
            shellTarget: tab.isLocal ? tab.target.shellType : null,
          ),
        ),
        if (tab.host case final host?)
          GlassIconButton(
            key: const ValueKey('terminal-sftp'),
            icon: Icons.folder_copy_rounded,
            tooltip: l10n.navSftp,
            style: GlassIconButtonStyle.plain,
            size: GlassSizes.tab,
            iconSize: GlassSizes.iconRow,
            onPressed: () async {
              await ref.read(sftpControllerProvider.notifier).connect(host);
              if (context.mounted) context.go(AppRoutes.sftp);
            },
          ),
        if (!AppPlatform.isMobile) _SplitMenu(tab: tab),
      ],
    );
  }
}

/// Split the focused pane right or down (up to [TerminalTabsController.maxPanes]
/// per tab), or close it.
class _SplitMenu extends ConsumerWidget {
  const _SplitMenu({required this.tab});

  final TerminalTab tab;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final controller = ref.read(terminalTabsProvider.notifier);
    final commands = ref.watch(appCommandsProvider);
    final host = tab.owner ?? tab;
    final room = host.allPanes.length < TerminalTabsController.maxPanes;
    String shortcut(AppCommandId id) => commandFor(id, commands).shortcutLabel;
    return GlassMenuButton<String>(
      key: const ValueKey('terminal-split'),
      entries: [
        GlassMenuItem(
          value: 'right',
          label: l10n.terminalSplitRight,
          icon: Icons.vertical_split_rounded,
          shortcut: shortcut(AppCommandId.splitTerminalRight),
          enabled: room,
        ),
        GlassMenuItem(
          value: 'down',
          label: l10n.terminalSplitDown,
          icon: Icons.horizontal_split_rounded,
          shortcut: shortcut(AppCommandId.splitTerminalDown),
          enabled: room,
        ),
        if (host.panes.isNotEmpty) ...[
          GlassMenuItem(
            value: 'next',
            label: l10n.terminalNextPane,
            icon: Icons.switch_left_rounded,
            shortcut: shortcut(AppCommandId.nextTerminalPane),
          ),
          const GlassMenuDivider(),
          GlassMenuItem(value: 'close', label: l10n.terminalClosePane, icon: Icons.close_rounded),
        ],
      ],
      onSelected: (action) => switch (action) {
        'right' => unawaited(runWithFeedback(context, () => controller.split(tab, Axis.horizontal))),
        'down' => unawaited(runWithFeedback(context, () => controller.split(tab, Axis.vertical))),
        'next' => controller.focusNextPane(),
        _ => unawaited(controller.close(tab)),
      },
      builder: (context, open) => GlassIconButton(
        icon: host.splitAxis == Axis.vertical && host.panes.isNotEmpty
            ? Icons.horizontal_split_rounded
            : Icons.vertical_split_rounded,
        tooltip: room ? l10n.terminalSplit : l10n.terminalSplitFull(TerminalTabsController.maxPanes),
        style: GlassIconButtonStyle.plain,
        size: GlassSizes.tab,
        iconSize: GlassSizes.iconRow,
        onPressed: open,
      ),
    );
  }
}

/// The terminal card (§4.9): the terminal theme background, 100 % opaque,
/// `r.card`, 10 px padding in the same colour, a hairline border; the
/// viewport itself is square. Connect-time prompts dock at its top on the
/// secure (opaque) material — they may overlap the terminal.
class _TerminalCard extends ConsumerWidget {
  const _TerminalCard({required this.state, required this.active, super.key});

  final TerminalTabsState state;
  final TerminalTab active;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = GlassTokens.of(context);
    final scheme = ref.watch(
      localSettingsProvider.select((s) => s.value?.terminalColorScheme ?? TerminalColorScheme.system),
    );
    final custom = ref.watch(localSettingsProvider.select((s) => s.value?.customTerminalColors));
    final theme = AppTheme.terminalTheme(Theme.of(context).brightness, scheme: scheme, custom: custom);
    return DecoratedBox(
      key: const ValueKey('terminal-card'),
      decoration: ShapeDecoration(
        color: theme.background,
        shape: GlassRadii.shape(tokens.radii.card).copyWith(side: BorderSide(color: tokens.surfaces.hairlineCard)),
      ),
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: IndexedStack(
          index: state.activeIndex.clamp(0, state.tabs.length - 1),
          sizing: StackFit.expand,
          children: [
            for (final tab in state.tabs)
              tab.panes.isEmpty
                  ? _PaneView(pane: tab, active: identical(tab, state.activeTab))
                  : _SplitPanes(
                      key: ValueKey('terminal-split-${tab.sessionId.value}'),
                      tab: tab,
                      current: identical(tab, state.activeTab),
                      foreground: theme.foreground,
                    ),
          ],
        ),
      ),
    );
  }
}

/// One session with its connect-time prompt docked at the top (the
/// unknown host key or the password); the prompt takes the keyboard.
class _PaneView extends StatelessWidget {
  const _PaneView({required this.pane, required this.active});

  final TerminalTab pane;
  final bool active;

  @override
  Widget build(BuildContext context) {
    final hostKey = pane.hostKey;
    final prompt = pane.passwordPrompt;
    final Widget? docked = hostKey != null && !hostKey.changed
        ? _HostKeyPromptCard(tab: pane, hostKey: hostKey)
        : prompt != null
        ? TerminalPasswordPromptCard(
            key: ValueKey('password-prompt-${pane.sessionId.value}'),
            tab: pane,
            prompt: prompt,
          )
        : null;
    return Stack(
      children: [
        Positioned.fill(
          child: TerminalPane(
            key: ValueKey('${pane.sessionId.value}:${pane.renderGeneration}'),
            tab: pane,
            active: active && docked == null,
          ),
        ),
        if (docked != null)
          Positioned(
            top: 2,
            left: 0,
            right: 0,
            child: Center(
              child: ConstrainedBox(constraints: const BoxConstraints(maxWidth: 520), child: docked),
            ),
          ),
      ],
    );
  }
}

/// A tab split into panes (TODO-07) along its axis: each pane has a slim
/// title row with its close button; a click gives it the keyboard; the
/// dividers resize neighbours.
class _SplitPanes extends ConsumerWidget {
  const _SplitPanes({required this.tab, required this.current, required this.foreground, super.key});

  final TerminalTab tab;
  final bool current;
  final Color foreground;

  static const _divider = 9.0;
  static const _minShare = .12;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    final controller = ref.read(terminalTabsProvider.notifier);
    final panes = tab.allPanes;
    final horizontal = tab.splitAxis == Axis.horizontal;
    return LayoutBuilder(
      builder: (context, box) {
        final total = (horizontal ? box.maxWidth : box.maxHeight) - _divider * (panes.length - 1);
        final children = <Widget>[];
        for (final (i, pane) in panes.indexed) {
          final focused = i == tab.focusedPane;
          if (i > 0) {
            children.add(
              MouseRegion(
                cursor: horizontal ? SystemMouseCursors.resizeColumn : SystemMouseCursors.resizeRow,
                child: GestureDetector(
                  key: ValueKey('terminal-pane-divider-$i'),
                  behavior: HitTestBehavior.opaque,
                  onPanUpdate: (details) {
                    if (total <= 0) return;
                    final delta = (horizontal ? details.delta.dx : details.delta.dy) / total;
                    final shares = [...tab.paneShares];
                    if (shares.length != panes.length) return;
                    final before = shares[i - 1] + delta, after = shares[i] - delta;
                    if (before < _minShare || after < _minShare) return;
                    shares[i - 1] = before;
                    shares[i] = after;
                    controller.resizePanes(tab, shares);
                  },
                  child: SizedBox(
                    width: horizontal ? _divider : null,
                    height: horizontal ? null : _divider,
                    child: Center(
                      child: Container(
                        width: horizontal ? 1 : null,
                        height: horizontal ? null : 1,
                        color: foreground.withValues(alpha: .18),
                      ),
                    ),
                  ),
                ),
              ),
            );
          }
          final share = i < tab.paneShares.length ? tab.paneShares[i] : 1 / panes.length;
          children.add(
            Expanded(
              flex: (share * 1000).round().clamp(1, 1000),
              child: Listener(
                key: ValueKey('terminal-pane-$i'),
                behavior: HitTestBehavior.translucent,
                onPointerDown: (_) => controller.focusPane(pane),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    SizedBox(
                      height: 22,
                      child: Row(
                        children: [
                          Container(
                            width: 6,
                            height: 6,
                            margin: const EdgeInsets.only(right: GlassSpacing.s6),
                            decoration: BoxDecoration(
                              shape: BoxShape.circle,
                              color: focused ? tokens.palette.accent : foreground.withValues(alpha: .25),
                            ),
                          ),
                          Expanded(
                            child: Text(
                              pane.displayTitle,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: tokens.typography.caption.copyWith(
                                color: foreground.withValues(alpha: focused ? .9 : .5),
                                fontWeight: focused ? FontWeight.w600 : FontWeight.w400,
                              ),
                            ),
                          ),
                          IconButton(
                            key: ValueKey('terminal-pane-close-$i'),
                            tooltip: l10n.terminalClosePane,
                            visualDensity: VisualDensity.compact,
                            padding: EdgeInsets.zero,
                            iconSize: 14,
                            constraints: const BoxConstraints.tightFor(width: 22, height: 22),
                            color: foreground.withValues(alpha: .6),
                            icon: const Icon(Icons.close_rounded),
                            onPressed: () => unawaited(controller.close(pane)),
                          ),
                        ],
                      ),
                    ),
                    Expanded(
                      child: _PaneView(pane: pane, active: current && focused),
                    ),
                  ],
                ),
              ),
            ),
          );
        }
        return Flex(direction: tab.splitAxis, crossAxisAlignment: CrossAxisAlignment.stretch, children: children);
      },
    );
  }
}

/// Unknown host key (TOFU with confirmation): a trust decision on the secure
/// material — host, fingerprint on `surface.inset`, reject / once / save.
class _HostKeyPromptCard extends ConsumerWidget {
  const _HostKeyPromptCard({required this.tab, required this.hostKey});

  final TerminalTab tab;
  final HostKeyInfo hostKey;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final controller = ref.read(terminalTabsProvider.notifier);
    return SecureSurface(
      key: const ValueKey('host-key-prompt'),
      radius: tokens.radii.card,
      padding: const EdgeInsets.all(GlassSpacing.card),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            children: [
              Icon(Icons.gpp_maybe_rounded, color: tokens.palette.warning, size: 22),
              const SizedBox(width: GlassSpacing.s8),
              Expanded(
                child: Text(
                  l10n.terminalUnknownHostTitle(hostKey.hostPattern),
                  style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                ),
              ),
            ],
          ),
          const SizedBox(height: GlassSpacing.s6),
          Text(l10n.terminalUnknownHostMessage),
          const SizedBox(height: GlassSpacing.s8),
          ContentSurface(
            kind: ContentSurfaceKind.inset,
            padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s12, vertical: GlassSpacing.s8),
            child: SelectableText(
              '${hostKey.keyType}  ${hostKey.fingerprintSha256}',
              style: tokens.typography.mono.copyWith(fontSize: 12, color: tokens.palette.label),
            ),
          ),
          const SizedBox(height: GlassSpacing.s12),
          Wrap(
            alignment: WrapAlignment.end,
            spacing: GlassSpacing.s8,
            runSpacing: GlassSpacing.s8,
            children: [
              GlassButton(
                style: GlassButtonStyle.destructiveQuiet,
                onPressed: () => controller.answerHostKey(tab, HostKeyDecision.reject),
                label: l10n.terminalHostKeyReject,
              ),
              GlassButton(
                onPressed: () => controller.answerHostKey(tab, HostKeyDecision.acceptOnce),
                label: l10n.terminalHostKeyAcceptOnce,
              ),
              GlassButton.prominent(
                key: const ValueKey('accept-host-key'),
                onPressed: () => controller.answerHostKey(tab, HostKeyDecision.acceptAndSave),
                label: l10n.terminalHostKeyAcceptAndSave,
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _SessionBanners extends ConsumerWidget {
  const _SessionBanners({required this.tab});

  final TerminalTab tab;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final controller = ref.read(terminalTabsProvider.notifier);
    final hostKey = tab.hostKey;
    Widget pad(Widget child) => Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
      child: child,
    );
    if (hostKey != null && hostKey.changed) {
      // Fixed, localized security text; the core's English diagnostic
      // (tab.message) says the same and is not repeated.
      return pad(
        InfoBanner(
          key: const ValueKey('host-key-changed'),
          tone: BannerTone.danger,
          title: l10n.terminalHostKeyChangedTitle(hostKey.hostPattern),
          message: l10n.terminalHostKeyChangedMessage(hostKey.keyType, hostKey.fingerprintSha256),
          action: GlassButton.plain(onPressed: () => controller.close(tab), label: l10n.terminalCloseTab),
        ),
      );
    }
    switch (tab.state) {
      case SessionConnectionState.connecting || SessionConnectionState.reconnecting:
        // tab.message is the core's technical route description (not translated).
        final route = tab.message;
        final reconnecting = tab.state == SessionConnectionState.reconnecting;
        return pad(
          Row(
            children: [
              const SizedBox.square(dimension: 14, child: CircularProgressIndicator(strokeWidth: 2)),
              const SizedBox(width: GlassSpacing.s8),
              Expanded(
                child: Text(switch ((reconnecting, route)) {
                  (true, final r?) => l10n.terminalReconnectingRoute(r),
                  (true, null) => l10n.terminalReconnecting,
                  (false, final r?) => l10n.terminalConnectingRoute(r),
                  (false, null) => l10n.terminalConnecting,
                }),
              ),
            ],
          ),
        );
      case SessionConnectionState.disconnected:
        // Localized title; the core's English reason (tab.message) is the detail.
        return pad(
          InfoBanner(
            key: const ValueKey('reconnect-banner'),
            tone: BannerTone.warning,
            title: tab.exited ? l10n.terminalSessionEnded : l10n.terminalDisconnected,
            message: tab.message ?? l10n.terminalConnectionClosed,
            action: Wrap(
              spacing: GlassSpacing.s4,
              children: [
                GlassButton.plain(
                  key: const ValueKey('reconnect'),
                  icon: Icons.refresh_rounded,
                  onPressed: () => controller.reconnect(tab),
                  label: tab.isLocal ? l10n.terminalLocalRestart : l10n.terminalReconnect,
                ),
                GlassButton.plain(onPressed: () => controller.close(tab), label: l10n.commonClose),
              ],
            ),
          ),
        );
      case SessionConnectionState.connected ||
          SessionConnectionState.awaitingHostKey ||
          SessionConnectionState.awaitingPassword:
        return const SizedBox.shrink();
    }
  }
}

/// Connect-time password prompt (host set to "ask when connecting", or no
/// credential). The password is used once and never stored.
/// Connect-time question for an account password or, for an encrypted key
/// without a stored passphrase, the key's passphrase (TODO-05).
class TerminalPasswordPromptCard extends ConsumerStatefulWidget {
  const TerminalPasswordPromptCard({required this.tab, required this.prompt, super.key});

  final TerminalTab tab;
  final TerminalPasswordPrompt prompt;

  @override
  ConsumerState<TerminalPasswordPromptCard> createState() => TerminalPasswordPromptCardState();
}

class TerminalPasswordPromptCardState extends ConsumerState<TerminalPasswordPromptCard> {
  final _password = TextEditingController();

  @override
  void dispose() {
    _password.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final secret = SecretText(_password.text);
    _password.clear();
    await ref.read(terminalTabsProvider.notifier).answerPassword(widget.tab, secret);
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    final prompt = widget.prompt;
    final keyName = prompt.keyName;
    return SecureSurface(
      key: const ValueKey('terminal-password-prompt'),
      radius: tokens.radii.card,
      padding: const EdgeInsets.all(GlassSpacing.card),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            children: [
              Icon(
                keyName != null ? Icons.key_rounded : Icons.password_rounded,
                color: tokens.palette.accent,
                size: 22,
              ),
              const SizedBox(width: GlassSpacing.s8),
              Expanded(
                child: Text(
                  keyName != null
                      ? l10n.terminalKeyPassphrasePromptTitle(keyName, prompt.hostLabel)
                      : l10n.terminalPasswordPromptTitle(prompt.username, prompt.hostLabel),
                  style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                ),
              ),
            ],
          ),
          if (prompt.retry) ...[
            const SizedBox(height: GlassSpacing.s6),
            GateErrorText(text: keyName != null ? l10n.terminalKeyPassphraseRetry : l10n.terminalPasswordRetry),
          ],
          const SizedBox(height: GlassSpacing.s12),
          SecretField(
            key: const ValueKey('terminal-password'),
            controller: _password,
            label: keyName != null ? l10n.terminalKeyPassphraseLabel : l10n.terminalPasswordLabel,
            autofocus: true,
            helper: keyName != null ? l10n.terminalKeyPassphraseHelper : l10n.terminalPasswordHelper,
            onSubmitted: (_) => _submit(),
          ),
          const SizedBox(height: GlassSpacing.s12),
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              GlassButton(
                onPressed: () => ref.read(terminalTabsProvider.notifier).answerPassword(widget.tab, null),
                label: l10n.commonCancel,
              ),
              const SizedBox(width: GlassSpacing.s8),
              GlassButton.prominent(
                key: const ValueKey('terminal-password-submit'),
                onPressed: _submit,
                label: l10n.commonConnect,
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Explicit control chords work with touch IMEs (which send composed text
/// rather than hardware key events). Never closes or recreates a session.
class _MobileTerminalKeys extends ConsumerWidget {
  const _MobileTerminalKeys({required this.tab});
  final TerminalTab tab;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final keyboardOpen = MediaQuery.viewInsetsOf(context).bottom > 0;
    return ExcludeFocus(
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Padding(
          padding: const EdgeInsets.only(top: 6),
          child: Row(
            children: [
              GlassIconButton(
                key: const ValueKey('terminal-keyboard'),
                icon: keyboardOpen ? Icons.keyboard_hide_rounded : Icons.keyboard_rounded,
                tooltip: context.l10n.mobileKeyboard,
                style: GlassIconButtonStyle.plain,
                onPressed: () =>
                    SystemChannels.textInput.invokeMethod<void>(keyboardOpen ? 'TextInput.hide' : 'TextInput.show'),
              ),
              Builder(
                builder: (anchorContext) => GlassIconButton(
                  key: const ValueKey('terminal-context-menu'),
                  icon: Icons.more_horiz_rounded,
                  tooltip: context.l10n.terminalContextMenu,
                  style: GlassIconButtonStyle.plain,
                  onPressed: () => showTerminalContextMenu(context, ref, tab, glassAnchorRect(anchorContext)),
                ),
              ),
              for (final chord in [
                ('Esc', TerminalKey.escape, false),
                ('Tab', TerminalKey.tab, false),
                ('Ctrl+C', TerminalKey.keyC, true),
                ('Ctrl+D', TerminalKey.keyD, true),
                ('Ctrl+L', TerminalKey.keyL, true),
              ])
                Padding(
                  padding: const EdgeInsets.only(right: 4),
                  child: GlassButton(
                    key: ValueKey('terminal-key-${chord.$1}'),
                    label: chord.$1,
                    onPressed: tab.isConnected ? () => tab.terminal.keyInput(chord.$2, ctrl: chord.$3) : null,
                  ),
                ),
              for (final arrow in [
                (Icons.arrow_back_rounded, TerminalKey.arrowLeft, '←'),
                (Icons.arrow_downward_rounded, TerminalKey.arrowDown, '↓'),
                (Icons.arrow_upward_rounded, TerminalKey.arrowUp, '↑'),
                (Icons.arrow_forward_rounded, TerminalKey.arrowRight, '→'),
              ])
                GlassIconButton(
                  icon: arrow.$1,
                  tooltip: arrow.$3,
                  onPressed: tab.isConnected ? () => tab.terminal.keyInput(arrow.$2) : null,
                ),
            ],
          ),
        ),
      ),
    );
  }
}
