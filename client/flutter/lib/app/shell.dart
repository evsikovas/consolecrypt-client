import 'dart:async';

import 'package:consolecrypt/account/profile_switcher.dart';
import 'package:consolecrypt/app/about.dart';
import 'package:consolecrypt/app/app_info.dart';
import 'package:consolecrypt/app/brand.dart';
import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/symbols.dart';
import 'package:consolecrypt/app/workspace_tools.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sftp/edit_flows.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

part 'mobile_shell.dart';

final class _NavItem {
  const _NavItem(this.branch, this.icon, {this.syncedOnly = false});

  final ShellBranch branch;
  final IconData icon;

  /// Devices apply to synced profiles only (ADR-0106).
  final bool syncedOnly;
}

/// Sidebar / toolbar label of a shell branch.
String shellBranchLabel(ShellBranch branch, AppLocalizations l) => switch (branch) {
  ShellBranch.hosts => l.navHosts,
  ShellBranch.groups => l.navGroups,
  ShellBranch.credentials => l.navCredentials,
  ShellBranch.knownHosts => l.settingsKnownHostsTitle,
  ShellBranch.terminal => l.navTerminal,
  ShellBranch.sftp => l.navSftp,
  ShellBranch.tunnels => l.navTunnels,
  ShellBranch.devices => l.navDevices,
  ShellBranch.sync => l.navSync,
  ShellBranch.backups => l.navBackups,
  ShellBranch.settings => l.navSettings,
  ShellBranch.sharing => l.sharingTitle,
};

const _navItems = [
  _NavItem(ShellBranch.hosts, Icons.dns_rounded),
  _NavItem(ShellBranch.credentials, Icons.key_rounded),
  _NavItem(ShellBranch.knownHosts, Icons.verified_user_outlined),
  _NavItem(ShellBranch.sharing, Icons.people_outline_rounded, syncedOnly: true),
  _NavItem(ShellBranch.terminal, Icons.terminal_rounded),
  _NavItem(ShellBranch.sftp, Icons.folder_copy_rounded),
  _NavItem(ShellBranch.tunnels, Icons.swap_horiz_rounded),
  _NavItem(ShellBranch.devices, Icons.devices_rounded, syncedOnly: true),
  _NavItem(ShellBranch.sync, Icons.sync_rounded),
  _NavItem(ShellBranch.backups, Icons.save_alt_rounded),
  _NavItem(ShellBranch.settings, Icons.settings_rounded),
];

// Older /groups links still open the group browser inside the Hosts section.
ShellBranch _navigationBranch(ShellBranch branch) => branch == ShellBranch.groups ? ShellBranch.hosts : branch;

/// Branches whose content scrolls in lists: the only routes where one
/// toolbar group may use a live backdrop (§3 rule 1). Terminal, SFTP and AI
/// Chat keep all chrome static.
const _liveToolbarBranches = {
  ShellBranch.hosts,
  ShellBranch.groups,
  ShellBranch.credentials,
  ShellBranch.knownHosts,
  ShellBranch.tunnels,
  ShellBranch.devices,
  ShellBranch.sync,
  ShellBranch.backups,
  ShellBranch.settings,
  ShellBranch.sharing,
};

/// User-collapsed sidebar (session-scoped; windows < 1000 px are always
/// compact).
final sidebarCollapsedProvider = NotifierProvider<SidebarCollapsed, bool>(SidebarCollapsed.new);

class SidebarCollapsed extends Notifier<bool> {
  @override
  bool build() => false;

  void toggle() => state = !state;
}

/// Floating inset or edge-to-edge sidebar (Settings → Appearance).
final sidebarStyleProvider = Provider<GlassSidebarStyle>((ref) {
  final style = ref.watch(localSettingsProvider.select((s) => s.value?.sidebarStyle ?? SidebarStyle.floating));
  return style == SidebarStyle.edgeToEdge ? GlassSidebarStyle.edgeToEdge : GlassSidebarStyle.floating;
});

/// Toolbar title for the current [location] of [branch].
String shellTitle(AppLocalizations l10n, ShellBranch branch, String location, Map<ObjectId, Host> hosts) {
  if (branch == ShellBranch.hosts && location.startsWith('${AppRoutes.hosts}/')) {
    if (location == AppRoutes.newHost) return l10n.hostEditorNewTitle;
    final id = location.substring(AppRoutes.hosts.length + 1).split('/').first;
    final name = hosts[ObjectId(id)]?.name;
    return name == null ? l10n.hostEditorEditTitleFallback : l10n.hostEditorEditTitle(name);
  }
  return shellBranchLabel(branch, l10n);
}

/// Desktop shell (LIQUID_GLASS_SPEC §3, §4.1) over the static ambient
/// backdrop: a floating (or edge-to-edge) glass sidebar, a 52-pt toolbar band
/// with glass capsule groups (sidebar toggle + title · search / command
/// palette · New connection, status pill, overflow menu), content-layer
/// banners and the page below the band. On macOS the unified transparent
/// title bar puts the traffic lights into the sidebar's top 52 pt (compact:
/// into the toolbar band).
///
/// The layout is a fixed [Stack] of the three regions, so switching between
/// compact and expanded never re-creates the pages (their state survives).
class AppShell extends ConsumerStatefulWidget {
  const AppShell({required this.navigationShell, super.key, this.location = AppRoutes.hosts, this.requestedTool});

  final StatefulNavigationShell navigationShell;

  /// Current router location (toolbar title of nested routes).
  final String location;

  final String? requestedTool;

  @override
  ConsumerState<AppShell> createState() => _AppShellState();
}

class _AppShellState extends ConsumerState<AppShell> {
  @override
  void initState() {
    super.initState();
    _openRequestedTool();
  }

  @override
  void didUpdateWidget(AppShell oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.requestedTool != widget.requestedTool) _openRequestedTool();
  }

  void _openRequestedTool() {
    final requested = WorkspaceTool.values.where((t) => t.name == widget.requestedTool).firstOrNull;
    if (requested == null) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      ref.read(workspaceToolsProvider.notifier).open(requested);
      context.go(widget.location);
    });
  }

  @override
  Widget build(BuildContext context) {
    if (AppPlatform.isMobile) {
      return _MobileShell(navigationShell: widget.navigationShell, location: widget.location);
    }
    final navigationShell = widget.navigationShell;
    final location = widget.location;

    final tokens = GlassTokens.of(context);
    final scope = GlassScope.of(context);
    final width = MediaQuery.sizeOf(context).width;
    final narrowWindow = width < GlassSizes.compactBreakpoint;
    final compact = narrowWindow || ref.watch(sidebarCollapsedProvider);
    final style = ref.watch(sidebarStyleProvider);
    final floating = style == GlassSidebarStyle.floating;
    final inset = floating ? tokens.radii.shellInset : 0.0;
    final trafficLights = tokens.platform == TargetPlatform.macOS && scope.unifiedTitlebar;
    // Compact + traffic lights: the lights move into the toolbar band, which
    // then spans the window above the icon rail (§4.1).
    final toolbarOnTop = compact && trafficLights;
    final branch = ShellBranch.values[navigationShell.currentIndex];
    final sidebarWidth = (compact ? GlassSizes.sidebarCompactWidth : GlassSizes.sidebarWidth) + inset;
    const band = GlassSizes.toolbarBand;
    final tools = ref.watch(workspaceToolsProvider);
    final integratedTools =
        ref.watch(
          localSettingsProvider.select(
            (settings) => settings.value?.workspacePanelStyle ?? WorkspacePanelStyle.floating,
          ),
        ) ==
        WorkspacePanelStyle.expanded;
    final profileId = ref.watch(activeProfileProvider.select((p) => p?.id));

    return Scaffold(
      body: LayoutBuilder(
        builder: (context, constraints) {
          final w = constraints.maxWidth;
          final h = constraints.maxHeight;
          final contentLeft = sidebarWidth;
          const railWidth = 60.0;
          final panelOpen = tools.selected != null;
          final rightInset = integratedTools ? 0.0 : inset;
          final railSpace = integratedTools && panelOpen ? 0.0 : railWidth;
          final available = (w - sidebarWidth - railSpace - rightInset).clamp(0.0, double.infinity);
          final docked = available >= (integratedTools ? 960 : 1000);
          final panelWidth = docked ? tools.width.clamp(360.0, available - 600.0) : 420.0.clamp(0.0, available);
          final panelRight = integratedTools ? 0.0 : railWidth + inset;
          final panelSpace = docked && panelOpen ? panelWidth + 8 : 0.0;
          final contentWidth = available - panelSpace;
          final sidebarRect = toolbarOnTop
              ? Rect.fromLTWH(0, band - inset, sidebarWidth, h - band + inset)
              : Rect.fromLTWH(0, 0, sidebarWidth, h);
          final toolbarRect = toolbarOnTop
              ? Rect.fromLTWH(0, 0, w - railWidth - inset, band)
              : Rect.fromLTWH(contentLeft, 0, available, band);
          final contentRect = Rect.fromLTWH(contentLeft, band, contentWidth, h - band);
          return Stack(
            children: [
              Positioned.fromRect(
                rect: sidebarRect,
                child: _Sidebar(
                  navigationShell: navigationShell,
                  overlayTools: !docked,
                  compact: compact,
                  style: style,
                  reserveTrafficLights: trafficLights && !toolbarOnTop,
                ),
              ),
              Positioned.fromRect(
                rect: contentRect,
                child: MediaQuery(
                  data: MediaQuery.of(context).copyWith(size: Size(contentWidth, h - band)),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      const _ShellBanners(),
                      // Offers to recover SFTP edit sessions left by a crash / quit (once per unlock).
                      Expanded(
                        child: ExcludeFocus(
                          excluding: panelOpen && !docked,
                          child: SftpEditRecoveryListener(child: navigationShell),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
              Positioned(
                top: band,
                bottom: 0,
                right: rightInset,
                width: railWidth,
                child: Offstage(
                  offstage: integratedTools && panelOpen,
                  child: WorkspaceToolRail(integrated: integratedTools),
                ),
              ),
              Positioned.fromRect(
                rect: Rect.fromLTWH(contentLeft, band, available, h - band),
                child: panelOpen && !docked
                    ? ModalBarrier(
                        key: const ValueKey('workspace-tools-barrier'),
                        color: tokens.surfaces.barrier,
                        dismissible: true,
                        onDismiss: () => ref.read(workspaceToolsProvider.notifier).close(),
                        semanticsLabel: context.l10n.workspaceToolsClose,
                      )
                    : const SizedBox.shrink(),
              ),
              Positioned(
                top: band,
                bottom: integratedTools ? 0 : inset,
                right: panelRight,
                width: panelWidth,
                child: WorkspaceToolPanel(key: ValueKey(profileId), integrated: integratedTools),
              ),
              Positioned(
                top: band + 12,
                bottom: inset + 12,
                right: panelRight + panelWidth,
                width: 8,
                child: panelOpen && docked ? WorkspaceToolResizeHandle(width: panelWidth) : const SizedBox.shrink(),
              ),
              Positioned.fromRect(
                rect: toolbarRect,
                child: _ShellToolbar(
                  // Recreate native passthrough regions when the band moves.
                  key: ValueKey('toolbar-$toolbarOnTop-$compact-$floating'),
                  branch: branch,
                  location: location,
                  compact: compact,
                  canCollapse: !narrowWindow,
                  leadingInset: toolbarOnTop ? GlassSizes.trafficLightsLeading - GlassSpacing.s12 : 0,
                ),
              ),
            ],
          );
        },
      ),
    );
  }
}

class _Sidebar extends ConsumerWidget {
  const _Sidebar({
    required this.navigationShell,
    required this.overlayTools,
    required this.compact,
    required this.style,
    required this.reserveTrafficLights,
  });

  final StatefulNavigationShell navigationShell;
  final bool overlayTools;
  final bool compact;
  final GlassSidebarStyle style;
  final bool reserveTrafficLights;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profile = ref.watch(activeProfileProvider);
    final tabs = ref.watch(terminalTabsProvider).tabs.length;
    final pending = ref.watch(devicesProvider).value?.pendingRequests.length ?? 0;
    final l10n = context.l10n;
    final items = _navItems.where((i) => !i.syncedOnly || (profile?.isSynced ?? false)).toList();
    return GlassSidebar(
      style: style,
      compact: compact,
      reserveTrafficLights: reserveTrafficLights,
      header: compact
          ? Center(child: ProfileSwitcher(compact: compact))
          : Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Padding(padding: EdgeInsets.fromLTRB(8, 0, 8, 16), child: BrandMark.wordmark(height: 27)),
                ProfileSwitcher(compact: compact),
              ],
            ),
      footer: _SidebarFooter(compact: compact),
      children: [
        for (final item in items) ...[
          if (item.branch == ShellBranch.terminal || item.branch == ShellBranch.sync)
            const Padding(padding: EdgeInsets.symmetric(horizontal: 8, vertical: 8), child: Divider(height: 1)),
          GlassSidebarItem(
            key: ValueKey('nav-${item.branch.name}'),
            icon: item.icon,
            leading: AppSymbolIcon(AppSymbol.values.byName(item.branch.name)),
            label: shellBranchLabel(item.branch, l10n),
            compact: compact,
            selected: _navigationBranch(ShellBranch.values[navigationShell.currentIndex]) == item.branch,
            badge: switch (item.branch) {
              ShellBranch.terminal when tabs > 0 => '$tabs',
              ShellBranch.devices when pending > 0 => '$pending',
              _ => null,
            },
            badgeTone: item.branch == ShellBranch.devices ? GlassTone.warning : GlassTone.neutral,
            onPressed: () {
              if (overlayTools) {
                ref.read(workspaceToolsProvider.notifier).close();
              }
              navigationShell.goBranch(
                item.branch.index,
                initialLocation: navigationShell.currentIndex == item.branch.index,
              );
            },
          ),
        ],
      ],
    );
  }
}

/// Lock (glass-free plain icon button; also ⌘L / Ctrl+L and the toolbar
/// menu) and, when compact, the sync status as an icon-only pill.
class _SidebarFooter extends ConsumerWidget {
  const _SidebarFooter({required this.compact});

  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final lock = commandFor(AppCommandId.lockVault);
    final lockButton = GlassIconButton(
      key: const ValueKey('lock-vault'),
      icon: Icons.lock_rounded,
      tooltip: context.l10n.shellLockVaultTooltip(lock.shortcutLabel),
      style: GlassIconButtonStyle.plain,
      iconSize: GlassSizes.iconRow,
      onPressed: () => ref.read(vaultServiceProvider).lock(),
    );
    if (compact) {
      return Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          const SyncStatusPill(compact: true),
          const SizedBox(height: GlassSpacing.s4),
          lockButton,
        ],
      );
    }
    return Row(children: [lockButton, const Spacer()]);
  }
}

/// How a sync state is shown (icon, tone, short label, pulsing).
typedef SyncPresentation = ({IconData? icon, GlassTone tone, String label, bool pulsing});

/// Status pill mapping (§4.1): Synced (success dot), Syncing… (pulsing),
/// Offline (warning, cloud_off), Sync error (danger), Local only (lock,
/// neutral — never green), paused.
SyncPresentation describeSyncStatus(SyncStatus status, AppLocalizations l10n) => switch (status.state) {
  SyncState.localOnly => (
    icon: Icons.lock_rounded,
    tone: GlassTone.neutral,
    label: l10n.syncStateLocalOnly,
    pulsing: false,
  ),
  SyncState.idle => (icon: null, tone: GlassTone.success, label: l10n.syncStateSynced, pulsing: false),
  SyncState.syncing => (icon: null, tone: GlassTone.accent, label: l10n.syncStateSyncing, pulsing: true),
  SyncState.offline => (
    icon: Icons.cloud_off_rounded,
    tone: GlassTone.warning,
    label: status.pendingChanges > 0 ? l10n.syncStateOfflinePending(status.pendingChanges) : l10n.syncStateOffline,
    pulsing: false,
  ),
  SyncState.error => (
    icon: Icons.sync_problem_rounded,
    tone: GlassTone.danger,
    label: l10n.syncStateError,
    pulsing: false,
  ),
  SyncState.paused => (
    icon: Icons.pause_circle_rounded,
    tone: GlassTone.neutral,
    label: l10n.syncStatePaused,
    pulsing: false,
  ),
};

/// Longer description for tooltips.
String describeSyncTooltip(SyncStatus status, AppLocalizations l10n) {
  if (status.state == SyncState.localOnly) return l10n.syncIndicatorLocalTooltip;
  final label = status.state == SyncState.idle && status.lastSyncAt != null
      ? l10n.syncStateSyncedAgo(formatRelative(l10n, status.lastSyncAt!))
      : describeSyncStatus(status, l10n).label;
  return status.pendingChanges > 0 ? l10n.syncIndicatorPendingTooltip(label, status.pendingChanges) : label;
}

/// Sync / "Local only" status pill (§4.1). A click opens a glass popover
/// with the last sync, server, pending changes and "Sync now".
class SyncStatusPill extends ConsumerWidget {
  const SyncStatusPill({super.key, this.compact = false, this.shortLabel = false});

  /// Icon-only (compact sidebar footer).
  final bool compact;

  /// A short successful-sync label leaves more room for desktop search.
  /// Other states keep their descriptive labels and truthful status colours.
  final bool shortLabel;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final status = ref.watch(syncStatusProvider).value ?? SyncStatus.paused;
    final l10n = context.l10n;
    final p = describeSyncStatus(status, l10n);
    final tokens = GlassTokens.of(context);
    void open(BuildContext anchorContext) => unawaited(
      showGlassPopover<void>(
        context: anchorContext,
        anchor: glassAnchorRect(anchorContext),
        width: 300,
        builder: (context) => const _SyncPopover(),
      ),
    );
    if (compact) {
      return Builder(
        builder: (context) => GlassIconButton(
          key: const ValueKey('sync-indicator'),
          icon: p.icon ?? (p.tone == GlassTone.success ? Icons.cloud_done_rounded : Icons.sync_rounded),
          tooltip: describeSyncTooltip(status, l10n),
          style: GlassIconButtonStyle.plain,
          iconSize: GlassSizes.iconRow,
          color: p.tone == GlassTone.neutral ? tokens.secondaryLabel : tokens.palette.tone(p.tone),
          onPressed: () => open(context),
        ),
      );
    }
    return Builder(
      builder: (context) => GlassStatusPill(
        key: const ValueKey('sync-indicator'),
        label: shortLabel && status.state == SyncState.idle ? l10n.syncStateSyncedShort : p.label,
        tone: p.tone,
        icon: p.icon,
        pulsing: p.pulsing,
        tooltip: describeSyncTooltip(status, l10n),
        onPressed: () => open(context),
      ),
    );
  }
}

class _SyncPopover extends ConsumerWidget {
  const _SyncPopover();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final status = ref.watch(syncStatusProvider).value ?? SyncStatus.paused;
    final profile = ref.watch(activeProfileProvider);
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    final p = describeSyncStatus(status, l10n);
    final router = GoRouter.of(context);
    void go(String route) {
      Navigator.of(context).pop();
      router.go(route);
    }

    Widget fact(String label, String value) => Padding(
      padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Text(label, style: t.callout.copyWith(color: tokens.secondaryLabel)),
          ),
          const SizedBox(width: GlassSpacing.s8),
          Flexible(
            child: Text(
              value,
              textAlign: TextAlign.end,
              style: t.callout.copyWith(color: tokens.palette.label),
            ),
          ),
        ],
      ),
    );

    final local = status.state == SyncState.localOnly;
    return Column(
      key: const ValueKey('sync-popover'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          children: [
            Icon(
              p.icon ?? Icons.cloud_done_rounded,
              size: 18,
              color: p.tone == GlassTone.neutral ? tokens.secondaryLabel : tokens.palette.tone(p.tone),
            ),
            const SizedBox(width: GlassSpacing.s8),
            Expanded(
              child: Text(p.label, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
            ),
          ],
        ),
        const SizedBox(height: GlassSpacing.s8),
        if (local)
          Text(l10n.syncIndicatorLocalTooltip, style: t.callout.copyWith(color: tokens.secondaryLabel))
        else ...[
          fact(
            l10n.syncScreenLastSync,
            status.lastSyncAt == null ? l10n.shellSyncNever : formatRelative(l10n, status.lastSyncAt!),
          ),
          if (profile?.serverUrl != null) fact(l10n.settingsServerLabel, profile!.serverUrl!.host),
          fact(l10n.syncScreenPendingChanges, '${status.pendingChanges}'),
        ],
        const SizedBox(height: GlassSpacing.s12),
        Row(
          mainAxisAlignment: MainAxisAlignment.end,
          children: [
            GlassButton.plain(
              key: const ValueKey('sync-popover-details'),
              label: local ? l10n.syncScreenEnableSync : l10n.shellSyncDetails,
              size: GlassControlSize.sm,
              onPressed: () => go(AppRoutes.sync),
            ),
            if (!local) ...[
              const SizedBox(width: GlassSpacing.s6),
              GlassButton(
                key: const ValueKey('sync-popover-now'),
                label: l10n.syncScreenSyncNow,
                icon: Icons.sync_rounded,
                size: GlassControlSize.sm,
                onPressed: status.state == SyncState.syncing || status.state == SyncState.paused
                    ? null
                    : () {
                        Navigator.of(context).pop();
                        unawaited(runWithFeedback(context, () => ref.read(syncServiceProvider).syncNow()));
                      },
              ),
            ],
          ],
        ),
      ],
    );
  }
}

class _ShellToolbar extends ConsumerWidget {
  const _ShellToolbar({
    required this.branch,
    required this.location,
    required this.compact,
    required this.canCollapse,
    required this.leadingInset,
    super.key,
  });

  final ShellBranch branch;
  final String location;
  final bool compact;
  final bool canCollapse;
  final double leadingInset;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final dispatcher = ref.read(appCommandDispatcherProvider);
    final hosts = ref.watch(hostByIdProvider);
    final palette = commandFor(AppCommandId.commandPalette);
    final newTab = commandFor(AppCommandId.newTerminalTab);
    final title = shellTitle(l10n, branch, location, hosts);
    final live = _liveToolbarBranches.contains(branch) ? BackdropMode.live : BackdropMode.static;
    // The connection action stays circular at every desktop width. Search
    // uses the released space; compact windows move status to the sidebar.
    // One tinted action per view (§4.2): editors own theirs ("Save"), so
    // the toolbar's New connection steps back to plain glass there.
    final pageHasPrimary = location.startsWith('${AppRoutes.hosts}/');
    return GlassToolbar(
      leadingInset: leadingInset,
      leadingMaxWidth: 220,
      leading: [
        GlassToolbarGroup(
          children: [
            if (canCollapse)
              GlassIconButton(
                key: const ValueKey('toggle-sidebar'),
                icon: Icons.view_sidebar_rounded,
                tooltip: l10n.shellToggleSidebar,
                style: GlassIconButtonStyle.plain,
                onPressed: () => ref.read(sidebarCollapsedProvider.notifier).toggle(),
              ),
            GlassToolbarTitle(title),
          ],
        ),
      ],
      center: _SearchCapsule(
        backdrop: live,
        placeholder: l10n.shellSearchPlaceholder,
        shortcut: palette.shortcutLabel,
        onPressed: () => dispatcher.invoke(AppCommandId.commandPalette),
      ),
      trailing: [
        GlassIconButton(
          key: const ValueKey('new-connection'),
          icon: Icons.add_rounded,
          size: 40,
          style: pageHasPrimary ? GlassIconButtonStyle.glass : GlassIconButtonStyle.prominent,
          tooltip: l10n.shellNewConnectionTooltip(newTab.shortcutLabel),
          onPressed: () => dispatcher.invoke(AppCommandId.newTerminalTab),
        ),
        if (!compact) const SyncStatusPill(shortLabel: true),
        const _OverflowMenu(),
      ],
    );
  }
}

/// The toolbar's search / command capsule (§4.1): placeholder and ⌘K keycap.
class _SearchCapsule extends StatelessWidget {
  const _SearchCapsule({
    required this.backdrop,
    required this.placeholder,
    required this.shortcut,
    required this.onPressed,
  });

  final BackdropMode backdrop;
  final String placeholder;
  final String shortcut;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return GlassInteractive(
      key: const ValueKey('open-palette'),
      semanticLabel: placeholder,
      onPressed: onPressed,
      builder: (context, state) => GlassFocusRing(
        visible: state.focusVisible,
        shape: const StadiumBorder(),
        child: GlassCapsule(
          backdrop: backdrop,
          interactive: true,
          child: LayoutBuilder(
            builder: (context, constraints) => Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.search_rounded, size: 18, color: tokens.secondaryLabel),
                if (constraints.maxWidth > 80) ...[
                  const SizedBox(width: GlassSpacing.s6),
                  Flexible(
                    fit: FlexFit.tight,
                    child: Text(
                      placeholder,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: tokens.typography.body.copyWith(color: tokens.secondaryLabel),
                    ),
                  ),
                ],
                if (constraints.maxWidth > 220) ...[
                  const SizedBox(width: GlassSpacing.s6),
                  DecoratedBox(
                    decoration: ShapeDecoration(color: tokens.surfaces.inset, shape: GlassRadii.shape(tokens.radii.xs)),
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
                      child: Text(shortcut, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                    ),
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

enum _MenuAction { newHost, newTab, palette, settings, about, lock }

class _OverflowMenu extends ConsumerWidget {
  const _OverflowMenu();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final dispatcher = ref.read(appCommandDispatcherProvider);
    GlassMenuItem<_MenuAction> item(_MenuAction action, AppCommandId? id, String label, IconData icon) => GlassMenuItem(
      key: ValueKey('shell-menu-${action.name}'),
      value: action,
      label: label,
      icon: icon,
      shortcut: id == null ? null : commandFor(id).shortcutLabel,
    );
    return GlassMenuButton<_MenuAction>(
      entries: [
        item(_MenuAction.newHost, AppCommandId.newHost, l10n.commandNewHost, Icons.add_rounded),
        item(_MenuAction.newTab, AppCommandId.newTerminalTab, l10n.commandNewTerminalTab, Icons.terminal_rounded),
        item(_MenuAction.palette, AppCommandId.commandPalette, l10n.commandPalette, Icons.search_rounded),
        const GlassMenuDivider(),
        item(_MenuAction.settings, AppCommandId.openSettings, l10n.commandOpenSettings, Icons.settings_rounded),
        item(_MenuAction.about, null, l10n.aboutTitle(kAppName), Icons.info_rounded),
        const GlassMenuDivider(),
        item(_MenuAction.lock, AppCommandId.lockVault, l10n.commandLockVault, Icons.lock_rounded),
      ],
      onSelected: (action) => switch (action) {
        _MenuAction.newHost => dispatcher.invoke(AppCommandId.newHost),
        _MenuAction.newTab => dispatcher.invoke(AppCommandId.newTerminalTab),
        _MenuAction.palette => dispatcher.invoke(AppCommandId.commandPalette),
        _MenuAction.settings => dispatcher.invoke(AppCommandId.openSettings),
        _MenuAction.about => unawaited(showAboutConsoleCrypt(rootNavigatorKey.currentContext ?? context)),
        _MenuAction.lock => dispatcher.invoke(AppCommandId.lockVault),
      },
      builder: (context, open) => GlassIconButton(
        key: const ValueKey('shell-menu'),
        icon: Icons.more_horiz_rounded,
        tooltip: GlassStrings.of(context).moreActions,
        onPressed: open,
      ),
    );
  }
}

/// Content-layer banners under the toolbar (§4.1), e.g. a pending device
/// approval: a card in the role colour, never glass.
class _ShellBanners extends ConsumerWidget {
  const _ShellBanners();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final pending = ref.watch(devicesProvider).value?.pendingRequests ?? const <DeviceTrustRequest>[];
    if (pending.isEmpty) return const SizedBox.shrink();
    if (AppPlatform.isMobile) {
      return Material(
        color: GlassTokens.of(context).palette.warning.withValues(alpha: .12),
        child: ListTile(
          key: const ValueKey('pending-approval-banner'),
          leading: const Icon(Icons.devices_other_rounded, size: 22),
          title: Text(
            context.l10n.shellApprovalBannerTitle(pending.length),
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
          ),
          trailing: const Icon(Icons.chevron_right_rounded),
          onTap: () => context.go(AppRoutes.devices),
        ),
      );
    }
    final first = pending.first;
    final l10n = context.l10n;
    final pad = pagePadding(context);
    return Padding(
      padding: EdgeInsets.fromLTRB(pad, GlassSpacing.s4, pad, GlassSpacing.s4),
      child: InfoBanner(
        key: const ValueKey('pending-approval-banner'),
        tone: BannerTone.warning,
        icon: Icons.devices_other_rounded,
        title: l10n.shellApprovalBannerTitle(pending.length),
        message: l10n.shellApprovalBannerMessage(
          first.device.name,
          first.device.platform.localized(l10n),
          formatRelative(l10n, first.createdAt),
        ),
        action: TextButton(onPressed: () => context.go(AppRoutes.devices), child: Text(l10n.commonReview)),
      ),
    );
  }
}
