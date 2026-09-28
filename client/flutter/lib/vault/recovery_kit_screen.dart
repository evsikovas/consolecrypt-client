import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:consolecrypt/vault/onboarding_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';
import 'package:qr_flutter/qr_flutter.dart';

/// Onboarding step 2: the printable Recovery Kit (vault ID, 24 words, QR,
/// server, date). Words and QR stay hidden until explicitly revealed.
class RecoveryKitScreen extends ConsumerStatefulWidget {
  const RecoveryKitScreen({super.key});

  @override
  ConsumerState<RecoveryKitScreen> createState() => _RecoveryKitScreenState();
}

class _RecoveryKitScreenState extends ConsumerState<RecoveryKitScreen> {
  bool _revealed = false;
  bool _saved = false;
  bool _busy = false;

  Future<void> _regenerate() async {
    setState(() => _busy = true);
    await runWithFeedback(context, () => ref.read(onboardingControllerProvider.notifier).regenerateKit());
    if (mounted) setState(() => _busy = false);
  }

  @override
  Widget build(BuildContext context) {
    final kit = ref.watch(onboardingControllerProvider).kit;
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    // A content page (§4.14): words and QR on opaque paper, rendered only
    // after an explicit reveal — never blurred as a "hidden" state.
    return GateScaffold(
      material: GateMaterial.content,
      maxWidth: 860,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GateHeader(
            icon: Icons.health_and_safety_rounded,
            title: l10n.recoveryKitTitle,
            subtitle: l10n.recoveryKitSubtitle,
          ),
          if (kit == null) ...[
            InfoBanner(tone: BannerTone.warning, message: l10n.recoveryKitLostFromMemory),
            const SizedBox(height: GlassSpacing.s16),
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: GlassButton.prominent(
                onPressed: _busy ? null : _regenerate,
                busy: _busy,
                label: l10n.recoveryKitGenerateNew,
              ),
            ),
          ] else ...[
            _KitDetails(kit: kit),
            const SizedBox(height: GlassSpacing.s16),
            if (!_revealed)
              _HiddenKitPlaceholder(onReveal: () => setState(() => _revealed = true))
            else
              RecoveryKitPaper(kit: kit),
            const SizedBox(height: GlassSpacing.s16),
            Wrap(
              spacing: GlassSpacing.s8,
              runSpacing: GlassSpacing.s8,
              children: [
                GlassButton(
                  // TODO(client/ui): native print / "Save as PDF" of the kit — needs a
                  // printing plugin decision (ADR-0101 keeps deps minimal); next: evaluate
                  // `printing` (Apache-2.0) at M5 and render the same layout to PDF.
                  onPressed: () => showSnack(context, l10n.recoveryKitPrintUnavailable),
                  icon: Icons.print_rounded,
                  label: l10n.recoveryKitPrint,
                ),
                if (_revealed)
                  GlassButton(
                    onPressed: () => copySecretWithNotice(
                      context,
                      ref,
                      kit.exposeWords().join(' '),
                      what: l10n.copyWhatRecoveryWords,
                    ),
                    icon: Icons.copy_rounded,
                    label: l10n.recoveryKitCopyWords,
                  ),
              ],
            ),
            const SizedBox(height: GlassSpacing.s16),
            CheckboxListTile(
              key: const ValueKey('kit-saved'),
              contentPadding: EdgeInsets.zero,
              controlAffinity: ListTileControlAffinity.leading,
              value: _saved,
              onChanged: _revealed ? (v) => setState(() => _saved = v ?? false) : null,
              title: Text(l10n.recoveryKitSavedConfirmation),
              subtitle: _revealed ? null : Text(l10n.recoveryKitRevealFirst),
            ),
            const SizedBox(height: GlassSpacing.s8),
            Align(
              alignment: AlignmentDirectional.centerEnd,
              child: GlassButton.prominent(
                key: const ValueKey('kit-continue'),
                size: GlassControlSize.lg,
                onPressed: _saved ? () => context.go(AppRoutes.verifyKit) : null,
                label: l10n.commonContinue,
              ),
            ),
            const SizedBox(height: GlassSpacing.s8),
          ],
          Text(l10n.recoveryKitKeyNotStored, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
        ],
      ),
    );
  }
}

class _KitDetails extends StatelessWidget {
  const _KitDetails({required this.kit});

  final RecoveryKit kit;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    return ContentSurface(
      kind: ContentSurfaceKind.paper,
      padding: const EdgeInsets.symmetric(horizontal: GlassSpacing.s16, vertical: GlassSpacing.s12),
      child: Column(
        children: [
          LabeledValue(
            label: l10n.recoveryKitVaultId,
            value: SelectableText(
              kit.vaultId.value,
              style: tokens.typography.mono.copyWith(color: tokens.palette.label),
            ),
          ),
          LabeledValue(
            label: l10n.recoveryKitServer,
            value: Text(kit.serverUrl?.toString() ?? l10n.recoveryKitNoServerCopy),
          ),
          LabeledValue(label: l10n.recoveryKitCreated, value: Text(formatDateTime(l10n, kit.createdAt))),
        ],
      ),
    );
  }
}

/// Hidden state: a solid placeholder on paper — the words are not rendered
/// at all until revealed.
class _HiddenKitPlaceholder extends StatelessWidget {
  const _HiddenKitPlaceholder({required this.onReveal});

  final VoidCallback onReveal;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    return ContentSurface(
      kind: ContentSurfaceKind.paper,
      padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s40, horizontal: GlassSpacing.s24),
      child: Column(
        children: [
          Icon(Icons.visibility_off_rounded, size: 28, color: tokens.secondaryLabel),
          const SizedBox(height: GlassSpacing.s8),
          Text(
            l10n.recoveryKitPrivacyHint,
            textAlign: TextAlign.center,
            style: tokens.typography.body.copyWith(color: tokens.palette.label),
          ),
          const SizedBox(height: GlassSpacing.s16),
          GlassButton(
            key: const ValueKey('reveal-kit'),
            size: GlassControlSize.lg,
            icon: Icons.visibility_rounded,
            onPressed: onReveal,
            label: l10n.recoveryKitShow,
          ),
        ],
      ),
    );
  }
}

/// Revealed Recovery Kit (§4.14): a 4-column numbered word grid on paper and
/// the QR on pure white with a 16 px quiet zone (white even in dark mode).
/// Only build it after an explicit reveal.
class RecoveryKitPaper extends StatelessWidget {
  const RecoveryKitPaper({required this.kit, super.key, this.qrSize = 176});

  final RecoveryKit kit;
  final double qrSize;

  @override
  Widget build(BuildContext context) {
    final words = kit.exposeWords();
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final wordStyle = t.mono.copyWith(
      fontSize: 15,
      height: 22 / 15,
      fontWeight: FontWeight.w500,
      color: tokens.palette.label,
    );
    final rows = (words.length / 4).ceil();
    final grid = Column(
      children: [
        for (var r = 0; r < rows; r++)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s4),
            child: Row(
              children: [
                for (var c = 0; c < 4; c++)
                  Expanded(
                    child: Builder(
                      builder: (context) {
                        final i = r * 4 + c;
                        if (i >= words.length) return const SizedBox.shrink();
                        return Row(
                          children: [
                            SizedBox(
                              width: 22,
                              child: Text(
                                '${i + 1}',
                                textAlign: TextAlign.end,
                                style: t.caption.copyWith(color: tokens.palette.tertiary),
                              ),
                            ),
                            const SizedBox(width: GlassSpacing.s8),
                            Expanded(
                              child: Text(words[i], key: ValueKey('kit-word-$i'), style: wordStyle),
                            ),
                          ],
                        );
                      },
                    ),
                  ),
              ],
            ),
          ),
      ],
    );
    return ContentSurface(
      kind: ContentSurfaceKind.paper,
      padding: const EdgeInsets.all(GlassSpacing.s24),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(child: grid),
          const SizedBox(width: GlassSpacing.s24),
          DecoratedBox(
            decoration: ShapeDecoration(
              color: const Color(0xFFFFFFFF),
              shape: GlassRadii.shape(tokens.radii.md).copyWith(side: BorderSide(color: tokens.surfaces.hairlineCard)),
            ),
            child: Padding(
              padding: const EdgeInsets.all(GlassSpacing.s16),
              child: QrImageView(
                data: kit.exposeQrPayload(),
                size: qrSize,
                padding: EdgeInsets.zero,
                backgroundColor: const Color(0xFFFFFFFF),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
