import 'dart:async';

import 'package:consolecrypt/ai/command_palette.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

enum AppCommandId { commandPalette, newTerminalTab, closeTerminalTab, newHost, lockVault, openSettings, snippets }

/// A global command with its platform shortcut (Cmd on macOS, Ctrl on
/// Windows). Shown in the macOS menu bar and handled in-app everywhere.
final class AppCommand {
  const AppCommand(this.id, this.key, {this.shift = false});

  final AppCommandId id;
  final LogicalKeyboardKey key;
  final bool shift;

  SingleActivator get activator => AppPlatform.primary(key, shift: shift);

  String get shortcutLabel => AppPlatform.shortcutLabel(key, shift: shift);

  /// Menu / palette title in the UI language.
  String label(AppLocalizations l) => switch (id) {
    AppCommandId.commandPalette => l.commandPalette,
    AppCommandId.newTerminalTab => l.commandNewTerminalTab,
    AppCommandId.closeTerminalTab => l.commandCloseTerminalTab,
    AppCommandId.newHost => l.commandNewHost,
    AppCommandId.lockVault => l.commandLockVault,
    AppCommandId.openSettings => l.commandOpenSettings,
    AppCommandId.snippets => l.navSnippets,
  };
}

const appCommands = [
  AppCommand(AppCommandId.commandPalette, LogicalKeyboardKey.keyK),
  AppCommand(AppCommandId.newTerminalTab, LogicalKeyboardKey.keyT),
  AppCommand(AppCommandId.closeTerminalTab, LogicalKeyboardKey.keyW),
  AppCommand(AppCommandId.newHost, LogicalKeyboardKey.keyN),
  AppCommand(AppCommandId.lockVault, LogicalKeyboardKey.keyL),
  AppCommand(AppCommandId.openSettings, LogicalKeyboardKey.comma),
  AppCommand(AppCommandId.snippets, LogicalKeyboardKey.period),
];

AppCommand commandFor(AppCommandId id) => appCommands.firstWhere((c) => c.id == id);

class AppCommandIntent extends Intent {
  const AppCommandIntent(this.id);

  final AppCommandId id;
}

/// Executes [AppCommand]s. Invocations of the same command within 250 ms
/// are collapsed: on macOS both the native menu item and the in-app
/// shortcut may observe one key press.
final class AppCommandDispatcher {
  AppCommandDispatcher(this._ref);

  final Ref _ref;
  final Map<AppCommandId, DateTime> _last = {};

  bool get enabled => _ref.read(appStageProvider) == AppStage.unlocked;

  /// Handles app shortcuts before an embedded widget (the terminal)
  /// consumes them. Returns true if [event] triggered a command.
  bool handleKeyEvent(KeyEvent event) {
    if (event is! KeyDownEvent || !enabled) return false;
    for (final command in appCommands) {
      if (command.activator.accepts(event, HardwareKeyboard.instance)) {
        invoke(command.id);
        return true;
      }
    }
    return false;
  }

  void invoke(AppCommandId id) {
    if (!enabled) return;
    final now = DateTime.now();
    final last = _last[id];
    if (last != null && now.difference(last) < const Duration(milliseconds: 250)) return;
    _last[id] = now;

    final router = _ref.read(routerProvider);
    final context = rootNavigatorKey.currentContext;
    switch (id) {
      case AppCommandId.commandPalette:
        if (context != null) unawaited(showCommandPalette(context));
      case AppCommandId.newTerminalTab:
        if (context != null) unawaited(_newTab(context));
      case AppCommandId.closeTerminalTab:
        if (router.state.matchedLocation.startsWith(AppRoutes.terminal)) {
          unawaited(_ref.read(terminalTabsProvider.notifier).closeActive());
        }
      case AppCommandId.newHost:
        router.go(AppRoutes.newHost);
      case AppCommandId.lockVault:
        unawaited(_ref.read(vaultServiceProvider).lock());
      case AppCommandId.openSettings:
        router.go(AppRoutes.settings);
      case AppCommandId.snippets:
        _ref.read(workspaceToolsProvider.notifier).toggle(WorkspaceTool.snippets);
    }
  }

  Future<void> _newTab(BuildContext context) async {
    final host = await showHostPicker(context, title: context.l10n.commandNewTabPickerTitle);
    if (host == null) return;
    await _ref.read(terminalTabsProvider.notifier).open(host);
    _ref.read(routerProvider).go(AppRoutes.terminal);
  }
}

final appCommandDispatcherProvider = Provider<AppCommandDispatcher>(AppCommandDispatcher.new);
