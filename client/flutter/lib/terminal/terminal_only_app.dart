import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/terminal/terminal_fonts.dart';
import 'package:consolecrypt/terminal/terminal_minimal_mode.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class TerminalOnlyApp extends StatelessWidget {
  const TerminalOnlyApp({
    super.key,
    required this.service,
    required this.initialShell,
    this.openWindow = openTerminalOnlyWindow,
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
      // material_ui widgets look up their own Material and Cupertino delegates.
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      localeListResolutionCallback: resolveSystemLocale,
      builder: (_, child) => GlassAppScope(unifiedTitlebar: AppPlatform.isMacOS, child: child!),
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
  TerminalTabPlacement _placement = TerminalTabPlacement.left;
  bool _tabsCollapsed = true;
  double _tabWidth = GlassTabColumn.defaultWidth;

  @override
  void initState() {
    super.initState();
    FocusManager.instance.addEarlyKeyEventHandler(_shortcut);
    if (widget.initialShell case final profile?) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) unawaited(_openShell(profile));
      });
    }
  }

  @override
  void dispose() {
    FocusManager.instance.removeEarlyKeyEventHandler(_shortcut);
    super.dispose();
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

  Future<void> _newWindow() async {
    try {
      await widget.openWindow();
    } on Object {
      if (mounted) showSnack(context, context.l10n.terminalOnlyWindowError, error: true);
    }
  }

  KeyEventResult _shortcut(KeyEvent event) {
    final keys = HardwareKeyboard.instance;
    final primary = AppPlatform.usesMeta
        ? keys.isMetaPressed && !keys.isControlPressed
        : keys.isControlPressed && !keys.isMetaPressed;
    if (event is KeyUpEvent || !primary || keys.isAltPressed) return KeyEventResult.ignored;

    final physical = event.physicalKey;
    final logical = event.logicalKey;
    final newWindow =
        keys.isShiftPressed && (physical == PhysicalKeyboardKey.keyN || logical == LogicalKeyboardKey.keyN);
    final palette =
        !keys.isShiftPressed &&
        (const [PhysicalKeyboardKey.keyK, PhysicalKeyboardKey.keyT, PhysicalKeyboardKey.keyN].contains(physical) ||
            const [LogicalKeyboardKey.keyK, LogicalKeyboardKey.keyT, LogicalKeyboardKey.keyN].contains(logical));
    if (newWindow || palette) {
      if (event is KeyDownEvent) unawaited(newWindow ? _newWindow() : _palette());
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  Widget _toolbar() {
    final l = context.l10n;
    return GlassToolbarGroup(
      key: const ValueKey('terminal-only-toolbar'),
      children: [
        GlassMenuButton<TerminalTabPlacement>(
          entries: [
            GlassMenuItem(
              key: const ValueKey('terminal-only-tabs-top'),
              value: TerminalTabPlacement.top,
              checked: _placement == TerminalTabPlacement.top,
              label: l.terminalMinimalTabsTop,
            ),
            GlassMenuItem(
              key: const ValueKey('terminal-only-tabs-left'),
              value: TerminalTabPlacement.left,
              checked: _placement == TerminalTabPlacement.left,
              label: l.terminalMinimalTabsLeft,
            ),
          ],
          onSelected: (value) {
            if (mounted) setState(() => _placement = value);
          },
          builder: (_, open) => GlassIconButton(
            key: const ValueKey('terminal-only-tab-placement'),
            icon: Icons.tune_rounded,
            tooltip: l.terminalMinimalOptions,
            style: GlassIconButtonStyle.plain,
            size: GlassSizes.tab,
            iconSize: GlassSizes.iconRow,
            onPressed: open,
          ),
        ),
        GlassIconButton(
          key: const ValueKey('terminal-only-palette'),
          icon: Icons.keyboard_command_key_rounded,
          tooltip: '${l.terminalOnlyPalette} (${AppPlatform.shortcutLabel(LogicalKeyboardKey.keyK)})',
          style: GlassIconButtonStyle.plain,
          size: GlassSizes.tab,
          iconSize: GlassSizes.iconRow,
          onPressed: () => unawaited(_palette()),
        ),
      ],
    );
  }

  Future<void> _tabMenu(TerminalTab tab, Offset anchor, TerminalTabsController controller) async {
    final close = await showGlassMenu<bool>(
      context: context,
      anchor: anchor & Size.zero,
      entries: [GlassMenuItem(value: true, label: GlassStrings.of(context).closeTab, icon: Icons.close_rounded)],
    );
    if (mounted && close == true) await controller.closeTab(tab);
  }

  @override
  Widget build(BuildContext context) {
    final tabs = ref.watch(terminalTabsProvider);
    final controller = ref.read(terminalTabsProvider.notifier);
    final active = tabs.active;
    final l = context.l10n;
    final left = _placement == TerminalTabPlacement.left;
    final toolbar = _toolbar();
    final chrome = GlassScope.of(context);
    final header = left || chrome.unifiedTitlebar;
    final background = AppTheme.terminalTheme(Theme.of(context).brightness).background;
    final items = [
      for (final tab in tabs.tabs)
        GlassTab(
          key: ValueKey(tab.sessionId),
          title: tab.displayTitle,
          state: switch (tab.state) {
            SessionConnectionState.connected => GlassTabState.connected,
            SessionConnectionState.disconnected => GlassTabState.disconnected,
            _ => GlassTabState.reconnecting,
          },
        ),
    ];
    final selected = items.isEmpty ? null : tabs.activeIndex.clamp(0, items.length - 1);
    void newTab() => unawaited(_palette());
    void closeTab(int i) => unawaited(controller.closeTab(tabs.tabs[i]));
    void menu(int i, Offset anchor) => unawaited(_tabMenu(tabs.tabs[i], anchor, controller));
    final screen = Scaffold(
      backgroundColor: background,
      body: GlassBlurSuppressor(
        budget: chrome.budget,
        child: FocusScope(
          autofocus: true,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(GlassSpacing.s4, 0, GlassSpacing.s4, GlassSpacing.s4),
            child: LayoutBuilder(
              builder: (context, bounds) => Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (header)
                    GlassToolbar(
                      leadingInset: chrome.unifiedTitlebar ? chrome.trafficLightsLeading : 0,
                      trailing: [toolbar],
                    ),
                  if (!left)
                    GlassTabStrip(
                      key: const ValueKey('terminal-only-tab-strip'),
                      tabs: items,
                      activeIndex: selected,
                      onSelect: controller.activate,
                      onClose: closeTab,
                      onContextMenu: menu,
                      onAdd: newTab,
                      onReorder: controller.moveTab,
                      trailing: header ? null : toolbar,
                    ),
                  const SizedBox(height: GlassSpacing.s4),
                  Expanded(
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        if (left)
                          GlassTabColumn(
                            key: const ValueKey('terminal-only-tab-column'),
                            tabs: items,
                            activeIndex: selected,
                            width: _tabWidth,
                            maxWidth: (bounds.maxWidth - 360 - GlassTabColumn.handleWidth).clamp(
                              0.0,
                              GlassTabColumn.defaultMaxWidth,
                            ),
                            collapsed: _tabsCollapsed,
                            onSelect: controller.activate,
                            onClose: closeTab,
                            onContextMenu: menu,
                            onAdd: newTab,
                            onReorder: controller.moveTab,
                            onResize: (width) => setState(() => _tabWidth = width),
                            onCollapsedChanged: (value) => setState(() => _tabsCollapsed = value),
                          ),
                        Expanded(
                          key: const ValueKey('terminal-only-viewport'),
                          child: Padding(
                            padding: const EdgeInsets.all(GlassSpacing.s4),
                            child: ContentSurface(
                              kind: ContentSurfaceKind.solid,
                              color: background,
                              padding: const EdgeInsets.all(GlassSpacing.s8),
                              child: Column(
                                children: [
                                  if (_opening || active?.state == SessionConnectionState.connecting)
                                    const LinearProgressIndicator(),
                                  if (_error != null) Padding(padding: const EdgeInsets.all(8), child: Text(_error!)),
                                  Expanded(
                                    child: active == null
                                        ? Center(
                                            child: GlassButton(label: l.paletteNewLocalTerminal, onPressed: newTab),
                                          )
                                        : TerminalPane(
                                            key: ValueKey((active.sessionId, active.renderGeneration)),
                                            tab: active,
                                          ),
                                  ),
                                  if (active?.state == SessionConnectionState.disconnected)
                                    Padding(
                                      padding: const EdgeInsets.all(8),
                                      child: Text(
                                        active!.exited ? l.terminalOnlyShellExited : l.terminalOnlyShellError,
                                      ),
                                    ),
                                ],
                              ),
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
    if (!AppPlatform.isMacOS) return screen;
    return PlatformMenuBar(
      menus: [
        const PlatformMenu(
          label: 'ConsoleCrypt',
          menus: [
            PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.hide),
            PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.quit),
          ],
        ),
        PlatformMenu(
          label: l.menuFile,
          menus: [
            PlatformMenuItem(
              label: l.terminalOnlyNewWindow,
              shortcut: AppPlatform.primary(LogicalKeyboardKey.keyN, shift: true),
              onSelected: () => unawaited(_newWindow()),
            ),
            PlatformMenuItem(
              label: l.commandNewTabPickerTitle,
              shortcut: AppPlatform.primary(LogicalKeyboardKey.keyT),
              onSelected: () => unawaited(_palette()),
            ),
          ],
        ),
        PlatformMenu(
          label: l.menuView,
          menus: [
            PlatformMenuItem(
              label: l.terminalOnlyPalette,
              shortcut: AppPlatform.primary(LogicalKeyboardKey.keyK),
              onSelected: () => unawaited(_palette()),
            ),
          ],
        ),
      ],
      child: screen,
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
              return Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
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
                    else if (snapshot.hasError)
                      Text(l.terminalOnlyShellError)
                    else if (matches.isEmpty)
                      Text(_query.isEmpty ? l.terminalLocalUnavailable : l.terminalLocalShellNoMatch)
                    else
                      Flexible(
                        child: ListView.builder(
                          shrinkWrap: true,
                          primary: false,
                          itemCount: matches.length,
                          itemBuilder: (context, index) {
                            final profile = matches[index];
                            return ListTile(
                              key: ValueKey('terminal-only-shell-${profile.id}'),
                              leading: const Icon(Icons.terminal_rounded),
                              title: Text(profile.label, maxLines: 1, overflow: TextOverflow.ellipsis),
                              onTap: _openingWindow ? null : () => Navigator.pop(context, profile),
                            );
                          },
                        ),
                      ),
                  ],
                ),
              );
            },
          ),
        ),
      ),
    );
  }
}
