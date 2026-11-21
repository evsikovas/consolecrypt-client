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

String stateText(ContainerState state, String? raw) => switch (state) {
  ContainerState.created => 'Создан',
  ContainerState.running => 'Запущен',
  ContainerState.exited => 'Остановлен',
  ContainerState.unknown => 'Неизвестно (${raw ?? 'нет данных'})',
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
  final messenger = ScaffoldMessenger.of(context);
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
    messenger.showSnackBar(SnackBar(content: Text(errorText(e))));
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
      title: Text(stop ? 'Остановить контейнер?' : 'Перезапустить контейнер?'),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 440),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            InfoTile(label: 'Контейнер', value: row.name ?? 'нет данных'),
            const SizedBox(height: 8),
            InfoTile(label: 'ID', value: row.id, mono: true),
            const SizedBox(height: 14),
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(Icons.info_outline_rounded, size: 18, color: autoRemove == false ? p.muted : p.warning),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    autoRemove == true
                        ? 'Auto-remove включён: runtime может удалить контейнер после остановки.'
                        : autoRemove == false
                        ? 'Auto-remove выключен.'
                        : 'Флаг auto-remove неизвестен. После остановки runtime может удалить контейнер.',
                    style: TextStyle(color: p.muted, fontSize: 13),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Отмена')),
        FilledButton(
          style: FilledButton.styleFrom(backgroundColor: stop ? p.danger : p.accent),
          onPressed: blocked ? null : () => Navigator.pop(context, true),
          child: Text(stop ? 'Остановить' : 'Перезапустить'),
        ),
      ],
    );
  }
}

Future<void> openContainerTerminal(BuildContext context, String id, {String? workdir}) async {
  final messenger = ScaffoldMessenger.maybeOf(context);
  try {
    await WslcApiScope.of(context).openTerminal(containerId: id, workdir: workdir);
  } catch (e) {
    messenger?.showSnackBar(SnackBar(content: Text(errorText(e))));
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
        TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Отмена')),
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
    title: 'Удалить контейнер?',
    message:
        '$name\n${row.id}\n\n'
        '${running ? 'Контейнер запущен: он будет принудительно остановлен и удалён. ' : ''}'
        'Удаление нельзя отменить. Именованные тома сохранятся.',
    action: 'Удалить',
  );
  if (!ok || !context.mounted) return false;
  final messenger = ScaffoldMessenger.of(context);
  final error = await controller.removeContainer(row.id, force: running);
  messenger.showSnackBar(SnackBar(content: Text(error ?? 'Контейнер $name удалён')));
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
      tooltip: 'Ещё действия',
      icon: const Icon(Icons.more_vert_rounded),
      position: PopupMenuPosition.under,
      onSelected: (action) => action(),
      itemBuilder: (_) => [
        item(
          Icons.open_in_browser_rounded,
          web == null ? 'Открыть в браузере' : 'Открыть ${web.url('localhost')}',
          running && web != null ? () => openPort(context, web, 'localhost') : null,
        ),
        item(Icons.terminal_rounded, 'Открыть терминал', running ? () => openContainerTerminal(context, row.id) : null),
        item(Icons.info_outline_rounded, 'Подробности', () => controller.openContainer(row.id)),
        item(Icons.code_rounded, 'Команда запуска…', () => showRunCommand(context, controller, row.id)),
        item(Icons.archive_outlined, 'Экспорт файловой системы в tar…', () => exportContainer(context, row)),
        const PopupMenuDivider(),
        item(Icons.content_copy_rounded, 'Копировать ID', () => copyText(context, row.id)),
        if (row.name != null) item(Icons.badge_outlined, 'Копировать имя', () => copyText(context, row.name!)),
        const PopupMenuDivider(),
        item(
          Icons.delete_outline_rounded,
          'Удалить',
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
  final path = await WslcApiScope.of(context)
      .pickSaveFile(title: 'Экспорт файловой системы контейнера', fileName: '$name.tar', extensions: const ['tar']);
  if (path == null || !context.mounted) return;
  await showJobDialog(
    context,
    title: 'Экспорт $name',
    icon: Icons.archive_outlined,
    start: () => WslcApiScope.of(context).startExport(containerId: row.id, output: path),
    successText: 'Сохранено: $path',
  );
}
