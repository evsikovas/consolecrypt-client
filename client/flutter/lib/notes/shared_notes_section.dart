import 'dart:async';

import 'package:consolecrypt/ai/markdown_text.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/notes/note_models.dart';
import 'package:consolecrypt/notes/notes_panel.dart';
import 'package:consolecrypt/notes/notes_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Notes everyone with access to the shared object sees, under the personal
/// notes of the object. Tasks and reminders stay personal.
class SharedNotesSection extends ConsumerWidget {
  const SharedNotesSection({required this.shareId, required this.target, required this.query, super.key});
  final String shareId;
  final NoteTarget target;
  final String query;

  bool _matches(SharedNote note) {
    final q = query.trim().toLowerCase();
    return q.isEmpty ||
        note.title.toLowerCase().contains(q) ||
        note.body.toLowerCase().contains(q) ||
        note.tags.any((tag) => tag.toLowerCase().contains(q));
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n, tokens = GlassTokens.of(context), t = tokens.typography;
    final state = ref.watch(sharedNotesProvider(shareId));
    final shared = state.value;
    if (shared != null && !shared.enabled) return const SizedBox.shrink();
    final scope = noteScope(target);
    final notes = [
      for (final note in shared?.notes ?? const <SharedNote>[])
        if (sameNoteScope(note.scope, scope) && _matches(note)) note,
    ]..sort((a, b) => (b.updatedAt ?? DateTime(0)).compareTo(a.updatedAt ?? DateTime(0)));
    final canWrite = shared != null && !shared.waitingForOwner;
    return Column(
      key: const ValueKey('shared-notes'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: GlassSpacing.s8),
        Row(
          children: [
            Icon(Icons.group_outlined, size: 15, color: tokens.secondaryLabel),
            const SizedBox(width: GlassSpacing.s6),
            Expanded(
              child: Text(l.notesShared, style: t.callout.copyWith(color: tokens.secondaryLabel)),
            ),
            GlassIconButton(
              key: const ValueKey('shared-notes-refresh'),
              icon: Icons.refresh_rounded,
              tooltip: l.notesSharedRefresh,
              style: GlassIconButtonStyle.plain,
              onPressed: () => ref.invalidate(sharedNotesProvider(shareId)),
            ),
            GlassIconButton(
              key: const ValueKey('shared-notes-new'),
              icon: Icons.add_rounded,
              tooltip: l.notesSharedNew,
              style: GlassIconButtonStyle.plain,
              onPressed: canWrite ? () => _open(context, ref, null, shared) : null,
            ),
          ],
        ),
        if (state.isLoading && shared == null)
          const Padding(padding: EdgeInsets.all(GlassSpacing.s8), child: LinearProgressIndicator())
        else if (state.hasError && shared == null)
          Text(l.notesSharedFailed, style: t.caption.copyWith(color: tokens.palette.warning))
        else ...[
          if (shared?.waitingForOwner ?? false)
            Text(l.notesSharedWaiting, style: t.caption.copyWith(color: tokens.secondaryLabel)),
          if ((shared?.pending ?? 0) > 0)
            Text(l.notesSharedPending(shared!.pending), style: t.caption.copyWith(color: tokens.secondaryLabel)),
          if (notes.isEmpty && canWrite)
            Text(l.notesSharedEmpty, style: t.caption.copyWith(color: tokens.secondaryLabel)),
          for (final note in notes)
            InkWell(
              key: ValueKey('shared-note-${note.id}'),
              borderRadius: BorderRadius.circular(8),
              onTap: () => _open(context, ref, note, shared),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 8),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Padding(
                      padding: const EdgeInsets.all(4),
                      child: Icon(Icons.people_alt_outlined, size: 16, color: tokens.secondaryLabel),
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
                            style: t.bodyEmph.copyWith(color: tokens.palette.label),
                          ),
                          if (note.body.trim().isNotEmpty)
                            Text(
                              note.body.trim().split('\n').first,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: t.caption.copyWith(color: tokens.secondaryLabel),
                            ),
                          const SizedBox(height: GlassSpacing.s4),
                          Wrap(
                            spacing: 10,
                            runSpacing: 4,
                            crossAxisAlignment: WrapCrossAlignment.center,
                            children: [
                              Text(_author(l, note, shared), style: t.caption.copyWith(color: tokens.secondaryLabel)),
                              if (note.updatedAt case final updated?)
                                Text(
                                  formatNoteTime(context, updated),
                                  style: t.caption.copyWith(color: tokens.secondaryLabel),
                                ),
                              if (note.tags.isNotEmpty) TagChips(tags: note.tags),
                            ],
                          ),
                        ],
                      ),
                    ),
                    if (note.canDelete)
                      GlassIconButton(
                        key: ValueKey('shared-note-delete-${note.id}'),
                        icon: Icons.delete_outline_rounded,
                        tooltip: l.notesDelete,
                        style: GlassIconButtonStyle.plain,
                        onPressed: () => _delete(context, ref, note),
                      ),
                  ],
                ),
              ),
            ),
        ],
      ],
    );
  }

  static String _author(AppLocalizations l, SharedNote note, SharedNotes? shared) => note.mine
      ? l.notesAuthorYou
      : note.authorUserId == shared?.ownerUserId
      ? l.notesAuthorOwner
      : l.notesAuthorMember;

  Future<void> _open(BuildContext context, WidgetRef ref, SharedNote? note, SharedNotes? shared) async {
    final saved = await showAppDialog<bool>(
      context,
      builder: (_) => _SharedNoteDialog(
        shareId: shareId,
        note: note,
        scope: noteScope(target),
        author: note == null ? null : _author(context.l10n, note, shared),
      ),
    );
    if (saved ?? false) ref.invalidate(sharedNotesProvider(shareId));
  }

  Future<void> _delete(BuildContext context, WidgetRef ref, SharedNote note) async {
    final l = context.l10n;
    final yes = await showConfirmDialog(
      context,
      title: l.notesDelete,
      message: l.notesSharedDeleteMessage(note.title),
      confirmLabel: l.commonDelete,
      destructive: true,
    );
    final service = ref.read(sharedNoteServiceProvider);
    if (!yes || !context.mounted || service == null) return;
    final done = await runWithFeedback(context, () async {
      await service.deleteShared(shareId, note.id);
      return true;
    });
    if (done ?? false) ref.invalidate(sharedNotesProvider(shareId));
  }
}

/// Writes this user's shared note, or shows someone else's.
class _SharedNoteDialog extends ConsumerStatefulWidget {
  const _SharedNoteDialog({required this.shareId, required this.note, required this.scope, required this.author});
  final String shareId;
  final SharedNote? note;
  final Map<String, dynamic>? scope;
  final String? author;

  @override
  ConsumerState<_SharedNoteDialog> createState() => _SharedNoteDialogState();
}

class _SharedNoteDialogState extends ConsumerState<_SharedNoteDialog> {
  late final _title = TextEditingController(text: widget.note?.title ?? '');
  late final _body = TextEditingController(text: widget.note?.body ?? '');
  late final _tags = TextEditingController(text: widget.note?.tags.join(', ') ?? '');
  bool _busy = false;

  bool get _editable => widget.note?.mine ?? true;

  @override
  void dispose() {
    _title.dispose();
    _body.dispose();
    _tags.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final service = ref.read(sharedNoteServiceProvider);
    final title = _title.text.trim();
    if (service == null || title.isEmpty || _busy) return;
    setState(() => _busy = true);
    final saved = await runWithFeedback(
      context,
      () => service.saveShared(
        widget.shareId,
        id: widget.note?.id ?? '',
        title: title,
        body: _body.text,
        tags: [
          for (final tag in _tags.text.split(','))
            if (tag.trim().isNotEmpty) tag.trim(),
        ],
        // A note keeps the part of the object it was written about.
        scope: widget.note?.scope ?? widget.scope,
      ),
    );
    if (!mounted) return;
    if (saved != null) {
      closeDialog(context, true);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final note = widget.note;
    return GlassDialog(
      title: note == null ? l.notesSharedNew : (_editable ? note.title : note.title),
      width: 520,
      content: _editable
          ? Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(l.notesSharedHint, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                const SizedBox(height: GlassSpacing.s8),
                GlassField(
                  key: const ValueKey('shared-note-title'),
                  controller: _title,
                  placeholder: l.notesTitle,
                  autofocus: note == null,
                ),
                const SizedBox(height: GlassSpacing.s8),
                GlassField(
                  key: const ValueKey('shared-note-body'),
                  controller: _body,
                  placeholder: l.notesBody,
                  maxLines: 10,
                  keyboardType: TextInputType.multiline,
                  textInputAction: TextInputAction.newline,
                ),
                const SizedBox(height: GlassSpacing.s8),
                GlassField(key: const ValueKey('shared-note-tags'), controller: _tags, placeholder: l.notesTags),
              ],
            )
          : Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (widget.author case final author?)
                  Text(author, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                const SizedBox(height: GlassSpacing.s8),
                if (note!.body.trim().isEmpty)
                  Text(l.notesBodyEmpty, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel))
                else
                  Flexible(child: SingleChildScrollView(child: MarkdownText(note.body))),
                if (note.tags.isNotEmpty) ...[const SizedBox(height: GlassSpacing.s8), TagChips(tags: note.tags)],
                const SizedBox(height: GlassSpacing.s8),
                Text(l.notesSharedReadOnly, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
              ],
            ),
      secondaryActions: [
        GlassButton(label: _editable ? l.commonCancel : l.commonClose, onPressed: () => closeDialog<bool>(context)),
      ],
      primaryAction: _editable
          ? GlassButton.prominent(
              key: const ValueKey('shared-note-save'),
              label: l.commonSave,
              busy: _busy,
              onPressed: _busy ? null : () => unawaited(_save()),
            )
          : null,
    );
  }
}
