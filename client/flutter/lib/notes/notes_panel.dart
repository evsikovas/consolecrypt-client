import 'dart:async';

import 'package:consolecrypt/ai/markdown_text.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/notes_providers.dart';
import 'package:consolecrypt/notes/shared_notes_section.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

enum _NotesMode { object, all, tasks }

/// Notes, tasks and reminders in the right workspace panel. "This object"
/// follows what the current section shows ([branch]).
class NotesPanel extends ConsumerStatefulWidget {
  const NotesPanel({required this.branch, super.key});
  final ShellBranch branch;

  @override
  ConsumerState<NotesPanel> createState() => _NotesPanelState();
}

class _NotesPanelState extends ConsumerState<NotesPanel> {
  _NotesMode? _mode;
  bool _notLinkedOnly = false;
  String _query = '';
  Note? _editing;

  _NotesMode _effectiveMode(NoteContext? context) => _mode ?? (context == null ? _NotesMode.all : _NotesMode.object);

  /// Shows the notes of a host picked from the inventory in the Object tab.
  Future<void> _chooseHost() async {
    final host = await showHostPicker(context, title: context.l10n.notesChooseHost, protocol: null);
    if (host == null || !mounted) return;
    ref.read(notesChosenObjectProvider.notifier).choose(hostNoteContext(host));
  }

  bool _matches(Note note) {
    final query = _query.trim().toLowerCase();
    return query.isEmpty ||
        note.title.toLowerCase().contains(query) ||
        note.body.toLowerCase().contains(query) ||
        note.tags.any((tag) => tag.toLowerCase().contains(query));
  }

  void _open(Note note) => setState(() => _editing = note);

  Future<void> _save(Note note) async {
    final service = ref.read(noteServiceProvider);
    if (service == null) return;
    await runWithFeedback(context, () => service.save(note));
  }

  @override
  Widget build(BuildContext context) {
    // A reminder's "Open" may arrive before this panel was first shown.
    if (ref.watch(notesFocusProvider) case final id?) {
      scheduleMicrotask(() {
        if (!mounted) return;
        final note = ref.read(notesProvider).value?.where((n) => n.id == id).firstOrNull;
        ref.read(notesFocusProvider.notifier).clear();
        if (note != null) setState(() => _editing = note);
      });
    }
    // A host card's "Notes" opens the Object tab on that host.
    ref.listen(notesChosenObjectProvider, (_, chosen) {
      if (chosen != null) setState(() => _mode = _NotesMode.object);
    });
    final chosen = ref.watch(notesChosenObjectProvider);
    final current = chosen ?? ref.watch(noteContextProvider(widget.branch));
    final editing = _editing;
    if (editing != null) {
      return _NoteEditor(
        key: ValueKey(('note-editor', editing.id.isEmpty ? editing.hashCode : editing.id)),
        initial: editing,
        current: current,
        onClose: () => setState(() => _editing = null),
      );
    }
    final l = context.l10n, tokens = GlassTokens.of(context);
    final mode = _effectiveMode(current);
    final notes = ref.watch(notesProvider);
    ref.watch(notesClockProvider);
    final now = DateTime.now();
    final all = (notes.value ?? const <Note>[]).where(_matches).toList();
    final List<Note> visible;
    final List<Note> done;
    switch (mode) {
      case _NotesMode.object:
        visible = all.where((n) => n.target == current?.target).toList()..sort(compareNotes);
        done = const [];
      case _NotesMode.all:
        visible = [
          for (final note in all)
            if (!_notLinkedOnly || noteNotLinked(ref, note)) note,
        ]..sort(compareNotes);
        done = const [];
      case _NotesMode.tasks:
        visible = all.where((n) => n.task?.done == false).toList()..sort(compareTasks);
        done = all.where((n) => n.task?.done ?? false).toList()..sort(compareNotes);
    }
    final hostNotes = mode == _NotesMode.object ? _hostNotes(current) : null;
    final shareId = mode == _NotesMode.object && current != null ? ref.watch(noteShareProvider(current.target)) : null;
    final empty = visible.isEmpty && done.isEmpty && hostNotes == null && shareId == null;
    final notLinked = mode == _NotesMode.all
        ? (notes.value ?? const <Note>[]).where((note) => noteNotLinked(ref, note)).length
        : 0;
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GlassSegmented<_NotesMode>(
            key: const ValueKey('notes-mode'),
            inChrome: false,
            expand: true,
            semanticLabel: l.navNotes,
            segments: [
              GlassSegment(
                key: const ValueKey('notes-mode-object'),
                value: _NotesMode.object,
                label: l.notesThisObject,
              ),
              GlassSegment(key: const ValueKey('notes-mode-all'), value: _NotesMode.all, label: l.notesAll),
              GlassSegment(key: const ValueKey('notes-mode-tasks'), value: _NotesMode.tasks, label: l.notesTasks),
            ],
            selected: mode,
            onChanged: (value) => setState(() => _mode = value),
          ),
          const SizedBox(height: GlassSpacing.s8),
          if (mode == _NotesMode.object)
            Padding(
              padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
              child: Row(
                children: [
                  Icon(current?.icon ?? Icons.dns_outlined, size: 15, color: tokens.secondaryLabel),
                  const SizedBox(width: GlassSpacing.s6),
                  Expanded(
                    child: Text(
                      current?.label ?? l.notesNoObject,
                      key: const ValueKey('notes-context'),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                    ),
                  ),
                  if (chosen != null)
                    GlassIconButton(
                      key: const ValueKey('notes-object-clear'),
                      icon: Icons.close_rounded,
                      tooltip: l.notesChosenClear,
                      style: GlassIconButtonStyle.plain,
                      onPressed: () => ref.read(notesChosenObjectProvider.notifier).choose(null),
                    ),
                  GlassIconButton(
                    key: const ValueKey('notes-choose-host'),
                    icon: Icons.manage_search_rounded,
                    tooltip: l.notesChooseHost,
                    style: GlassIconButtonStyle.plain,
                    onPressed: () => unawaited(_chooseHost()),
                  ),
                ],
              ),
            ),
          Row(
            children: [
              Expanded(
                child: GlassField(
                  key: const ValueKey('notes-search'),
                  search: true,
                  leadingIcon: Icons.search_rounded,
                  placeholder: l.notesSearchHint,
                  onChanged: (value) => setState(() => _query = value),
                ),
              ),
              const SizedBox(width: GlassSpacing.s6),
              GlassIconButton(
                key: const ValueKey('notes-new'),
                icon: Icons.add_rounded,
                tooltip: l.notesNew,
                onPressed: () => _open(
                  Note(
                    title: '',
                    target: mode == _NotesMode.object ? current?.target : null,
                    targetLabel: mode == _NotesMode.object ? current?.label : null,
                    task: mode == _NotesMode.tasks ? const NoteTask() : null,
                  ),
                ),
              ),
            ],
          ),
          if (mode == _NotesMode.all && (notLinked > 0 || _notLinkedOnly)) ...[
            const SizedBox(height: GlassSpacing.s6),
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: GlassSelect<bool>(
                key: const ValueKey('notes-filter'),
                semanticLabel: l.notesFilter,
                value: _notLinkedOnly,
                items: [
                  GlassSelectItem(value: false, label: l.notesFilterAll),
                  GlassSelectItem(value: true, label: l.notesFilterNotLinked(notLinked)),
                ],
                onChanged: (value) => setState(() => _notLinkedOnly = value),
              ),
            ),
          ],
          const SizedBox(height: GlassSpacing.s8),
          Expanded(
            child: empty
                ? EmptyState(
                    icon: Icons.sticky_note_2_outlined,
                    title: _query.trim().isNotEmpty
                        ? l.notesNothingFound
                        : mode == _NotesMode.object
                        ? (current == null ? l.notesNoObjectTitle : l.notesEmptyObject(current.label))
                        : l.notesEmptyTitle,
                    message: _query.trim().isNotEmpty
                        ? null
                        : mode == _NotesMode.object && current == null
                        ? l.notesNoObjectMessage
                        : l.notesEmptyMessage,
                    action: mode == _NotesMode.object && current == null
                        ? GlassButton(
                            key: const ValueKey('notes-empty-choose-host'),
                            label: l.notesChooseHost,
                            icon: Icons.manage_search_rounded,
                            onPressed: () => unawaited(_chooseHost()),
                          )
                        : null,
                  )
                : ListView(
                    key: const ValueKey('notes-list'),
                    children: [
                      ?hostNotes,
                      if (shareId != null && current != null && visible.isEmpty)
                        Padding(
                          padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s8),
                          child: Text(
                            l.notesEmptyObject(current.label),
                            style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                          ),
                        ),
                      for (final note in visible)
                        _NoteRow(
                          note: note,
                          now: now,
                          showTarget: mode != _NotesMode.object,
                          current: current,
                          onOpen: () => _open(note),
                          onSave: _save,
                        ),
                      if (done.isNotEmpty)
                        Theme(
                          data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
                          child: ExpansionTile(
                            key: const ValueKey('notes-done'),
                            tilePadding: EdgeInsets.zero,
                            title: Text(
                              l.notesCompleted(done.length),
                              style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                            ),
                            children: [
                              for (final note in done)
                                _NoteRow(
                                  note: note,
                                  now: now,
                                  showTarget: true,
                                  current: current,
                                  onOpen: () => _open(note),
                                  onSave: _save,
                                ),
                            ],
                          ),
                        ),
                      if (shareId != null && current != null)
                        SharedNotesSection(shareId: shareId, target: current.target, query: _query),
                    ],
                  ),
          ),
        ],
      ),
    );
  }

  /// The free-text notes of the host card, with a way to move them here.
  Widget? _hostNotes(NoteContext? current) {
    final target = current?.target;
    if (target is! NoteObjectTarget || target.kind != NoteObjectKind.host) return null;
    final host = ref.watch(hostsProvider).value?.where((h) => h.id.value == target.id).firstOrNull;
    if (host == null || host.notes.trim().isEmpty) return null;
    final l = context.l10n, tokens = GlassTokens.of(context);
    return Padding(
      key: const ValueKey('notes-host-card'),
      padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
      child: ContentSurface(
        padding: const EdgeInsets.all(GlassSpacing.s8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(l.notesHostNotesTitle, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
            const SizedBox(height: GlassSpacing.s4),
            Text(host.notes, style: tokens.typography.callout.copyWith(color: tokens.palette.label)),
            const SizedBox(height: GlassSpacing.s6),
            Align(
              alignment: AlignmentDirectional.centerEnd,
              child: GlassButton(
                key: const ValueKey('notes-host-move'),
                label: l.notesHostNotesMove,
                icon: Icons.drive_file_move_outline,
                onPressed: () => _moveHostNotes(host, target),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _moveHostNotes(Host host, NoteTarget target) async {
    final service = ref.read(noteServiceProvider), inventory = ref.read(inventoryServiceProvider);
    if (service == null) return;
    final title = context.l10n.notesFromHostTitle(host.name);
    await runWithFeedback(context, () async {
      // The note exists before the host field is cleared: nothing is lost.
      await service.save(Note(title: title, body: host.notes, target: target));
      await inventory.saveHost(host.withNotes(''));
    });
  }
}

/// One note in the list: task check, title, first line and its facts.
class _NoteRow extends ConsumerWidget {
  const _NoteRow({
    required this.note,
    required this.now,
    required this.showTarget,
    required this.current,
    required this.onOpen,
    required this.onSave,
  });
  final Note note;
  final DateTime now;
  final bool showTarget;
  final NoteContext? current;
  final VoidCallback onOpen;
  final Future<void> Function(Note note) onSave;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n, tokens = GlassTokens.of(context), t = tokens.typography;
    final task = note.task;
    final overdue = note.overdue(now);
    final preview = note.body.trim().split('\n').first;
    final target = showTarget && note.target != null ? noteTargetLabel(ref, note.target!) : null;
    final facts = <Widget>[
      if (target != null && target.$1 != null) _Fact(icon: target.$2, text: target.$1!, color: tokens.secondaryLabel),
      if (target != null && target.$1 == null) ...[
        _Fact(
          key: ValueKey('note-not-linked-${note.id}'),
          icon: Icons.link_off_rounded,
          text: l.notesNotLinked,
          color: tokens.palette.warning,
        ),
        if (note.targetLabel case final former?)
          _Fact(icon: target.$2, text: former, color: tokens.palette.tertiary, strike: true),
      ],
      if (task?.dueAt case final due? when !(task?.done ?? false))
        _Fact(
          icon: Icons.event_rounded,
          text: overdue ? l.notesOverdueAt(formatNoteTime(context, due)) : formatNoteTime(context, due),
          color: overdue ? tokens.palette.danger : tokens.secondaryLabel,
        ),
      if (note.remindAt case final remind?)
        _Fact(
          icon: Icons.notifications_none_rounded,
          text: formatNoteTime(context, remind),
          color: note.reminderDue(now) ? tokens.palette.warning : tokens.secondaryLabel,
        ),
      if (note.tags.isNotEmpty) TagChips(tags: note.tags),
    ];
    return Padding(
      key: ValueKey('note-${note.id}'),
      padding: const EdgeInsets.only(bottom: 2),
      child: InkWell(
        onTap: onOpen,
        borderRadius: BorderRadius.circular(8),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 8),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (task != null)
                SizedBox.square(
                  dimension: 24,
                  child: Checkbox(
                    key: ValueKey('note-done-${note.id}'),
                    value: task.done,
                    visualDensity: VisualDensity.compact,
                    semanticLabel: task.done ? l.notesMarkOpen : l.notesMarkDone,
                    onChanged: (value) => onSave(
                      note.copyWith(
                        task: () => NoteTask(done: value ?? false, dueAt: task.dueAt),
                      ),
                    ),
                  ),
                )
              else
                Padding(
                  padding: const EdgeInsets.all(4),
                  child: Icon(
                    note.pinned ? Icons.push_pin_rounded : Icons.sticky_note_2_outlined,
                    size: 16,
                    color: tokens.secondaryLabel,
                  ),
                ),
              const SizedBox(width: GlassSpacing.s6),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      note.title,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: t.bodyEmph.copyWith(
                        color: task?.done ?? false ? tokens.secondaryLabel : tokens.palette.label,
                        decoration: task?.done ?? false ? TextDecoration.lineThrough : null,
                      ),
                    ),
                    if (preview.isNotEmpty)
                      Text(
                        preview,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: t.caption.copyWith(color: tokens.secondaryLabel),
                      ),
                    if (facts.isNotEmpty) ...[
                      const SizedBox(height: GlassSpacing.s4),
                      Wrap(spacing: 10, runSpacing: 4, crossAxisAlignment: WrapCrossAlignment.center, children: facts),
                    ],
                  ],
                ),
              ),
              GlassMenuButton<_RowAction>(
                entries: [
                  GlassMenuItem(
                    value: _RowAction.pin,
                    label: note.pinned ? l.notesUnpin : l.notesPin,
                    icon: Icons.push_pin_outlined,
                  ),
                  if (note.remindAt != null || task != null)
                    GlassMenuItem(value: _RowAction.snooze, label: l.notesSnooze, icon: Icons.snooze_rounded),
                  if (current != null && note.target != current!.target)
                    GlassMenuItem(
                      value: _RowAction.attach,
                      label: l.notesAttachHere(current!.label),
                      icon: Icons.link_rounded,
                    ),
                  GlassMenuItem(
                    key: ValueKey('note-rebind-${note.id}'),
                    value: _RowAction.rebind,
                    label: l.notesAttachTo,
                    icon: Icons.add_link_rounded,
                  ),
                  if (note.target != null)
                    GlassMenuItem(value: _RowAction.detach, label: l.notesDetach, icon: Icons.link_off_rounded),
                  const GlassMenuDivider(),
                  GlassMenuItem(
                    value: _RowAction.delete,
                    label: l.notesDelete,
                    icon: Icons.delete_outline_rounded,
                    destructive: true,
                  ),
                ],
                onSelected: (action) => _act(context, ref, action),
                builder: (context, open) => GlassIconButton(
                  key: ValueKey('note-menu-${note.id}'),
                  icon: Icons.more_horiz_rounded,
                  tooltip: l.hostsMoreTooltip,
                  style: GlassIconButtonStyle.plain,
                  onPressed: open,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Future<void> _act(BuildContext context, WidgetRef ref, _RowAction action) async {
    switch (action) {
      case _RowAction.pin:
        await onSave(note.copyWith(pinned: !note.pinned));
      case _RowAction.snooze:
        await onSave(note.copyWith(remindAt: () => DateTime.now().add(const Duration(hours: 1))));
      case _RowAction.attach:
        await onSave(note.copyWith(target: () => current?.target, targetLabel: () => current?.label));
      case _RowAction.rebind:
        final host = await showHostPicker(context, title: context.l10n.notesAttachTo, protocol: null);
        if (host == null || !context.mounted) return;
        final to = hostNoteContext(host);
        await onSave(note.copyWith(target: () => to.target, targetLabel: () => to.label));
      case _RowAction.detach:
        await onSave(note.copyWith(target: () => null, targetLabel: () => null));
      case _RowAction.delete:
        final l = context.l10n;
        final yes = await showConfirmDialog(
          context,
          title: l.notesDelete,
          message: l.notesDeleteMessage(note.title),
          confirmLabel: l.commonDelete,
          destructive: true,
        );
        final service = ref.read(noteServiceProvider);
        if (yes && context.mounted && service != null) await runWithFeedback(context, () => service.delete(note.id));
    }
  }
}

enum _RowAction { pin, snooze, attach, rebind, detach, delete }

class _Fact extends StatelessWidget {
  const _Fact({required this.icon, required this.text, required this.color, this.strike = false, super.key});
  final IconData icon;
  final String text;
  final Color color;

  /// The former name of an object that is gone.
  final bool strike;

  @override
  Widget build(BuildContext context) => Row(
    mainAxisSize: MainAxisSize.min,
    children: [
      Icon(icon, size: 13, color: color),
      const SizedBox(width: 3),
      Flexible(
        child: Text(
          text,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: GlassTokens.of(context).typography.caption
              .copyWith(color: color, decoration: strike ? TextDecoration.lineThrough : null),
        ),
      ),
    ],
  );
}

/// Name and icon of what a note is attached to; a null name means the saved
/// object no longer exists here (an empty one: still loading).
(String?, IconData) noteTargetLabel(WidgetRef ref, NoteTarget target) => switch (target) {
  NoteObjectTarget(kind: NoteObjectKind.host || NoteObjectKind.rdpHost, :final id) => (
    ref.watch(hostsProvider).value?.where((h) => h.id.value == id).firstOrNull?.name ??
        (ref.watch(hostsProvider).hasValue ? null : ''),
    Icons.dns_outlined,
  ),
  NoteObjectTarget(kind: NoteObjectKind.dbConnection, :final id) => () {
    final list = ref.watch(dbConnectionsProvider(ref.watch(dbScopeProvider)));
    return (
      list.value?.where((c) => c.id == id).firstOrNull?.name ?? (list.hasValue ? null : ''),
      Icons.storage_rounded,
    );
  }(),
  NoteObjectTarget(kind: NoteObjectKind.kubeContext, :final id) => () {
    final list = ref.watch(kubeConnectionsProvider(ref.watch(kubeScopeProvider)));
    return (
      list.value?.where((c) => c.id == id).firstOrNull?.name ?? (list.hasValue ? null : ''),
      Icons.anchor_rounded,
    );
  }(),
  NoteKubeTarget(:final contextId, :final namespace, :final kind, :final name) => () {
    final list = ref.watch(kubeConnectionsProvider(ref.watch(kubeScopeProvider)));
    final gone = list.hasValue && !list.value!.any((c) => c.id == contextId);
    return (gone ? null : noteKubeLabel(kind, namespace, name), Icons.anchor_rounded);
  }(),
  NoteDockerTarget(:final hostId, :final name) => () {
    final hosts = ref.watch(hostsProvider);
    final gone = hostId != null && hosts.hasValue && !hosts.value!.any((h) => h.id.value == hostId);
    return (gone ? null : name, Icons.view_in_ar_outlined);
  }(),
  NoteUnknownTarget() => (null, Icons.link_off_rounded),
};

/// Whether [note] is attached to an object that is gone (deleted, left with
/// a share or a team, or unknown to this version). Such notes stay, marked
/// "Not linked", and can be attached again.
bool noteNotLinked(WidgetRef ref, Note note) => note.target != null && noteTargetLabel(ref, note.target!).$1 == null;

String formatNoteTime(BuildContext context, DateTime time) {
  final locale = Localizations.localeOf(context).toLanguageTag();
  final now = DateTime.now();
  final sameDay = time.year == now.year && time.month == now.month && time.day == now.day;
  return sameDay
      ? DateFormat.Hm(locale).format(time)
      : time.year == now.year
      ? DateFormat.MMMd(locale).add_Hm().format(time)
      : DateFormat.yMMMd(locale).add_Hm().format(time);
}

/// Edits one note in place. Changes are saved after a short pause, with
/// Save (⌘S / Ctrl+S) and when the editor closes; a note is created once it
/// has a title or text. Leaving with changes that could not be saved asks
/// first.
class _NoteEditor extends ConsumerStatefulWidget {
  const _NoteEditor({required this.initial, required this.current, required this.onClose, super.key});
  final Note initial;
  final NoteContext? current;
  final VoidCallback onClose;

  @override
  ConsumerState<_NoteEditor> createState() => _NoteEditorState();
}

class _NoteEditorState extends ConsumerState<_NoteEditor> {
  late final _title = TextEditingController(text: widget.initial.title);
  late final _body = TextEditingController(text: widget.initial.body);
  late final _tags = TextEditingController(text: widget.initial.tags.join(', '));
  late Note _note = widget.initial;
  bool _preview = false, _dirty = false, _failed = false;
  Timer? _debounce;
  Future<void> _saving = Future.value();

  @override
  void initState() {
    super.initState();
    _preview = widget.initial.id.isNotEmpty && widget.initial.body.isNotEmpty;
  }

  @override
  void dispose() {
    _debounce?.cancel();
    _title.dispose();
    _body.dispose();
    _tags.dispose();
    super.dispose();
  }

  Note _draft() {
    final body = _body.text;
    var title = _title.text.trim();
    if (title.isEmpty) {
      // A quick note without a title is named after its first line.
      final first = body.trim().split('\n').first.trim();
      title = first.length > 80 ? first.substring(0, 80) : first;
    }
    return Note(
      id: _note.id,
      title: title,
      body: body,
      tags: [
        for (final tag in _tags.text.split(','))
          if (tag.trim().isNotEmpty) tag.trim(),
      ],
      pinned: _note.pinned,
      task: _note.task,
      remindAt: _note.remindAt,
      target: _note.target,
      // The live name while the object exists, else the name it had.
      targetLabel: _note.target == null
          ? null
          : switch (noteTargetLabel(ref, _note.target!).$1) {
              final name? when name.isNotEmpty => name,
              _ => _note.targetLabel,
            },
      createdAt: _note.createdAt,
      updatedAt: _note.updatedAt,
    );
  }

  void _changed() {
    if (!_dirty) setState(() => _dirty = true);
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 800), () => unawaited(_save()));
  }

  void _update(Note note) {
    setState(() => _note = note);
    _changed();
  }

  /// Saves are serialized so a slow save cannot create the note twice.
  Future<void> _save() {
    _debounce?.cancel();
    return _saving = _saving.then((_) async {
      if (!_dirty) return;
      final draft = _draft();
      final service = ref.read(noteServiceProvider);
      if (draft.title.isEmpty || service == null) return;
      _dirty = false;
      try {
        final saved = await service.save(draft);
        if (mounted) {
          setState(() {
            _failed = false;
            _note = saved.copyWith(task: () => _note.task, remindAt: () => _note.remindAt);
          });
        }
      } on Object {
        _dirty = true;
        if (mounted) setState(() => _failed = true);
      }
    });
  }

  /// Save now: the button and ⌘S / Ctrl+S.
  Future<void> _saveNow() async {
    if (_note.id.isEmpty) _dirty = true;
    await _save();
    if (mounted) setState(() {});
  }

  Future<void> _close() async {
    await _save();
    if (!mounted) return;
    if (_dirty && _draft().title.isNotEmpty) {
      final l = context.l10n;
      final leave = await showConfirmDialog(
        context,
        title: l.notesUnsavedTitle,
        message: l.notesUnsavedMessage,
        confirmLabel: l.notesDiscardChanges,
        destructive: true,
      );
      if (!leave || !mounted) return;
    }
    widget.onClose();
  }

  Future<void> _chooseTarget() async {
    final host = await showHostPicker(context, title: context.l10n.notesChooseHost, protocol: null);
    if (host == null || !mounted) return;
    final to = hostNoteContext(host);
    _update(_note.copyWith(target: () => to.target, targetLabel: () => to.label));
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context), t = tokens.typography;
    final current = widget.current;
    final task = _note.task;
    final targetLabel = _note.target == null ? null : noteTargetLabel(ref, _note.target!);
    final canSave = _draft().title.isNotEmpty && (_dirty || _note.id.isEmpty);
    final status = _failed
        ? (l.notesSaveFailed, tokens.palette.danger)
        : _dirty && _draft().title.isNotEmpty
        ? (l.notesUnsaved, tokens.palette.warning)
        : _note.id.isNotEmpty
        ? (l.notesSaved, tokens.secondaryLabel)
        : null;
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyS, meta: true): () => unawaited(_saveNow()),
        const SingleActivator(LogicalKeyboardKey.keyS, control: true): () => unawaited(_saveNow()),
      },
      child: Padding(
        padding: const EdgeInsets.fromLTRB(8, 4, 12, 12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                GlassIconButton(
                  key: const ValueKey('note-back'),
                  icon: Icons.arrow_back_rounded,
                  tooltip: l.notesBack,
                  style: GlassIconButtonStyle.plain,
                  onPressed: () => unawaited(_close()),
                ),
                const SizedBox(width: GlassSpacing.s6),
                Expanded(
                  child: GlassSegmented<bool>(
                    key: const ValueKey('note-preview'),
                    inChrome: false,
                    expand: true,
                    semanticLabel: l.notesBody,
                    segments: [
                      GlassSegment(value: false, label: l.notesEdit),
                      GlassSegment(value: true, label: l.notesPreview),
                    ],
                    selected: _preview,
                    onChanged: (value) => setState(() => _preview = value),
                  ),
                ),
                const SizedBox(width: GlassSpacing.s6),
                GlassButton.prominent(
                  key: const ValueKey('note-save'),
                  label: l.commonSave,
                  icon: Icons.check_rounded,
                  size: GlassControlSize.sm,
                  tooltip: l.notesSaveTooltip,
                  onPressed: canSave ? () => unawaited(_saveNow()) : null,
                ),
              ],
            ),
            if (status case (final text, final color))
              Padding(
                padding: const EdgeInsets.only(top: GlassSpacing.s4, left: GlassSpacing.s4),
                child: Text(
                  text,
                  key: const ValueKey('note-status'),
                  style: t.caption.copyWith(color: color),
                ),
              ),
            const SizedBox(height: GlassSpacing.s8),
            Expanded(
              child: ListView(
                children: [
                  GlassField(
                    key: const ValueKey('note-title'),
                    controller: _title,
                    placeholder: l.notesTitle,
                    autofocus: widget.initial.id.isEmpty,
                    onChanged: (_) => _changed(),
                  ),
                  const SizedBox(height: GlassSpacing.s8),
                  if (_preview)
                    ContentSurface(
                      padding: const EdgeInsets.all(GlassSpacing.s8),
                      child: _body.text.trim().isEmpty
                          ? Text(l.notesBodyEmpty, style: t.callout.copyWith(color: tokens.secondaryLabel))
                          : MarkdownText(_body.text),
                    )
                  else
                    GlassField(
                      key: const ValueKey('note-body'),
                      controller: _body,
                      placeholder: l.notesBody,
                      maxLines: 12,
                      keyboardType: TextInputType.multiline,
                      textInputAction: TextInputAction.newline,
                      onChanged: (_) => _changed(),
                    ),
                  const SizedBox(height: GlassSpacing.s8),
                  GlassField(
                    key: const ValueKey('note-tags'),
                    controller: _tags,
                    placeholder: l.notesTags,
                    onChanged: (_) => _changed(),
                  ),
                  const SizedBox(height: GlassSpacing.s12),
                  _Line(
                    label: l.notesTask,
                    child: Switch.adaptive(
                      key: const ValueKey('note-task'),
                      value: task != null,
                      onChanged: (on) => _update(_note.copyWith(task: () => on ? const NoteTask() : null)),
                    ),
                  ),
                  if (task != null) ...[
                    _Line(
                      label: l.notesDue,
                      child: _WhenSelect(
                        key: const ValueKey('note-due'),
                        value: task.dueAt,
                        onChanged: (when) => _update(
                          _note.copyWith(
                            task: () => NoteTask(done: task.done, dueAt: when),
                          ),
                        ),
                      ),
                    ),
                    _Line(
                      label: l.notesDone,
                      child: Switch.adaptive(
                        key: const ValueKey('note-task-done'),
                        value: task.done,
                        onChanged: (done) => _update(
                          _note.copyWith(
                            task: () => NoteTask(done: done, dueAt: task.dueAt),
                          ),
                        ),
                      ),
                    ),
                  ],
                  _Line(
                    label: l.notesReminder,
                    child: _WhenSelect(
                      key: const ValueKey('note-reminder'),
                      value: _note.remindAt,
                      onChanged: (when) => _update(_note.copyWith(remindAt: () => when)),
                    ),
                  ),
                  _Line(
                    label: l.notesAttachedTo,
                    child: GlassSelect<int>(
                      key: const ValueKey('note-target'),
                      semanticLabel: l.notesAttachedTo,
                      value: _note.target == null ? 0 : (_note.target == current?.target ? 1 : 2),
                      items: [
                        GlassSelectItem(value: 0, label: l.notesNoTarget),
                        if (current != null) GlassSelectItem(value: 1, label: current.label),
                        if (_note.target != null && _note.target != current?.target)
                          GlassSelectItem(
                            value: 2,
                            label:
                                targetLabel?.$1 ??
                                (_note.targetLabel == null
                                    ? l.notesNotLinked
                                    : l.notesNotLinkedWas(_note.targetLabel!)),
                          ),
                        GlassSelectItem(value: 3, label: l.notesChooseHostEllipsis),
                      ],
                      onChanged: (value) {
                        if (value == 3) return unawaited(_chooseTarget());
                        _update(
                          _note.copyWith(
                            target: () => switch (value) {
                              0 => null,
                              1 => current?.target,
                              _ => _note.target,
                            },
                            targetLabel: () => switch (value) {
                              0 => null,
                              1 => current?.label,
                              _ => _note.targetLabel,
                            },
                          ),
                        );
                      },
                    ),
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

class _Line extends StatelessWidget {
  const _Line({required this.label, required this.child});
  final String label;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        children: [
          Expanded(
            child: Text(label, style: tokens.typography.callout.copyWith(color: tokens.palette.label)),
          ),
          Flexible(
            child: Align(alignment: AlignmentDirectional.centerEnd, child: child),
          ),
        ],
      ),
    );
  }
}

enum _When { none, hour, evening, tomorrow, week, custom, current }

/// A due time or reminder: quick choices, or a typed `YYYY-MM-DD HH:MM`.
class _WhenSelect extends StatelessWidget {
  const _WhenSelect({required this.value, required this.onChanged, super.key});
  final DateTime? value;
  final ValueChanged<DateTime?> onChanged;

  static DateTime? resolve(_When when, DateTime now) => switch (when) {
    _When.none => null,
    _When.hour => now.add(const Duration(hours: 1)),
    _When.evening => DateTime(now.year, now.month, now.day, 18),
    _When.tomorrow => DateTime(now.year, now.month, now.day + 1, 9),
    _When.week => DateTime(now.year, now.month, now.day + 7, 9),
    _When.custom || _When.current => null,
  };

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final now = DateTime.now();
    return GlassSelect<_When>(
      semanticLabel: l.notesWhenCustom,
      value: value == null ? _When.none : _When.current,
      items: [
        if (value != null) GlassSelectItem(value: _When.current, label: formatNoteTime(context, value!)),
        GlassSelectItem(value: _When.none, label: l.notesWhenNone),
        GlassSelectItem(value: _When.hour, label: l.notesWhenInHour),
        if (now.hour < 17) GlassSelectItem(value: _When.evening, label: l.notesWhenThisEvening),
        GlassSelectItem(value: _When.tomorrow, label: l.notesWhenTomorrow),
        GlassSelectItem(value: _When.week, label: l.notesWhenNextWeek),
        GlassSelectItem(value: _When.custom, label: l.notesWhenCustom),
      ],
      onChanged: (when) async {
        if (when == _When.current) return;
        if (when != _When.custom) return onChanged(resolve(when, DateTime.now()));
        final typed = await showTextInputDialog(
          context,
          title: l.notesWhenCustom,
          label: l.notesWhenCustomHint,
          initial: _typedFormat.format(value ?? now.add(const Duration(hours: 1))),
        );
        if (typed == null || !context.mounted) return;
        final parsed = parseNoteTime(typed);
        if (parsed == null) {
          showSnack(context, l.notesInvalidTime);
        } else {
          onChanged(parsed);
        }
      },
    );
  }
}

final _typedFormat = DateFormat('yyyy-MM-dd HH:mm'); // l10n-ignore: date pattern

/// `YYYY-MM-DD HH:MM` in local time, or null.
DateTime? parseNoteTime(String text) {
  final match = RegExp(r'^\s*(\d{4})-(\d{1,2})-(\d{1,2})[ T](\d{1,2}):(\d{2})\s*$').firstMatch(text);
  if (match == null) return null;
  final [year, month, day, hour, minute] = [for (var i = 1; i <= 5; i++) int.parse(match.group(i)!)];
  final time = DateTime(year, month, day, hour, minute);
  return time.month == month && time.day == day && hour < 24 && minute < 60 ? time : null;
}
