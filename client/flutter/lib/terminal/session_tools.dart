import 'dart:async';

import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_format.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:consolecrypt/docker/docker_workspace.dart';
import 'package:consolecrypt/snippets/run_flow.dart';
import 'package:consolecrypt/snippets/snippet_editor.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final _engine = FutureProvider.autoDispose.family<DockerSessionInfo, TerminalTab>((ref, tab) async {
  final service = ref.watch(dockerServiceProvider)!;
  var disposed = false;
  DockerSessionInfo? connected;
  ref.onDispose(() {
    disposed = true;
    if (connected case final session?) unawaited(service.disconnect(session.id).catchError((Object _) {}));
  });
  final session = await service.connect(tab.host!.id.value);
  connected = session;
  if (disposed) await service.disconnect(session.id);
  return session;
});

class SessionDockerPanel extends ConsumerWidget {
  const SessionDockerPanel({required this.tab, super.key});
  final TerminalTab tab;
  @override
  Widget build(BuildContext context, WidgetRef ref) => ref
      .watch(_engine(tab))
      .when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (e, _) => Center(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(dockerErrorText(context.l10n, e)),
              TextButton(onPressed: () => ref.invalidate(_engine(tab)), child: Text(context.l10n.dockerRetry)),
            ],
          ),
        ),
        data: (s) => DockerWorkspace(
          service: ref.watch(dockerServiceProvider)!,
          session: s,
          sessionPanel: true,
          onDisconnect: () => ref.read(workspaceToolsProvider.notifier).close(),
        ),
      );
}

final _commands = StreamProvider.autoDispose.family<List<String>, TerminalTab>((ref, tab) async* {
  final svc = ref.watch(terminalServiceProvider);
  if (svc is! TerminalHistoryReader ||
      ref.watch(vaultSettingsProvider).value?.terminalHistoryMode == TerminalHistoryMode.disabled) {
    yield const [];
    return;
  }
  var closed = false;
  ref.onDispose(() => closed = true);
  while (!closed) {
    final rows = await (svc as TerminalHistoryReader).commands(tab.sessionId);
    if (closed) return;
    yield rows;
    if (!tab.isLocal) return;
    await Future<void>.delayed(const Duration(seconds: 2));
  }
});

class SessionHistoryPanel extends ConsumerWidget {
  const SessionHistoryPanel({required this.tab, super.key});
  final TerminalTab tab;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n, t = GlassTokens.of(context);
    if (ref.watch(vaultSettingsProvider).value?.terminalHistoryMode == TerminalHistoryMode.disabled) {
      return Center(child: Text(l.dbHistoryDisabled));
    }
    final commands = ref.watch(_commands(tab));
    final list = commands.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(child: Text(l.sessionHistoryUnavailable)),
      data: (rows) => rows.isEmpty
          ? Center(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Text(tab.isLocal ? l.sessionHistoryEmpty : l.serverHistoryEmpty, textAlign: TextAlign.center),
              ),
            )
          : ListView.separated(
              itemCount: rows.length,
              separatorBuilder: (_, _) => const Divider(height: 1),
              itemBuilder: (_, i) {
                final command = rows[i];
                return ListTile(
                  dense: true,
                  title: SelectableText(command, style: t.typography.mono.copyWith(fontSize: 12)),
                  trailing: GlassMenuButton<String>(
                    entries: [
                      if (!tab.isLocal)
                        GlassMenuItem(value: 'run', label: l.codeBlocksRun, icon: Icons.play_arrow_rounded),
                      GlassMenuItem(value: 'insert', label: l.codeBlocksInsert, icon: Icons.keyboard_return_rounded),
                      GlassMenuItem(value: 'copy', label: l.commonCopy, icon: Icons.copy_rounded),
                      GlassMenuItem(value: 'snippet', label: l.sessionSnip, icon: Icons.bookmark_add_outlined),
                    ],
                    onSelected: (action) async {
                      if (!identical(tab, ref.read(terminalTabsProvider).active) || !tab.isConnected || tab.exited) {
                        return;
                      }
                      switch (action) {
                        case 'run':
                          await runCommandFlow(context, ref, command: command, expectedTab: tab);
                        case 'insert':
                          ref.read(terminalTabsProvider.notifier).insertIntoActive(command);
                        case 'copy':
                          await copyPlainWithNotice(context, ref, command, what: l.copyWhatCommand);
                        case 'snippet':
                          await showSnippetEditor(context, initialTemplate: command);
                      }
                    },
                    builder: (context, open) => GlassIconButton(
                      icon: Icons.more_horiz_rounded,
                      tooltip: l.hostsMoreTooltip,
                      style: GlassIconButtonStyle.plain,
                      size: 28,
                      onPressed: open,
                    ),
                  ),
                );
              },
            ),
    );
    if (tab.isLocal) return list;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsetsDirectional.fromSTEB(12, 4, 8, 4),
          child: Row(
            children: [
              Expanded(child: Text(l.serverShellHistory, style: t.typography.bodyEmph)),
              GlassIconButton(
                tooltip: l.dockerRefresh,
                icon: Icons.refresh_rounded,
                size: 28,
                style: GlassIconButtonStyle.plain,
                onPressed: commands.isLoading ? null : () => ref.invalidate(_commands(tab)),
              ),
            ],
          ),
        ),
        const Divider(height: 1),
        Expanded(child: list),
      ],
    );
  }
}
