import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Shows an app dialog on glass (LIQUID_GLASS_SPEC §4.7). [builder] normally
/// returns a [GlassDialog]. Standard dialogs are live `glass.thick`; pass
/// [secure] for secrets, trust decisions and risky commands (opaque
/// `glass.secure`, no blur, no animated contents).
Future<T?> showAppDialog<T>(
  BuildContext context, {
  required WidgetBuilder builder,
  bool secure = false,
  bool barrierDismissible = true,
}) => showGlassDialog<T>(
  context,
  builder: builder,
  opaque: secure,
  variant: secure ? GlassVariant.secure : GlassVariant.thick,
  barrierDismissible: barrierDismissible,
);

/// Pops the dialog around [context] with [result].
void closeDialog<T>(BuildContext context, [T? result]) => Navigator.of(context).pop(result);

/// Yes/no confirmation on a glass dialog. Destructive confirmations use the
/// danger-filled button (the only place it is allowed, §4.2), a danger
/// icon, and initial focus on Cancel.
Future<bool> showConfirmDialog(
  BuildContext context, {
  required String title,
  required String message,
  required String confirmLabel,
  bool destructive = false,
  bool secure = false,
  Widget? extra,
}) async {
  final result = await showAppDialog<bool>(
    context,
    secure: secure,
    builder: (context) {
      final l10n = context.l10n;
      return GlassDialog(
        title: title,
        icon: destructive ? Icons.warning_rounded : null,
        iconTone: GlassTone.danger,
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(message),
            if (extra != null) ...[const SizedBox(height: GlassSpacing.s12), extra],
          ],
        ),
        secondaryActions: [
          GlassButton(
            key: const ValueKey('confirm-cancel'),
            label: l10n.commonCancel,
            autofocus: destructive,
            onPressed: () => closeDialog(context, false),
          ),
        ],
        primaryAction: destructive
            ? GlassButton.destructive(
                key: const ValueKey('confirm-ok'),
                label: confirmLabel,
                onPressed: () => closeDialog(context, true),
              )
            : GlassButton.prominent(
                key: const ValueKey('confirm-ok'),
                label: confirmLabel,
                autofocus: true,
                onPressed: () => closeDialog(context, true),
              ),
        onSubmit: destructive ? null : () => closeDialog(context, true),
      );
    },
  );
  return result ?? false;
}

/// Toast (LIQUID_GLASS_SPEC §4.11): glass capsule at the bottom centre.
/// Errors stay until dismissed.
void showSnack(BuildContext context, String message, {bool error = false, GlassTone? tone}) {
  if (!context.mounted) return;
  final effective = tone ?? (error ? GlassTone.danger : GlassTone.neutral);
  GlassToast.show(
    context,
    GlassToastRequest(
      message: message,
      tone: effective,
      icon: switch (effective) {
        GlassTone.danger => Icons.error_rounded,
        GlassTone.warning => Icons.warning_rounded,
        GlassTone.success => Icons.check_circle_rounded,
        _ => Icons.info_rounded,
      },
    ),
  );
}

/// Runs [action]; on [AppException] shows its localized message.
/// Returns `null` on failure.
Future<T?> runWithFeedback<T>(BuildContext context, Future<T> Function() action, {String? success}) async {
  try {
    final result = await action();
    if (success != null && context.mounted) showSnack(context, success, tone: GlassTone.success);
    return result;
  } on AppException catch (e) {
    if (context.mounted) showSnack(context, errorMessage(context.l10n, e), error: true);
    return null;
  }
}

/// Copies a secret; the clipboard is cleared automatically. The toast shows
/// a countdown ring and "Clears in N s", never the value (§4.11). [what] is a
/// localized noun from the `copyWhat*` keys (defaults to "Secret").
Future<void> copySecretWithNotice(BuildContext context, WidgetRef ref, String value, {String? what}) async {
  final clipboard = ref.read(secureClipboardProvider);
  await clipboard.copySecret(value);
  if (context.mounted) {
    final l10n = context.l10n;
    GlassToast.show(
      context,
      GlassToastRequest(message: l10n.copiedNotice(what ?? l10n.copyWhatSecret), countdown: clipboard.clearAfter),
    );
  }
}

/// Copies non-secret text (public keys, fingerprints, commands). [what] is
/// a localized noun from the `copyWhat*` keys (defaults to "Text").
Future<void> copyPlainWithNotice(BuildContext context, WidgetRef ref, String value, {String? what}) async {
  await ref.read(secureClipboardProvider).copyPlain(value);
  if (context.mounted) {
    showSnack(context, context.l10n.copiedNotice(what ?? context.l10n.copyWhatText), tone: GlassTone.success);
  }
}

/// Single-field text prompt (rename, new folder…). Returns `null` on cancel.
/// The dialog owns its controller, so it is disposed only after the route's
/// exit animation (disposing it right after the dialog returns can crash).
Future<String?> showTextInputDialog(
  BuildContext context, {
  required String title,
  String initial = '',
  String? confirmLabel,
  String? label,
}) => showAppDialog<String>(
  context,
  builder: (context) => _TextInputDialog(
    title: title,
    initial: initial,
    confirmLabel: confirmLabel ?? context.l10n.commonSave,
    label: label,
  ),
);

class _TextInputDialog extends StatefulWidget {
  const _TextInputDialog({required this.title, required this.initial, required this.confirmLabel, this.label});

  final String title;
  final String initial;
  final String confirmLabel;
  final String? label;

  @override
  State<_TextInputDialog> createState() => _TextInputDialogState();
}

class _TextInputDialogState extends State<_TextInputDialog> {
  late final TextEditingController _controller = TextEditingController(text: widget.initial);

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => GlassDialog(
    title: widget.title,
    width: 440,
    content: TextField(
      key: const ValueKey('text-input-dialog-field'),
      controller: _controller,
      autofocus: true,
      decoration: InputDecoration(labelText: widget.label),
      onSubmitted: (v) => closeDialog(context, v),
    ),
    secondaryActions: [GlassButton(label: context.l10n.commonCancel, onPressed: () => closeDialog<String>(context))],
    primaryAction: GlassButton.prominent(
      label: widget.confirmLabel,
      onPressed: () => closeDialog(context, _controller.text),
    ),
  );
}
