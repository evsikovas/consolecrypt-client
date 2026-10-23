import 'dart:async';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

final _ownCodeProvider = FutureProvider.autoDispose<VerificationCode>(
  (ref) => ref.watch(devicesServiceProvider).currentDeviceVerificationCode(),
);

/// Protocol 1.11: polls the approval progress (the approver's announcement
/// and the pairing code over both devices' keys).
final _progressProvider = StreamProvider.autoDispose<ApprovalProgress?>((ref) {
  final service = ref.watch(devicesServiceProvider);
  final out = StreamController<ApprovalProgress?>();
  Future<void> poll() async {
    ApprovalProgress? progress;
    try {
      progress = await service.approvalProgress();
    } on Object {
      progress = null;
    }
    if (!out.isClosed) out.add(progress);
  }

  unawaited(poll());
  final timer = Timer.periodic(const Duration(seconds: 3), (_) => unawaited(poll()));
  ref.onDispose(() {
    timer.cancel();
    unawaited(out.close());
  });
  return out.stream;
});

/// The *new* device's side of ADR-0004 approval: shows this device's
/// verification code and waits until a trusted device approves. With a
/// protocol 1.11 server the approving device announces itself; both then
/// show the pairing code computed from **both** devices' keys, and this
/// device pins the approver only after the user confirmed the match here.
class ApprovalWaitScreen extends ConsumerStatefulWidget {
  const ApprovalWaitScreen({super.key});

  @override
  ConsumerState<ApprovalWaitScreen> createState() => _ApprovalWaitScreenState();
}

class _ApprovalWaitScreenState extends ConsumerState<ApprovalWaitScreen> {
  bool _matches = false, _busy = false;
  VerificationCode? _confirmedFor;

  Future<void> _confirm(ApprovalProgress progress) async {
    final code = progress.pairingCode;
    if (code == null || !_matches || _busy) return;
    setState(() => _busy = true);
    final ok = await runWithFeedback(context, () async {
      await ref.read(devicesServiceProvider).confirmApprover(progress, code);
      return true;
    });
    if (!mounted) return;
    setState(() {
      _busy = false;
      if (ok ?? false) _confirmedFor = code;
    });
  }

  Widget _pairing(BuildContext context, ApprovalProgress progress, VerificationCode code) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final name = progress.approverName ?? progress.approverDeviceId?.value ?? '';
    final confirmed = progress.approverConfirmed || _confirmedFor == code;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          l10n.approvalWaitPairingTitle(name),
          key: const ValueKey('approval-pairing-title'),
          style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label),
        ),
        const SizedBox(height: GlassSpacing.s4),
        Text(l10n.approvalWaitPairingHelp(name)),
        const SizedBox(height: GlassSpacing.s8),
        VerificationCodeView(key: const ValueKey('approval-pairing-code'), code: code),
        const SizedBox(height: GlassSpacing.s8),
        if (confirmed)
          Text(l10n.approvalWaitPairingConfirmed, key: const ValueKey('approval-pairing-confirmed'))
        else ...[
          // Own Material: the gate's secure surface has a background fill.
          Material(
            type: MaterialType.transparency,
            child: CheckboxListTile(
              key: const ValueKey('approval-pairing-matches'),
              contentPadding: EdgeInsets.zero,
              controlAffinity: ListTileControlAffinity.leading,
              value: _matches,
              onChanged: _busy ? null : (v) => setState(() => _matches = v ?? false),
              title: Text(l10n.approvalWaitPairingCheckbox(name)),
            ),
          ),
          Align(
            alignment: AlignmentDirectional.centerEnd,
            child: GlassButton.prominent(
              key: const ValueKey('approval-pairing-confirm'),
              label: l10n.approvalWaitPairingConfirm,
              onPressed: _matches && !_busy ? () => _confirm(progress) : null,
            ),
          ),
        ],
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final code = ref.watch(_ownCodeProvider);
    final progress = ref.watch(_progressProvider).value;
    final session = ref.watch(authStateProvider).value?.session;
    final dev = ref.watch(developerControlsProvider);
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    final deviceName = session?.deviceName;
    // Verification codes live on the secure material (§4.13): opaque, no
    // blur, nothing animated around the digits.
    return GateScaffold(
      maxWidth: 600,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GateHeader(
            icon: Icons.devices_rounded,
            title: l10n.approvalWaitTitle,
            subtitle: deviceName == null ? l10n.approvalWaitSubtitleUnnamed : l10n.approvalWaitSubtitle(deviceName),
          ),
          if (progress?.pairingCode case final pairing?)
            _pairing(context, progress!, pairing)
          else ...[
            Text(l10n.approvalWaitCodeLabel, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
            const SizedBox(height: GlassSpacing.s8),
            AsyncValueView(
              value: code,
              data: (c) => VerificationCodeView(code: c),
            ),
          ],
          const SizedBox(height: GlassSpacing.s16),
          Row(
            children: [
              const SizedBox.square(dimension: 16, child: CircularProgressIndicator(strokeWidth: 2)),
              const SizedBox(width: GlassSpacing.s12),
              Expanded(child: Text(l10n.approvalWaitWaiting)),
            ],
          ),
          const SizedBox(height: GlassSpacing.s16),
          InfoBanner(tone: BannerTone.warning, message: l10n.approvalWaitMismatchWarning),
          const SizedBox(height: GlassSpacing.s20),
          OverflowBar(
            alignment: MainAxisAlignment.spaceBetween,
            overflowAlignment: OverflowBarAlignment.end,
            spacing: GlassSpacing.s8,
            overflowSpacing: GlassSpacing.s8,
            children: [
              GlassButton(
                onPressed: () async {
                  await ref.read(vaultServiceProvider).cancelDeviceApproval();
                  if (context.mounted) context.go(AppRoutes.unlock);
                },
                label: l10n.commonCancel,
              ),
              if (dev != null)
                GlassButton(
                  key: const ValueKey('simulate-approval'),
                  onPressed: () async {
                    await dev.simulateApprovalFromOtherDevice();
                    if (context.mounted) context.go(AppRoutes.hosts);
                  },
                  label: l10n.approvalWaitSimulateApproval,
                ),
            ],
          ),
        ],
      ),
    );
  }
}
