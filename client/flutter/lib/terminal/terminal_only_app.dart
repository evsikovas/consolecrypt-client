import 'dart:async';
import 'dart:io';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/terminal/terminal_fonts.dart';
import 'package:consolecrypt/terminal/terminal_minimal_mode.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

const terminalOnlyFlag = '--terminal-only';

Future<void> _openTerminalWindow() async {
  await Process.start(Platform.resolvedExecutable, [terminalOnlyFlag], mode: ProcessStartMode.detached);
}

class TerminalOnlyApp extends StatelessWidget {
  const TerminalOnlyApp({
    super.key,
    required this.service,
    required this.initialShell,
    this.openWindow = _openTerminalWindow,
  });

  final TerminalService service;
  final LocalShellProfile? initialShell;
  final Future<void> Function() openWindow;

  @override
  Widget build(BuildContext context) => ProviderScope(
    overrides: [
      terminalOnlyModeProvider.overrideWithValue(true),
      terminalServiceProvider.overrideWithValue(service),
      localSettingsProvider.overrideWith((ref) => Stream.value(const LocalSettings())),
      terminalFontProvider.overrideWithValue(resolveTerminalFont(null, null)),
    ],
    child: MaterialApp(
      title: 'ConsoleCrypt',
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      home: _TerminalOnlyScreen(initialShell: initialShell, openWindow: openWindow),
    ),
  );
}

class _TerminalOnlyScreen extends ConsumerStatefulWidget {
  const _TerminalOnlyScreen({required this.initialShell, required this.openWindow});

  final LocalShellProfile? initialShell;
  final Future<void> Function() openWindow;

  @override
  ConsumerState<_TerminalOnlyScreen> createState() => _TerminalOnlyScreenState();
}

class _TerminalOnlyScreenState extends ConsumerState<_TerminalOnlyScreen> {
  bool _opening = false;
  bool _paletteOpen = false;
  String? _error;
  TerminalTabPlacement _placement = TerminalTabPlacement.top;

  @override
  void initState() {
    super.initState();
    if (widget.initialShell case final profile?) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) unawaited(_openShell(profile));
      });
    }
  }

  Future<void> _openShell(LocalShellProfile profile) async {
    if (_opening) return;
    setState(() {
      _opening = true;
      _error = null;
    });
    try {
      await ref.read(terminalTabsProvider.notifier).openTarget(LocalTerminalTarget(profile));
    } on Object {
      if (mounted) setState(() => _error = context.l10n.terminalOnlyShellError);
    } finally {
      if (mounted) setState(() => _opening = false);
    }
  }

  Future<void> _palette() async {
    if (_paletteOpen) return;
    _paletteOpen = true;
    final loadProfiles = ref.read(terminalServiceProvider).localProfiles;
    try {
      final selection = await showDialog<LocalShellProfile>(
        context: context,
        builder: (_) => _ShellPalette(loadProfiles: loadProfiles, openWindow: widget.openWindow),
      );
      if (mounted && selection != null) await _openShell(selection);
    } finally {
      _paletteOpen = false;
    }
  }

  bool _shortcut(KeyEvent event) {
    if (event is! KeyDownEvent ||
        !const [LogicalKeyboardKey.keyK, LogicalKeyboardKey.keyN, LogicalKeyboardKey.keyT].contains(event.logicalKey) ||
        !AppPlatform.primary(event.logicalKey).accepts(event, HardwareKeyboard.instance)) {
      return false;
    }
    unawaited(_palette());
    return true;
  }

  Widget _tabs(TerminalTabsState state, TerminalTabsController controller, {required bool left}) {
    final l = context.l10n;
    final buttons = [
      GlassIconButton(
        key: const ValueKey('terminal-only-new-tab'),
        icon: Icons.add_rounded,
        tooltip: '${l.commandNewTabPickerTitle} (${AppPlatform.shortcutLabel(LogicalKeyboardKey.keyT)})',
        size: 28,
        iconSize: 16,
        onPressed: () => unawaited(_palette()),
      ),
      GlassIconButton(
        key: const ValueKey('terminal-only-palette'),
        icon: Icons.keyboard_command_key_rounded,
        tooltip: '${l.terminalOnlyPalette} (${AppPlatform.shortcutLabel(LogicalKeyboardKey.keyK)})',
        size: 28,
        iconSize: 16,
        onPressed: () => unawaited(_palette()),
      ),
      PopupMenuButton<TerminalTabPlacement>(
        key: const ValueKey('terminal-only-tab-placement'),
        icon: Icon(left ? Icons.view_sidebar_outlined : Icons.tab_rounded, size: 16),
        tooltip: left ? l.terminalMinimalTabsLeft : l.terminalMinimalTabsTop,
        padding: EdgeInsets.zero,
        constraints: const BoxConstraints(minWidth: 160),
        onSelected: (value) => setState(() => _placement = value),
        itemBuilder: (context) => [
          CheckedPopupMenuItem(
            key: const ValueKey('terminal-only-tabs-top'),
            value: TerminalTabPlacement.top,
            checked: !left,
            child: Text(l.terminalMinimalTabsTop),
          ),
          CheckedPopupMenuItem(
            key: const ValueKey('terminal-only-tabs-left'),
            value: TerminalTabPlacement.left,
            checked: left,
            child: Text(l.terminalMinimalTabsLeft),
          ),
        ],
      ),
    ];
    return Flex(
      direction: left ? Axis.vertical : Axis.horizontal,
      children: [
        Expanded(
          child: ListView.separated(
            key: const ValueKey('terminal-only-tabs'),
            scrollDirection: left ? Axis.vertical : Axis.horizontal,
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 3),
            itemCount: state.tabs.length,
            separatorBuilder: (_, _) => left ? const SizedBox(height: 4) : const SizedBox(width: 4),
            itemBuilder: (context, index) {
              final tab = state.tabs[index];
              final button = GlassButton(
                key: ValueKey(tab.sessionId),
                label: tab.target.label,
                expand: left,
                style: identical(state.active, tab) ? GlassButtonStyle.prominent : GlassButtonStyle.plain,
                onPressed: () => controller.activate(index),
              );
              return Row(
                children: [
                  if (left) Expanded(child: button) else button,
                  GlassIconButton(
                    icon: Icons.close_rounded,
                    tooltip: l.commonClose,
                    size: 24,
                    iconSize: 14,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => unawaited(controller.close(tab)),
                  ),
                ],
              );
            },
          ),
        ),
        Row(mainAxisAlignment: MainAxisAlignment.center, mainAxisSize: MainAxisSize.min, children: buttons),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final tabs = ref.watch(terminalTabsProvider);
    final controller = ref.read(terminalTabsProvider.notifier);
    final active = tabs.active;
    final l = context.l10n;
    final left = _placement == TerminalTabPlacement.left;
    return Scaffold(
      body: Focus(
        autofocus: active == null,
        onKeyEvent: (_, event) => _shortcut(event) ? KeyEventResult.handled : KeyEventResult.ignored,
        child: Flex(
          direction: left ? Axis.horizontal : Axis.vertical,
          children: [
            SizedBox(
              height: left ? double.infinity : 36,
              width: left ? 156 : double.infinity,
              child: _tabs(tabs, controller, left: left),
            ),
            Expanded(
              child: Column(
                children: [
                  if (_opening || active?.state == SessionConnectionState.connecting) const LinearProgressIndicator(),
                  if (_error != null) Padding(padding: const EdgeInsets.all(8), child: Text(_error!)),
                  Expanded(
                    child: active == null
                        ? Center(
                            child: GlassButton(
                              label: l.paletteNewLocalTerminal,
                              onPressed: () => unawaited(_palette()),
                            ),
                          )
                        : TerminalPane(
                            key: ValueKey((active.sessionId, active.renderGeneration)),
                            tab: active,
                            onShortcut: _shortcut,
                          ),
                  ),
                  if (active?.state == SessionConnectionState.disconnected)
                    Padding(
                      padding: const EdgeInsets.all(8),
                      child: Text(active!.exited ? l.terminalOnlyShellExited : l.terminalOnlyShellError),
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _ShellPalette extends StatefulWidget {
  const _ShellPalette({required this.loadProfiles, required this.openWindow});

  final Future<LocalShellProfiles> Function() loadProfiles;
  final Future<void> Function() openWindow;

  @override
  State<_ShellPalette> createState() => _ShellPaletteState();
}

class _ShellPaletteState extends State<_ShellPalette> {
  late final Future<LocalShellProfiles> _profiles = widget.loadProfiles();
  String _query = '';
  bool _openingWindow = false, _windowFailed = false;

  Future<void> _openWindow() async {
    if (_openingWindow) return;
    setState(() => _openingWindow = true);
    try {
      await widget.openWindow();
      if (mounted && ModalRoute.of(context)?.isCurrent == true) Navigator.pop(context);
    } on Object {
      if (mounted) {
        setState(() {
          _windowFailed = true;
          _openingWindow = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return Dialog(
      backgroundColor: Theme.of(context).colorScheme.surface,
      child: SizedBox(
        width: 440,
        child: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .72),
          child: FutureBuilder<LocalShellProfiles>(
            future: _profiles,
            builder: (context, snapshot) {
              final matches = (snapshot.data?.profiles ?? const <LocalShellProfile>[])
                  .where((p) => '${p.label} ${p.id} ${p.shell}'.toLowerCase().contains(_query))
                  .toList();
              final showNewWindow = l.terminalOnlyNewWindow.toLowerCase().contains(_query);
              return ListView(
                shrinkWrap: true,
                primary: false,
                padding: const EdgeInsets.all(16),
                children: [
                  Text(l.terminalOnlyPalette, style: Theme.of(context).textTheme.titleMedium),
                  const SizedBox(height: 12),
                  TextField(
                    key: const ValueKey('terminal-only-search'),
                    autofocus: true,
                    decoration: InputDecoration(
                      prefixIcon: const Icon(Icons.search_rounded),
                      hintText: l.terminalLocalShellSearch,
                    ),
                    onChanged: (value) => setState(() => _query = value.trim().toLowerCase()),
                    onSubmitted: (_) {
                      if (matches.isNotEmpty) {
                        Navigator.pop(context, matches.first);
                      } else if (showNewWindow) {
                        unawaited(_openWindow());
                      }
                    },
                  ),
                  if (showNewWindow)
                    ListTile(
                      key: const ValueKey('terminal-only-new-window'),
                      leading: const Icon(Icons.open_in_new_rounded),
                      title: Text(l.terminalOnlyNewWindow),
                      onTap: _openingWindow ? null : () => unawaited(_openWindow()),
                    ),
                  if (_windowFailed) Text(l.terminalOnlyWindowError),
                  if (snapshot.connectionState == ConnectionState.waiting)
                    const LinearProgressIndicator()
                  else if (matches.isEmpty)
                    Text(_query.isEmpty ? l.terminalLocalUnavailable : l.terminalLocalShellNoMatch),
                  for (final profile in matches)
                    ListTile(
                      key: ValueKey('terminal-only-shell-${profile.id}'),
                      leading: const Icon(Icons.terminal_rounded),
                      title: Text(profile.label, maxLines: 1, overflow: TextOverflow.ellipsis),
                      onTap: _openingWindow ? null : () => Navigator.pop(context, profile),
                    ),
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}
