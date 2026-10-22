import 'package:consolecrypt/account/profile_switcher.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// `account_disabled` (enterprise sign-in, ТР-12): the organisation
/// disabled the account; every session was revoked. The synced profile
/// stays locked and its local data encrypted. The user can try signing in
/// again (access restored), switch profiles or delete this profile from the
/// device.
class AccountDisabledScreen extends ConsumerStatefulWidget {
  const AccountDisabledScreen({super.key});

  @override
  ConsumerState<AccountDisabledScreen> createState() => _AccountDisabledScreenState();
}

class _AccountDisabledScreenState extends ConsumerState<AccountDisabledScreen> {
  bool _busy = false;

  Future<void> _signInAgain() async {
    setState(() => _busy = true);
    await runWithFeedback(context, () => ref.read(authServiceProvider).clearAccountDisabled());
    if (!mounted) return;
    setState(() => _busy = false);
    context.go(AppRoutes.login);
  }

  Future<void> _delete() async {
    final profile = ref.read(activeProfileProvider);
    if (profile == null) return;
    final l10n = context.l10n;
    final ok = await showConfirmDialog(
      context,
      title: l10n.accountDisabledDeleteTitle(profile.name),
      message: l10n.accountDisabledDeleteMessage,
      confirmLabel: l10n.commonDelete,
      destructive: true,
    );
    if (!ok || !mounted) return;
    setState(() => _busy = true);
    await runWithFeedback(context, () => ref.read(profileServiceProvider).delete(profile.id));
    if (mounted) setState(() => _busy = false);
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final profile = ref.watch(activeProfileProvider);
    final server = profile?.serverUrl?.host ?? profile?.serverUrl?.toString() ?? '—';
    return GateScaffold(
      key: const ValueKey('account-disabled-screen'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GateHeader(
            icon: Icons.person_off_rounded,
            title: l10n.accountDisabledTitle,
            subtitle: l10n.accountDisabledBody(server),
          ),
          if (profile != null)
            Text(
              profile.accountEmail ?? profile.name,
              textAlign: TextAlign.center,
              style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel),
            ),
          const SizedBox(height: GlassSpacing.s20),
          GlassButton.prominent(
            key: const ValueKey('account-disabled-sign-in'),
            size: GlassControlSize.lg,
            expand: true,
            busy: _busy,
            onPressed: _busy ? null : _signInAgain,
            label: l10n.accountDisabledSignIn,
          ),
          const SizedBox(height: GlassSpacing.s8),
          GlassButton(
            key: const ValueKey('account-disabled-delete'),
            style: GlassButtonStyle.destructiveQuiet,
            expand: true,
            onPressed: _busy ? null : _delete,
            label: l10n.accountDisabledDelete,
          ),
          const SizedBox(height: GlassSpacing.s12),
          const ProfileSwitcherRow(),
        ],
      ),
    );
  }
}
