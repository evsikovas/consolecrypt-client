import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/docker/compose_ai_dialog.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/compose/compose_runner.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

Future<void> showComposeDialog(BuildContext context, DeskController c) => showWslcDialog<void>(
  context: context,
  barrierDismissible: false,
  containsSecrets: false,
  builder: (_) => ComposeDialog(controller: c),
);

/// Stands in for the runner before anything runs.
final _idle = ValueNotifier(0);

const _sample = '''services:
  web:
    image: nginx:latest
    ports:
      - "8080:80"
    volumes:
      - ./site:/usr/share/nginx/html:ro''';

class ComposeDialog extends ConsumerStatefulWidget {
  const ComposeDialog({super.key, required this.controller, this.initialYaml});
  final DeskController controller;
  final String? initialYaml;
  @override
  ConsumerState<ComposeDialog> createState() => _ComposeDialogState();
}

class _ComposeDialogState extends ConsumerState<ComposeDialog> {
  late final api = WslcApiScope.of(context);

  late final yaml = TextEditingController(text: widget.initialYaml ?? '');
  final project = TextEditingController();
  final folder = TextEditingController();
  Timer? _debounce;
  int _seq = 0;
  ComposePlan? plan;
  String? parseError;
  bool parsing = false;
  bool rebuild = false;
  ComposeRunner? runner;
  bool generating = false;

  Future<void> _generateAi() async {
    if (generating) return;
    final profile = ref.read(activeProfileProvider)?.id;
    bool current() =>
        mounted &&
        profile != null &&
        ref.read(activeProfileProvider)?.id == profile &&
        ref.read(vaultStatusProvider).value?.isUnlocked == true;
    if (!current()) return;
    setState(() => generating = true);
    try {
      final result = await showComposeAiDialog(context);
      if (!mounted || result == null || !current()) return;
      if (yaml.text.trim().isNotEmpty) {
        final accepted = await showConfirmDialog(
          context,
          title: context.l10n.composeAiReplaceTitle,
          message: context.l10n.composeAiReplaceHelp,
          confirmLabel: context.l10n.composeAiUse,
        );
        if (!accepted || !current()) return;
      }
      yaml.text = result;
      await _parse();
    } finally {
      if (mounted) setState(() => generating = false);
    }
  }

  @override
  void initState() {
    super.initState();
    if (yaml.text.isNotEmpty) unawaited(_parse());
  }

  @override
  void dispose() {
    _debounce?.cancel();
    runner?.dispose();
    yaml.dispose();
    project.dispose();
    folder.dispose();
    super.dispose();
  }

  String? _text(TextEditingController c) => c.text.trim().isEmpty ? null : c.text.trim();

  void _changed([String _ = '']) {
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 400), _parse);
  }

  Future<void> _parse() async {
    final seq = ++_seq;
    if (yaml.text.trim().isEmpty) {
      setState(() {
        plan = null;
        parseError = null;
        parsing = false;
      });
      return;
    }
    setState(() => parsing = true);
    try {
      final p = await api.parseCompose(yaml: yaml.text, projectDir: _text(folder), projectName: _text(project));
      if (!mounted || seq != _seq) return;
      setState(() {
        plan = p;
        parseError = null;
        parsing = false;
      });
    } catch (e) {
      if (!mounted || seq != _seq) return;
      setState(() {
        plan = null;
        parseError = e is AppError ? e.message : errorText(e);
        parsing = false;
      });
    }
  }

  Future<void> _open() async {
    final notify = WslcApiScope.notifierOf(context);
    final path = await api.pickFile(title: context.l10n.wslcDockerComposeFile);
    if (path == null || !mounted) return;
    try {
      final document = await api.readComposeFile(path: path);
      if (!mounted) return;
      yaml.text = document;
      final slash = path.lastIndexOf('\\');
      if (slash > 0) folder.text = path.substring(0, slash);
      await _parse();
    } catch (e) {
      notify(errorText(e), error: true);
    }
  }

  Future<void> _pickFolder() async {
    final dir = await api.pickFolder(title: context.l10n.wslcComposeProjectDirectory);
    if (dir == null || !mounted) return;
    folder.text = dir;
    await _parse();
  }

  Future<void> _up() async {
    final p = plan;
    if (p == null) return;
    runner?.dispose();
    final r = ComposeRunner(p, widget.controller);
    setState(() => runner = r);
    await r.up(rebuild: rebuild);
  }

  Future<void> _down() async {
    final p = plan;
    if (p == null) return;
    final withVolumes = await showWslcDialog<bool>(
      context: context,
      builder: (_) => _DownDialog(plan: p),
    );
    if (withVolumes == null || !mounted) return;
    runner?.dispose();
    final r = ComposeRunner(p, widget.controller);
    setState(() => runner = r);
    await r.down(volumes: withVolumes);
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final r = runner;
    return Dialog(
      insetPadding: const EdgeInsets.all(28),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 1180, maxHeight: 820),
        child: ListenableBuilder(
          listenable: r ?? _idle,
          builder: (context, _) {
            final busy = r?.busy ?? false;
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(24, 20, 16, 12),
                  child: Row(
                    children: [
                      const GradientIcon(Icons.dynamic_feed_rounded, size: 38),
                      const SizedBox(width: 14),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              context.l10n.wslcDockerComposeFile,
                              style: const TextStyle(fontSize: 19, fontWeight: FontWeight.w700),
                            ),
                            Text(
                              context.l10n.wslcPasteDockerComposeYmlToStartTheServices,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: const TextStyle(fontSize: 12.5),
                            ),
                          ],
                        ),
                      ),
                      if (r == null) ...[
                        TextButton.icon(
                          key: const ValueKey('wslc-compose-ai'),
                          onPressed: generating ? null : _generateAi,
                          icon: const Icon(Icons.auto_awesome_rounded, size: 18),
                          label: Text(context.l10n.composeAiGenerate),
                        ),
                        TextButton.icon(
                          onPressed: _open,
                          icon: const Icon(Icons.file_open_outlined, size: 18),
                          label: Text(context.l10n.wslcOpenFile),
                        ),
                      ],
                      IconButton(
                        tooltip: context.l10n.commonClose,
                        onPressed: busy ? null : () => Navigator.pop(context),
                        icon: const Icon(Icons.close_rounded),
                      ),
                    ],
                  ),
                ),
                Divider(height: 1, color: p.border),
                Expanded(child: r == null ? _editor(p) : _Progress(runner: r)),
                Divider(height: 1, color: p.border),
                Padding(padding: const EdgeInsets.fromLTRB(24, 12, 24, 16), child: _footer(p, r, busy)),
              ],
            );
          },
        ),
      ),
    );
  }

  Widget _footer(DeskPalette p, ComposeRunner? r, bool busy) {
    final c = widget.controller;
    final plan = this.plan;
    final canRun = plan != null && !parsing && c.canMutate;
    if (r != null) {
      return Row(
        children: [
          if (!busy)
            TextButton.icon(
              onPressed: () {
                r.dispose();
                setState(() => runner = null);
              },
              icon: const Icon(Icons.arrow_back_rounded, size: 18),
              label: Text(context.l10n.wslcBackToFile),
            ),
          const Spacer(),
          if (busy)
            OutlinedButton.icon(
              onPressed: r.cancelled ? null : r.cancel,
              icon: const Icon(Icons.stop_rounded, size: 18),
              label: Text(r.cancelled ? context.l10n.wslcStopping : context.l10n.aiChatStop),
            )
          else
            FilledButton(onPressed: () => Navigator.pop(context), child: Text(context.l10n.commonDone)),
        ],
      );
    }
    final builds = plan?.services.any((s) => s.build != null) ?? false;
    return Row(
      children: [
        if (builds)
          Material(
            type: MaterialType.transparency,
            child: InkWell(
              borderRadius: BorderRadius.circular(8),
              onTap: () => setState(() => rebuild = !rebuild),
              child: Row(
                children: [
                  Checkbox(value: rebuild, onChanged: (v) => setState(() => rebuild = v ?? false)),
                  Text(context.l10n.wslcRebuildImagesBuild),
                  const SizedBox(width: 8),
                ],
              ),
            ),
          ),
        const Spacer(),
        OutlinedButton.icon(
          style: OutlinedButton.styleFrom(foregroundColor: p.danger),
          onPressed: canRun ? _down : null,
          icon: const Icon(Icons.delete_sweep_outlined, size: 18),
          label: Text(context.l10n.wslcStopAndRemove),
        ),
        const SizedBox(width: 10),
        FilledButton.icon(
          onPressed: canRun ? _up : null,
          icon: const Icon(Icons.play_arrow_rounded, size: 18),
          label: Text(context.l10n.localAiStart),
        ),
      ],
    );
  }

  Widget _editor(DeskPalette p) => Row(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      Expanded(
        flex: 11,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(24, 16, 16, 16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Expanded(
                child: TextField(
                  controller: yaml,
                  onChanged: _changed,
                  maxLines: null,
                  expands: true,
                  textAlignVertical: TextAlignVertical.top,
                  style: const TextStyle(fontFamily: 'Consolas', fontSize: 12.5, height: 1.35),
                  decoration: InputDecoration(
                    hintText: _sample,
                    hintStyle: TextStyle(color: p.faint, fontFamily: 'Consolas'),
                  ),
                ),
              ),
              const SizedBox(height: 12),
              Row(
                children: [
                  Expanded(
                    flex: 2,
                    child: TextField(
                      controller: project,
                      onChanged: _changed,
                      decoration: InputDecoration(
                        labelText: context.l10n.automationProjectMenu,
                        hintText: context.l10n.wslcFromNameOrTheDirectory,
                      ),
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    flex: 5,
                    child: TextField(
                      controller: folder,
                      onChanged: _changed,
                      decoration: InputDecoration(
                        labelText: context.l10n.wslcProjectDirectory,
                        hintText: context.l10n.wslcRelativePathsEnvEnvFileAndBuild,
                        suffixIcon: IconButton(
                          tooltip: context.l10n.wslcChooseDirectory,
                          onPressed: _pickFolder,
                          icon: const Icon(Icons.folder_open_rounded, size: 20),
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
      VerticalDivider(width: 1, color: p.border),
      Expanded(flex: 10, child: _preview(p)),
    ],
  );

  Widget _preview(DeskPalette p) {
    final plan = this.plan;
    if (yaml.text.trim().isEmpty) {
      return EmptyState(
        icon: Icons.dynamic_feed_rounded,
        title: context.l10n.wslcPasteDockerComposeYml,
        hint: context.l10n.wslcOrOpenAFileToPreviewThePlan,
      );
    }
    if (parseError != null) {
      return Padding(
        padding: const EdgeInsets.all(20),
        child: _Notice(color: p.danger, icon: Icons.error_outline_rounded, lines: [parseError!]),
      );
    }
    if (plan == null) {
      return EmptyState(icon: Icons.dynamic_feed_rounded, title: context.l10n.wslcParsingFile, loading: true);
    }
    final commands = [...plan.commands, for (final s in plan.services) ...s.commands];
    return ListView(
      padding: const EdgeInsets.fromLTRB(20, 16, 24, 16),
      children: [
        Row(
          children: [
            Text(
              context.l10n.wslcProject(plan.project),
              style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w700),
            ),
            const SizedBox(width: 10),
            Pill(
              plural(
                plan.services.length,
                context.l10n.wslcService,
                context.l10n.wslcServices,
                context.l10n.wslcServicesComposeDialog,
                locale: context.l10n.localeName,
              ),
              color: p.accent,
            ),
            if (parsing) ...[
              const SizedBox(width: 10),
              const SizedBox(width: 14, height: 14, child: CircularProgressIndicator(strokeWidth: 2)),
            ],
          ],
        ),
        if (plan.warnings.isNotEmpty) ...[
          const SizedBox(height: 12),
          _Notice(
            color: p.warning,
            icon: Icons.info_outline_rounded,
            title: context.l10n.wslcDifferencesFromDocker,
            lines: plan.warnings,
          ),
        ],
        const SizedBox(height: 14),
        _caption(p, context.l10n.wslcServicesInStartupOrder),
        for (final s in plan.services) _ServiceTile(service: s),
        if (plan.networks.isNotEmpty || plan.volumes.isNotEmpty) ...[
          const SizedBox(height: 10),
          _caption(p, context.l10n.wslcNetworksAndVolumes),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            children: [
              for (final n in plan.networks)
                _chip(p, Icons.hub_rounded, n.isExternal ? '${n.spec.name}${context.l10n.wslcExternal}' : n.spec.name),
              for (final v in plan.volumes)
                _chip(
                  p,
                  Icons.storage_rounded,
                  v.isExternal ? '${v.spec.name}${context.l10n.wslcExternalComposeDialog}' : v.spec.name,
                ),
            ],
          ),
        ],
        const SizedBox(height: 10),
        Theme(
          data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
          child: ExpansionTile(
            tilePadding: EdgeInsets.zero,
            title: Text(
              context.l10n.wslcWSLcCommands(commands.length),
              style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600),
            ),
            children: [
              Container(
                width: double.infinity,
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(color: p.terminal, borderRadius: BorderRadius.circular(10)),
                child: WslcSelectableText(
                  commands.join('\n\n'),
                  minLines: 1,
                  style: TextStyle(color: p.terminalText, fontFamily: 'Consolas', fontSize: 11.5),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

Widget _caption(DeskPalette p, String text) => Padding(
  padding: const EdgeInsets.only(bottom: 8),
  child: Text(
    text.toUpperCase(),
    style: TextStyle(color: p.faint, fontSize: 10.5, fontWeight: FontWeight.w700, letterSpacing: .7),
  ),
);

Widget _chip(DeskPalette p, IconData icon, String text, {Color? color}) => Container(
  padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
  decoration: BoxDecoration(
    color: p.raised,
    borderRadius: BorderRadius.circular(6),
    border: Border.all(color: p.border),
  ),
  child: Row(
    mainAxisSize: MainAxisSize.min,
    children: [
      Icon(icon, size: 13, color: color ?? p.muted),
      const SizedBox(width: 5),
      Flexible(
        child: Text(
          text,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(fontSize: 11.5, color: color ?? p.muted),
        ),
      ),
    ],
  ),
);

class _ServiceTile extends StatelessWidget {
  const _ServiceTile({required this.service});
  final ComposeService service;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final s = service;
    final spec = s.spec;
    String port(PortSpec x) =>
        '${x.hostIp == '0.0.0.0' || x.hostIp == null ? '' : '${x.hostIp}:'}'
        '${x.hostPort == 0 ? '*' : x.hostPort}→${x.containerPort}${x.udp ? '/udp' : ''}';
    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.fromLTRB(12, 10, 12, 10),
      decoration: BoxDecoration(
        color: p.raised.withValues(alpha: .55),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: p.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.view_in_ar_rounded, size: 17, color: p.success),
              const SizedBox(width: 8),
              Text(s.service, style: const TextStyle(fontWeight: FontWeight.w700)),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  s.containerName,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: p.faint, fontSize: 12),
                ),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            children: [
              _chip(
                p,
                s.build != null ? Icons.construction_rounded : Icons.layers_rounded,
                s.build != null ? context.l10n.wslcBuild(s.image) : s.image,
                color: p.accentEnd,
              ),
              for (final x in spec.ports) _chip(p, Icons.lan_outlined, port(x)),
              for (final m in spec.mounts)
                _chip(
                  p,
                  m.volume ? Icons.storage_rounded : Icons.folder_rounded,
                  '${m.volume ? m.source : m.source.split('\\').last}:${m.target}'
                  '${m.readOnly ? ' (ro)' : ''}',
                ),
              if (spec.env.isNotEmpty) _chip(p, Icons.key_rounded, context.l10n.wslcVariables(spec.env.length)),
              for (final d in s.dependsOn)
                _chip(
                  p,
                  Icons.subdirectory_arrow_right_rounded,
                  context.l10n.wslcAfter(d.service, switch (d.condition) {
                    'healthy' => context.l10n.wslcHealthy,
                    'completed' => context.l10n.wslcCompleted,
                    _ => '',
                  }),
                  color: p.warning,
                ),
            ],
          ),
        ],
      ),
    );
  }
}

class _Notice extends StatelessWidget {
  const _Notice({required this.color, required this.icon, required this.lines, this.title});
  final Color color;
  final IconData icon;
  final List<String> lines;
  final String? title;

  @override
  Widget build(BuildContext context) => Container(
    padding: const EdgeInsets.fromLTRB(12, 10, 12, 10),
    decoration: BoxDecoration(
      color: color.withValues(alpha: .08),
      borderRadius: BorderRadius.circular(10),
      border: Border.all(color: color.withValues(alpha: .3)),
    ),
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Icon(icon, size: 18, color: color),
        const SizedBox(width: 10),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (title != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: 4),
                  child: Text(
                    title!,
                    style: TextStyle(color: color, fontWeight: FontWeight.w700),
                  ),
                ),
              for (final line in lines)
                Padding(
                  padding: const EdgeInsets.only(bottom: 3),
                  child: WslcSelectableText(
                    lines.length > 1 ? '• $line' : line,
                    minLines: 1,
                    style: const TextStyle(fontSize: 12.5),
                  ),
                ),
            ],
          ),
        ),
      ],
    ),
  );
}

class _Progress extends StatelessWidget {
  const _Progress({required this.runner});
  final ComposeRunner runner;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final r = runner;
    final done = !r.busy;
    return ListView(
      padding: const EdgeInsets.fromLTRB(24, 16, 24, 16),
      children: [
        if (done)
          Padding(
            padding: const EdgeInsets.only(bottom: 12),
            child: r.error == null
                ? _Notice(
                    color: p.success,
                    icon: Icons.check_circle_outline_rounded,
                    lines: [context.l10n.wslcProjectReady(r.plan.project)],
                  )
                : _Notice(
                    color: p.danger,
                    icon: Icons.error_outline_rounded,
                    title: context.l10n.noticesRuntimeStopped,
                    lines: [r.error!],
                  ),
          ),
        for (final s in r.steps) _StepTile(step: s),
      ],
    );
  }
}

class _StepTile extends StatelessWidget {
  const _StepTile({required this.step});
  final ComposeStep step;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final s = step;
    final icon = switch (s.status) {
      StepStatus.pending => Icon(Icons.circle_outlined, size: 18, color: p.faint),
      StepStatus.running => const SizedBox(width: 18, height: 18, child: CircularProgressIndicator(strokeWidth: 2)),
      StepStatus.done => Icon(Icons.check_circle_rounded, size: 18, color: p.success),
      StepStatus.skipped => Icon(Icons.remove_circle_outline_rounded, size: 18, color: p.muted),
      StepStatus.failed => Icon(Icons.error_rounded, size: 18, color: p.danger),
    };
    final showLog = s.log.isNotEmpty && (s.status == StepStatus.running || s.status == StepStatus.failed);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 5),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Padding(padding: const EdgeInsets.only(top: 1), child: icon),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(s.title, style: const TextStyle(fontWeight: FontWeight.w600)),
                    if (s.note != null)
                      WslcSelectableText(
                        s.note!,
                        minLines: 1,
                        style: TextStyle(fontSize: 12.5, color: s.status == StepStatus.failed ? p.danger : p.muted),
                      ),
                  ],
                ),
              ),
            ],
          ),
          if (showLog)
            Container(
              margin: const EdgeInsets.only(left: 28, top: 6),
              padding: const EdgeInsets.all(10),
              decoration: BoxDecoration(color: p.terminal, borderRadius: BorderRadius.circular(8)),
              child: Text(
                s.log.skip(s.log.length > 10 ? s.log.length - 10 : 0).join('\n'),
                style: TextStyle(color: p.terminalText, fontFamily: 'Consolas', fontSize: 11.5),
              ),
            ),
        ],
      ),
    );
  }
}

class _DownDialog extends StatefulWidget {
  const _DownDialog({required this.plan});
  final ComposePlan plan;
  @override
  State<_DownDialog> createState() => _DownDialogState();
}

class _DownDialogState extends State<_DownDialog> {
  bool volumes = false;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final plan = widget.plan;
    final ownVolumes = plan.volumes.where((v) => !v.isExternal).length;
    return AlertDialog(
      icon: GradientIcon(Icons.delete_sweep_outlined, color: p.danger, size: 44),
      title: Text(context.l10n.wslcStopAndRemoveComposeDialog(plan.project)),
      content: SizedBox(
        width: 440,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(context.l10n.wslcAsWithDockerComposeDownThisStopsAnd),
            if (ownVolumes > 0)
              Material(
                type: MaterialType.transparency,
                child: CheckboxListTile(
                  contentPadding: EdgeInsets.zero,
                  controlAffinity: ListTileControlAffinity.leading,
                  value: volumes,
                  onChanged: (v) => setState(() => volumes = v ?? false),
                  title: Text(context.l10n.wslcAlsoRemoveProjectVolumes(ownVolumes)),
                  subtitle: Text(context.l10n.wslcDataInTheseVolumesWillBeLostV, style: TextStyle(color: p.danger)),
                ),
              ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(context.l10n.commonCancel)),
        FilledButton(
          style: FilledButton.styleFrom(backgroundColor: p.danger),
          onPressed: () => Navigator.pop(context, volumes),
          child: Text(context.l10n.commonDelete),
        ),
      ],
    );
  }
}
