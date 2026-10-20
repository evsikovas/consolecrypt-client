import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_connection_dialog.dart';
import 'package:consolecrypt/db/db_import_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class DbConnectionsScreen extends ConsumerStatefulWidget {
  const DbConnectionsScreen({required this.onOpen, this.onShare, super.key});
  final ValueChanged<DbConnection> onOpen;
  final ValueChanged<DbConnection>? onShare;
  @override
  ConsumerState<DbConnectionsScreen> createState() => _DbConnectionsScreenState();
}

class _DbConnectionsScreenState extends ConsumerState<DbConnectionsScreen> {
  String _search = '';
  String? _tag, _group;
  Future<void> _edit([DbConnection? connection]) =>
      showAppDialog<DbConnection>(context, secure: true, builder: (_) => DbConnectionDialog(connection: connection));
  Future<void> _delete(DbConnection connection) async {
    final scope = ref.read(dbScopeProvider), service = ref.read(dbServiceProvider), l = context.l10n;
    final confirmed = await showConfirmDialog(
      context,
      title: l.commonDelete,
      message: l.dbDeleteHelp,
      confirmLabel: l.commonDelete,
      destructive: true,
      secure: true,
      extra: Text(connection.name),
    );
    if (!confirmed || !mounted || !identical(scope, ref.read(dbScopeProvider)) || !scope.unlocked || service == null) {
      return;
    }
    try {
      await service.deleteConnection(connection.id);
      if (mounted && identical(scope, ref.read(dbScopeProvider))) ref.invalidate(dbConnectionsProvider(scope));
    } catch (_) {
      if (mounted && identical(scope, ref.read(dbScopeProvider))) showSnack(context, l.dbFailed, error: true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, scope = ref.watch(dbScopeProvider), service = ref.watch(dbServiceProvider);
    if (!scope.unlocked) return Center(child: Text(l.dbLocked));
    if (service == null) return Center(child: Text(l.dbUnavailable));
    final inventory = ref.watch(dbConnectionsProvider(scope));
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.dbTitle, style: Theme.of(context).textTheme.headlineSmall),
          Text(l.dbSubtitle),
          const SizedBox(height: 12),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              GlassButton(
                key: const ValueKey('db-new-connection'),
                label: l.dbNewConnection,
                icon: Icons.add,
                onPressed: _edit,
              ),
              GlassButton(
                key: const ValueKey('db-import'),
                label: l.dbImport,
                icon: Icons.file_download_outlined,
                onPressed: () => showAppDialog<bool>(context, secure: true, builder: (_) => const DbImportDialog()),
              ),
              GlassIconButton(
                icon: Icons.refresh,
                tooltip: l.dbRefresh,
                onPressed: () => ref.invalidate(dbConnectionsProvider(scope)),
              ),
            ],
          ),
          const SizedBox(height: 12),
          TextField(
            key: const ValueKey('db-search'),
            decoration: InputDecoration(labelText: l.commonSearch, prefixIcon: const Icon(Icons.search)),
            onChanged: (v) => setState(() => _search = v.toLowerCase()),
          ),
          const SizedBox(height: 12),
          Expanded(
            child: inventory.when(
              loading: () => const Center(child: CircularProgressIndicator()),
              error: (_, _) => Center(child: Text(l.dbFailed)),
              data: (connections) {
                final tags = connections.expand((c) => c.tags).toSet().toList()..sort();
                final groups = connections.map((c) => c.groupId).whereType<String>().toSet().toList()..sort();
                final shown =
                    connections
                        .where(
                          (c) =>
                              (_tag == null || c.tags.contains(_tag)) &&
                              (_group == null || c.groupId == _group) &&
                              [
                                c.name,
                                c.host,
                                c.database,
                                c.engine.label,
                                ...c.tags,
                              ].any((s) => s.toLowerCase().contains(_search)),
                        )
                        .toList()
                      ..sort((a, b) => a.name.compareTo(b.name));
                return Column(
                  children: [
                    if (tags.isNotEmpty || groups.isNotEmpty)
                      SingleChildScrollView(
                        scrollDirection: Axis.horizontal,
                        child: Row(
                          children: [
                            for (final tag in tags)
                              Padding(
                                padding: const EdgeInsets.only(right: 6),
                                child: FilterChip(
                                  label: Text(tag),
                                  selected: tag == _tag,
                                  onSelected: (v) => setState(() => _tag = v ? tag : null),
                                ),
                              ),
                            for (final group in groups)
                              Padding(
                                padding: const EdgeInsets.only(right: 6),
                                child: FilterChip(
                                  label: Text(group),
                                  selected: group == _group,
                                  onSelected: (v) => setState(() => _group = v ? group : null),
                                ),
                              ),
                          ],
                        ),
                      ),
                    if (connections.isEmpty) Text(l.dbEmpty),
                    Expanded(
                      child: ListView.separated(
                        itemCount: shown.length,
                        separatorBuilder: (_, _) => const SizedBox(height: 8),
                        itemBuilder: (context, i) {
                          final c = shown[i];
                          return ContentSurface(
                            child: ListTile(
                              leading: Icon(
                                Icons.storage,
                                color: c.color != null && RegExp(r'^#[a-fA-F0-9]{6}$').hasMatch(c.color!)
                                    ? Color(int.parse('ff${c.color!.substring(1)}', radix: 16))
                                    : null,
                              ),
                              title: Text(c.name),
                              subtitle: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Text(c.engine.label),
                                  Text('${c.host}:${c.port}'), // l10n-ignore: Saved endpoint metadata.
                                  if (c.database.isNotEmpty) Text(c.database),
                                  if (c.production)
                                    Text(l.dbProd, style: TextStyle(color: Theme.of(context).colorScheme.error)),
                                ],
                              ),
                              onTap: () => widget.onOpen(c),
                              trailing: PopupMenuButton<String>(
                                key: ValueKey('db-menu-${c.id}'),
                                itemBuilder: (_) => [
                                  PopupMenuItem(value: 'open', child: Text(l.commonConnect)),
                                  PopupMenuItem(value: 'edit', child: Text(l.commonEdit)),
                                  if (widget.onShare != null)
                                    PopupMenuItem(value: 'share', child: Text(l.sharingTitle)),
                                  PopupMenuItem(value: 'delete', child: Text(l.commonDelete)),
                                ],
                                onSelected: (action) {
                                  switch (action) {
                                    case 'open':
                                      widget.onOpen(c);
                                    case 'edit':
                                      _edit(c);
                                    case 'delete':
                                      _delete(c);
                                    case 'share':
                                      widget.onShare?.call(c);
                                  }
                                },
                              ),
                            ),
                          );
                        },
                      ),
                    ),
                  ],
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}
