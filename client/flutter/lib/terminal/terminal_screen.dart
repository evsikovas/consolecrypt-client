import 'package:consolecrypt/ai/command_palette.dart';
import 'package:consolecrypt/app/commands.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/terminal/terminal_context_menu.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

class TerminalScreen extends ConsumerWidget {
  const TerminalScreen({super.key});

  Future<void> _newTab(BuildContext context, WidgetRef ref) async {
    final host = await showHostPicker(context, title: context.l10n.commandNewTabPickerTitle);
    if (host != null) await ref.read(terminalTabsProvider.notifier).open(host);
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(terminalTabsProvider);
    final l10n = context.l10n;
    final pad = AppPlatform.isMobile ? 6.0 : pagePadding(context);
    final Widget body;
    if (state.tabs.isEmpty) {
      body = Material(
        type: MaterialType.transparency,
        child: EmptyState(
          icon: Icons.terminal_rounded,
          title: l10n.terminalEmptyTitle,
          message: l10n.terminalEmptyMessage(commandFor(AppCommandId.newTerminalTab).shortcutLabel),
          action: GlassButton.prominent(
            key: const ValueKey('terminal-connect'),
            size: GlassControlSize.lg,
            onPressed: () => _newTab(context, ref),
            icon: Icons.add_rounded,
            label: l10n.terminalConnectToHost,
          ),
        ),
      );
    } else {
      final active = state.active!;
      // Tab strip above the card, banners between them, the terminal card
      // fully opaque below (§4.9). Context menus are transient user actions.
      body = Material(
        type: MaterialType.transparency,
        child: Padding(
          padding: EdgeInsets.fromLTRB(pad, GlassSpacing.s8, pad, pad),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _TabStrip(state: state, active: active, onNewTab: () => _newTab(context, ref)),
              const SizedBox(height: GlassSpacing.s8),
              _SessionBanners(tab: active),
              Expanded(
                child: _TerminalCard(state: state, active: active),
              ),
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

class _TabStrip extends ConsumerWidget {
  const _TabStrip({required this.state, required this.active, required this.onNewTab});

  final TerminalTabsState state;
  final TerminalTab active;
  final VoidCallback onNewTab;

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
    return GlassTabStrip(
      activeIndex: state.activeIndex.clamp(0, state.tabs.length - 1),
      tabs: [
        for (final (i, tab) in state.tabs.indexed)
          GlassTab(
            key: ValueKey('tab-${tab.host.name}-$i'),
            title: tab.title,
            subtitle: tab.host.username,
            state: _state(tab),
            tooltip: switch (_state(tab)) {
              GlassTabState.connected => strings.tabConnected,
              GlassTabState.reconnecting => strings.tabReconnecting,
              GlassTabState.disconnected => strings.tabDisconnected,
              GlassTabState.none => null,
            },
          ),
      ],
      onSelect: controller.activate,
      onClose: (i) => controller.close(state.tabs[i]),
      onAdd: onNewTab,
      trailing: AppPlatform.isMobile ? null : _SessionToolbar(tab: active),
    );
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
          onPressed: () => showCommandPalette(context, selectedText: tab.selectedText, hostId: tab.host.id),
        ),
        GlassIconButton(
          key: const ValueKey('terminal-sftp'),
          icon: Icons.folder_copy_rounded,
          tooltip: l10n.navSftp,
          style: GlassIconButtonStyle.plain,
          size: GlassSizes.tab,
          iconSize: GlassSizes.iconRow,
          onPressed: () async {
            await ref.read(sftpControllerProvider.notifier).connect(tab.host);
            if (context.mounted) context.go(AppRoutes.sftp);
          },
        ),
        // TODO(client/ui): split panes (horizontal/vertical) — needs a layout model for
        // tabs; next: after M4 when sessions are real.
        GlassIconButton(
          icon: Icons.vertical_split_rounded,
          tooltip: l10n.terminalSplitViewLater,
          style: GlassIconButtonStyle.plain,
          size: GlassSizes.tab,
          iconSize: GlassSizes.iconRow,
          onPressed: null,
        ),
      ],
    );
  }
}

/// The terminal card (§4.9): the terminal theme background, 100 % opaque,
/// `r.card`, 10 px padding in the same colour, a hairline border; the
/// viewport itself is square. Connect-time prompts dock at its top on the
/// secure (opaque) material — they may overlap the terminal.
class _TerminalCard extends ConsumerWidget {
  const _TerminalCard({required this.state, required this.active});

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
    final hostKey = active.hostKey;
    final prompt = active.passwordPrompt;
    final Widget? docked = hostKey != null && !hostKey.changed
        ? _HostKeyPromptCard(tab: active, hostKey: hostKey)
        : prompt != null
        ? _PasswordPromptCard(key: ValueKey('password-prompt-${active.sessionId.value}'), tab: active, prompt: prompt)
        : null;
    return DecoratedBox(
      key: const ValueKey('terminal-card'),
      decoration: ShapeDecoration(
        color: theme.background,
        shape: GlassRadii.shape(tokens.radii.card).copyWith(side: BorderSide(color: tokens.surfaces.hairlineCard)),
      ),
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Stack(
          children: [
            Positioned.fill(
              child: IndexedStack(
                index: state.activeIndex.clamp(0, state.tabs.length - 1),
                children: [
                  for (final tab in state.tabs)
                    TerminalPane(
                      key: ValueKey(tab.sessionId),
                      tab: tab,
                      active: identical(tab, active) && docked == null,
                    ),
                ],
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
        ),
      ),
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
                  label: l10n.terminalReconnect,
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
class _PasswordPromptCard extends ConsumerStatefulWidget {
  const _PasswordPromptCard({required this.tab, required this.prompt, super.key});

  final TerminalTab tab;
  final TerminalPasswordPrompt prompt;

  @override
  ConsumerState<_PasswordPromptCard> createState() => _PasswordPromptCardState();
}

class _PasswordPromptCardState extends ConsumerState<_PasswordPromptCard> {
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
              Icon(Icons.password_rounded, color: tokens.palette.accent, size: 22),
              const SizedBox(width: GlassSpacing.s8),
              Expanded(
                child: Text(
                  l10n.terminalPasswordPromptTitle(widget.prompt.username, widget.prompt.hostLabel),
                  style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
                ),
              ),
            ],
          ),
          if (widget.prompt.retry) ...[
            const SizedBox(height: GlassSpacing.s6),
            GateErrorText(text: l10n.terminalPasswordRetry),
          ],
          const SizedBox(height: GlassSpacing.s12),
          SecretField(
            key: const ValueKey('terminal-password'),
            controller: _password,
            label: l10n.terminalPasswordLabel,
            autofocus: true,
            helper: l10n.terminalPasswordHelper,
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
