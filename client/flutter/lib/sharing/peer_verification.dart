import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/sharing_dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:qr_flutter/qr_flutter.dart';

/// Status of a colleague device's keys (TR-13): "Chosen (not verified)",
/// "Verified (date)", "Keys changed" — icon, text and colour.
class PeerTrustBadge extends StatelessWidget {
  const PeerTrustBadge(this.trust, {this.verifiedAt, super.key});
  final PeerTrust trust;
  final DateTime? verifiedAt;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final (key, tone, icon, label) = switch (trust) {
      PeerTrust.verified => (
        'peer-trust-verified',
        GlassTone.success,
        Icons.verified_user_rounded,
        verifiedAt == null ? l.sharingTrustVerifiedNoDate : l.sharingTrustVerified(formatDate(l, verifiedAt!)),
      ),
      PeerTrust.chosen => ('peer-trust-chosen', GlassTone.warning, Icons.help_outline_rounded, l.sharingTrustChosen),
      PeerTrust.keysChanged => (
        'peer-trust-keys-changed',
        GlassTone.danger,
        Icons.gpp_bad_rounded,
        l.sharingTrustKeysChanged,
      ),
      PeerTrust.none => ('peer-trust-none', GlassTone.neutral, Icons.person_outline_rounded, l.sharingTrustNew),
    };
    final tokens = GlassTokens.of(context);
    final fg = tone == GlassTone.neutral ? tokens.secondaryLabel : tokens.palette.tone(tone);
    // Like GlassBadge, but the label wraps on narrow phones.
    return DecoratedBox(
      key: ValueKey(key),
      decoration: ShapeDecoration(
        color: tokens.palette.toneFill(tone).withValues(alpha: 0.16),
        shape: GlassRadii.shape(tokens.radii.md),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s8, vertical: GlassSpacing.s2),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 14, color: fg),
            const SizedBox(width: GlassSpacing.s4),
            Flexible(
              child: Text(
                label,
                style: tokens.typography.caption.copyWith(color: fg, fontWeight: FontWeight.w600),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Opens the key verification of a colleague device (TR-11). Resolves to
/// the updated pin when the user verified it, `null` otherwise.
Future<PeerPin?> showPeerVerification(BuildContext context, SharingIdentity identity, {String? email}) =>
    showAppDialog<PeerPin>(
      context,
      secure: true,
      builder: (_) => PeerVerificationDialog(identity: identity, email: email),
    );

enum _Method { safetyNumber, qrCode }

/// Safety number (typed — never pasted, never filled in) or QR code. The
/// numbers are computed by the core from both parties' keys; the dialog only
/// shows them and passes what the user entered back to the core.
class PeerVerificationDialog extends ConsumerStatefulWidget {
  const PeerVerificationDialog({required this.identity, this.email, super.key});
  final SharingIdentity identity;
  final String? email;
  @override
  ConsumerState<PeerVerificationDialog> createState() => _PeerVerificationDialogState();
}

class _PeerVerificationDialogState extends ConsumerState<PeerVerificationDialog> {
  late final Object? _scope = ref.read(sharingSessionScopeProvider);
  late final Future<SafetyNumber> _number = ref.read(sharingServiceProvider).safetyNumber(widget.identity);
  final _typed = TextEditingController();
  final _qr = TextEditingController();
  var _method = _Method.safetyNumber;
  var _busy = false;
  Object? _error;

  @override
  void dispose() {
    _typed.dispose();
    _qr.dispose();
    super.dispose();
  }

  bool get _ready => switch (_method) {
    _Method.safetyNumber => _typed.text.replaceAll(RegExp(r'\D'), '').length == 60,
    _Method.qrCode => _qr.text.trim().isNotEmpty,
  };

  Future<void> _verify() async {
    if (_busy || !_ready || !sharingSessionCurrent(ref, _scope)) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final pin = await ref
          .read(sharingServiceProvider)
          .verifyPeer(
            widget.identity,
            _method == _Method.safetyNumber ? 'safety_number' : 'qr_code',
            _method == _Method.safetyNumber ? _typed.text : _qr.text,
            email: widget.email,
          );
      if (!mounted || !sharingSessionCurrent(ref, _scope)) return;
      showSnack(context, context.l10n.peerVerifySuccess, tone: GlassTone.success);
      closeDialog<PeerPin>(context, pin);
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return SharingDialogGuard(
      profile: _scope,
      child: GlassDialog(
        key: const ValueKey('peer-verification-dialog'),
        title: l.peerVerifyTitle,
        icon: Icons.verified_user_outlined,
        width: 620,
        content: FutureBuilder<SafetyNumber>(
          future: _number,
          builder: (context, snap) {
            final number = snap.data;
            return ConstrainedBox(
              constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(context).height * .65),
              child: SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      l.peerVerifyDevice(widget.identity.deviceId),
                      style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                    ),
                    const SizedBox(height: GlassSpacing.s8),
                    PeerTrustBadge(widget.identity.trust, verifiedAt: widget.identity.verifiedAt),
                    const SizedBox(height: GlassSpacing.s12),
                    GlassSegmented<_Method>(
                      segments: [
                        GlassSegment(value: _Method.safetyNumber, label: l.peerVerifySafetyNumber),
                        GlassSegment(value: _Method.qrCode, label: l.peerVerifyQr),
                      ],
                      selected: _method,
                      onChanged: _busy ? null : (m) => setState(() => _method = m),
                    ),
                    const SizedBox(height: GlassSpacing.s12),
                    if (snap.hasError)
                      Text(errorMessage(l, snap.error!))
                    else if (number == null)
                      const LinearProgressIndicator()
                    else if (_method == _Method.safetyNumber) ...[
                      Text(l.peerVerifySafetyNumberHelp),
                      const SizedBox(height: GlassSpacing.s12),
                      Text(l.peerVerifyOwnNumber, style: tokens.typography.bodyEmph),
                      const SizedBox(height: GlassSpacing.s4),
                      _SafetyNumberView(number.groups),
                      const SizedBox(height: GlassSpacing.s12),
                      // Paste is disabled on purpose: the number must come from
                      // the colleague's voice, not from a message.
                      Actions(
                        actions: {PasteTextIntent: DoNothingAction(consumesKey: true)},
                        child: TextField(
                          key: const ValueKey('peer-verify-number'),
                          controller: _typed,
                          enabled: !_busy,
                          enableInteractiveSelection: false,
                          contextMenuBuilder: null,
                          keyboardType: TextInputType.number,
                          inputFormatters: [FilteringTextInputFormatter.allow(RegExp(r'[0-9 \-]'))],
                          decoration: InputDecoration(
                            labelText: l.peerVerifyEnterNumber,
                            helperText: l.peerVerifyNoPaste,
                          ),
                          onChanged: (_) => setState(() {}),
                        ),
                      ),
                    ] else ...[
                      Text(l.peerVerifyQrHelp),
                      const SizedBox(height: GlassSpacing.s12),
                      Center(
                        child: DecoratedBox(
                          key: const ValueKey('peer-verify-qr'),
                          decoration: ShapeDecoration(
                            color: const Color(0xFFFFFFFF),
                            shape: GlassRadii.shape(tokens.radii.md),
                          ),
                          child: Padding(
                            padding: const EdgeInsets.all(GlassSpacing.s12),
                            child: QrImageView(
                              data: number.ownQrPayload,
                              size: 200,
                              padding: EdgeInsets.zero,
                              backgroundColor: const Color(0xFFFFFFFF),
                            ),
                          ),
                        ),
                      ),
                      const SizedBox(height: GlassSpacing.s12),
                      TextField(
                        key: const ValueKey('peer-verify-qr-input'),
                        controller: _qr,
                        enabled: !_busy,
                        minLines: 1,
                        maxLines: 3,
                        decoration: InputDecoration(labelText: l.peerVerifyQrInput),
                        onChanged: (_) => setState(() {}),
                      ),
                    ],
                    if (_error != null) ...[
                      const SizedBox(height: GlassSpacing.s8),
                      Text(
                        errorMessage(l, _error!),
                        key: const ValueKey('peer-verify-error'),
                        style: TextStyle(color: tokens.palette.danger),
                      ),
                    ],
                  ],
                ),
              ),
            );
          },
        ),
        secondaryActions: [
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog<PeerPin>(context)),
        ],
        primaryAction: GlassButton.prominent(
          key: const ValueKey('peer-verify-confirm'),
          label: l.peerVerifyConfirm,
          onPressed: _busy || !_ready ? null : _verify,
        ),
      ),
    );
  }
}

class _SafetyNumberView extends StatelessWidget {
  const _SafetyNumberView(this.groups);
  final List<String> groups;
  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final rows = <String>[for (var i = 0; i < groups.length; i += 4) groups.skip(i).take(4).join('  ')];
    return DecoratedBox(
      key: const ValueKey('peer-safety-number'),
      decoration: ShapeDecoration(color: tokens.surfaces.inset, shape: GlassRadii.shape(tokens.radii.md)),
      child: Padding(
        padding: const EdgeInsets.all(GlassSpacing.s12),
        child: SelectionArea(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              for (final r in rows) Text(r, style: tokens.typography.code.copyWith(color: tokens.palette.label)),
            ],
          ),
        ),
      ),
    );
  }
}
