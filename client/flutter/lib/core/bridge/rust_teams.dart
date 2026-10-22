import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/bridge/rust_backend.dart';
import 'package:consolecrypt/core/services/team_service.dart';
import 'package:consolecrypt/src/rust/api/teams.dart' as rs;
import 'package:consolecrypt/teams/team_models.dart';

/// Core events after which teams, members or invitations must be re-read.
const _teamEvents = {
  'teams_changed',
  'team_warning',
  'team_removed',
  'team_invitation_received',
  'team_join_requested',
  'vault_unlocked',
  'vault_locked',
  'lagged',
};

final class RustTeamService implements TeamService {
  RustTeamService(this._hub) {
    _hub.eventListeners.add(_onEvent);
  }

  final RustBackend _hub;
  final _changes = StreamController<void>.broadcast();

  void _onEvent(Json e) {
    final type = e['type'];
    if (_teamEvents.contains(type) || (type == 'objects_changed' && e['kind'] == 'team_membership')) {
      _changes.add(null);
    }
  }

  Future<T> _write<T>(Future<T> Function() call) async {
    final r = await guard(call);
    _changes.add(null);
    return r;
  }

  Future<void> _reloadInventory() => _hub.reloadAll();

  @override
  Stream<void> get changes => _changes.stream;

  @override
  Future<List<Team>> list() async => decodeList(await guard(rs.teamsList)).map(Team.fromJson).toList();

  @override
  Future<List<Team>> refresh() async {
    final teams = decodeList(await guard(rs.teamsRefresh)).map(Team.fromJson).toList();
    await _reloadInventory();
    _changes.add(null);
    return teams;
  }

  @override
  Future<Map<String, String>> objectVaults() async => {
    for (final j in decodeList(await guard(rs.teamsObjectVaults))) j['object_id']! as String: j['vault_id']! as String,
  };

  @override
  Future<Team> create({required String name, String? color, String? icon, String? description}) async {
    final t = Team.fromJson(
      decodeObject(await _write(() => rs.teamsCreate(name: name, color: color, icon: icon, description: description))),
    );
    await _reloadInventory();
    return t;
  }

  @override
  Future<List<TeamMember>> members(String vaultId) async =>
      decodeList(await guard(() => rs.teamsMembers(vaultId: vaultId))).map(TeamMember.fromJson).toList();

  @override
  Future<List<TeamLogEntry>> log(String vaultId) async =>
      decodeList(await guard(() => rs.teamsLog(vaultId: vaultId))).map(TeamLogEntry.fromJson).toList();

  @override
  Future<TeamSettings> settings(String vaultId) async =>
      TeamSettings(decodeObject(await guard(() => rs.teamsSettings(vaultId: vaultId))));

  @override
  Future<TeamSettings> saveSettings(String vaultId, TeamSettings settings) async => TeamSettings(
    decodeObject(await _write(() => rs.teamsSaveSettings(vaultId: vaultId, settingsJson: jsonEncode(settings.raw)))),
  );

  @override
  Future<Team> setPreferences(
    String vaultId, {
    String? localColor,
    required int sortOrder,
    required bool hidden,
  }) async => Team.fromJson(
    decodeObject(
      await _write(
        () => rs.teamsSetPreferences(vaultId: vaultId, localColor: localColor, sortOrder: sortOrder, hidden: hidden),
      ),
    ),
  );

  @override
  Future<void> invite(String vaultId, String email, TeamRole role, {String? note}) =>
      _write(() => rs.teamsInvite(vaultId: vaultId, email: email, roleName: role.wireName, note: note));

  @override
  Future<List<TeamInvitation>> invitations(String vaultId) async =>
      decodeList(await guard(() => rs.teamsInvitations(vaultId: vaultId))).map(TeamInvitation.fromJson).toList();

  @override
  Future<List<TeamInvitation>> incomingInvitations() async =>
      decodeList(await guard(rs.teamsIncomingInvitations)).map(TeamInvitation.fromJson).toList();

  @override
  Future<TeamJoin> accept(String invitationId) async =>
      TeamJoin.fromJson(decodeObject(await _write(() => rs.teamsAcceptInvitation(invitationId: invitationId))));

  @override
  Future<void> closeInvitation(String invitationId) =>
      _write(() => rs.teamsCloseInvitation(invitationId: invitationId));

  @override
  Future<List<TeamJoinRequest>> joinRequests(String vaultId) async =>
      decodeList(await guard(() => rs.teamsJoinRequests(vaultId: vaultId))).map(TeamJoinRequest.fromJson).toList();

  @override
  Future<Team> confirmMember(String vaultId, String invitationId) async => Team.fromJson(
    decodeObject(await _write(() => rs.teamsConfirmMember(vaultId: vaultId, invitationId: invitationId))),
  );

  @override
  Future<Team> changeRole(String vaultId, String userId, TeamRole role) async => Team.fromJson(
    decodeObject(await _write(() => rs.teamsChangeRole(vaultId: vaultId, userId: userId, roleName: role.wireName))),
  );

  @override
  Future<Team> transferOwner(String vaultId, String userId) async =>
      Team.fromJson(decodeObject(await _write(() => rs.teamsTransferOwner(vaultId: vaultId, userId: userId))));

  @override
  Future<Team> removeMember(String vaultId, String userId) async =>
      Team.fromJson(decodeObject(await _write(() => rs.teamsRemoveMember(vaultId: vaultId, userId: userId))));

  @override
  Future<void> leave(String vaultId) async {
    await _write(() => rs.teamsLeave(vaultId: vaultId));
    await _reloadInventory();
  }

  @override
  Future<Team> rotateKey(String vaultId) async =>
      Team.fromJson(decodeObject(await _write(() => rs.teamsRotateKey(vaultId: vaultId))));

  @override
  Future<String> createGroup(String vaultId, String name) async {
    final g = decodeObject(
      await _write(
        () => rs.teamsCreateGroup(
          vaultId: vaultId,
          groupJson: jsonEncode({
            'id': '',
            'name': name,
            'parent_id': null,
            'inherited_username': null,
            'inherited_port': null,
            'inherited_credential_id': null,
            'inherited_jump_profile_id': null,
            'tags': <String>[],
            'created_at_ms': 0,
            'updated_at_ms': 0,
          }),
        ),
      ),
    );
    await _hub.reload({'group'});
    return g['id']! as String;
  }

  @override
  Future<List<String>> copyObjects(List<String> ids, String? targetVaultId) async {
    final r = decodeStringList(await _write(() => rs.teamsCopyObjects(ids: ids, targetVaultId: targetVaultId)));
    await _reloadInventory();
    return r;
  }

  @override
  Future<List<String>> moveObjects(List<String> ids, String? targetVaultId) async {
    final r = decodeStringList(await _write(() => rs.teamsMoveObjects(ids: ids, targetVaultId: targetVaultId)));
    await _reloadInventory();
    return r;
  }
}
