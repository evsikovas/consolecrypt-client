import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/keepass/kdbx_client.dart';
import 'package:consolecrypt/keepass/password_manager.dart';
import 'package:flutter/gestures.dart';
import 'package:material_ui/material_ui.dart';

class KdbxEntryDrag {
  const KdbxEntryDrag(this.source, this.entry);
  final String source;
  final String entry;
}

/// Metadata-only workspace. Secrets are requested by the caller only for an
/// explicit copy or edit; the list and drag feedback never contain them.
class KdbxWorkspace extends StatelessWidget {
  const KdbxWorkspace({
    super.key,
    required this.manager,
    required this.session,
    required this.entries,
    required this.group,
    required this.selected,
    required this.search,
    required this.busy,
    required this.error,
    required this.onOpen,
    required this.onCreate,
    required this.onWebDav,
    required this.onSource,
    required this.onEditSource,
    required this.onSourceAction,
    required this.onGenerator,
    required this.onGroup,
    required this.onEntry,
    required this.onEdit,
    required this.onDelete,
    required this.onCopy,
    required this.onMove,
    required this.onNewEntry,
    required this.onNewGroup,
    required this.onLock,
    required this.onExport,
    required this.onProtect,
    required this.onRetrySave,
    required this.onSearch,
    required this.sidebarWidth,
    required this.detailsWidth,
    required this.onSidebarResize,
    required this.onDetailsResize,
  });
  final PasswordManager manager;
  final PasswordVaultSession? session;
  final List<KdbxData> entries;
  final String? group;
  final KdbxData? selected;
  final TextEditingController search;
  final bool busy;
  final String? error;
  final VoidCallback onOpen, onCreate, onWebDav, onNewEntry, onNewGroup, onLock, onExport, onRetrySave;
  final ValueChanged<KdbxSource> onSource, onEditSource;
  final void Function(KdbxSource, String) onSourceAction;
  final VoidCallback onGenerator;
  final ValueChanged<String?> onGroup;
  final ValueChanged<String?> onEntry;
  final ValueChanged<KdbxData> onEdit, onDelete;
  final void Function(KdbxData, String) onCopy;
  final void Function(KdbxEntryDrag, String) onMove;
  final ValueChanged<String> onProtect, onSearch;
  final double sidebarWidth, detailsWidth;
  final void Function(double delta, double minimum, double maximum) onSidebarResize, onDetailsResize;

  Widget _splitter(BuildContext context, String key, String label, ValueChanged<double> onDelta) => Semantics(
    label: label,
    onIncrease: () => onDelta(20),
    onDecrease: () => onDelta(-20),
    child: MouseRegion(
      cursor: SystemMouseCursors.resizeColumn,
      child: GestureDetector(
        key: ValueKey(key),
        behavior: HitTestBehavior.opaque,
        dragStartBehavior: DragStartBehavior.down,
        onHorizontalDragUpdate: (details) => onDelta(details.delta.dx),
        child: SizedBox(
          width: 10,
          child: Center(child: Container(width: 1, color: Theme.of(context).dividerColor)),
        ),
      ),
    ),
  );

  Widget _addMenu(BuildContext context) => GlassMenuButton<String>(
    entries: [
      GlassMenuItem(
        key: const ValueKey('keepass-add-open'),
        value: 'open',
        label: context.l10n.keepassOpen,
        icon: Icons.folder_open_rounded,
      ),
      GlassMenuItem(
        key: const ValueKey('keepass-add-create'),
        value: 'create',
        label: context.l10n.keepassCreate,
        icon: Icons.add_rounded,
      ),
      GlassMenuItem(
        key: const ValueKey('keepass-add-dav'),
        value: 'dav',
        label: context.l10n.keepassDavAdd,
        icon: Icons.cloud_outlined,
      ),
    ],
    onSelected: (action) => switch (action) {
      'open' => onOpen(),
      'create' => onCreate(),
      _ => onWebDav(),
    },
    builder: (context, open) => GlassIconButton(
      key: const ValueKey('keepass-add-database'),
      icon: Icons.add_rounded,
      size: 28,
      iconSize: 18,
      style: GlassIconButtonStyle.plain,
      tooltip: context.l10n.keepassAddDatabase,
      onPressed: busy ? null : open,
    ),
  );

  List<GlassMenuEntry<String>> _sourceEntries(BuildContext context, KdbxSource source) {
    final l = context.l10n;
    final opened = manager.sessions[source.id];
    final enabled = !busy && opened?.busy != true;
    GlassMenuItem<String> item(String action, String label, IconData icon, {bool destructive = false}) => GlassMenuItem(
      key: ValueKey('keepass-source-$action'),
      value: action,
      label: label,
      icon: icon,
      enabled: enabled,
      destructive: destructive,
    );
    return [
      item('open', opened == null ? l.keepassUnlock : l.keepassSelectSource, Icons.folder_open_rounded),
      if (opened != null) ...[
        item('lock', l.keepassLock, Icons.lock_outline_rounded),
        if (opened.data['dirty'] == true) item('save', l.keepassRetrySave, Icons.save_outlined),
        item('export', l.keepassExport, Icons.save_alt_rounded),
        const GlassMenuDivider(),
        item('hello', l.keepassHello, Icons.fingerprint_rounded),
        item('security_key', l.keepassSecurityKey, Icons.usb_rounded),
      ],
      if (source.remote) item('settings', l.commonEdit, Icons.settings_outlined),
      const GlassMenuDivider(),
      item('remove', l.keepassRemoveSource, Icons.delete_outline_rounded, destructive: true),
    ];
  }

  Future<void> _sourceContext(BuildContext context, KdbxSource source, Offset position) async {
    final action = await showGlassMenu<String>(
      context: context,
      anchor: Rect.fromLTWH(position.dx, position.dy, 0, 0),
      entries: _sourceEntries(context, source),
    );
    if (context.mounted && action != null) onSourceAction(source, action);
  }

  Widget _sourceTile(BuildContext context, KdbxSource source) => GestureDetector(
    onSecondaryTapDown: busy ? null : (details) => unawaited(_sourceContext(context, source, details.globalPosition)),
    onLongPressStart: busy || !AppPlatform.isMobile
        ? null
        : (details) => unawaited(_sourceContext(context, source, details.globalPosition)),
    child: ListTile(
      key: ValueKey('keepass-source-${source.id}'),
      dense: true,
      visualDensity: VisualDensity.standard,
      minTileHeight: source.remote ? 56 : 48,
      minVerticalPadding: 4,
      minLeadingWidth: 20,
      horizontalTitleGap: 10,
      contentPadding: const EdgeInsets.symmetric(horizontal: 10, vertical: 2),
      titleAlignment: ListTileTitleAlignment.center,
      selected: session?.source.id == source.id,
      selectedColor: Theme.of(context).colorScheme.onSurface,
      selectedTileColor: Theme.of(context).colorScheme.primary.withValues(alpha: .14),
      leading: Icon(
        manager.sessions.containsKey(source.id) ? Icons.lock_open_rounded : Icons.lock_outline_rounded,
        size: 18,
      ),
      title: Text(
        source.name,
        key: ValueKey('keepass-source-title-${source.id}'),
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
      ),
      subtitle: source.remote ? const Text('WebDAV') : null, // l10n-ignore: protocol.
      onTap: busy ? null : () => onSource(source),
      trailing: source.remote
          ? GlassIconButton(
              key: ValueKey('keepass-source-settings-${source.id}'),
              icon: Icons.settings_outlined,
              size: 28,
              iconSize: 18,
              style: GlassIconButtonStyle.plain,
              tooltip: context.l10n.commonEdit,
              onPressed: busy ? null : () => onEditSource(source),
            )
          : GlassMenuButton<String>(
              entries: _sourceEntries(context, source),
              onSelected: (action) => onSourceAction(source, action),
              builder: (context, open) => GlassIconButton(
                key: ValueKey('keepass-source-menu-${source.id}'),
                icon: Icons.more_horiz_rounded,
                size: 28,
                iconSize: 18,
                style: GlassIconButtonStyle.plain,
                tooltip: context.l10n.keepassMore,
                onPressed: busy ? null : open,
              ),
            ),
    ),
  );

  Widget _sources(BuildContext context, {bool welcome = false}) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    mainAxisSize: MainAxisSize.min,
    children: [
      if (!welcome)
        Padding(
          key: const ValueKey('keepass-databases-header'),
          padding: const EdgeInsets.fromLTRB(10, 6, 10, 6),
          child: Row(
            children: [
              Expanded(child: Text(context.l10n.keepassDatabases, style: Theme.of(context).textTheme.titleSmall)),
              _addMenu(context),
            ],
          ),
        ),
      Flexible(
        child: ListView(
          shrinkWrap: welcome,
          padding: const EdgeInsets.symmetric(vertical: 4),
          children: [for (final source in manager.sources) _sourceTile(context, source)],
        ),
      ),
    ],
  );

  bool _entryAvailable(PasswordVaultSession opened, String id) =>
      !busy &&
      manager.current(opened) &&
      manager.selected == opened.source.id &&
      !opened.busy &&
      opened.rows('entries').any((row) => row['id'] == id);

  Future<void> _entryContext(BuildContext context, KdbxData entry, Offset position) async {
    final opened = session!;
    final id = entry['id']! as String;
    if (!_entryAvailable(opened, id)) return;
    final l = context.l10n;
    final anchor = Rect.fromLTWH(position.dx, position.dy, 0, 0);
    final action = await showGlassMenu<String>(
      context: context,
      anchor: anchor,
      entries: [
        GlassMenuItem(
          key: const ValueKey('keepass-entry-open'),
          value: 'open',
          label: l.keepassSelectCard,
          icon: Icons.open_in_new_rounded,
        ),
        GlassMenuItem(
          key: const ValueKey('keepass-entry-move'),
          value: 'move',
          label: l.keepassMoveToGroup,
          icon: Icons.swap_horiz_rounded,
          enabled: opened.rows('groups').any((g) => g['id'] != entry['group']),
        ),
        const GlassMenuDivider(),
        GlassMenuItem(
          key: const ValueKey('keepass-entry-delete'),
          value: 'delete',
          label: l.commonDelete,
          icon: Icons.delete_outline_rounded,
          destructive: true,
        ),
      ],
    );
    if (!context.mounted || !_entryAvailable(opened, id)) return;
    final current = opened.rows('entries').firstWhere((row) => row['id'] == id);
    switch (action) {
      case 'open':
        onEntry(id);
      case 'delete':
        onDelete(current);
      case 'move':
        final group = await showGlassMenu<String>(
          context: context,
          anchor: anchor,
          entries: [
            for (final row in opened.rows('groups'))
              GlassMenuItem(
                key: ValueKey('keepass-entry-destination-${row['id']}'),
                value: row['id']! as String,
                label: row['id'] == opened.data['root'] ? l.keepassUngrouped : row['name']! as String,
                icon: Icons.folder_outlined,
                checked: row['id'] == current['group'],
                enabled: row['id'] != current['group'],
              ),
          ],
        );
        if (context.mounted &&
            group != null &&
            _entryAvailable(opened, id) &&
            opened.rows('groups').any((row) => row['id'] == group) &&
            opened.rows('entries').any((row) => row['id'] == id && row['group'] != group)) {
          onMove(KdbxEntryDrag(opened.source.id, id), group);
        }
    }
  }

  Widget _welcome(BuildContext context) {
    final l = context.l10n;
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 740),
        child: SingleChildScrollView(
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(Icons.password_rounded, size: 56, color: Theme.of(context).colorScheme.primary),
                const SizedBox(height: 20),
                Text(l.keepassWelcome, style: Theme.of(context).textTheme.headlineSmall, textAlign: TextAlign.center),
                const SizedBox(height: 10),
                Text(l.keepassWelcomeHelp, textAlign: TextAlign.center),
                if (AppPlatform.isMobile) ...[
                  const SizedBox(height: 12),
                  Text(
                    l.keepassMobileFilesHelp,
                    textAlign: TextAlign.center,
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ],
                const SizedBox(height: 28),
                Wrap(
                  spacing: 12,
                  runSpacing: 12,
                  alignment: WrapAlignment.center,
                  children: [
                    GlassButton(
                      key: const ValueKey('keepass-create'),
                      label: l.keepassCreate,
                      icon: Icons.add_rounded,
                      onPressed: busy ? null : onCreate,
                    ),
                    GlassButton(
                      key: const ValueKey('keepass-open'),
                      label: l.keepassOpen,
                      icon: Icons.folder_open_rounded,
                      onPressed: busy ? null : onOpen,
                    ),
                    GlassButton(
                      key: const ValueKey('keepass-webdav'),
                      label: l.keepassDavAdd,
                      icon: Icons.cloud_outlined,
                      onPressed: busy ? null : onWebDav,
                    ),
                  ],
                ),
                const SizedBox(height: 16),
                GlassButton.plain(
                  key: const ValueKey('keepass-generator-open'),
                  label: l.keepassGenerator,
                  icon: Icons.auto_fix_high_rounded,
                  onPressed: busy ? null : onGenerator,
                ),
                if (manager.sources.isNotEmpty) ...[
                  const SizedBox(height: 28),
                  const Divider(),
                  Text(l.keepassDatabases, style: Theme.of(context).textTheme.titleSmall),
                  ConstrainedBox(
                    constraints: const BoxConstraints(maxHeight: 240),
                    child: _sources(context, welcome: true),
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _folder(BuildContext context, {required String id, required String label, int depth = 0, bool all = false}) =>
      DragTarget<KdbxEntryDrag>(
        onWillAcceptWithDetails: (details) =>
            !busy &&
            details.data.source == session!.source.id &&
            session!.rows('entries').any((e) => e['id'] == details.data.entry && e['group'] != id),
        onAcceptWithDetails: (details) => onMove(details.data, id),
        builder: (context, candidates, rejected) => DecoratedBox(
          decoration: BoxDecoration(
            color: candidates.isEmpty ? null : Theme.of(context).colorScheme.primary.withValues(alpha: .18),
            borderRadius: BorderRadius.circular(8),
          ),
          child: Padding(
            padding: EdgeInsets.only(left: depth * 14.0),
            child: ListTile(
              key: ValueKey('keepass-group-${all ? 'all' : id}'),
              dense: true,
              selected: all ? group == null : group == id,
              selectedColor: Theme.of(context).colorScheme.onSurface,
              selectedTileColor: Theme.of(context).colorScheme.primary.withValues(alpha: .14),
              leading: Icon(all ? Icons.folder_copy_outlined : Icons.folder_outlined, size: 18),
              title: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis),
              onTap: () => onGroup(all ? null : id),
            ),
          ),
        ),
      );

  Widget _sidebar(BuildContext context) {
    final l = context.l10n;
    final groups = session!.rows('groups');
    int depth(KdbxData row) {
      var n = 0;
      var parent = row['parent'];
      final seen = <Object?>{};
      while (parent != null && seen.add(parent)) {
        final ancestor = groups.where((g) => g['id'] == parent).firstOrNull;
        if (ancestor == null) break;
        n++;
        parent = ancestor['parent'];
      }
      return n;
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.all(12),
          child: Text(l.keepassGroups, style: Theme.of(context).textTheme.titleSmall),
        ),
        Expanded(
          child: ListView(
            padding: EdgeInsets.zero,
            children: [
              _folder(context, id: session!.data['root']! as String, label: l.keepassAllGroups, all: true),
              for (final row in groups)
                _folder(
                  context,
                  id: row['id']! as String,
                  label: row['id'] == session!.data['root'] ? l.keepassUngrouped : row['label']! as String,
                  depth: depth(row),
                ),
            ],
          ),
        ),
        const Divider(height: 1),
        SizedBox(key: const ValueKey('keepass-databases-bottom'), height: 180, child: _sources(context)),
      ],
    );
  }

  Widget _entryList(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      Padding(
        padding: const EdgeInsets.all(12),
        child: GlassField(
          fieldKey: const ValueKey('keepass-search'),
          controller: search,
          size: GlassFieldSize.lg,
          search: true,
          placeholder: context.l10n.keepassSearch,
          leadingIcon: Icons.search_rounded,
          onChanged: onSearch,
        ),
      ),
      Expanded(
        child: entries.isEmpty
            ? Center(child: Text(context.l10n.keepassEmpty))
            : ListView.separated(
                padding: const EdgeInsets.symmetric(horizontal: 8),
                itemCount: entries.length,
                separatorBuilder: (_, _) => const Divider(height: 1),
                itemBuilder: (context, index) {
                  final entry = entries[index];
                  final title = (entry['title']! as String).isEmpty
                      ? context.l10n.keepassEntry
                      : entry['title']! as String;
                  final subtitle = [
                    entry['username'],
                    entry['url'],
                  ].whereType<String>().where((v) => v.isNotEmpty).join(' · ');
                  return Draggable<KdbxEntryDrag>(
                    data: KdbxEntryDrag(session!.source.id, entry['id']! as String),
                    maxSimultaneousDrags: busy || AppPlatform.isMobile ? 0 : 1,
                    feedback: Material(
                      color: Colors.transparent,
                      child: Container(
                        width: 260,
                        padding: const EdgeInsets.all(14),
                        decoration: BoxDecoration(
                          color: Theme.of(context).colorScheme.surface,
                          borderRadius: BorderRadius.circular(12),
                          boxShadow: const [BoxShadow(blurRadius: 12, color: Colors.black26)],
                        ),
                        child: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
                      ),
                    ),
                    child: GestureDetector(
                      onSecondaryTapDown: busy
                          ? null
                          : (details) => unawaited(_entryContext(context, entry, details.globalPosition)),
                      onLongPressStart: busy || !AppPlatform.isMobile
                          ? null
                          : (details) => unawaited(_entryContext(context, entry, details.globalPosition)),
                      child: ListTile(
                        key: ValueKey('keepass-entry-${entry['id']}'),
                        minTileHeight: 84,
                        minVerticalPadding: 16,
                        selected: selected?['id'] == entry['id'],
                        selectedColor: Theme.of(context).colorScheme.onSurface,
                        selectedTileColor: Theme.of(context).colorScheme.primary.withValues(alpha: .14),
                        leading: const Icon(Icons.key_outlined, size: 22),
                        title: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
                        subtitle: subtitle.isEmpty
                            ? null
                            : Text(subtitle, maxLines: 1, overflow: TextOverflow.ellipsis),
                        trailing: AppPlatform.isMobile
                            ? Builder(
                                builder: (context) => GlassIconButton(
                                  key: ValueKey('keepass-entry-menu-${entry['id']}'),
                                  icon: Icons.more_horiz_rounded,
                                  tooltip: context.l10n.keepassMore,
                                  style: GlassIconButtonStyle.plain,
                                  onPressed: busy
                                      ? null
                                      : () => unawaited(
                                          _entryContext(context, entry, glassAnchorRect(context).bottomLeft),
                                        ),
                                ),
                              )
                            : const Icon(Icons.chevron_right_rounded, size: 18),
                        onTap: () => onEntry(entry['id']! as String),
                      ),
                    ),
                  );
                },
              ),
      ),
    ],
  );

  Widget _details(BuildContext context, {bool compact = false}) {
    final l = context.l10n;
    final entry = selected;
    if (entry == null) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Text(l.keepassNoSelection, textAlign: TextAlign.center),
        ),
      );
    }
    Widget field(String key, String label, {String? value}) => Container(
      key: ValueKey('keepass-detail-$key'),
      margin: const EdgeInsets.only(bottom: 6),
      padding: const EdgeInsets.fromLTRB(12, 8, 8, 8),
      constraints: const BoxConstraints(minHeight: 60),
      decoration: BoxDecoration(
        border: Border.all(color: Theme.of(context).dividerColor),
        borderRadius: BorderRadius.circular(12),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(label, style: Theme.of(context).textTheme.labelMedium),
                const SizedBox(height: 4),
                SelectableText(
                  value ?? '••••••••',
                  style: Theme.of(context).textTheme.bodyLarge,
                ), // l10n-ignore: secret mask.
              ],
            ),
          ),
          GlassIconButton(
            key: ValueKey('keepass-detail-copy-$key'),
            icon: Icons.copy_outlined,
            size: 28,
            iconSize: 18,
            style: GlassIconButtonStyle.plain,
            tooltip: '${l.keepassCopyField}: $label',
            onPressed: busy ? null : () => onCopy(entry, key),
          ),
        ],
      ),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.all(12),
          child: Row(
            children: [
              if (compact)
                GlassIconButton(
                  key: const ValueKey('keepass-back-entries'),
                  icon: Icons.arrow_back_rounded,
                  tooltip: l.keepassBackEntries,
                  onPressed: () => onEntry(null),
                ),
              Expanded(
                child: Text(
                  entry['title']! as String,
                  style: Theme.of(context).textTheme.titleLarge,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
        ),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  session!.rows('groups').where((g) => g['id'] == entry['group']).firstOrNull?['name'] as String? ?? '',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                const SizedBox(height: 12),
                field('UserName', l.keepassUsername, value: entry['username']! as String),
                field('Password', l.keepassEntryPassword),
                if ((entry['url']! as String).isNotEmpty) field('URL', l.keepassUrl, value: entry['url']! as String),
                field('Notes', l.keepassNotes, value: entry['notes'] as String? ?? ''),
                for (final key in (entry['fields'] as List<Object?>? ?? []).cast<String>().where(
                  (k) => !['title', 'username', 'password', 'url', 'notes'].contains(k.toLowerCase()),
                ))
                  field(key, key == 'Notes' ? l.keepassNotes : key),
              ],
            ),
          ),
        ),
        const Divider(height: 1),
        Padding(
          padding: const EdgeInsets.all(12),
          child: Row(
            children: [
              GlassButton(
                key: const ValueKey('keepass-edit-entry'),
                label: l.commonEdit,
                icon: Icons.edit_outlined,
                onPressed: busy ? null : () => onEdit(entry),
              ),
              const Spacer(),
              GlassIconButton(
                icon: Icons.delete_outline_rounded,
                tooltip: l.commonDelete,
                onPressed: busy ? null : () => onDelete(entry),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _compactNavigation(BuildContext context) {
    final l = context.l10n;
    return Padding(
      key: const ValueKey('keepass-compact-navigation'),
      padding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
      child: Row(
        children: [
          Expanded(
            child: GlassMenuButton<String>(
              entries: [
                for (final source in manager.sources)
                  GlassMenuItem(
                    key: ValueKey('keepass-compact-source-${source.id}'),
                    value: source.id,
                    label: source.name,
                    checked: source.id == session!.source.id,
                    icon: manager.sessions.containsKey(source.id)
                        ? Icons.lock_open_rounded
                        : Icons.lock_outline_rounded,
                  ),
              ],
              onSelected: (id) {
                final source = manager.sources.where((s) => s.id == id).firstOrNull;
                if (source != null) onSource(source);
              },
              builder: (context, open) => GlassButton(
                key: const ValueKey('keepass-compact-sources'),
                label: session!.source.name,
                icon: Icons.lock_open_rounded,
                expand: true,
                onPressed: busy ? null : open,
              ),
            ),
          ),
          const SizedBox(width: 6),
          GlassMenuButton<String>(
            entries: [
              GlassMenuItem(
                value: 'all',
                label: l.keepassAllGroups,
                checked: group == null,
                icon: Icons.folder_copy_outlined,
              ),
              for (final row in session!.rows('groups'))
                GlassMenuItem(
                  key: ValueKey('keepass-compact-group-${row['id']}'),
                  value: row['id']! as String,
                  label: row['id'] == session!.data['root'] ? l.keepassUngrouped : row['name']! as String,
                  checked: group == row['id'],
                  icon: Icons.folder_outlined,
                ),
            ],
            onSelected: (id) => onGroup(id == 'all' ? null : id),
            builder: (context, open) => GlassIconButton(
              key: const ValueKey('keepass-compact-groups'),
              icon: Icons.folder_outlined,
              tooltip: l.keepassGroups,
              style: GlassIconButtonStyle.plain,
              onPressed: busy ? null : open,
            ),
          ),
          _addMenu(context),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final compact = MediaQuery.sizeOf(context).width < 700;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (busy)
          Row(
            children: [
              const Expanded(child: LinearProgressIndicator(minHeight: 2)),
              if (manager.opening) GlassButton.plain(label: l.commonCancel, onPressed: manager.cancelOpening),
            ],
          ),
        if (error != null)
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: InfoBanner(tone: BannerTone.danger, message: error!),
          ),
        if (session != null)
          Padding(
            padding: const EdgeInsets.only(bottom: 12),
            child: Wrap(
              spacing: 12,
              runSpacing: 10,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 280),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        session!.source.name,
                        style: Theme.of(context).textTheme.titleMedium,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      Text(
                        session!.busy
                            ? l.keepassAutosaving
                            : session!.data['dirty'] == true
                            ? l.keepassDirty
                            : l.keepassAutosaved,
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    ],
                  ),
                ),
                if (compact)
                  GlassIconButton(
                    key: const ValueKey('keepass-new-entry'),
                    tooltip: l.keepassNewEntry,
                    icon: Icons.add_rounded,
                    onPressed: busy ? null : onNewEntry,
                  )
                else
                  GlassButton(
                    key: const ValueKey('keepass-new-entry'),
                    label: l.keepassNewEntry,
                    icon: Icons.add_rounded,
                    onPressed: busy ? null : onNewEntry,
                  ),
                if (compact)
                  GlassIconButton(
                    key: const ValueKey('keepass-new-group'),
                    tooltip: l.keepassNewGroup,
                    icon: Icons.create_new_folder_outlined,
                    onPressed: busy ? null : onNewGroup,
                  )
                else
                  GlassButton(
                    key: const ValueKey('keepass-new-group'),
                    label: l.keepassNewGroup,
                    icon: Icons.create_new_folder_outlined,
                    onPressed: busy ? null : onNewGroup,
                  ),
                if (session!.data['dirty'] == true && !busy)
                  GlassButton(
                    key: const ValueKey('keepass-save'),
                    label: l.keepassRetrySave,
                    icon: Icons.refresh_rounded,
                    onPressed: onRetrySave,
                  ),
                if (compact)
                  GlassIconButton(
                    key: const ValueKey('keepass-generator-open'),
                    tooltip: l.keepassGenerator,
                    icon: Icons.auto_fix_high_rounded,
                    onPressed: busy ? null : onGenerator,
                  )
                else
                  GlassButton(
                    key: const ValueKey('keepass-generator-open'),
                    label: l.keepassGenerator,
                    icon: Icons.auto_fix_high_rounded,
                    onPressed: busy ? null : onGenerator,
                  ),
                GlassMenuButton<String>(
                  entries: _sourceEntries(context, session!.source),
                  onSelected: (action) => onSourceAction(session!.source, action),
                  builder: (context, open) => GlassIconButton(
                    key: const ValueKey('keepass-database-actions'),
                    icon: Icons.more_vert_rounded,
                    tooltip: l.keepassMore,
                    onPressed: busy ? null : open,
                  ),
                ),
              ],
            ),
          ),
        Expanded(
          child: session == null
              ? _welcome(context)
              : LayoutBuilder(
                  builder: (context, constraints) {
                    if (constraints.maxWidth < 700) {
                      return Column(
                        children: [
                          _compactNavigation(context),
                          Expanded(child: selected == null ? _entryList(context) : _details(context, compact: true)),
                        ],
                      );
                    }
                    final wide = constraints.maxWidth >= 1050;
                    final sideLimit = wide
                        ? (constraints.maxWidth - 580).clamp(170.0, 420.0)
                        : (constraints.maxWidth - 250).clamp(170.0, 300.0);
                    final side = sidebarWidth.clamp(170.0, sideLimit);
                    final detail = detailsWidth.clamp(300.0, (constraints.maxWidth - side - 280).clamp(300.0, 720.0));
                    return Row(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        SizedBox(width: side, child: _sidebar(context)),
                        _splitter(
                          context,
                          'keepass-sidebar-resize',
                          l.keepassResizeGroups,
                          (delta) => onSidebarResize(delta, 170, sideLimit),
                        ),
                        Expanded(
                          child: !wide && selected != null ? _details(context, compact: true) : _entryList(context),
                        ),
                        if (wide) ...[
                          _splitter(
                            context,
                            'keepass-details-resize',
                            l.keepassResizeCard,
                            (delta) =>
                                onDetailsResize(delta, 300, (constraints.maxWidth - side - 280).clamp(300.0, 720.0)),
                          ),
                          SizedBox(
                            key: const ValueKey('keepass-details-pane'),
                            width: detail,
                            child: _details(context),
                          ),
                        ],
                      ],
                    );
                  },
                ),
        ),
      ],
    );
  }
}
