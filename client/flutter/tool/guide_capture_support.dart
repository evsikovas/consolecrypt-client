// Website guide fixtures only. These adapters render production widgets but
// perform no cryptography, networking or sharing authorization.
import 'dart:convert';

import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/mock/mock_team_service.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/database_sharing_service.dart';
import 'package:consolecrypt/core/services/enrollment_service.dart';
import 'package:consolecrypt/core/services/file_dialog_service.dart';
import 'package:consolecrypt/core/services/rdp_sharing_service.dart';
import 'package:consolecrypt/core/services/sharing_service.dart';
import 'package:consolecrypt/settings/cli_installer.dart';
import 'package:consolecrypt/sharing/enrollment_models.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';
import 'package:consolecrypt/teams/team_models.dart';

import 'key_trust_capture_support.dart';

const guideScreens = [
  'welcome',
  'hosts',
  'inventory-groups',
  'inventory-production',
  'known-hosts',
  'terminal-menu',
  'terminal-selection-scroll',
  'terminal-tabs-overflow',
  'terminal-local-picker',
  'terminal-local-tabs',
  'terminal-wsl-picker',
  'host-palette',
  'local-terminal-palette',
  'host-import',
  'about-license',
  'host-menu',
  'host-groups',
  'connection-picker',
  'rdp-host-picker',
  'sftp-preview',
  'sftp-settings',
  'sftp-open-as-text',
  'snippet-packages',
  'snippet-catalog',
  'workspace-ai-selection',
  'workspace-snippets-expanded',
  'workspace-ai-expanded',
  'local-ai-install',
  'local-ai-progress',
  'local-ai-ready',
  'local-ai-catalog',
  'appearance',
  'vault-settings',
  'security-settings',
  'security-settings-windows',
  'panel-mode-settings',
  'backups',
  'updates',
  'sync',
  'credentials',
  'devices',
  'tunnels',
  'sharing-main',
  'sharing-deleted',
  'sharing-recipient',
  'sharing-roles',
  'sharing-secret',
  'enrollment-owner',
  'db-connections',
  'db-connections-list',
  'db-connection-form',
  'db-workspace',
  'db-explain',
  'db-ai',
  'db-shared',
  'db-share',
  'db-prod-unlock',
  'db-prod-write',
  'db-row-selection',
  'db-ddl',
  'db-query-review',
  'db-preview-tab',
  'db-details',
  ...guideDb092Screens,
  ...guideKeyTrustScreens,
  'step-up-biometric',
  'clipboard-settings',
  'docker-hosts',
  'docker-containers',
  'docker-details',
  'docker-json',
  'docker-volumes',
  'docker-compose-menu',
  'docker-compose-yaml',
  'docker-database',
  'docker-forwards',
  'notes-object',
  'notes-tasks',
  'notes-editor',
  'notes-menu',
  'notes-editor-save',
  'notes-host',
  'notes-not-linked',
  'enterprise-login-sso',
  'enterprise-login-ldap',
  'enterprise-settings',
  'enterprise-disabled',
  'teams-settings',
  'teams-create',
  'teams-join-code',
  'teams-members',
  'teams-invite',
  'teams-compare',
  'teams-policies',
  'teams-history',
  'teams-hosts',
  'teams-hosts-section',
  'teams-host-menu',
  'teams-terminal',
  'teams-run-confirm',
  'kube-import',
  'kube-logs',
  'kube-shell',
  'kube-forward',
  'kube-yaml',
  'kube-rollout',
  'kube-ai',
  ...guideExtraScreens,
  ...guide093Screens,
];

/// 0.9.3 additions, handled by `_release093Screen` in design_preview.dart.
const guide093Screens = ['terminal-split', 'sharing-rdp', 'settings-cli', 'db-cassandra-trace'];

/// Settings → Command line as in an installed app: the command is bundled
/// and not yet installed. Nothing is linked or changed.
final class GuideCliInstaller extends CliInstaller {
  @override
  Future<CliCommandState> state() async => const CliCommandState(
    bundled: '/Applications/ConsoleCrypt.app/Contents/Helpers/consolecrypt-cli',
    installed: false,
    location: '/usr/local/bin/consolecrypt-cli',
  );
}

/// 0.9.2 database cases handled by `_database092Screen` in design_preview.dart.
const guideDb092Screens = [
  'db-sqlite-connection',
  'db-cassandra-connection',
  'db-sqlite-tree',
  'db-designer',
  'db-designer-save',
  'db-result-toolbar',
  'db-result-record',
  'db-csv-read',
  'db-csv-map',
  'db-snippets-picker',
];

/// CSV import picks a demo path; the mock service never opens it.
final class GuideCsvFiles implements FileDialogService {
  @override
  Future<String?> chooseOpenFile({List<String> extensions = const []}) async =>
      '/Users/demo/Downloads/orders-2026-10.csv';

  @override
  Future<String?> chooseSaveFile({required String suggestedName, List<String> extensions = const []}) async => null;

  @override
  Future<bool> finishSaveFile(String path) async => false;

  @override
  Future<String?> chooseDirectory() async => null;

  @override
  Future<String?> chooseApplication({required String label, required String confirmButtonText}) async => null;
}

/// Cases handled by `_extraScreen` in design_preview.dart.
const guideExtraScreens = [
  'login-standard',
  'restore-backup',
  'unlock',
  'security-key-pin',
  'security-key-touch',
  'recovery',
  'settings-overview',
  'account-password',
  'disconnect',
  'change-passphrase',
  'recovery-kit-new',
  'sync-popover',
  'profiles-menu',
  'device-approve',
  'host-editor',
  'host-editor-jump',
  'host-editor-security',
  'host-effective',
  'ssh-config-import',
  'group-editor',
  'jump-profiles',
  'credential-chooser',
  'credential-generate',
  'tunnel-editor',
  'snippets-editor',
  'snippets-variables',
  'snippets-run-confirm',
  'terminal-tab-menu',
  'terminal-settings',
  'shortcuts-settings',
  'ai-providers',
  'ai-provider-dialog',
  'sftp-actions',
  'sftp-get-info',
  'sftp-local-pane',
  'sftp-transfers',
  'share-accept',
  'share-access',
];

// Explicit synthetic profiles. Capture fixtures never discover or spawn a
// native shell, read a user's home directory, or enumerate real WSL installs.
const guideMacShellProfiles = [LocalShellProfile(id: 'macos:zsh', kind: 'zsh', label: 'zsh', shell: 'zsh')];
const guideWindowsShellProfiles = [
  LocalShellProfile(id: 'windows:powershell', kind: 'powershell', label: 'Windows PowerShell', shell: 'powershell'),
  LocalShellProfile(id: 'windows:cmd', kind: 'cmd', label: 'Command Prompt', shell: 'cmd'),
  LocalShellProfile(id: 'wsl:Ubuntu', kind: 'wsl', label: 'Ubuntu', shell: 'sh', distribution: 'Ubuntu'),
  LocalShellProfile(id: 'wsl:Debian', kind: 'wsl', label: 'Debian', shell: 'sh', distribution: 'Debian'),
];

/// Keep the seeded topology while replacing addresses with reserved domains.
/// Credential values stay inside MockBackend and are never revealed here.
Future<void> prepareGuideInventory(MockBackend backend, String screen) async {
  final data = backend.cloud.activeData!;
  final addresses = {for (final host in data.hosts) host.address: '${host.name}.example.test'};
  for (var i = 0; i < data.hosts.length; i++) {
    final host = data.hosts[i];
    data.hosts[i] = Host(
      id: host.id,
      name: host.name,
      address: addresses[host.address]!,
      createdAt: host.createdAt,
      updatedAt: host.updatedAt,
      port: host.port,
      username: host.username,
      credentialId: host.credentialId,
      groupId: host.groupId,
      jumpChain: host.jumpChain,
      jumpProfileId: host.jumpProfileId,
      proxyId: host.proxyId,
      hostKeyPolicy: host.hostKeyPolicy,
      backend: host.backend,
      keepaliveSecs: host.keepaliveSecs,
      agentForwarding: host.agentForwarding,
      tags: host.tags,
      notes: host.notes,
      metadata: host.metadata,
    );
  }
  for (var i = 0; i < data.knownHosts.length; i++) {
    final old = data.knownHosts[i];
    var pattern = old.hostPattern;
    for (final entry in addresses.entries) {
      pattern = pattern.replaceAll(entry.key, entry.value);
    }
    data.knownHosts[i] = KnownHost(
      id: old.id,
      hostPattern: pattern,
      keyType: old.keyType,
      publicKey: old.publicKey,
      fingerprintSha256: old.fingerprintSha256,
      source: old.source,
      addedAt: old.addedAt,
      updatedAt: old.updatedAt,
    );
  }
  for (var i = 0; i < data.tunnels.length; i++) {
    final old = data.tunnels[i];
    data.tunnels[i] = Tunnel(
      id: old.id,
      name: old.name,
      kind: old.kind,
      hostId: old.hostId,
      bindHost: 'localhost',
      bindPort: old.bindPort,
      targetHost: old.targetHost == null ? null : 'service.example.test',
      targetPort: old.targetPort,
      autoStart: false,
      createdAt: old.createdAt,
      updatedAt: old.updatedAt,
    );
  }
  backend.inventory.onDataChanged(data);
  backend.tunnels.onDataChanged(data);
  await backend.profiles.rename(backend.profiles.currentProfiles.activeId!, 'you@example.test');
  if (screen != 'devices' && screen != 'device-approve') {
    for (final request in backend.devices.currentSnapshot.pendingRequests.toList()) {
      await backend.devices.reject(request.requestId);
    }
  }
  if (screen == 'backups') {
    await backend.backups.exportBackup(path: '/Users/demo/Backups/Workspace-2026-10-01.ccbackup');
  }
}

// Public comparison examples, not real keys, accounts or pairing transcripts.
const guideOwner = SharingIdentity(
  instance: 'guide-instance',
  userId: 'guide-owner',
  deviceId: 'Owner Mac',
  encryptionKey: 'synthetic-public-x-owner',
  signingKey: 'synthetic-public-ed-owner',
  code: 'CC1:0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF',
);
const guideRecipient = SharingIdentity(
  instance: 'guide-instance',
  userId: 'guide-colleague',
  deviceId: 'Colleague laptop',
  encryptionKey: 'synthetic-public-x-recipient',
  signingKey: 'synthetic-public-ed-recipient',
  code: 'CC1:89ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF01234567',
);
const guideUnexpectedMetadata = 'UNEXPECTED_FIELD_MUST_NOT_RENDER';

/// Twelve synthetic groups of a safety number (not computed from keys).
const guideSafetyNumberGroups = [
  '30517',
  '88214',
  '06639',
  '41902',
  '57260',
  '19384',
  '72045',
  '63118',
  '95471',
  '20836',
  '48593',
  '11760',
];

final class GuideSharingService implements SharingService, DatabaseSharingService, RdpSharingService {
  GuideSharingService({this.databases = false, this.banner = const []});

  /// Databases screens list shared database connections as well.
  final bool databases;

  /// Unverified recipients of existing secrets (the one-time banner).
  final List<PeerPin> banner;

  // Key pins (0.9.2): choosing pins on first use, verifying marks the pin
  // verified. Synthetic: no key is compared or stored.
  PeerPin _pin(SharingIdentity identity, PeerTrust status, String? email) => PeerPin(
    verificationId: 'guide-pin-${identity.deviceId}',
    deviceId: identity.deviceId,
    userId: identity.userId,
    status: status,
    verifiedAt: status == PeerTrust.verified ? DateTime(2026, 10, 23, 11, 40) : null,
    email: email,
  );
  @override
  Future<PeerPin> choosePeer(SharingIdentity identity, {String? email}) async =>
      _pin(identity, PeerTrust.chosen, email);
  @override
  Future<SafetyNumber> safetyNumber(SharingIdentity identity) async => SafetyNumber(
    groups: guideSafetyNumberGroups,
    ownQrPayload: 'consolecrypt-peer:v1:synthetic-guide-example',
    trust: identity.trust,
  );
  @override
  Future<PeerPin> verifyPeer(SharingIdentity identity, String method, String input, {String? email}) async =>
      _pin(identity, PeerTrust.verified, email);
  @override
  Future<List<PeerPin>> verificationBanner() async => banner;
  @override
  Future<void> dismissVerificationBanner() async {}
  @override
  Future<SharingPolicy> policy() async => const SharingPolicy();
  @override
  Future<SharingPolicy> setPolicy(SharingPolicy policy) async => policy;

  static Map<String, Object?> _db(
    String name,
    String engine,
    String host,
    int port,
    String database, {
    required bool production,
    String routeKind = 'direct',
  }) => {
    'name': name,
    'engine': engine,
    'host': host,
    'port': port,
    'database': database,
    'username': 'reporting',
    'tls_mode': 'verify',
    'production': production,
    'route_kind': routeKind,
    'requires_route': routeKind != 'direct',
    'includes_credentials': false,
    'tags': <String>[],
  };

  SharingItem _dbItem(String id, Map<String, Object?> data, {required bool owned, SharingTrust? trust}) => SharingItem(
    id: 'guide-share-$id',
    itemId: 'guide-item-$id',
    kind: SharingKind.dbConnection,
    ownerUserId: owned ? guideOwner.userId : guideRecipient.userId,
    ownerDeviceId: owned ? guideOwner.deviceId : guideRecipient.deviceId,
    revision: 1,
    epoch: 1,
    owned: owned,
    trust: trust ?? SharingTrust.verified,
    role: SharingRole.reader,
    members: owned ? const [guideOwner, guideRecipient] : const [],
    previewJson: jsonEncode({'kind': 'db_connection', 'data': data}),
  );

  List<SharingItem> get databaseItems => [
    _dbItem(
      'db-orders',
      _db('orders-replica', 'postgresql', 'orders-ro.internal', 5432, 'orders', production: true, routeKind: 'relay'),
      owned: false,
    ),
    _dbItem(
      'db-warehouse',
      _db('warehouse', 'mysql', 'warehouse.internal', 3306, 'stock', production: false),
      owned: false,
      trust: SharingTrust.unverified,
    ),
    _dbItem(
      'db-shop',
      _db('shop-prod', 'postgresql', 'db.shop.internal', 5432, 'shop', production: true, routeKind: 'relay'),
      owned: true,
    ),
  ];

  @override
  Future<DbSharingPreview> previewDbConnection(String connectionId, {bool includeCredentials = false}) async =>
      DbSharingPreview(
        snapshotStamp: 'guide-stamp',
        previewJson: jsonEncode({
          'kind': 'db_connection',
          'data': _db(
            'shop-prod',
            'postgresql',
            'db.shop.internal',
            5432,
            'shop',
            production: true,
            routeKind: 'relay',
          ),
        }),
      );

  SharingItem get ownedHost => SharingItem(
    id: 'guide-share-host',
    itemId: 'guide-item-host',
    kind: SharingKind.host,
    ownerUserId: guideOwner.userId,
    ownerDeviceId: guideOwner.deviceId,
    revision: 3,
    epoch: 2,
    owned: true,
    trust: SharingTrust.verified,
    role: SharingRole.editor,
    members: const [guideOwner, guideRecipient],
    memberRoles: [const SharingGrant(guideRecipient, SharingRole.editor)],
    previewJson: hostPreview,
  );

  String get hostPreview => jsonEncode({
    'kind': 'host',
    'data': {
      'name': 'web-01',
      'address': 'web-01.example.test',
      'port': 22,
      'username': 'you',
      'tags': ['web', 'demo'],
    },
  });

  @override
  Future<SharingStatus> status() async => const SharingStatus(
    enabled: true,
    instance: 'guide-instance',
    identity: guideOwner,
    supportsGroups: true,
    supportsSecrets: true,
    supportsOwnerOnlineEnrollment: true,
    supportsDbConnections: true,
    supportsLeave: true,
    supportsRestore: true,
  );

  /// Owner-deleted items still restorable (fixed dates, masked previews).
  @override
  Future<List<SharingDeletedItem>> deleted() async => [
    SharingDeletedItem(
      id: 'guide-deleted-host',
      kind: SharingKind.host,
      restorableUntil: DateTime(2026, 11, 18),
      recipients: 2,
      previewJson: jsonEncode({
        'kind': 'host',
        'data': {'name': 'staging-web', 'address': 'staging-web.example.test', 'port': 22},
      }),
    ),
    SharingDeletedItem(
      id: 'guide-deleted-snippet',
      kind: SharingKind.snippet,
      restorableUntil: DateTime(2026, 11, 9),
      recipients: 1,
      previewJson: jsonEncode({
        'kind': 'snippet',
        'data': {'name': 'Restart nginx', 'template': 'sudo systemctl restart nginx', 'snippet_type': 'shell'},
      }),
    ),
  ];
  @override
  Future<List<SharingIdentity>> discover(String email) async => const [guideRecipient];

  /// An incoming item and its owner (read-only: accepting is never run).
  @override
  Future<SharingInvitation> inspect(String id) async =>
      SharingInvitation((await list()).firstWhere((i) => i.id == id), guideRecipient);
  @override
  Future<List<SharingOutboxEntry>> outbox() async => const [];
  @override
  Future<List<SharingItem>> list({bool refresh = false}) async => [
    if (databases) ...databaseItems,
    ownedHost,
    SharingItem(
      id: 'guide-share-snippet',
      itemId: 'guide-item-snippet',
      kind: SharingKind.snippet,
      ownerUserId: guideRecipient.userId,
      ownerDeviceId: guideRecipient.deviceId,
      revision: 2,
      epoch: 1,
      owned: false,
      trust: SharingTrust.verified,
      role: SharingRole.reader,
      previewJson: jsonEncode({
        'kind': 'snippet',
        'data': {
          'name': 'Disk health',
          'description': 'Read-only diagnostics',
          'template': 'df -h',
          'snippet_type': 'shell',
        },
      }),
    ),
    SharingItem(
      id: 'guide-share-secret',
      itemId: 'guide-item-secret',
      kind: SharingKind.secret,
      ownerUserId: guideRecipient.userId,
      ownerDeviceId: guideRecipient.deviceId,
      revision: 1,
      epoch: 1,
      owned: false,
      trust: SharingTrust.verified,
      role: SharingRole.reader,
      previewJson: await previewSecret('guide-credential'),
    ),
  ];
  @override
  Future<String> preview(SharingKind kind, String objectId, {bool includeNotes = false}) async => hostPreview;
  @override
  Future<String> previewSecret(String credentialId, {bool passphrase = false}) async => jsonEncode({
    'kind': 'secret',
    'data': {'name': 'Deployment credential', 'secret_kind': 'password', 'value': guideUnexpectedMetadata},
  });

  /// The RDP host card a colleague would receive (FEAT-05): metadata only.
  @override
  Future<RdpSharingPreview> previewRdpHost(
    String hostId, {
    bool includePassword = false,
    bool includeNotes = false,
  }) async => RdpSharingPreview(
    snapshotStamp: 'guide-rdp',
    previewJson: jsonEncode({
      'kind': 'rdp_host',
      'data': {
        'name': 'Windows demo',
        'address': 'windows.example.test',
        'port': 3389,
        'username': 'demo',
        'domain': 'EXAMPLE',
        'tags': ['office'],
        'notes': null,
        'includes_password': includePassword,
      },
    }),
  );

  @override
  dynamic noSuchMethod(Invocation invocation) => throw StateError('Guide capture may not perform sharing mutations.');
}

final class GuideEnrollmentService extends UnavailableEnrollmentService {
  @override
  Future<List<EnrollmentGrant>> grants(String shareId) async => const [];
  @override
  Future<List<EnrollmentRequest>> requests(String shareId) async => const [];
  @override
  Future<List<EnrollmentRequest>> pendingRequests() async => const [];
  @override
  Future<List<EnrollmentRequest>> processAutomatic() async => const [];
}

// Synthetic teams: the signed-in demo account owns "Ops" (the Production
// group and its hosts) and is a viewer of "Support" (the Staging group). One
// colleague has accepted an invitation and waits for the code comparison,
// another invitation is pending, and a third team invites this account.
const guideTeamOps = 'guide-team-ops';
const guideTeamSupport = 'guide-team-support';
const guideJoinRequest = 'guide-join-dmitry';
const guideIncomingInvitation = 'guide-invitation-platform';

MockTeamService guideTeams(MockBackend backend, {required bool en}) {
  const self = 'you@example.test';
  final teams = MockTeamService(selfEmail: self, inventory: backend.inventory);
  final day = DateTime.utc(2026, 10, 12, 9, 30);
  DateTime at(int days, [int hours = 0]) => day.add(Duration(days: days, hours: hours));
  teams.seedTeam(
    vaultId: guideTeamOps,
    name: en ? 'Ops' : 'Эксплуатация',
    role: TeamRole.owner,
    color: 'teal',
    description: en ? 'Production servers and on-call runbooks' : 'Продуктовые серверы и инструкции дежурных',
    policies: const TeamPolicies(productionByDefault: true, blockAiContext: true, confirmRiskyCommands: true),
    keyEpoch: 2,
    members: [
      TeamMember(userId: 'guide-self', email: self, role: TeamRole.owner, isSelf: true, joinedAt: at(0)),
      TeamMember(userId: 'guide-anna', email: 'anna@example.test', role: TeamRole.admin, joinedAt: at(1)),
      TeamMember(userId: 'guide-boris', email: 'boris@example.test', role: TeamRole.editor, joinedAt: at(1, 2)),
      TeamMember(userId: 'guide-vera', email: 'vera@example.test', role: TeamRole.viewer, joinedAt: at(3)),
    ],
    log: [
      TeamLogEntry(seq: 1, op: 'genesis', actorEmail: self, issuedAt: at(0)),
      TeamLogEntry(
        seq: 2,
        op: 'add_member',
        actorEmail: self,
        issuedAt: at(1),
        targetEmail: 'anna@example.test',
        role: TeamRole.editor,
      ),
      TeamLogEntry(
        seq: 3,
        op: 'add_member',
        actorEmail: self,
        issuedAt: at(1, 2),
        targetEmail: 'boris@example.test',
        role: TeamRole.editor,
      ),
      TeamLogEntry(
        seq: 4,
        op: 'change_role',
        actorEmail: self,
        issuedAt: at(2),
        targetEmail: 'anna@example.test',
        role: TeamRole.admin,
      ),
      TeamLogEntry(
        seq: 5,
        op: 'add_member',
        actorEmail: 'anna@example.test',
        issuedAt: at(3),
        targetEmail: 'vera@example.test',
        role: TeamRole.viewer,
      ),
      TeamLogEntry(
        seq: 6,
        op: 'add_member',
        actorEmail: 'anna@example.test',
        issuedAt: at(4),
        targetEmail: 'gleb@example.test',
        role: TeamRole.editor,
      ),
      TeamLogEntry(seq: 7, op: 'remove_member', actorEmail: self, issuedAt: at(6), targetEmail: 'gleb@example.test'),
      TeamLogEntry(seq: 8, op: 'rotate_key', actorEmail: self, issuedAt: at(6), keyEpoch: 2),
    ],
  );
  teams.seedTeam(
    vaultId: guideTeamSupport,
    name: en ? 'Support' : 'Поддержка',
    role: TeamRole.viewer,
    color: 'orange',
    policies: const TeamPolicies(confirmRiskyCommands: true),
    members: [
      TeamMember(userId: 'guide-lead', email: 'lead@example.test', role: TeamRole.owner, joinedAt: at(0)),
      TeamMember(userId: 'guide-self', email: self, role: TeamRole.viewer, isSelf: true, joinedAt: at(5)),
    ],
    log: [
      TeamLogEntry(seq: 1, op: 'genesis', actorEmail: 'lead@example.test', issuedAt: at(0)),
      TeamLogEntry(
        seq: 2,
        op: 'add_member',
        actorEmail: 'lead@example.test',
        issuedAt: at(5),
        targetEmail: self,
        role: TeamRole.viewer,
      ),
    ],
  );
  teams
    ..seedJoinRequest(
      const TeamJoinRequest(
        invitationId: guideJoinRequest,
        vaultId: guideTeamOps,
        inviteeEmail: 'dmitry@example.test',
        role: TeamRole.editor,
        code: '48291 07536 91802 36415 70284 15937',
      ),
    )
    ..seedInvitation(
      TeamInvitation(
        invitationId: 'guide-invitation-kate',
        vaultId: guideTeamOps,
        inviterEmail: self,
        inviteeEmail: 'kate@example.test',
        role: TeamRole.viewer,
        status: 'pending',
        createdAt: at(7),
        expiresAt: at(14),
      ),
    )
    ..seedIncoming(
      TeamInvitation(
        invitationId: guideIncomingInvitation,
        vaultId: 'guide-team-platform',
        inviterEmail: 'lead@example.test',
        inviteeEmail: self,
        role: TeamRole.editor,
        status: 'pending',
        note: en ? 'Access to the test benches for the release' : 'Доступ к стендам на время релиза',
        createdAt: at(8),
        expiresAt: at(15),
      ),
    );
  final data = backend.cloud.activeData!;
  Set<String> subtree(String name) {
    final root = data.groups.firstWhere((g) => g.name == name).id;
    final ids = {root};
    var grew = true;
    while (grew) {
      grew = false;
      for (final g in data.groups) {
        if (g.parentId != null && ids.contains(g.parentId) && ids.add(g.id)) grew = true;
      }
    }
    return {for (final id in ids) id.value};
  }

  for (final (group, vault) in [('Production', guideTeamOps), ('Staging', guideTeamSupport)]) {
    final groups = subtree(group);
    for (final id in groups) {
      teams.objects[id] = vault;
    }
    for (final host in data.hosts) {
      if (host.groupId != null && groups.contains(host.groupId!.value)) teams.objects[host.id.value] = vault;
    }
  }
  return teams;
}
