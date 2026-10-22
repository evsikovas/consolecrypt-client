import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_query_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

class DbHistoryPanel extends ConsumerStatefulWidget {
  const DbHistoryPanel({required this.connection, required this.service, required this.onDraft, super.key});
  final DbConnection connection;
  final DbHistoryService service;
  final ValueChanged<String> onDraft;
  @override
  ConsumerState<DbHistoryPanel> createState() => _DbHistoryPanelState();
}

class _DbHistoryPanelState extends ConsumerState<DbHistoryPanel> {
  late final _scope = ref.read(dbScopeProvider);
  final _entries = <DbHistoryEntry>[], _cursors = <String>{};
  DbHistoryPolicy _policy = DbHistoryPolicy.local;
  bool _favorites = false, _busy = false, _failed = false;
  bool? _succeeded;
  String _search = '';
  String? _cursor;
  int _generation = 0;
  Timer? _debounce;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(dbScopeProvider));
  @override
  void initState() {
    super.initState();
    unawaited(_load());
    unawaited(_loadPolicy());
  }

  @override
  void dispose() {
    _generation++;
    _debounce?.cancel();
    _entries.clear();
    super.dispose();
  }

  Future<void> _loadPolicy() async {
    try {
      final policy = await widget.service.historyPolicy(widget.connection.id);
      if (_current) setState(() => _policy = policy);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    }
  }

  Future<void> _load({bool more = false}) async {
    if (!_current || (more && (_busy || _cursor == null))) return;
    final generation = ++_generation, cursor = more ? _cursor : null;
    setState(() {
      _busy = true;
      _failed = false;
      if (!more) {
        _entries.clear();
        _cursors.clear();
        _cursor = null;
      }
    });
    try {
      final page = await widget.service.history(
        widget.connection.id,
        query: _search,
        succeeded: _succeeded,
        favoritesOnly: _favorites,
        cursor: cursor,
      );
      if (!_current || generation != _generation) return;
      if (page.entries.length > 500 ||
          (page.nextCursor != null && (page.nextCursor == cursor || _cursors.contains(page.nextCursor)))) {
        throw const FormatException('Invalid history page');
      }
      setState(() {
        final ids = _entries.map((e) => e.id).toSet();
        _entries.addAll(
          page.entries
              .where((e) => e.connectionId == widget.connection.id && ids.add(e.id))
              .take(10000 - _entries.length),
        );
        if (cursor != null) _cursors.add(cursor);
        _cursor = _entries.length < 10000 ? page.nextCursor : null;
      });
    } catch (_) {
      if (_current && generation == _generation) setState(() => _failed = true);
    } finally {
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  Future<void> _policyChanged(DbHistoryPolicy? policy) async {
    if (policy == null || !_current) return;
    try {
      await widget.service.setHistoryPolicy(widget.connection.id, policy);
      if (_current) setState(() => _policy = policy);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    }
  }

  Future<void> _clear() async {
    final l = context.l10n;
    final yes = await showConfirmDialog(
      context,
      title: l.dbClearHistory,
      message: l.dbClearHistoryHelp,
      confirmLabel: l.commonDelete,
      destructive: true,
      secure: true,
    );
    if (!_current || !yes) return;
    try {
      await widget.service.deleteHistory(widget.connection.id);
      if (_current) await _load();
    } catch (_) {
      if (_current) setState(() => _failed = true);
    }
  }

  Future<void> _favorite(DbHistoryEntry entry) async {
    try {
      await widget.service.setFavorite(entry.id, !entry.favorite);
      if (_current) await _load();
    } catch (_) {
      if (_current) setState(() => _failed = true);
    }
  }

  Future<void> _snippet(DbHistoryEntry entry) async {
    final name = TextEditingController();
    try {
      final approved = await showAppDialog<bool>(
        context,
        secure: true,
        builder: (context) => GlassDialog(
          title: context.l10n.dbSaveSnippet,
          content: TextField(
            controller: name,
            decoration: InputDecoration(labelText: context.l10n.dbSnippetName),
          ),
          secondaryActions: [
            GlassButton(label: context.l10n.commonCancel, onPressed: () => closeDialog<bool>(context, false)),
          ],
          primaryAction: GlassButton(label: context.l10n.commonSave, onPressed: () => closeDialog<bool>(context, true)),
        ),
      );
      if (!_current || approved != true || name.text.trim().isEmpty) return;
      await widget.service.saveHistorySnippet(entry.id, name.text.trim());
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      name.clear();
      name.dispose();
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.watch(dbScopeProvider);
    if (!_current) return const SizedBox.shrink();
    final l = context.l10n;
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(8),
          child: Column(
            children: [
              TextField(
                decoration: InputDecoration(labelText: l.commonSearch),
                onChanged: (v) {
                  _search = v;
                  _debounce?.cancel();
                  _debounce = Timer(const Duration(milliseconds: 250), _load);
                },
              ),
              SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  children: [
                    FilterChip(
                      label: Text(l.dbFavorites),
                      selected: _favorites,
                      onSelected: (v) {
                        setState(() => _favorites = v);
                        _load();
                      },
                    ),
                    PopupMenuButton<bool?>(
                      tooltip: l.dbAllQueries,
                      onSelected: (v) {
                        setState(() => _succeeded = v);
                        _load();
                      },
                      itemBuilder: (_) => [
                        PopupMenuItem(value: null, child: Text(l.dbAllQueries)),
                        PopupMenuItem(value: true, child: Text(l.dbSuccessful)),
                        PopupMenuItem(value: false, child: Text(l.dbWithErrors)),
                      ],
                    ),
                    PopupMenuButton<DbHistoryPolicy>(
                      tooltip: l.dbHistory,
                      onSelected: _policyChanged,
                      itemBuilder: (_) => [
                        for (final p in DbHistoryPolicy.values)
                          // History of a device-local connection stays here too.
                          if (p != DbHistoryPolicy.encryptedSync || !widget.connection.engine.deviceLocal)
                          CheckedPopupMenuItem(
                            value: p,
                            checked: p == _policy,
                            child: Text(switch (p) {
                              DbHistoryPolicy.local => l.dbHistoryLocal,
                              DbHistoryPolicy.encryptedSync => l.dbHistorySync,
                              DbHistoryPolicy.disabled => l.dbHistoryDisabled,
                            }),
                          ),
                      ],
                    ),
                    IconButton(tooltip: l.dbClearHistory, icon: const Icon(Icons.delete_outline), onPressed: _clear),
                  ],
                ),
              ),
            ],
          ),
        ),
        if (_busy) const LinearProgressIndicator(),
        if (_failed) TextButton(onPressed: _load, child: Text(l.commonRetry)),
        Expanded(
          child: ListView.builder(
            itemCount: _entries.length + (_cursor == null ? 0 : 1),
            itemBuilder: (_, i) {
              if (i == _entries.length) {
                return TextButton(onPressed: _busy ? null : () => _load(more: true), child: Text(l.dbLoadMore));
              }
              final entry = _entries[i];
              final when = DateFormat.yMd().add_Hm().format(entry.createdAt.toLocal());
              final failed = entry.errorCode == null ? '' : ' ⚠'; // l10n-ignore: warning glyph
              return ListTile(
                title: Text(
                  entry.text,
                  maxLines: 3,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontFamily: 'monospace'),
                ),
                subtitle: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      '$when · ${l.dbElapsed(entry.elapsedMs)} · ${entry.rows} · ${entry.engine.label}$failed', // l10n-ignore: metadata
                    ),
                    if (entry.productionWrite)
                      Padding(
                        padding: const EdgeInsets.only(top: GlassSpacing.s4),
                        child: GlassBadge(
                          key: ValueKey('db-history-prod-write-${entry.id}'),
                          label: l.dbHistoryProdWrite,
                          tone: GlassTone.danger,
                          icon: Icons.edit_note_rounded,
                          dense: true,
                        ),
                      ),
                  ],
                ),
                onTap: () {
                  if (_current) widget.onDraft(entry.text);
                },
                leading: IconButton(
                  tooltip: l.dbFavorites,
                  icon: Icon(entry.favorite ? Icons.star : Icons.star_outline),
                  onPressed: () => _favorite(entry),
                ),
                trailing: IconButton(
                  tooltip: l.dbSaveSnippet,
                  icon: const Icon(Icons.bookmark_add_outlined),
                  onPressed: () => _snippet(entry),
                ),
              );
            },
          ),
        ),
      ],
    );
  }
}
