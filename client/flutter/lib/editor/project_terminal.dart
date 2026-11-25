import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/terminal/terminal_pane.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:consolecrypt/terminal/terminal_target_picker.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Owns a separate tab controller so an editor PTY has exactly one terminal view.
class ProjectTerminal extends ConsumerStatefulWidget {
  const ProjectTerminal({required this.directory, required this.child, super.key});
  final String directory;
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
      final target = await showTerminalTargetPicker(context, localOnly: true);
      if (!mounted || target is! LocalTerminalTarget) return;
      await ref
          .read(terminalTabsProvider.notifier)
          .openTarget(LocalTerminalTarget(target.profile, directory: widget.directory));
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
                                  style: TextStyle(fontWeight: selected == tab ? FontWeight.bold : FontWeight.normal),
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
                    child: TerminalPane(key: ValueKey(selected.sessionId), tab: selected),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}
