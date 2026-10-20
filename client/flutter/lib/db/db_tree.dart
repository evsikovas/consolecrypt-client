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

  @override
  Widget build(BuildContext context) {
    ref.watch(dbScopeProvider);
    if (!_current) return const SizedBox.shrink();
    final l = context.l10n, rows = _visible();
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(8),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  decoration: InputDecoration(labelText: l.dbTreeSearch),
                  onChanged: (v) => setState(() => _filter = v.toLowerCase()),
                ),
              ),
              IconButton(
                tooltip: l.commonRetry,
                icon: const Icon(Icons.refresh),
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
          child: ListView.builder(
            itemCount: rows.length,
            itemBuilder: (context, index) {
              final (node, parent, depth) = rows[index];
              if (node == null) {
                final branch = _branches[parent?.id ?? '']!;
                return Padding(
                  padding: EdgeInsets.only(left: depth * 12.0),
                  child: branch.loading
                      ? const LinearProgressIndicator()
                      : TextButton(
                          onPressed: () => _load(parent, more: branch.cursor != null, refresh: branch.failed),
                          child: Text(branch.failed ? l.commonRetry : l.dbLoadMore),
                        ),
                );
              }
              final object = node.reference, expanded = _expanded.contains(node.reference.id);
              return Padding(
                padding: EdgeInsets.only(left: depth * 12.0),
                child: ListTile(
                  key: ValueKey('db-object-${object.id}'),
                  leading: node.hasChildren
                      ? IconButton(
                          icon: Icon(expanded ? Icons.expand_more : Icons.chevron_right),
                          onPressed: () {
                            setState(() {
                              if (!_expanded.remove(object.id)) _expanded.add(object.id);
                            });
                            if (!expanded) _load(object);
                          },
                        )
                      : Icon(node.primaryKey ? Icons.key : Icons.table_rows, size: 20),
                  title: Text(object.name, maxLines: 1, overflow: TextOverflow.ellipsis),
                  subtitle: node.detail == null
                      ? null
                      : Text(node.detail!, maxLines: 2, overflow: TextOverflow.ellipsis),
                  onTap: () => widget.onSelect?.call(object),
                  trailing: PopupMenuButton<String>(
                    onSelected: (a) => _action(a, object),
                    itemBuilder: (_) => [
                      PopupMenuItem(value: 'insert', child: Text(l.dbInsertName)),
                      if (const [
                        DbObjectKind.table,
                        DbObjectKind.view,
                        DbObjectKind.materializedView,
                        DbObjectKind.collection,
                        DbObjectKind.redisKey,
                      ].contains(object.kind))
                        PopupMenuItem(value: 'query', child: Text(l.dbFirstRows)),
                      if (widget.session.engine.sql) PopupMenuItem(value: 'ddl', child: Text(l.dbDdl)),
                      if (widget.service is DbObjectInspectService) ...[
                        PopupMenuItem(value: 'inspect', child: Text(l.dbInspect)),
                        if (object.kind == DbObjectKind.collection)
                          PopupMenuItem(value: 'sample', child: Text(l.dbSampleSchema)),
                      ],
                    ],
                  ),
                ),
              );
            },
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
