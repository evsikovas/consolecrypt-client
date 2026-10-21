import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:xterm/xterm.dart';

Future<bool> openKubePodTerminal(
  BuildContext context, {
  required KubeSession session,
  required String contextName,
  required KubeResourceRef resource,
  required List<String> containers,
}) async =>
    await showAppDialog<bool>(
      context,
      secure: true,
      builder: (_) =>
          _PodTerminalDialog(session: session, contextName: contextName, resource: resource, containers: containers),
    ) ??
    false;

class _PodTerminalDialog extends ConsumerStatefulWidget {
  const _PodTerminalDialog({
    required this.session,
    required this.contextName,
    required this.resource,
    required this.containers,
  });
  final KubeSession session;
  final String contextName;
  final KubeResourceRef resource;
  final List<String> containers;
  @override
  ConsumerState<_PodTerminalDialog> createState() => _PodTerminalDialogState();
}

class _PodTerminalDialogState extends ConsumerState<_PodTerminalDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  late String? _container = widget.containers.firstOrNull;
  String _shell = '/bin/sh';
  bool _attach = false, _busy = false, _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));

  Future<void> _open() async {
    if (!_current || _busy || !widget.session.canExec || widget.resource.namespace == null) return;
    final controller = ref.read(terminalTabsProvider.notifier);
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final tab = await controller.openTarget(
        KubernetesTerminalTarget(
          sessionId: widget.session.id,
          contextName: widget.contextName,
          namespace: widget.resource.namespace!,
          pod: widget.resource.name,
          container: _container,
          command: _attach ? null : [_shell],
        ),
      );
      if (!mounted || !_current) {
        await controller.close(tab);
        return;
      }
      closeDialog(context, true);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    return GlassDialog(
      key: const ValueKey('kube-terminal-dialog'),
      title: l.kubePodTerminal,
      content: !_current
          ? const SizedBox.shrink()
          : SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text('${widget.contextName} · ${widget.resource.namespace}/${widget.resource.name}'),
                  if (!widget.session.canExec) Text(l.kubeExecProductionBlocked),
                  if (widget.containers.isNotEmpty)
                    GlassDropdownField<String>(
                      key: const ValueKey('kube-terminal-container'),
                      value: _container,
                      label: l.kubeContainer,
                      items: [for (final name in widget.containers) GlassSelectItem(value: name, label: name)],
                      onChanged: _busy ? null : (value) => setState(() => _container = value),
                    ),
                  SwitchListTile(
                    key: const ValueKey('kube-terminal-attach'),
                    contentPadding: EdgeInsets.zero,
                    title: Text(l.kubeAttach),
                    subtitle: Text(l.kubeAttachHelp),
                    value: _attach,
                    onChanged: _busy ? null : (value) => setState(() => _attach = value),
                  ),
                  if (!_attach)
                    GlassDropdownField<String>(
                      key: const ValueKey('kube-terminal-shell'),
                      value: _shell,
                      label: l.kubeShell,
                      items: [
                        GlassSelectItem(value: '/__consolecrypt_auto_shell__', label: l.kubeAutoShell),
                        for (final shell in const ['/bin/sh', '/bin/bash', '/bin/ash'])
                          GlassSelectItem(value: shell, label: shell),
                      ],
                      onChanged: _busy ? null : (value) => setState(() => _shell = value),
                    ),
                  if (_busy) const LinearProgressIndicator(),
                  if (_failed) Text(l.kubeTerminalFailed, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                ],
              ),
            ),
      primaryAction: GlassButton.prominent(
        key: const ValueKey('kube-terminal-open'),
        label: _attach ? l.kubeAttach : l.kubeOpenShell,
        onPressed: !_busy && widget.session.canExec && _current ? _open : null,
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog(context, false))],
    );
  }
}

/// Reuses the application's terminal controller: bounded scrollback, keyboard,
/// selection, snippets and native WebSocket resizing have one implementation.
class KubeTerminalPanel extends ConsumerStatefulWidget {
  const KubeTerminalPanel({
    required this.sessionId,
    required this.onHide,
    this.closeRouteOnLock = false,
    this.extraSessions = const {},
    super.key,
  });
  final String sessionId;
  final Set<TerminalSessionId> extraSessions;
  final VoidCallback onHide;
  final bool closeRouteOnLock;
  @override
  ConsumerState<KubeTerminalPanel> createState() => _KubeTerminalPanelState();
}

class _KubeTerminalPanelState extends ConsumerState<KubeTerminalPanel> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  TerminalTab? _selected;
  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next) || !widget.closeRouteOnLock) return;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(kubeScopeProvider))) return const SizedBox.shrink();
    final l = context.l10n;
    final state = ref.watch(terminalTabsProvider);
    final tabs = state.tabs
        .where(
          (tab) =>
              widget.extraSessions.contains(tab.sessionId) ||
              (tab.target is KubernetesTerminalTarget &&
                  (tab.target as KubernetesTerminalTarget).sessionId == widget.sessionId),
        )
        .toList();
    final selected = tabs.contains(_selected)
        ? _selected
        : tabs.contains(state.active)
        ? state.active
        : tabs.lastOrNull;
    return SecureSurface(
      opaque: true,
      padding: const EdgeInsets.all(8),
      shadow: false,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: Row(
                    children: [
                      for (final tab in tabs)
                        Padding(
                          padding: const EdgeInsets.only(right: 6),
                          child: InputChip(
                            key: ValueKey('kube-terminal-tab-${tab.sessionId}'),
                            label: Text(
                              tab.target is KubernetesTerminalTarget
                                  ? ((tab.target as KubernetesTerminalTarget).container ?? tab.target.label)
                                  : tab.target.label,
                            ),
                            selected: identical(tab, selected),
                            onSelected: (_) {
                              setState(() => _selected = tab);
                              ref.read(terminalTabsProvider.notifier).activate(state.tabs.indexOf(tab));
                            },
                            onDeleted: () => unawaited(ref.read(terminalTabsProvider.notifier).close(tab)),
                            deleteButtonTooltipMessage: l.commonClose,
                          ),
                        ),
                    ],
                  ),
                ),
              ),
              GlassIconButton(icon: Icons.keyboard_arrow_down, tooltip: l.kubeHidePanel, onPressed: widget.onHide),
            ],
          ),
          if (selected == null)
            Expanded(child: Center(child: Text(l.kubeTerminalEmpty)))
          else ...[
            Text(
              '${selected.target.label} · ${switch (selected.state) {
                SessionConnectionState.connected => l.kubeTerminalConnected,
                SessionConnectionState.connecting => l.kubeConnecting,
                _ => l.kubeTerminalDisconnected,
              }}',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: GlassTokens.of(context).typography.caption.copyWith(color: GlassTokens.of(context).secondaryLabel),
            ),
            const SizedBox(height: GlassSpacing.s4),
            Expanded(
              child: TerminalPane(key: ValueKey((selected.sessionId, selected.renderGeneration)), tab: selected),
            ),
            if (AppPlatform.isMobile)
              SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  children: [
                    for (final key in [
                      ('Esc', TerminalKey.escape, false),
                      ('Tab', TerminalKey.tab, false),
                      ('Ctrl+C', TerminalKey.keyC, true),
                      ('Ctrl+D', TerminalKey.keyD, true),
                      ('↑', TerminalKey.arrowUp, false),
                      ('↓', TerminalKey.arrowDown, false),
                      ('←', TerminalKey.arrowLeft, false),
                      ('→', TerminalKey.arrowRight, false),
                    ])
                      TextButton(
                        onPressed: selected.isConnected ? () => selected.terminal.keyInput(key.$2, ctrl: key.$3) : null,
                        child: Text(key.$1),
                      ),
                  ],
                ),
              ),
          ],
        ],
      ),
    );
  }
}
