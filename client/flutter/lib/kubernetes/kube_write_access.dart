import 'dart:async';

import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
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
      final approved =
          await showAppDialog<bool>(
            context,
            secure: true,
            barrierDismissible: false,
            builder: (_) => const _PassphraseConfirmation(),
          ) ??
          false;
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

Future<bool> showTypedKubeConfirmation(
  BuildContext context, {
  required String expected,
  required String title,
  required String message,
  required String confirmLabel,
}) async =>
    await showAppDialog<bool>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (_) =>
          _TypedConfirmation(expected: expected, title: title, message: message, confirmLabel: confirmLabel),
    ) ??
    false;

class _TypedConfirmation extends ConsumerStatefulWidget {
  const _TypedConfirmation({
    required this.expected,
    required this.title,
    required this.message,
    required this.confirmLabel,
  });
  final String expected, title, message, confirmLabel;
  @override
  ConsumerState<_TypedConfirmation> createState() => _TypedConfirmationState();
}

class _TypedConfirmationState extends ConsumerState<_TypedConfirmation> {
  final _text = TextEditingController();
  late final KubeScope _scope;
  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
  }

  @override
  void dispose() {
    _text.clear();
    _text.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      _text.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    return GlassDialog(
      key: const ValueKey('kube-typed-confirmation'),
      title: widget.title,
      content: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(widget.message),
            const SizedBox(height: 12),
            SelectableText(widget.expected, style: GlassTokens.of(context).typography.mono),
            const SizedBox(height: 12),
            TextField(
              key: const ValueKey('kube-confirm-name'),
              controller: _text,
              decoration: InputDecoration(labelText: l.kubeTypeName),
              autocorrect: false,
              enableSuggestions: false,
              enableIMEPersonalizedLearning: false,
              onChanged: (_) => setState(() {}),
            ),
          ],
        ),
      ),
      primaryAction: GlassButton.destructive(
        key: const ValueKey('kube-confirm-action'),
        label: widget.confirmLabel,
        onPressed:
            widget.expected.isNotEmpty &&
                _text.text == widget.expected &&
                identical(_scope, ref.read(kubeScopeProvider))
            ? () => closeDialog(context, true)
            : null,
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog(context, false))],
    );
  }
}

/// Step-up confirmation of a production write window: the vault passphrase
/// is checked by the core without touching the lock state.
class _PassphraseConfirmation extends ConsumerStatefulWidget {
  const _PassphraseConfirmation();
  @override
  ConsumerState<_PassphraseConfirmation> createState() => _PassphraseConfirmationState();
}

class _PassphraseConfirmationState extends ConsumerState<_PassphraseConfirmation> {
  final _text = TextEditingController();
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  bool _busy = false, _wrong = false;

  @override
  void dispose() {
    _text.clear();
    _text.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_busy || _text.text.isEmpty || !identical(_scope, ref.read(kubeScopeProvider))) return;
    setState(() {
      _busy = true;
      _wrong = false;
    });
    final passphrase = SecretText(_text.text);
    _text.clear();
    var ok = false;
    try {
      ok = await ref.read(vaultServiceProvider).verifyPassphrase(passphrase);
    } catch (_) {
      ok = false;
    } finally {
      passphrase.wipe();
    }
    if (!mounted) return;
    if (ok && identical(_scope, ref.read(kubeScopeProvider))) {
      closeDialog(context, true);
    } else {
      setState(() {
        _busy = false;
        _wrong = true;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      _text.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    return GlassDialog(
      key: const ValueKey('kube-passphrase-confirmation'),
      title: l.kubeAllowChanges,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.kubeAllowChangesHelp),
          const SizedBox(height: GlassSpacing.s12),
          GlassField(
            key: const ValueKey('kube-confirm-passphrase'),
            controller: _text,
            fieldKey: const ValueKey('kube-confirm-passphrase-input'),
            placeholder: l.kubeVaultPasswordConfirm,
            semanticLabel: l.kubeVaultPasswordConfirm,
            secret: true,
            autofocus: true,
            enabled: !_busy,
            errorText: _wrong ? l.kubeVaultPasswordWrong : null,
            onSubmitted: (_) => unawaited(_submit()),
            onChanged: (_) => setState(() => _wrong = false),
          ),
        ],
      ),
      primaryAction: GlassButton.destructive(
        key: const ValueKey('kube-confirm-action'),
        label: l.kubeAllowChanges,
        onPressed: _busy || _text.text.isEmpty ? null : () => unawaited(_submit()),
      ),
      secondaryActions: [GlassButton(label: l.commonCancel, onPressed: () => closeDialog(context, false))],
    );
  }
}
