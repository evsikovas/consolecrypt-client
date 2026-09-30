import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showSharingGroupPublish(BuildContext context, {required String groupId}) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingGroupPublishDialog(groupId: groupId));

Future<void> showSharingGroupEdit(BuildContext context, SharingItem item) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingGroupPublishDialog.edit(item));

class SharingGroupPublishDialog extends ConsumerStatefulWidget {
  const SharingGroupPublishDialog({required this.groupId, super.key}) : editing = null;
  const SharingGroupPublishDialog.edit(this.editing, {super.key}) : groupId = null;
  final String? groupId;
  final SharingItem? editing;
  @override
  ConsumerState<SharingGroupPublishDialog> createState() => _SharingGroupPublishDialogState();
}

class _SharingGroupPublishDialogState extends ConsumerState<SharingGroupPublishDialog> {
  Object? _profile;
  List<SharingItem> _available = [];
  List<Map<String, Object?>> _retained = [];
  List<String> _tags = [];
  final _name = TextEditingController();
  final Set<String> _selected = {};
  List<SharingGrant> _grants = [];
  String? _preview;
  bool _busy = false, _ready = false, _failed = false;
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    _profile = ref.read(sharingSessionScopeProvider);
    _load();
  }

  @override
  void dispose() {
    _name.clear();
    _name.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    final generation = ++_generation;
    setState(() => _busy = true);
    try {
      final service = ref.read(sharingServiceProvider);
      final status = await service.status();
      if (!status.enabled || status.locked || !status.supportsGroups) throw StateError('unavailable');
      final items = await service.list();
      if (!mounted || !sharingSessionCurrent(ref, _profile) || generation != _generation) return;
      _available = items
          .where(
            (item) =>
                item.trust == SharingTrust.verified &&
                item.previewJson != null &&
                item.kind != SharingKind.group &&
                (item.kind != SharingKind.secret || status.supportsSecrets),
          )
          .toList(growable: false);
      final String preview;
      if (widget.editing case final item?) {
        if (!item.canEdit || item.kind != SharingKind.group || item.data == null) throw StateError('unavailable');
        final data = item.data!;
        _name.text = data['name'] as String;
        _tags = (data['tags'] as List).cast<String>();
        final refs = <Map<String, Object?>>[];
        final seen = <String>{};
        for (final raw in data['children'] as List) {
          final r = raw as Map<String, dynamic>;
          final id = r['share_id'] as String;
          final object = r['item_id'] as String;
          final kind = r['kind'] as String;
          // Nested groups need a separate bounded navigation/edit contract.
          // Fail closed instead of silently dropping unsupported references.
          if (!const ['host', 'snippet', 'secret'].contains(kind) || !seen.add(id)) throw StateError('unavailable');
          refs.add({'share_id': id, 'item_id': object, 'kind': kind});
        }
        _selected.addAll(seen);
        _retained = refs
            .where(
              (r) =>
                  !_available.any((i) => i.id == r['share_id'] && i.itemId == r['item_id'] && i.kind.name == r['kind']),
            )
            .toList(growable: false);
        _available = _available.where((i) => !_retained.any((r) => r['share_id'] == i.id)).toList(growable: false);
        preview = _editorProjection();
      } else {
        preview = await service.previewGroup(widget.groupId!, _childrenJson());
      }
      if (mounted && sharingSessionCurrent(ref, _profile) && generation == _generation) {
        setState(() {
          _preview = preview;
          _ready = true;
          _failed = false;
        });
      }
    } catch (_) {
      if (mounted && generation == _generation) setState(() => _failed = true);
    } finally {
      if (mounted && generation == _generation) setState(() => _busy = false);
    }
  }

  String _childrenJson() => jsonEncode([
    for (final item in _available)
      if (_selected.contains(item.id)) {'share_id': item.id, 'item_id': item.itemId, 'kind': item.kind.name},
    for (final reference in _retained)
      if (_selected.contains(reference['share_id'])) reference,
  ]);

  String _editorProjection() => jsonEncode({
    'kind': 'group',
    'data': {'name': _name.text, 'tags': _tags, 'children': jsonDecode(_childrenJson())},
  });

  Future<void> _select(String id, bool selected) async {
    if (_busy || !sharingSessionCurrent(ref, _profile)) return;
    final generation = ++_generation;
    setState(() {
      if (selected) {
        _selected.add(id);
      } else {
        _selected.remove(id);
      }
      _preview = null;
      _busy = true;
      _failed = false;
    });
    try {
      final preview = widget.editing == null
          ? await ref.read(sharingServiceProvider).previewGroup(widget.groupId!, _childrenJson())
          : _editorProjection();
      if (mounted && generation == _generation && sharingSessionCurrent(ref, _profile)) {
        setState(() => _preview = preview);
      }
    } catch (_) {
      if (mounted && generation == _generation) setState(() => _failed = true);
    } finally {
      if (mounted && generation == _generation) setState(() => _busy = false);
    }
  }

  Future<void> _publish() async {
    if (_busy ||
        _preview == null ||
        (widget.editing == null && _grants.isEmpty) ||
        (widget.editing != null && _name.text.trim().isEmpty) ||
        !sharingSessionCurrent(ref, _profile)) {
      return;
    }
    setState(() => _busy = true);
    try {
      final Object? result;
      if (widget.editing case final item?) {
        if (!item.canEdit) return;
        result = await runWithFeedback(context, () => ref.read(sharingServiceProvider).edit(item.id, _preview!));
      } else {
        result = await runWithFeedback(context, () => ref.read(sharingServiceProvider).publish(_preview!, _grants));
      }
      if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
      if (result != null) {
        // Child ACLs are independent. This publishes only the group projection.
        await runWithFeedback(context, () => ref.read(sharingServiceProvider).flush());
        if (!mounted || !sharingSessionCurrent(ref, _profile)) return;
        refreshSharing(ref);
        closeDialog<void>(context);
      }
    } catch (_) {
      if (mounted) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final previewTags = _preview == null
        ? const <String>[]
        : (((jsonDecode(_preview!) as Map<String, dynamic>)['data'] as Map<String, dynamic>)['tags'] as List)
              .cast<String>();
    return SharingDialogGuard(
      profile: _profile,
      child: GlassDialog(
        key: const ValueKey('sharing-group-dialog'),
        title: widget.editing == null ? l.sharingGroup : l.sharingEdit,
        icon: Icons.folder_shared_outlined,
        width: 600,
        content: ConstrainedBox(
          constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(l.sharingCollectionHelp),
                if (_busy) const LinearProgressIndicator(),
                if (_failed) Text(l.sharingUnavailable),
                if (_preview != null) ...[
                  const SizedBox(height: 12),
                  SharingProjectionView(_preview!),
                  if (previewTags.isNotEmpty) ...[
                    Text(l.tagsLabel, style: Theme.of(context).textTheme.labelMedium),
                    Text(previewTags.join(' · ')),
                  ],
                ],
                if (_ready) ...[
                  if (widget.editing != null)
                    TextField(
                      key: const ValueKey('sharing-group-name'),
                      controller: _name,
                      enabled: !_busy,
                      decoration: InputDecoration(labelText: l.sharingName),
                      onChanged: (_) => setState(() => _preview = _editorProjection()),
                    ),
                  const SizedBox(height: 12),
                  Text(l.sharingCollectionChildren, style: Theme.of(context).textTheme.titleMedium),
                  if (_available.isEmpty) Text(l.sharingCollectionEmpty),
                  for (final item in _available)
                    CheckboxListTile(
                      key: ValueKey('sharing-child-${item.id}'),
                      contentPadding: EdgeInsets.zero,
                      title: Text(item.name ?? item.id),
                      subtitle: Text(_kindLabel(context, item.kind)),
                      value: _selected.contains(item.id),
                      onChanged: _busy || (_selected.length >= 256 && !_selected.contains(item.id))
                          ? null
                          : (value) => _select(item.id, value == true),
                    ),
                  for (final reference in _retained)
                    CheckboxListTile(
                      key: ValueKey('sharing-unresolved-${reference['share_id']}'),
                      contentPadding: EdgeInsets.zero,
                      secondary: const Icon(Icons.lock_outline),
                      title: Text(l.sharingCollectionUnavailable),
                      value: _selected.contains(reference['share_id']),
                      onChanged: _busy ? null : (value) => _select(reference['share_id'] as String, value == true),
                    ),
                  if (widget.editing == null) ...[
                    const SizedBox(height: 16),
                    SharingRecipients(onChanged: (grants) => setState(() => _grants = grants)),
                  ],
                ],
              ],
            ),
          ),
        ),
        secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<void>(context))],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('sharing-group-publish'),
          label: widget.editing == null ? l.sharingPublishConfirm : l.commonSave,
          onPressed:
              _busy ||
                  _preview == null ||
                  (widget.editing == null && _grants.isEmpty) ||
                  (widget.editing != null && _name.text.trim().isEmpty)
              ? null
              : _publish,
        ),
      ),
    );
  }
}

/// Resolves exact signed references against independently verified items.
/// A readable collection alone never makes a missing/untrusted child readable.
class SharingCollectionView extends StatelessWidget {
  const SharingCollectionView({required this.group, required this.items, this.onOpen, super.key});
  final SharingItem group;
  final List<SharingItem> items;
  final ValueChanged<SharingItem>? onOpen;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final children = group.kind == SharingKind.group && group.trust == SharingTrust.verified
        ? (group.data?['children'] as List?)
        : null;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.sharingCollectionHelp),
        for (final raw in children ?? const [])
          if (raw is Map<String, dynamic>) _child(context, raw),
      ],
    );
  }

  Widget _child(BuildContext context, Map<String, dynamic> reference) {
    SharingItem? resolved;
    for (final item in items) {
      if (item.id == reference['share_id'] &&
          item.itemId == reference['item_id'] &&
          item.kind.name == reference['kind'] &&
          item.trust == SharingTrust.verified &&
          item.previewJson != null) {
        resolved = item;
        break;
      }
    }
    final child = resolved;
    return ListTile(
      key: ValueKey('sharing-reference-${reference['share_id']}'),
      leading: Icon(child == null ? Icons.lock_outline : Icons.link),
      title: Text(child?.name ?? context.l10n.sharingCollectionUnavailable),
      subtitle: child == null ? null : Text(_kindLabel(context, child.kind)),
      enabled: child != null && onOpen != null,
      onTap: child == null || onOpen == null ? null : () => onOpen!(child),
    );
  }
}

String _kindLabel(BuildContext context, SharingKind kind) => switch (kind) {
  SharingKind.host => context.l10n.sharingHost,
  SharingKind.snippet => context.l10n.sharingSnippet,
  SharingKind.group => context.l10n.sharingGroup,
  SharingKind.secret => context.l10n.sharingSecret,
};
