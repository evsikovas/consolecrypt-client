import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:material_ui/material_ui.dart';

class SudoPasswordDialog extends StatefulWidget {
  const SudoPasswordDialog({required this.host, this.retry = false, this.path, super.key});
  final String host;
  final bool retry;
  final String? path;

  @override
  State<SudoPasswordDialog> createState() => _SudoPasswordDialogState();
}

class _SudoPasswordDialogState extends State<SudoPasswordDialog> {
  final controller = TextEditingController();

  void submit() {
    if (ModalRoute.of(context)?.isCurrent != true || (widget.path == null && controller.text.isEmpty)) return;
    final secret = SecretText(controller.text);
    controller.clear();
    closeDialog(context, secret);
  }

  @override
  void dispose() {
    controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return GlassDialog(
      title: widget.path == null ? l.captureSudoPassword : l.editorSaveSudo,
      icon: Icons.admin_panel_settings_outlined,
      content: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          if (widget.path case final path?) ...[
            Text(widget.retry ? l.editorSudoDenied : l.editorSudoRequired),
            const SizedBox(height: 8),
            SelectableText('${widget.host} · $path'),
          ] else
            Text(widget.host),
          const SizedBox(height: 8),
          GlassField(
            key: const ValueKey('sudo-password'),
            placeholder: widget.path == null ? null : l.captureSudoPassword,
            controller: controller,
            secret: true,
            autofocus: true,
            onSubmitted: (_) => submit(),
          ),
          if (widget.retry && widget.path == null) Text(l.terminalPasswordRetry),
        ],
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<SecretText>(context))],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('sudo-confirm'),
        label: widget.path == null ? l.commonContinue : l.editorSaveSudo,
        onPressed: submit,
      ),
    );
  }
}
