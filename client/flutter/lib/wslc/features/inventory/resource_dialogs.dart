import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/job_dialog.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

const builtinNetworks = {'bridge', 'host', 'none'};

/// Runs a short management command, reports the outcome and re-reads [sections].
Future<bool> runAndRefresh(
  BuildContext context,
  DeskController controller,
  Future<void> Function() action,
  String done,
  Set<DeskSection> sections,
) async {
  final messenger = ScaffoldMessenger.of(context);
  try {
    await action();
    messenger.showSnackBar(SnackBar(content: Text(done)));
    await controller.refreshSections(sections);
    return true;
  } catch (e) {
    messenger.showSnackBar(SnackBar(content: Text(errorText(e))));
    return false;
  }
}

List<EnvVar>? _pairs(String text) {
  final out = <EnvVar>[];
  for (final line in text.split('\n')) {
    final t = line.trim();
    if (t.isEmpty) continue;
    final i = t.indexOf('=');
    if (i <= 0) return null;
    out.add(EnvVar(key: t.substring(0, i), value: t.substring(i + 1)));
  }
  return out;
}

String? _text(TextEditingController c) => c.text.trim().isEmpty ? null : c.text.trim();

class _Form extends StatelessWidget {
  const _Form({
    required this.icon,
    required this.title,
    required this.children,
    required this.action,
    required this.onSubmit,
    this.error,
  });
  final IconData icon;
  final String title;
  final List<Widget> children;
  final String action;
  final VoidCallback onSubmit;
  final String? error;

  @override
  Widget build(BuildContext context) => AlertDialog(
    icon: GradientIcon(icon, size: 44),
    title: Text(title),
    content: SizedBox(
      width: 480,
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            for (final c in children) ...[c, const SizedBox(height: 10)],
            if (error != null) Text(error!, style: TextStyle(color: DeskPalette.of(context).danger, fontSize: 12.5)),
          ],
        ),
      ),
    ),
    actions: [
      TextButton(onPressed: () => Navigator.pop(context), child: const Text('Отмена')),
      FilledButton(onPressed: onSubmit, child: Text(action)),
    ],
  );
}

Future<void> createVolume(BuildContext context, DeskController c) async {
  final spec = await showWslcDialog<VolumeSpec>(context: context, builder: (_) => const _VolumeForm());
  if (spec == null || !context.mounted) return;
  await runAndRefresh(context, c, () => WslcApiScope.of(context).createVolume(spec: spec), 'Том ${spec.name} создан', {
    DeskSection.storage,
  });
}

class _VolumeForm extends StatefulWidget {
  const _VolumeForm();
  @override
  State<_VolumeForm> createState() => _VolumeFormState();
}

class _VolumeFormState extends State<_VolumeForm> {
  final name = TextEditingController();
  final opts = TextEditingController();
  String driver = 'guest';
  String? error;

  @override
  void dispose() {
    name.dispose();
    opts.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => _Form(
    icon: Icons.storage_rounded,
    title: 'Новый том',
    action: 'Создать',
    error: error,
    onSubmit: () {
      final options = _pairs(opts.text);
      if (name.text.trim().isEmpty || options == null) {
        setState(
          () => error = name.text.trim().isEmpty ? 'Укажите имя тома' : 'Параметры — по одному KEY=VALUE в строке',
        );
        return;
      }
      Navigator.pop(
        context,
        VolumeSpec(
          name: name.text.trim(),
          driver: driver == 'guest' ? null : driver,
          options: options,
          labels: const [],
        ),
      );
    },
    children: [
      TextField(
        controller: name,
        autofocus: true,
        decoration: const InputDecoration(hintText: 'Имя тома, например app-data'),
      ),
      DropdownButtonFormField<String>(
        initialValue: driver,
        isExpanded: true,
        items: const [
          DropdownMenuItem(value: 'guest', child: Text('guest — в файловой системе ВМ (по умолчанию)')),
          DropdownMenuItem(value: 'vhd', child: Text('vhd — отдельный виртуальный диск')),
        ],
        onChanged: (v) => setState(() => driver = v ?? 'guest'),
      ),
      TextField(
        controller: opts,
        minLines: 1,
        maxLines: 3,
        decoration: const InputDecoration(hintText: 'Параметры драйвера KEY=VALUE (необязательно)'),
      ),
    ],
  );
}

Future<void> createNetwork(BuildContext context, DeskController c) async {
  final spec = await showWslcDialog<NetworkSpec>(context: context, builder: (_) => const _NetworkForm());
  if (spec == null || !context.mounted) return;
  await runAndRefresh(
    context,
    c,
    () => WslcApiScope.of(context).createNetwork(spec: spec),
    'Сеть ${spec.name} создана',
    {DeskSection.networks},
  );
}

class _NetworkForm extends StatefulWidget {
  const _NetworkForm();
  @override
  State<_NetworkForm> createState() => _NetworkFormState();
}

class _NetworkFormState extends State<_NetworkForm> {
  final name = TextEditingController();
  final subnet = TextEditingController();
  final gateway = TextEditingController();
  final range = TextEditingController();
  bool internal = false;
  String? error;

  @override
  void dispose() {
    for (final t in [name, subnet, gateway, range]) {
      t.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => _Form(
    icon: Icons.hub_rounded,
    title: 'Новая сеть',
    action: 'Создать',
    error: error,
    onSubmit: () {
      if (name.text.trim().isEmpty) {
        setState(() => error = 'Укажите имя сети');
        return;
      }
      Navigator.pop(
        context,
        NetworkSpec(
          name: name.text.trim(),
          subnet: _text(subnet),
          gateway: _text(gateway),
          ipRange: _text(range),
          internal: internal,
          labels: const [],
        ),
      );
    },
    children: [
      TextField(
        controller: name,
        autofocus: true,
        decoration: const InputDecoration(hintText: 'Имя сети, например backend'),
      ),
      TextField(
        controller: subnet,
        decoration: const InputDecoration(hintText: 'Подсеть CIDR, например 10.231.0.0/24 (необязательно)'),
      ),
      TextField(
        controller: gateway,
        decoration: const InputDecoration(hintText: 'Шлюз, например 10.231.0.1'),
      ),
      TextField(
        controller: range,
        decoration: const InputDecoration(hintText: 'Диапазон адресов контейнеров CIDR (необязательно)'),
      ),
      CheckboxListTile(
        contentPadding: EdgeInsets.zero,
        value: internal,
        onChanged: (v) => setState(() => internal = v ?? false),
        title: const Text('Внутренняя сеть без выхода наружу'),
      ),
    ],
  );
}

/// Connects a container to a network; either side may be preselected.
Future<void> connectToNetwork(BuildContext context, DeskController c, {String? network, String? containerId}) async {
  final result = await showWslcDialog<(String, String, String?, String?)>(
    context: context,
    builder: (_) => _ConnectForm(controller: c, network: network, containerId: containerId),
  );
  if (result == null || !context.mounted) return;
  final (net, id, alias, ip) = result;
  await runAndRefresh(
    context,
    c,
    () => WslcApiScope.of(context).connectNetwork(networkName: net, containerId: id, alias: alias, ip: ip),
    'Контейнер подключён к сети $net',
    {DeskSection.containers, DeskSection.networks},
  );
}

class _ConnectForm extends StatefulWidget {
  const _ConnectForm({required this.controller, this.network, this.containerId});
  final DeskController controller;
  final String? network;
  final String? containerId;
  @override
  State<_ConnectForm> createState() => _ConnectFormState();
}

class _ConnectFormState extends State<_ConnectForm> {
  late String? network = widget.network;
  late String? container = widget.containerId;
  final alias = TextEditingController();
  final ip = TextEditingController();
  String? error;

  @override
  void dispose() {
    alias.dispose();
    ip.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    return _Form(
      icon: Icons.settings_ethernet_rounded,
      title: 'Подключить к сети',
      action: 'Подключить',
      error: error,
      onSubmit: () {
        if (network == null || container == null) {
          setState(() => error = 'Выберите сеть и контейнер');
          return;
        }
        Navigator.pop(context, (network!, container!, _text(alias), _text(ip)));
      },
      children: [
        DropdownButtonFormField<String>(
          initialValue: network,
          isExpanded: true,
          hint: const Text('Сеть'),
          items: [
            for (final n in c.networks)
              if (n.name != null && n.name != 'host' && n.name != 'none')
                DropdownMenuItem(value: n.name, child: Text(n.name!)),
          ],
          onChanged: widget.network == null ? (v) => setState(() => network = v) : null,
        ),
        DropdownButtonFormField<String>(
          initialValue: container,
          isExpanded: true,
          hint: const Text('Контейнер'),
          items: [
            for (final r in c.containers)
              DropdownMenuItem(
                value: r.id,
                child: Text(r.name ?? r.id.substring(0, 12), overflow: TextOverflow.ellipsis),
              ),
          ],
          onChanged: widget.containerId == null ? (v) => setState(() => container = v) : null,
        ),
        TextField(
          controller: alias,
          decoration: const InputDecoration(hintText: 'Псевдоним в сети (необязательно)'),
        ),
        TextField(
          controller: ip,
          decoration: const InputDecoration(hintText: 'Фиксированный IPv4 из подсети (необязательно)'),
        ),
      ],
    );
  }
}

Future<void> disconnectFromNetwork(BuildContext context, DeskController c, String network, String containerId) =>
    runAndRefresh(
      context,
      c,
      () => WslcApiScope.of(context).disconnectNetwork(networkName: network, containerId: containerId),
      'Контейнер отключён от сети $network',
      {DeskSection.containers, DeskSection.networks},
    );

String _stamp() {
  final t = DateTime.now();
  String two(int v) => v.toString().padLeft(2, '0');
  return '${t.year}${two(t.month)}${two(t.day)}-${two(t.hour)}${two(t.minute)}';
}

/// Packs a named volume into a .tar.gz on the Windows disk via a helper container.
Future<void> backupVolume(BuildContext context, String volume) async {
  final path = await WslcApiScope.of(
    context,
  ).pickSaveFile(title: 'Бэкап тома $volume', fileName: '$volume-${_stamp()}.tar.gz', extensions: const ['gz', 'tgz']);
  if (path == null || !context.mounted) return;
  await showJobDialog(
    context,
    title: 'Бэкап тома $volume',
    icon: Icons.backup_outlined,
    start: () => WslcApiScope.of(context).startVolumeBackup(volumeName: volume, output: path),
    successText: 'Бэкап сохранён: $path',
  );
}

/// Restores a .tar.gz into [volume]; asks for a new volume name when none is given.
Future<void> restoreVolume(BuildContext context, DeskController c, {String? volume}) async {
  final path = await WslcApiScope.of(context).pickFile(title: 'Архив тома (.tar.gz)');
  if (path == null || !context.mounted) return;
  final choice = await showWslcDialog<(String, bool)>(
    context: context,
    builder: (_) => _RestoreForm(volume: volume),
  );
  if (choice == null || !context.mounted) return;
  final (target, clear) = choice;
  final ok = await showJobDialog(
    context,
    title: 'Восстановление тома $target',
    icon: Icons.settings_backup_restore_rounded,
    start: () => WslcApiScope.of(context).startVolumeRestore(volumeName: target, input: path, clear: clear),
    successText: 'Том $target восстановлен',
  );
  if (ok) await c.refreshSections({DeskSection.storage});
}

class _RestoreForm extends StatefulWidget {
  const _RestoreForm({this.volume});
  final String? volume;
  @override
  State<_RestoreForm> createState() => _RestoreFormState();
}

class _RestoreFormState extends State<_RestoreForm> {
  late final name = TextEditingController(text: widget.volume ?? '');
  bool clear = false;
  String? error;

  @override
  void dispose() {
    name.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => _Form(
    icon: Icons.settings_backup_restore_rounded,
    title: widget.volume == null ? 'Восстановить в новый том' : 'Восстановить том',
    action: 'Восстановить',
    error: error,
    onSubmit: () {
      if (name.text.trim().isEmpty) {
        setState(() => error = 'Укажите имя тома');
        return;
      }
      Navigator.pop(context, (name.text.trim(), clear));
    },
    children: [
      TextField(
        controller: name,
        readOnly: widget.volume != null,
        decoration: const InputDecoration(hintText: 'Имя тома (несуществующий будет создан)'),
      ),
      CheckboxListTile(
        contentPadding: EdgeInsets.zero,
        value: clear,
        onChanged: (v) => setState(() => clear = v ?? false),
        title: const Text('Очистить том перед восстановлением'),
        subtitle: const Text('Иначе файлы из архива добавятся поверх текущих'),
      ),
    ],
  );
}

class VolumeMenu extends StatelessWidget {
  const VolumeMenu({super.key, required this.volume, required this.controller});
  final String volume;
  final DeskController controller;

  @override
  Widget build(BuildContext context) => PopupMenuButton<VoidCallback>(
    tooltip: 'Ещё действия',
    icon: const Icon(Icons.more_vert_rounded),
    enabled: controller.canMutate,
    position: PopupMenuPosition.under,
    onSelected: (action) => action(),
    itemBuilder: (_) => [
      PopupMenuItem(value: () => backupVolume(context, volume), child: const Text('Бэкап в архив…')),
      PopupMenuItem(
        value: () => restoreVolume(context, controller, volume: volume),
        child: const Text('Восстановить из архива…'),
      ),
    ],
  );
}
