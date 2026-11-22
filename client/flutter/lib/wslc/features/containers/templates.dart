import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

/// What a template keeps: no container name (each container needs its own), and
/// environment values only on request. Settings live in the encrypted profile.
RunSpec templateSpec(RunSpec s, {required bool withEnvValues}) => RunSpec(
  image: s.image,
  ports: s.ports,
  mounts: s.mounts,
  env: [for (final e in s.env) EnvVar(key: e.key, value: withEnvValues ? e.value : '')],
  command: s.command,
  entrypoint: s.entrypoint,
  workdir: s.workdir,
  user: s.user,
  network: s.network,
  cpus: s.cpus,
  memory: s.memory,
  autoRemove: s.autoRemove,
  start: s.start,
  extras: s.extras,
);

/// Asks for a name and saves [spec] as a template.
Future<void> saveTemplateFlow(BuildContext context, DeskController c, RunSpec spec) async {
  final result = await showWslcDialog<(String, bool)>(
    context: context,
    builder: (_) => _SaveTemplateDialog(
      suggested: spec.image.split('/').last.split(':').first,
      hasEnv: spec.env.any((e) => e.value.isNotEmpty),
      taken: {for (final t in c.templates) t.name},
    ),
  );
  if (result == null || !context.mounted) return;
  final (name, withEnv) = result;
  final notify = WslcApiScope.notifierOf(context);
  final l10n = context.l10n;
  final error = await c.saveTemplate(name, templateSpec(spec, withEnvValues: withEnv));
  notify(error ?? l10n.wslcTemplateSaved(name), error: error != null);
}

class _SaveTemplateDialog extends StatefulWidget {
  const _SaveTemplateDialog({required this.suggested, required this.hasEnv, required this.taken});
  final String suggested;
  final bool hasEnv;
  final Set<String> taken;
  @override
  State<_SaveTemplateDialog> createState() => _SaveTemplateDialogState();
}

class _SaveTemplateDialogState extends State<_SaveTemplateDialog> {
  late final name = TextEditingController(text: widget.suggested);
  bool withEnv = false;

  @override
  void dispose() {
    name.dispose();
    super.dispose();
  }

  void _save() {
    final text = name.text.trim();
    if (text.isNotEmpty) Navigator.pop(context, (text, withEnv));
  }

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final text = name.text.trim();
    return AlertDialog(
      icon: GradientIcon(Icons.bookmark_add_outlined, color: p.accent),
      title: Text(context.l10n.wslcSaveAsTemplate),
      content: SizedBox(
        width: 440,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            TextField(
              controller: name,
              autofocus: true,
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) => _save(),
              decoration: InputDecoration(
                labelText: context.l10n.settingsVaultNameLabel,
                helperText: widget.taken.contains(text) ? context.l10n.wslcATemplateWithThisNameWillBeReplaced : null,
              ),
            ),
            if (widget.hasEnv)
              Material(
                type: MaterialType.transparency,
                child: CheckboxListTile(
                  contentPadding: EdgeInsets.zero,
                  controlAffinity: ListTileControlAffinity.leading,
                  value: withEnv,
                  onChanged: (v) => setState(() => withEnv = v ?? false),
                  title: Text(context.l10n.wslcSaveEnvironmentVariableValues),
                  subtitle: Text(context.l10n.wslcOtherwiseOnlyNamesAreSavedEnterTheValues),
                ),
              ),
            const SizedBox(height: 8),
            Text(
              context.l10n.wslcTheTemplateExcludesTheContainerNameTemplatesAre,
              style: TextStyle(color: p.faint, fontSize: 12),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: Text(context.l10n.commonCancel)),
        FilledButton(onPressed: text.isEmpty ? null : _save, child: Text(context.l10n.commonSave)),
      ],
    );
  }
}

/// Wizard header menu: start from a template or save the current form.
class TemplateMenu extends StatelessWidget {
  const TemplateMenu({super.key, required this.controller, required this.current, required this.onLoad});
  final DeskController controller;

  /// The form as it stands; null while it cannot be built.
  final RunSpec? current;
  final ValueChanged<RunSpec> onLoad;

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final spec = current;
    Widget item(IconData icon, String title, [String? subtitle]) => Row(
      children: [
        Icon(icon, size: 18, color: p.muted),
        const SizedBox(width: 10),
        Flexible(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(title, overflow: TextOverflow.ellipsis),
              if (subtitle != null)
                Text(
                  subtitle,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: p.faint, fontSize: 11.5),
                ),
            ],
          ),
        ),
      ],
    );
    return PopupMenuButton<VoidCallback>(
      tooltip: context.l10n.wslcTemplates,
      icon: const Icon(Icons.bookmarks_outlined),
      constraints: const BoxConstraints(minWidth: 260, maxWidth: 360),
      onSelected: (action) => action(),
      itemBuilder: (context) => [
        for (final t in controller.templates)
          PopupMenuItem(value: () => onLoad(t.spec), child: item(Icons.bookmark_outline_rounded, t.name, t.spec.image)),
        if (controller.templates.isNotEmpty) const PopupMenuDivider(),
        PopupMenuItem(
          enabled: spec != null && spec.image.isNotEmpty,
          value: () => saveTemplateFlow(context, controller, spec!),
          child: item(Icons.bookmark_add_outlined, context.l10n.wslcSaveAsTemplateTemplates),
        ),
      ],
    );
  }
}
