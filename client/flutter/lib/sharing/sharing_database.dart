import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/database_sharing_service.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/db/db_providers.dart';
import 'package:consolecrypt/db/db_service.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showSharingDbConnection(BuildContext context, {required String connectionId}) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingDbPublishDialog(connectionId: connectionId));

/// Why the shared route needs the receiver's own route object, if it does.
String? sharingDbRouteNotice(BuildContext context, Map<String, dynamic>? data) => switch (data?['route_kind']) {
  'relay' => context.l10n.sharingDbRouteRelay,
  'kubernetes' => context.l10n.sharingDbRouteKubernetes,
  'docker' => context.l10n.sharingDbRouteDocker,
  _ => null,
};

class SharingDbPublishDialog extends ConsumerStatefulWidget {
  const SharingDbPublishDialog({required this.connectionId, super.key});
  final String connectionId;
  @override
  ConsumerState<SharingDbPublishDialog> createState() => _SharingDbPublishDialogState();
}

class _SharingDbPublishDialogState extends ConsumerState<SharingDbPublishDialog> {
  late final Object _scope = ref.read(sharingSessionScopeProvider);
  DbSharingPreview? _preview;
  bool _credentials = false, _allowCredentials = false, _busy = false, _failed = false, _unsupported = false;
  List<SharingGrant> _grants = [];
  bool get _current => mounted && sharingSessionCurrent(ref, _scope);
  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    if (!_current) return;
    setState(() {
      _busy = true;
      _failed = false;
      _preview = null;
    });
    try {
      final service = ref.read(sharingServiceProvider);
      final status = await service.status();
      if (!_current) return;
      if (!status.enabled || status.locked || !status.supportsDbConnections || service is! DatabaseSharingService) {
        setState(() => _unsupported = true);
        return;
      }
      _allowCredentials = status.supportsSecrets;
      if (!_allowCredentials) _credentials = false;
      final preview = await (service as DatabaseSharingService).previewDbConnection(
        widget.connectionId,
        includeCredentials: _credentials,
      );
      if (_current) {
        setState(() {
          _preview = preview;
          _unsupported = false;
        });
      }
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  Future<void> _publish() async {
    final preview = _preview;
    final service = ref.read(sharingServiceProvider);
    if (!_current || _busy || preview == null || _grants.isEmpty || service is! DatabaseSharingService) return;
    setState(() => _busy = true);
    final item = await runWithFeedback(
      context,
      () => (service as DatabaseSharingService).publishDbConnection(
        widget.connectionId,
        List.of(_grants),
        expectedStamp: preview.snapshotStamp,
        includeCredentials: _credentials,
      ),
    );
    if (!mounted || !_current) return;
    if (item != null) {
      await runWithFeedback(context, service.flush);
      if (!mounted || !_current) return;
      refreshSharing(ref);
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final preview = _preview;
    final notice = preview == null
        ? null
        : sharingDbRouteNotice(
            context,
            (jsonDecode(preview.previewJson) as Map<String, dynamic>)['data'] as Map<String, dynamic>?,
          );
    return SharingDialogGuard(
      profile: _scope,
      child: GlassDialog(
        title: l.sharingPublish,
        icon: Icons.share_outlined,
        content: SizedBox(
          width: 600,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (_busy) const LinearProgressIndicator(),
                  if (_unsupported) Text(l.sharingDbUnsupported, key: const ValueKey('sharing-db-unsupported')),
                  if (_failed) ...[
                    Text(l.sharingUnavailable),
                    Text(l.sharingDbSnapshotChanged),
                    GlassButton(label: l.commonRetry, onPressed: _busy ? null : _load),
                  ],
                  if (preview != null) ...[
                    Text(l.sharingPreview, style: Theme.of(context).textTheme.titleSmall),
                    const SizedBox(height: 8),
                    SharingProjectionView(preview.previewJson),
                    if (notice != null) Text(notice, key: const ValueKey('sharing-db-route-notice')),
                    Text(l.sharingDbExclude),
                    if (_allowCredentials)
                      CheckboxListTile(
                        key: const ValueKey('sharing-db-include-credentials'),
                        contentPadding: EdgeInsets.zero,
                        title: Text(l.sharingDbIncludeCredentials),
                        value: _credentials,
                        onChanged: _busy
                            ? null
                            : (value) {
                                _credentials = value == true;
                                _load();
                              },
                      ),
                    if (_credentials) Text(l.sharingDbCredentialWarning),
                    const SizedBox(height: 16),
                    SharingRecipients(
                      key: const ValueKey('sharing-db-recipients'),
                      initialGrants: _grants,
                      onChanged: (grants) => setState(() => _grants = grants),
                    ),
                  ],
                ],
              ),
            ),
          ),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('sharing-db-publish-confirm'),
          label: l.sharingPublishConfirm,
          onPressed: _busy || preview == null || _grants.isEmpty ? null : _publish,
        ),
      ),
    );
  }
}

Future<void> showSharingCopyDbConnection(BuildContext context, SharingItem item) =>
    showAppDialog<void>(context, secure: true, builder: (_) => SharingDbCopyDialog(item: item));

/// A route the receiver may select for the shared connection's route shape.
final class _RouteOption {
  const _RouteOption(this.key, this.label, this.route);
  final String key, label;
  final DbRoute route;
}

class SharingDbCopyDialog extends ConsumerStatefulWidget {
  const SharingDbCopyDialog({required this.item, super.key});
  final SharingItem item;
  @override
  ConsumerState<SharingDbCopyDialog> createState() => _SharingDbCopyDialogState();
}

class _SharingDbCopyDialogState extends ConsumerState<SharingDbCopyDialog> {
  late final Object _scope = ref.read(sharingSessionScopeProvider);
  late String? _route = _routeKind == 'direct' ? '' : null;
  DbConnectionChoices? _choices;
  bool _credentials = false, _busy = false;
  bool get _current => mounted && sharingSessionCurrent(ref, _scope);
  Map<String, dynamic>? get _data => widget.item.data;
  String? get _routeKind => _data?['route_kind'] as String?;

  @override
  void initState() {
    super.initState();
    _loadChoices();
  }

  Future<void> _loadChoices() async {
    try {
      final choices = await ref.read(dbServiceProvider)?.connectionChoices();
      if (_current) setState(() => _choices = choices ?? const DbConnectionChoices());
    } catch (_) {
      if (_current) setState(() => _choices = const DbConnectionChoices());
    }
  }

  List<_RouteOption> _options(BuildContext context) {
    final l = context.l10n, data = _data, choices = _choices;
    if (data == null || choices == null) return const [];
    const relays = {DbRouteKind.ssh, DbRouteKind.jump, DbRouteKind.proxy, DbRouteKind.tunnel};
    String relayLabel(DbRouteChoice c) => switch (c.route.kind) {
      DbRouteKind.ssh => '${l.dbSsh}: ${c.label}', // l10n-ignore: route kind and saved name.
      DbRouteKind.jump => '${l.dbJump}: ${c.label}', // l10n-ignore: route kind and saved name.
      DbRouteKind.proxy => '${l.dbProxy}: ${c.label}', // l10n-ignore: route kind and saved name.
      DbRouteKind.tunnel => l.dbRouteTunnel(c.label),
      _ => c.label,
    };
    return switch (_routeKind) {
      'direct' || 'relay' => [
        if (_routeKind == 'direct') _RouteOption('', l.dbDirect, const DbRoute()),
        for (final (i, c) in choices.routes.indexed)
          if (relays.contains(c.route.kind)) _RouteOption('r$i', relayLabel(c), c.route),
      ],
      'kubernetes' => [
        for (final (i, c) in choices.routes.indexed)
          if (c.route.kind == DbRouteKind.kubernetes)
            _RouteOption(
              'r$i',
              l.dbRouteKubernetes(c.label),
              DbRoute(
                kind: DbRouteKind.kubernetes,
                reference: c.route.reference,
                namespace: data['route_namespace'] as String? ?? '',
                targetKind: DbKubeTargetKind.parse(data['route_target_kind'] as String?),
                targetName: data['route_target_name'] as String? ?? '',
              ),
            ),
      ],
      'docker' => [
        _RouteOption(
          'd',
          l.dockerLocalHost,
          DbRoute(kind: DbRouteKind.docker, container: data['route_container'] as String? ?? ''),
        ),
        for (final (i, c) in choices.routes.indexed)
          if (c.route.kind == DbRouteKind.ssh)
            _RouteOption(
              'r$i',
              c.label,
              DbRoute(
                kind: DbRouteKind.docker,
                reference: c.route.reference,
                container: data['route_container'] as String? ?? '',
              ),
            ),
      ],
      _ => const [],
    };
  }

  Future<void> _copy(_RouteOption option) async {
    final service = ref.read(sharingServiceProvider);
    if (!_current || _busy || widget.item.trust != SharingTrust.verified || service is! DatabaseSharingService) {
      return;
    }
    final scope = ref.read(dbScopeProvider);
    setState(() => _busy = true);
    final saved = await runWithFeedback(context, () async {
      await (service as DatabaseSharingService).copyDbConnection(
        widget.item.id,
        route: option.route.toJson(),
        includeCredentials: _credentials,
      );
      return true;
    }, success: context.l10n.sharingSaved);
    if (!mounted || !_current) return;
    if (saved == true) {
      ref.invalidate(dbConnectionsProvider(scope));
      closeDialog<void>(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final data = _data;
    final options = _options(context);
    final selected = options.where((o) => o.key == _route).firstOrNull;
    final notice = sharingDbRouteNotice(context, data);
    final allowed =
        widget.item.kind == SharingKind.dbConnection &&
        widget.item.trust == SharingTrust.verified &&
        widget.item.previewJson != null &&
        ref.watch(sharingServiceProvider) is DatabaseSharingService;
    return SharingDialogGuard(
      profile: _scope,
      child: GlassDialog(
        title: l.sharingCopyPersonal,
        icon: Icons.storage_rounded,
        content: SizedBox(
          width: 560,
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (_busy || (allowed && _choices == null)) const LinearProgressIndicator(),
                  if (!allowed) Text(l.sharingUnavailable),
                  if (allowed) ...[
                    SharingProjectionView(widget.item.previewJson!),
                    Text(l.sharingDbCopyHelp),
                    if (notice != null) ...[Text(notice), Text(l.sharingDbRouteRequired)],
                    const SizedBox(height: 12),
                    if (_choices != null && options.isEmpty)
                      Text(l.sharingDbNoRoute, key: const ValueKey('sharing-db-no-route'))
                    else if (_choices != null)
                      GlassDropdownField<String>(
                        key: const ValueKey('sharing-db-route'),
                        value: selected?.key,
                        label: l.dbRoute,
                        items: [
                          for (final o in options)
                            GlassSelectItem(value: o.key, label: o.label, key: ValueKey('sharing-db-route-${o.key}')),
                        ],
                        onChanged: _busy ? null : (value) => setState(() => _route = value),
                      ),
                    if (data?['includes_credentials'] == true) ...[
                      CheckboxListTile(
                        key: const ValueKey('sharing-db-import-credentials'),
                        contentPadding: EdgeInsets.zero,
                        title: Text(l.sharingDbImportCredentials),
                        value: _credentials,
                        onChanged: _busy ? null : (value) => setState(() => _credentials = value == true),
                      ),
                      if (_credentials) Text(l.sharingDbCredentialWarning),
                    ],
                  ],
                ],
              ),
            ),
          ),
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<void>(context)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('sharing-db-copy-confirm'),
          label: l.commonSave,
          onPressed: !allowed || _busy || selected == null ? null : () => _copy(selected),
        ),
      ),
    );
  }
}
