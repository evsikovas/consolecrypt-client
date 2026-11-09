import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Presentation only: never owns or restarts a terminal session. Not synced.
final terminalMinimalModeProvider = NotifierProvider<TerminalMinimalMode, bool>(TerminalMinimalMode.new);

class TerminalMinimalMode extends Notifier<bool> {
  @override
  bool build() {
    ref.listen(activeProfileProvider.select((p) => p?.id), (_, _) => leave());
    ref.listen(vaultStatusProvider.select((s) => s.value?.isUnlocked ?? false), (_, unlocked) {
      if (!unlocked) leave();
    });
    return false;
  }

  void toggle() {
    if (AppPlatform.isMobile) return;
    if (!state) ref.read(workspaceToolsProvider.notifier).close();
    state = !state;
  }

  void leave() => state = false;
}
