import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_exposure.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:consolecrypt/vault/onboarding_controller.dart';
import 'package:consolecrypt/vault/recovery_kit_print.dart';
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
  bool _printing = false;

  Future<void> _regenerate() async {
    setState(() => _busy = true);
    await runWithFeedback(context, () => ref.read(onboardingControllerProvider.notifier).regenerateKit());
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _print(RecoveryKit kit) async {
    final l10n = context.l10n;
    setState(() => _printing = true);
    try {
      await printRecoveryKit(ref.read(recoveryKitPrinterProvider), kit, l10n);
    } on Exception {
      if (mounted) showSnack(context, l10n.recoveryKitPrintFailed, error: true);
    } finally {
      if (mounted) setState(() => _printing = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final kit = ref.watch(onboardingControllerProvider).kit;
    final canPrint = ref.watch(recoveryKitCanPrintProvider).value ?? false;
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
            if (_revealed) ...[
              Wrap(
                spacing: GlassSpacing.s8,
                runSpacing: GlassSpacing.s8,
                children: [
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
                  // Printing goes straight to the system print dialog (no
                  // temp file); offered only where that dialog is available.
                  if (canPrint)
                    GlassButton(
                      key: const ValueKey('kit-print'),
                      onPressed: _printing ? null : () => _print(kit),
                      busy: _printing,
                      icon: Icons.print_rounded,
                      label: l10n.recoveryKitPrint,
                    ),
                ],
              ),
              if (canPrint) ...[
                const SizedBox(height: GlassSpacing.s8),
                Text(
                  l10n.recoveryKitPrintHint,
                  style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
                ),
              ],
              const SizedBox(height: GlassSpacing.s16),
            ],
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

/// Revealed words use as many columns as fit at the current text scale.
/// QR moves below the words on narrow screens and keeps its white quiet zone.
/// Only build this widget after an explicit reveal.
class RecoveryKitPaper extends StatelessWidget {
  const RecoveryKitPaper({required this.kit, super.key, this.qrSize = 176});

  final RecoveryKit kit;
  final double qrSize;

  // The words are on screen whenever the paper is: it counts as a revealed
  // secret (screen-capture protection).
  @override
  Widget build(BuildContext context) => RevealedSecret(child: _paper(context));

  Widget _paper(BuildContext context) {
    final words = kit.exposeWords();
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final wordStyle = t.mono.copyWith(
      fontSize: 15,
      height: 22 / 15,
      fontWeight: FontWeight.w500,
      color: tokens.palette.label,
    );
    final numberStyle = t.caption.copyWith(color: tokens.palette.tertiary);
    final textScaler = MediaQuery.textScalerOf(context);
    double textWidth(String value, TextStyle style) {
      final painter = TextPainter(
        text: TextSpan(text: value, style: style),
        textDirection: TextDirection.ltr,
        textScaler: textScaler,
      )..layout();
      final width = painter.width.ceilToDouble();
      painter.dispose();
      return width;
    }

    final numberWidth = textWidth('${words.length}', numberStyle);
    final wordWidth = words.fold<double>(0, (width, word) {
      final measured = textWidth(word, wordStyle);
      return measured > width ? measured : width;
    });
    const gap = GlassSpacing.s16;
    final cellWidth = numberWidth + GlassSpacing.s8 + wordWidth + 2;

    Widget wordGrid(double width) {
      final columns = ((width + gap) / (cellWidth + gap)).floor().clamp(1, 4);
      final rows = (words.length / columns).ceil();
      return Directionality(
        textDirection: TextDirection.ltr,
        child: Column(
          key: const ValueKey('kit-word-grid'),
          children: [
            for (var row = 0; row < rows; row++)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s4),
                child: Row(
                  children: [
                    for (var column = 0; column < columns; column++) ...[
                      if (column > 0) const SizedBox(width: gap),
                      Expanded(
                        child: Builder(
                          builder: (context) {
                            final index = row * columns + column;
                            if (index >= words.length) return const SizedBox.shrink();
                            return Row(
                              children: [
                                SizedBox(
                                  width: numberWidth,
                                  child: Text('${index + 1}', textAlign: TextAlign.end, style: numberStyle),
                                ),
                                const SizedBox(width: GlassSpacing.s8),
                                Expanded(
                                  child: Text(
                                    words[index],
                                    key: ValueKey('kit-word-$index'),
                                    style: wordStyle,
                                    softWrap: false,
                                    maxLines: 1,
                                  ),
                                ),
                              ],
                            );
                          },
                        ),
                      ),
                    ],
                  ],
                ),
              ),
          ],
        ),
      );
    }

    Widget qr(double availableWidth) => DecoratedBox(
      key: const ValueKey('kit-qr'),
      decoration: ShapeDecoration(
        color: const Color(0xFFFFFFFF),
        shape: GlassRadii.shape(tokens.radii.md).copyWith(side: BorderSide(color: tokens.surfaces.hairlineCard)),
      ),
      child: Padding(
        padding: const EdgeInsets.all(GlassSpacing.s16),
        child: QrImageView(
          data: kit.exposeQrPayload(),
          size: qrSize.clamp(1, availableWidth - GlassSpacing.s32).toDouble(),
          padding: EdgeInsets.zero,
          backgroundColor: const Color(0xFFFFFFFF),
        ),
      ),
    );

    return LayoutBuilder(
      builder: (context, outer) {
        final padding = outer.maxWidth < 480 ? GlassSpacing.s16 : GlassSpacing.s24;
        return ContentSurface(
          kind: ContentSurfaceKind.paper,
          padding: EdgeInsets.all(padding),
          child: LayoutBuilder(
            builder: (context, constraints) {
              final width = constraints.maxWidth;
              final qrWidth = qrSize + GlassSpacing.s32;
              final wide = width >= cellWidth * 4 + gap * 3 + GlassSpacing.s24 + qrWidth;
              if (wide) {
                final gridWidth = width - qrWidth - GlassSpacing.s24;
                return Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Expanded(child: wordGrid(gridWidth)),
                    const SizedBox(width: GlassSpacing.s24),
                    qr(qrWidth),
                  ],
                );
              }
              return Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  // Extreme accessibility scaling can exceed even one column.
                  // Preserve the full words with horizontal scrolling in that case.
                  if (width < cellWidth)
                    SingleChildScrollView(
                      scrollDirection: Axis.horizontal,
                      child: SizedBox(width: cellWidth, child: wordGrid(cellWidth)),
                    )
                  else
                    wordGrid(width),
                  const SizedBox(height: GlassSpacing.s24),
                  Center(child: qr(width)),
                ],
              );
            },
          ),
        );
      },
    );
  }
}
