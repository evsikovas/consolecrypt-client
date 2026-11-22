import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/wslc/app/desk_controller.dart';
import 'package:consolecrypt/wslc/app/theme.dart';
import 'package:consolecrypt/wslc/features/containers/run_wizard.dart';
import 'package:consolecrypt/wslc/shared/error_text.dart';
import 'package:consolecrypt/wslc/shared/ports.dart';
import 'package:consolecrypt/wslc/shared/widgets.dart';
import 'package:consolecrypt/wslc/src/rust/models.dart';
import 'package:consolecrypt/wslc/wslc_scope.dart';
import 'package:material_ui/material_ui.dart';

RunSpec respec(RunSpec s, {String? name, List<EnvVar>? env}) => RunSpec(
  image: s.image,
  name: name ?? s.name,
  ports: s.ports,
  mounts: s.mounts,
  env: env ?? s.env,
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

/// Shows `wslc run …` for an existing container and offers to clone it in the wizard.
Future<void> showRunCommand(BuildContext context, DeskController controller, String containerId) async {
  final notify = WslcApiScope.notifierOf(context);
  final RunSpec spec;
  try {
    spec = await WslcApiScope.of(context).containerRunSpec(containerId: containerId, includeEnv: true);
  } catch (e) {
    notify(errorText(e), error: true);
    return;
  }
  if (!context.mounted) return;
  final clone = await showWslcDialog<RunSpec>(
    context: context,
    builder: (_) => _RunCommandDialog(spec: spec),
  );
  if (clone == null || !context.mounted) return;
  final id = await showRunWizard(context, controller, initial: clone);
  if (id != null) controller.openContainer(id);
}

class _RunCommandDialog extends StatefulWidget {
  const _RunCommandDialog({required this.spec});
  final RunSpec spec;
  @override
  State<_RunCommandDialog> createState() => _RunCommandDialogState();
}

class _RunCommandDialogState extends State<_RunCommandDialog> {
  late final api = WslcApiScope.of(context);

  bool withEnv = false;

  RunSpec get shown => withEnv ? widget.spec : respec(widget.spec, env: const []);

  @override
  Widget build(BuildContext context) {
    final p = DeskPalette.of(context);
    final envCount = widget.spec.env.length;
    String command;
    try {
      command = api.copyRunCommand(spec: shown);
    } catch (e) {
      command = e is AppError ? e.message : errorText(e);
    }
    return Dialog(
      insetPadding: const EdgeInsets.all(40),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 760),
        child: Padding(
          padding: const EdgeInsets.all(22),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  const GradientIcon(Icons.code_rounded, size: 36),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      context.l10n.wslcStartupCommandContainerDetailsPage,
                      style: const TextStyle(fontSize: 18, fontWeight: FontWeight.w700),
                    ),
                  ),
                  IconButton(
                    tooltip: context.l10n.commonClose,
                    onPressed: () => Navigator.pop(context),
                    icon: const Icon(Icons.close_rounded),
                  ),
                ],
              ),
              const SizedBox(height: 6),
              Text(
                context.l10n.wslcPowerShellCommandSettingsThatMatchTheImageCMD,
                style: TextStyle(color: p.muted, fontSize: 12.5),
              ),
              const SizedBox(height: 14),
              ConstrainedBox(
                constraints: const BoxConstraints(minHeight: 90, maxHeight: 240),
                child: CodeView(command),
              ),
              if (envCount > 0) ...[
                const SizedBox(height: 10),
                SwitchListTile(
                  contentPadding: EdgeInsets.zero,
                  value: withEnv,
                  onChanged: (v) => setState(() => withEnv = v),
                  title: Text(context.l10n.wslcIncludeEnvironmentVariableValues(envCount)),
                  subtitle: Text(
                    context.l10n.wslcValuesMayContainPasswordsAndTokens,
                    style: TextStyle(color: p.warning, fontSize: 12),
                  ),
                ),
              ],
              const SizedBox(height: 14),
              Row(
                children: [
                  OutlinedButton.icon(
                    onPressed: () => Navigator.pop(
                      context,
                      respec(shown, name: widget.spec.name == null ? null : '${widget.spec.name}-copy'),
                    ),
                    icon: const Icon(Icons.copy_all_rounded, size: 18),
                    label: Text(context.l10n.wslcCloneInWizard),
                  ),
                  const Spacer(),
                  FilledButton.icon(
                    onPressed: () => copyText(context, command),
                    icon: const Icon(Icons.content_copy_rounded, size: 18),
                    label: Text(context.l10n.wslcCopy),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}
