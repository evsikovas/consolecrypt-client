import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:consolecrypt/devices/devices_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Device approval (LIQUID_GLASS_SPEC §4.13) — maximum legibility: the
/// secure material at α 1.0 regardless of the Glass setting, a darker
/// barrier, and no live blur anywhere on screen while it is open.
Future<void> showApproveDeviceDialog(BuildContext context, DeviceTrustRequest request) => showGlassDialog<void>(
  context,
  variant: GlassVariant.secure,
  opaque: true,
  barrierDismissible: false,
  builder: (_) => ApproveDeviceDialog(request: request),
);

/// The approving (trusted) device's side of ADR-0004: shows the code computed
/// locally from the keys the server reports for the new device. The user must
/// explicitly confirm it equals the code on the new device's screen — the only
/// defence against a malicious server substituting its own key. Approve stays
/// disabled until then; "Codes don't match" rejects the request and the same
/// dialog turns into a warning.
class ApproveDeviceDialog extends ConsumerStatefulWidget {
  const ApproveDeviceDialog({required this.request, super.key});

  final DeviceTrustRequest request;

  @override
  ConsumerState<ApproveDeviceDialog> createState() => _ApproveDeviceDialogState();
}

class _ApproveDeviceDialogState extends ConsumerState<ApproveDeviceDialog> {
  late final Future<VerificationCode> _code = ref
      .read(devicesServiceProvider)
      .verificationCodeFor(widget.request.requestId);
  bool _confirmed = false;
  bool _busy = false;
  bool _rejected = false;
  AppException? _error;

  Future<void> _approve(VerificationCode code) async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await ref.read(devicesServiceProvider).approve(widget.request.requestId, confirmedCode: code);
      if (mounted) {
        final message = context.l10n.approveDeviceApproved(widget.request.device.name);
        showSnack(context, message, tone: GlassTone.success);
        closeDialog<void>(context);
      }
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _mismatch() async {
    setState(() => _busy = true);
    try {
      await ref.read(devicesServiceProvider).reject(widget.request.requestId);
    } on AppException catch (_) {
      // The warning below matters more than a failed reject.
    }
    if (mounted) {
      setState(() {
        _busy = false;
        _rejected = true;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final device = widget.request.device;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    if (_rejected) {
      return GlassDialog(
        key: const ValueKey('approve-device-dialog'),
        width: 600,
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(Icons.gpp_bad_rounded, size: 40, color: tokens.palette.danger),
            const SizedBox(height: GlassSpacing.s12),
            Semantics(
              header: true,
              child: Text(
                l10n.approveDeviceRejectedTitle,
                key: const ValueKey('approve-device-rejected'),
                style: t.title2.copyWith(color: tokens.palette.label),
              ),
            ),
            const SizedBox(height: GlassSpacing.s8),
            Text(l10n.approveDeviceRejectedMessage),
          ],
        ),
        primaryAction: GlassButton.prominent(
          autofocus: true,
          onPressed: () => closeDialog<void>(context),
          label: l10n.commonOk,
        ),
      );
    }
    final requested = widget.request.expiresAt.isAfter(DateTime.now())
        ? l10n.devicesPendingMeta(
            device.platform.localized(l10n),
            formatRelative(l10n, widget.request.createdAt),
            formatRemaining(l10n, widget.request.expiresAt),
          )
        : l10n.devicesPendingMetaExpired(
            device.platform.localized(l10n),
            formatRelative(l10n, widget.request.createdAt),
          );
    return FutureBuilder<VerificationCode>(
      future: _code,
      builder: (context, snap) {
        final code = snap.data;
        return GlassDialog(
          key: const ValueKey('approve-device-dialog'),
          icon: platformIcon(device.platform),
          iconTone: GlassTone.neutral,
          title: l10n.approveDeviceTitle(device.name),
          width: 600,
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(requested, style: t.callout.copyWith(color: tokens.secondaryLabel)),
              const SizedBox(height: GlassSpacing.s16),
              if (snap.hasError)
                GateErrorText(text: errorMessage(l10n, snap.error!))
              else if (code == null)
                const SizedBox(height: 120, child: Center(child: CircularProgressIndicator()))
              else ...[
                Text(l10n.approveDeviceStepShowsCode(device.name)),
                const SizedBox(height: GlassSpacing.s4),
                Text(l10n.approveDeviceStepCompare),
                const SizedBox(height: GlassSpacing.s16),
                VerificationCodeView(code: code),
                const SizedBox(height: GlassSpacing.s16),
                CheckboxListTile(
                  key: const ValueKey('codes-match'),
                  contentPadding: EdgeInsets.zero,
                  controlAffinity: ListTileControlAffinity.leading,
                  value: _confirmed,
                  onChanged: _busy ? null : (v) => setState(() => _confirmed = v ?? false),
                  title: Text(l10n.approveDeviceCodesMatchCheckbox(device.name)),
                ),
                const SizedBox(height: GlassSpacing.s4),
                InfoBanner(tone: BannerTone.warning, message: l10n.approveDeviceWarning),
                if (_error != null) ...[
                  const SizedBox(height: GlassSpacing.s8),
                  GateErrorText(key: const ValueKey('approve-error'), text: errorMessage(l10n, _error!)),
                ],
              ],
            ],
          ),
          leadingAction: GlassButton(
            key: const ValueKey('codes-mismatch'),
            style: GlassButtonStyle.destructiveQuiet,
            icon: Icons.gpp_bad_rounded,
            onPressed: _busy ? null : _mismatch,
            label: l10n.approveDeviceCodesDontMatch,
          ),
          secondaryActions: [
            GlassButton.plain(
              autofocus: true,
              onPressed: _busy ? null : () => closeDialog<void>(context),
              label: l10n.commonCancel,
            ),
          ],
          primaryAction: GlassButton.prominent(
            key: const ValueKey('approve-device'),
            onPressed: code != null && _confirmed && !_busy ? () => _approve(code) : null,
            label: l10n.approveDeviceApprove,
          ),
        );
      },
    );
  }
}
