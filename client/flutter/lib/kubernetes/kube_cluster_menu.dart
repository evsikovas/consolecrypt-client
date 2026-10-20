import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_connection_editor.dart';
import 'package:consolecrypt/kubernetes/kube_export_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_inventory_service.dart';
import 'package:consolecrypt/kubernetes/kube_organization_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/sharing/sharing_kubernetes.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Colour labels offered for clusters (shared with the connection editor).
const kubeClusterColors = ['#4285F4', '#22A06B', '#E56910', '#AE2E24', '#8270DB', '#1D9BB2'];

Color? kubeClusterColor(String? value) {
  if (value == null) return null;
  final rgb = int.tryParse(value.replaceFirst('#', ''), radix: 16);
  return rgb == null ? null : Color(0xFF000000 | rgb);
}

/// Context menu of a cluster row: rename, properties, production marker,
/// colour label and delete. [current] clusters cannot be deleted while open.
Future<void> showKubeClusterMenu(
  BuildContext context,
  WidgetRef ref,
  KubeConnection connection,
  Offset position, {
  required bool current,
}) async {
  final l = context.l10n;
  final service = ref.read(kubeServiceProvider);
  final scope = ref.read(kubeScopeProvider);
  final editable = service is KubeInventoryManagementService;
  final action = await showGlassMenu<String>(
    context: context,
    anchor: position & const Size(1, 1),
    entries: [
      GlassMenuItem(
        key: const ValueKey('kube-cluster-rename'),
        value: 'rename',
        label: l.commonRename,
        icon: Icons.drive_file_rename_outline,
        enabled: editable,
      ),
      GlassMenuItem(
        key: const ValueKey('kube-cluster-properties'),
        value: 'properties',
        label: l.kubeClusterProperties,
        icon: Icons.tune_rounded,
        enabled: editable,
      ),
      const GlassMenuDivider(),
      GlassMenuItem(
        key: const ValueKey('kube-cluster-share'),
        value: 'share',
        label: l.sharingTitle,
        icon: Icons.share_outlined,
      ),
      if (editable)
        GlassMenuItem(
          key: const ValueKey('kube-cluster-export'),
          value: 'export',
          label: l.kubeExport,
          icon: Icons.ios_share_rounded,
        ),
      const GlassMenuDivider(),
      GlassMenuItem(
        key: const ValueKey('kube-cluster-production'),
        value: 'production',
        label: connection.production ? l.kubeMarkAsTest : l.kubeMarkAsProduction,
        icon: connection.production ? Icons.science_outlined : Icons.shield_outlined,
        enabled: editable,
      ),
      if (service is KubeOrganizationService)
        GlassMenuItem(
          key: const ValueKey('kube-cluster-color'),
          value: 'color',
          label: l.kubeClusterColorMenu,
          icon: Icons.palette_outlined,
        ),
      const GlassMenuDivider(),
      GlassMenuItem(
        key: const ValueKey('kube-cluster-delete'),
        value: 'delete',
        label: l.commonDelete,
        icon: Icons.delete_outline,
        destructive: true,
        enabled: !current,
      ),
    ],
  );
  if (action == null || !context.mounted || !scope.unlocked || !identical(scope, ref.read(kubeScopeProvider))) return;

  Future<void> update({String? name, bool? production}) async {
    if (service is! KubeInventoryManagementService) return;
    await runWithFeedback(
      context,
      () => (service as KubeInventoryManagementService).updateContext(
        connection.id,
        name: name ?? connection.name,
        namespace: connection.namespace,
        production: production ?? connection.production,
        allowExecInProd: connection.allowExecInProd,
        allowPortForwardInProd: connection.allowPortForwardInProd,
        recordEvents: connection.recordEvents,
      ),
    );
    if (identical(scope, ref.read(kubeScopeProvider))) ref.invalidate(kubeConnectionsProvider);
  }

  switch (action) {
    case 'rename':
      final name = (await showTextInputDialog(context, title: l.commonRename, initial: connection.name))?.trim();
      if (name == null || name.isEmpty || name.length > 253 || name == connection.name || !context.mounted) return;
      await update(name: name);
    case 'share':
      await showSharingKubeContext(context, contextId: connection.id);
    case 'export':
      await showAppDialog<void>(
        context,
        secure: true,
        builder: (_) => KubeExportDialog(connection: connection, service: service as KubeInventoryManagementService),
      );
    case 'properties':
      await showAppDialog<bool>(
        context,
        barrierDismissible: false,
        builder: (_) => KubeConnectionEditor(connection: connection),
      );
    case 'production':
      if (connection.production) {
        final approved = await showConfirmDialog(
          context,
          title: connection.name,
          message: l.kubeMarkAsTestConfirm,
          confirmLabel: l.kubeMarkAsTest,
          destructive: true,
        );
        if (!approved || !context.mounted) return;
      }
      await update(production: !connection.production);
    case 'color':
      final picked = await showGlassPopover<(String?,)>(
        context: context,
        anchor: position & const Size(1, 1),
        builder: (context) => _ColorPicker(selected: connection.color),
      );
      if (picked == null || picked.$1 == connection.color || !context.mounted) return;
      await runWithFeedback(
        context,
        () => (service as KubeOrganizationService).saveKubeClusterLabels(
          connection.id,
          tags: connection.tags,
          color: picked.$1,
        ),
      );
      if (identical(scope, ref.read(kubeScopeProvider))) ref.invalidate(kubeConnectionsProvider);
    case 'delete':
      final approved = await showConfirmDialog(
        context,
        title: connection.name,
        message: l.kubeDeleteHelp,
        confirmLabel: l.commonDelete,
        destructive: true,
        secure: true,
      );
      if (!approved || !context.mounted) return;
      await runWithFeedback(context, () => service.deleteContext(connection.id));
      if (identical(scope, ref.read(kubeScopeProvider))) ref.invalidate(kubeConnectionsProvider);
  }
}

class _ColorPicker extends StatelessWidget {
  const _ColorPicker({required this.selected});
  final String? selected;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final l = context.l10n;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (final value in <String?>[null, ...kubeClusterColors])
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 3),
            child: Tooltip(
              message: value ?? l.kubeNoColor,
              child: GlassInteractive(
                key: ValueKey('kube-cluster-color-${value ?? 'none'}'),
                semanticLabel: value ?? l.kubeNoColor,
                selected: selected == value,
                onPressed: () => Navigator.of(context).pop((value,)),
                builder: (context, state) => Container(
                  width: 24,
                  height: 24,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: kubeClusterColor(value) ?? Colors.transparent,
                    border: Border.all(
                      color: selected == value || state.hovered ? tokens.palette.label : tokens.surfaces.separator,
                      width: selected == value ? 2.5 : 1,
                    ),
                  ),
                  child: value == null ? Icon(Icons.block_rounded, size: 13, color: tokens.secondaryLabel) : null,
                ),
              ),
            ),
          ),
      ],
    );
  }
}

/// Opens the cluster menu without awaiting it (gesture callbacks).
void openKubeClusterMenu(
  BuildContext context,
  WidgetRef ref,
  KubeConnection connection,
  Offset position, {
  required bool current,
}) => unawaited(showKubeClusterMenu(context, ref, connection, position, current: current));
