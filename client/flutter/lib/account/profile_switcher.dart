import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:material_ui/material_ui.dart';

IconData profileIcon(Profile p) => p.isLocal ? Icons.laptop_mac_rounded : Icons.cloud_rounded;

Future<void> switchProfile(BuildContext context, WidgetRef ref, Profile target) async {
  await runWithFeedback(context, () => ref.read(profileServiceProvider).switchTo(target.id));
  if (context.mounted) context.go(AppRoutes.hosts); // the gate routes to unlock / login
}

/// Deletes [profile] from this device after a strong confirmation: a local
/// profile needs an explicit acknowledgement that its vault is gone for good
/// unless a backup exists (ADR-0106). Then routes onwards (welcome, or the
/// next profile's unlock).
Future<void> confirmAndDeleteProfile(BuildContext context, WidgetRef ref, Profile profile) async {
  final ok = await showAppDialog<bool>(context, builder: (_) => _DeleteProfileDialog(profile: profile));
  if (ok != true || !context.mounted) return;
  await runWithFeedback(context, () => ref.read(profileServiceProvider).delete(profile.id));
  if (context.mounted) context.go(AppRoutes.hosts);
}

class _DeleteProfileDialog extends StatefulWidget {
  const _DeleteProfileDialog({required this.profile});

  final Profile profile;

  @override
  State<_DeleteProfileDialog> createState() => _DeleteProfileDialogState();
}

class _DeleteProfileDialogState extends State<_DeleteProfileDialog> {
  bool _acknowledged = false;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final local = widget.profile.isLocal;
    final canDelete = !local || _acknowledged;
    return GlassDialog(
      key: const ValueKey('delete-profile-dialog'),
      icon: Icons.warning_rounded,
      iconTone: GlassTone.danger,
      title: l10n.settingsRemoveProfileTitle(widget.profile.name),
      width: 520,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(local ? l10n.deleteProfileLocalWarning : l10n.settingsRemoveProfileSyncedMessage),
          if (local) ...[
            const SizedBox(height: GlassSpacing.s12),
            CheckboxListTile(
              key: const ValueKey('delete-profile-ack'),
              contentPadding: EdgeInsets.zero,
              controlAffinity: ListTileControlAffinity.leading,
              value: _acknowledged,
              onChanged: (v) => setState(() => _acknowledged = v ?? false),
              title: Text(l10n.deleteProfileAcknowledge),
            ),
          ],
        ],
      ),
      secondaryActions: [
        GlassButton(label: l10n.commonCancel, autofocus: true, onPressed: () => closeDialog(context, false)),
      ],
      primaryAction: GlassButton.destructive(
        key: const ValueKey('delete-profile-confirm'),
        label: l10n.settingsRemoveProfileConfirm,
        onPressed: canDelete ? () => closeDialog(context, true) : null,
      ),
    );
  }
}

sealed class _ProfileAction {
  const _ProfileAction();
}

final class _SwitchTo extends _ProfileAction {
  const _SwitchTo(this.profile);

  final Profile profile;
}

final class _AddProfile extends _ProfileAction {
  const _AddProfile();
}

final class _ManageProfiles extends _ProfileAction {
  const _ManageProfiles();
}

/// Sidebar header (§4.1): the active profile as a capsule (name + synced /
/// local icon) that opens a glass menu to switch or add profiles. It sits
/// on the glass sidebar, so it is a fill, not another pane of glass.
/// Switching locks the current vault first (VRK is zeroised in core).
class ProfileSwitcher extends ConsumerWidget {
  const ProfileSwitcher({super.key, this.compact = false});

  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(profilesProvider).value ?? ProfilesState.empty;
    final active = state.active;
    final tokens = GlassTokens.of(context);
    final l10n = context.l10n;
    if (active == null) return const SizedBox.shrink();
    final entries = <GlassMenuEntry<_ProfileAction>>[
      for (final p in state.profiles)
        GlassMenuItem<_ProfileAction>(
          key: ValueKey('profile-${p.id.value}'),
          value: _SwitchTo(p),
          label: p.name,
          subtitle: p.localizedSubtitle(l10n),
          icon: profileIcon(p),
          checked: p.id == active.id,
        ),
      const GlassMenuDivider(),
      GlassMenuItem(value: const _AddProfile(), label: l10n.profileSwitcherAddProfileMenu, icon: Icons.add_rounded),
      GlassMenuItem(
        value: const _ManageProfiles(),
        label: l10n.profileSwitcherManageProfiles,
        icon: Icons.manage_accounts_rounded,
      ),
    ];
    return GlassMenuButton<_ProfileAction>(
      entries: entries,
      onSelected: (action) {
        switch (action) {
          case _SwitchTo(:final profile) when profile.id != active.id:
            unawaited(switchProfile(context, ref, profile));
          case _SwitchTo():
            break;
          case _AddProfile():
            context.go(AppRoutes.welcome);
          case _ManageProfiles():
            context.go(AppRoutes.settings);
        }
      },
      builder: (context, open) {
        final avatar = SizedBox.square(
          dimension: 24,
          child: DecoratedBox(
            decoration: ShapeDecoration(color: tokens.sidebarSelection, shape: const CircleBorder()),
            child: Icon(profileIcon(active), size: 14, color: tokens.palette.accent),
          ),
        );
        return Tooltip(
          message: l10n.profileSwitcherTooltip,
          child: GlassInteractive(
            key: const ValueKey('profile-switcher'),
            semanticLabel: '${active.name}, ${active.localizedSubtitle(l10n)}',
            onPressed: open,
            builder: (context, s) {
              const shape = StadiumBorder();
              final bg = s.pressed
                  ? tokens.surfaces.fillPressed
                  : s.hovered
                  ? Color.alphaBlend(tokens.surfaces.fillHover, tokens.surfaces.fillField)
                  : tokens.surfaces.fillField;
              return GlassFocusRing(
                visible: s.focusVisible,
                shape: shape,
                child: DecoratedBox(
                  decoration: ShapeDecoration(color: bg, shape: shape),
                  child: SizedBox(
                    height: compact
                        ? 32
                        : math.max(
                            36.0,
                            (tokens.typography.bodyEmph.fontSize! + tokens.typography.caption.fontSize!) * 1.2 + 4,
                          ),
                    width: compact ? 32 : null,
                    child: compact
                        ? Center(child: avatar)
                        : Padding(
                            padding: const EdgeInsetsDirectional.only(start: 6, end: 8),
                            child: Row(
                              children: [
                                avatar,
                                const SizedBox(width: GlassSpacing.s8),
                                Expanded(
                                  child: Column(
                                    mainAxisAlignment: MainAxisAlignment.center,
                                    crossAxisAlignment: CrossAxisAlignment.start,
                                    children: [
                                      Text(
                                        active.name,
                                        maxLines: 1,
                                        overflow: TextOverflow.ellipsis,
                                        style: tokens.typography.bodyEmph.copyWith(
                                          color: tokens.palette.label,
                                          height: 1.2,
                                        ),
                                      ),
                                      Text(
                                        active.localizedSubtitle(l10n),
                                        maxLines: 1,
                                        overflow: TextOverflow.ellipsis,
                                        style: tokens.typography.caption.copyWith(
                                          color: tokens.secondaryLabel,
                                          fontWeight: FontWeight.w400,
                                          height: 1.2,
                                        ),
                                      ),
                                    ],
                                  ),
                                ),
                                Icon(Icons.unfold_more_rounded, size: 16, color: tokens.secondaryLabel),
                              ],
                            ),
                          ),
                  ),
                ),
              );
            },
          ),
        );
      },
    );
  }
}

/// Compact row for gate screens: other profiles + "Add profile".
class ProfileSwitcherRow extends ConsumerWidget {
  const ProfileSwitcherRow({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(profilesProvider).value ?? ProfilesState.empty;
    final others = state.profiles.where((p) => p.id != state.activeId).toList();
    final l10n = context.l10n;
    return Wrap(
      spacing: GlassSpacing.s8,
      runSpacing: GlassSpacing.s6,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        for (final p in others)
          GlassButton(
            label: l10n.profileSwitcherSwitchTo(p.name),
            icon: profileIcon(p),
            size: GlassControlSize.sm,
            onPressed: () => switchProfile(context, ref, p),
          ),
        GlassButton(
          label: l10n.profileSwitcherAddProfile,
          icon: Icons.add_rounded,
          size: GlassControlSize.sm,
          onPressed: () => context.go(AppRoutes.welcome),
        ),
      ],
    );
  }
}
