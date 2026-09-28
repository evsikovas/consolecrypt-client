import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/risk_badge.dart';
import 'package:consolecrypt/snippets/run_flow.dart';
import 'package:consolecrypt/snippets/snippet_editor.dart';
import 'package:consolecrypt/snippets/starter_catalog_dialog.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

enum _SnippetAction { edit, move, runMany, delete }

class SnippetsScreen extends ConsumerStatefulWidget {
  const SnippetsScreen({super.key, this.embedded = false});
  final bool embedded;

  @override
  ConsumerState<SnippetsScreen> createState() => _SnippetsScreenState();
}

class _SnippetsScreenState extends ConsumerState<SnippetsScreen> {
  String _query = '';
  SnippetType? _type;
  String? _package;
  final Set<String> _tags = {};
  List<SnippetSearchHit>? _hits;

  Future<void> _search(String q) async {
    setState(() => _query = q);
    if (q.trim().isEmpty) {
      setState(() => _hits = null);
      return;
    }
    try {
      final hits = await ref.read(snippetServiceProvider).search(q, limit: 100);
      if (mounted && q == _query) setState(() => _hits = hits);
    } on AppException catch (e) {
      if (mounted && q == _query) showSnack(context, errorMessage(context.l10n, e), error: true);
    }
  }

  List<GlassMenuEntry<_SnippetAction>> _entries(Snippet s) {
    final l = context.l10n;
    return [
      GlassMenuItem(value: _SnippetAction.runMany, label: l.snippetRunMany, icon: Icons.playlist_play_rounded),
      GlassMenuItem(value: _SnippetAction.edit, label: l.commonEdit, icon: Icons.edit_rounded),
      GlassMenuItem(value: _SnippetAction.move, label: l.snippetMovePackage, icon: Icons.folder_open_rounded),
      const GlassMenuDivider(),
      GlassMenuItem(value: _SnippetAction.delete, label: l.commonDelete, icon: Icons.delete_rounded, destructive: true),
    ];
  }

  Future<void> _action(_SnippetAction action, Snippet s) async {
    switch (action) {
      case _SnippetAction.edit:
        await showSnippetEditor(context, snippet: s);
      case _SnippetAction.delete:
        await _delete(s);
      case _SnippetAction.runMany:
        await runSnippetInManyFlow(context, ref, s);
      case _SnippetAction.move:
        final profile = ref.read(activeProfileProvider)?.id;
        final name = await showTextInputDialog(
          context,
          title: context.l10n.snippetMovePackage,
          initial: s.packageName ?? '',
          label: context.l10n.snippetPackage,
        );
        if (name == null || !mounted || profile != ref.read(activeProfileProvider)?.id) return;
        final current = ref.read(snippetsProvider).value?.where((item) => item.id == s.id).firstOrNull;
        if (current == null) return;
        await runWithFeedback(
          context,
          () => ref
              .read(snippetServiceProvider)
              .saveSnippet(
                current.copyWith(
                  packageName: name.trim(),
                  clearPackage: name.trim().isEmpty,
                  updatedAt: DateTime.now().toUtc(),
                ),
              ),
        );
    }
  }

  Future<void> _editPackage({required bool delete}) async {
    final name = _package;
    if (name == null || name.isEmpty) return;
    final profile = ref.read(activeProfileProvider)?.id;
    final l = context.l10n;
    final members = ref.read(snippetsProvider).value?.where((s) => s.packageName == name).toList() ?? [];
    String? newName;
    if (delete) {
      if (!await showConfirmDialog(
        context,
        title: l.snippetPackageDelete,
        message: l.snippetPackageDeleteMessage(members.length),
        confirmLabel: l.commonDelete,
        destructive: true,
      )) {
        return;
      }
    } else {
      newName = await showTextInputDialog(
        context,
        title: l.snippetPackageRename,
        initial: name,
        label: l.snippetPackage,
      );
      if (newName == null) return;
    }
    if (!mounted || profile != ref.read(activeProfileProvider)?.id) return;
    final service = ref.read(snippetServiceProvider);
    await runWithFeedback(context, () async {
      for (final member in members) {
        if (!mounted || profile != ref.read(activeProfileProvider)?.id) return;
        final current = ref.read(snippetsProvider).value?.where((s) => s.id == member.id).firstOrNull;
        if (current == null || current.packageName != name) continue;
        if (delete) {
          await service.deleteSnippet(current.id);
        } else {
          await service.saveSnippet(
            current.copyWith(
              packageName: newName!.trim(),
              clearPackage: newName.trim().isEmpty,
              updatedAt: DateTime.now().toUtc(),
            ),
          );
        }
      }
    });
    if (mounted) setState(() => _package = delete ? null : newName?.trim());
  }

  Future<void> _delete(Snippet s) async {
    final l10n = context.l10n;
    final ok = await showConfirmDialog(
      context,
      title: l10n.snippetsDeleteTitle(s.name),
      message: l10n.snippetsDeleteMessage,
      confirmLabel: l10n.commonDelete,
      destructive: true,
    );
    if (ok && mounted) await runWithFeedback(context, () => ref.read(snippetServiceProvider).deleteSnippet(s.id));
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final all = ref.watch(snippetsProvider).value ?? const <Snippet>[];
    // Keep search results fresh when snippets change.
    ref.listen(snippetsProvider, (_, _) {
      if (_query.isNotEmpty) _search(_query);
    });
    final packages = {
      for (final s in all)
        if (s.packageName != null) s.packageName!,
    }.toList()..sort();
    if (_package != null && _package!.isNotEmpty && !packages.contains(_package)) _package = null;
    final allTags = {
      for (final s in all)
        if (_package == null || (s.packageName ?? '') == _package) ...s.tags,
    }.toList()..sort();
    final base =
        _hits?.map((h) => (h.snippet, h.matchKind)).toList() ??
        ([...all]..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase())))
            .map((s) => (s, SearchMatchKind.text))
            .toList();
    final visible = base
        .where((e) => _type == null || e.$1.snippetType == _type)
        .where((e) => _package == null || (e.$1.packageName ?? '') == _package)
        .where((e) => _tags.every(e.$1.tags.contains))
        .toList();
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    return PageScaffold(
      embedded: widget.embedded,
      title: l10n.navSnippets,
      subtitle: l10n.snippetSyncHint,
      actions: [
        GlassButton(
          key: const ValueKey('add-snippet'),
          onPressed: () => showSnippetEditor(context, packageName: _package),
          icon: Icons.add_rounded,
          label: l10n.snippetsNewButton,
        ),
        GlassButton(
          key: const ValueKey('snippet-starters'),
          onPressed: () => showStarterCatalog(context),
          icon: Icons.inventory_2_outlined,
          label: l10n.snippetStarterCatalog,
        ),
      ],
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: GlassSelect<String?>(
                  key: const ValueKey('snippet-package-filter'),
                  expand: true,
                  value: _package,
                  semanticLabel: l10n.snippetPackage,
                  items: [
                    GlassSelectItem(value: null, label: l10n.snippetAllPackages, icon: Icons.code_rounded),
                    GlassSelectItem(value: '', label: l10n.snippetNoPackage, icon: Icons.folder_open_rounded),
                    for (final name in packages)
                      GlassSelectItem(
                        value: name,
                        label: '$name (${all.where((s) => s.packageName == name).length})',
                        icon: Icons.folder_outlined,
                      ),
                  ],
                  onChanged: (value) => setState(() {
                    _package = value;
                    _tags.clear();
                  }),
                ),
              ),
              if (_package != null && _package!.isNotEmpty)
                GlassMenuButton<bool>(
                  entries: [
                    GlassMenuItem(value: false, label: l10n.snippetPackageRename, icon: Icons.edit_rounded),
                    GlassMenuItem(
                      value: true,
                      label: l10n.snippetPackageDelete,
                      icon: Icons.delete_rounded,
                      destructive: true,
                    ),
                  ],
                  onSelected: (delete) => _editPackage(delete: delete),
                  builder: (context, open) => GlassIconButton(
                    key: const ValueKey('snippet-package-menu'),
                    icon: Icons.more_horiz_rounded,
                    tooltip: l10n.hostsMoreTooltip,
                    onPressed: open,
                  ),
                ),
            ],
          ),
          const SizedBox(height: GlassSpacing.s8),
          Flex(
            direction: widget.embedded ? Axis.vertical : Axis.horizontal,
            crossAxisAlignment: widget.embedded ? CrossAxisAlignment.stretch : CrossAxisAlignment.center,
            mainAxisSize: MainAxisSize.min,
            children: [
              Flexible(
                flex: widget.embedded ? 0 : 1,
                child: GlassField(
                  key: const ValueKey('snippet-search'),
                  search: true,
                  size: GlassFieldSize.lg,
                  leadingIcon: Icons.search_rounded,
                  placeholder: l10n.snippetsSearchHint,
                  onChanged: _search,
                ),
              ),
              SizedBox(width: widget.embedded ? 0 : 12, height: widget.embedded ? 8 : 0),
              GlassSelect<SnippetType?>(
                key: const ValueKey('snippet-type'),
                value: _type,
                semanticLabel: l10n.snippetsTypeLabel,
                items: [
                  GlassSelectItem(value: null, label: l10n.snippetsAllTypes),
                  for (final type in SnippetType.values) GlassSelectItem(value: type, label: type.localized(l10n)),
                ],
                onChanged: (v) => setState(() => _type = v),
              ),
            ],
          ),
          if (allTags.isNotEmpty) ...[
            const SizedBox(height: GlassSpacing.s8),
            Wrap(
              spacing: GlassSpacing.s6,
              runSpacing: GlassSpacing.s6,
              children: [
                for (final tag in allTags)
                  FilterChip(
                    label: Text(tag),
                    selected: _tags.contains(tag),
                    onSelected: (v) => setState(() => v ? _tags.add(tag) : _tags.remove(tag)),
                  ),
              ],
            ),
          ],
          const SizedBox(height: GlassSpacing.s12),
          Expanded(
            child: visible.isEmpty
                ? EmptyState(
                    icon: Icons.code_rounded,
                    title: all.isEmpty ? l10n.snippetsEmptyTitle : l10n.snippetsNothingFound,
                    message: all.isEmpty ? l10n.snippetsEmptyMessage : null,
                  )
                : ContentList(
                    itemCount: visible.length,
                    itemBuilder: (context, i) {
                      final (s, kind) = visible[i];
                      final title = Row(
                        children: [
                          Flexible(
                            child: Text(
                              s.name,
                              overflow: TextOverflow.ellipsis,
                              style: t.bodyEmph.copyWith(color: tokens.palette.label),
                            ),
                          ),
                          const SizedBox(width: GlassSpacing.s8),
                          RiskBadge(risk: s.riskLevel, dense: true),
                          if (s.source == SnippetSource.ai) ...[
                            const SizedBox(width: GlassSpacing.s6),
                            Tooltip(
                              message: l10n.snippetsDraftedByAi,
                              child: Icon(Icons.auto_awesome_rounded, size: 14, color: tokens.secondaryLabel),
                            ),
                          ],
                          if (kind == SearchMatchKind.semantic) ...[
                            const SizedBox(width: GlassSpacing.s6),
                            Tooltip(
                              message: l10n.snippetsSemanticMatch,
                              child: Icon(Icons.psychology_rounded, size: 14, color: tokens.secondaryLabel),
                            ),
                          ],
                        ],
                      );
                      final subtitle = Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            s.template,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: t.mono.copyWith(fontSize: 12, color: tokens.secondaryLabel),
                          ),
                          const SizedBox(height: GlassSpacing.s4),
                          if (_package == null && s.packageName != null)
                            Text(s.packageName!, style: t.caption.copyWith(color: tokens.secondaryLabel)),
                          if (widget.embedded)
                            Wrap(
                              spacing: 8,
                              runSpacing: 4,
                              crossAxisAlignment: WrapCrossAlignment.center,
                              children: [
                                Text(
                                  s.snippetType.localized(l10n),
                                  style: t.caption.copyWith(color: tokens.secondaryLabel),
                                ),
                                if (s.usageCount > 0)
                                  Text(
                                    l10n.snippetsUsedCount(s.usageCount),
                                    style: t.caption.copyWith(color: tokens.secondaryLabel),
                                  ),
                                TagChips(tags: s.tags),
                              ],
                            )
                          else
                            Row(
                              children: [
                                Text(
                                  s.snippetType.localized(l10n),
                                  style: t.caption.copyWith(color: tokens.secondaryLabel),
                                ),
                                const SizedBox(width: GlassSpacing.s8),
                                Expanded(child: TagChips(tags: s.tags)),
                                if (s.usageCount > 0)
                                  Text(
                                    l10n.snippetsUsedCount(s.usageCount),
                                    style: t.caption.copyWith(color: tokens.secondaryLabel),
                                  ),
                              ],
                            ),
                        ],
                      );
                      final actionButtons = <Widget>[
                        GlassButton(
                          key: ValueKey('paste-${s.name}'),
                          label: l10n.snippetPaste,
                          icon: Icons.keyboard_return_rounded,
                          onPressed: () => insertSnippetFlow(context, ref, s),
                        ),
                        const SizedBox(width: GlassSpacing.s4),
                        GlassButton(
                          key: ValueKey('run-${s.name}'),
                          onPressed: () => runSnippetFlow(context, ref, s),
                          icon: Icons.play_arrow_rounded,
                          label: l10n.snippetsRun,
                        ),
                        const SizedBox(width: GlassSpacing.s4),
                        GlassMenuButton<_SnippetAction>(
                          entries: _entries(s),
                          onSelected: (action) => _action(action, s),
                          builder: (context, open) => GlassIconButton(
                            key: ValueKey('snippet-menu-${s.name}'),
                            tooltip: l10n.hostsMoreTooltip,
                            icon: Icons.more_horiz_rounded,
                            style: GlassIconButtonStyle.plain,
                            onPressed: open,
                          ),
                        ),
                      ];
                      final actions = widget.embedded
                          ? Wrap(
                              alignment: WrapAlignment.end,
                              crossAxisAlignment: WrapCrossAlignment.center,
                              spacing: 4,
                              runSpacing: 4,
                              children: actionButtons,
                            )
                          : Row(mainAxisSize: MainAxisSize.min, children: actionButtons);
                      if (widget.embedded) {
                        return Padding(
                          key: ValueKey('snippet-${s.name}'),
                          padding: const EdgeInsets.all(10),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.stretch,
                            children: [
                              InkWell(
                                onTap: () => showSnippetEditor(context, snippet: s),
                                onDoubleTap: () => runSnippetFlow(context, ref, s),
                                onSecondaryTapDown: (details) async {
                                  final action = await showGlassMenu(
                                    context: context,
                                    anchor: Rect.fromLTWH(details.globalPosition.dx, details.globalPosition.dy, 1, 1),
                                    entries: _entries(s),
                                  );
                                  if (action != null && mounted) await _action(action, s);
                                },
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.stretch,
                                  children: [title, const SizedBox(height: 6), subtitle],
                                ),
                              ),
                              const SizedBox(height: 10),
                              Align(alignment: Alignment.centerRight, child: actions),
                            ],
                          ),
                        );
                      }
                      return ListTile(
                        key: ValueKey('snippet-${s.name}'),
                        title: title,
                        subtitle: subtitle,
                        onTap: () => showSnippetEditor(context, snippet: s),
                        trailing: actions,
                      );
                    },
                  ),
          ),
        ],
      ),
    );
  }
}
