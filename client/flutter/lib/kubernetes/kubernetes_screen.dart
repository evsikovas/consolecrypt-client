import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_import_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubernetesScreen extends ConsumerStatefulWidget {
  const KubernetesScreen({super.key});
  @override
  ConsumerState<KubernetesScreen> createState() => _KubernetesScreenState();
}

class _KubernetesScreenState extends ConsumerState<KubernetesScreen> {
  String _query = '';

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
    final state = ref.watch(kubeConnectionsProvider(scope));
    final items =
        state.value
            ?.where(
              (connection) =>
                  '${connection.name} ${connection.clusterName} ${connection.server} ${connection.namespace}'
                      .toLowerCase()
                      .contains(_query.toLowerCase()),
            )
            .toList() ??
        [];
    return PageScaffold(
      title: l.kubeTitle,
      subtitle: l.kubeSubtitle,
      actions: [
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
                return Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: ContentSurface(
                    child: ListTile(
                      key: ValueKey('kube-saved-${connection.id}'),
                      title: Row(
                        children: [
                          Expanded(child: Text(connection.name)),
                          if (connection.production)
                            const Chip(label: Text('PROD')), // l10n-ignore: standard environment marker
                        ],
                      ),
                      subtitle: Text('${connection.server}\n${l.kubeNamespace}: ${connection.namespace}'),
                      isThreeLine: true,
                      trailing: GlassIconButton(
                        tooltip: l.commonDelete,
                        icon: Icons.delete_outline,
                        onPressed: () => _delete(connection),
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
