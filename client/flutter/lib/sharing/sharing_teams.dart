import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/teams/team_models.dart';
import 'package:consolecrypt/teams/team_providers.dart';
import 'package:consolecrypt/teams/teams_settings.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Team access is independent of the device-to-device sharing capability.
/// Only inventory already decrypted by the core is displayed here.
class SharingTeams extends ConsumerWidget {
  const SharingTeams({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    if (ref.watch(teamsSupportedProvider).value != true) return const SizedBox.shrink();
    final l = context.l10n;
    return Padding(
      padding: const EdgeInsets.only(bottom: 20),
      child: SectionCard(
        key: const ValueKey('sharing-teams'),
        title: l.teamsTitle,
        subtitle: l.sharingTeamsHelp,
        icon: Icons.groups_rounded,
        child: AsyncValueView<List<Team>>(
          value: ref.watch(teamsProvider),
          data: (teams) => teams.isEmpty
              ? Text(l.teamsEmpty)
              : Column(children: [for (final team in teams) _TeamAccess(team: team)]),
        ),
      ),
    );
  }
}

class _TeamAccess extends ConsumerWidget {
  const _TeamAccess({required this.team});
  final Team team;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l = context.l10n;
    final objects = ref.watch(objectTeamsProvider);
    final mapping = objects.value ?? const <String, String>{};
    bool belongs(String id) => mapping[id] == team.vaultId;
    final resources = <(IconData, String)>[
      for (final h in ref.watch(hostsProvider).value ?? const <Host>[])
        if (belongs(h.id.value)) (Icons.dns_rounded, h.name),
      for (final g in ref.watch(groupsProvider).value ?? const <Group>[])
        if (belongs(g.id.value)) (Icons.folder_outlined, g.name),
      for (final c in ref.watch(credentialsProvider).value ?? const <Credential>[])
        if (belongs(c.id.value)) (Icons.key_rounded, c.name),
      for (final s in ref.watch(snippetsProvider).value ?? const <Snippet>[])
        if (belongs(s.id.value)) (Icons.code_rounded, s.name),
    ];
    return ExpansionTile(
      key: ValueKey('sharing-team-${team.vaultId}'),
      title: TeamChip(team: team),
      subtitle: Text(
        [
          team.role == null ? l.teamsReadOnly : teamRoleLabel(l, team.role!),
          if (team.open && objects.hasValue) l.sharingTeamResources(resources.length),
        ].join(' · '),
      ),
      children: [
        if (team.open) ...[
          AsyncValueView<List<TeamMember>>(
            value: ref.watch(teamMembersProvider(team.vaultId)),
            data: (members) => Column(
              children: [
                for (final member in members)
                  ListTile(
                    dense: true,
                    leading: const Icon(Icons.person_outline_rounded, size: 18),
                    title: Text(member.email),
                    trailing: Text(teamRoleLabel(l, member.role)),
                  ),
              ],
            ),
          ),
          if (objects.isLoading)
            const LinearProgressIndicator()
          else if (objects.hasError)
            Text(l.sharingBlocked)
          else if (resources.isEmpty)
            Padding(padding: const EdgeInsets.all(12), child: Text(l.sharingTeamNoResources)),
          for (final resource in resources)
            ListTile(dense: true, leading: Icon(resource.$1, size: 18), title: SelectableText(resource.$2)),
          GlassButton.plain(
            label: l.navSettings,
            icon: Icons.settings_outlined,
            onPressed: () => showTeamDialog(context, team.vaultId),
          ),
        ] else
          Padding(padding: const EdgeInsets.all(12), child: Text(l.sharingTeamLocked)),
      ],
    );
  }
}
