import 'package:consolecrypt/account/profile_switcher.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// Vault unlock: passphrase, OS authentication via the device envelope,
/// device approval for untrusted devices, and the path to recovery.
class UnlockScreen extends ConsumerStatefulWidget {
  const UnlockScreen({super.key});

  @override
  ConsumerState<UnlockScreen> createState() => _UnlockScreenState();
}

class _UnlockScreenState extends ConsumerState<UnlockScreen> with WidgetsBindingObserver {
  final _passphrase = TextEditingController();
  AppException? _error;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) ref.read(vaultServiceProvider).refreshDeviceUnlockAvailability().ignore();
    });
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed && !_busy) {
      ref.read(vaultServiceProvider).refreshDeviceUnlockAvailability().ignore();
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _passphrase.dispose();
    super.dispose();
  }

  Future<void> _guard(Future<void> Function() action) async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await action();
      if (mounted) context.go(AppRoutes.hosts);
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _unlock() async {
    if (_passphrase.text.isEmpty) return;
    final secret = SecretText(_passphrase.text);
    await _guard(() async {
      try {
        await ref.read(vaultServiceProvider).unlockWithPassphrase(secret);
        _passphrase.clear();
      } finally {
        secret.wipe();
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final status = ref.watch(vaultStatusProvider).value;
    final profile = ref.watch(activeProfileProvider);
    final l10n = context.l10n;
    final vaultName = status?.vaultName;
    final synced = profile?.isSynced ?? false;
    final tokens = GlassTokens.of(context);
    // Password prompt: the secure material (opaque, nothing animated, §0.3).
    return GateScaffold(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GateHeader(
            icon: Icons.lock_rounded,
            title: vaultName == null ? l10n.unlockTitleGeneric : l10n.unlockTitle(vaultName),
            subtitle: profile == null ? null : '${profile.name} · ${profile.localizedSubtitle(l10n)}',
          ),
          SecretField(
            key: const ValueKey('unlock-passphrase'),
            controller: _passphrase,
            label: l10n.unlockPassphraseLabel,
            autofocus: true,
            enabled: !_busy,
            autofillHints: const [AutofillHints.password],
            onSubmitted: (_) => _unlock(),
          ),
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s8),
            GateErrorText(key: const ValueKey('unlock-error'), text: errorMessage(l10n, _error!)),
          ],
          const SizedBox(height: GlassSpacing.s16),
          GlassButton.prominent(
            key: const ValueKey('unlock-submit'),
            size: GlassControlSize.lg,
            expand: true,
            busy: _busy,
            onPressed: _busy ? null : _unlock,
            label: l10n.unlockSubmit,
          ),
          const SizedBox(height: GlassSpacing.s8),
          GlassButton(
            key: const ValueKey('unlock-device'),
            size: GlassControlSize.lg,
            expand: true,
            onPressed: (status?.deviceUnlockAvailable ?? false) && !_busy
                ? () => _guard(() => ref.read(vaultServiceProvider).unlockWithDevice(reason: l10n.unlockOsAuthReason))
                : null,
            icon: Icons.fingerprint_rounded,
            label: l10n.unlockWithOsAuth(status?.deviceUnlock.authName(l10n) ?? l10n.platformSystemAuthentication),
          ),
          if (synced && !(status?.deviceTrusted ?? true)) ...[
            const SizedBox(height: GlassSpacing.s16),
            InfoBanner(
              title: l10n.unlockUntrustedTitle,
              message: l10n.unlockUntrustedMessage,
              action: TextButton(
                key: const ValueKey('request-approval'),
                onPressed: _busy
                    ? null
                    : () => runWithFeedback(context, () => ref.read(vaultServiceProvider).requestDeviceApproval()).then(
                        (_) {
                          if (context.mounted) context.go(AppRoutes.approval);
                        },
                      ),
                child: Text(l10n.unlockApproveFromOtherDevice),
              ),
            ),
          ],
          const SizedBox(height: GlassSpacing.s8),
          OverflowBar(
            alignment: MainAxisAlignment.spaceBetween,
            overflowAlignment: OverflowBarAlignment.end,
            spacing: GlassSpacing.s8,
            overflowSpacing: GlassSpacing.s8,
            children: [
              GlassButton.plain(
                key: const ValueKey('forgot-passphrase'),
                onPressed: () => context.go(AppRoutes.recovery),
                label: l10n.unlockForgotPassphrase,
              ),
              if (synced)
                GlassButton.plain(
                  onPressed: () async {
                    await ref.read(authServiceProvider).logout();
                    if (context.mounted) context.go(AppRoutes.hosts);
                  },
                  label: l10n.unlockSignOut,
                ),
              if (profile != null)
                GlassButton(
                  key: const ValueKey('unlock-delete-profile'),
                  style: GlassButtonStyle.destructiveQuiet,
                  size: GlassControlSize.sm,
                  icon: Icons.delete_rounded,
                  onPressed: _busy ? null : () => confirmAndDeleteProfile(context, ref, profile),
                  label: l10n.unlockDeleteProfile,
                ),
            ],
          ),
          Padding(
            padding: const EdgeInsets.symmetric(vertical: GlassSpacing.s12),
            child: Divider(height: 1, color: tokens.surfaces.separator),
          ),
          const ProfileSwitcherRow(),
          const DemoHints(),
        ],
      ),
    );
  }
}
