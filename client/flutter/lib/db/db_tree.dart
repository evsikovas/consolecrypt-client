import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

IconData _kindIcon(DbTreeNode node) => switch (node.reference.kind) {
  DbObjectKind.database => Icons.storage_rounded,
  DbObjectKind.redisDatabase => node.reference.parentName == null ? Icons.storage_rounded : Icons.folder_outlined,
  DbObjectKind.schema => Icons.account_tree_outlined,
  DbObjectKind.table || DbObjectKind.collection => Icons.table_chart_outlined,
  DbObjectKind.view || DbObjectKind.materializedView => Icons.view_quilt_outlined,
  DbObjectKind.sequence => Icons.format_list_numbered,
  DbObjectKind.function || DbObjectKind.procedure => Icons.functions,
  DbObjectKind.column =>
    node.primaryKey ? Icons.key : (node.foreignKey != null ? Icons.link : Icons.view_column_outlined),
  DbObjectKind.indexObject => Icons.bolt_outlined,
  DbObjectKind.constraint => Icons.rule,
  DbObjectKind.trigger => Icons.flash_on_outlined,
  DbObjectKind.dictionary => Icons.menu_book_outlined,
  DbObjectKind.projection => Icons.layers_outlined,
  DbObjectKind.redisKey => Icons.vpn_key_outlined,
};

/// Objects whose creation statement or description can be shown.
const _describable = {
  DbObjectKind.table,
  DbObjectKind.view,
  DbObjectKind.materializedView,
  DbObjectKind.sequence,
  DbObjectKind.function,
  DbObjectKind.procedure,
  DbObjectKind.dictionary,
  DbObjectKind.collection,
  DbObjectKind.redisKey,
};

final class _Branch {
  final nodes = <DbTreeNode>[];
  final cursors = <String>{};
  String? cursor;
  bool loading = false, failed = false;
  DateTime? loadedAt;
  int generation = 0;
}

class DbObjectTree extends ConsumerStatefulWidget {
  const DbObjectTree({
    required this.session,
    required this.service,
    required this.onDraft,
    required this.onInsert,
    this.onSelect,
    super.key,
  });
  final DbSessionInfo session;
  final DbQueryService service;
  final ValueChanged<String> onDraft, onInsert;
  final ValueChanged<DbObjectRef>? onSelect;
  @override
  ConsumerState<DbObjectTree> createState() => _DbObjectTreeState();
}

class _DbObjectTreeState extends ConsumerState<DbObjectTree> {
  late final DbScope _scope = ref.read(dbScopeProvider);
  final _branches = <String, _Branch>{}, _expanded = <String>{};
  String _filter = '';
  String? _selected;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));
  @override
  void initState() {
    super.initState();
    unawaited(_load(null));
  }

  Future<void> _load(DbObjectRef? parent, {bool more = false, bool refresh = false}) async {
    if (!_current) return;
    final key = parent?.id ?? '';
    final branch = _branches.putIfAbsent(key, _Branch.new);
    if (branch.loading) return;
    if (!more && !refresh && branch.loadedAt != null && DateTime.now().difference(branch.loadedAt!).inSeconds < 30) {
      return;
    }
    final cursor = more ? branch.cursor : null;
    if (more && cursor == null) return;
    final generation = ++branch.generation;
    setState(() {
      branch.loading = true;
      branch.failed = false;
    });
    try {
      final page = await widget.service
          .tree(widget.session.id, parent: parent, cursor: cursor, refresh: refresh)
          .timeout(const Duration(seconds: 30));
      if (!_current || generation != branch.generation) return;
      if (page.nodes.length > 2000 || (more && page.nextCursor != null && branch.cursors.contains(page.nextCursor))) {
        throw const FormatException('Invalid database tree page');
      }
      setState(() {
        if (!more) {
          branch.nodes.clear();
          branch.cursors.clear();
        }
        final ids = branch.nodes.map((n) => n.reference.id).toSet();
        for (final node in page.nodes) {
          if (branch.nodes.length >= 10000) break;
          if (ids.add(node.reference.id)) branch.nodes.add(node);
        }
        if (cursor != null) branch.cursors.add(cursor);
        branch.cursor = branch.nodes.length < 10000 ? page.nextCursor : null;
        branch.loadedAt = DateTime.now();
      });
    } catch (_) {
      if (_current && generation == branch.generation) setState(() => branch.failed = true);
    } finally {
      if (_current && generation == branch.generation) setState(() => branch.loading = false);
    }
  }

  Future<void> _action(String action, DbObjectRef object) async {
    if (!_current) return;
    SecretText? value;
    try {
      switch (action) {
        case 'insert':
          final identifier = await widget.service.identifier(widget.session.id, object);
          if (_current) widget.onInsert(identifier);
          return;
        case 'query':
          final text = await widget.service.objectQuery(widget.session.id, object);
          if (_current) widget.onDraft(text);
          return;
        case 'ddl':
          value = await widget.service.ddl(widget.session.id, object);
        case 'inspect':
        case 'sample':
          final service = widget.service;
          if (service is! DbObjectInspectService) return;
          if (action == 'sample') {
            final approved = await showAppDialog<bool>(
              context,
              secure: true,
              builder: (context) => GlassDialog(
                title: context.l10n.dbSampleSchema,
                content: Text(context.l10n.dbSampleHelp),
                secondaryActions: [
                  GlassButton(label: context.l10n.commonCancel, onPressed: () => closeDialog<bool>(context, false)),
                ],
                primaryAction: GlassButton(
                  label: context.l10n.commonContinue,
                  onPressed: () => closeDialog<bool>(context, true),
                ),
              ),
            );
            if (!_current || approved != true) return;
          }
          final document = await (service as DbObjectInspectService).inspectObject(
            widget.session.id,
            object,
            sampleDocuments: action == 'sample' ? 20 : null,
          );
          value = SecretText(const JsonEncoder.withIndent('  ').convert(document));
      }
      if (!mounted || !_current || value == null) return;
      await showAppDialog<void>(
        context,
        secure: true,
        builder: (_) => DbProtectedTextDialog(title: object.name, value: value!),
      );
    } catch (_) {
      if (mounted && _current) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(context.l10n.dbOperationFailed)));
      }
    } finally {
      value?.wipe();
    }
  }

  List<(DbTreeNode?, DbObjectRef?, int)> _visible() {
    final rows = <(DbTreeNode?, DbObjectRef?, int)>[];
    void walk(DbObjectRef? parent, int depth) {
      final branch = _branches[parent?.id ?? ''];
      if (branch == null || depth > 12) return;
      for (final node in branch.nodes) {
        final matches = _filter.isEmpty || node.reference.name.toLowerCase().contains(_filter);
        // Keep expanded ancestors visible so search never hides navigation.
        if (matches || _expanded.contains(node.reference.id)) rows.add((node, parent, depth));
        if (_expanded.contains(node.reference.id)) walk(node.reference, depth + 1);
      }
      if (branch.loading || branch.failed || branch.cursor != null) rows.add((null, parent, depth));
    }

    walk(null, 0);
    return rows;
  }

  /// Actions of one object, offered from the row menu and right click.
  List<GlassMenuEntry<String>> _menu(AppLocalizations l, DbObjectRef object) => [
    if (const [
      DbObjectKind.table,
      DbObjectKind.view,
      DbObjectKind.materializedView,
      DbObjectKind.collection,
      DbObjectKind.redisKey,
      DbObjectKind.redisDatabase,
    ].contains(object.kind))
      GlassMenuItem(value: 'query', label: l.dbFirstRows, icon: Icons.table_rows_outlined),
    GlassMenuItem(value: 'insert', label: l.dbInsertName, icon: Icons.input_rounded),
    if (_describable.contains(object.kind)) GlassMenuItem(value: 'ddl', label: l.dbDdl, icon: Icons.code_rounded),
    if (widget.service is DbObjectInspectService) ...[
      GlassMenuItem(value: 'inspect', label: l.dbInspect, icon: Icons.info_outline_rounded),
      if (object.kind == DbObjectKind.collection)
        GlassMenuItem(value: 'sample', label: l.dbSampleSchema, icon: Icons.schema_outlined),
    ],
  ];

  Future<void> _menuAt(DbObjectRef object, Rect anchor) async {
    final value = await showGlassMenu<String>(context: context, anchor: anchor, entries: _menu(context.l10n, object));
    if (value != null && _current) await _action(value, object);
  }

  void _toggle(DbObjectRef object) {
    final expanded = _expanded.contains(object.id);
    setState(() => expanded ? _expanded.remove(object.id) : _expanded.add(object.id));
    if (!expanded) unawaited(_load(object));
  }

  @override
  Widget build(BuildContext context) {
    ref.watch(dbScopeProvider);
    if (!_current) return const SizedBox.shrink();
    final l = context.l10n, tokens = GlassTokens.of(context), p = tokens.palette, rows = _visible();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(GlassSpacing.s8, GlassSpacing.s8, GlassSpacing.s4, GlassSpacing.s6),
          child: Row(
            children: [
              Expanded(
                child: GlassField(
                  key: const ValueKey('db-tree-search'),
                  search: true,
                  placeholder: l.dbTreeSearch,
                  onChanged: (v) => setState(() => _filter = v.toLowerCase()),
                ),
              ),
              GlassIconButton(
                key: const ValueKey('db-tree-refresh'),
                icon: Icons.refresh_rounded,
                tooltip: l.commonRetry,
                style: GlassIconButtonStyle.plain,
                onPressed: () {
                  _expanded.clear();
                  _branches.clear();
                  _load(null, refresh: true);
                },
              ),
            ],
          ),
        ),
        Expanded(
          child: ScrollEdgeEffect(
            bottom: true,
            extent: 12,
            child: ListView.builder(
              padding: const EdgeInsets.fromLTRB(GlassSpacing.s4, 0, GlassSpacing.s4, GlassSpacing.s8),
              itemCount: rows.length,
              itemExtent: 28,
              itemBuilder: (context, index) {
                final (node, parent, depth) = rows[index];
                final indent = 8.0 + depth * 14.0;
                if (node == null) {
                  final branch = _branches[parent?.id ?? '']!;
                  return Padding(
                    padding: EdgeInsets.only(left: indent + 18),
                    child: Align(
                      alignment: Alignment.centerLeft,
                      child: branch.loading
                          ? const SizedBox(width: 80, child: LinearProgressIndicator(minHeight: 2))
                          : GlassButton.plain(
                              label: branch.failed ? l.commonRetry : l.dbLoadMore,
                              onPressed: () => _load(parent, more: branch.cursor != null, refresh: branch.failed),
                            ),
                    ),
                  );
                }
                final object = node.reference, expanded = _expanded.contains(object.id);
                final selected = _selected == object.id;
                return GestureDetector(
                  onSecondaryTapUp: (details) => unawaited(
                    _menuAt(object, Rect.fromLTWH(details.globalPosition.dx, details.globalPosition.dy, 1, 1)),
                  ),
                  onLongPressStart: (details) => unawaited(
                    _menuAt(object, Rect.fromLTWH(details.globalPosition.dx, details.globalPosition.dy, 1, 1)),
                  ),
                  onDoubleTap: node.hasChildren ? () => _toggle(object) : () => unawaited(_action('insert', object)),
                  child: GlassInteractive(
                    key: ValueKey('db-object-${object.id}'),
                    selected: selected,
                    semanticLabel: object.name,
                    onPressed: () {
                      setState(() => _selected = object.id);
                      widget.onSelect?.call(object);
                      if (node.hasChildren) _toggle(object);
                    },
                    builder: (context, state) => DecoratedBox(
                      decoration: ShapeDecoration(
                        color: selected
                            ? tokens.sidebarSelection
                            : state.hovered
                            ? tokens.surfaces.fillHover
                            : Colors.transparent,
                        shape: GlassRadii.shape(tokens.radii.row),
                      ),
                      child: Padding(
                        padding: EdgeInsets.only(left: indent, right: 2),
                        child: Row(
                          children: [
                            SizedBox(
                              width: 16,
                              child: node.hasChildren
                                  ? Icon(
                                      expanded ? Icons.expand_more_rounded : Icons.chevron_right_rounded,
                                      size: 16,
                                      color: tokens.secondaryLabel,
                                    )
                                  : null,
                            ),
                            const SizedBox(width: 4),
                            Icon(
                              _kindIcon(node),
                              size: 15,
                              color: node.primaryKey
                                  ? p.warning
                                  : node.foreignKey != null
                                  ? p.accent
                                  : tokens.secondaryLabel,
                            ),
                            const SizedBox(width: 6),
                            Flexible(
                              child: Text(
                                object.name,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: (selected ? tokens.typography.bodyEmph : tokens.typography.body).copyWith(
                                  color: selected ? p.accent : p.label,
                                ),
                              ),
                            ),
                            if (node.detail case final detail? when detail.isNotEmpty) ...[
                              const SizedBox(width: 6),
                              Expanded(
                                child: Text(
                                  detail,
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                  style: tokens.typography.caption.copyWith(color: p.tertiary),
                                ),
                              ),
                            ] else
                              const Spacer(),
                            if (state.hovered || selected)
                              GlassMenuButton<String>(
                                key: ValueKey('db-object-menu-${object.id}'),
                                entries: _menu(l, object),
                                onSelected: (value) => unawaited(_action(value, object)),
                                builder: (context, open) => GlassIconButton(
                                  icon: Icons.more_horiz_rounded,
                                  tooltip: MaterialLocalizations.of(context).moreButtonTooltip,
                                  size: 22,
                                  iconSize: 14,
                                  style: GlassIconButtonStyle.plain,
                                  onPressed: open,
                                ),
                              ),
                          ],
                        ),
                      ),
                    ),
                  ),
                );
              },
            ),
          ),
        ),
      ],
    );
  }
}

class DbProtectedTextDialog extends ConsumerStatefulWidget {
  const DbProtectedTextDialog({required this.title, required this.value, super.key});
  final String title;
  final SecretText value;
  @override
  ConsumerState<DbProtectedTextDialog> createState() => _DbProtectedTextDialogState();
}

class _DbProtectedTextDialogState extends ConsumerState<DbProtectedTextDialog> {
  late final _scope = ref.read(dbScopeProvider);
  @override
  Widget build(BuildContext context) {
    ref.listen(dbScopeProvider, (_, next) {
      if (!identical(next, _scope)) {
        widget.value.wipe();
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) closeDialog<void>(context);
        });
      }
    });
    if (!_scope.unlocked || !identical(_scope, ref.watch(dbScopeProvider)) || widget.value.isWiped) {
      return const SizedBox.shrink();
    }
    return GlassDialog(
      title: widget.title,
      content: SizedBox(
        width: 760,
        child: SingleChildScrollView(
          child: SelectableText(widget.value.expose(), style: const TextStyle(fontFamily: 'monospace')),
        ),
      ),
      secondaryActions: [GlassButton(label: context.l10n.commonClose, onPressed: () => closeDialog<void>(context))],
    );
  }
}
