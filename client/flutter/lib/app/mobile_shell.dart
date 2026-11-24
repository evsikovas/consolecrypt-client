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
  static const _defaultPrimary = [ShellBranch.hosts, ShellBranch.terminal, ShellBranch.sftp];

  @override
  void initState() {
    super.initState();
    _listenDeviceUnlockOffer(this);
  }

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
    final kubeAlerts = kubernetesEnabled
        ? ref.watch(kubeGitOpsAlertsProvider.select((alerts) => alerts.values.fold(0, (sum, count) => sum + count)))
        : 0;
    final preferences = ref.watch(navigationPreferencesProvider);
    final primary = (preferences?.ordered(_defaultPrimary, (branch) => branch.name) ?? _defaultPrimary)
        .where((branch) => preferences?.visible(branch.name) ?? true)
        .toList();
    final selected = _more ? primary.length : primary.indexOf(_navigationBranch(branch));
    final keyboardOpen = MediaQuery.viewInsetsOf(context).bottom > 0;
    final terminalFocused = branch == ShellBranch.terminal && !_more && ref.watch(terminalMinimalModeProvider);
    final rdpVisible = branch == ShellBranch.rdp && !_more && !toolsOpen;
    // The remote desktop and its input controls need the short landscape
    // viewport. Navigation returns as soon as the keyboard is dismissed.
    final hideAppBar =
        toolsOpen ||
        terminalFocused ||
        (rdpVisible && (keyboardOpen || MediaQuery.orientationOf(context) == Orientation.landscape));
    return PopScope(
      canPop: !toolsOpen && !_more && branch == ShellBranch.hosts,
      onPopInvokedWithResult: (didPop, _) {
        if (didPop) return;
        if (toolsOpen) {
          ref.read(workspaceToolsProvider.notifier).close();
        } else if (terminalFocused) {
          ref.read(terminalMinimalModeProvider.notifier).leave();
        } else if (_more) {
          setState(() => _more = false);
        } else {
          _go(ShellBranch.hosts);
        }
      },
      child: Scaffold(
        key: const ValueKey('mobile-shell'),
        resizeToAvoidBottomInset: true,
        appBar: hideAppBar
            ? null
            : AppBar(
                backgroundColor: tokens.surfaces.contentSolid,
                surfaceTintColor: Colors.transparent,
                elevation: 0,
                automaticallyImplyLeading: false,
                titleSpacing: 16,
                title: const BrandMark.wordmark(height: 25),
                shape: Border(bottom: BorderSide(color: tokens.surfaces.hairlineCard)),
                actions: [
                  GlassIconButton(
                    key: const ValueKey('mobile-snippets'),
                    icon: Icons.code_rounded,
                    tooltip: l.navSnippets,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.snippets),
                  ),
                  GlassIconButton(
                    key: const ValueKey('mobile-notes'),
                    icon: Icons.sticky_note_2_outlined,
                    tooltip: l.navNotes,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.notes),
                  ),
                  GlassIconButton(
                    key: const ValueKey('mobile-ai'),
                    icon: Icons.auto_awesome_rounded,
                    tooltip: l.navAiChat,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.ai),
                  ),
                  const CriticalNoticeButton(),
                  const SizedBox(width: 8),
                ],
              ),
        body: SafeArea(
          top: hideAppBar,
          bottom: keyboardOpen || toolsOpen,
          child: Column(
            children: [
              if (hideAppBar) const CriticalNoticeAttentionBar(),
              Expanded(
                child: Stack(
                  fit: StackFit.expand,
                  children: [
                    Offstage(
                      offstage: _more || toolsOpen,
                      child: Column(
                        children: [
                          Offstage(offstage: rdpVisible && keyboardOpen, child: const _ShellBanners()),
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
                          GlassButton(
                            key: const ValueKey('mobile-customize-navigation'),
                            icon: Icons.tune_rounded,
                            label: l.navigationCustomize,
                            onPressed: () => showNavigationCustomization(context),
                          ),
                          for (final item in ref.watch(availableNavigationSectionsProvider))
                            if (!item.syncedOnly || profile?.isLocal == false)
                              ListTile(
                                key: ValueKey('mobile-nav-${item.branch.name}'),
                                minVerticalPadding: 14,
                                leading: Icon(item.icon),
                                title: Text(shellBranchLabel(item.branch, l)),
                                trailing: item.branch == ShellBranch.kubernetes && kubeAlerts > 0
                                    ? GlassBadge(label: '$kubeAlerts', tone: GlassTone.warning, dense: true)
                                    : const Icon(Icons.chevron_right_rounded),
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
                    WorkspaceToolPanel(
                      key: ValueKey('mobile-tools-${profile?.id.value}'),
                      integrated: true,
                      branch: branch,
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
        bottomNavigationBar: keyboardOpen || toolsOpen || terminalFocused
            ? null
            : primary.isEmpty
            ? SafeArea(
                child: SizedBox(
                  height: 72,
                  child: Center(
                    child: GlassButton(
                      key: const ValueKey('mobile-all-sections-only'),
                      label: l.navigationAllSections,
                      icon: Icons.apps_rounded,
                      onPressed: () => setState(() => _more = true),
                    ),
                  ),
                ),
              )
            : SafeArea(
                top: false,
                minimum: const EdgeInsets.fromLTRB(GlassSpacing.s8, 0, GlassSpacing.s8, GlassSpacing.s8),
                child: Material(
                  shape: GlassRadii.shape(tokens.radii.panel)
                      .copyWith(side: BorderSide(color: tokens.surfaces.hairlineCard)),
                  color: tokens.surfaces.contentSolid,
                  clipBehavior: Clip.antiAlias,
                  child: NavigationBar(
                    key: const ValueKey('mobile-navigation'),
                    selectedIndex: selected < 0 ? primary.length : selected,
                    height: 64,
                    elevation: 0,
                    backgroundColor: tokens.surfaces.contentSolid,
                    indicatorColor: tokens.sidebarSelection,
                    onDestinationSelected: (index) {
                      if (index == primary.length) {
                        setState(() => _more = true);
                      } else {
                        _go(primary[index]);
                      }
                    },
                    destinations: [
                      for (final branch in primary)
                        NavigationDestination(
                          icon: Icon(navigationSections.firstWhere((section) => section.branch == branch).icon),
                          label: shellBranchLabel(branch, l),
                        ),
                      NavigationDestination(
                        icon: Badge(isLabelVisible: kubeAlerts > 0, child: const Icon(Icons.grid_view_rounded)),
                        label: l.mobileMore,
                      ),
                    ],
                  ),
                ),
              ),
      ),
    );
  }
}
