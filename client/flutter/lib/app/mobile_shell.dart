part of 'shell.dart';

/// Touch-first shell: the workspace keeps its Navigator and session state
/// while tools cover it. No desktop rails consume the phone's viewport.
class _MobileShell extends ConsumerStatefulWidget {
  const _MobileShell({required this.navigationShell, required this.location});
  final StatefulNavigationShell navigationShell;
  final String location;

  @override
  ConsumerState<_MobileShell> createState() => _MobileShellState();
}

class _MobileShellState extends ConsumerState<_MobileShell> {
  bool _more = false;
  static const _primary = [ShellBranch.hosts, ShellBranch.terminal, ShellBranch.sftp];

  void _go(ShellBranch branch) {
    FocusManager.instance.primaryFocus?.unfocus();
    ref.read(workspaceToolsProvider.notifier).close();
    setState(() => _more = false);
    widget.navigationShell.goBranch(branch.index);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final branch = ShellBranch.values[widget.navigationShell.currentIndex];
    final toolsOpen = ref.watch(workspaceToolsProvider.select((s) => s.selected != null));
    final profile = ref.watch(activeProfileProvider);
    final selected = _more ? 3 : _primary.indexOf(branch);
    final keyboardOpen = MediaQuery.viewInsetsOf(context).bottom > 0;
    return PopScope(
      canPop: !toolsOpen && !_more && branch == ShellBranch.hosts,
      onPopInvokedWithResult: (didPop, _) {
        if (didPop) return;
        if (toolsOpen) {
          ref.read(workspaceToolsProvider.notifier).close();
        } else if (_more) {
          setState(() => _more = false);
        } else {
          _go(ShellBranch.hosts);
        }
      },
      child: Scaffold(
        key: const ValueKey('mobile-shell'),
        resizeToAvoidBottomInset: true,
        appBar: toolsOpen
            ? null
            : AppBar(
                backgroundColor: tokens.surfaces.contentSolid,
                surfaceTintColor: Colors.transparent,
                elevation: 0,
                automaticallyImplyLeading: false,
                titleSpacing: 16,
                title: const BrandMark.wordmark(height: 25),
                actions: [
                  GlassIconButton(
                    key: const ValueKey('mobile-snippets'),
                    icon: Icons.code_rounded,
                    tooltip: l.navSnippets,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.snippets),
                  ),
                  GlassIconButton(
                    key: const ValueKey('mobile-ai'),
                    icon: Icons.auto_awesome_rounded,
                    tooltip: l.navAiChat,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.ai),
                  ),
                  const SizedBox(width: 8),
                ],
              ),
        body: SafeArea(
          top: toolsOpen,
          bottom: keyboardOpen || toolsOpen,
          child: Stack(
            fit: StackFit.expand,
            children: [
              Offstage(
                offstage: _more || toolsOpen,
                child: Column(
                  children: [
                    const _ShellBanners(),
                    Expanded(child: SftpEditRecoveryListener(child: widget.navigationShell)),
                  ],
                ),
              ),
              if (_more && !toolsOpen)
                ListView(
                  key: const ValueKey('mobile-more-page'),
                  padding: const EdgeInsets.fromLTRB(16, 12, 16, 24),
                  children: [
                    const ProfileSwitcher(),
                    const SizedBox(height: 20),
                    for (final item in _navItems)
                      if (!_primary.contains(item.branch) && (!item.syncedOnly || profile?.isLocal == false))
                        ListTile(
                          key: ValueKey('mobile-nav-${item.branch.name}'),
                          minVerticalPadding: 14,
                          leading: Icon(item.icon),
                          title: Text(shellBranchLabel(item.branch, l)),
                          trailing: const Icon(Icons.chevron_right_rounded),
                          onTap: () => _go(item.branch),
                        ),
                    const SizedBox(height: 16),
                    GlassButton(
                      icon: Icons.lock_outline_rounded,
                      label: l.settingsLockNow,
                      expand: true,
                      onPressed: () => runWithFeedback(context, () => ref.read(vaultServiceProvider).lock()),
                    ),
                  ],
                ),
              WorkspaceToolPanel(key: ValueKey('mobile-tools-${profile?.id.value}'), integrated: true),
            ],
          ),
        ),
        bottomNavigationBar: keyboardOpen || toolsOpen
            ? null
            : NavigationBar(
                key: const ValueKey('mobile-navigation'),
                selectedIndex: selected < 0 ? 3 : selected,
                height: 72,
                backgroundColor: tokens.surfaces.contentSolid,
                indicatorColor: tokens.sidebarSelection,
                onDestinationSelected: (index) {
                  if (index == 3) {
                    setState(() => _more = true);
                  } else {
                    _go(_primary[index]);
                  }
                },
                destinations: [
                  NavigationDestination(
                    icon: const Icon(Icons.dns_outlined),
                    selectedIcon: const Icon(Icons.dns_rounded),
                    label: l.navHosts,
                  ),
                  NavigationDestination(icon: const Icon(Icons.terminal_rounded), label: l.navTerminal),
                  NavigationDestination(
                    icon: const Icon(Icons.folder_outlined),
                    selectedIcon: const Icon(Icons.folder_rounded),
                    label: l.navSftp,
                  ),
                  NavigationDestination(icon: const Icon(Icons.grid_view_rounded), label: l.mobileMore),
                ],
              ),
      ),
    );
  }
}
