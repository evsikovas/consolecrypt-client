import 'dart:async';

import 'package:consolecrypt/ai/command_palette.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/sidebar_state.dart';
import 'package:consolecrypt/app/theme/glass_metrics.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:consolecrypt/terminal/terminal_target_picker.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

enum AppCommandId {
  commandPalette,
  newTerminalTab,
  closeTerminalTab,
  newHost,
  lockVault,
  openSettings,
  snippets,
  toggleSidebar,
}

/// A key combination: the platform primary modifier (⌘ on macOS, Ctrl
/// elsewhere) and/or Alt, optionally with Shift.
@immutable
final class ShortcutBinding {
  const ShortcutBinding(this.key, {this.primary = true, this.shift = false, this.alt = false});

  final LogicalKeyboardKey key;
  final bool primary, shift, alt;

  SingleActivator get activator => SingleActivator(
    key,
    meta: primary && AppPlatform.usesMeta,
    control: primary && !AppPlatform.usesMeta,
    shift: shift,
    alt: alt,
  );

  String get label => AppPlatform.shortcutLabel(key, shift: shift, alt: alt, primary: primary);

  /// Settings form: modifier letters, a colon and the logical key id.
  String encode() => '${primary ? 'p' : ''}${shift ? 's' : ''}${alt ? 'a' : ''}:${key.keyId}';

  static ShortcutBinding? decode(String value) {
    final (mods, id) = switch (value.split(':')) {
      [final mods, final id] => (mods, int.tryParse(id)),
      _ => ('', null),
    };
    final key = id == null ? null : LogicalKeyboardKey.findKeyByKeyId(id);
    if (key == null) return null;
    final binding = ShortcutBinding(
      key,
      primary: mods.contains('p'),
      shift: mods.contains('s'),
      alt: mods.contains('a'),
    );
    return binding.valid ? binding : null;
  }

  /// App shortcuts need ⌘/Ctrl or Alt so plain typing always reaches the
  /// terminal, and a real key (not a lone modifier).
  bool get valid => (primary || alt) && !_modifiers.contains(key);

  static final _modifiers = {
    ...LogicalKeyboardKey.expandSynonyms({
      LogicalKeyboardKey.shift,
      LogicalKeyboardKey.control,
      LogicalKeyboardKey.alt,
      LogicalKeyboardKey.meta,
    }),
    LogicalKeyboardKey.shift,
    LogicalKeyboardKey.control,
    LogicalKeyboardKey.alt,
    LogicalKeyboardKey.meta,
    LogicalKeyboardKey.capsLock,
    LogicalKeyboardKey.fn,
  };

  /// The combination currently held with [key] (from a key-down event).
  static ShortcutBinding fromKey(LogicalKeyboardKey key) {
    final keyboard = HardwareKeyboard.instance;
    return ShortcutBinding(
      key,
      primary: AppPlatform.usesMeta ? keyboard.isMetaPressed : keyboard.isControlPressed,
      shift: keyboard.isShiftPressed,
      alt: keyboard.isAltPressed,
    );
  }

  /// Taken by the OS or by the terminal (copy/paste, shell editing).
  bool get reserved {
    String k(LogicalKeyboardKey key, {bool shift = false}) => ShortcutBinding(key, shift: shift).encode();
    final taken = AppPlatform.usesMeta
        ? {
            for (final key in [
              LogicalKeyboardKey.keyQ,
              LogicalKeyboardKey.keyH,
              LogicalKeyboardKey.keyM,
              LogicalKeyboardKey.keyC,
              LogicalKeyboardKey.keyV,
              LogicalKeyboardKey.keyX,
              LogicalKeyboardKey.keyA,
              LogicalKeyboardKey.keyZ,
              LogicalKeyboardKey.space,
              LogicalKeyboardKey.tab,
            ])
              k(key),
            k(LogicalKeyboardKey.keyZ, shift: true),
          }
        : {
            for (final key in [
              LogicalKeyboardKey.keyC,
              LogicalKeyboardKey.keyV,
              LogicalKeyboardKey.keyZ,
              LogicalKeyboardKey.keyD,
              LogicalKeyboardKey.keyR,
              LogicalKeyboardKey.keyA,
              LogicalKeyboardKey.keyE,
              LogicalKeyboardKey.keyU,
            ])
              k(key),
            k(LogicalKeyboardKey.keyC, shift: true),
            k(LogicalKeyboardKey.keyV, shift: true),
            const ShortcutBinding(LogicalKeyboardKey.f4, primary: false, alt: true).encode(),
          };
    return taken.contains(encode());
  }

  @override
  bool operator ==(Object other) =>
      other is ShortcutBinding &&
      other.key == key &&
      other.primary == primary &&
      other.shift == shift &&
      other.alt == alt;

  @override
  int get hashCode => Object.hash(key, primary, shift, alt);
}

/// A global command with its shortcut. Shown in the macOS menu bar and
/// handled in-app everywhere. [binding] is null when the user turned the
/// shortcut off.
final class AppCommand {
  const AppCommand(this.id, this.binding);

  final AppCommandId id;
  final ShortcutBinding? binding;

  SingleActivator? get activator => binding?.activator;

  String get shortcutLabel => binding?.label ?? '';

  /// Menu / palette title in the UI language.
  String label(AppLocalizations l) => switch (id) {
    AppCommandId.commandPalette => l.commandPalette,
    AppCommandId.newTerminalTab => l.commandNewTerminalTab,
    AppCommandId.closeTerminalTab => l.commandCloseTerminalTab,
    AppCommandId.newHost => l.commandNewHost,
    AppCommandId.lockVault => l.commandLockVault,
    AppCommandId.openSettings => l.commandOpenSettings,
    AppCommandId.snippets => l.navSnippets,
    AppCommandId.toggleSidebar => l.commandToggleSidebar,
  };
}

/// Factory shortcuts.
const appCommands = [
  AppCommand(AppCommandId.commandPalette, ShortcutBinding(LogicalKeyboardKey.keyK)),
  AppCommand(AppCommandId.newTerminalTab, ShortcutBinding(LogicalKeyboardKey.keyT)),
  AppCommand(AppCommandId.closeTerminalTab, ShortcutBinding(LogicalKeyboardKey.keyW)),
  AppCommand(AppCommandId.newHost, ShortcutBinding(LogicalKeyboardKey.keyN)),
  AppCommand(AppCommandId.lockVault, ShortcutBinding(LogicalKeyboardKey.keyL)),
  AppCommand(AppCommandId.openSettings, ShortcutBinding(LogicalKeyboardKey.comma)),
  AppCommand(AppCommandId.snippets, ShortcutBinding(LogicalKeyboardKey.period)),
  AppCommand(AppCommandId.toggleSidebar, ShortcutBinding(LogicalKeyboardKey.keyS, shift: true)),
];

/// Commands with the user's overrides (`id → encoded binding`, empty =
/// turned off); unknown or invalid values fall back to the factory one.
List<AppCommand> resolveAppCommands(Map<String, String> overrides) => [
  for (final command in appCommands)
    switch (overrides[command.id.name]) {
      null => command,
      '' => AppCommand(command.id, null),
      final value => AppCommand(command.id, ShortcutBinding.decode(value) ?? command.binding),
    },
];

final appCommandsProvider = Provider<List<AppCommand>>(
  (ref) => resolveAppCommands(ref.watch(localSettingsProvider.select((s) => s.value?.shortcutOverrides)) ?? const {}),
);

/// True while the settings capture a new shortcut: app shortcuts and the
/// macOS menu key equivalents step aside so the combination can be read.
final shortcutCaptureProvider = NotifierProvider<ShortcutCapture, bool>(ShortcutCapture.new);

final class ShortcutCapture extends Notifier<bool> {
  @override
  bool build() => false;

  set active(bool value) => state = value;
}

AppCommand commandFor(AppCommandId id, [List<AppCommand> commands = appCommands]) =>
    commands.firstWhere((c) => c.id == id);

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
    if (event is! KeyDownEvent || !enabled || _ref.read(shortcutCaptureProvider)) return false;
    for (final command in _ref.read(appCommandsProvider)) {
      if (command.activator?.accepts(event, HardwareKeyboard.instance) ?? false) {
        invoke(command.id);
        return true;
      }
    }
    return false;
  }

  void invoke(AppCommandId id) {
    if (!enabled || _ref.read(shortcutCaptureProvider)) return;
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
      case AppCommandId.toggleSidebar:
        // Phones have no sidebar; narrow windows keep it compact (no toggle).
        final narrow = context != null && MediaQuery.sizeOf(context).width < GlassSizes.compactBreakpoint;
        if (!AppPlatform.isMobile && !narrow) _ref.read(sidebarCollapsedProvider.notifier).toggle();
    }
  }

  Future<void> _newTab(BuildContext context) async {
    final profile = _ref.read(activeProfileProvider)?.id;
    final target = await showTerminalTargetPicker(context);
    if (target == null || !context.mounted || !enabled || profile != _ref.read(activeProfileProvider)?.id) return;
    final tab = await runWithFeedback(context, () => _ref.read(terminalTabsProvider.notifier).openTarget(target));
    if (tab != null && enabled) _ref.read(routerProvider).go(AppRoutes.terminal);
  }
}

final appCommandDispatcherProvider = Provider<AppCommandDispatcher>(AppCommandDispatcher.new);
