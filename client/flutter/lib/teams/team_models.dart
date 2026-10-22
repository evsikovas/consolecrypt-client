/// Teams (protocol 1.9): team vaults shared by several accounts. Mirrors
/// `cc_app_core::team_dto` (JSON, snake_case).
library;

typedef TeamJson = Map<String, Object?>;

enum TeamRole {
  owner('owner'),
  admin('admin'),
  editor('editor'),
  viewer('viewer');

  const TeamRole(this.wireName);
  final String wireName;

  static TeamRole? fromWire(Object? v) => switch (v) {
    'owner' => TeamRole.owner,
    'admin' => TeamRole.admin,
    'editor' => TeamRole.editor,
    'viewer' => TeamRole.viewer,
    _ => null,
  };

  bool get canEdit => this != TeamRole.viewer;
  bool get isAdmin => this == TeamRole.owner || this == TeamRole.admin;
}

/// Team policies the core enforces for the team's resources.
final class TeamPolicies {
  const TeamPolicies({
    this.productionByDefault = false,
    this.blockAiContext = false,
    this.clipboardClearSeconds = 0,
    this.confirmRiskyCommands = false,
  });

  factory TeamPolicies.fromJson(Object? json) {
    final j = json is Map ? json.cast<String, Object?>() : const <String, Object?>{};
    return TeamPolicies(
      productionByDefault: j['production_by_default'] == true,
      blockAiContext: j['block_ai_context'] == true,
      clipboardClearSeconds: (j['clipboard_clear_seconds'] as num? ?? 0).toInt(),
      confirmRiskyCommands: j['confirm_risky_commands'] == true,
    );
  }

  final bool productionByDefault;
  final bool blockAiContext;
  final int clipboardClearSeconds;
  final bool confirmRiskyCommands;

  TeamJson toJson() => {
    'production_by_default': productionByDefault,
    'block_ai_context': blockAiContext,
    'clipboard_clear_seconds': clipboardClearSeconds,
    'confirm_risky_commands': confirmRiskyCommands,
  };

  TeamPolicies copyWith({
    bool? productionByDefault,
    bool? blockAiContext,
    int? clipboardClearSeconds,
    bool? confirmRiskyCommands,
  }) => TeamPolicies(
    productionByDefault: productionByDefault ?? this.productionByDefault,
    blockAiContext: blockAiContext ?? this.blockAiContext,
    clipboardClearSeconds: clipboardClearSeconds ?? this.clipboardClearSeconds,
    confirmRiskyCommands: confirmRiskyCommands ?? this.confirmRiskyCommands,
  );
}

final class Team {
  const Team({
    required this.vaultId,
    required this.name,
    this.color,
    this.teamColor,
    this.localColor,
    this.icon,
    this.description,
    this.policies = const TeamPolicies(),
    this.role,
    this.membersCount = 0,
    this.keyEpoch = 0,
    this.rotationRequired = false,
    this.warning,
    this.offline = false,
    this.pending = false,
    this.open = false,
    this.sortOrder = 0,
    this.hidden = false,
    this.objectCount = 0,
  });

  factory Team.fromJson(TeamJson j) => Team(
    vaultId: j['vault_id']! as String,
    name: j['name'] as String? ?? '',
    color: j['color'] as String?,
    teamColor: j['team_color'] as String?,
    localColor: j['local_color'] as String?,
    icon: j['icon'] as String?,
    description: j['description'] as String?,
    policies: TeamPolicies.fromJson(j['policies']),
    role: TeamRole.fromWire(j['role']),
    membersCount: (j['members_count'] as num? ?? 0).toInt(),
    keyEpoch: (j['key_epoch'] as num? ?? 0).toInt(),
    rotationRequired: j['rotation_required'] == true,
    warning: j['warning'] as String?,
    offline: j['offline'] == true,
    pending: j['pending'] == true,
    open: j['open'] == true,
    sortOrder: (j['sort_order'] as num? ?? 0).toInt(),
    hidden: j['hidden'] == true,
    objectCount: (j['object_count'] as num? ?? 0).toInt(),
  );

  final String vaultId;
  final String name;

  /// Colour to display (personal override, else the team colour).
  final String? color;
  final String? teamColor;
  final String? localColor;
  final String? icon;
  final String? description;
  final TeamPolicies policies;
  final TeamRole? role;
  final int membersCount;
  final int keyEpoch;
  final bool rotationRequired;

  /// `invalid_log` | `log_rollback`: paused, read-only.
  final String? warning;
  final bool offline;
  final bool pending;
  final bool open;
  final int sortOrder;
  final bool hidden;
  final int objectCount;

  bool get canEdit => warning == null && (role?.canEdit ?? false);
  bool get isAdmin => warning == null && (role?.isAdmin ?? false);
}

final class TeamMember {
  const TeamMember({required this.userId, required this.email, required this.role, this.isSelf = false, this.joinedAt});

  factory TeamMember.fromJson(TeamJson j) => TeamMember(
    userId: j['user_id']! as String,
    email: j['email'] as String? ?? '',
    role: TeamRole.fromWire(j['role']) ?? TeamRole.viewer,
    isSelf: j['is_self'] == true,
    joinedAt: _date(j['joined_at_ms']),
  );

  final String userId;
  final String email;
  final TeamRole role;
  final bool isSelf;
  final DateTime? joinedAt;
}

final class TeamInvitation {
  const TeamInvitation({
    required this.invitationId,
    required this.vaultId,
    required this.inviterEmail,
    required this.inviteeEmail,
    required this.role,
    required this.status,
    this.note,
    this.createdAt,
    this.expiresAt,
  });

  factory TeamInvitation.fromJson(TeamJson j) => TeamInvitation(
    invitationId: j['invitation_id']! as String,
    vaultId: j['vault_id']! as String,
    inviterEmail: j['inviter_email'] as String? ?? '',
    inviteeEmail: j['invitee_email'] as String? ?? '',
    role: TeamRole.fromWire(j['role']) ?? TeamRole.viewer,
    status: j['status'] as String? ?? '',
    note: j['note'] as String?,
    createdAt: _date(j['created_at_ms']),
    expiresAt: _date(j['expires_at_ms']),
  );

  final String invitationId;
  final String vaultId;
  final String inviterEmail;
  final String inviteeEmail;
  final TeamRole role;

  /// `pending` | `accepted` | `completed` | `declined` | `cancelled` | `expired`.
  final String status;

  /// Plain text the server can read.
  final String? note;
  final DateTime? createdAt;
  final DateTime? expiresAt;
}

/// An accepted invitation waiting for this admin's code comparison.
final class TeamJoinRequest {
  const TeamJoinRequest({
    required this.invitationId,
    required this.vaultId,
    required this.inviteeEmail,
    required this.role,
    required this.code,
  });

  factory TeamJoinRequest.fromJson(TeamJson j) => TeamJoinRequest(
    invitationId: j['invitation_id']! as String,
    vaultId: j['vault_id']! as String,
    inviteeEmail: j['invitee_email'] as String? ?? '',
    role: TeamRole.fromWire(j['role']) ?? TeamRole.viewer,
    code: j['code'] as String? ?? '',
  );

  final String invitationId;
  final String vaultId;
  final String inviteeEmail;
  final TeamRole role;
  final String code;
}

/// After accepting: the code the invitee reads to the admin.
final class TeamJoin {
  const TeamJoin({required this.invitationId, required this.vaultId, required this.inviterEmail, required this.code});

  factory TeamJoin.fromJson(TeamJson j) => TeamJoin(
    invitationId: j['invitation_id']! as String,
    vaultId: j['vault_id']! as String,
    inviterEmail: j['inviter_email'] as String? ?? '',
    code: j['code'] as String? ?? '',
  );

  final String invitationId;
  final String vaultId;
  final String inviterEmail;
  final String code;
}

final class TeamLogEntry {
  const TeamLogEntry({
    required this.seq,
    required this.op,
    required this.actorEmail,
    this.issuedAt,
    this.targetEmail,
    this.role,
    this.keyEpoch,
  });

  factory TeamLogEntry.fromJson(TeamJson j) => TeamLogEntry(
    seq: (j['seq'] as num? ?? 0).toInt(),
    op: j['op'] as String? ?? '',
    actorEmail: j['actor_email'] as String? ?? '',
    issuedAt: _date(j['issued_at_ms']),
    targetEmail: j['target_email'] as String?,
    role: TeamRole.fromWire(j['role']),
    keyEpoch: (j['key_epoch'] as num?)?.toInt(),
  );

  final int seq;

  /// `genesis` | `add_member` | `change_role` | `remove_member` | `leave` |
  /// `transfer_owner` | `rotate_key`.
  final String op;
  final String actorEmail;
  final DateTime? issuedAt;
  final String? targetEmail;
  final TeamRole? role;
  final int? keyEpoch;
}

/// Team settings (the team vault's `VaultSettingsDto`); the raw JSON is
/// kept so fields the UI does not edit survive a save.
final class TeamSettings {
  const TeamSettings(this.raw);

  final TeamJson raw;

  String get name => raw['vault_name'] as String? ?? '';
  String? get color => raw['color'] as String?;
  String? get icon => raw['icon'] as String?;
  String? get description => raw['description'] as String?;
  TeamPolicies get policies => TeamPolicies.fromJson(raw['policies']);

  TeamSettings copyWith({
    String? name,
    String? color,
    bool clearColor = false,
    String? description,
    TeamPolicies? policies,
  }) => TeamSettings({
    ...raw,
    'vault_name': ?name,
    if (clearColor) 'color': null else 'color': ?color,
    'description': ?description,
    if (policies != null) 'policies': policies.toJson(),
  });
}

DateTime? _date(Object? ms) {
  final v = (ms as num?)?.toInt() ?? 0;
  return v <= 0 ? null : DateTime.fromMillisecondsSinceEpoch(v, isUtc: true);
}
