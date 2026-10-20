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
      secure: true,
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
    if (_selected != null && identical(_selectedScope, scope) && scope.unlocked && service is KubeBrowserService) {
      return KubeBrowser(
        key: ValueKey((_selectedScope, _selected!.id)),
        connection: _selected!,
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
          TextField(
            key: const ValueKey('kube-search'),
            decoration: InputDecoration(labelText: l.commonSearch, prefixIcon: const Icon(Icons.search)),
            onChanged: (value) => setState(() => _query = value),
          ),
          if (tags.isNotEmpty)
            SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: Row(
                children: [
                  for (final tag in tags)
                    Padding(
                      padding: const EdgeInsets.only(right: 6),
                      child: FilterChip(
                        label: Text(tag),
                        selected: _tag == tag,
                        onSelected: (selected) => setState(() => _tag = selected ? tag : null),
                      ),
                    ),
                ],
              ),
            ),
          const SizedBox(height: 12),
          if (state.isLoading) const LinearProgressIndicator(),
          if (state.hasError) Text(l.kubeLoadFailed),
          if (!state.isLoading && !state.hasError && (state.value?.isEmpty ?? true)) Text(l.kubeEmpty),
          Expanded(
            child: ListView.builder(
              key: const ValueKey('kube-contexts'),
              itemCount: items.length,
              itemBuilder: (context, index) {
                final connection = items[index];
                final status = statuses[connection.id];
                final statusText = switch (status?.state) {
                  null => l.kubeStatusUnknown,
                  KubeConnectionState.checking => l.kubeStatusChecking,
                  KubeConnectionState.available => l.kubeStatusAvailable,
                  KubeConnectionState.forbidden => l.kubeStatusForbidden,
                  KubeConnectionState.failed => l.kubeStatusFailed,
                };
                return Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: ContentSurface(
                    child: ListTile(
                      key: ValueKey('kube-saved-${connection.id}'),
                      leading: connection.color == null
                          ? null
                          : Icon(
                              Icons.circle,
                              size: 12,
                              color: Color(
                                int.tryParse('FF${connection.color!.replaceFirst('#', '')}', radix: 16) ?? 0xFF4285F4,
                              ),
                            ),
                      title: Row(
                        children: [
                          Expanded(child: Text(connection.name)),
                          if (connection.production)
                            const Chip(label: Text('PROD')), // l10n-ignore: standard environment marker
                        ],
                      ),
                      subtitle: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Text(
                            // l10n-ignore: Saved cluster metadata and already localized namespace label.
                            /* l10n-ignore: Saved cluster metadata and already localized namespace label. */ '${connection.clusterName} · ${connection.server}\n${l.kubeNamespace}: ${connection.namespace}${connection.tags.isEmpty ? '' : ' · ${connection.tags.join(', ')}'}',
                          ),
                          Wrap(
                            spacing: 6,
                            crossAxisAlignment: WrapCrossAlignment.center,
                            children: [
                              Icon(switch (status?.state) {
                                null => Icons.help_outline,
                                KubeConnectionState.checking => Icons.hourglass_empty,
                                KubeConnectionState.available => Icons.check_circle_outline,
                                KubeConnectionState.forbidden => Icons.lock_outline,
                                KubeConnectionState.failed => Icons.error_outline,
                              }, size: 14),
                              Text(statusText, key: ValueKey('kube-status-${connection.id}')),
                              if (status?.serverVersion case final String version) Text(version),
                            ],
                          ),
                        ],
                      ),
                      isThreeLine: true,
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (service is KubeBrowserService)
                            GlassIconButton(
                              key: ValueKey('kube-connect-${connection.id}'),
                              tooltip: l.kubeConnect,
                              icon: Icons.play_arrow_outlined,
                              onPressed: () => setState(() {
                                _selected = connection;
                                _selectedScope = scope;
                              }),
                            ),
                          PopupMenuButton<String>(
                            key: ValueKey('kube-menu-${connection.id}'),
                            onSelected: (action) {
                              if (action == 'share') {
                                showSharingKubeContext(context, contextId: connection.id);
                              } else if (action == 'history' && service is KubeHistoryService) {
                                showAppDialog<void>(
                                  context,
                                  secure: true,
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
                            itemBuilder: (_) => [
                              PopupMenuItem(value: 'share', child: Text(l.sharingTitle)),
                              if (service is KubeHistoryService)
                                PopupMenuItem(value: 'history', child: Text(l.kubeHistory)),
                              if (service is KubeInventoryManagementService) ...[
                                PopupMenuItem(value: 'settings', child: Text(l.kubeConnectionSettings)),
                                PopupMenuItem(value: 'export', child: Text(l.kubeExport)),
                              ],
                              PopupMenuItem(value: 'delete', child: Text(l.commonDelete)),
                            ],
                          ),
                        ],
                      ),
                    ),
                  ),
                );
              },
            ),
          ),
        ],
      ),
    );
  }
}
