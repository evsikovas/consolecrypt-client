import 'dart:async';

import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/notes_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Shows a toast when a reminder goes off while the app is open, then clears
/// the reminder so other devices do not repeat it once synced.
class NotesReminderWatcher extends ConsumerStatefulWidget {
  const NotesReminderWatcher({required this.child, super.key});
  final Widget child;

  @override
  ConsumerState<NotesReminderWatcher> createState() => _NotesReminderWatcherState();
}

class _NotesReminderWatcherState extends ConsumerState<NotesReminderWatcher> {
  final _shown = <String>{};

  void _check() {
    if (!mounted) return;
    final now = DateTime.now();
    final service = ref.read(noteServiceProvider);
    for (final note in ref.read(notesProvider).value ?? const <Note>[]) {
      final at = note.remindAt;
      if (at == null || at.isAfter(now) || !_shown.add('${note.id}@${at.millisecondsSinceEpoch}')) continue;
      final l = context.l10n;
      GlassToast.show(
        context,
        GlassToastRequest(
          message: l.notesReminderToast(note.title),
          tone: GlassTone.warning,
          icon: Icons.notifications_active_outlined,
          duration: const Duration(seconds: 30),
          actionLabel: l.notesOpen,
          onAction: () {
            ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.notes);
            ref.read(notesFocusProvider.notifier).focus(note.id);
          },
        ),
      );
      if (service != null) unawaited(service.save(note.copyWith(remindAt: () => null)).then((_) {}, onError: (_) {}));
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(notesClockProvider, (_, _) => _check());
    ref.listen(notesProvider, (_, _) => scheduleMicrotask(_check));
    return widget.child;
  }
}
