import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/vault/vault_passphrase_confirmation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final kubeWriteAccessProvider = FutureProvider.autoDispose.family<KubeWriteAccess, KubeSession>((ref, session) {
  final service = ref.watch(kubeServiceProvider);
  if (service is! KubeMutationService) return KubeWriteAccess(production: session.production);
  return (service as KubeMutationService).writeAccess(session.id);
});

class KubeWriteBanner extends ConsumerStatefulWidget {
  const KubeWriteBanner({required this.session, required this.contextName, super.key, this.compact = false});
  final KubeSession session;
  final String contextName;

  /// One-line pill for toolbars instead of the full-width banner.
  final bool compact;
  @override
  ConsumerState<KubeWriteBanner> createState() => _KubeWriteBannerState();
}

class _KubeWriteBannerState extends ConsumerState<KubeWriteBanner> {
  late final KubeScope _scope;
  Timer? _timer;
  bool _busy = false, _failed = false;

  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));

  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    _timer = Timer.periodic(const Duration(seconds: 1), (_) {
      if (!_current) return;
      final access = ref.read(kubeWriteAccessProvider(widget.session)).value;
      if (access?.expiresAt != null) {
        setState(() {});
        if (!access!.canWrite) ref.invalidate(kubeWriteAccessProvider(widget.session));
      }
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _change(bool open) async {
    final service = ref.read(kubeServiceProvider);
    if (!_current || _busy || service is! KubeMutationService || (open && AppPlatform.isMobile)) return;
    if (open) {
      final l = context.l10n;
      final approved = await showVaultPassphraseConfirmation<KubeScope>(
        context,
        scope: kubeScopeProvider,
        title: l.kubeAllowChanges,
        message: l.kubeAllowChangesHelp,
        confirmLabel: l.kubeAllowChanges,
      );
      if (!_current || !approved) return;
    }
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final mutations = service as KubeMutationService;
      if (open) {
        await mutations.requestWriteWindow(widget.session.id, widget.contextName);
      } else {
        await mutations.closeWriteWindow(widget.session.id);
      }
      if (_current) ref.invalidate(kubeWriteAccessProvider(widget.session));
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final access = ref.watch(kubeWriteAccessProvider(widget.session)).value;
    final production = access?.production ?? widget.session.production;
    final active = production && (access?.canWrite ?? false);
    if (!production) return const SizedBox.shrink();
    final remaining = access?.expiresAt?.difference(DateTime.now().toUtc()).inSeconds ?? 0;
    final duration =
        '${(remaining.clamp(0, 600) ~/ 60).toString().padLeft(2, '0')}:${(remaining.clamp(0, 600) % 60).toString().padLeft(2, '0')}';
    if (widget.compact) {
      final tokens = GlassTokens.of(context);
      final p = tokens.palette;
      final tone = active ? GlassTone.warning : GlassTone.danger;
      final color = p.tone(tone);
      return DecoratedBox(
        key: const ValueKey('kube-write-banner'),
        decoration: ShapeDecoration(
          color: p.toneFill(tone).withValues(alpha: 0.14),
          shape: StadiumBorder(side: BorderSide(color: color.withValues(alpha: 0.35))),
        ),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(10, 2, 4, 2),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(active ? Icons.lock_open_rounded : Icons.lock_rounded, size: 14, color: color),
              const SizedBox(width: 6),
              Flexible(
                child: Text(
                  active ? l.kubeWriteWindowRemaining(duration) : l.kubeReadOnly,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: tokens.typography.callout.copyWith(color: p.label, fontWeight: FontWeight.w600),
                ),
              ),
              if (_failed) ...[
                const SizedBox(width: 6),
                Tooltip(
                  message: l.kubeWriteWindowFailed,
                  child: Icon(Icons.error_outline, size: 14, color: p.danger),
                ),
              ],
              if ((access?.canRequestWindow == true && !AppPlatform.isMobile) || active) ...[
                const SizedBox(width: 2),
                GlassIconButton(
                  key: const ValueKey('kube-write-window'),
                  icon: active ? Icons.lock_rounded : Icons.lock_open_rounded,
                  tooltip: active ? l.kubeCloseWriteWindow : l.kubeAllowChanges,
                  style: GlassIconButtonStyle.plain,
                  size: 24,
                  iconSize: 15,
                  onPressed: _busy ? null : () => _change(!active),
                ),
              ] else
                const SizedBox(width: 6),
            ],
          ),
        ),
      );
    }
    return Container(
      key: const ValueKey('kube-write-banner'),
      padding: const EdgeInsets.all(10),
      margin: const EdgeInsets.only(bottom: 8),
      decoration: BoxDecoration(
        border: Border.all(color: Theme.of(context).colorScheme.error),
        borderRadius: BorderRadius.circular(10),
        color: active ? Theme.of(context).colorScheme.errorContainer : null,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Icon(active ? Icons.lock_open : Icons.lock_outline, size: 18),
              const SizedBox(width: 8),
              Expanded(child: Text(active ? l.kubeWriteWindowRemaining(duration) : l.kubeReadOnly)),
            ],
          ),
          if (_failed) Text(l.kubeWriteWindowFailed),
          if ((access?.canRequestWindow == true && !AppPlatform.isMobile) || active)
            Align(
              alignment: Alignment.centerLeft,
              child: GlassButton.plain(
                key: const ValueKey('kube-write-window'),
                label: active ? l.kubeCloseWriteWindow : l.kubeAllowChanges,
                onPressed: _busy ? null : () => _change(!active),
              ),
            ),
        ],
      ),
    );
  }
}

/// Confirms a cluster change (or the removal of saved data) with the vault
/// passphrase. [subject] — the resource or context it applies to — is shown
/// for review. Closes unapproved when the cluster scope changes.
Future<bool> showKubePassphraseConfirmation(
  BuildContext context, {
  required String title,
  required String message,
  required String confirmLabel,
  String? subject,
}) => showVaultPassphraseConfirmation<KubeScope>(
  context,
  scope: kubeScopeProvider,
  title: title,
  message: message,
  confirmLabel: confirmLabel,
  details: subject == null || subject.isEmpty
      ? null
      : SelectableText(
          subject,
          key: const ValueKey('kube-confirm-subject'),
          style: GlassTokens.of(context).typography.mono,
        ),
);
