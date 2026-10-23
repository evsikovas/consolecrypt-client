import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/vault/user_presence.dart';
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
/// passphrase — or, where this device offers it, with the OS check (Touch
/// ID / Windows Hello / fingerprint, see [UserPresence]). Cancelled or
/// failed biometry falls back to the passphrase field. The typed text is
/// cleared before the check and the copy is wiped after it.
mixin VaultPassphraseCheck<T extends ConsumerStatefulWidget> on ConsumerState<T> {
  final passphrase = TextEditingController();
  bool stepUpBusy = false, passphraseWrong = false;

  /// The OS check failed, was cancelled or refused: the passphrase field
  /// is shown with a note.
  bool presenceFailed = false;
  bool _passphraseChosen = false;

  /// What this confirmation is for; the presence token is bound to it.
  StepUpAction get stepUpAction => StepUpAction.confirm;

  /// The session / object the token is bound to.
  String get stepUpObject => '';

  /// What the OS prompt says the app is trying to do.
  String get stepUpReason => context.l10n.stepUpOsReason;

  /// The OS check offered now (null: the passphrase field is shown).
  DeviceAuthKind? get presenceKind => _passphraseChosen ? null : ref.read(userPresenceProvider).kindFor(stepUpAction);

  /// The confirm action can start: the OS check is offered or a
  /// passphrase is typed.
  bool get stepUpReady => presenceKind != null || passphrase.text.isNotEmpty;

  void usePassphrase() => setState(() {
    _passphraseChosen = true;
    presenceFailed = false;
  });

  void usePresence() => setState(() {
    _passphraseChosen = false;
    presenceFailed = false;
    passphraseWrong = false;
    passphrase.clear();
  });

  /// The proof for this step-up: the OS prompt's presence token, or the
  /// typed passphrase. Null when nothing was obtained (the OS check
  /// failed or was cancelled → passphrase mode). The caller wipes it.
  Future<StepUpProof?> obtainStepUp() async {
    if (stepUpBusy) return null;
    if (presenceKind != null) {
      setState(() {
        stepUpBusy = true;
        passphraseWrong = false;
        presenceFailed = false;
      });
      try {
        final token = await ref.read(userPresenceProvider).verify(stepUpAction, stepUpObject, reason: stepUpReason);
        return PresenceProof(token);
      } on Object {
        // Cancelled, failed, locked out, invalidated or not offered.
        if (mounted) {
          setState(() {
            _passphraseChosen = true;
            presenceFailed = true;
          });
        }
        return null;
      } finally {
        if (mounted) setState(() => stepUpBusy = false);
      }
    }
    if (passphrase.text.isEmpty) return null;
    final secret = SecretText(passphrase.text);
    passphrase.clear();
    return PassphraseProof(secret);
  }

  /// Marks [proof] as refused by the core: a wrong passphrase stays under
  /// the field, a refused token switches to the passphrase.
  void stepUpRejected(StepUpProof proof) => setState(() {
    if (proof is PassphraseProof) {
      passphraseWrong = true;
    } else {
      _passphraseChosen = true;
      presenceFailed = true;
    }
  });

  /// For confirmations the app enforces itself: obtains the proof and
  /// checks it now; true when it checked out.
  Future<bool> checkStepUp() async {
    final proof = await obtainStepUp();
    if (proof == null || !mounted) {
      proof?.wipe();
      return false;
    }
    setState(() {
      stepUpBusy = true;
      passphraseWrong = false;
    });
    var ok = false;
    try {
      ok = await ref.read(userPresenceProvider).confirm(proof, action: stepUpAction, object: stepUpObject);
    } catch (_) {
      ok = false;
    } finally {
      proof.wipe();
    }
    if (mounted) {
      setState(() => stepUpBusy = false);
      if (!ok) stepUpRejected(proof);
    }
    return ok;
  }

  /// The passphrase field, or in OS-check mode a line saying how the
  /// action is confirmed ([hint]) with the switch to the passphrase.
  Widget stepUpField({Key? key, bool enabled = true, bool hint = true, ValueChanged<String>? onSubmitted}) {
    final l = context.l10n, tokens = GlassTokens.of(context);
    final offered = ref.watch(userPresenceProvider).kindFor(stepUpAction);
    final kind = _passphraseChosen ? null : offered;
    if (kind != null) {
      return Column(
        key: const ValueKey('step-up-presence'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          if (hint)
            Row(
              children: [
                Icon(presenceIcon(kind), size: 16, color: tokens.secondaryLabel),
                const SizedBox(width: GlassSpacing.s6),
                Expanded(
                  child: Text(
                    l.stepUpPresenceHint(DeviceUnlockInfo(kind: kind).authName(l)),
                    style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                  ),
                ),
              ],
            ),
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton.plain(
              key: const ValueKey('step-up-use-passphrase'),
              label: l.stepUpUsePassphrase,
              icon: Icons.password_rounded,
              onPressed: enabled && !stepUpBusy ? usePassphrase : null,
            ),
          ),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (presenceFailed) ...[
          Text(
            l.stepUpPresenceFailed,
            key: const ValueKey('step-up-presence-failed'),
            style: tokens.typography.caption.copyWith(color: tokens.palette.warning),
          ),
          const SizedBox(height: GlassSpacing.s6),
        ],
        VaultPassphraseField(
          key: key ?? const ValueKey('vault-passphrase-confirm'),
          controller: passphrase,
          wrong: passphraseWrong,
          enabled: enabled && !stepUpBusy,
          onChanged: (_) => setState(() => passphraseWrong = false),
          onSubmitted: onSubmitted,
        ),
        if (offered != null)
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton.plain(
              key: const ValueKey('step-up-use-presence'),
              label: presenceConfirmLabel(l, offered),
              icon: presenceIcon(offered),
              onPressed: enabled && !stepUpBusy ? usePresence : null,
            ),
          ),
      ],
    );
  }

  @override
  void dispose() {
    passphrase
      ..clear()
      ..dispose();
    super.dispose();
  }
}

/// "Confirm with Touch ID" / "… Windows Hello" / "… fingerprint".
String presenceConfirmLabel(AppLocalizations l, DeviceAuthKind kind) => switch (kind) {
  DeviceAuthKind.touchId => l.stepUpConfirmTouchId,
  DeviceAuthKind.faceId => l.stepUpConfirmFaceId,
  DeviceAuthKind.windowsHello => l.stepUpConfirmWindowsHello,
  DeviceAuthKind.deviceCredential => l.stepUpConfirmDeviceCredential,
  DeviceAuthKind.fingerprint || DeviceAuthKind.keyring => l.stepUpConfirmFingerprint,
};

IconData presenceIcon(DeviceAuthKind kind) => switch (kind) {
  DeviceAuthKind.faceId => Icons.face_rounded,
  DeviceAuthKind.deviceCredential => Icons.password_rounded,
  _ => Icons.fingerprint_rounded,
};

/// Confirms a dangerous action with the vault passphrase instead of a typed
/// name. [scope] is the vault scope the action belongs to: when it changes
/// (lock, profile switch) the dialog clears the field and closes unapproved;
/// [live] hides it once that scope is no longer usable. [details] (the
/// reviewed query, the resource) sits between [message] and the field.
/// With [expiresAt] the approval is refused after that moment. Where the
/// device offers it, the primary action is "Confirm with Touch ID" (or
/// Windows Hello / fingerprint) for [action] on [object]; passphrase-only
/// actions never offer it.
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
  StepUpAction action = StepUpAction.confirm,
  String object = '',
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
        action: action,
        object: object,
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
    this.action = StepUpAction.confirm,
    this.object = '',
  });
  final ProviderListenable<S> scope;
  final bool Function(S scope)? live;
  final String title, message, confirmLabel;
  final Widget? details;
  final DateTime? expiresAt;
  final String? expiredMessage;
  final double width;
  final StepUpAction action;
  final String object;
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
  StepUpAction get stepUpAction => widget.action;

  @override
  String get stepUpObject => widget.object;

  @override
  String get stepUpReason => context.l10n.stepUpOsReasonFor(widget.title);

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
    if (_closing || stepUpBusy || !stepUpReady || _expired || !_current) return;
    final ok = await checkStepUp();
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
    ref.watch(userPresenceProvider);
    final presence = presenceKind;
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
            stepUpField(enabled: !expired && !_closing, hint: false, onSubmitted: (_) => unawaited(_submit())),
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
        label: presence == null ? widget.confirmLabel : presenceConfirmLabel(l, presence),
        icon: presence == null ? null : presenceIcon(presence),
        busy: stepUpBusy,
        onPressed: expired || stepUpBusy || _closing || !stepUpReady ? null : () => unawaited(_submit()),
      ),
    );
  }
}
