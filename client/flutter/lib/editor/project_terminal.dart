import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/editor/editor_toolbar_button.dart';
import 'package:consolecrypt/terminal/terminal_screen.dart' show TerminalSessionPane;
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:consolecrypt/terminal/terminal_target_picker.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Owns a separate tab controller so an editor PTY has exactly one terminal view.
class ProjectTerminal extends ConsumerStatefulWidget {
  const ProjectTerminal({
    required this.directory,
    required this.active,
    required this.child,
    required this.onMoveToTerminal,
    this.host,
    super.key,
  });
  final String directory;
  final bool active;
  final Host? host;
  final Widget child;
  final void Function(TerminalTabsController, TerminalTab) onMoveToTerminal;
  @override
  ConsumerState<ProjectTerminal> createState() => _ProjectTerminalState();
}

class _ProjectTerminalState extends ConsumerState<ProjectTerminal> {
  bool opening = false, visible = true;
  Future<void> _launch({bool moveToTerminal = false}) async {
    if (opening) return;
    setState(() => opening = true);
    try {
      final tabs = ref.read(terminalTabsProvider.notifier), host = widget.host;
      final TerminalTab tab;
      if (host == null) {
        final target = await showTerminalTargetPicker(context, localOnly: true);
        if (!mounted || target is! LocalTerminalTarget) return;
        tab = await tabs.openTarget(LocalTerminalTarget(target.profile, directory: widget.directory));
      } else {
        final directory = widget.directory;
        // A terminal interprets control bytes as keys, even inside shell quotes.
        if (!directory.startsWith('/') || RegExp(r'[\x00-\x1f\x7f-\x9f]').hasMatch(directory)) {
          throw FormatException(context.l10n.editorOutside);
        }
        tab = await tabs.openTarget(SshTerminalTarget(host));
        if (!mounted) return;
        final quoted = directory.replaceAll("'", "'\\''");
        tabs.runInTab(tab, "cd '$quoted'");
      }
      if (!mounted) return;
      if (moveToTerminal) {
        widget.onMoveToTerminal(tabs, tab);
      } else {
        setState(() => visible = true);
      }
    } catch (error) {
      if (mounted) showSnack(context, error.toString(), error: true);
    } finally {
      if (mounted) setState(() => opening = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final tabs = ref.watch(terminalTabsProvider);
    final selected = tabs.active;
    final l = context.l10n;
    return LayoutBuilder(
      builder: (context, box) => Column(
        children: [
          Expanded(child: widget.child),
          const Divider(height: 1),
          SizedBox(
            height: 36,
            child: Row(
              children: [
                EditorToolbarButton(
                  key: const ValueKey('editor-terminal-toggle'),
                  selected: selected != null && visible,
                  tooltip: selected != null && visible ? l.editorHideTerminal : l.editorShowTerminal,
                  onPressed: opening
                      ? null
                      : selected == null
                      ? _launch
                      : () => setState(() => visible = !visible),
                  icon: Icons.terminal_rounded,
                ),
                if (selected != null) ...[
                  Expanded(
                    child: ListView(
                      scrollDirection: Axis.horizontal,
                      children: [
                        for (final tab in tabs.tabs)
                          Row(
                            children: [
                              TextButton(
                                onPressed: () {
                                  ref.read(terminalTabsProvider.notifier).activate(tabs.tabs.indexOf(tab));
                                  setState(() => visible = true);
                                },
                                child: Text(
                                  tab.target.label,
                                  style: TextStyle(fontWeight: selected == tab ? FontWeight.w600 : FontWeight.normal),
                                ),
                              ),
                              EditorToolbarButton(
                                tooltip: l.commonClose,
                                onPressed: () => ref.read(terminalTabsProvider.notifier).close(tab),
                                icon: Icons.close,
                              ),
                            ],
                          ),
                      ],
                    ),
                  ),
                  EditorToolbarButton(
                    tooltip: l.commandNewTerminalTab,
                    onPressed: opening ? null : _launch,
                    icon: Icons.add,
                  ),
                ] else
                  const Spacer(),
                EditorToolbarButton(
                  key: const ValueKey('editor-terminal-move'),
                  tooltip: l.editorMoveTerminal,
                  icon: Icons.open_in_full_rounded,
                  onPressed: opening
                      ? null
                      : () {
                          if (selected == null) {
                            _launch(moveToTerminal: true);
                          } else {
                            widget.onMoveToTerminal(ref.read(terminalTabsProvider.notifier), selected);
                          }
                        },
                ),
              ],
            ),
          ),
          if (selected != null && visible)
            SizedBox(
              height: (box.maxHeight * 0.32).clamp(0, 300).toDouble(),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (selected.message != null && !selected.isConnected)
                    Text(selected.message!, maxLines: 2, overflow: TextOverflow.ellipsis),
                  Expanded(
                    child: TerminalSessionPane(pane: selected, active: widget.active),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
