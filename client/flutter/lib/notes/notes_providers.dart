import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/note_service.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final noteServiceProvider = Provider<NoteService?>((ref) => ref.watch(appServicesProvider).notes);

/// Counts change signals: equal values would not notify dependents.
final _noteChanges = StreamProvider<int>((ref) {
  var count = 0;
  return (ref.watch(noteServiceProvider)?.changes() ?? const Stream<void>.empty()).map((_) => ++count);
});

/// Notes exist only while a profile is unlocked.
final notesUnlockedProvider = Provider<bool>((ref) => ref.watch(appStageProvider) == AppStage.unlocked);

/// Every note of the unlocked profile; empty while locked.
final notesProvider = FutureProvider<List<Note>>((ref) async {
  ref.watch(activeProfileProvider.select((p) => p?.id));
  final unlocked = ref.watch(notesUnlockedProvider);
  final service = ref.watch(noteServiceProvider);
  if (!unlocked || service == null) return const [];
  ref.watch(_noteChanges);
  return service.notes();
});

/// Open tasks past due plus reminders that went off: the rail badge.
final notesAttentionProvider = Provider<int>((ref) {
  ref.watch(notesClockProvider);
  final now = DateTime.now();
  return (ref.watch(notesProvider).value ?? const [])
      .where((note) => note.overdue(now) || note.reminderDue(now))
      .length;
});

/// Ticks every 30 seconds so due states and reminders refresh.
final notesClockProvider = StreamProvider<DateTime>(
  (ref) => Stream.periodic(const Duration(seconds: 30), (_) => DateTime.now()),
);

/// The object a section shows right now, offered to the notes panel.
final class NoteContext {
  const NoteContext({required this.target, required this.label, this.icon = Icons.dns_outlined});
  final NoteTarget target;
  final String label;
  final IconData icon;

  @override
  bool operator ==(Object other) => other is NoteContext && other.target == target && other.label == label;
  @override
  int get hashCode => Object.hash(target, label);
}

/// Sections without a Riverpod-visible selection (databases, Docker,
/// Kubernetes) publish their open object here, keyed by section.
final noteContextsProvider = NotifierProvider<NoteContexts, Map<ShellBranch, NoteContext>>(NoteContexts.new);

class NoteContexts extends Notifier<Map<ShellBranch, NoteContext>> {
  @override
  Map<ShellBranch, NoteContext> build() {
    ref.watch(activeProfileProvider.select((p) => p?.id));
    ref.watch(notesUnlockedProvider);
    return const {};
  }

  void publish(ShellBranch branch, NoteContext? context) {
    if (state[branch] == context) return;
    state = {
      for (final entry in state.entries)
        if (entry.key != branch) entry.key: entry.value,
      branch: ?context,
    };
  }
}

/// A section's open object for the notes panel. Publishing waits for a
/// microtask so provider state never changes during a build, and the
/// object is withdrawn when the section's state is disposed.
mixin NoteContextPublisher<T extends ConsumerStatefulWidget> on ConsumerState<T> {
  ShellBranch get noteBranch;
  late final NoteContexts _noteContexts;
  NoteContext? _published;

  @override
  void initState() {
    super.initState();
    _noteContexts = ref.read(noteContextsProvider.notifier);
  }

  void publishNoteContext(NoteContext? context) {
    if (context == _published) return;
    _published = context;
    final contexts = _noteContexts, branch = noteBranch;
    scheduleMicrotask(() {
      try {
        contexts.publish(branch, context);
      } on Object {
        // The profile changed or locked meanwhile; its contexts are gone.
      }
    });
  }

  @override
  void dispose() {
    publishNoteContext(null);
    super.dispose();
  }
}

/// The object of [branch]: the active terminal tab, or what the section
/// published.
final noteContextProvider = Provider.family<NoteContext?, ShellBranch>((ref, branch) {
  if (branch != ShellBranch.terminal) return ref.watch(noteContextsProvider)[branch];
  ref.watch(terminalTabsProvider.select((s) => s.revision));
  final target = ref.watch(terminalTabsProvider).active?.target;
  return switch (target) {
    SshTerminalTarget(:final host) => NoteContext(
      target: NoteObjectTarget(NoteObjectKind.host, host.id.value),
      label: host.name,
    ),
    DockerTerminalTarget(:final hostId?, :final containerName) => NoteContext(
      target: NoteDockerTarget(
        hostId: hostId == 'local' ? null : hostId,
        name: containerName,
      ), // l10n-ignore: Docker host id
      label: containerName,
      icon: Icons.view_in_ar_outlined,
    ),
    KubernetesTerminalTarget(:final contextId?, :final namespace, :final pod) => NoteContext(
      target: NoteKubeTarget(contextId: contextId, resource: 'pods', kind: 'Pod', namespace: namespace, name: pod),
      label: '$namespace/$pod',
      icon: Icons.anchor_rounded,
    ),
    _ => null,
  };
});

/// The notes object of a saved host (SSH or RDP).
NoteContext hostNoteContext(Host host) => host.isRdp
    ? NoteContext(
        target: NoteObjectTarget(NoteObjectKind.rdpHost, host.id.value),
        label: host.name,
        icon: Icons.desktop_windows_outlined,
      )
    : NoteContext(target: NoteObjectTarget(NoteObjectKind.host, host.id.value), label: host.name);

/// An object chosen for the "Object" tab (a host card's "Notes" or the
/// panel's host chooser). It replaces the section's own object until
/// cleared; a profile switch or lock clears it.
final notesChosenObjectProvider = NotifierProvider<NotesChosenObject, NoteContext?>(NotesChosenObject.new);

class NotesChosenObject extends Notifier<NoteContext?> {
  @override
  NoteContext? build() {
    ref.watch(activeProfileProvider.select((p) => p?.id));
    ref.watch(notesUnlockedProvider);
    return null;
  }

  void choose(NoteContext? context) => state = context;
}

/// Opens the notes panel on [host]'s notes.
void showHostNotes(WidgetRef ref, Host host) {
  ref.read(notesChosenObjectProvider.notifier).choose(hostNoteContext(host));
  ref.read(workspaceToolsProvider.notifier).open(WorkspaceTool.notes);
}

/// Asks the notes panel to open one note (a reminder's "Open").
final notesFocusProvider = NotifierProvider<NotesFocus, String?>(NotesFocus.new);

class NotesFocus extends Notifier<String?> {
  @override
  String? build() => null;
  void focus(String id) => state = id;
  void clear() => state = null;
}

/// Notes travelling with shares, when the backend offers them.
final sharedNoteServiceProvider = Provider<SharedNoteService?>(
  (ref) => switch (ref.watch(noteServiceProvider)) {
    final SharedNoteService service => service,
    _ => null,
  },
);

/// Local objects linked to shares (`kind:id` → share id).
final noteLinksProvider = FutureProvider<Map<String, String>>((ref) async {
  ref.watch(activeProfileProvider.select((p) => p?.id));
  final service = ref.watch(sharedNoteServiceProvider);
  if (!ref.watch(notesUnlockedProvider) || service == null) return const {};
  ref.watch(_noteChanges);
  try {
    return await service.shareLinks();
  } on Object {
    return const {};
  }
});

/// The share carrying notes about [target], if any: a link recorded when the
/// object was published or copied, or the binding of a shared host copy.
final noteShareProvider = Provider.family<String?, NoteTarget>((ref, target) {
  final key = noteLinkKey(target);
  if (key == null) return null;
  final linked = ref.watch(noteLinksProvider).value?[key];
  if (linked != null) return linked;
  if (target case NoteObjectTarget(kind: NoteObjectKind.host, :final id)) {
    final host = ref.watch(hostsProvider).value?.where((h) => h.id.value == id).firstOrNull;
    return host?.metadata['cc.shared.share'];
  }
  return null;
});

/// Shared notes of one share; reloaded explicitly after changes.
final sharedNotesProvider = FutureProvider.autoDispose.family<SharedNotes, String>((ref, shareId) async {
  final service = ref.watch(sharedNoteServiceProvider);
  if (service == null) return SharedNotes(shareId: shareId);
  return service.sharedNotes(shareId);
});
