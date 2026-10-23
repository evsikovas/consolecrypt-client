import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/sharing/sharing_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// "Sharing: verified recipients only" (audit H3, TR-14) — a vault setting,
/// shown while sharing is available. The core enforces it.
class SharingPolicySettings extends ConsumerStatefulWidget {
  const SharingPolicySettings({super.key});
  @override
  ConsumerState<SharingPolicySettings> createState() => _SharingPolicySettingsState();
}

class _SharingPolicySettingsState extends ConsumerState<SharingPolicySettings> {
  bool _busy = false;

  Future<void> _set(SharingPolicy policy) async {
    setState(() => _busy = true);
    await runWithFeedback(context, () => ref.read(sharingServiceProvider).setPolicy(policy));
    if (!mounted) return;
    ref.invalidate(sharingPolicyProvider);
    setState(() => _busy = false);
  }

  @override
  Widget build(BuildContext context) {
    final status = ref.watch(sharingStatusProvider).value;
    final policy = ref.watch(sharingPolicyProvider).value;
    if (status == null || !status.enabled || status.locked || policy == null) return const SizedBox.shrink();
    final l = context.l10n;
    final tokens = GlassTokens.of(context);
    return Column(
      key: const ValueKey('sharing-policy-settings'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const SizedBox(height: GlassSpacing.s8),
        Text(l.sharingPolicyTitle, style: tokens.typography.bodyEmph.copyWith(color: tokens.palette.label)),
        Text(l.sharingPolicyHelp, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
        SwitchListTile.adaptive(
          key: const ValueKey('sharing-policy-sensitive'),
          contentPadding: EdgeInsets.zero,
          secondary: const Icon(Icons.verified_user_rounded),
          title: Text(l.sharingPolicySensitive),
          value: policy.sensitiveVerifiedOnly,
          onChanged: _busy
              ? null
              : (v) => _set(SharingPolicy(sensitiveVerifiedOnly: v, otherVerifiedOnly: policy.otherVerifiedOnly)),
        ),
        SwitchListTile.adaptive(
          key: const ValueKey('sharing-policy-other'),
          contentPadding: EdgeInsets.zero,
          secondary: const Icon(Icons.share_outlined),
          title: Text(l.sharingPolicyOther),
          value: policy.otherVerifiedOnly,
          onChanged: _busy
              ? null
              : (v) => _set(SharingPolicy(sensitiveVerifiedOnly: policy.sensitiveVerifiedOnly, otherVerifiedOnly: v)),
        ),
      ],
    );
  }
}
