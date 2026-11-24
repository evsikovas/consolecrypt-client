import 'dart:io';

import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

const terminalOnlyFlag = '--terminal-only';

Future<void> openTerminalOnlyWindow() async {
  await Process.start(Platform.resolvedExecutable, [terminalOnlyFlag], mode: ProcessStartMode.detached);
}

Future<bool> showTerminalOnlyWindow(BuildContext context) async {
  try {
    await openTerminalOnlyWindow();
    return true;
  } on Object {
    if (context.mounted) showSnack(context, context.l10n.terminalOnlyWindowError, error: true);
    return false;
  }
}

/// A separate local-terminal process started with --terminal-only.
final terminalOnlyModeProvider = Provider<bool>((ref) => false);

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
    if (!state) ref.read(workspaceToolsProvider.notifier).close();
    state = !state;
  }

  void leave() => state = false;
}
