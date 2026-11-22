import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/compose/compose_dialog.dart';
import 'package:consolecrypt/wslc/features/containers/templates.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/ports.dart';
import 'package:consolecrypt/wslc/shared/shell_words.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

/// Opens the "new container" wizard. Returns the created container ID.
Future<String?> showRunWizard(BuildContext context, DeskController controller, {RunSpec? initial}) async {
  // Open the next dialog from the workspace context, which still owns its API
  // lease. Navigator.context is above that scope and cannot open WSLc dialogs.
  final result = await showWslcDialog<Object>(
    context: context,
    barrierDismissible: false,
    containsSecrets: false,
    builder: (_) => _WizardHost(controller: controller, initial: initial),
  );
  if (result == _WizardAction.compose && context.mounted) {
    await showComposeDialog(context, controller);
    return null;
  }
  return result is String ? result : null;
}

enum _WizardAction { compose }

/// Rebuilds the wizard from scratch when a template is loaded into it.
class _WizardHost extends StatefulWidget {
  const _WizardHost({required this.controller, this.initial});
  final DeskController controller;
  final RunSpec? initial;
  @override
  State<_WizardHost> createState() => _WizardHostState();
}

class _WizardHostState extends State<_WizardHost> {
  late RunSpec? spec = widget.initial;
  int generation = 0;

  @override
  Widget build(BuildContext context) => RunWizard(
    key: ValueKey(generation),
    controller: widget.controller,
    initial: spec,
    onTemplate: (s) => setState(() {
      spec = s;
      generation++;
    }),
  );
}

class _PortRow {
  _PortRow([PortSpec? p])
    : host = TextEditingController(text: p == null ? '' : '${p.hostPort}'),
      container = TextEditingController(text: p == null ? '' : '${p.containerPort}'),
      allInterfaces = p?.hostIp == '0.0.0.0',
      udp = p?.udp ?? false;
  final TextEditingController host;
  final TextEditingController container;
  bool allInterfaces;
  bool udp;

  PortSpec spec() => PortSpec(
    hostIp: allInterfaces ? '0.0.0.0' : null,
    hostPort: int.tryParse(host.text.trim()) ?? 0,
    containerPort: int.tryParse(container.text.trim()) ?? 0,
    udp: udp,
  );
}

class _MountRow {
  _MountRow([MountSpec? m])
    : volume = m?.volume ?? false,
      source = TextEditingController(text: m?.source ?? ''),
      target = TextEditingController(text: m?.target ?? ''),
      readOnly = m?.readOnly ?? false;
  bool volume;
  final TextEditingController source;
  final TextEditingController target;
  bool readOnly;

  MountSpec spec() =>
      MountSpec(volume: volume, source: source.text.trim(), target: target.text.trim(), readOnly: readOnly);
}

class _EnvRow {
  _EnvRow([EnvVar? e])
    : key = TextEditingController(text: e?.key ?? ''),
      value = TextEditingController(text: e?.value ?? '');
  final TextEditingController key;
  final TextEditingController value;
  bool visible = false;
}

class RunWizard extends StatefulWidget {
  const RunWizard({super.key, required this.controller, this.initial, this.onTemplate});
  final DeskController controller;
  final RunSpec? initial;

  /// Loads a template into a fresh form.
  final ValueChanged<RunSpec>? onTemplate;
  @override
  State<RunWizard> createState() => _RunWizardState();
}

class _RunWizardState extends State<RunWizard> {
  late final image = TextEditingController(text: widget.initial?.image ?? '');
  late final name = TextEditingController(text: widget.initial?.name ?? '');
  late final command = TextEditingController(text: (widget.initial?.command ?? const []).map(_quote).join(' '));
  late final entrypoint = TextEditingController(text: widget.initial?.entrypoint ?? '');
  late final workdir = TextEditingController(text: widget.initial?.workdir ?? '');
  late final user = TextEditingController(text: widget.initial?.user ?? '');
  late final cpus = TextEditingController(text: widget.initial?.cpus ?? '');
  late final memory = TextEditingController(text: widget.initial?.memory ?? '');
  late final ports = [...?widget.initial?.ports.map(_PortRow.new)];
  late final mounts = [...?widget.initial?.mounts.map(_MountRow.new)];
  late final env = [...?widget.initial?.env.map(_EnvRow.new)];
  late String? network = widget.initial?.network;
  late bool autoRemove = widget.initial?.autoRemove ?? false;
  late bool start = widget.initial?.start ?? true;
  bool advanced = false;
  bool busy = false;
  String? submitError;

  static String _quote(String s) =>
      s.isEmpty || s.contains(RegExp(r'''[\s'"]''')) ? "'${s.replaceAll("'", "'\"'\"'")}'" : s;

  @override
  void initState() {
    super.initState();
    final i = widget.initial;
    advanced =
        i != null &&
        (i.command.isNotEmpty ||
            i.entrypoint != null ||
            i.workdir != null ||
            i.user != null ||
            i.network != null ||
            i.cpus != null ||
            i.memory != null ||
            i.autoRemove ||
            !i.start);
  }

  String? text(TextEditingController c) => c.text.trim().isEmpty ? null : c.text.trim();

  /// Builds the spec; returns an error text instead when the command line cannot be split.
  (RunSpec?, String?) buildSpec() {
    final List<String> args;
    try {
      args = shellWords(command.text, l10n: context.l10n);
    } on FormatException catch (e) {
      return (null, context.l10n.wslcCommand(e.message));
    }
    return (
      RunSpec(
        image: image.text.trim(),
        name: text(name),
        ports: [for (final p in ports) p.spec()],
        mounts: [for (final m in mounts) m.spec()],
        env: [
          for (final e in env)
            if (e.key.text.trim().isNotEmpty) EnvVar(key: e.key.text.trim(), value: e.value.text),
        ],
        command: args,
        entrypoint: text(entrypoint),
        workdir: text(workdir),
        user: text(user),
        network: network,
        cpus: text(cpus),
        memory: text(memory),
        autoRemove: autoRemove,
        start: start,
        extras: widget.initial?.extras,
      ),
      null,
    );
  }

  Future<void> submit(RunSpec spec) async {
    setState(() {
      busy = true;
      submitError = null;
    });
    try {
      final id = await WslcApiScope.of(context).runContainer(spec: spec);
      await widget.controller.refresh();
      if (mounted) Navigator.pop(context, id);
    } catch (e) {
      if (mounted) {
        setState(() {
          busy = false;
          submitError = errorText(e);
        });
      }
    }
  }

  Future<void> pickFolder(_MountRow row) async {
    final path = await WslcApiScope.of(context).pickFolder(title: context.l10n.wslcDirectoryToMount);
    if (path != null && mounted) setState(() => row.source.text = path);
  }

  @override
  void dispose() {
    for (final c in [image, name, command, entrypoint, workdir, user, cpus, memory]) {
      c.dispose();
    }
    for (final p in ports) {
      p.host.dispose();
      p.container.dispose();
    }
    for (final m in mounts) {
      m.source.dispose();
      m.target.dispose();
    }
    for (final e in env) {
      e.key.dispose();
      e.value.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final c = widget.controller;
    final (spec, splitError) = buildSpec();
    String? previewError = splitError;
    var preview = '';
    if (spec != null) {
      try {
        preview = WslcApiScope.of(context).previewRunCommand(spec: spec);
      } catch (e) {
        previewError = e is AppError ? e.message : errorText(e);
      }
    }
    final images = {
      for (final i in c.images)
        if (i.repository != null) '${i.repository}${i.tag == null ? '' : ':${i.tag}'}',
    }.toList();
    final volumes = [for (final v in c.volumes) v.name];
    return Dialog(
      insetPadding: const EdgeInsets.all(32),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 880, maxHeight: 780),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(24, 20, 16, 12),
              child: Row(
                children: [
                  const GradientIcon(Icons.add_box_rounded, size: 38),
                  const SizedBox(width: 14),
                  Expanded(
                    child: Text(
                      context.l10n.wslcNewContainer,
                      style: const TextStyle(fontSize: 19, fontWeight: FontWeight.w700),
                    ),
                  ),
                  if (!busy)
                    TextButton.icon(
                      key: const ValueKey('wslc-wizard-compose'),
                      onPressed: () => Navigator.of(context).pop(_WizardAction.compose),
                      icon: const Icon(Icons.dynamic_feed_rounded, size: 18),
                      label: Text(context.l10n.wslcFromCompose),
                    ),
                  if (widget.onTemplate != null && !busy)
                    TemplateMenu(controller: c, current: spec, onLoad: widget.onTemplate!),
                  IconButton(
                    tooltip: context.l10n.commonClose,
                    onPressed: busy ? null : () => Navigator.pop(context),
                    icon: const Icon(Icons.close_rounded),
                  ),
                ],
              ),
            ),
            Divider(height: 1, color: p.border),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.fromLTRB(24, 16, 24, 16),
                children: [
                  Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Expanded(
                        flex: 3,
                        child: _Labeled(
                          context.l10n.dockerImage,
                          Autocomplete<String>(
                            initialValue: image.value,
                            optionsBuilder: (v) => images.where((i) => i.toLowerCase().contains(v.text.toLowerCase())),
                            onSelected: (v) => setState(() => image.text = v),
                            fieldViewBuilder: (context, field, focus, _) {
                              field.addListener(() {
                                if (image.text != field.text) {
                                  image.text = field.text;
                                  setState(() {});
                                }
                              });
                              return TextField(
                                controller: field,
                                focusNode: focus,
                                decoration: const InputDecoration(
                                  hintText: 'mcr.microsoft.com/…:tag', // l10n-ignore: registry image reference example.
                                  prefixIcon: Icon(Icons.layers_rounded, size: 18),
                                ),
                              );
                            },
                          ),
                        ),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        flex: 2,
                        child: _Labeled(
                          context.l10n.wslcNameOptional,
                          TextField(
                            controller: name,
                            onChanged: (_) => setState(() {}),
                            decoration: const InputDecoration(hintText: 'my-app'),
                          ),
                        ),
                      ),
                    ],
                  ),
                  if (image.text.trim().isNotEmpty && !images.contains(image.text.trim()))
                    Padding(
                      padding: const EdgeInsets.only(top: 6),
                      child: Text(
                        context.l10n.wslcTheImageIsNotAvailableLocallyWSLcWill,
                        style: TextStyle(color: p.warning, fontSize: 12),
                      ),
                    ),
                  _Section(
                    title: context.l10n.kubePorts,
                    hint: context.l10n.wslcThisComputerOnlyTheWholeNetwork,
                    onAdd: () => setState(() => ports.add(_PortRow())),
                    addLabel: context.l10n.validationFieldPort,
                    children: [
                      for (final row in ports)
                        Row(
                          children: [
                            SizedBox(
                              width: 136,
                              child: DropdownButtonFormField<bool>(
                                initialValue: row.allInterfaces,
                                isDense: true,
                                isExpanded: true,
                                items: const [
                                  DropdownMenuItem(
                                    value: false,
                                    child: Text('127.0.0.1', overflow: TextOverflow.ellipsis),
                                  ),
                                  DropdownMenuItem(
                                    value: true,
                                    child: Text('0.0.0.0', overflow: TextOverflow.ellipsis),
                                  ),
                                ],
                                onChanged: (v) => setState(() => row.allInterfaces = v ?? false),
                              ),
                            ),
                            const SizedBox(width: 8),
                            Expanded(
                              child: _Field(row.host, context.l10n.wslcHostPort, onChanged: () => setState(() {})),
                            ),
                            const Padding(
                              padding: EdgeInsets.symmetric(horizontal: 8),
                              child: Icon(Icons.arrow_forward_rounded, size: 16),
                            ),
                            Expanded(
                              child: _Field(
                                row.container,
                                context.l10n.wslcContainerPort,
                                onChanged: () => setState(() {}),
                              ),
                            ),
                            const SizedBox(width: 8),
                            SizedBox(
                              width: 92,
                              child: DropdownButtonFormField<bool>(
                                initialValue: row.udp,
                                isDense: true,
                                isExpanded: true,
                                items: [
                                  for (final protocol in const ['TCP', 'UDP'])
                                    DropdownMenuItem(
                                      value: protocol == 'UDP',
                                      child: Text(protocol, overflow: TextOverflow.ellipsis),
                                    ),
                                ],
                                onChanged: (v) => setState(() => row.udp = v ?? false),
                              ),
                            ),
                            _Remove(() => setState(() => ports.remove(row))),
                          ],
                        ),
                    ],
                  ),
                  _Section(
                    title: context.l10n.wslcDirectoriesAndVolumes,
                    hint: context.l10n.wslcWindowsDirectoryOrNamedVolumePathInContainer,
                    onAdd: () => setState(() => mounts.add(_MountRow())),
                    addLabel: context.l10n.commonAdd,
                    children: [
                      for (final row in mounts)
                        Row(
                          children: [
                            SizedBox(
                              width: 130,
                              child: DropdownButtonFormField<bool>(
                                initialValue: row.volume,
                                isDense: true,
                                isExpanded: true,
                                items: [
                                  DropdownMenuItem(
                                    value: false,
                                    child: Text(context.l10n.backupsFolderLabel, overflow: TextOverflow.ellipsis),
                                  ),
                                  DropdownMenuItem(
                                    value: true,
                                    child: Text(context.l10n.wslcVolumeRunWizard, overflow: TextOverflow.ellipsis),
                                  ),
                                ],
                                onChanged: (v) => setState(() => row.volume = v ?? false),
                              ),
                            ),
                            const SizedBox(width: 8),
                            Expanded(
                              flex: 3,
                              child: row.volume
                                  ? Autocomplete<String>(
                                      initialValue: row.source.value,
                                      optionsBuilder: (v) => volumes.where((n) => n.contains(v.text)),
                                      onSelected: (v) => setState(() => row.source.text = v),
                                      fieldViewBuilder: (context, f, focus, _) {
                                        f.addListener(() {
                                          if (row.source.text != f.text) {
                                            row.source.text = f.text;
                                            setState(() {});
                                          }
                                        });
                                        return TextField(
                                          controller: f,
                                          focusNode: focus,
                                          decoration: InputDecoration(
                                            hintText: context.l10n.wslcVolumeNameCreatedIfNeeded,
                                          ),
                                        );
                                      },
                                    )
                                  : _Field(
                                      row.source,
                                      context.l10n.wslcCPathToFolder,
                                      onChanged: () => setState(() {}),
                                      suffix: IconButton(
                                        tooltip: context.l10n.wslcChooseDirectory,
                                        onPressed: () => pickFolder(row),
                                        icon: const Icon(Icons.folder_open_rounded, size: 18),
                                      ),
                                    ),
                            ),
                            const Padding(
                              padding: EdgeInsets.symmetric(horizontal: 8),
                              child: Icon(Icons.arrow_forward_rounded, size: 16),
                            ),
                            Expanded(
                              flex: 2,
                              child: _Field(
                                row.target,
                                context.l10n.wslcPathInContainer,
                                onChanged: () => setState(() {}),
                              ),
                            ),
                            const SizedBox(width: 4),
                            Tooltip(
                              message: context.l10n.riskReadOnly,
                              child: FilterChip(
                                label: Text(context.l10n.riskReadOnly),
                                selected: row.readOnly,
                                onSelected: (v) => setState(() => row.readOnly = v),
                              ),
                            ),
                            _Remove(() => setState(() => mounts.remove(row))),
                          ],
                        ),
                    ],
                  ),
                  _Section(
                    title: context.l10n.dockerEnvironment,
                    hint: context.l10n.wslcPassedThroughATemporaryFileRatherThanThe,
                    onAdd: () => setState(() => env.add(_EnvRow())),
                    addLabel: context.l10n.wslcVariable,
                    children: [
                      for (final row in env)
                        Row(
                          children: [
                            Expanded(
                              flex: 2,
                              child: _Field(row.key, context.l10n.wslcNAME, onChanged: () => setState(() {})),
                            ),
                            const Padding(padding: EdgeInsets.symmetric(horizontal: 8), child: Text('=')),
                            Expanded(
                              flex: 3,
                              child: RevealedSecret(
                                visible: row.visible && row.value.text.isNotEmpty,
                                child: _Field(
                                  row.value,
                                  context.l10n.copyWhatValue,
                                  obscure: !row.visible,
                                  onChanged: () => setState(() {}),
                                  suffix: IconButton(
                                    tooltip: row.visible ? context.l10n.commonHide : context.l10n.commonShow,
                                    onPressed: () => setState(() => row.visible = !row.visible),
                                    icon: Icon(
                                      row.visible ? Icons.visibility_off_rounded : Icons.visibility_rounded,
                                      size: 18,
                                    ),
                                  ),
                                ),
                              ),
                            ),
                            _Remove(() => setState(() => env.remove(row))),
                          ],
                        ),
                    ],
                  ),
                  const SizedBox(height: 8),
                  InkWell(
                    borderRadius: BorderRadius.circular(8),
                    onTap: () => setState(() => advanced = !advanced),
                    child: Padding(
                      padding: const EdgeInsets.symmetric(vertical: 8),
                      child: Row(
                        children: [
                          Icon(advanced ? Icons.expand_less_rounded : Icons.expand_more_rounded),
                          const SizedBox(width: 6),
                          Text(context.l10n.wslcAdvanced, style: const TextStyle(fontWeight: FontWeight.w700)),
                        ],
                      ),
                    ),
                  ),
                  if (advanced) ...[
                    _Labeled(
                      context.l10n.wslcCommandOverridesTheImageCMD,
                      _Field(
                        command,
                        "sh -c 'echo hello'", // l10n-ignore: literal shell command example.
                        mono: true,
                        onChanged: () => setState(() {}),
                      ),
                    ),
                    const SizedBox(height: 10),
                    Row(
                      children: [
                        Expanded(
                          child: _Labeled('Entrypoint', _Field(entrypoint, '', onChanged: () => setState(() {}))),
                        ),
                        const SizedBox(width: 10),
                        Expanded(
                          child: _Labeled(
                            context.l10n.wslcWorkingDirectory,
                            _Field(workdir, '/app', onChanged: () => setState(() {})),
                          ),
                        ),
                        const SizedBox(width: 10),
                        Expanded(
                          child: _Labeled(
                            context.l10n.snippetSourceUser,
                            _Field(user, '1000:1000', onChanged: () => setState(() {})),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 10),
                    Row(
                      children: [
                        Expanded(
                          child: _Labeled(
                            context.l10n.hostMetricsNetwork,
                            DropdownButtonFormField<String?>(
                              initialValue: network,
                              isDense: true,
                              isExpanded: true,
                              items: [
                                DropdownMenuItem(
                                  value: null,
                                  child: Text(context.l10n.dbDesignDefault, overflow: TextOverflow.ellipsis),
                                ),
                                for (final n in c.networks)
                                  if (n.name != null)
                                    DropdownMenuItem(
                                      value: n.name,
                                      child: Text(n.name!, overflow: TextOverflow.ellipsis),
                                    ),
                              ],
                              onChanged: (v) => setState(() => network = v),
                            ),
                          ),
                        ),
                        const SizedBox(width: 10),
                        Expanded(child: _Labeled('CPU', _Field(cpus, '1.5', onChanged: () => setState(() {})))),
                        const SizedBox(width: 10),
                        Expanded(
                          child: _Labeled(
                            context.l10n.kubeOverviewMemory,
                            _Field(memory, '512m', onChanged: () => setState(() {})),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 10),
                    Wrap(
                      spacing: 18,
                      children: [
                        _Check(
                          context.l10n.wslcRemoveAfterStoppingRm,
                          autoRemove,
                          (v) => setState(() => autoRemove = v),
                        ),
                        _Check(context.l10n.wslcCreateWithoutStarting, !start, (v) => setState(() => start = !v)),
                      ],
                    ),
                  ],
                ],
              ),
            ),
            Divider(height: 1, color: p.border),
            Padding(
              padding: const EdgeInsets.fromLTRB(24, 14, 24, 18),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Container(
                    padding: const EdgeInsets.fromLTRB(12, 8, 4, 8),
                    decoration: BoxDecoration(
                      color: p.terminal,
                      borderRadius: BorderRadius.circular(10),
                      border: Border.all(color: p.border),
                    ),
                    child: Row(
                      children: [
                        Expanded(
                          child: WslcSelectableText(
                            previewError ?? preview,
                            maxLines: 3,
                            minLines: 1,
                            style: TextStyle(
                              fontFamily: monoFont,
                              fontFamilyFallback: monoFallback,
                              fontSize: 12,
                              color: previewError == null ? p.terminalText : p.warning,
                            ),
                          ),
                        ),
                        IconButton(
                          tooltip: context.l10n.dockerCopyCommand,
                          style: IconButton.styleFrom(foregroundColor: p.terminalText),
                          onPressed: previewError == null ? () => copyText(context, preview) : null,
                          icon: const Icon(Icons.copy_rounded, size: 17),
                        ),
                      ],
                    ),
                  ),
                  if (submitError != null)
                    Padding(
                      padding: const EdgeInsets.only(top: 10),
                      child: WslcSelectableText(
                        submitError!,
                        maxLines: 5,
                        style: TextStyle(color: p.danger, fontSize: 12.5),
                      ),
                    ),
                  const SizedBox(height: 12),
                  Row(
                    children: [
                      if (busy) ...[
                        const SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2)),
                        const SizedBox(width: 10),
                        Expanded(
                          child: Text(
                            context.l10n.wslcCreatingPullingTheImageIfItIsMissing,
                            style: TextStyle(color: p.muted, fontSize: 12.5),
                          ),
                        ),
                      ] else
                        const Spacer(),
                      TextButton(
                        onPressed: busy ? null : () => Navigator.pop(context),
                        child: Text(context.l10n.commonCancel),
                      ),
                      const SizedBox(width: 8),
                      FilledButton.icon(
                        onPressed: busy || previewError != null || spec == null ? null : () => submit(spec),
                        icon: Icon(start ? Icons.play_arrow_rounded : Icons.add_box_outlined, size: 18),
                        label: Text(start ? context.l10n.wslcCreateAndStart : context.l10n.commonCreate),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Labeled extends StatelessWidget {
  const _Labeled(this.label, this.child);
  final String label;
  final Widget child;

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      Text(
        label,
        style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600, color: DeskPalette.of(context).muted),
      ),
      const SizedBox(height: 6),
      child,
    ],
  );
}

class _Field extends StatelessWidget {
  const _Field(
    this.controller,
    this.hint, {
    required this.onChanged,
    this.suffix,
    this.obscure = false,
    this.mono = false,
  });
  final TextEditingController controller;
  final String hint;
  final VoidCallback onChanged;
  final Widget? suffix;
  final bool obscure;
  final bool mono;

  @override
  Widget build(BuildContext context) => TextField(
    controller: controller,
    obscureText: obscure,
    onChanged: (_) => onChanged(),
    style: mono ? const TextStyle(fontFamily: monoFont, fontFamilyFallback: monoFallback) : null,
    decoration: InputDecoration(hintText: hint, suffixIcon: suffix),
  );
}

class _Remove extends StatelessWidget {
  const _Remove(this.onPressed);
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: context.l10n.wslcRemove,
    onPressed: onPressed,
    icon: const Icon(Icons.remove_circle_outline_rounded, size: 20),
  );
}

class _Check extends StatelessWidget {
  const _Check(this.label, this.value, this.onChanged);
  final String label;
  final bool value;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) => Row(
    mainAxisSize: MainAxisSize.min,
    children: [
      Checkbox(value: value, onChanged: (v) => onChanged(v ?? false)),
      Text(label),
    ],
  );
}

class _Section extends StatelessWidget {
  const _Section({
    required this.title,
    required this.hint,
    required this.onAdd,
    required this.addLabel,
    required this.children,
  });
  final String title;
  final String hint;
  final VoidCallback onAdd;
  final String addLabel;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Text(title, style: const TextStyle(fontWeight: FontWeight.w700)),
              const SizedBox(width: 10),
              Expanded(
                child: Text(
                  hint,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: p.faint, fontSize: 12),
                ),
              ),
              TextButton.icon(onPressed: onAdd, icon: const Icon(Icons.add_rounded, size: 18), label: Text(addLabel)),
            ],
          ),
          for (final child in children) ...[const SizedBox(height: 8), child],
        ],
      ),
    );
  }
}
