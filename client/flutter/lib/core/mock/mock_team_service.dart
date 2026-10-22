import 'dart:async';

import 'package:consolecrypt/core/mock/mock_inventory_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/services/team_service.dart';
import 'package:consolecrypt/teams/team_models.dart';

/// In-memory teams for widget tests and the website guide captures. No
/// signatures, keys or networking: membership changes are plain list edits.
/// The real backend is `RustTeamService`.
final class MockTeamService implements TeamService {
  MockTeamService({required this.selfEmail, this.inventory, DateTime Function()? clock})
    : _clock = clock ?? DateTime.now;

  /// The signed-in account (marked "you" in member lists).
  final String selfEmail;

  /// Groups created "in the team" are saved here when present.
  final MockInventoryService? inventory;
  final DateTime Function() _clock;

  final _teams = <String, Team>{};
  final _members = <String, List<TeamMember>>{};
  final _log = <String, List<TeamLogEntry>>{};
  final _settings = <String, TeamSettings>{};
  final _invitations = <String, List<TeamInvitation>>{};
  final _joinRequests = <String, List<TeamJoinRequest>>{};
  final _incoming = <TeamInvitation>[];

  /// Object id → team vault id (absent = personal).
  final objects = <String, String>{};
  final _changes = StreamController<void>.broadcast();
  var _next = 0;

  void _changed() => _changes.add(null);

  Team _require(String vaultId) =>
      _teams[vaultId] ??
      (throw AppException(AppErrorCode.notFound, 'team $vaultId not found')); // l10n-ignore: diagnostic

  /// Adds a team as it would arrive from the server (tests, demo data).
  void seedTeam({
    required String vaultId,
    required String name,
    required TeamRole role,
    String? color,
    String? description,
    TeamPolicies policies = const TeamPolicies(),
    List<TeamMember> members = const [],
    List<TeamLogEntry> log = const [],
    int keyEpoch = 1,
  }) {
    _settings[vaultId] = TeamSettings({
      'vault_name': name,
      'color': color,
      'description': description,
      'policies': policies.toJson(),
    });
    _members[vaultId] = [...members];
    _log[vaultId] = [...log];
    _invitations[vaultId] = [];
    _joinRequests[vaultId] = [];
    _teams[vaultId] = Team(
      vaultId: vaultId,
      name: name,
      color: color,
      teamColor: color,
      description: description,
      policies: policies,
      role: role,
      membersCount: members.length,
      keyEpoch: keyEpoch,
      open: true,
      sortOrder: _teams.length,
    );
    _changed();
  }

  /// An outgoing invitation of [vaultId] (pending until the invitee accepts).
  void seedInvitation(TeamInvitation invitation) {
    (_invitations[invitation.vaultId] ??= []).add(invitation);
    _changed();
  }

  /// An accepted invitation waiting for this admin's code comparison.
  void seedJoinRequest(TeamJoinRequest request) {
    (_joinRequests[request.vaultId] ??= []).add(request);
    _changed();
  }

  /// An invitation addressed to this account.
  void seedIncoming(TeamInvitation invitation) {
    _incoming.add(invitation);
    _changed();
  }

  Team _copy(
    Team t, {
    String? name,
    String? color,
    bool clearColor = false,
    String? teamColor,
    String? localColor,
    bool setLocalColor = false,
    String? description,
    TeamPolicies? policies,
    TeamRole? role,
    int? membersCount,
    int? keyEpoch,
    int? sortOrder,
    bool? hidden,
  }) {
    final team = clearColor ? null : (teamColor ?? t.teamColor);
    final local = setLocalColor ? localColor : t.localColor;
    return Team(
      vaultId: t.vaultId,
      name: name ?? t.name,
      color: color ?? local ?? team,
      teamColor: team,
      localColor: local,
      icon: t.icon,
      description: description ?? t.description,
      policies: policies ?? t.policies,
      role: role ?? t.role,
      membersCount: membersCount ?? t.membersCount,
      keyEpoch: keyEpoch ?? t.keyEpoch,
      warning: t.warning,
      offline: t.offline,
      pending: t.pending,
      open: t.open,
      sortOrder: sortOrder ?? t.sortOrder,
      hidden: hidden ?? t.hidden,
      objectCount: t.objectCount,
    );
  }

  void _appendLog(String vaultId, String op, {String? target, TeamRole? role, int? keyEpoch}) {
    final log = _log[vaultId] ??= [];
    log.add(
      TeamLogEntry(
        seq: log.length + 1,
        op: op,
        actorEmail: selfEmail,
        issuedAt: _clock().toUtc(),
        targetEmail: target,
        role: role,
        keyEpoch: keyEpoch,
      ),
    );
  }

  Team _rotate(String vaultId) {
    final t = _require(vaultId);
    final next = _copy(t, keyEpoch: t.keyEpoch + 1);
    _teams[vaultId] = next;
    _appendLog(vaultId, 'rotate_key', keyEpoch: next.keyEpoch);
    return next;
  }

  @override
  Stream<void> get changes => _changes.stream;

  @override
  Future<bool> supported() async => true;

  @override
  Future<List<Team>> list() async => _teams.values.toList()..sort((a, b) => a.sortOrder.compareTo(b.sortOrder));

  @override
  Future<List<Team>> refresh() => list();

  @override
  Future<Map<String, String>> objectVaults() async => Map.unmodifiable(objects);

  @override
  Future<Team> create({required String name, String? color, String? icon, String? description}) async {
    final vaultId = 'team-mock-${++_next}';
    final now = _clock().toUtc();
    seedTeam(
      vaultId: vaultId,
      name: name,
      role: TeamRole.owner,
      color: color,
      description: description,
      members: [TeamMember(userId: 'self', email: selfEmail, role: TeamRole.owner, isSelf: true, joinedAt: now)],
      log: [TeamLogEntry(seq: 1, op: 'genesis', actorEmail: selfEmail, issuedAt: now)],
    );
    return _teams[vaultId]!;
  }

  @override
  Future<List<TeamMember>> members(String vaultId) async => List.unmodifiable(_members[vaultId] ?? const []);

  @override
  Future<List<TeamLogEntry>> log(String vaultId) async =>
      (_log[vaultId] ?? const <TeamLogEntry>[]).reversed.toList(growable: false);

  @override
  Future<TeamSettings> settings(String vaultId) async {
    _require(vaultId);
    return _settings[vaultId]!;
  }

  @override
  Future<TeamSettings> saveSettings(String vaultId, TeamSettings settings) async {
    final t = _require(vaultId);
    if (!t.isAdmin) throw const AppException(AppErrorCode.unsupported, 'only admins change team settings');
    _settings[vaultId] = settings;
    _teams[vaultId] = _copy(
      t,
      name: settings.name,
      teamColor: settings.color,
      clearColor: settings.color == null,
      description: settings.description,
      policies: settings.policies,
    );
    _changed();
    return settings;
  }

  @override
  Future<Team> setPreferences(
    String vaultId, {
    String? localColor,
    required int sortOrder,
    required bool hidden,
  }) async {
    final t = _copy(
      _require(vaultId),
      localColor: localColor,
      setLocalColor: true,
      sortOrder: sortOrder,
      hidden: hidden,
    );
    _teams[vaultId] = t;
    _changed();
    return t;
  }

  @override
  Future<void> invite(String vaultId, String email, TeamRole role, {String? note}) async {
    final t = _require(vaultId);
    if (!t.isAdmin) throw const AppException(AppErrorCode.unsupported, 'only admins invite');
    final now = _clock().toUtc();
    seedInvitation(
      TeamInvitation(
        invitationId: 'invitation-${++_next}',
        vaultId: vaultId,
        inviterEmail: selfEmail,
        inviteeEmail: email,
        role: role,
        status: 'pending',
        note: note,
        createdAt: now,
        expiresAt: now.add(const Duration(days: 7)),
      ),
    );
  }

  @override
  Future<List<TeamInvitation>> invitations(String vaultId) async =>
      List.unmodifiable(_invitations[vaultId] ?? const []);

  @override
  Future<List<TeamInvitation>> incomingInvitations() async =>
      _incoming.where((i) => i.status == 'pending').toList(growable: false);

  @override
  Future<TeamJoin> accept(String invitationId) async {
    final i = _incoming.indexWhere((x) => x.invitationId == invitationId);
    if (i < 0) {
      throw AppException(AppErrorCode.notFound, 'invitation $invitationId not found'); // l10n-ignore: diagnostic
    }
    final inv = _incoming.removeAt(i);
    _changed();
    // Synthetic comparison code (6 groups of 5 digits); not derived from keys.
    return TeamJoin(
      invitationId: inv.invitationId,
      vaultId: inv.vaultId,
      inviterEmail: inv.inviterEmail,
      code: '48291 07536 91802 36415 70284 15937',
    );
  }

  @override
  Future<void> closeInvitation(String invitationId) async {
    _incoming.removeWhere((i) => i.invitationId == invitationId);
    for (final list in _invitations.values) {
      list.removeWhere((i) => i.invitationId == invitationId);
    }
    for (final list in _joinRequests.values) {
      list.removeWhere((r) => r.invitationId == invitationId);
    }
    _changed();
  }

  @override
  Future<List<TeamJoinRequest>> joinRequests(String vaultId) async =>
      List.unmodifiable(_joinRequests[vaultId] ?? const []);

  @override
  Future<Team> confirmMember(String vaultId, String invitationId) async {
    final t = _require(vaultId);
    final requests = _joinRequests[vaultId] ?? [];
    final r = requests.where((x) => x.invitationId == invitationId).firstOrNull;
    if (r == null) {
      throw AppException(AppErrorCode.notFound, 'join request $invitationId not found'); // l10n-ignore: diagnostic
    }
    requests.remove(r);
    _invitations[vaultId]?.removeWhere((i) => i.invitationId == invitationId);
    final members = _members[vaultId] ??= [];
    members.add(TeamMember(userId: 'user-${++_next}', email: r.inviteeEmail, role: r.role, joinedAt: _clock().toUtc()));
    _appendLog(vaultId, 'add_member', target: r.inviteeEmail, role: r.role);
    final next = _copy(t, membersCount: members.length);
    _teams[vaultId] = next;
    _changed();
    return next;
  }

  TeamMember _member(String vaultId, String userId) =>
      (_members[vaultId] ?? const <TeamMember>[]).where((m) => m.userId == userId).firstOrNull ??
      (throw AppException(AppErrorCode.notFound, 'member $userId not found')); // l10n-ignore: diagnostic

  void _replaceMember(String vaultId, TeamMember m, TeamRole role) {
    final members = _members[vaultId]!;
    members[members.indexOf(m)] = TeamMember(
      userId: m.userId,
      email: m.email,
      role: role,
      isSelf: m.isSelf,
      joinedAt: m.joinedAt,
    );
  }

  @override
  Future<Team> changeRole(String vaultId, String userId, TeamRole role) async {
    final m = _member(vaultId, userId);
    _replaceMember(vaultId, m, role);
    _appendLog(vaultId, 'change_role', target: m.email, role: role);
    _changed();
    return _require(vaultId);
  }

  @override
  Future<Team> transferOwner(String vaultId, String userId) async {
    final m = _member(vaultId, userId);
    final self = _members[vaultId]!.firstWhere((x) => x.isSelf);
    _replaceMember(vaultId, m, TeamRole.owner);
    _replaceMember(vaultId, self, TeamRole.admin);
    _appendLog(vaultId, 'transfer_owner', target: m.email);
    final next = _copy(_require(vaultId), role: TeamRole.admin);
    _teams[vaultId] = next;
    _changed();
    return next;
  }

  @override
  Future<Team> removeMember(String vaultId, String userId) async {
    final m = _member(vaultId, userId);
    final members = _members[vaultId]!..remove(m);
    _appendLog(vaultId, 'remove_member', target: m.email);
    _teams[vaultId] = _copy(_require(vaultId), membersCount: members.length);
    final next = _rotate(vaultId);
    _changed();
    return next;
  }

  @override
  Future<void> leave(String vaultId) async {
    _require(vaultId);
    _teams.remove(vaultId);
    objects.removeWhere((_, v) => v == vaultId);
    _changed();
  }

  @override
  Future<Team> rotateKey(String vaultId) async {
    final next = _rotate(vaultId);
    _changed();
    return next;
  }

  @override
  Future<String> createGroup(String vaultId, String name) async {
    _require(vaultId);
    final group = Group.create(name);
    await inventory?.saveGroup(group);
    objects[group.id.value] = vaultId;
    _changed();
    return group.id.value;
  }

  /// The mock keeps ids: copies are not duplicated, only attributed.
  @override
  Future<List<String>> copyObjects(List<String> ids, String? targetVaultId) => moveObjects(ids, targetVaultId);

  @override
  Future<List<String>> moveObjects(List<String> ids, String? targetVaultId) async {
    for (final id in ids) {
      if (targetVaultId == null) {
        objects.remove(id);
      } else {
        objects[id] = targetVaultId;
      }
    }
    _changed();
    return ids;
  }
}
