import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/containers/run_command_dialog.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/job_dialog.dart';
import 'package:consolecrypt/wslc/shared/ports.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

String stateText(AppLocalizations l10n, ContainerState state, String? raw) => switch (state) {
  ContainerState.created => l10n.dockerStateCreated,
  ContainerState.running => l10n.dockerStateRunning,
  ContainerState.exited => l10n.dockerForwardStopped,
  ContainerState.unknown => raw == null || raw.isEmpty ? l10n.dockerStateUnknown : l10n.wslcUnknown(raw),
};

Color stateColor(DeskPalette p, ContainerState state) => switch (state) {
  ContainerState.running => p.success,
  ContainerState.created => p.accentEnd,
  ContainerState.exited => p.faint,
  ContainerState.unknown => p.warning,
};

bool canStart(ContainerState s) => s == ContainerState.created || s == ContainerState.exited;
bool canStop(ContainerState s) => s == ContainerState.running;

/// Confirms stop/restart, runs the mutation and reports failures.
/// [known] avoids a second inspect when the details are already on screen.
Future<OperationResult?> runContainerAction(
  BuildContext context,
  DeskController controller,
  ContainerSummary row,
  String action, {
  ContainerDetails? known,
}) async {
  final notify = WslcApiScope.notifierOf(context);
  try {
    if (action != 'start') {
      final d = known ?? await WslcApiScope.of(context).inspectContainer(containerId: row.id, revealEnvironment: false);
      if (!context.mounted) return null;
      final accepted = await showWslcDialog<bool>(
        context: context,
        builder: (context) => _ConfirmDialog(row: row, action: action, details: d),
      );
      if (accepted != true) return null;
    }
    return await controller.operate(row.id, action);
  } catch (e) {
    notify(errorText(e), error: true);
    return null;
  }
}

class _ConfirmDialog extends StatelessWidget {
  const _ConfirmDialog({required this.row, required this.action, required this.details});
  final ContainerSummary row;
  final String action;
  final ContainerDetails details;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final stop = action == 'stop';
    final autoRemove = details.autoRemove;
    final blocked = action == 'restart' && autoRemove == true;
    return AlertDialog(
      icon: GradientIcon(
        stop ? Icons.stop_rounded : Icons.restart_alt_rounded,
        color: stop ? p.danger : p.warning,
        size: 44,
      ),
      title: Text(stop ? context.l10n.wslcStopThisContainer : context.l10n.wslcRestartThisContainer),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 440),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            InfoTile(label: context.l10n.kubeContainer, value: row.name ?? context.l10n.automationResultMissing),
            const SizedBox(height: 8),
            InfoTile(label: context.l10n.dockerContainerId, value: row.id, mono: true),
            const SizedBox(height: 14),
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(Icons.info_outline_rounded, size: 18, color: autoRemove == false ? p.muted : p.warning),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    autoRemove == true
                        ? context.l10n.wslcAutoRemoveIsEnabledTheRuntimeMayRemove
                        : autoRemove == false
                        ? context.l10n.wslcAutoRemoveIsDisabled
                        : context.l10n.wslcTheAutoRemoveSettingIsUnknownTheRuntime,
                    style: TextStyle(color: p.muted, fontSize: 13),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(context.l10n.commonCancel)),
        FilledButton(
          style: FilledButton.styleFrom(backgroundColor: stop ? p.danger : p.accent),
          onPressed: blocked ? null : () => Navigator.pop(context, true),
          child: Text(stop ? context.l10n.dockerActionStop : context.l10n.dockerActionRestart),
        ),
      ],
    );
  }
}

Future<void> openContainerTerminal(BuildContext context, String id, {String? workdir}) async {
  final notify = WslcApiScope.notifierOf(context);
  try {
    await WslcApiScope.of(context).openTerminal(containerId: id, workdir: workdir);
  } catch (e) {
    notify(errorText(e), error: true);
  }
}

Future<bool> confirmDanger(
  BuildContext context, {
  required String title,
  required String message,
  required String action,
  IconData icon = Icons.delete_outline_rounded,
}) async {
  final p = DeskPalette.of(context);
  final accepted = await showWslcDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      icon: GradientIcon(icon, color: p.danger, size: 44),
      title: Text(title),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 440, maxHeight: 320),
        child: SingleChildScrollView(
          child: Text(message, style: TextStyle(color: p.muted, height: 1.45)),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(context.l10n.commonCancel)),
        FilledButton(
          style: FilledButton.styleFrom(backgroundColor: p.danger),
          onPressed: () => Navigator.pop(context, true),
          child: Text(action),
        ),
      ],
    ),
  );
  return accepted == true;
}

/// Asks, removes (forcing a running container) and reports the outcome.
Future<bool> removeContainerFlow(BuildContext context, DeskController controller, ContainerSummary row) async {
  final running = row.state == ContainerState.running;
  final name = row.name ?? shortId(row.id);
  final ok = await confirmDanger(
    context,
    title: context.l10n.wslcRemoveThisContainer,
    message: context.l10n.wslcRemovalCannotBeUndoneNamedVolumesWillBeContainerActions(
      name,
      row.id,
      running ? context.l10n.wslcTheContainerIsRunningItWillBeForcibly : '',
    ),
    action: context.l10n.commonDelete,
  );
  if (!ok || !context.mounted) return false;
  final notify = WslcApiScope.notifierOf(context);
  final l10n = context.l10n;
  final error = await controller.removeContainer(row.id, force: running);
  notify(error ?? l10n.wslcContainerRemoved(name), error: error != null);
  return error == null;
}

/// "More" menu in the style of Docker Desktop.
class ContainerMenu extends StatelessWidget {
  const ContainerMenu({super.key, required this.row, required this.controller});
  final ContainerSummary row;
  final DeskController controller;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final running = row.state == ContainerState.running;
    final web = parsePorts(row.portsDisplay).where((port) => port.published && port.tcp).firstOrNull;
    final canRemove = controller.canMutate && !controller.busyIds.contains(row.id);
    PopupMenuItem<VoidCallback> item(IconData icon, String text, VoidCallback? action, {Color? color}) =>
        PopupMenuItem<VoidCallback>(
          value: action,
          enabled: action != null,
          height: 40,
          child: Row(
            children: [
              Icon(icon, size: 18, color: action == null ? null : color),
              const SizedBox(width: 12),
              Flexible(
                child: Text(
                  text,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: action == null ? null : color),
                ),
              ),
            ],
          ),
        );
    return PopupMenuButton<VoidCallback>(
      tooltip: context.l10n.wslcMoreActions,
      icon: const Icon(Icons.more_vert_rounded),
      position: PopupMenuPosition.under,
      onSelected: (action) => action(),
      itemBuilder: (_) => [
        item(
          Icons.open_in_browser_rounded,
          web == null ? context.l10n.wslcOpenInBrowser : context.l10n.wslcOpen(web.url('localhost')),
          running && web != null ? () => openPort(context, web, 'localhost') : null,
        ),
        item(
          Icons.terminal_rounded,
          context.l10n.terminalLocalOpen,
          running ? () => openContainerTerminal(context, row.id) : null,
        ),
        item(Icons.info_outline_rounded, context.l10n.wslcDetails, () => controller.openContainer(row.id)),
        item(Icons.code_rounded, context.l10n.wslcStartupCommand, () => showRunCommand(context, controller, row.id)),
        item(Icons.archive_outlined, context.l10n.wslcExportFilesystemAsTar, () => exportContainer(context, row)),
        const PopupMenuDivider(),
        item(Icons.content_copy_rounded, context.l10n.wslcCopyID, () => copyText(context, row.id)),
        if (row.name != null) item(Icons.badge_outlined, context.l10n.wslcCopyName, () => copyText(context, row.name!)),
        const PopupMenuDivider(),
        item(
          Icons.delete_outline_rounded,
          context.l10n.commonDelete,
          canRemove ? () => removeContainerFlow(context, controller, row) : null,
          color: p.danger,
        ),
      ],
    );
  }
}

/// `wslc export` of a container filesystem into a tar chosen in a save dialog.
Future<void> exportContainer(BuildContext context, ContainerSummary row) async {
  final name = row.name ?? row.id.substring(0, 12);
  final path = await WslcApiScope.of(
    context,
  ).pickSaveFile(title: context.l10n.wslcExportContainerFilesystem, fileName: '$name.tar', extensions: const ['tar']);
  if (path == null || !context.mounted) return;
  await showJobDialog(
    context,
    title: context.l10n.wslcExporting(name),
    icon: Icons.archive_outlined,
    start: () => WslcApiScope.of(context).startExport(containerId: row.id, output: path),
    successText: context.l10n.wslcSaved(path),
  );
}
