import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/language_picker.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

/// First launch / "Add profile": Local is a first-class choice (ADR-0106).
class WelcomeScreen extends ConsumerStatefulWidget {
  const WelcomeScreen({super.key});

  @override
  ConsumerState<WelcomeScreen> createState() => _WelcomeScreenState();
}

class _WelcomeScreenState extends ConsumerState<WelcomeScreen> {
  final _name = TextEditingController();

  /// The localized default name last put into [_name]; replaced when the
  /// language changes unless the user edited it.
  String? _defaultName;
  bool _busy = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final localized = context.l10n.welcomeDefaultProfileName;
    if (_defaultName == null || _name.text == _defaultName) _name.text = localized;
    _defaultName = localized;
  }

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  Future<void> _useLocally() async {
    setState(() => _busy = true);
    final created = await runWithFeedback(
      context,
      () => ref.read(profileServiceProvider).createLocalProfile(name: _name.text),
    );
    if (!mounted) return;
    setState(() => _busy = false);
    if (created != null) context.go(AppRoutes.onboarding);
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(profilesProvider).value ?? ProfilesState.empty;
    final hasProfiles = state.profiles.isNotEmpty;
    final resume = hasProfiles && state.active == null;
    final local = ref.watch(localSettingsProvider).value ?? const LocalSettings();
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    return GateScaffold(
      material: GateMaterial.glass,
      maxWidth: 760,
      leading: state.active != null
          ? GlassButton(
              onPressed: () => context.go(AppRoutes.hosts),
              icon: Icons.arrow_back_rounded,
              label: l10n.commonBack,
            )
          : null,
      trailing: const LanguageMenuButton(),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          GateHeader(
            title: resume ? l10n.welcomeResumeTitle : (hasProfiles ? l10n.welcomeTitleAddProfile : l10n.welcomeTitle),
            subtitle: resume ? l10n.welcomeResumeHelp : l10n.welcomeSubtitle,
          ),
          if (resume) ...[
            for (final profile in state.profiles)
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: GlassButton.prominent(
                  key: ValueKey('resume-profile-${profile.id.value}'),
                  label: profile.name,
                  icon: profile.isLocal ? Icons.laptop_mac_rounded : Icons.cloud_rounded,
                  busy: _busy,
                  expand: true,
                  onPressed: _busy
                      ? null
                      : () async {
                          setState(() => _busy = true);
                          final opened = await runWithFeedback(context, () async {
                            await ref.read(profileServiceProvider).switchTo(profile.id);
                            return true;
                          });
                          if (!mounted) return;
                          setState(() => _busy = false);
                          if (opened == true && context.mounted) context.go(AppRoutes.hosts);
                        },
                ),
              ),
            Text(l10n.welcomeKeychainHelp, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
            CheckboxListTile(
              key: const ValueKey('reopen-last-profile'),
              contentPadding: EdgeInsets.zero,
              value: local.reopenLastProfile,
              controlAffinity: ListTileControlAffinity.leading,
              title: Text(l10n.reopenLastProfile),
              subtitle: Text(l10n.reopenLastProfileHelp),
              onChanged: _busy
                  ? null
                  : (value) => runWithFeedback(context, () {
                      final settings = ref.read(settingsServiceProvider);
                      return settings.updateLocal(settings.currentLocal.copyWith(reopenLastProfile: value ?? false));
                    }),
            ),
            const SizedBox(height: 24),
          ],
          LayoutBuilder(
            builder: (context, constraints) {
              final local = _OptionCard(
                key: const ValueKey('option-local'),
                icon: Icons.laptop_mac_rounded,
                title: l10n.welcomeLocalTitle,
                body: l10n.welcomeLocalBody,
                extra: TextField(
                  controller: _name,
                  decoration: InputDecoration(labelText: l10n.welcomeProfileNameLabel),
                ),
                action: GlassButton.prominent(
                  key: const ValueKey('welcome-local'),
                  size: GlassControlSize.lg,
                  expand: true,
                  busy: _busy,
                  onPressed: _busy ? null : _useLocally,
                  label: l10n.welcomeLocalAction,
                ),
              );
              final server = _OptionCard(
                key: const ValueKey('option-server'),
                icon: Icons.cloud_sync_rounded,
                title: l10n.welcomeServerTitle,
                body: l10n.welcomeServerBody,
                action: GlassButton(
                  key: const ValueKey('welcome-server'),
                  size: GlassControlSize.lg,
                  expand: true,
                  onPressed: _busy ? null : () => context.go('${AppRoutes.login}?new=1'),
                  label: l10n.welcomeServerAction,
                ),
              );
              if (constraints.maxWidth < 600) {
                return Column(
                  children: [
                    IntrinsicHeight(child: local),
                    const SizedBox(height: GlassSpacing.s12),
                    IntrinsicHeight(child: server),
                  ],
                );
              }
              return IntrinsicHeight(
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Expanded(child: local),
                    const SizedBox(width: GlassSpacing.s12),
                    Expanded(child: server),
                  ],
                ),
              );
            },
          ),
          const SizedBox(height: GlassSpacing.s16),
          Align(
            alignment: AlignmentDirectional.centerStart,
            child: GlassButton.plain(
              onPressed: () => context.go(AppRoutes.restore),
              icon: Icons.settings_backup_restore_rounded,
              label: l10n.welcomeRestoreFromBackup,
            ),
          ),
          const SizedBox(height: GlassSpacing.s4),
          Text(l10n.welcomeProfilesHint, style: tokens.typography.callout.copyWith(color: tokens.secondaryLabel)),
          const DemoHints(),
        ],
      ),
    );
  }
}

/// A choice on the welcome hero. It sits on glass, so it is a fill
/// (vibrancy-on-glass), not a card or another pane of glass (§3 rule 4).
class _OptionCard extends StatelessWidget {
  const _OptionCard({
    required this.icon,
    required this.title,
    required this.body,
    required this.action,
    super.key,
    this.extra,
  });

  final IconData icon;
  final String title;
  final String body;
  final Widget action;
  final Widget? extra;

  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    return DecoratedBox(
      decoration: ShapeDecoration(color: tokens.surfaces.fillField, shape: GlassRadii.shape(tokens.radii.card)),
      child: Padding(
        padding: const EdgeInsets.all(GlassSpacing.card),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: Container(
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: tokens.palette.accent.withValues(alpha: .09),
                  borderRadius: BorderRadius.circular(14),
                ),
                child: Icon(icon, size: 26, color: tokens.palette.accent),
              ),
            ),
            const SizedBox(height: GlassSpacing.s12),
            Text(title, style: t.title2.copyWith(color: tokens.palette.label)),
            const SizedBox(height: GlassSpacing.s6),
            Text(body, style: t.body.copyWith(color: tokens.secondaryLabel)),
            const Spacer(),
            if (extra != null) ...[const SizedBox(height: GlassSpacing.s12), extra!],
            const SizedBox(height: GlassSpacing.s16),
            action,
          ],
        ),
      ),
    );
  }
}
