import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:consolecrypt/vault/onboarding_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// Onboarding step 3: mandatory re-entry of 3 random words.
class VerifyKitScreen extends ConsumerStatefulWidget {
  const VerifyKitScreen({super.key});

  @override
  ConsumerState<VerifyKitScreen> createState() => _VerifyKitScreenState();
}

class _VerifyKitScreenState extends ConsumerState<VerifyKitScreen> {
  final Map<int, TextEditingController> _controllers = {};
  bool _mismatch = false;
  bool _busy = false;

  TextEditingController _controllerFor(int position) => _controllers.putIfAbsent(position, TextEditingController.new);

  @override
  void dispose() {
    for (final c in _controllers.values) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _verify() async {
    final controller = ref.read(onboardingControllerProvider.notifier);
    final answers = {for (final e in _controllers.entries) e.key: e.value.text};
    if (!controller.verify(answers)) {
      setState(() => _mismatch = true);
      return;
    }
    final local = ref.read(activeProfileProvider)?.isLocal ?? false;
    if (local) {
      // ADR-0106: local profiles see the "no cloud copy" notice first.
      context.go(AppRoutes.localNotice);
      return;
    }
    setState(() => _busy = true);
    await runWithFeedback(context, controller.complete);
    if (mounted) context.go(AppRoutes.hosts);
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(onboardingControllerProvider);
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    if (state.kit == null) {
      return GateScaffold(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            GateHeader(title: l10n.verifyKitTitle),
            InfoBanner(tone: BannerTone.warning, message: l10n.verifyKitNoKit),
            const SizedBox(height: GlassSpacing.s16),
            GlassButton.prominent(
              size: GlassControlSize.lg,
              onPressed: () => context.go(AppRoutes.recoveryKit),
              label: l10n.verifyKitBackToRecoveryKit,
            ),
          ],
        ),
      );
    }
    // Recovery words are secrets: the secure material, mono inputs, no
    // autocorrect; failures never reveal the expected words (§4.14).
    return GateScaffold(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GateHeader(icon: Icons.fact_check_rounded, title: l10n.verifyKitTitle, subtitle: l10n.verifyKitSubtitle),
          for (final (i, position) in state.positions.indexed) ...[
            TextField(
              key: ValueKey('verify-word-$position'),
              controller: _controllerFor(position),
              autofocus: i == 0,
              autocorrect: false,
              enableSuggestions: false,
              enableIMEPersonalizedLearning: false,
              style: tokens.typography.mono.copyWith(color: tokens.palette.label),
              decoration: InputDecoration(labelText: l10n.verifyKitWordLabel(position + 1)),
              onChanged: (_) => setState(() => _mismatch = false),
              onSubmitted: (_) => i == state.positions.length - 1 ? _verify() : null,
            ),
            const SizedBox(height: GlassSpacing.s12),
          ],
          if (_mismatch) GateErrorText(key: const ValueKey('verify-error'), text: l10n.verifyKitMismatch),
          const SizedBox(height: GlassSpacing.s16),
          OverflowBar(
            alignment: MainAxisAlignment.spaceBetween,
            overflowAlignment: OverflowBarAlignment.end,
            spacing: GlassSpacing.s8,
            overflowSpacing: GlassSpacing.s8,
            children: [
              GlassButton(onPressed: () => context.go(AppRoutes.recoveryKit), label: l10n.verifyKitBackToKit),
              GlassButton.prominent(
                key: const ValueKey('verify-submit'),
                size: GlassControlSize.lg,
                busy: _busy,
                onPressed: _busy ? null : _verify,
                label: l10n.verifyKitSubmit,
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// ADR-0106: shown once at the end of local onboarding.
class LocalNoticeScreen extends ConsumerStatefulWidget {
  const LocalNoticeScreen({super.key});

  @override
  ConsumerState<LocalNoticeScreen> createState() => _LocalNoticeScreenState();
}

class _LocalNoticeScreenState extends ConsumerState<LocalNoticeScreen> {
  bool _busy = false;

  Future<void> _finish(String route) async {
    setState(() => _busy = true);
    await runWithFeedback(context, ref.read(onboardingControllerProvider.notifier).complete);
    if (mounted) context.go(route);
  }

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final l10n = context.l10n;
    Widget point(IconData icon, String title, String body) => Padding(
      padding: const EdgeInsets.only(bottom: GlassSpacing.s16),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, color: tokens.palette.accent, size: 22),
          const SizedBox(width: GlassSpacing.s12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title, style: t.bodyEmph.copyWith(color: tokens.palette.label)),
                const SizedBox(height: GlassSpacing.s2),
                Text(body, style: t.body.copyWith(color: tokens.secondaryLabel)),
              ],
            ),
          ),
        ],
      ),
    );
    return GateScaffold(
      material: GateMaterial.glass,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GateHeader(
            icon: Icons.cloud_off_rounded,
            title: l10n.onboardingLocalNoticeTitle,
            subtitle: l10n.onboardingLocalNoticeSubtitle,
          ),
          point(Icons.health_and_safety_rounded, l10n.onboardingLocalNoticeKitTitle, l10n.onboardingLocalNoticeKitBody),
          point(Icons.save_alt_rounded, l10n.onboardingLocalNoticeBackupsTitle, l10n.onboardingLocalNoticeBackupsBody),
          point(Icons.warning_rounded, l10n.onboardingLocalNoticeLossTitle, l10n.onboardingLocalNoticeLossBody),
          const SizedBox(height: GlassSpacing.s8),
          OverflowBar(
            alignment: MainAxisAlignment.spaceBetween,
            overflowAlignment: OverflowBarAlignment.end,
            spacing: GlassSpacing.s8,
            overflowSpacing: GlassSpacing.s8,
            children: [
              GlassButton(
                onPressed: _busy ? null : () => _finish(AppRoutes.backups),
                label: l10n.onboardingLocalNoticeSetUpBackups,
              ),
              GlassButton.prominent(
                key: const ValueKey('local-notice-continue'),
                size: GlassControlSize.lg,
                busy: _busy,
                onPressed: _busy ? null : () => _finish(AppRoutes.hosts),
                label: l10n.onboardingLocalNoticeContinue,
              ),
            ],
          ),
        ],
      ),
    );
  }
}
