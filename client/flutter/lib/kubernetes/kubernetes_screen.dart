import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_connection_editor.dart';
import 'package:consolecrypt/kubernetes/kube_connection_status.dart';
import 'package:consolecrypt/kubernetes/kube_export_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_history_panel.dart';
import 'package:consolecrypt/kubernetes/kube_history_service.dart';
import 'package:consolecrypt/kubernetes/kube_icons.dart';
import 'package:consolecrypt/kubernetes/kube_import_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_inventory_service.dart';
import 'package:consolecrypt/kubernetes/kube_navigation.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/sharing/sharing_kubernetes.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubernetesScreen extends ConsumerStatefulWidget {
  const KubernetesScreen({super.key});
  @override
  ConsumerState<KubernetesScreen> createState() => _KubernetesScreenState();
}

class _KubernetesScreenState extends ConsumerState<KubernetesScreen> {
  String _query = '';
  String? _tag;
  KubeConnection? _selected;
  KubeScope? _selectedScope;

  Future<void> _import() async {
    final scope = ref.read(kubeScopeProvider);
    if (!scope.unlocked) return;
    final saved = await showAppDialog<bool>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (_) => const KubeImportDialog(),
    );
    if (!mounted || !identical(scope, ref.read(kubeScopeProvider))) return;
    if (saved == true) showSnack(context, context.l10n.kubeSaved, tone: GlassTone.success);
  }

  Future<void> _edit([KubeConnection? connection]) async {
    if (!ref.read(kubeScopeProvider).unlocked) return;
    await showAppDialog<bool>(
      context,
      // A new connection takes a token or private key; editing an existing one
      // changes names, colours and policy only.
      secure: connection == null,
      barrierDismissible: false,
      builder: (_) => KubeConnectionEditor(connection: connection),
    );
  }

  Future<void> _delete(KubeConnection connection) async {
    final scope = ref.read(kubeScopeProvider);
    final service = ref.read(kubeServiceProvider);
    if (!scope.unlocked) return;
    final approved = await showConfirmDialog(
      context,
      title: connection.name,
      message: context.l10n.kubeDeleteHelp,
      confirmLabel: context.l10n.commonDelete,
      destructive: true,
      secure: true,
    );
    if (!approved || !mounted || !identical(scope, ref.read(kubeScopeProvider))) return;
    await runWithFeedback(context, () => service.deleteContext(connection.id));
    if (mounted && identical(scope, ref.read(kubeScopeProvider))) ref.invalidate(kubeConnectionsProvider);
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final scope = ref.watch(kubeScopeProvider);
    final service = ref.watch(kubeServiceProvider);
    final request = ref.watch(kubeNavigationProvider);
    final state = ref.watch(kubeConnectionsProvider(scope));
    final statuses = ref.watch(kubeConnectionStatusProvider);
    if (request != null && identical(request.scope, scope) && scope.unlocked && _selected?.id != request.contextId) {
      final connection = state.value?.where((entry) => entry.id == request.contextId).firstOrNull;
      if (connection != null) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (!mounted ||
              !identical(scope, ref.read(kubeScopeProvider)) ||
              !identical(request, ref.read(kubeNavigationProvider))) {
            return;
          }
          setState(() {
            _selected = connection;
            _selectedScope = scope;
          });
        });
      } else if (state.hasValue) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) ref.read(kubeNavigationProvider.notifier).consume(request);
        });
      }
    }
    // After an unlock, reopen the cluster whose session survived the lock.
    final parked = ref.watch(kubeParkedSessionsProvider);
    final open = _selected != null && identical(_selectedScope, scope);
    if (!open && scope.unlocked && request == null && parked.isNotEmpty) {
      final connection = state.value?.where((entry) => parked.containsKey(entry.id)).firstOrNull;
      if (connection != null) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (!mounted ||
              (_selected != null && identical(_selectedScope, scope)) ||
              !identical(scope, ref.read(kubeScopeProvider))) {
            return;
          }
          setState(() {
            _selected = connection;
            _selectedScope = scope;
          });
        });
      }
    }
    if (_selected != null && identical(_selectedScope, scope) && scope.unlocked && service is KubeBrowserService) {
      return KubeBrowser(
        key: ValueKey((_selectedScope, _selected!.id)),
        // The saved settings may have changed (cluster menu, editor) since
        // the cluster was opened; pass the fresh copy.
        connection: state.value?.where((c) => c.id == _selected!.id).firstOrNull ?? _selected!,
        connections: state.value ?? const [],
        onSelectContext: (connection) {
          if (!identical(scope, ref.read(kubeScopeProvider)) || !scope.unlocked) return;
          setState(() {
            _selected = connection;
            _selectedScope = scope;
          });
        },
        service: service as KubeBrowserService,
        onClose: () => setState(() {
          final request = ref.read(kubeNavigationProvider);
          if (request != null && request.contextId == _selected?.id) {
            ref.read(kubeNavigationProvider.notifier).consume(request);
          }
          _selected = null;
          _selectedScope = null;
        }),
      );
    }
    final items =
        state.value
            ?.where(
              (connection) =>
                  '${connection.name} ${connection.clusterName} ${connection.server} ${connection.namespace} ${connection.tags.join(' ')}'
                      .toLowerCase()
                      .contains(_query.toLowerCase()) &&
                  (_tag == null || connection.tags.contains(_tag)),
            )
            .toList() ??
        [];
    items.sort((a, b) {
      final group = a.clusterName.compareTo(b.clusterName);
      return group == 0 ? a.name.compareTo(b.name) : group;
    });
    final tags = (state.value ?? const <KubeConnection>[]).expand((c) => c.tags).toSet().toList()..sort();
    return PageScaffold(
      title: l.kubeTitle,
      subtitle: l.kubeSubtitle,
      actions: [
        GlassButton(key: const ValueKey('kube-new'), label: l.kubeNewConnection, icon: Icons.add, onPressed: _edit),
        GlassButton(
          key: const ValueKey('kube-import'),
          label: l.kubeImport,
          icon: Icons.file_open_outlined,
          onPressed: _import,
        ),
        GlassIconButton(
          icon: Icons.refresh,
          tooltip: l.kubeRefresh,
          onPressed: () => ref.invalidate(kubeConnectionsProvider),
        ),
      ],
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              SizedBox(
                width: 320,
                child: GlassField(
                  key: const ValueKey('kube-search'),
                  fieldKey: const ValueKey('kube-search-input'),
                  search: true,
                  placeholder: l.commonSearch,
                  onChanged: (value) => setState(() => _query = value),
                ),
              ),
              const SizedBox(width: GlassSpacing.s8),
              Expanded(
                child: SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: Row(
                    children: [
                      for (final tag in tags)
                        Padding(
                          padding: const EdgeInsets.only(right: 6),
                          child: GlassButton.plain(
                            key: ValueKey('kube-tag-$tag'),
                            label: tag,
                            icon: _tag == tag ? Icons.check_rounded : Icons.sell_outlined,
                            onPressed: () => setState(() => _tag = _tag == tag ? null : tag),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: GlassSpacing.s12),
          if (state.isLoading) const LinearProgressIndicator(minHeight: 2),
          if (state.hasError) Text(l.kubeLoadFailed),
          if (!state.isLoading && !state.hasError && (state.value?.isEmpty ?? true))
            Expanded(
              child: EmptyState(
                icon: Icons.cloud_queue_rounded,
                title: l.kubeEmpty,
                action: GlassButton.prominent(label: l.kubeImport, icon: Icons.file_open_outlined, onPressed: _import),
              ),
            )
          else
            Expanded(
              child: ContentList(
                key: const ValueKey('kube-contexts'),
                itemCount: items.length,
                itemBuilder: (context, index) {
                  final connection = items[index];
                  return _ContextRow(
                    connection: connection,
                    status: statuses[connection.id],
                    canConnect: service is KubeBrowserService,
                    onConnect: () => setState(() {
                      _selected = connection;
                      _selectedScope = scope;
                    }),
                    onAction: (action) {
                      if (action == 'share') {
                        showSharingKubeContext(context, contextId: connection.id);
                      } else if (action == 'history' && service is KubeHistoryService) {
                        showAppDialog<void>(
                          context,
                          // Browsing view: regular glass in the app palette.
                          builder: (_) =>
                              KubeHistoryDialog(connection: connection, service: service as KubeHistoryService),
                        );
                      } else if (action == 'delete') {
                        _delete(connection);
                      } else if (action == 'settings') {
                        _edit(connection);
                      } else if (action == 'export' && service is KubeInventoryManagementService) {
                        showAppDialog<void>(
                          context,
                          secure: true,
                          builder: (_) => KubeExportDialog(
                            connection: connection,
                            service: service as KubeInventoryManagementService,
                          ),
                        );
                      }
                    },
                    actions: [
                      GlassMenuItem(value: 'share', label: l.sharingTitle, icon: Icons.share_outlined),
                      if (service is KubeHistoryService)
                        GlassMenuItem(value: 'history', label: l.kubeHistory, icon: Icons.history),
                      if (service is KubeInventoryManagementService) ...[
                        GlassMenuItem(value: 'settings', label: l.kubeConnectionSettings, icon: Icons.tune_rounded),
                        GlassMenuItem(value: 'export', label: l.kubeExport, icon: Icons.ios_share_rounded),
                      ],
                      const GlassMenuDivider(),
                      GlassMenuItem(
                        value: 'delete',
                        label: l.commonDelete,
                        icon: Icons.delete_outline,
                        destructive: true,
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

/// One saved cluster context: colour/wheel glyph, name, PROD marker, server
/// and namespace, last explicit check, Connect and a context menu.
class _ContextRow extends StatelessWidget {
  const _ContextRow({
    required this.connection,
    required this.status,
    required this.canConnect,
    required this.onConnect,
    required this.onAction,
    required this.actions,
  });
  final KubeConnection connection;
  final KubeConnectionStatus? status;
  final bool canConnect;
  final VoidCallback onConnect;
  final ValueChanged<String> onAction;
  final List<GlassMenuEntry<String>> actions;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    final p = tokens.palette;
    final color = connection.color == null
        ? p.accent
        : Color(int.tryParse('FF${connection.color!.replaceFirst('#', '')}', radix: 16) ?? 0xFF4285F4);
    final (statusText, statusColor) = switch (status?.state) {
      null => (l.kubeStatusUnknown, p.tertiary),
      KubeConnectionState.checking => (l.kubeStatusChecking, p.warning),
      KubeConnectionState.available => (l.kubeStatusAvailable, p.success),
      KubeConnectionState.forbidden => (l.kubeStatusForbidden, p.danger),
      KubeConnectionState.failed => (l.kubeStatusFailed, p.danger),
    };
    final details = [
      Uri.tryParse(connection.server)?.authority ?? connection.server,
      if (connection.namespace.isNotEmpty) connection.namespace,
      ...connection.tags,
    ].where((part) => part.isNotEmpty).join(' · ');
    return GlassInteractive(
      key: ValueKey('kube-saved-${connection.id}'),
      semanticLabel: connection.name,
      onPressed: canConnect ? onConnect : null,
      builder: (context, state) => DecoratedBox(
        decoration: ShapeDecoration(
          color: state.hovered ? tokens.surfaces.fillHover : Colors.transparent,
          shape: GlassRadii.shape(tokens.radii.row),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
          child: Row(
            children: [
              KubeWheelIcon(size: 26, color: color),
              const SizedBox(width: 14),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Flexible(
                          child: Text(
                            connection.name,
                            overflow: TextOverflow.ellipsis,
                            style: tokens.typography.bodyEmph.copyWith(color: p.label),
                          ),
                        ),
                        if (connection.production) ...[
                          const SizedBox(width: 8),
                          const GlassBadge(
                            label: /* l10n-ignore: environment marker */ 'PROD',
                            tone: GlassTone.danger,
                            dense: true,
                          ),
                        ],
                      ],
                    ),
                    const SizedBox(height: 2),
                    Text(
                      details,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: tokens.typography.mono.copyWith(color: tokens.secondaryLabel, fontSize: 12),
                    ),
                    const SizedBox(height: 4),
                    Row(
                      children: [
                        StatusDot(color: statusColor, size: 7),
                        const SizedBox(width: 6),
                        Flexible(
                          child: Text(
                            statusText,
                            key: ValueKey('kube-status-${connection.id}'),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                          ),
                        ),
                        if (status?.serverVersion case final String version) ...[
                          const SizedBox(width: 6),
                          Text(version, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
                        ],
                      ],
                    ),
                  ],
                ),
              ),
              if (canConnect)
                MediaQuery.sizeOf(context).width < 600
                    ? GlassIconButton(
                        key: ValueKey('kube-connect-${connection.id}'),
                        icon: Icons.play_arrow_rounded,
                        tooltip: l.kubeConnect,
                        onPressed: onConnect,
                      )
                    : GlassButton.prominent(
                        key: ValueKey('kube-connect-${connection.id}'),
                        label: l.kubeConnect,
                        icon: Icons.play_arrow_rounded,
                        onPressed: onConnect,
                      ),
              const SizedBox(width: 6),
              GlassMenuButton<String>(
                key: ValueKey('kube-menu-${connection.id}'),
                entries: actions,
                onSelected: onAction,
                builder: (context, open) => GlassIconButton(
                  icon: Icons.more_horiz_rounded,
                  tooltip: MaterialLocalizations.of(context).moreButtonTooltip,
                  style: GlassIconButtonStyle.plain,
                  onPressed: open,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
