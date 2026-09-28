import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

final _ownCodeProvider = FutureProvider.autoDispose<VerificationCode>(
  (ref) => ref.watch(devicesServiceProvider).currentDeviceVerificationCode(),
);

/// The *new* device's side of ADR-0004 approval: shows this device's
/// verification code and waits until a trusted device approves.
class ApprovalWaitScreen extends ConsumerWidget {
  const ApprovalWaitScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final code = ref.watch(_ownCodeProvider);
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
          Text(l10n.approvalWaitCodeLabel, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
          const SizedBox(height: GlassSpacing.s8),
          AsyncValueView(
            value: code,
            data: (c) => VerificationCodeView(code: c),
          ),
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
