import 'package:consolecrypt/core/mock/mock_team_service.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/teams/team_models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('mock teams: create, invite, accept, confirm, remove with key rotation', () async {
    final teams = MockTeamService(selfEmail: 'you@example.test', clock: () => DateTime.utc(2026, 10, 1));
    expect(await teams.supported(), isTrue);
    final team = await teams.create(name: 'Ops', color: 'teal');
    expect(team.role, TeamRole.owner);
    expect((await teams.members(team.vaultId)).single.isSelf, isTrue);

    await teams.invite(team.vaultId, 'anna@example.test', TeamRole.editor, note: 'hi');
    final invitation = (await teams.invitations(team.vaultId)).single;
    expect(invitation.status, 'pending');
    expect(invitation.expiresAt, DateTime.utc(2026, 10, 8));

    teams.seedJoinRequest(
      TeamJoinRequest(
        invitationId: invitation.invitationId,
        vaultId: team.vaultId,
        inviteeEmail: 'anna@example.test',
        role: TeamRole.editor,
        code: '11111 22222 33333 44444 55555 66666',
      ),
    );
    final confirmed = await teams.confirmMember(team.vaultId, invitation.invitationId);
    expect(confirmed.membersCount, 2);
    expect(await teams.joinRequests(team.vaultId), isEmpty);
    expect(await teams.invitations(team.vaultId), isEmpty);

    final anna = (await teams.members(team.vaultId)).firstWhere((m) => m.email == 'anna@example.test');
    final after = await teams.removeMember(team.vaultId, anna.userId);
    expect(after.membersCount, 1);
    expect(after.keyEpoch, team.keyEpoch + 1);
    expect((await teams.log(team.vaultId)).map((e) => e.op), ['rotate_key', 'remove_member', 'add_member', 'genesis']);
  });

  test('mock teams: incoming invitation yields a join code; settings and objects', () async {
    final teams = MockTeamService(selfEmail: 'you@example.test');
    teams.seedTeam(vaultId: 't1', name: 'Support', role: TeamRole.viewer, color: 'orange');
    teams.seedIncoming(
      const TeamInvitation(
        invitationId: 'i1',
        vaultId: 't2',
        inviterEmail: 'lead@example.test',
        inviteeEmail: 'you@example.test',
        role: TeamRole.editor,
        status: 'pending',
      ),
    );
    final join = await teams.accept('i1');
    expect(join.code.split(' '), hasLength(6));
    expect(await teams.incomingInvitations(), isEmpty);

    await expectLater(teams.saveSettings('t1', await teams.settings('t1')), throwsA(isA<AppException>()));
    final mine = await teams.setPreferences('t1', localColor: 'red', sortOrder: 0, hidden: false);
    expect(mine.color, 'red');
    expect(mine.teamColor, 'orange');

    await teams.moveObjects(['host-1'], 't1');
    expect(await teams.objectVaults(), {'host-1': 't1'});
    await teams.moveObjects(['host-1'], null);
    expect(await teams.objectVaults(), isEmpty);
  });
}
