// Website guide fixtures only. These adapters render production widgets but
// perform no cryptography, networking or sharing authorization.
import 'dart:convert';

import 'package:consolecrypt/core/mock/mock_backend.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/enrollment_service.dart';
import 'package:consolecrypt/core/services/sharing_service.dart';
import 'package:consolecrypt/sharing/enrollment_models.dart';
import 'package:consolecrypt/sharing/sharing_models.dart';

const guideScreens = [
  'welcome',
  'hosts',
  'inventory-groups',
  'inventory-production',
  'known-hosts',
  'terminal-menu',
  'sftp-preview',
  'sftp-settings',
  'snippet-packages',
  'snippet-catalog',
  'workspace-ai-selection',
  'appearance',
  'vault-settings',
  'security-settings',
  'panel-mode-settings',
  'backups',
  'updates',
  'sync',
  'credentials',
  'devices',
  'tunnels',
  'sharing-main',
  'sharing-recipient',
  'sharing-roles',
  'sharing-secret',
  'enrollment-owner',
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
  if (screen != 'devices') {
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

final class GuideSharingService implements SharingService {
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
    memberRoles: [SharingGrant(guideRecipient, SharingRole.editor, guideRecipient.code)],
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
  );
  @override
  Future<List<SharingIdentity>> discover(String email) async => const [guideRecipient];
  @override
  Future<List<SharingOutboxEntry>> outbox() async => const [];
  @override
  Future<List<SharingItem>> list({bool refresh = false}) async => [
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
