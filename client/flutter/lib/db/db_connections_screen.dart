import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/view_mode_toggle.dart';
import 'package:consolecrypt/db/db_connection_dialog.dart';
import 'package:consolecrypt/db/db_errors.dart';
import 'package:consolecrypt/db/db_import_dialog.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_transport_badge.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class DbConnectionsScreen extends ConsumerStatefulWidget {
  const DbConnectionsScreen({required this.onOpen, this.onShare, this.shared = const [], this.onOpenShared, super.key});
  final ValueChanged<DbConnection> onOpen;
  final ValueChanged<DbConnection>? onShare;

  /// Selectively shared connections (incoming and published by this device).
  final List<SharingItem> shared;
  final ValueChanged<SharingItem>? onOpenShared;
  @override
  ConsumerState<DbConnectionsScreen> createState() => _DbConnectionsScreenState();
}

class _DbConnectionsScreenState extends ConsumerState<DbConnectionsScreen> {
  String _search = '';
  String? _tag, _group;

  /// Shown until the device-local setting catches up with the user's choice.
  CollectionView? _view;

  Future<void> _edit([DbConnection? connection]) =>
      showAppDialog<DbConnection>(context, secure: true, builder: (_) => DbConnectionDialog(connection: connection));

  Future<void> _setView(CollectionView view) async {
    setState(() => _view = view);
    final settings = ref.read(settingsServiceProvider);
    try {
      await settings.updateLocal(settings.currentLocal.copyWith(databasesView: view));
    } catch (_) {
      // Layout preference only; the in-memory choice still applies.
    }
  }

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

  void _act(_DbAction action, DbConnection c) {
    switch (action) {
      case _DbAction.open:
        widget.onOpen(c);
      case _DbAction.edit:
        unawaited(_edit(c));
      case _DbAction.share:
        widget.onShare?.call(c);
      case _DbAction.delete:
        unawaited(_delete(c));
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, scope = ref.watch(dbScopeProvider), service = ref.watch(dbServiceProvider);
    if (!scope.unlocked) return Center(child: Text(l.dbLocked));
    if (service == null) return Center(child: Text(l.dbUnavailable));
    final inventory = ref.watch(dbConnectionsProvider(scope));
    final cards =
        (_view ?? ref.watch(localSettingsProvider.select((s) => s.value?.databasesView)) ?? CollectionView.cards) ==
        CollectionView.cards;
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
          Row(
            children: [
              Expanded(
                child: GlassField(
                  key: const ValueKey('db-search'),
                  search: true,
                  leadingIcon: Icons.search_rounded,
                  placeholder: l.commonSearch,
                  onChanged: (v) => setState(() => _search = v.toLowerCase()),
                ),
              ),
              const SizedBox(width: 8),
              ViewModeToggle(
                keyPrefix: 'db-view',
                cards: cards,
                onChanged: (cards) => unawaited(_setView(cards ? CollectionView.cards : CollectionView.list)),
              ),
            ],
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
                final sharedShown = widget.shared
                    .where(
                      (item) => [
                        item.name ?? '',
                        item.data?['host'] as String? ?? '',
                        item.data?['database'] as String? ?? '',
                      ].any((s) => s.toLowerCase().contains(_search)),
                    )
                    .toList(growable: false);
                final sharedRows = sharedShown.isEmpty ? 0 : sharedShown.length + 1;
                final observed = ref.watch(dbLastSecurityProvider);
                Widget entry(DbConnection c) => _DbConnectionEntry(
                  connection: c,
                  security: observed[c.id] ?? DbTransportSecurity.expected(c, loopback: dbLoopbackHost(c.host)),
                  cards: cards,
                  canShare: widget.onShare != null,
                  onOpen: () => widget.onOpen(c),
                  onAction: (action) => _act(action, c),
                );
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    if (tags.isNotEmpty || groups.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.only(bottom: 12),
                        child: SingleChildScrollView(
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
                      ),
                    if (connections.isEmpty && sharedShown.isEmpty) Text(l.dbEmpty),
                    Expanded(
                      child: LayoutBuilder(
                        builder: (context, box) {
                          final scale = MediaQuery.textScalerOf(context).scale(14) / 14;
                          final columns = cards ? (box.maxWidth / (290 * scale.clamp(1, 1.4))).floor().clamp(1, 4) : 1;
                          return ListView.builder(
                            key: const ValueKey('db-connections-list'),
                            itemCount: sharedRows + (shown.length / columns).ceil(),
                            itemBuilder: (context, row) {
                              if (row == 0 && sharedRows > 0) {
                                return Padding(
                                  key: const ValueKey('db-shared-header'),
                                  padding: const EdgeInsets.only(bottom: 6),
                                  child: Text(
                                    '${l.dbSharedTitle} · ${l.dbSharedCount(sharedShown.length)}', // l10n-ignore: localized parts.
                                    style: Theme.of(context).textTheme.titleSmall,
                                  ),
                                );
                              }
                              if (row < sharedRows) {
                                final item = sharedShown[row - 1];
                                return Padding(
                                  padding: EdgeInsets.only(bottom: row == sharedRows - 1 ? 16 : 6),
                                  child: _DbSharedEntry(item: item, onOpen: () => widget.onOpenShared?.call(item)),
                                );
                              }
                              final index = row - sharedRows;
                              return Padding(
                                padding: EdgeInsets.only(bottom: cards ? 12 : 6),
                                child: columns == 1
                                    ? entry(shown[index])
                                    : Row(
                                        crossAxisAlignment: CrossAxisAlignment.start,
                                        children: [
                                          for (var col = 0; col < columns; col++) ...[
                                            if (col > 0) const SizedBox(width: 12),
                                            Expanded(
                                              child: index * columns + col < shown.length
                                                  ? entry(shown[index * columns + col])
                                                  : const SizedBox.shrink(),
                                            ),
                                          ],
                                        ],
                                      ),
                              );
                            },
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

enum _DbAction { open, edit, share, delete }

/// `#RRGGBB` label colour of a saved connection, if valid.
Color? _connectionColor(String? hex) => hex != null && RegExp(r'^#[a-fA-F0-9]{6}$').hasMatch(hex)
    ? Color(int.parse('ff${hex.substring(1)}', radix: 16))
    : null;

IconData _engineIcon(DbEngine engine) => switch (engine.wireName) {
  'redis' => Icons.memory_rounded,
  'mongodb' => Icons.data_object_rounded,
  'clickhouse' => Icons.view_column_rounded,
  _ => Icons.storage_rounded,
};

/// One saved connection: a card in the grid or a compact single-line row.
class _DbConnectionEntry extends StatelessWidget {
  const _DbConnectionEntry({
    required this.connection,
    required this.security,
    required this.cards,
    required this.canShare,
    required this.onOpen,
    required this.onAction,
  });

  final DbConnection connection;

  /// Seen in the last session, or what the settings guarantee; null when
  /// only the server can tell.
  final DbTransportSecurity? security;
  final bool cards, canShare;
  final VoidCallback onOpen;
  final ValueChanged<_DbAction> onAction;

  Widget? _routeHint(BuildContext context, GlassTokens tokens) {
    final l = context.l10n;
    final (IconData, String)? hint = switch (connection.route.kind) {
      DbRouteKind.direct => null,
      DbRouteKind.ssh => (Icons.terminal_rounded, l.dbSsh),
      DbRouteKind.jump => (Icons.alt_route_rounded, l.dbJump),
      DbRouteKind.proxy => (Icons.hub_outlined, l.dbProxy),
      DbRouteKind.kubernetes => (Icons.anchor_rounded, l.dbRouteKubeHint),
      DbRouteKind.tunnel => (Icons.swap_horiz_rounded, l.dbRouteTunnelHint),
      DbRouteKind.docker => (Icons.view_in_ar_outlined, l.dbRouteDockerHint),
    };
    if (hint == null) return null;
    return Tooltip(
      key: ValueKey('db-route-${connection.id}'),
      message: hint.$2,
      child: Icon(hint.$1, size: 15, color: tokens.secondaryLabel),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final c = connection;
    final tint = _connectionColor(c.color);
    final caption = [
      c.engine.label,
      '${c.host}:${c.port}', // l10n-ignore: Saved endpoint metadata.
      if (c.database.isNotEmpty) c.database,
    ].join(' · ');
    final route = _routeHint(context, tokens);
    final avatarSize = cards ? 36.0 : 28.0;
    final avatar = SizedBox.square(
      dimension: avatarSize,
      child: DecoratedBox(
        decoration: ShapeDecoration(
          color: tint?.withValues(alpha: .16) ?? tokens.surfaces.inset,
          shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(avatarSize * .28)),
        ),
        child: Center(
          child: Icon(_engineIcon(c.engine), size: avatarSize * .55, color: tint ?? tokens.secondaryLabel),
        ),
      ),
    );
    final name = Row(
      children: [
        Flexible(
          child: Text(
            c.name,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: t.bodyEmph.copyWith(color: tokens.palette.label),
          ),
        ),
        if (route != null) ...[const SizedBox(width: GlassSpacing.s6), route],
        if (security case final security?) ...[
          const SizedBox(width: GlassSpacing.s6),
          DbTransportBadge(security, compact: true, key: ValueKey('db-transport-${connection.id}')),
        ],
      ],
    );
    final captionText = Text(
      caption,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: t.callout.copyWith(color: tokens.secondaryLabel),
    );
    final menu = GlassMenuButton<_DbAction>(
      entries: [
        GlassMenuItem(value: _DbAction.open, label: l.commonConnect, icon: Icons.play_arrow_rounded),
        GlassMenuItem(value: _DbAction.edit, label: l.commonEdit, icon: Icons.edit_rounded),
        if (canShare) GlassMenuItem(value: _DbAction.share, label: l.sharingTitle, icon: Icons.share_outlined),
        const GlassMenuDivider(),
        GlassMenuItem(value: _DbAction.delete, label: l.commonDelete, icon: Icons.delete_rounded, destructive: true),
      ],
      onSelected: onAction,
      builder: (context, open) => GlassIconButton(
        key: ValueKey('db-menu-${c.id}'),
        icon: Icons.more_horiz_rounded,
        tooltip: l.hostsMoreTooltip,
        style: GlassIconButtonStyle.plain,
        onPressed: open,
      ),
    );
    final badge = c.production
        ? Padding(
            padding: const EdgeInsets.only(left: 8),
            child: GlassBadge(label: l.dbProd, tone: GlassTone.danger, dense: true),
          )
        : null;
    return GlassInteractive(
      key: ValueKey('db-connection-${c.id}'),
      semanticLabel: '${c.name}, $caption, ${l.commonConnect}',
      onPressed: onOpen,
      builder: (context, state) => GlassFocusRing(
        visible: state.focusVisible,
        shape: GlassRadii.shape(tokens.radii.card),
        child: ContentSurface(
          padding: cards
              ? const EdgeInsets.all(GlassSpacing.card)
              : const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
          color: state.hovered ? Color.alphaBlend(tokens.surfaces.fillHover, tokens.surfaces.content) : null,
          child: cards
              ? Row(
                  children: [
                    avatar,
                    const SizedBox(width: 10),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [name, const SizedBox(height: 4), captionText],
                      ),
                    ),
                    ?badge,
                    const SizedBox(width: 4),
                    menu,
                  ],
                )
              : Row(
                  children: [
                    avatar,
                    const SizedBox(width: 10),
                    Expanded(flex: 2, child: name),
                    const SizedBox(width: 12),
                    Expanded(flex: 3, child: captionText),
                    ?badge,
                    const SizedBox(width: 4),
                    menu,
                  ],
                ),
        ),
      ),
    );
  }
}

/// A selectively shared connection: opens its sharing details, where the
/// receiver saves a personal copy and the owner manages members or revokes.
class _DbSharedEntry extends StatelessWidget {
  const _DbSharedEntry({required this.item, required this.onOpen});
  final SharingItem item;
  final VoidCallback onOpen;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final d = item.data;
    final engine = DbEngine.values.where((e) => e.wireName == d?['engine']).firstOrNull;
    final caption = [
      if (engine != null) engine.label,
      if (d?['host'] case final String host) '$host:${d?['port']}', // l10n-ignore: Shared endpoint metadata.
      if (d?['database'] case final String database when database.isNotEmpty) database,
      if (item.role case final role?) role == SharingRole.editor ? l.sharingEditor : l.sharingReader,
    ].join(' · ');
    return GlassInteractive(
      key: ValueKey('db-shared-${item.id}'),
      semanticLabel: '${item.name ?? l.sharingInvitation}, ${l.dbSharedBadge}',
      onPressed: onOpen,
      builder: (context, state) => GlassFocusRing(
        visible: state.focusVisible,
        shape: GlassRadii.shape(tokens.radii.card),
        child: ContentSurface(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
          color: state.hovered ? Color.alphaBlend(tokens.surfaces.fillHover, tokens.surfaces.content) : null,
          child: Row(
            children: [
              Icon(
                engine == null ? Icons.storage_rounded : _engineIcon(engine),
                size: 18,
                color: tokens.secondaryLabel,
              ),
              const SizedBox(width: 10),
              Expanded(
                flex: 2,
                child: Text(
                  item.name ?? l.sharingInvitation,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: t.bodyEmph.copyWith(color: tokens.palette.label),
                ),
              ),
              const SizedBox(width: 12),
              Expanded(
                flex: 3,
                child: Text(
                  caption,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: t.callout.copyWith(color: tokens.secondaryLabel),
                ),
              ),
              if (d?['production'] == true) ...[
                GlassBadge(label: l.dbProd, tone: GlassTone.danger, dense: true),
                const SizedBox(width: 6),
              ],
              GlassBadge(
                key: ValueKey('db-shared-badge-${item.id}'),
                label: item.owned ? l.dbSharedOwned : l.dbSharedBadge,
                tone: GlassTone.accent,
                icon: item.owned ? Icons.share_outlined : Icons.people_outline_rounded,
                dense: true,
              ),
              const SizedBox(width: 8),
              GlassButton(
                label: item.trust == SharingTrust.unverified ? l.sharingAccept : l.dbSharedOpen,
                onPressed: onOpen,
              ),
            ],
          ),
        ),
      ),
    );
  }
}
