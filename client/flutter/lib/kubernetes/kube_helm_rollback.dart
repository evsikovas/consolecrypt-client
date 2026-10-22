import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_flux_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:consolecrypt/vault/vault_passphrase_confirmation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeHelmRollbackDialog extends ConsumerStatefulWidget {
  const KubeHelmRollbackDialog({required this.session, required this.release, required this.service, super.key});
  final KubeSession session;
  final KubeHelmSummary release;
  final KubeHelmCliService service;
  @override
  ConsumerState<KubeHelmRollbackDialog> createState() => _KubeHelmRollbackDialogState();
}

class _KubeHelmRollbackDialogState extends ConsumerState<KubeHelmRollbackDialog>
    with VaultPassphraseCheck<KubeHelmRollbackDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _path = TextEditingController();
  HelmRollbackReview? _review;
  FluxCliOutput? _output;
  bool _busy = false, _failed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  bool get _canWrite => _current && (ref.read(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false);
  void _discard() {
    final review = _review;
    _review = null;
    if (review != null) unawaited(widget.service.cancelHelmRollback(review.ticket).catchError((Object _) {}));
  }

  @override
  void dispose() {
    _discard();
    _output?.dispose();
    _path.dispose();
    super.dispose();
  }

  Future<void> _prepare() async {
    if (!_canWrite || _busy) return;
    _discard();
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final review = await widget.service.reviewHelmRollback(
        widget.session.id,
        widget.release.namespace,
        widget.release.name,
        widget.release.revision,
        manualPath: _path.text.trim().isEmpty ? null : _path.text.trim(),
      );
      if (!_current) {
        await widget.service.cancelHelmRollback(review.ticket);
        return;
      }
      setState(() => _review = review);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  bool _reviewUsable(HelmRollbackReview? review) =>
      _canWrite &&
      !_busy &&
      review != null &&
      identical(review, _review) &&
      !review.expiresAt.isBefore(DateTime.now().toUtc());

  Future<void> _apply() async {
    final review = _review;
    if (!_reviewUsable(review) || !passphraseEntered || passphraseBusy) return;
    if (!await checkPassphrase() || !_reviewUsable(review)) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final output = await widget.service.approveHelmRollback(review!.ticket);
      if (!_current) {
        output.dispose();
        return;
      }
      _review = null;
      setState(() {
        _output = output;
        _failed = output.exitCode != 0;
      });
    } catch (_) {
      _discard();
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      _discard();
      _output?.dispose();
      _output = null;
      passphrase.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    ref.watch(kubeWriteAccessProvider(widget.session));
    final l = context.l10n;
    final review = _review;
    return GlassDialog(
      title: l.kubeHelmRollback,
      width: 950,
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 550),
        child: !_current
            ? const SizedBox.shrink()
            : SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      '${widget.release.namespace}/${widget.release.name} · ${l.kubeHelmRevision(widget.release.revision)}',
                    ),
                    Text(l.kubeHelmRollbackHelp),
                    if (review == null && _output == null)
                      TextField(
                        key: const ValueKey('kube-helm-binary'),
                        controller: _path,
                        enabled: !_busy,
                        autocorrect: false,
                        enableSuggestions: false,
                        decoration: InputDecoration(labelText: l.kubeHelmBinary),
                      ),
                    if (review != null) ...[
                      SelectableText(
                        // l10n-ignore: Reviewed executable path, SHA-256 digest, revision numbers and exact JSON argv.
                        /* l10n-ignore: Reviewed executable path, SHA-256 digest, revision numbers and exact JSON argv. */ '${review.path}\nSHA-256: ${review.sha256}\n${review.currentRevision} → ${review.revision}\n${jsonEncode(review.arguments)}',
                      ),
                      const SizedBox(height: GlassSpacing.s12),
                      passphraseField(
                        key: const ValueKey('kube-helm-rollback-passphrase'),
                        enabled: !_busy,
                        onSubmitted: (_) => unawaited(_apply()),
                      ),
                    ],
                    if (_busy) const LinearProgressIndicator(),
                    if (_failed) Text(l.kubeHelmRollbackFailed),
                    if (_output != null) SelectableText('${_output!.stdout}\n${_output!.stderr}'),
                  ],
                ),
              ),
      ),
      primaryAction: _output != null
          ? GlassButton(label: l.commonClose, onPressed: () => closeDialog(context, true))
          : review == null
          ? GlassButton(
              key: const ValueKey('kube-helm-rollback-review'),
              label: l.kubeReviewChanges,
              onPressed: _canWrite && !_busy ? _prepare : null,
            )
          : GlassButton.destructive(
              key: const ValueKey('kube-helm-rollback-apply'),
              label: l.kubeHelmRollback,
              busy: passphraseBusy,
              onPressed: _canWrite && !_busy && !passphraseBusy && passphraseEntered ? () => unawaited(_apply()) : null,
            ),
      secondaryActions: _output != null
          ? const []
          : [GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog(context, false))],
    );
  }
}
