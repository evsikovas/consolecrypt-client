import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/containers/run_wizard.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/job_dialog.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

/// Asks for an image reference and pulls it with a live log.
Future<void> pullImage(BuildContext context, DeskController controller) async {
  final image = await showWslcDialog<String>(context: context, builder: (_) => const _PullForm());
  if (image == null || image.isEmpty || !context.mounted) return;
  final ok = await showJobDialog(
    context,
    title: 'Загрузка $image',
    icon: Icons.cloud_download_rounded,
    start: () => WslcApiScope.of(context).startPull(image: image),
    successText: 'Образ загружен',
  );
  if (ok) await controller.refreshSections({DeskSection.images});
}

class _PullForm extends StatefulWidget {
  const _PullForm();
  @override
  State<_PullForm> createState() => _PullFormState();
}

class _PullFormState extends State<_PullForm> {
  final field = TextEditingController();

  @override
  void dispose() {
    field.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    icon: const GradientIcon(Icons.cloud_download_rounded, size: 44),
    title: const Text('Загрузить образ'),
    content: SizedBox(
      width: 460,
      child: TextField(
        controller: field,
        autofocus: true,
        decoration: const InputDecoration(
          hintText: 'mcr.microsoft.com/azurelinux/base/nginx:1.30',
          prefixIcon: Icon(Icons.layers_rounded, size: 18),
        ),
        onSubmitted: (v) => Navigator.pop(context, v.trim()),
      ),
    ),
    actions: [
      TextButton(onPressed: () => Navigator.pop(context), child: const Text('Отмена')),
      FilledButton(onPressed: () => Navigator.pop(context, field.text.trim()), child: const Text('Загрузить')),
    ],
  );
}

/// Collects `wslc build` options and runs the build with a live log.
Future<void> buildImage(BuildContext context, DeskController controller) async {
  final spec = await showWslcDialog<BuildSpec>(context: context, builder: (_) => const _BuildForm());
  if (spec == null || !context.mounted) return;
  final ok = await showJobDialog(
    context,
    title: 'Сборка ${spec.tags.isEmpty ? 'образа' : spec.tags.join(', ')}',
    icon: Icons.construction_rounded,
    start: () => WslcApiScope.of(context).startBuild(spec: spec),
    successText: 'Образ собран',
  );
  if (ok) await controller.refreshSections({DeskSection.images});
}

class _BuildForm extends StatefulWidget {
  const _BuildForm();
  @override
  State<_BuildForm> createState() => _BuildFormState();
}

class _BuildFormState extends State<_BuildForm> {
  late final api = WslcApiScope.of(context);

  final context_ = TextEditingController();
  final dockerfile = TextEditingController();
  final tags = TextEditingController();
  final buildArgs = TextEditingController();
  final target = TextEditingController();
  bool noCache = false;
  bool pull = false;
  String? error;

  @override
  void dispose() {
    for (final c in [context_, dockerfile, tags, buildArgs, target]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> pickContext() async {
    final path = await api.pickFolder(title: 'Папка с контекстом сборки');
    if (path != null && mounted) setState(() => context_.text = path);
  }

  Future<void> pickDockerfile() async {
    final path = await api.pickFile(title: 'Dockerfile');
    if (path != null && mounted) setState(() => dockerfile.text = path);
  }

  void submit() {
    final args = <EnvVar>[];
    for (final line in buildArgs.text.split('\n')) {
      final t = line.trim();
      if (t.isEmpty) continue;
      final i = t.indexOf('=');
      if (i <= 0) {
        setState(() => error = 'Аргумент сборки «$t» должен быть в виде KEY=VALUE');
        return;
      }
      args.add(EnvVar(key: t.substring(0, i), value: t.substring(i + 1)));
    }
    if (context_.text.trim().isEmpty) {
      setState(() => error = 'Выберите папку с контекстом сборки');
      return;
    }
    Navigator.pop(
      context,
      BuildSpec(
        context: context_.text.trim(),
        dockerfile: dockerfile.text.trim().isEmpty ? null : dockerfile.text.trim(),
        tags: [
          for (final t in tags.text.split(RegExp(r'[,\s]+')))
            if (t.isNotEmpty) t,
        ],
        buildArgs: args,
        target: target.text.trim().isEmpty ? null : target.text.trim(),
        noCache: noCache,
        pull: pull,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    Widget label(String text) => Padding(
      padding: const EdgeInsets.only(bottom: 6, top: 12),
      child: Text(
        text,
        style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600, color: p.muted),
      ),
    );
    return AlertDialog(
      icon: const GradientIcon(Icons.construction_rounded, size: 44),
      title: const Text('Собрать образ'),
      content: SizedBox(
        width: 560,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              label('Папка с контекстом сборки'),
              TextField(
                controller: context_,
                decoration: InputDecoration(
                  hintText: r'C:\projects\app',
                  suffixIcon: IconButton(
                    tooltip: 'Выбрать папку',
                    onPressed: pickContext,
                    icon: const Icon(Icons.folder_open_rounded, size: 18),
                  ),
                ),
              ),
              label('Dockerfile (необязательно, по умолчанию из контекста)'),
              TextField(
                controller: dockerfile,
                decoration: InputDecoration(
                  hintText: r'C:\projects\app\Dockerfile',
                  suffixIcon: IconButton(
                    tooltip: 'Выбрать файл',
                    onPressed: pickDockerfile,
                    icon: const Icon(Icons.description_outlined, size: 18),
                  ),
                ),
              ),
              label('Теги через запятую'),
              TextField(
                controller: tags,
                decoration: const InputDecoration(hintText: 'my/app:1.0, my/app:latest'),
              ),
              label('Аргументы сборки, по одному KEY=VALUE в строке'),
              TextField(
                controller: buildArgs,
                minLines: 2,
                maxLines: 4,
                style: const TextStyle(fontFamily: monoFont, fontFamilyFallback: monoFallback),
                decoration: const InputDecoration(hintText: 'VERSION=1.0'),
              ),
              label('Целевая стадия (необязательно)'),
              TextField(
                controller: target,
                decoration: const InputDecoration(hintText: 'runtime'),
              ),
              const SizedBox(height: 8),
              Wrap(
                spacing: 16,
                children: [
                  FilterChip(
                    label: const Text('Без кэша'),
                    selected: noCache,
                    onSelected: (v) => setState(() => noCache = v),
                  ),
                  FilterChip(
                    label: const Text('Обновить базовые образы'),
                    selected: pull,
                    onSelected: (v) => setState(() => pull = v),
                  ),
                ],
              ),
              if (error != null) ...[
                const SizedBox(height: 10),
                Text(error!, style: TextStyle(color: p.danger, fontSize: 12.5)),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Отмена')),
        FilledButton(onPressed: submit, child: const Text('Собрать')),
      ],
    );
  }
}

/// `repository:tag` when the image has one, otherwise its full ID.
String imageRef(ImageSummary r) => r.repository != null && r.tag != null ? '${r.repository}:${r.tag}' : r.id;

/// Tag, push and "new container from this image".
class ImageMenu extends StatelessWidget {
  const ImageMenu({super.key, required this.image, required this.controller});
  final ImageSummary image;
  final DeskController controller;

  @override
  Widget build(BuildContext context) {
    final ref = imageRef(image);
    final named = image.repository != null && image.tag != null;
    return PopupMenuButton<VoidCallback>(
      tooltip: 'Ещё действия',
      icon: const Icon(Icons.more_vert_rounded),
      position: PopupMenuPosition.under,
      enabled: controller.canMutate,
      onSelected: (action) => action(),
      itemBuilder: (_) => [
        PopupMenuItem(
          value: () async {
            final id = await showRunWizard(
              context,
              controller,
              initial: RunSpec(
                image: ref,
                ports: const [],
                mounts: const [],
                env: const [],
                command: const [],
                autoRemove: false,
                start: true,
              ),
            );
            if (id != null) controller.openContainer(id);
          },
          child: const Text('Создать контейнер…'),
        ),
        PopupMenuItem(value: () => tagImage(context, controller, ref), child: const Text('Добавить тег…')),
        PopupMenuItem(enabled: named, value: () => pushImage(context, ref), child: const Text('Отправить в реестр…')),
        PopupMenuItem(value: () => saveImage(context, ref), child: const Text('Сохранить в tar…')),
      ],
    );
  }
}

Future<void> tagImage(BuildContext context, DeskController controller, String source) async {
  final target = await showWslcDialog<String>(
    context: context,
    builder: (_) => _TextPrompt(
      icon: Icons.sell_outlined,
      title: 'Новый тег для $source',
      hint: 'registry.example.com/team/app:1.0',
      initial: source.startsWith('sha256:') ? '' : '${source.split(':').first}:',
      action: 'Добавить тег',
    ),
  );
  if (target == null || target.isEmpty || !context.mounted) return;
  final messenger = ScaffoldMessenger.of(context);
  try {
    await WslcApiScope.of(context).tagImage(source: source, target: target);
    messenger.showSnackBar(SnackBar(content: Text('Добавлен тег $target')));
    await controller.refreshSections({DeskSection.images});
  } catch (e) {
    messenger.showSnackBar(SnackBar(content: Text(errorText(e))));
  }
}

Future<void> pushImage(BuildContext context, String image) async {
  final ok = await showWslcDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      icon: const GradientIcon(Icons.cloud_upload_rounded, size: 44),
      title: const Text('Отправить образ?'),
      content: Text(
        '$image\n\nОбраз будет отправлен в реестр из его имени. Если нужна авторизация, '
        'сначала войдите через «Реестры».',
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Отмена')),
        FilledButton(onPressed: () => Navigator.pop(context, true), child: const Text('Отправить')),
      ],
    ),
  );
  if (ok != true || !context.mounted) return;
  await showJobDialog(
    context,
    title: 'Отправка $image',
    icon: Icons.cloud_upload_rounded,
    start: () => WslcApiScope.of(context).startPush(image: image),
    successText: 'Образ отправлен',
  );
}

class _TextPrompt extends StatefulWidget {
  const _TextPrompt({
    required this.icon,
    required this.title,
    required this.hint,
    required this.initial,
    required this.action,
  });
  final IconData icon;
  final String title;
  final String hint;
  final String initial;
  final String action;
  @override
  State<_TextPrompt> createState() => _TextPromptState();
}

class _TextPromptState extends State<_TextPrompt> {
  late final field = TextEditingController(text: widget.initial);

  @override
  void dispose() {
    field.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    icon: GradientIcon(widget.icon, size: 44),
    title: Text(widget.title),
    content: SizedBox(
      width: 480,
      child: TextField(
        controller: field,
        autofocus: true,
        decoration: InputDecoration(hintText: widget.hint),
        onSubmitted: (v) => Navigator.pop(context, v.trim()),
      ),
    ),
    actions: [
      TextButton(onPressed: () => Navigator.pop(context), child: const Text('Отмена')),
      FilledButton(onPressed: () => Navigator.pop(context, field.text.trim()), child: Text(widget.action)),
    ],
  );
}

/// Login with username and password or token (sent via stdin), logout per server.
Future<void> manageRegistries(BuildContext context, DeskController controller) => showWslcDialog<void>(
  context: context,
  builder: (_) => _RegistriesDialog(controller: controller),
);

class _RegistriesDialog extends StatefulWidget {
  const _RegistriesDialog({required this.controller});
  final DeskController controller;
  @override
  State<_RegistriesDialog> createState() => _RegistriesDialogState();
}

class _RegistriesDialogState extends State<_RegistriesDialog> {
  late final api = WslcApiScope.of(context);

  final server = TextEditingController();
  final user = TextEditingController();
  final password = TextEditingController();
  bool busy = false;
  String? message;
  bool failed = false;

  @override
  void dispose() {
    server.dispose();
    user.dispose();
    password.dispose();
    super.dispose();
  }

  String? get serverOrNull => server.text.trim().isEmpty ? null : server.text.trim();

  Future<void> login() async {
    setState(() {
      busy = true;
      message = null;
    });
    try {
      await api.registryLogin(server: serverOrNull, username: user.text.trim(), password: password.text);
      widget.controller.registries.add(serverOrNull ?? 'по умолчанию');
      password.clear();
      setState(() {
        failed = false;
        message = 'Вход выполнен';
      });
    } catch (e) {
      setState(() {
        failed = true;
        message = errorText(e);
      });
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Future<void> logout(String name) async {
    setState(() => busy = true);
    try {
      await api.registryLogout(server: name == 'по умолчанию' ? null : name);
      widget.controller.registries.remove(name);
      setState(() {
        failed = false;
        message = 'Выход из $name выполнен';
      });
    } catch (e) {
      setState(() {
        failed = true;
        message = errorText(e);
      });
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final known = widget.controller.registries.toList()..sort();
    return AlertDialog(
      icon: const GradientIcon(Icons.vpn_key_outlined, size: 44),
      title: const Text('Реестры'),
      content: SizedBox(
        width: 480,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: server,
              decoration: const InputDecoration(
                hintText: 'Сервер, например ghcr.io (пусто — по умолчанию)',
                prefixIcon: Icon(Icons.dns_outlined, size: 18),
              ),
            ),
            const SizedBox(height: 10),
            TextField(
              controller: user,
              decoration: const InputDecoration(
                hintText: 'Имя пользователя',
                prefixIcon: Icon(Icons.person_outline_rounded, size: 18),
              ),
            ),
            const SizedBox(height: 10),
            TextField(
              controller: password,
              obscureText: true,
              decoration: const InputDecoration(
                hintText: 'Пароль или токен доступа',
                prefixIcon: Icon(Icons.password_rounded, size: 18),
              ),
              onSubmitted: (_) => busy ? null : login(),
            ),
            const SizedBox(height: 8),
            Text(
              'Пароль передаётся WSLc через stdin и нигде не сохраняется приложением.',
              style: TextStyle(color: p.faint, fontSize: 12),
            ),
            if (message != null) ...[
              const SizedBox(height: 10),
              WslcSelectableText(
                message!,
                maxLines: 4,
                style: TextStyle(color: failed ? p.danger : p.success, fontSize: 12.5),
              ),
            ],
            if (known.isNotEmpty) ...[
              const SizedBox(height: 14),
              Text(
                'Вход в этом запуске',
                style: TextStyle(color: p.muted, fontWeight: FontWeight.w600),
              ),
              for (final name in known)
                Row(
                  children: [
                    Expanded(child: Text(name)),
                    TextButton(onPressed: busy ? null : () => logout(name), child: const Text('Выйти')),
                  ],
                ),
            ],
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Закрыть')),
        FilledButton(
          onPressed: busy || user.text.trim().isEmpty && password.text.isEmpty ? null : login,
          child: busy
              ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
              : const Text('Войти'),
        ),
      ],
    );
  }
}

String _tarName(String ref) {
  final base = ref.startsWith('sha256:') ? ref.substring(7, 19) : ref.split('/').last.replaceAll(':', '_');
  return '$base.tar';
}

/// `wslc save` of one image into a tar chosen in a save dialog.
Future<void> saveImage(BuildContext context, String ref) async {
  final path = await WslcApiScope.of(context)
      .pickSaveFile(title: 'Сохранить образ в tar', fileName: _tarName(ref), extensions: const ['tar']);
  if (path == null || !context.mounted) return;
  await showJobDialog(
    context,
    title: 'Сохранение $ref',
    icon: Icons.archive_outlined,
    start: () => WslcApiScope.of(context).startSave(images: [ref], output: path),
    successText: 'Сохранено: $path',
  );
}

/// `wslc load` of images from a tar made by save.
Future<void> loadImages(BuildContext context, DeskController controller) async {
  final path = await WslcApiScope.of(context).pickFile(title: 'Tar-архив с образами (wslc save / docker save)');
  if (path == null || !context.mounted) return;
  final ok = await showJobDialog(
    context,
    title: 'Загрузка образов из ${path.split(RegExp(r'[\\/]')).last}',
    icon: Icons.unarchive_outlined,
    start: () => WslcApiScope.of(context).startLoad(input: path),
    successText: 'Образы загружены',
  );
  if (ok) await controller.refreshSections({DeskSection.images});
}

/// `wslc import` of a filesystem tar (from export) as a new image.
Future<void> importFilesystem(BuildContext context, DeskController controller) async {
  final path = await WslcApiScope.of(context).pickFile(title: 'Tar-архив файловой системы (wslc export)');
  if (path == null || !context.mounted) return;
  final name = await showWslcDialog<String>(
    context: context,
    builder: (_) => const _TextPrompt(
      icon: Icons.drive_folder_upload_outlined,
      title: 'Имя нового образа',
      hint: 'my/imported:1 (необязательно)',
      initial: '',
      action: 'Импортировать',
    ),
  );
  if (name == null || !context.mounted) return;
  final ok = await showJobDialog(
    context,
    title: 'Импорт ${path.split(RegExp(r'[\\/]')).last}',
    icon: Icons.drive_folder_upload_outlined,
    start: () => WslcApiScope.of(context).startImport(input: path, image: name.isEmpty ? null : name),
    successText: 'Образ создан',
  );
  if (ok) await controller.refreshSections({DeskSection.images});
}

/// Toolbar menu for archive operations on the Images page.
class ImageArchiveMenu extends StatelessWidget {
  const ImageArchiveMenu({super.key, required this.controller});
  final DeskController controller;

  @override
  Widget build(BuildContext context) => PopupMenuButton<VoidCallback>(
    tooltip: 'Импорт из архивов',
    icon: const Icon(Icons.more_horiz_rounded),
    enabled: controller.canMutate,
    position: PopupMenuPosition.under,
    onSelected: (action) => action(),
    itemBuilder: (_) => [
      PopupMenuItem(value: () => loadImages(context, controller), child: const Text('Загрузить образы из tar (load)…')),
      PopupMenuItem(
        value: () => importFilesystem(context, controller),
        child: const Text('Импорт файловой системы из tar (import)…'),
      ),
    ],
  );
}
