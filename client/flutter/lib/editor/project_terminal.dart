import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/terminal/terminal_screen.dart' show TerminalSessionPane;
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:consolecrypt/terminal/terminal_target_picker.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Owns a separate tab controller so an editor PTY has exactly one terminal view.
class ProjectTerminal extends ConsumerStatefulWidget {
  const ProjectTerminal({required this.directory, required this.active, required this.child, this.host, super.key});
  final String directory;
  final bool active;
  final Host? host;
  final Widget child;
  @override
  ConsumerState<ProjectTerminal> createState() => _ProjectTerminalState();
}

class _ProjectTerminalState extends ConsumerState<ProjectTerminal> {
  bool opening = false, visible = true;
  Future<void> _launch() async {
    if (opening) return;
    setState(() => opening = true);
    try {
      final tabs = ref.read(terminalTabsProvider.notifier), host = widget.host;
      if (host == null) {
        final target = await showTerminalTargetPicker(context, localOnly: true);
        if (!mounted || target is! LocalTerminalTarget) return;
        await tabs.openTarget(LocalTerminalTarget(target.profile, directory: widget.directory));
      } else {
        final directory = widget.directory;
        // A terminal interprets control bytes as keys, even inside shell quotes.
        if (!directory.startsWith('/') || RegExp(r'[\x00-\x1f\x7f-\x9f]').hasMatch(directory)) {
          throw FormatException(context.l10n.editorOutside);
        }
        final tab = await tabs.openTarget(SshTerminalTarget(host));
        if (!mounted) return;
        final quoted = directory.replaceAll("'", "'\\''");
        tabs.runInTab(tab, "cd '$quoted'");
      }
      if (mounted) setState(() => visible = true);
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
                TextButton.icon(
                  onPressed: opening
                      ? null
                      : selected == null
                      ? _launch
                      : () => setState(() => visible = !visible),
                  icon: const Icon(Icons.terminal, size: 16),
                  label: Text(l.navTerminal),
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
                              IconButton(
                                tooltip: l.commonClose,
                                onPressed: () => ref.read(terminalTabsProvider.notifier).close(tab),
                                icon: const Icon(Icons.close, size: 14),
                              ),
                            ],
                          ),
                      ],
                    ),
                  ),
                  IconButton(
                    tooltip: l.commonAdd,
                    onPressed: opening ? null : _launch,
                    icon: const Icon(Icons.add, size: 18),
                  ),
                ],
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
