import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show ProviderListenable;
import 'package:material_ui/material_ui.dart';

/// Obscured vault passphrase input of a step-up confirmation. [wrong] shows
/// the wrong-passphrase error under the field.
class VaultPassphraseField extends StatelessWidget {
  const VaultPassphraseField({
    required this.controller,
    super.key,
    this.wrong = false,
    this.enabled = true,
    this.autofocus = true,
    this.onChanged,
    this.onSubmitted,
  });

  final TextEditingController controller;
  final bool wrong, enabled, autofocus;
  final ValueChanged<String>? onChanged;
  final ValueChanged<String>? onSubmitted;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return SecretField(
      controller: controller,
      label: l.vaultPassphraseConfirmLabel,
      autofocus: autofocus,
      enabled: enabled,
      errorText: wrong ? l.vaultPassphraseConfirmWrong : null,
      textInputAction: TextInputAction.done,
      onChanged: onChanged,
      onSubmitted: onSubmitted,
    );
  }
}

/// Step-up check for dialogs that confirm a dangerous action with the vault
/// passphrase. The core verifies it without touching the lock state; the
/// typed text is cleared before the check and the copy is wiped after it.
mixin VaultPassphraseCheck<T extends ConsumerStatefulWidget> on ConsumerState<T> {
  final passphrase = TextEditingController();
  bool passphraseBusy = false, passphraseWrong = false;

  bool get passphraseEntered => passphrase.text.isNotEmpty;

  /// Verifies the typed passphrase; true when it matched. A wrong one
  /// leaves the error under the field.
  Future<bool> checkPassphrase() async {
    if (passphraseBusy || passphrase.text.isEmpty) return false;
    setState(() {
      passphraseBusy = true;
      passphraseWrong = false;
    });
    final secret = SecretText(passphrase.text);
    passphrase.clear();
    var ok = false;
    try {
      ok = await ref.read(vaultServiceProvider).verifyPassphrase(secret);
    } catch (_) {
      ok = false;
    } finally {
      secret.wipe();
    }
    if (mounted) {
      setState(() {
        passphraseBusy = false;
        passphraseWrong = !ok;
      });
    }
    return ok;
  }

  /// The passphrase field wired to this check.
  Widget passphraseField({Key? key, bool enabled = true, ValueChanged<String>? onSubmitted}) => VaultPassphraseField(
    key: key ?? const ValueKey('vault-passphrase-confirm'),
    controller: passphrase,
    wrong: passphraseWrong,
    enabled: enabled && !passphraseBusy,
    onChanged: (_) => setState(() => passphraseWrong = false),
    onSubmitted: onSubmitted,
  );

  @override
  void dispose() {
    passphrase
      ..clear()
      ..dispose();
    super.dispose();
  }
}

/// Confirms a dangerous action with the vault passphrase instead of a typed
/// name. [scope] is the vault scope the action belongs to: when it changes
/// (lock, profile switch) the dialog clears the field and closes unapproved;
/// [live] hides it once that scope is no longer usable. [details] (the
/// reviewed query, the resource) sits between [message] and the field.
/// With [expiresAt] the approval is refused after that moment.
Future<bool> showVaultPassphraseConfirmation<S>(
  BuildContext context, {
  required ProviderListenable<S> scope,
  required String title,
  required String message,
  required String confirmLabel,
  bool Function(S scope)? live,
  Widget? details,
  DateTime? expiresAt,
  String? expiredMessage,
  double width = 480,
  Key? dialogKey,
}) async =>
    await showAppDialog<bool>(
      context,
      secure: true,
      barrierDismissible: false,
      builder: (_) => VaultPassphraseConfirmation<S>(
        key: dialogKey ?? const ValueKey('vault-passphrase-confirmation'),
        scope: scope,
        live: live,
        title: title,
        message: message,
        confirmLabel: confirmLabel,
        details: details,
        expiresAt: expiresAt,
        expiredMessage: expiredMessage,
        width: width,
      ),
    ) ??
    false;

class VaultPassphraseConfirmation<S> extends ConsumerStatefulWidget {
  const VaultPassphraseConfirmation({
    required this.scope,
    required this.title,
    required this.message,
    required this.confirmLabel,
    super.key,
    this.live,
    this.details,
    this.expiresAt,
    this.expiredMessage,
    this.width = 480,
  });
  final ProviderListenable<S> scope;
  final bool Function(S scope)? live;
  final String title, message, confirmLabel;
  final Widget? details;
  final DateTime? expiresAt;
  final String? expiredMessage;
  final double width;
  @override
  ConsumerState<VaultPassphraseConfirmation<S>> createState() => _VaultPassphraseConfirmationState<S>();
}

class _VaultPassphraseConfirmationState<S> extends ConsumerState<VaultPassphraseConfirmation<S>>
    with VaultPassphraseCheck<VaultPassphraseConfirmation<S>> {
  late final S _scope = ref.read(widget.scope);
  Timer? _timer;
  bool _closing = false;

  bool get _expired => widget.expiresAt != null && !DateTime.now().isBefore(widget.expiresAt!);
  bool get _current => identical(_scope, ref.read(widget.scope)) && (widget.live?.call(_scope) ?? true);

  @override
  void initState() {
    super.initState();
    if (widget.expiresAt case final expiresAt?) {
      _timer = Timer(expiresAt.difference(DateTime.now()), () {
        if (mounted) setState(() {});
      });
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _submit() async {
    if (_closing || passphraseBusy || !passphraseEntered || _expired || !_current) return;
    final ok = await checkPassphrase();
    if (!mounted || !ok || _expired || !_current) return;
    _closing = true;
    closeDialog(context, true);
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(widget.scope, (_, next) {
      if (identical(next, _scope)) return;
      passphrase.clear();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final current = identical(_scope, ref.watch(widget.scope)) && (widget.live?.call(_scope) ?? true);
    if (!current) return const SizedBox.shrink();
    final l = context.l10n, tokens = GlassTokens.of(context);
    final expired = _expired;
    return GlassDialog(
      title: widget.title,
      icon: Icons.warning_rounded,
      iconTone: GlassTone.danger,
      width: widget.width,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(widget.message),
            if (widget.details case final details?) ...[const SizedBox(height: GlassSpacing.s12), details],
            const SizedBox(height: GlassSpacing.s12),
            passphraseField(enabled: !expired && !_closing, onSubmitted: (_) => unawaited(_submit())),
            if (expired && widget.expiredMessage != null) ...[
              const SizedBox(height: GlassSpacing.s8),
              Text(
                widget.expiredMessage!,
                key: const ValueKey('vault-passphrase-expired'),
                style: tokens.typography.caption.copyWith(color: tokens.palette.danger),
              ),
            ],
          ],
        ),
      ),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('vault-passphrase-cancel'),
          label: l.commonCancel,
          onPressed: () => closeDialog(context, false),
        ),
      ],
      primaryAction: GlassButton.destructive(
        key: const ValueKey('vault-passphrase-confirm-action'),
        label: widget.confirmLabel,
        busy: passphraseBusy,
        onPressed: expired || passphraseBusy || _closing || !passphraseEntered ? null : () => unawaited(_submit()),
      ),
    );
  }
}
