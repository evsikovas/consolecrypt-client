import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:consolecrypt/settings/color_picker.dart';
import 'package:consolecrypt/teams/team_models.dart';
import 'package:consolecrypt/teams/team_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

String teamRoleLabel(AppLocalizations l, TeamRole role) => switch (role) {
  TeamRole.owner => l.teamsRoleOwner,
  TeamRole.admin => l.teamsRoleAdmin,
  TeamRole.editor => l.teamsRoleEditor,
  TeamRole.viewer => l.teamsRoleViewer,
};

/// Small coloured square marking a team (sidebar, chips, rows).
class TeamColorDot extends StatelessWidget {
  const TeamColorDot({required this.team, super.key, this.size = 10});

  final Team team;
  final double size;

  @override
  Widget build(BuildContext context) {
    final color = teamColor(team.color) ?? GlassTokens.of(context).secondaryLabel;
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(color: color, borderRadius: BorderRadius.circular(size / 4)),
    );
  }
}

/// Chip with the team colour and name (lists, editors, tabs).
class TeamChip extends StatelessWidget {
  const TeamChip({required this.team, super.key, this.dense = true});

  final Team team;
  final bool dense;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final readOnly = team.role != null && !team.canEdit;
    return Semantics(
      label: l10n.teamsChipSemantics(team.name),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          TeamColorDot(team: team, size: dense ? 8 : 10),
          const SizedBox(width: GlassSpacing.s4),
          Text(team.name.isEmpty ? l10n.teamsUnnamed : team.name, style: GlassTokens.of(context).typography.caption),
          if (readOnly) ...[const SizedBox(width: GlassSpacing.s4), GlassBadge(label: l10n.teamsReadOnly, dense: true)],
        ],
      ),
    );
  }
}

/// Settings → Teams (TZ teams ТР-28).
class TeamsSettingsSection extends ConsumerWidget {
  const TeamsSettingsSection({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final profile = ref.watch(activeProfileProvider);
    if (profile == null || profile.isLocal) return const SizedBox.shrink();
    if (ref.watch(teamsSupportedProvider).value != true) return const SizedBox.shrink();
    final teams = ref.watch(teamsProvider);
    final incoming = ref.watch(incomingTeamInvitationsProvider).value ?? const <TeamInvitation>[];
    return SectionCard(
      key: const ValueKey('settings-teams'),
      title: l10n.teamsTitle,
      icon: Icons.groups_rounded,
      subtitle: l10n.teamsSubtitle,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final inv in incoming) _IncomingInvitationRow(invitation: inv),
          if (incoming.isNotEmpty) const SizedBox(height: GlassSpacing.s8),
          AsyncValueView<List<Team>>(
            value: teams,
            data: (list) => list.isEmpty
                ? Text(l10n.teamsEmpty, style: GlassTokens.of(context).typography.callout)
                : Column(children: [for (final t in list) _TeamRow(team: t)]),
          ),
          const SizedBox(height: GlassSpacing.s12),
          Wrap(
            spacing: GlassSpacing.s8,
            runSpacing: GlassSpacing.s8,
            children: [
              GlassButton(
                key: const ValueKey('teams-create'),
                icon: Icons.group_add_rounded,
                label: l10n.teamsCreate,
                onPressed: () => showCreateTeamDialog(context),
              ),
              GlassButton.plain(
                key: const ValueKey('teams-refresh'),
                icon: Icons.refresh_rounded,
                label: l10n.teamsRefresh,
                onPressed: () => runWithFeedback(context, () => ref.read(teamServiceProvider).refresh()),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _TeamRow extends ConsumerWidget {
  const _TeamRow({required this.team});

  final Team team;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final String meta;
    if (team.pending) {
      meta = l10n.teamsPendingConfirmation;
    } else if (!team.open) {
      meta = l10n.teamsUnavailable;
    } else {
      meta = l10n.teamsRowMeta(team.role == null ? '—' : teamRoleLabel(l10n, team.role!), team.membersCount);
    }
    return ListTile(
      key: ValueKey('team-row-${team.vaultId}'),
      contentPadding: EdgeInsets.zero,
      leading: TeamColorDot(team: team, size: 14),
      title: Text(team.name.isEmpty ? l10n.teamsUnnamed : team.name),
      subtitle: Text(meta, style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel)),
      trailing: Wrap(
        spacing: GlassSpacing.s4,
        children: [
          if (team.warning != null) GlassBadge(label: l10n.teamsWarningBadge, tone: GlassTone.danger),
          if (team.rotationRequired && team.isAdmin)
            GlassBadge(label: l10n.teamsRotationBadge, tone: GlassTone.warning),
          if (team.offline) GlassBadge(label: l10n.teamsOfflineBadge),
        ],
      ),
      onTap: team.open ? () => showTeamDialog(context, team.vaultId) : null,
    );
  }
}

class _IncomingInvitationRow extends ConsumerWidget {
  const _IncomingInvitationRow({required this.invitation});

  final TeamInvitation invitation;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final service = ref.read(teamServiceProvider);
    return InfoBanner(
      key: ValueKey('team-invitation-${invitation.invitationId}'),
      tone: BannerTone.info,
      message: [
        l10n.teamsInvitationFrom(invitation.inviterEmail, teamRoleLabel(l10n, invitation.role)),
        if (invitation.note != null) l10n.teamsInvitationNote(invitation.note!),
      ].join('\n'),
      action: Wrap(
        spacing: GlassSpacing.s8,
        children: [
          GlassButton.plain(
            label: l10n.teamsDecline,
            onPressed: () => runWithFeedback(context, () => service.closeInvitation(invitation.invitationId)),
          ),
          GlassButton.prominent(
            key: ValueKey('team-accept-${invitation.invitationId}'),
            label: l10n.teamsAccept,
            onPressed: () async {
              final join = await runWithFeedback(context, () => service.accept(invitation.invitationId));
              if (join != null && context.mounted) await showTeamJoinCodeDialog(context, join);
            },
          ),
        ],
      ),
    );
  }
}

/// Choose a team (copy / move targets). `null` on cancel.
Future<Team?> pickTeam(BuildContext context, List<Team> teams) => showAppDialog<Team>(
  context,
  builder: (context) {
    final l10n = context.l10n;
    return GlassDialog(
      key: const ValueKey('team-picker'),
      title: l10n.teamsPickTitle,
      icon: Icons.groups_rounded,
      width: 420,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final t in teams)
            ListTile(
              key: ValueKey('team-pick-${t.vaultId}'),
              contentPadding: EdgeInsets.zero,
              leading: TeamColorDot(team: t, size: 14),
              title: Text(t.name.isEmpty ? l10n.teamsUnnamed : t.name),
              onTap: () => closeDialog(context, t),
            ),
        ],
      ),
      secondaryActions: [GlassButton.plain(label: l10n.commonCancel, onPressed: () => closeDialog<Team>(context))],
    );
  },
);

// ---- create ----------------------------------------------------------------------------

Future<void> showCreateTeamDialog(BuildContext context) =>
    showAppDialog<void>(context, builder: (_) => const _CreateTeamDialog());

class _CreateTeamDialog extends ConsumerStatefulWidget {
  const _CreateTeamDialog();

  @override
  ConsumerState<_CreateTeamDialog> createState() => _CreateTeamDialogState();
}

class _CreateTeamDialogState extends ConsumerState<_CreateTeamDialog> {
  final _name = TextEditingController();
  final _description = TextEditingController();
  String? _color = 'blue';
  bool _busy = false;
  AppException? _error;

  @override
  void dispose() {
    _name.dispose();
    _description.dispose();
    super.dispose();
  }

  Future<void> _create() async {
    if (_name.text.trim().isEmpty) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final description = _description.text.trim();
      await ref
          .read(teamServiceProvider)
          .create(name: _name.text.trim(), color: _color, description: description.isEmpty ? null : description);
      if (mounted) {
        showSnack(context, context.l10n.teamsCreated(_name.text.trim()), tone: GlassTone.success);
        closeDialog<void>(context);
      }
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return GlassDialog(
      key: const ValueKey('create-team-dialog'),
      title: l10n.teamsCreateTitle,
      icon: Icons.group_add_rounded,
      width: 520,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l10n.teamsCreateBody),
          const SizedBox(height: GlassSpacing.s12),
          GlassField(
            fieldKey: const ValueKey('team-name'),
            controller: _name,
            placeholder: l10n.teamsNameLabel,
            autofocus: true,
            onChanged: (_) => setState(() {}),
            onSubmitted: (_) => _create(),
          ),
          const SizedBox(height: GlassSpacing.s8),
          GlassField(controller: _description, placeholder: l10n.teamsDescriptionLabel, maxLines: 3),
          const SizedBox(height: GlassSpacing.s12),
          Text(l10n.teamsColorLabel, style: GlassTokens.of(context).typography.bodyEmph),
          const SizedBox(height: GlassSpacing.s4),
          TeamColorChooser(value: _color, onChanged: (c) => setState(() => _color = c)),
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s8),
            GateErrorText(text: errorMessage(l10n, _error!)),
          ],
        ],
      ),
      secondaryActions: [GlassButton.plain(label: l10n.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('team-create-confirm'),
        label: l10n.teamsCreate,
        onPressed: _busy || _name.text.trim().isEmpty ? null : _create,
      ),
    );
  }
}

/// Palette swatches + custom colour.
class TeamColorChooser extends StatelessWidget {
  const TeamColorChooser({required this.value, required this.onChanged, super.key, this.allowNone = false});

  final String? value;
  final ValueChanged<String?> onChanged;

  /// Offer "no colour" (e.g. clear the personal override).
  final bool allowNone;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    Widget swatch(String? name, Color? color, String label) {
      final selected = value == name;
      return Semantics(
        selected: selected,
        button: true,
        label: label,
        child: InkWell(
          key: ValueKey('team-color-${name ?? 'none'}'),
          borderRadius: BorderRadius.circular(8),
          onTap: () => onChanged(name),
          child: Container(
            width: 28,
            height: 28,
            decoration: BoxDecoration(
              color: color ?? Colors.transparent,
              borderRadius: BorderRadius.circular(8),
              border: Border.all(
                color: selected ? tokens.palette.label : tokens.palette.tertiary,
                width: selected ? 2.5 : 1,
              ),
            ),
            child: color == null ? Icon(Icons.block_rounded, size: 16, color: tokens.secondaryLabel) : null,
          ),
        ),
      );
    }

    final custom = value != null && !teamPalette.containsKey(value);
    return Wrap(
      spacing: GlassSpacing.s8,
      runSpacing: GlassSpacing.s8,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        if (allowNone) swatch(null, null, l10n.teamsColorNone),
        for (final e in teamPalette.entries) swatch(e.key, e.value, e.key),
        if (custom) swatch(value, teamColor(value), value!),
        GlassButton.plain(
          icon: Icons.palette_rounded,
          label: l10n.teamsColorCustom,
          onPressed: () async {
            final current = teamColor(value) ?? teamPalette['blue']!;
            final rgb = await pickRgbColor(context, title: l10n.teamsColorLabel, color: current.toARGB32() & 0xFFFFFF);
            if (rgb != null) onChanged('#${rgb.toRadixString(16).padLeft(6, '0')}');
          },
        ),
      ],
    );
  }
}

// ---- join codes ------------------------------------------------------------------------

/// Invitee side (ТР-6): the code to read to the inviting admin.
Future<void> showTeamJoinCodeDialog(BuildContext context, TeamJoin join) => showGlassDialog<void>(
  context,
  variant: GlassVariant.secure,
  opaque: true,
  builder: (context) {
    final l10n = context.l10n;
    return GlassDialog(
      key: const ValueKey('team-join-code'),
      icon: Icons.verified_user_rounded,
      iconTone: GlassTone.neutral,
      title: l10n.teamsJoinCodeTitle,
      width: 600,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l10n.teamsJoinCodeBody(join.inviterEmail)),
          const SizedBox(height: GlassSpacing.s16),
          VerificationCodeView(code: VerificationCode(join.code.split(' '))),
          const SizedBox(height: GlassSpacing.s16),
          InfoBanner(tone: BannerTone.info, message: l10n.teamsJoinCodeWait),
        ],
      ),
      primaryAction: GlassButton.prominent(
        autofocus: true,
        label: l10n.commonOk,
        onPressed: () => closeDialog<void>(context),
      ),
    );
  },
);

/// Admin side (ТР-7): compare the code with the invitee's screen. "Add"
/// stays disabled until the admin confirms the codes match; "Codes don't
/// match" rejects the join.
Future<void> showTeamCodeCompareDialog(BuildContext context, TeamJoinRequest request) => showGlassDialog<void>(
  context,
  variant: GlassVariant.secure,
  opaque: true,
  barrierDismissible: false,
  builder: (_) => _CodeCompareDialog(request: request),
);

class _CodeCompareDialog extends ConsumerStatefulWidget {
  const _CodeCompareDialog({required this.request});

  final TeamJoinRequest request;

  @override
  ConsumerState<_CodeCompareDialog> createState() => _CodeCompareDialogState();
}

class _CodeCompareDialogState extends ConsumerState<_CodeCompareDialog> {
  bool _confirmed = false;
  bool _busy = false;
  bool _rejected = false;
  AppException? _error;

  Future<void> _add() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await ref.read(teamServiceProvider).confirmMember(widget.request.vaultId, widget.request.invitationId);
      if (mounted) {
        showSnack(context, context.l10n.teamsMemberAdded(widget.request.inviteeEmail), tone: GlassTone.success);
        closeDialog<void>(context);
      }
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _mismatch() async {
    setState(() => _busy = true);
    try {
      await ref.read(teamServiceProvider).closeInvitation(widget.request.invitationId);
    } on AppException catch (_) {
      // The warning matters more than a failed reject.
    }
    if (mounted) {
      setState(() {
        _busy = false;
        _rejected = true;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    final r = widget.request;
    if (_rejected) {
      return GlassDialog(
        key: const ValueKey('team-code-rejected'),
        width: 600,
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(Icons.gpp_bad_rounded, size: 40, color: tokens.palette.danger),
            const SizedBox(height: GlassSpacing.s12),
            Text(l10n.teamsCodeRejectedTitle, style: tokens.typography.title2),
            const SizedBox(height: GlassSpacing.s8),
            Text(l10n.teamsCodeRejectedBody),
          ],
        ),
        primaryAction: GlassButton.prominent(
          autofocus: true,
          label: l10n.commonOk,
          onPressed: () => closeDialog<void>(context),
        ),
      );
    }
    return GlassDialog(
      key: const ValueKey('team-code-compare'),
      icon: Icons.person_add_alt_1_rounded,
      iconTone: GlassTone.neutral,
      title: l10n.teamsCompareTitle(r.inviteeEmail),
      width: 600,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l10n.teamsCompareRole(teamRoleLabel(l10n, r.role))),
          const SizedBox(height: GlassSpacing.s8),
          Text(l10n.teamsCompareBody(r.inviteeEmail)),
          const SizedBox(height: GlassSpacing.s16),
          VerificationCodeView(code: VerificationCode(r.code.split(' '))),
          const SizedBox(height: GlassSpacing.s16),
          CheckboxListTile(
            key: const ValueKey('team-codes-match'),
            contentPadding: EdgeInsets.zero,
            controlAffinity: ListTileControlAffinity.leading,
            value: _confirmed,
            onChanged: _busy ? null : (v) => setState(() => _confirmed = v ?? false),
            title: Text(l10n.teamsCodesMatchCheckbox(r.inviteeEmail)),
          ),
          InfoBanner(tone: BannerTone.warning, message: l10n.teamsCompareWarning),
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s8),
            GateErrorText(text: errorMessage(l10n, _error!)),
          ],
        ],
      ),
      leadingAction: GlassButton(
        key: const ValueKey('team-codes-mismatch'),
        style: GlassButtonStyle.destructiveQuiet,
        icon: Icons.gpp_bad_rounded,
        onPressed: _busy ? null : _mismatch,
        label: l10n.teamsCodesDontMatch,
      ),
      secondaryActions: [
        GlassButton.plain(
          autofocus: true,
          onPressed: _busy ? null : () => closeDialog<void>(context),
          label: l10n.commonCancel,
        ),
      ],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('team-add-member'),
        onPressed: _confirmed && !_busy ? _add : null,
        label: l10n.teamsAddMember,
      ),
    );
  }
}

// ---- team details --------------------------------------------------------------------

Future<void> showTeamDialog(BuildContext context, String vaultId) =>
    showAppDialog<void>(context, builder: (_) => _TeamDialog(vaultId: vaultId));

enum _TeamTab { members, settings, history }

class _TeamDialog extends ConsumerStatefulWidget {
  const _TeamDialog({required this.vaultId});

  final String vaultId;

  @override
  ConsumerState<_TeamDialog> createState() => _TeamDialogState();
}

class _TeamDialogState extends ConsumerState<_TeamDialog> {
  _TeamTab _tab = _TeamTab.members;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final team = ref.watch(teamByIdProvider)[widget.vaultId];
    if (team == null) {
      return GlassDialog(
        title: l10n.teamsTitle,
        content: Text(l10n.teamsUnavailable),
        primaryAction: GlassButton.prominent(label: l10n.commonOk, onPressed: () => closeDialog<void>(context)),
      );
    }
    return GlassDialog(
      key: const ValueKey('team-dialog'),
      title: team.name.isEmpty ? l10n.teamsUnnamed : team.name,
      icon: Icons.groups_rounded,
      width: 720,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (team.warning != null) ...[
            InfoBanner(
              tone: BannerTone.danger,
              message: team.warning == 'log_rollback' ? l10n.teamsWarningRollback : l10n.teamsWarningInvalidLog,
            ),
            const SizedBox(height: GlassSpacing.s8),
          ],
          if (team.rotationRequired && team.isAdmin) ...[
            InfoBanner(
              tone: BannerTone.warning,
              message: l10n.teamsRotationRequired,
              action: GlassButton(
                label: l10n.teamsRotateKey,
                onPressed: () => runWithFeedback(
                  context,
                  () => ref.read(teamServiceProvider).rotateKey(team.vaultId),
                  success: l10n.teamsKeyRotated,
                ),
              ),
            ),
            const SizedBox(height: GlassSpacing.s8),
          ],
          GlassSegmented<_TeamTab>(
            inChrome: false,
            segments: [
              GlassSegment(value: _TeamTab.members, label: l10n.teamsTabMembers),
              GlassSegment(value: _TeamTab.settings, label: l10n.teamsTabSettings),
              GlassSegment(value: _TeamTab.history, label: l10n.teamsTabHistory),
            ],
            selected: _tab,
            onChanged: (t) => setState(() => _tab = t),
          ),
          const SizedBox(height: GlassSpacing.s12),
          ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 460),
            child: SingleChildScrollView(
              child: switch (_tab) {
                _TeamTab.members => _MembersTab(team: team),
                _TeamTab.settings => _SettingsTab(team: team),
                _TeamTab.history => _HistoryTab(team: team),
              },
            ),
          ),
        ],
      ),
      primaryAction: GlassButton.prominent(label: l10n.commonDone, onPressed: () => closeDialog<void>(context)),
    );
  }
}

class _MembersTab extends ConsumerWidget {
  const _MembersTab({required this.team});

  final Team team;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final service = ref.read(teamServiceProvider);
    final members = ref.watch(teamMembersProvider(team.vaultId));
    final requests = ref.watch(teamJoinRequestsProvider(team.vaultId)).value ?? const <TeamJoinRequest>[];
    final invitations = (ref.watch(teamInvitationsProvider(team.vaultId)).value ?? const <TeamInvitation>[])
        .where((i) => i.status == 'pending')
        .toList();
    final tokens = GlassTokens.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final r in requests)
          InfoBanner(
            key: ValueKey('team-join-request-${r.invitationId}'),
            tone: BannerTone.warning,
            message: l10n.teamsJoinRequest(r.inviteeEmail, teamRoleLabel(l10n, r.role)),
            action: GlassButton.prominent(
              key: ValueKey('team-compare-${r.invitationId}'),
              label: l10n.teamsCompareCode,
              onPressed: () => showTeamCodeCompareDialog(context, r),
            ),
          ),
        AsyncValueView<List<TeamMember>>(
          value: members,
          data: (list) => Column(
            children: [
              for (final m in list)
                ListTile(
                  key: ValueKey('team-member-${m.userId}'),
                  contentPadding: EdgeInsets.zero,
                  leading: const Icon(Icons.person_rounded),
                  title: Text(m.isSelf ? l10n.teamsMemberYou(m.email) : m.email),
                  subtitle: Text(
                    teamRoleLabel(l10n, m.role),
                    style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                  ),
                  trailing: team.isAdmin && !m.isSelf && m.role != TeamRole.owner
                      ? _MemberMenu(team: team, member: m)
                      : null,
                ),
            ],
          ),
        ),
        if (invitations.isNotEmpty) ...[
          const SizedBox(height: GlassSpacing.s8),
          Text(l10n.teamsPendingInvitations, style: tokens.typography.bodyEmph),
          for (final i in invitations)
            ListTile(
              contentPadding: EdgeInsets.zero,
              leading: const Icon(Icons.mail_outline_rounded),
              title: Text(i.inviteeEmail),
              subtitle: Text(teamRoleLabel(l10n, i.role)),
              trailing: GlassButton.plain(
                label: l10n.teamsCancelInvitation,
                onPressed: () => runWithFeedback(context, () => service.closeInvitation(i.invitationId)),
              ),
            ),
        ],
        const SizedBox(height: GlassSpacing.s12),
        if (team.isAdmin)
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton(
              key: const ValueKey('team-invite'),
              icon: Icons.person_add_rounded,
              label: l10n.teamsInvite,
              onPressed: () => showInviteDialog(context, team),
            ),
          ),
      ],
    );
  }
}

class _MemberMenu extends ConsumerWidget {
  const _MemberMenu({required this.team, required this.member});

  final Team team;
  final TeamMember member;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final service = ref.read(teamServiceProvider);
    final owner = team.role == TeamRole.owner;
    final roles = [if (owner) TeamRole.admin, TeamRole.editor, TeamRole.viewer];
    return Wrap(
      spacing: GlassSpacing.s4,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        if (owner || member.role != TeamRole.admin)
          GlassSelect<TeamRole>(
            key: ValueKey('team-role-${member.userId}'),
            value: roles.contains(member.role) ? member.role : TeamRole.editor,
            items: [for (final r in roles) GlassSelectItem(value: r, label: teamRoleLabel(l10n, r))],
            onChanged: (r) {
              if (r != member.role) runWithFeedback(context, () => service.changeRole(team.vaultId, member.userId, r));
            },
          ),
        if (owner)
          GlassIconButton(
            icon: Icons.workspace_premium_rounded,
            tooltip: l10n.teamsTransferOwner,
            onPressed: () async {
              final ok = await showConfirmDialog(
                context,
                title: l10n.teamsTransferOwner,
                message: l10n.teamsTransferOwnerConfirm(member.email),
                confirmLabel: l10n.teamsTransferOwner,
              );
              if (ok && context.mounted) {
                await runWithFeedback(context, () => service.transferOwner(team.vaultId, member.userId));
              }
            },
          ),
        if (owner || member.role != TeamRole.admin)
          GlassIconButton(
            key: ValueKey('team-remove-${member.userId}'),
            icon: Icons.person_remove_rounded,
            tooltip: l10n.teamsRemoveMember,
            onPressed: () async {
              final ok = await showConfirmDialog(
                context,
                title: l10n.teamsRemoveMember,
                message: l10n.teamsRemoveMemberConfirm(member.email),
                confirmLabel: l10n.teamsRemoveMember,
                destructive: true,
              );
              if (ok && context.mounted) {
                await runWithFeedback(
                  context,
                  () => service.removeMember(team.vaultId, member.userId),
                  success: l10n.teamsKeyRotated,
                );
              }
            },
          ),
      ],
    );
  }
}

Future<void> showInviteDialog(BuildContext context, Team team) =>
    showAppDialog<void>(context, builder: (_) => _InviteDialog(team: team));

class _InviteDialog extends ConsumerStatefulWidget {
  const _InviteDialog({required this.team});

  final Team team;

  @override
  ConsumerState<_InviteDialog> createState() => _InviteDialogState();
}

class _InviteDialogState extends ConsumerState<_InviteDialog> {
  final _email = TextEditingController();
  final _note = TextEditingController();
  TeamRole _role = TeamRole.editor;
  bool _busy = false;
  AppException? _error;

  @override
  void dispose() {
    _email.dispose();
    _note.dispose();
    super.dispose();
  }

  Future<void> _invite() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final note = _note.text.trim();
      await ref
          .read(teamServiceProvider)
          .invite(widget.team.vaultId, _email.text.trim(), _role, note: note.isEmpty ? null : note);
      if (mounted) {
        showSnack(context, context.l10n.teamsInvited(_email.text.trim()), tone: GlassTone.success);
        closeDialog<void>(context);
      }
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final roles = [if (widget.team.role == TeamRole.owner) TeamRole.admin, TeamRole.editor, TeamRole.viewer];
    return GlassDialog(
      key: const ValueKey('team-invite-dialog'),
      title: l10n.teamsInviteTitle(widget.team.name),
      icon: Icons.person_add_rounded,
      width: 520,
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l10n.teamsInviteBody),
          const SizedBox(height: GlassSpacing.s12),
          GlassField(
            fieldKey: const ValueKey('team-invite-email'),
            controller: _email,
            placeholder: l10n.teamsInviteEmail,
            keyboardType: TextInputType.emailAddress,
            autofocus: true,
            onChanged: (_) => setState(() {}),
          ),
          const SizedBox(height: GlassSpacing.s8),
          LabeledValue(
            label: l10n.teamsRoleLabel,
            value: Align(
              alignment: Alignment.centerLeft,
              child: GlassSelect<TeamRole>(
                key: const ValueKey('team-invite-role'),
                value: _role,
                items: [for (final r in roles) GlassSelectItem(value: r, label: teamRoleLabel(l10n, r))],
                onChanged: (r) => setState(() => _role = r),
              ),
            ),
          ),
          const SizedBox(height: GlassSpacing.s8),
          GlassField(controller: _note, placeholder: l10n.teamsInviteNote),
          const SizedBox(height: GlassSpacing.s4),
          Text(
            l10n.teamsInviteNoteWarning,
            style: GlassTokens.of(context).typography.caption.copyWith(color: GlassTokens.of(context).secondaryLabel),
          ),
          if (_error != null) ...[
            const SizedBox(height: GlassSpacing.s8),
            GateErrorText(text: errorMessage(l10n, _error!)),
          ],
        ],
      ),
      secondaryActions: [GlassButton.plain(label: l10n.commonCancel, onPressed: () => closeDialog<void>(context))],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('team-invite-send'),
        label: l10n.teamsInviteSend,
        onPressed: _busy || !_email.text.contains('@') ? null : _invite,
      ),
    );
  }
}

class _SettingsTab extends ConsumerStatefulWidget {
  const _SettingsTab({required this.team});

  final Team team;

  @override
  ConsumerState<_SettingsTab> createState() => _SettingsTabState();
}

class _SettingsTabState extends ConsumerState<_SettingsTab> {
  TeamSettings? _draft;
  final _name = TextEditingController();
  final _description = TextEditingController();

  @override
  void dispose() {
    _name.dispose();
    _description.dispose();
    super.dispose();
  }

  void _load(TeamSettings s) {
    if (_draft != null) return;
    _draft = s;
    _name.text = s.name;
    _description.text = s.description ?? '';
  }

  Future<void> _save() async {
    final draft = _draft;
    if (draft == null) return;
    final saved = await runWithFeedback(
      context,
      () => ref
          .read(teamServiceProvider)
          .saveSettings(
            widget.team.vaultId,
            draft.copyWith(name: _name.text.trim(), description: _description.text.trim()),
          ),
      success: context.l10n.teamsSettingsSaved,
    );
    if (saved != null && mounted) setState(() => _draft = saved);
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final team = widget.team;
    final service = ref.read(teamServiceProvider);
    final settings = ref.watch(teamSettingsProvider(team.vaultId));
    final tokens = GlassTokens.of(context);
    final admin = team.isAdmin;
    return AsyncValueView<TeamSettings>(
      value: settings,
      data: (s) {
        _load(s);
        final draft = _draft ?? s;
        final p = draft.policies;
        void setPolicies(TeamPolicies v) => setState(() => _draft = draft.copyWith(policies: v));
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (!admin) ...[
              InfoBanner(tone: BannerTone.info, message: l10n.teamsSettingsReadOnly),
              const SizedBox(height: GlassSpacing.s8),
            ],
            GlassField(controller: _name, placeholder: l10n.teamsNameLabel, enabled: admin),
            const SizedBox(height: GlassSpacing.s8),
            GlassField(controller: _description, placeholder: l10n.teamsDescriptionLabel, enabled: admin, maxLines: 3),
            const SizedBox(height: GlassSpacing.s12),
            Text(l10n.teamsColorLabel, style: tokens.typography.bodyEmph),
            const SizedBox(height: GlassSpacing.s4),
            IgnorePointer(
              ignoring: !admin,
              child: TeamColorChooser(
                value: draft.color,
                onChanged: (c) =>
                    setState(() => _draft = c == null ? draft.copyWith(clearColor: true) : draft.copyWith(color: c)),
              ),
            ),
            const SizedBox(height: GlassSpacing.s12),
            Text(l10n.teamsPoliciesTitle, style: tokens.typography.bodyEmph),
            SwitchListTile.adaptive(
              contentPadding: EdgeInsets.zero,
              value: p.productionByDefault,
              onChanged: admin ? (v) => setPolicies(p.copyWith(productionByDefault: v)) : null,
              title: Text(l10n.teamsPolicyProduction),
            ),
            SwitchListTile.adaptive(
              key: const ValueKey('team-policy-ai'),
              contentPadding: EdgeInsets.zero,
              value: p.blockAiContext,
              onChanged: admin ? (v) => setPolicies(p.copyWith(blockAiContext: v)) : null,
              title: Text(l10n.teamsPolicyBlockAi),
            ),
            SwitchListTile.adaptive(
              contentPadding: EdgeInsets.zero,
              value: p.confirmRiskyCommands,
              onChanged: admin ? (v) => setPolicies(p.copyWith(confirmRiskyCommands: v)) : null,
              title: Text(l10n.teamsPolicyConfirmRisky),
            ),
            LabeledValue(
              label: l10n.teamsPolicyClipboard,
              value: Align(
                alignment: Alignment.centerLeft,
                child: GlassSelect<int>(
                  value: const [0, 10, 30, 60].contains(p.clipboardClearSeconds) ? p.clipboardClearSeconds : 0,
                  items: [
                    GlassSelectItem(value: 0, label: l10n.teamsPolicyClipboardDefault),
                    for (final s in const [10, 30, 60]) GlassSelectItem(value: s, label: l10n.teamsPolicySeconds(s)),
                  ],
                  onChanged: admin ? (v) => setPolicies(p.copyWith(clipboardClearSeconds: v)) : null,
                ),
              ),
            ),
            if (admin) ...[
              const SizedBox(height: GlassSpacing.s8),
              Align(
                alignment: Alignment.centerLeft,
                child: GlassButton.prominent(
                  key: const ValueKey('team-settings-save'),
                  label: l10n.commonSave,
                  onPressed: _save,
                ),
              ),
            ],
            const SizedBox(height: GlassSpacing.s16),
            Text(l10n.teamsMyColorTitle, style: tokens.typography.bodyEmph),
            const SizedBox(height: GlassSpacing.s4),
            Text(l10n.teamsMyColorBody, style: tokens.typography.callout),
            const SizedBox(height: GlassSpacing.s4),
            TeamColorChooser(
              allowNone: true,
              value: team.localColor,
              onChanged: (c) => runWithFeedback(
                context,
                () =>
                    service.setPreferences(team.vaultId, localColor: c, sortOrder: team.sortOrder, hidden: team.hidden),
              ),
            ),
            const SizedBox(height: GlassSpacing.s16),
            Wrap(
              spacing: GlassSpacing.s8,
              runSpacing: GlassSpacing.s8,
              children: [
                if (admin)
                  GlassButton(
                    icon: Icons.key_rounded,
                    label: l10n.teamsRotateKey,
                    onPressed: () =>
                        runWithFeedback(context, () => service.rotateKey(team.vaultId), success: l10n.teamsKeyRotated),
                  ),
                if (team.role != TeamRole.owner)
                  GlassButton(
                    key: const ValueKey('team-leave'),
                    style: GlassButtonStyle.destructiveQuiet,
                    icon: Icons.logout_rounded,
                    label: l10n.teamsLeave,
                    onPressed: () async {
                      final ok = await showConfirmDialog(
                        context,
                        title: l10n.teamsLeave,
                        message: l10n.teamsLeaveConfirm(team.name),
                        confirmLabel: l10n.teamsLeave,
                        destructive: true,
                      );
                      if (ok && context.mounted) {
                        final done = await runWithFeedback(context, () async {
                          await service.leave(team.vaultId);
                          return true;
                        });
                        if (done == true && context.mounted) closeDialog<void>(context);
                      }
                    },
                  ),
              ],
            ),
            const SizedBox(height: GlassSpacing.s8),
            Text(
              l10n.teamsKeyEpoch(team.keyEpoch),
              style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
            ),
          ],
        );
      },
    );
  }
}

class _HistoryTab extends ConsumerWidget {
  const _HistoryTab({required this.team});

  final Team team;

  String _describe(AppLocalizations l, TeamLogEntry e) {
    final target = e.targetEmail ?? '';
    final role = e.role == null ? '' : teamRoleLabel(l, e.role!);
    return switch (e.op) {
      'genesis' => l.teamsLogGenesis(e.actorEmail),
      'add_member' => l.teamsLogAdd(e.actorEmail, target, role),
      'change_role' => l.teamsLogRole(e.actorEmail, target, role),
      'remove_member' => l.teamsLogRemove(e.actorEmail, target),
      'leave' => l.teamsLogLeave(e.actorEmail),
      'transfer_owner' => l.teamsLogTransfer(e.actorEmail, target),
      'rotate_key' => l.teamsLogRotate(e.actorEmail, e.keyEpoch ?? 0),
      _ => e.op,
    };
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final tokens = GlassTokens.of(context);
    return AsyncValueView<List<TeamLogEntry>>(
      value: ref.watch(teamLogProvider(team.vaultId)),
      data: (entries) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final e in entries)
            ListTile(
              contentPadding: EdgeInsets.zero,
              dense: true,
              title: Text(_describe(l10n, e)),
              subtitle: e.issuedAt == null
                  ? null
                  : Text(
                      formatDateTime(l10n, e.issuedAt!),
                      style: tokens.typography.caption.copyWith(color: tokens.secondaryLabel),
                    ),
            ),
        ],
      ),
    );
  }
}
