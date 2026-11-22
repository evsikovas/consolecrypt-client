import 'package:consolecrypt/core/l10n/l10n.dart';
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
    title: context.l10n.wslcPulling(image),
    icon: Icons.cloud_download_rounded,
    start: () => WslcApiScope.of(context).startPull(image: image),
    successText: context.l10n.wslcImageDownloaded,
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
    title: Text(context.l10n.wslcPullImage),
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
      TextButton(onPressed: () => Navigator.pop(context), child: Text(context.l10n.commonCancel)),
      FilledButton(onPressed: () => Navigator.pop(context, field.text.trim()), child: Text(context.l10n.wslcDownload)),
    ],
  );
}

/// Collects `wslc build` options and runs the build with a live log.
Future<void> buildImage(BuildContext context, DeskController controller) async {
  final spec = await showWslcDialog<BuildSpec>(context: context, builder: (_) => const _BuildForm());
  if (spec == null || !context.mounted) return;
  final ok = await showJobDialog(
    context,
    title: context.l10n.wslcBuilding(spec.tags.isEmpty ? context.l10n.wslcImageImageDialogs : spec.tags.join(', ')),
    icon: Icons.construction_rounded,
    start: () => WslcApiScope.of(context).startBuild(spec: spec),
    successText: context.l10n.wslcImageBuilt,
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
    final path = await api.pickFolder(title: context.l10n.wslcBuildContextDirectory);
    if (path != null && mounted) setState(() => context_.text = path);
  }

  Future<void> pickDockerfile() async {
    final path = await api.pickFile(title: 'Dockerfile'); // l10n-ignore: standard build file name
    if (path != null && mounted) setState(() => dockerfile.text = path);
  }

  void submit() {
    final args = <EnvVar>[];
    for (final line in buildArgs.text.split('\n')) {
      final t = line.trim();
      if (t.isEmpty) continue;
      final i = t.indexOf('=');
      if (i <= 0) {
        setState(() => error = context.l10n.wslcBuildArgumentMustUseKEYVALUE(t));
        return;
      }
      args.add(EnvVar(key: t.substring(0, i), value: t.substring(i + 1)));
    }
    if (context_.text.trim().isEmpty) {
      setState(() => error = context.l10n.wslcChooseTheBuildContextDirectory);
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
      title: Text(context.l10n.wslcBuildImage),
      content: SizedBox(
        width: 560,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              label(context.l10n.wslcBuildContextDirectory),
              TextField(
                controller: context_,
                decoration: InputDecoration(
                  hintText: r'C:\projects\app', // l10n-ignore: Windows directory path example
                  suffixIcon: IconButton(
                    tooltip: context.l10n.wslcChooseDirectory,
                    onPressed: pickContext,
                    icon: const Icon(Icons.folder_open_rounded, size: 18),
                  ),
                ),
              ),
              label(context.l10n.wslcDockerfileOptionalDefaultsToTheOneInThe),
              TextField(
                controller: dockerfile,
                decoration: InputDecoration(
                  hintText: r'C:\projects\app\Dockerfile', // l10n-ignore: Windows file path example
                  suffixIcon: IconButton(
                    tooltip: context.l10n.kubeChooseFile,
                    onPressed: pickDockerfile,
                    icon: const Icon(Icons.description_outlined, size: 18),
                  ),
                ),
              ),
              label(context.l10n.wslcCommaSeparatedTags),
              TextField(
                controller: tags,
                decoration: const InputDecoration(
                  hintText: 'my/app:1.0, my/app:latest', // l10n-ignore: image tag examples
                ),
              ),
              label(context.l10n.wslcBuildArgumentsOneKEYVALUEPerLine),
              TextField(
                controller: buildArgs,
                minLines: 2,
                maxLines: 4,
                style: const TextStyle(fontFamily: monoFont, fontFamilyFallback: monoFallback),
                decoration: const InputDecoration(hintText: 'VERSION=1.0'), // l10n-ignore: build argument example
              ),
              label(context.l10n.wslcTargetStageOptional),
              TextField(
                controller: target,
                decoration: const InputDecoration(hintText: 'runtime'),
              ),
              const SizedBox(height: 8),
              Wrap(
                spacing: 16,
                children: [
                  FilterChip(
                    label: Text(context.l10n.wslcDisableCache),
                    selected: noCache,
                    onSelected: (v) => setState(() => noCache = v),
                  ),
                  FilterChip(
                    label: Text(context.l10n.wslcPullUpdatedBaseImages),
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
        TextButton(onPressed: () => Navigator.pop(context), child: Text(context.l10n.commonCancel)),
        FilledButton(onPressed: submit, child: Text(context.l10n.wslcBuildImageDialogs)),
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
      tooltip: context.l10n.wslcMoreActions,
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
          child: Text(context.l10n.wslcCreateContainer),
        ),
        PopupMenuItem(value: () => tagImage(context, controller, ref), child: Text(context.l10n.wslcAddTag)),
        PopupMenuItem(
          enabled: named,
          value: () => pushImage(context, ref),
          child: Text(context.l10n.wslcPushToRegistry),
        ),
        PopupMenuItem(value: () => saveImage(context, ref), child: Text(context.l10n.wslcSaveAsTar)),
      ],
    );
  }
}

Future<void> tagImage(BuildContext context, DeskController controller, String source) async {
  final target = await showWslcDialog<String>(
    context: context,
    builder: (_) => _TextPrompt(
      icon: Icons.sell_outlined,
      title: context.l10n.wslcNewTagFor(source),
      hint: 'registry.example.com/team/app:1.0',
      initial: source.startsWith('sha256:') ? '' : '${source.split(':').first}:',
      action: context.l10n.tagsAdd,
    ),
  );
  if (target == null || target.isEmpty || !context.mounted) return;
  final notify = WslcApiScope.notifierOf(context);
  final l10n = context.l10n;
  try {
    await WslcApiScope.of(context).tagImage(source: source, target: target);
    notify(l10n.wslcAddedTag(target));
    await controller.refreshSections({DeskSection.images});
  } catch (e) {
    notify(errorText(e), error: true);
  }
}

Future<void> pushImage(BuildContext context, String image) async {
  final ok = await showWslcDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      icon: const GradientIcon(Icons.cloud_upload_rounded, size: 44),
      title: Text(context.l10n.wslcPushImage),
      content: Text(context.l10n.wslcTheImageWillBePushedToTheRegistry(image)),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context, false), child: Text(context.l10n.commonCancel)),
        FilledButton(onPressed: () => Navigator.pop(context, true), child: Text(context.l10n.aiChatSend)),
      ],
    ),
  );
  if (ok != true || !context.mounted) return;
  await showJobDialog(
    context,
    title: context.l10n.wslcPushing(image),
    icon: Icons.cloud_upload_rounded,
    start: () => WslcApiScope.of(context).startPush(image: image),
    successText: context.l10n.wslcImagePushed,
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
      TextButton(onPressed: () => Navigator.pop(context), child: Text(context.l10n.commonCancel)),
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
  bool get canSignIn => !busy && user.text.trim().isNotEmpty && password.text.isNotEmpty;

  Future<void> login() async {
    final registry = serverOrNull;
    final controller = widget.controller;
    setState(() {
      busy = true;
      message = null;
    });
    try {
      await api.registryLogin(server: registry, username: user.text.trim(), password: password.text);
      controller.registries.add(registry);
      if (!mounted) return;
      password.clear();
      setState(() {
        failed = false;
        message = context.l10n.wslcSignedIn;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        failed = true;
        message = errorText(e);
      });
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Future<void> logout(String? registry) async {
    final controller = widget.controller;
    setState(() => busy = true);
    try {
      await api.registryLogout(server: registry);
      controller.registries.remove(registry);
      if (!mounted) return;
      setState(() {
        failed = false;
        message = context.l10n.wslcSignedOutOf(registry ?? context.l10n.glassModeStandard);
      });
    } catch (e) {
      if (!mounted) return;
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
    final known = widget.controller.registries.toList()..sort((a, b) => (a ?? '').compareTo(b ?? ''));
    return AlertDialog(
      icon: const GradientIcon(Icons.vpn_key_outlined, size: 44),
      title: Text(context.l10n.wslcRegistries),
      content: SizedBox(
        width: 480,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TextField(
              controller: server,
              decoration: InputDecoration(
                hintText: context.l10n.wslcServerEGGhcrIoLeaveBlankFor,
                prefixIcon: const Icon(Icons.dns_outlined, size: 18),
              ),
            ),
            const SizedBox(height: 10),
            TextField(
              controller: user,
              onChanged: (_) => setState(() {}),
              decoration: InputDecoration(
                hintText: context.l10n.credentialsDetailsUsername,
                prefixIcon: const Icon(Icons.person_outline_rounded, size: 18),
              ),
            ),
            const SizedBox(height: 10),
            TextField(
              controller: password,
              onChanged: (_) => setState(() {}),
              obscureText: true,
              decoration: InputDecoration(
                hintText: context.l10n.wslcPasswordOrAccessToken,
                prefixIcon: const Icon(Icons.password_rounded, size: 18),
              ),
              onSubmitted: (_) => canSignIn ? login() : null,
            ),
            const SizedBox(height: 8),
            Text(
              context.l10n.wslcThePasswordIsPassedToWSLcThroughStdin,
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
                context.l10n.wslcSignedInDuringThisSession,
                style: TextStyle(color: p.muted, fontWeight: FontWeight.w600),
              ),
              for (final name in known)
                Row(
                  children: [
                    Expanded(child: Text(name ?? context.l10n.glassModeStandard)),
                    TextButton(onPressed: busy ? null : () => logout(name), child: Text(context.l10n.settingsSignOut)),
                  ],
                ),
            ],
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(context.l10n.commonClose)),
        FilledButton(
          onPressed: canSignIn ? login : null,
          child: busy
              ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
              : Text(context.l10n.loginSubmitSignIn),
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
      .pickSaveFile(title: context.l10n.wslcSaveImageAsTar, fileName: _tarName(ref), extensions: const ['tar']);
  if (path == null || !context.mounted) return;
  await showJobDialog(
    context,
    title: context.l10n.wslcSaving(ref),
    icon: Icons.archive_outlined,
    start: () => WslcApiScope.of(context).startSave(images: [ref], output: path),
    successText: context.l10n.wslcSaved(path),
  );
}

/// `wslc load` of images from a tar made by save.
Future<void> loadImages(BuildContext context, DeskController controller) async {
  final path = await WslcApiScope.of(context).pickFile(title: context.l10n.wslcImageTarArchiveWslcSaveDockerSave);
  if (path == null || !context.mounted) return;
  final ok = await showJobDialog(
    context,
    title: context.l10n.wslcLoadingImagesFrom(path.split(RegExp(r'[\\/]')).last),
    icon: Icons.unarchive_outlined,
    start: () => WslcApiScope.of(context).startLoad(input: path),
    successText: context.l10n.wslcImagesLoaded,
  );
  if (ok) await controller.refreshSections({DeskSection.images});
}

/// `wslc import` of a filesystem tar (from export) as a new image.
Future<void> importFilesystem(BuildContext context, DeskController controller) async {
  final path = await WslcApiScope.of(context).pickFile(title: context.l10n.wslcFilesystemTarArchiveWslcExport);
  if (path == null || !context.mounted) return;
  final name = await showWslcDialog<String>(
    context: context,
    builder: (_) => _TextPrompt(
      icon: Icons.drive_folder_upload_outlined,
      title: context.l10n.wslcNewImageName,
      hint: context.l10n.wslcMyImportedOptional,
      initial: '',
      action: context.l10n.credentialImportAction,
    ),
  );
  if (name == null || !context.mounted) return;
  final ok = await showJobDialog(
    context,
    title: context.l10n.wslcImporting(path.split(RegExp(r'[\\/]')).last),
    icon: Icons.drive_folder_upload_outlined,
    start: () => WslcApiScope.of(context).startImport(input: path, image: name.isEmpty ? null : name),
    successText: context.l10n.wslcImageCreated,
  );
  if (ok) await controller.refreshSections({DeskSection.images});
}

/// Toolbar menu for archive operations on the Images page.
class ImageArchiveMenu extends StatelessWidget {
  const ImageArchiveMenu({super.key, required this.controller});
  final DeskController controller;

  @override
  Widget build(BuildContext context) => PopupMenuButton<VoidCallback>(
    tooltip: context.l10n.wslcImportFromArchives,
    icon: const Icon(Icons.more_horiz_rounded),
    enabled: controller.canMutate,
    position: PopupMenuPosition.under,
    onSelected: (action) => action(),
    itemBuilder: (_) => [
      PopupMenuItem(value: () => loadImages(context, controller), child: Text(context.l10n.wslcLoadImagesFromTarLoad)),
      PopupMenuItem(
        value: () => importFilesystem(context, controller),
        child: Text(context.l10n.wslcImportFilesystemFromTarImport),
      ),
    ],
  );
}
