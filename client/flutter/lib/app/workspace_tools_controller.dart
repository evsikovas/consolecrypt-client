import 'dart:async';

import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

enum WorkspaceTool { snippets, notes, ai, forwards, server, docker, history }

final activeSshTerminalProvider = Provider<TerminalTab?>(
  (ref) => ref.watch(
    terminalTabsProvider.select((state) {
      final tab = state.active;
      return tab != null && tab.host != null && tab.isConnected && !tab.exited ? tab : null;
    }),
  ),
);

final sessionHasDockerProvider = StreamProvider.autoDispose.family<bool, TerminalTab>((ref, tab) {
  final controller = StreamController<bool>();
  void emit() => controller.add(tab.hostMetrics.value.error == null && tab.hostMetrics.value.latest?.docker == true);
  tab.hostMetrics.addListener(emit);
  emit();
  ref.onDispose(() {
    tab.hostMetrics.removeListener(emit);
    unawaited(controller.close());
  });
  return controller.stream.distinct();
});

final class WorkspaceToolsState {
  const WorkspaceToolsState({this.selected, this.width = 420});
  final WorkspaceTool? selected;
  final double width;
}

final workspaceToolsProvider = NotifierProvider<WorkspaceToolsController, WorkspaceToolsState>(
  WorkspaceToolsController.new,
);

/// Session-only workspace state. Never synced or shared between profiles.
class WorkspaceToolsController extends Notifier<WorkspaceToolsState> {
  @override
  WorkspaceToolsState build() {
    ref.listen(activeProfileProvider.select((p) => p?.id), (previous, next) {
      if (previous != next) state = const WorkspaceToolsState();
    });
    ref.listen(vaultStatusProvider.select((s) => s.value?.isUnlocked ?? false), (_, unlocked) {
      if (!unlocked) state = const WorkspaceToolsState();
    });
    return const WorkspaceToolsState();
  }

  void open(WorkspaceTool tool) => state = WorkspaceToolsState(selected: tool, width: state.width);
  void close() => state = WorkspaceToolsState(width: state.width);
  void toggle(WorkspaceTool tool) => state.selected == tool ? close() : open(tool);
  void resize(double width) => state = WorkspaceToolsState(selected: state.selected, width: width.clamp(360, 560));
}
