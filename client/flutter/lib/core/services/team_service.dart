import 'package:consolecrypt/teams/team_models.dart';

/// Teams (protocol 1.9). Membership changes are signed by the core with the
/// member's own team key; the UI only drives them.
abstract interface class TeamService {
  /// Bumps whenever teams, members or invitations may have changed.
  Stream<void> get changes;

  Future<List<Team>> list();
  Future<List<Team>> refresh();

  /// Object id → team vault id (absent = personal).
  Future<Map<String, String>> objectVaults();

  Future<Team> create({required String name, String? color, String? icon, String? description});
  Future<List<TeamMember>> members(String vaultId);
  Future<List<TeamLogEntry>> log(String vaultId);
  Future<TeamSettings> settings(String vaultId);
  Future<TeamSettings> saveSettings(String vaultId, TeamSettings settings);
  Future<Team> setPreferences(String vaultId, {String? localColor, required int sortOrder, required bool hidden});

  Future<void> invite(String vaultId, String email, TeamRole role, {String? note});
  Future<List<TeamInvitation>> invitations(String vaultId);
  Future<List<TeamInvitation>> incomingInvitations();
  Future<TeamJoin> accept(String invitationId);
  Future<void> closeInvitation(String invitationId);
  Future<List<TeamJoinRequest>> joinRequests(String vaultId);
  Future<Team> confirmMember(String vaultId, String invitationId);

  Future<Team> changeRole(String vaultId, String userId, TeamRole role);
  Future<Team> transferOwner(String vaultId, String userId);
  Future<Team> removeMember(String vaultId, String userId);
  Future<void> leave(String vaultId);
  Future<Team> rotateKey(String vaultId);

  /// New group created directly in the team → its id.
  Future<String> createGroup(String vaultId, String name);
  Future<List<String>> copyObjects(List<String> ids, String? targetVaultId);
  Future<List<String>> moveObjects(List<String> ids, String? targetVaultId);
}

/// Backends without teams (local profiles, demo mode): nothing to show.
final class UnavailableTeamService implements TeamService {
  const UnavailableTeamService();

  Never _unavailable() => throw StateError('Teams capability unavailable'); // l10n-ignore: diagnostic

  @override
  Stream<void> get changes => const Stream.empty();
  @override
  Future<List<Team>> list() async => const [];
  @override
  Future<List<Team>> refresh() async => const [];
  @override
  Future<Map<String, String>> objectVaults() async => const {};
  @override
  Future<Team> create({required String name, String? color, String? icon, String? description}) async => _unavailable();
  @override
  Future<List<TeamMember>> members(String vaultId) async => const [];
  @override
  Future<List<TeamLogEntry>> log(String vaultId) async => const [];
  @override
  Future<TeamSettings> settings(String vaultId) async => _unavailable();
  @override
  Future<TeamSettings> saveSettings(String vaultId, TeamSettings settings) async => _unavailable();
  @override
  Future<Team> setPreferences(
    String vaultId, {
    String? localColor,
    required int sortOrder,
    required bool hidden,
  }) async => _unavailable();
  @override
  Future<void> invite(String vaultId, String email, TeamRole role, {String? note}) async => _unavailable();
  @override
  Future<List<TeamInvitation>> invitations(String vaultId) async => const [];
  @override
  Future<List<TeamInvitation>> incomingInvitations() async => const [];
  @override
  Future<TeamJoin> accept(String invitationId) async => _unavailable();
  @override
  Future<void> closeInvitation(String invitationId) async => _unavailable();
  @override
  Future<List<TeamJoinRequest>> joinRequests(String vaultId) async => const [];
  @override
  Future<Team> confirmMember(String vaultId, String invitationId) async => _unavailable();
  @override
  Future<Team> changeRole(String vaultId, String userId, TeamRole role) async => _unavailable();
  @override
  Future<Team> transferOwner(String vaultId, String userId) async => _unavailable();
  @override
  Future<Team> removeMember(String vaultId, String userId) async => _unavailable();
  @override
  Future<void> leave(String vaultId) async => _unavailable();
  @override
  Future<Team> rotateKey(String vaultId) async => _unavailable();
  @override
  Future<String> createGroup(String vaultId, String name) async => _unavailable();
  @override
  Future<List<String>> copyObjects(List<String> ids, String? targetVaultId) async => _unavailable();
  @override
  Future<List<String>> moveObjects(List<String> ids, String? targetVaultId) async => _unavailable();
}
