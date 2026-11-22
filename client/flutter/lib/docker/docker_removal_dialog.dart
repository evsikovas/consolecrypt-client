import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/docker_providers.dart';
import 'package:consolecrypt/docker/docker_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

String _resourceKind(AppLocalizations l, DockerRemovalKind kind) => switch (kind) {
  DockerRemovalKind.volume => l.dockerVolumes,
  DockerRemovalKind.network => l.dockerNetworks,
  DockerRemovalKind.bind => l.dockerRemovalBindMounts,
};

String dockerRemovalReason(AppLocalizations l, String reason) => switch (reason) {
  'shared' => l.dockerRemovalShared,
  'external' => l.dockerRemovalExternal,
  'default' => l.dockerRemovalDefaultNetwork,
  'bind' => l.dockerRemovalBindKept,
  'unknown' => l.dockerRemovalUnverified,
  'changed' => l.dockerRemovalChanged,
  'running' => l.dockerRemovalStopFirst,
  'auto_remove' => l.dockerRemovalAutoRemove,
  'not_found' => l.dockerRemovalMissing,
  'in_use' => l.dockerRemovalInUse,
  'cancelled' => l.dockerRemovalCancelled,
  'connection_failed' => l.dockerConnectionFailedTitle,
  'permission_denied' => l.dockerPermissionDeniedTitle,
  'not_running' => l.dockerNotRunningTitle,
  _ => l.dockerRequestFailed,
};

Future<DockerRemovalSelection?> showDockerRemovalPlan(BuildContext context, DockerRemovalPlan plan, String host) =>
    _removalDialog<DockerRemovalSelection>(
      context,
      builder: (_) => _DockerRemovalDialog(plan: plan, host: host),
    );

Future<T?> _removalDialog<T>(BuildContext context, {required WidgetBuilder builder}) => showAppDialog<T>(
  context,
  secure: true,
  barrierDismissible: false,
  builder: (_) => Consumer(
    builder: (context, ref, _) {
      ref.listen(dockerScopeProvider, (previous, next) {
        if (!next.unlocked || next.profileId != previous?.profileId) {
          final route = ModalRoute.of(context);
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (route != null && route.isActive) route.navigator?.removeRoute(route);
          });
        }
      });
      return builder(context);
    },
  ),
);

/// Null cancels, false removes the containers while keeping their volumes.
Future<bool?> confirmDockerProjectRemoval(
  BuildContext context,
  String host,
  String project,
  List<DockerContainer> targets,
) {
  var volumes = false;
  return _removalDialog<bool>(
    context,
    builder: (context) {
      final l = context.l10n;
      return StatefulBuilder(
        builder: (context, update) => GlassDialog(
          width: 560,
          title: l.dockerComposeRemoveTitle(project),
          icon: Icons.delete_outline_rounded,
          iconTone: GlassTone.danger,
          content: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              spacing: GlassSpacing.s8,
              children: [
                Text(host, style: Theme.of(context).textTheme.titleSmall),
                Text(l.dockerComposeBulkHelp(targets.length)),
                for (final target in targets) Text(target.name),
                Text(l.dockerComposeRemoveHelp),
                CheckboxListTile(
                  key: const ValueKey('docker-compose-remove-volumes'),
                  value: volumes,
                  onChanged: (value) => update(() => volumes = value == true),
                  title: Text(l.dockerComposeRemoveVolumes),
                  controlAffinity: ListTileControlAffinity.leading,
                  contentPadding: EdgeInsets.zero,
                ),
                if (volumes) InfoBanner(tone: BannerTone.danger, message: l.dockerRemovalVolumeWarning),
              ],
            ),
          ),
          primaryAction: GlassButton.destructive(
            key: const ValueKey('confirm-ok'),
            label: l.commonDelete,
            onPressed: () => closeDialog(context, volumes),
          ),
          secondaryActions: [
            GlassButton(
              key: const ValueKey('confirm-cancel'),
              label: l.commonCancel,
              autofocus: true,
              onPressed: () => closeDialog<bool>(context),
            ),
          ],
        ),
      );
    },
  );
}

Future<void> showDockerProjectRemovalIssues(BuildContext context, String summary, List<String> issues) =>
    _removalDialog<void>(
      context,
      builder: (context) => GlassDialog(
        width: 560,
        title: context.l10n.dockerRemovalResult,
        content: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            spacing: GlassSpacing.s8,
            children: [Text(summary), for (final issue in issues) Text(issue)],
          ),
        ),
        primaryAction: GlassButton(label: context.l10n.commonClose, onPressed: () => closeDialog<void>(context)),
      ),
    );

class _DockerRemovalDialog extends StatefulWidget {
  const _DockerRemovalDialog({required this.plan, required this.host});
  final DockerRemovalPlan plan;
  final String host;

  @override
  State<_DockerRemovalDialog> createState() => _DockerRemovalDialogState();
}

class _DockerRemovalDialogState extends State<_DockerRemovalDialog> {
  final _volumes = <String>{}, _networks = <String>{};
  bool _stop = false;

  @override
  Widget build(BuildContext context) {
    final plan = widget.plan;
    final l = context.l10n;
    final canRemove = plan.name.isNotEmpty && (!plan.running || _stop);
    return GlassDialog(
      width: 560,
      title: l.dockerRemoveTitle(plan.name),
      icon: Icons.delete_outline_rounded,
      iconTone: GlassTone.danger,
      content: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: GlassSpacing.s8,
          children: [
            if (widget.host.isNotEmpty) Text(widget.host, style: Theme.of(context).textTheme.titleSmall),
            Text(l.dockerRemoveHelp),
            if (plan.running)
              CheckboxListTile(
                key: const ValueKey('docker-remove-stop'),
                value: _stop,
                contentPadding: EdgeInsets.zero,
                controlAffinity: ListTileControlAffinity.leading,
                title: Text(l.dockerRemovalStopFirst),
                onChanged: (value) => setState(() => _stop = value == true),
              ),
            for (final kind in DockerRemovalKind.values)
              if (plan.resources.any((r) => r.kind == kind)) ...[
                Padding(
                  padding: const EdgeInsets.only(top: GlassSpacing.s8),
                  child: Semantics(
                    header: true,
                    child: Text(_resourceKind(l, kind), style: Theme.of(context).textTheme.titleSmall),
                  ),
                ),
                for (final resource in plan.resources.where((r) => r.kind == kind))
                  if (resource.eligible && resource.kind != DockerRemovalKind.bind)
                    _selectable(resource)
                  else
                    ListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      leading: const Icon(Icons.lock_outline_rounded, size: 18),
                      title: Text(resource.name),
                      subtitle: Text(
                        dockerRemovalReason(l, resource.kind == DockerRemovalKind.bind ? 'bind' : resource.reason),
                      ),
                    ),
              ],
            if (_volumes.isNotEmpty)
              InfoBanner(
                key: const ValueKey('docker-remove-volume-warning'),
                tone: BannerTone.danger,
                message: l.dockerRemovalVolumeWarning,
              ),
          ],
        ),
      ),
      primaryAction: GlassButton.destructive(
        key: const ValueKey('confirm-ok'),
        label: l.dockerActionRemove,
        onPressed: canRemove
            ? () => closeDialog(
                context,
                DockerRemovalSelection(
                  planId: plan.planId,
                  stopRunning: _stop,
                  volumes: _volumes.toList(),
                  networks: _networks.toList(),
                  confirmName: plan.name,
                ),
              )
            : null,
      ),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('confirm-cancel'),
          label: l.commonCancel,
          autofocus: true,
          onPressed: () => closeDialog<DockerRemovalSelection>(context),
        ),
      ],
    );
  }

  Widget _selectable(DockerRemovalResource resource) {
    final selected = resource.kind == DockerRemovalKind.volume ? _volumes : _networks;
    return CheckboxListTile(
      key: ValueKey('docker-remove-${resource.kind.name}-${resource.id}'),
      value: selected.contains(resource.id),
      dense: true,
      contentPadding: EdgeInsets.zero,
      title: Text(resource.name),
      controlAffinity: ListTileControlAffinity.leading,
      onChanged: (value) => setState(() {
        if (value == true) {
          selected.add(resource.id);
        } else {
          selected.remove(resource.id);
        }
      }),
    );
  }
}

Future<void> showDockerRemovalResult(BuildContext context, DockerRemovalResult result) => _removalDialog<void>(
  context,
  builder: (dialog) {
    final l = dialog.l10n;
    return GlassDialog(
      width: 560,
      title: l.dockerRemovalResult,
      content: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: GlassSpacing.s8,
          children: [
            Text(
              result.containerRemoved
                  ? l.dockerRemovalRemoved
                  : result.containerStopped
                  ? l.dockerRemovalStopped
                  : l.dockerRemovalKept,
            ),
            if (result.error != null) Text(dockerRemovalReason(l, result.error!)),
            for (final row in result.resources)
              ListTile(
                contentPadding: EdgeInsets.zero,
                dense: true,
                title: Text(row.name),
                leading: Tooltip(
                  message: _resourceKind(l, row.kind),
                  child: Icon(switch (row.kind) {
                    DockerRemovalKind.volume => Icons.storage_outlined,
                    DockerRemovalKind.network => Icons.hub_outlined,
                    DockerRemovalKind.bind => Icons.folder_outlined,
                  }, size: 18),
                ),
                subtitle: Text(switch (row.status) {
                  DockerRemovalStatus.removed => l.dockerRemovalResourceRemoved,
                  DockerRemovalStatus.skipped => l.dockerRemovalResourceSkipped(dockerRemovalReason(l, row.reason)),
                  DockerRemovalStatus.failed => l.dockerRemovalResourceFailed(dockerRemovalReason(l, row.reason)),
                }),
              ),
          ],
        ),
      ),
      primaryAction: GlassButton(
        key: const ValueKey('docker-removal-result-close'),
        label: l.commonClose,
        onPressed: () => closeDialog<void>(dialog),
      ),
    );
  },
);
