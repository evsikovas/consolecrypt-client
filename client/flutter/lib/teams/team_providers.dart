import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/team_service.dart';
import 'package:consolecrypt/teams/team_models.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

final teamServiceProvider = Provider<TeamService>(
  (ref) => ref.watch(appServicesProvider).teams ?? const UnavailableTeamService(),
);

/// Ticks whenever the core reports team changes.
final teamChangesProvider = StreamProvider<void>((ref) => ref.watch(teamServiceProvider).changes);

bool _teamsAvailable(Ref ref) {
  final profile = ref.watch(activeProfileProvider);
  final phase = ref.watch(vaultStatusProvider).value?.phase;
  return profile != null && !profile.isLocal && phase == VaultPhase.unlocked;
}

/// Teams of the open synced profile (sidebar order).
final teamsProvider = FutureProvider<List<Team>>((ref) async {
  ref.watch(teamChangesProvider);
  if (!_teamsAvailable(ref)) return const [];
  return ref.watch(teamServiceProvider).list();
});

/// Team by vault id.
final teamByIdProvider = Provider<Map<String, Team>>(
  (ref) => {for (final t in ref.watch(teamsProvider).value ?? const <Team>[]) t.vaultId: t},
);

/// Object id → team vault id (absent = personal). Follows inventory changes.
final objectTeamsProvider = FutureProvider<Map<String, String>>((ref) async {
  ref
    ..watch(teamChangesProvider)
    ..watch(hostsProvider)
    ..watch(groupsProvider)
    ..watch(credentialsProvider)
    ..watch(snippetsProvider);
  if (!_teamsAvailable(ref)) return const {};
  return ref.watch(teamServiceProvider).objectVaults();
});

/// The team of an object, if any.
Team? teamOf(WidgetRef ref, ObjectId id) {
  final vault = ref.watch(objectTeamsProvider).value?[id.value];
  return vault == null ? null : ref.watch(teamByIdProvider)[vault];
}

/// Clipboard clearing limit of the team an object belongs to (policy).
Duration? teamClipboardLimit(WidgetRef ref, ObjectId id) {
  final seconds = teamOf(ref, id)?.policies.clipboardClearSeconds ?? 0;
  return seconds > 0 ? Duration(seconds: seconds) : null;
}

final incomingTeamInvitationsProvider = FutureProvider.autoDispose<List<TeamInvitation>>((ref) async {
  ref.watch(teamChangesProvider);
  if (!_teamsAvailable(ref)) return const [];
  return ref.watch(teamServiceProvider).incomingInvitations();
});

final teamMembersProvider = FutureProvider.autoDispose.family<List<TeamMember>, String>((ref, vaultId) async {
  ref.watch(teamChangesProvider);
  return ref.watch(teamServiceProvider).members(vaultId);
});

final teamJoinRequestsProvider = FutureProvider.autoDispose.family<List<TeamJoinRequest>, String>((ref, vaultId) async {
  ref.watch(teamChangesProvider);
  final team = ref.watch(teamByIdProvider)[vaultId];
  if (team == null || !team.isAdmin) return const [];
  return ref.watch(teamServiceProvider).joinRequests(vaultId);
});

final teamInvitationsProvider = FutureProvider.autoDispose.family<List<TeamInvitation>, String>((ref, vaultId) async {
  ref.watch(teamChangesProvider);
  final team = ref.watch(teamByIdProvider)[vaultId];
  if (team == null || !team.isAdmin) return const [];
  return ref.watch(teamServiceProvider).invitations(vaultId);
});

final teamLogProvider = FutureProvider.autoDispose.family<List<TeamLogEntry>, String>((ref, vaultId) async {
  ref.watch(teamChangesProvider);
  return ref.watch(teamServiceProvider).log(vaultId);
});

final teamSettingsProvider = FutureProvider.autoDispose.family<TeamSettings, String>((ref, vaultId) async {
  ref.watch(teamChangesProvider);
  return ref.watch(teamServiceProvider).settings(vaultId);
});

/// Palette of team colours (name → colour), shared by pickers and chips.
const teamPalette = <String, Color>{
  'blue': Color(0xFF3B82F6),
  'teal': Color(0xFF14B8A6),
  'green': Color(0xFF22C55E),
  'yellow': Color(0xFFEAB308),
  'orange': Color(0xFFF97316),
  'red': Color(0xFFEF4444),
  'pink': Color(0xFFEC4899),
  'purple': Color(0xFF8B5CF6),
  'gray': Color(0xFF64748B),
};

/// Display colour of a stored team colour (palette name or `#rrggbb`).
Color? teamColor(String? value) {
  if (value == null) return null;
  final named = teamPalette[value];
  if (named != null) return named;
  final hex = RegExp(r'^#([0-9a-fA-F]{6})$').firstMatch(value);
  return hex == null ? null : Color(0xFF000000 | int.parse(hex.group(1)!, radix: 16));
}
