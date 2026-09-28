import 'package:consolecrypt/ai/terminal_attachment.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/snippets/snippet_editor.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

enum _TerminalAction { copy, paste, selectAll, clearSelection, askAi, snippet }

final _pastePending = Expando<bool>('terminal paste'); // l10n-ignore: debug name
final _menuOpen = Expando<bool>('terminal menu'); // l10n-ignore: debug name

bool _validOrigin(WidgetRef ref, TerminalTab tab, Object? profile) =>
    ref.read(activeProfileProvider)?.id == profile &&
    ref.read(vaultStatusProvider).value?.isUnlocked == true &&
    identical(ref.read(terminalTabsProvider).active, tab);

void selectTerminalBuffer(TerminalTab tab) {
  final buffer = tab.terminal.buffer;
  tab.controller.setSelection(
    buffer.createAnchor(0, 0),
    buffer.createAnchor(tab.terminal.viewWidth, buffer.height - 1),
  );
}

Future<void> pasteIntoTerminal(BuildContext context, WidgetRef ref, TerminalTab tab) async {
  if (_pastePending[tab] ?? false) return;
  _pastePending[tab] = true;
  try {
    await _pasteIntoTerminal(context, ref, tab);
  } finally {
    _pastePending[tab] = false;
  }
}

Future<void> _pasteIntoTerminal(BuildContext context, WidgetRef ref, TerminalTab tab) async {
  final profile = ref.read(activeProfileProvider)?.id;
  if (!tab.isConnected || !_validOrigin(ref, tab, profile)) return;
  final text = (await Clipboard.getData(Clipboard.kTextPlain))?.text;
  if (text == null || text.isEmpty || !context.mounted || !_validOrigin(ref, tab, profile) || !tab.isConnected) return;
  // Multiline/control-character paste can execute commands even without an
  // explicit Enter. The keyboard shortcut follows the same confirmation path.
  if (RegExp(r'[\x00-\x08\x0a-\x1f\x7f]').hasMatch(text)) {
    final l = context.l10n;
    final preview = text.length > 4000 ? '${text.substring(0, 4000)}…' : text;
    final visible = preview.replaceAllMapped(
      RegExp(r'[\x00-\x08\x0b-\x1f\x7f]'),
      (m) => '\\x${m[0]!.codeUnitAt(0).toRadixString(16).padLeft(2, '0')}',
    );
    final approved = await showConfirmDialog(
      context,
      title: l.terminalPasteConfirmTitle,
      message: l.terminalPasteConfirmMessage,
      confirmLabel: l.terminalPaste,
      destructive: true,
      extra: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 200),
        child: SingleChildScrollView(child: SelectableText(visible)),
      ),
    );
    if (!approved || !context.mounted || !_validOrigin(ref, tab, profile) || !tab.isConnected) return;
  }
  tab.terminal.paste(text);
  tab.controller.clearSelection();
}

Future<void> showTerminalContextMenu(BuildContext context, WidgetRef ref, TerminalTab tab, Rect anchor) async {
  final navigator = Navigator.of(context, rootNavigator: true);
  if (_menuOpen[navigator] ?? false) return;
  final profile = ref.read(activeProfileProvider)?.id;
  if (!_validOrigin(ref, tab, profile)) return;
  final selection = tab.rawSelectedText;
  final hasText = selection != null && selection.trim().isNotEmpty;
  final l = context.l10n;
  _menuOpen[navigator] = true;
  try {
    final action = await showGlassMenu<_TerminalAction>(
      context: context,
      anchor: anchor,
      minWidth: 240,
      entries: [
        GlassMenuItem(
          key: const ValueKey('terminal-menu-copy'),
          value: _TerminalAction.copy,
          label: l.commonCopy,
          icon: Icons.copy_rounded,
          enabled: selection != null,
          shortcut: AppPlatform.isMobile
              ? null
              : AppPlatform.usesMeta
              ? '⌘C'
              : 'Ctrl+Shift+C',
        ),
        GlassMenuItem(
          key: const ValueKey('terminal-menu-paste'),
          value: _TerminalAction.paste,
          label: l.terminalPaste,
          icon: Icons.content_paste_rounded,
          enabled: tab.isConnected,
          shortcut: AppPlatform.isMobile
              ? null
              : AppPlatform.usesMeta
              ? '⌘V'
              : 'Ctrl+V',
        ),
        const GlassMenuDivider(),
        GlassMenuItem(
          key: const ValueKey('terminal-menu-ask-ai'),
          value: _TerminalAction.askAi,
          label: l.terminalAskAi,
          icon: Icons.auto_awesome_rounded,
          enabled: hasText,
        ),
        GlassMenuItem(
          key: const ValueKey('terminal-menu-snippet'),
          value: _TerminalAction.snippet,
          label: l.terminalSaveSnippet,
          icon: Icons.code_rounded,
          enabled: hasText,
        ),
        const GlassMenuDivider(),
        GlassMenuItem(
          key: const ValueKey('terminal-menu-select-all'),
          value: _TerminalAction.selectAll,
          label: l.terminalSelectAll,
          icon: Icons.select_all_rounded,
          shortcut: AppPlatform.isMobile
              ? null
              : AppPlatform.usesMeta
              ? '⌘A'
              : 'Ctrl+A',
        ),
        GlassMenuItem(
          key: const ValueKey('terminal-menu-clear-selection'),
          value: _TerminalAction.clearSelection,
          label: l.terminalClearSelection,
          icon: Icons.deselect_rounded,
          enabled: tab.controller.selection != null,
        ),
      ],
    );
    if (action == null || !context.mounted || !_validOrigin(ref, tab, profile)) return;
    switch (action) {
      case _TerminalAction.copy:
        if (selection != null) await Clipboard.setData(ClipboardData(text: selection));
      case _TerminalAction.paste:
        await pasteIntoTerminal(context, ref, tab);
      case _TerminalAction.selectAll:
        selectTerminalBuffer(tab);
      case _TerminalAction.clearSelection:
        tab.controller.clearSelection();
      case _TerminalAction.askAi:
        if (hasText) {
          ref.read(terminalAttachmentProvider.notifier).attach(tab, selection);
          ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.ai);
        }
      case _TerminalAction.snippet:
        if (hasText) await showSnippetEditor(context, initialTemplate: selection);
    }
  } finally {
    _menuOpen[navigator] = false;
  }
}
