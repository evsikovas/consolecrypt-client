import 'package:consolecrypt/teams/team_models.dart';
import 'package:consolecrypt/teams/team_providers.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

void main() {
  test('team DTO parses roles, colours, warnings and policies', () {
    final t = Team.fromJson({
      'vault_id': 'v1',
      'name': 'Ops',
      'color': '#00aa00',
      'team_color': 'blue',
      'local_color': '#00aa00',
      'policies': {'block_ai_context': true, 'clipboard_clear_seconds': 30},
      'role': 'viewer',
      'members_count': 3,
      'key_epoch': 2,
      'open': true,
    });
    expect(t.role, TeamRole.viewer);
    expect(t.canEdit, isFalse);
    expect(t.isAdmin, isFalse);
    expect(t.policies.blockAiContext, isTrue);
    expect(t.policies.clipboardClearSeconds, 30);
    expect(teamColor(t.color), const Color(0xFF00AA00));
    expect(teamColor(t.teamColor), teamPalette['blue']);
    expect(teamColor('nope'), isNull);

    final paused = Team.fromJson({'vault_id': 'v2', 'role': 'owner', 'warning': 'invalid_log'});
    expect(paused.canEdit, isFalse, reason: 'a paused team is read only');
    expect(paused.isAdmin, isFalse);
  });

  test('team settings keep unknown fields and clear the colour', () {
    const s = TeamSettings({'id': 'x', 'vault_name': 'Ops', 'color': 'red', 'terminal_history_mode': 'local_only'});
    final edited = s.copyWith(name: 'Ops 2', clearColor: true, policies: const TeamPolicies(productionByDefault: true));
    expect(edited.raw['terminal_history_mode'], 'local_only');
    expect(edited.name, 'Ops 2');
    expect(edited.color, isNull);
    expect(edited.policies.productionByDefault, isTrue);
  });
}
