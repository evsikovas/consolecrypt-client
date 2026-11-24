import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:material_ui/material_ui.dart';

class CaptureSudoDialog extends StatefulWidget {
  const CaptureSudoDialog({required this.host, required this.retry, super.key});
  final String host;
  final bool retry;

  @override
  State<CaptureSudoDialog> createState() => _CaptureSudoDialogState();
}

class _CaptureSudoDialogState extends State<CaptureSudoDialog> {
  final controller = TextEditingController();

  void submit() {
    if (ModalRoute.of(context)?.isCurrent != true || controller.text.isEmpty) return;
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
      title: l.captureSudoPassword,
      icon: Icons.admin_panel_settings_outlined,
      content: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(widget.host),
          const SizedBox(height: 8),
          GlassField(controller: controller, secret: true, autofocus: true, onSubmitted: (_) => submit()),
          if (widget.retry) Text(l.terminalPasswordRetry),
        ],
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog<SecretText>(context))],
      primaryAction: GlassButton.prominent(label: l.commonContinue, onPressed: submit),
    );
  }
}
