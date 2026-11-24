import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/tmux_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final tmuxVersionProvider = FutureProvider.autoDispose.family<String?, TerminalTab>((ref, parent) async {
  final service = ref.watch(terminalServiceProvider);
  if (service is! TmuxService) return null;
  return (service as TmuxService).tmuxVersion(parent.sessionId);
});

final _sessions = FutureProvider.autoDispose.family<List<TmuxSession>, TerminalTab>((ref, parent) {
  return (ref.watch(terminalServiceProvider) as TmuxService).tmuxSessions(parent.sessionId);
});

class TmuxPanel extends ConsumerStatefulWidget {
  const TmuxPanel({required this.parent, super.key});
  final TerminalTab parent;
  @override
  ConsumerState<TmuxPanel> createState() => _TmuxPanelState();
}

class _TmuxPanelState extends ConsumerState<TmuxPanel> {
  late final Timer _poll;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    _poll = Timer.periodic(const Duration(seconds: 5), (_) {
      if (!_busy && !ref.read(_sessions(widget.parent)).isLoading) ref.invalidate(_sessions(widget.parent));
    });
  }

  @override
  void dispose() {
    _poll.cancel();
    super.dispose();
  }

  bool _current(TerminalTab? tab) =>
      mounted &&
      tab != null &&
      identical(ref.read(terminalTabsProvider).active, tab) &&
      identical(tab.tmuxParent ?? tab, widget.parent) &&
      tab.isConnected &&
      !tab.exited;

  Future<void> _perform(Future<void> Function(TerminalTab tab, TmuxService service) operation) async {
    final tab = ref.read(terminalTabsProvider).active;
    if (_busy || !_current(tab)) return;
    setState(() => _busy = true);
    try {
      await operation(tab!, ref.read(terminalServiceProvider) as TmuxService);
    } on Object {
      if (mounted) showSnack(context, context.l10n.tmuxError, error: true);
    } finally {
      if (mounted) {
        setState(() => _busy = false);
        ref.invalidate(_sessions(widget.parent));
      }
    }
  }

  Future<void> _name([TmuxSession? session]) => _perform((tab, service) async {
    final l = context.l10n;
    final name = (await showTextInputDialog(
      context,
      title: session == null ? l.tmuxCreate : l.commonRename,
      initial: session?.name ?? '',
      label: l.commonName,
      confirmLabel: session == null ? l.commonCreate : l.commonSave,
    ))?.trim();
    if (name == null || !_current(tab)) return;
    if (name.isEmpty ||
        utf8.encode(name).length > 128 ||
        name.startsWith('-') ||
        !RegExp(r'^[\p{L}\p{N} _-]+$', unicode: true).hasMatch(name)) {
      if (mounted) showSnack(context, l.tmuxNameHint, error: true);
      return;
    }
    if (session == null) {
      await service.createTmux(widget.parent.sessionId, name);
    } else {
      await service.renameTmux(widget.parent.sessionId, session, name);
      if (mounted && session.matches(tab.tmuxSession)) {
        ref.read(terminalTabsProvider.notifier).rename(tab, 'tmux · $name');
      }
    }
  });

  Future<void> _kill(TmuxSession session) => _perform((tab, service) async {
    final l = context.l10n;
    final ok = await showConfirmDialog(
      context,
      title: l.tmuxKill,
      message: l.tmuxConfirmKill(session.name),
      confirmLabel: l.tmuxKill,
      destructive: true,
    );
    if (ok && _current(tab)) await service.killTmux(widget.parent.sessionId, session);
  });

  Widget _button(IconData icon, String label, VoidCallback action, {Color? color}) => GlassIconButton(
    icon: icon,
    tooltip: label,
    size: 28,
    iconSize: 16,
    style: GlassIconButtonStyle.plain,
    color: color,
    onPressed: _busy ? null : action,
  );

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final t = GlassTokens.of(context);
    final parent = widget.parent;
    final version = ref.watch(tmuxVersionProvider(parent)).value ?? 'tmux';
    final rows = ref.watch(_sessions(parent));
    final tab = ref.watch(terminalTabsProvider.select((s) => s.active));
    final date = MaterialLocalizations.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 4, 8, 4),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  [version, if (rows.value case final entries?) l.tmuxCount(entries.length)].join(' · '),
                  style: t.typography.caption,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (tab?.tmuxParent == parent)
                _button(
                  Icons.logout_rounded,
                  l.tmuxDetach,
                  () => _perform((current, _) => ref.read(terminalTabsProvider.notifier).detachTmux(current)),
                ),
              _button(Icons.refresh_rounded, l.dockerRefresh, () => ref.invalidate(_sessions(parent))),
              _button(Icons.add_rounded, l.tmuxCreate, () => _name()),
            ],
          ),
        ),
        Expanded(
          child: rows.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (_, _) => Center(
              child: TextButton.icon(
                onPressed: () => ref.invalidate(_sessions(parent)),
                icon: const Icon(Icons.refresh_rounded),
                label: Text(l.dockerRetry),
              ),
            ),
            data: (entries) => entries.isEmpty
                ? Center(child: Text(l.tmuxEmpty, style: t.typography.caption))
                : ListView.separated(
                    padding: const EdgeInsets.fromLTRB(8, 4, 8, 8),
                    itemCount: entries.length,
                    separatorBuilder: (_, _) => const SizedBox(height: 6),
                    itemBuilder: (context, i) {
                      final entry = entries[i];
                      final attached = entry.matches(tab?.tmuxSession);
                      final activity =
                          '${date.formatShortDate(entry.activity)} ${date.formatTimeOfDay(TimeOfDay.fromDateTime(entry.activity))}';
                      return GlassPanel(
                        padding: const EdgeInsets.all(10),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Row(
                              children: [
                                Icon(
                                  Icons.layers_outlined,
                                  size: 17,
                                  color: attached ? Theme.of(context).colorScheme.primary : null,
                                ),
                                const SizedBox(width: 8),
                                Expanded(
                                  child: Text(
                                    entry.name,
                                    style: t.typography.bodyEmph,
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                  ),
                                ),
                                _button(
                                  attached ? Icons.logout_rounded : Icons.terminal_rounded,
                                  attached ? l.tmuxDetach : l.tmuxAttach,
                                  () => _perform(
                                    (current, _) => attached
                                        ? ref.read(terminalTabsProvider.notifier).detachTmux(current)
                                        : ref.read(terminalTabsProvider.notifier).attachTmux(current, entry),
                                  ),
                                ),
                                _button(Icons.edit_outlined, l.commonRename, () => _name(entry)),
                                _button(
                                  Icons.delete_outline_rounded,
                                  l.tmuxKill,
                                  () => _kill(entry),
                                  color: Theme.of(context).colorScheme.error,
                                ),
                              ],
                            ),
                            Text(l.tmuxCounts(entry.windows, entry.clients), style: t.typography.caption),
                            if (entry.path.isNotEmpty)
                              Padding(
                                padding: const EdgeInsets.only(top: 4),
                                child: SelectableText(entry.path, style: t.typography.mono.copyWith(fontSize: 11)),
                              ),
                            Text(l.tmuxActivity(activity), style: t.typography.caption),
                          ],
                        ),
                      );
                    },
                  ),
          ),
        ),
      ],
    );
  }
}
