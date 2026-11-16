import 'dart:async';

import 'package:consolecrypt/automation/automation_owner_confirmation.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/step_up.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/vault/vault_passphrase_confirmation.dart';
import 'package:material_ui/material_ui.dart';

final class AutomationOwnerConfirmationResult {
  const AutomationOwnerConfirmationResult.job(this.job) : error = null;
  const AutomationOwnerConfirmationResult.failed(this.error) : job = null;
  final AutomationJson? job;
  final Object? error;
}

/// Passphrase-only additional agreement. Cancellation never submits a proof;
/// wrong-passphrase retry retains the SAME Native review and original expiry.
class AutomationOwnerConfirmationDialog extends StatefulWidget {
  const AutomationOwnerConfirmationDialog({super.key, required this.service,
    required this.challenge, required this.original, this.utcNow});
  final AutomationProductionOwnerService service;
  final AutomationOwnerConfirmation challenge;
  final bool Function() original;
  final DateTime Function()? utcNow;
  @override
  State<AutomationOwnerConfirmationDialog> createState() => _AutomationOwnerConfirmationDialogState();
}

class _AutomationOwnerConfirmationDialogState extends State<AutomationOwnerConfirmationDialog> {
  final passphrase = TextEditingController();
  bool busy = false, wrong = false, retired = false;
  Timer? ticker;
  bool get current => mounted && !retired && widget.original() &&
      !widget.challenge.expired(widget.utcNow?.call() ?? DateTime.now().toUtc());
  @override
  void initState() {
    super.initState();
    ticker = Timer.periodic(const Duration(seconds: 1), (_) { if (!current) retire(); });
  }

  void retire() {
    if (retired || !mounted) return;
    finish(null);
  }

  void finish(AutomationOwnerConfirmationResult? result) {
    if (!mounted || retired) return;
    final route = ModalRoute.of(context);
    retired = true;
    passphrase.clear();
    // A command palette or another dialog may now be above this one. Retire
    // this exact route, never pop whichever unrelated route happens to be top.
    if (route != null && route.isActive) Navigator.of(context).removeRoute(route, result);
  }

  Future<void> confirm() async {
    if (busy || passphrase.text.isEmpty) return;
    if (!current) { retire(); return; }
    final proof = PassphraseProof(SecretText(passphrase.text));
    passphrase.clear();
    setState(() { busy = true; wrong = false; });
    try {
      final result = await widget.service.confirmOwnerRun(widget.challenge.reviewId, proof: proof);
      if (current) finish(AutomationOwnerConfirmationResult.job(result));
    } on AppException catch (error) {
      if (!current) return;
      if (error.code == AppErrorCode.wrongPassphrase) {
        setState(() => wrong = true);
      } else {
        finish(AutomationOwnerConfirmationResult.failed(error));
      }
    } catch (error) {
      if (current) finish(AutomationOwnerConfirmationResult.failed(error));
    } finally {
      proof.wipe();
      if (mounted && !retired) setState(() => busy = false);
    }
  }

  @override
  void dispose() {
    retired = true;
    ticker?.cancel();
    passphrase.clear();
    passphrase.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return AlertDialog(
      key: const ValueKey('automation-owner-confirmation-dialog'),
      title: Text(l.automationOwnerConfirmationTitle),
      content: SizedBox(width: 500, child: SingleChildScrollView(child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          InfoBanner(message: l.automationOwnerConfirmationHint, tone: BannerTone.warning),
          const SizedBox(height: 12),
          Text(l.automationOwnerManifestDigest),
          SelectableText(widget.challenge.manifestDigest, key: const ValueKey('automation-owner-manifest-digest')),
          const SizedBox(height: 12),
          VaultPassphraseField(key: const ValueKey('automation-owner-passphrase'),
            controller: passphrase, wrong: wrong, enabled: !busy,
            onChanged: (_) => setState(() => wrong = false), onSubmitted: (_) => confirm()),
          if (busy) const Padding(padding: EdgeInsets.only(top: 12), child: LinearProgressIndicator()),
        ],
      ))),
      actions: [
        TextButton(key: const ValueKey('automation-owner-cancel'),
          onPressed: busy ? null : () => finish(null), child: Text(l.commonCancel)),
        FilledButton(key: const ValueKey('automation-owner-confirm'),
          onPressed: busy || passphrase.text.isEmpty ? null : confirm, child: Text(l.automationOwnerConfirmAndRun)),
      ],
    );
  }
}
