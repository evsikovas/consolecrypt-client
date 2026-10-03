// Pure-Dart tests of the FRB adapter layer (lib/core/bridge/mapping.dart):
// app-core DTO JSON ↔ models and AppError codes → AppException. No native
// library is loaded here; the real core is covered by integration_test/.
import 'package:consolecrypt/core/bridge/effective_config.dart';
import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/src/rust/api/error.dart';
import 'package:flutter_test/flutter_test.dart';

BridgeError _e(String code, [Map<String, String> details = const {}, String? reason]) =>
    BridgeError(code: code, message: 'diag $code', reason: reason, details: details);

void main() {
  test('default editor persists locally, survives unrelated edits and resets explicitly', () {
    for (final editor in [
      const AppRef(AppRefKind.path, '/Applications/Visual Studio Code.app'),
      const AppRef(AppRefKind.bundleId, 'dev.zed.Zed'),
      const AppRef(AppRefKind.name, 'code'),
    ]) {
      final value = const LocalSettings().copyWith(sftpDefaultEditor: editor);
      final back = localSettingsFromJson(decodeObject(encodeJson(localSettingsToJson(value))));
      expect(back.sftpDefaultEditor, editor);
      expect(back.copyWith(terminalFontSize: 15).sftpDefaultEditor, editor);
      expect(back.copyWith(resetSftpDefaultEditor: true).sftpDefaultEditor, isNull);
    }
    for (final invalid in [
      null,
      'code',
      <String, Object?>{},
      {'kind': 'path'},
      {'kind': 'path', 'value': 12},
      {'kind': 'path', 'value': '  '},
      {'kind': 'path', 'value': 'bad\u0000path'},
      {'kind': 'unknown', 'value': 'code'},
    ]) {
      expect(localSettingsFromJson({'sftp_default_editor': invalid}).sftpDefaultEditor, isNull);
    }
  });

  test('device unlock requires explicit opt-in, known capability and an envelope', () {
    final dto = <String, Object?>{'kind': 'touch_id', 'has_device_envelope': true, 'available': true};
    expect(deviceUnlockFromJson(dto).available, isFalse, reason: 'older DTOs cannot silently opt in');
    dto['enabled'] = true;
    expect(deviceUnlockFromJson(dto).available, isTrue);
    dto['kind'] = 'unknown';
    expect(deviceUnlockFromJson(dto).available, isFalse);
    dto['kind'] = 'device_credential';
    expect(deviceUnlockFromJson(dto).kind, DeviceAuthKind.deviceCredential);
    dto['has_device_envelope'] = false;
    expect(deviceUnlockFromJson(dto).available, isFalse);
  });

  group('mapBridgeError', () {
    test('maps app-core codes to AppErrorCode + reason/args', () {
      final cases = <String, AppErrorCode>{
        'wrong_passphrase': AppErrorCode.wrongPassphrase,
        'wrong_recovery_key': AppErrorCode.invalidRecoveryKey,
        'invalid_credentials': AppErrorCode.invalidCredentials,
        'reauth_required': AppErrorCode.sessionExpired,
        'device_revoked': AppErrorCode.deviceRevoked,
        'device_not_trusted': AppErrorCode.deviceNotTrusted,
        'offline': AppErrorCode.serverUnreachable,
        'upgrade_required': AppErrorCode.incompatibleServer,
        'ssh_host_key_changed': AppErrorCode.hostKeyChanged,
        'ssh_host_key_rejected': AppErrorCode.hostKeyRejected,
        'ssh_auth_failed': AppErrorCode.authFailed,
        'approval': AppErrorCode.verificationMismatch,
        'cancelled': AppErrorCode.cancelled,
        'unsupported': AppErrorCode.unsupported,
        'storage': AppErrorCode.internal,
        'secure_store': AppErrorCode.secureStore,
        'something_new': AppErrorCode.internal,
      };
      for (final MapEntry(:key, :value) in cases.entries) {
        final e = mapBridgeError(_e(key));
        expect(e.code, value, reason: key);
        expect(e.message, 'diag $key');
      }
    });

    test('keeps structured details', () {
      final invalid = mapBridgeError(_e('invalid_input', {'field': 'port', 'rule': 'must be 1..=65535'}));
      expect(invalid.code, AppErrorCode.validation);
      expect(invalid.reason, AppErrorReason.invalidField);
      expect(invalid.args, {'field': 'port', 'rule': 'must be 1..=65535'});

      final limited = mapBridgeError(_e('rate_limited', {'retry_after_seconds': '30'}));
      expect(limited.code, AppErrorCode.rateLimited);
      expect(limited.args['retry_after_seconds'], '30');

      expect(mapBridgeError(_e('vault_locked')).reason, AppErrorReason.vaultLocked);
      expect(mapBridgeError(_e('no_active_profile')).reason, AppErrorReason.noActiveProfile);
      expect(mapBridgeError(_e('not_found', {'what': 'Host'})).reason, AppErrorReason.hostNotFound);
      expect(mapBridgeError(_e('already_exists', {'what': 'email'})).code, AppErrorCode.emailTaken);
      expect(mapBridgeError(_e('ssh_passphrase_required')).reason, AppErrorReason.keyPassphraseRequired);
    });

    test('server protocol codes follow the error_messages table', () {
      AppErrorCode server(String protocol) => mapBridgeError(_e('server', {'protocol_code': protocol})).code;
      expect(server('unauthorized'), AppErrorCode.sessionExpired);
      expect(server('refresh_token_reused'), AppErrorCode.sessionExpired);
      expect(server('forbidden'), AppErrorCode.forbidden);
      expect(server('email_not_verified'), AppErrorCode.emailNotVerified);
      expect(server('gone'), AppErrorCode.requestExpired);
      expect(server('payload_too_large'), AppErrorCode.payloadTooLarge);
      expect(server('invalid_proof'), AppErrorCode.invalidProof);
      expect(server('unavailable'), AppErrorCode.serverUnavailable);
    });

    test('a reason sent by the core wins over the derived one', () {
      final e = mapBridgeError(_e('invalid_input', {'field': 'x'}, 'weak_passphrase'));
      expect(e.reason, AppErrorReason.weakPassphrase);
    });

    test('toAppException never exposes unknown error text', () {
      final e = toAppException(StateError('secret-ish detail'));
      expect(e.code, AppErrorCode.internal);
      expect(e.message, isNot(contains('secret-ish')));
    });
  });

  group('DTO JSON', () {
    const hostJson = {
      'id': 'h1',
      'name': 'db',
      'address': '10.0.0.5',
      'port': 2222,
      'username': 'ops',
      'credential_id': 'c1',
      'group_id': null,
      'jump_chain': ['j1'],
      'jump_profile_id': null,
      'proxy_id': null,
      'proxy_command': 'nc %h %p',
      'host_key_policy': 'accept_new',
      'backend': 'open_ssh',
      'keepalive_secs': 30,
      'agent_forwarding': true,
      'tags': ['prod'],
      'notes': 'n',
      'metadata': {'cc.auth.inline_credential': 'c1'},
      'created_at_ms': 1700000000000,
      'updated_at_ms': 1700000001000,
      'auth_mode': 'inline_password',
    };

    test('host round trip keeps fields the model does not carry', () {
      final host = hostFromJson(hostJson);
      expect(host.port, 2222);
      expect(host.hostKeyPolicy, HostKeyPolicy.acceptNew);
      expect(host.backend, SshBackend.openSsh);
      expect(host.inlineCredentialId, const ObjectId('c1'));
      expect(host.createdAt, DateTime.fromMillisecondsSinceEpoch(1700000000000, isUtc: true));
      final back = hostToJson(host, base: hostJson);
      expect(back, {...hostJson, 'protocol': 'ssh', 'rdp_domain': null, 'rdp_width': 1280, 'rdp_height': 720});
      // A brand-new draft still produces every field app-core requires.
      final fresh = hostToJson(Host.create(name: 'x', address: 'y'));
      expect(fresh.keys, containsAll(hostJson.keys));
    });

    test('credential exposes only secret flags', () {
      final c = credentialFromJson({
        'id': 'c1',
        'name': 'key',
        'kind': 'ssh_private_key',
        'username': null,
        'has_secret': true,
        'has_remembered_passphrase': false,
        'key_encrypted': true,
        'key_algorithm': 'ecdsa_p256',
        'public_key': 'ecdsa-sha2-nistp256 AAAA',
        'certificate': null,
        'fingerprint': 'SHA256:x',
        'agent_path': null,
        'created_at_ms': 0,
        'updated_at_ms': 0,
        'owner_host_id': null,
      });
      expect(c.kind, CredentialKind.sshPrivateKey);
      expect(c.secretId, isNotNull);
      expect(c.remembersPassphrase, isFalse);
      expect(c.keyAlgorithm, KeyAlgorithm.ecdsaP256);
    });

    test('snippet, tunnel and settings round trips', () {
      final snippet = Snippet(
        id: const ObjectId('s1'),
        name: 'logs',
        snippetType: SnippetType.kubectl,
        template: 'kubectl logs {{pod}}',
        variables: const [SnippetVariable(name: 'pod', defaultValue: 'web')],
        riskLevel: RiskLevel.readOnly,
        createdAt: DateTime.utc(2026),
        updatedAt: DateTime.utc(2026),
        usageCount: 3,
      );
      final s2 = snippetFromJson(snippetToJson(snippet));
      expect(s2.variables.single.defaultValue, 'web');
      expect(s2.snippetType, SnippetType.kubectl);
      expect(s2.usageCount, 3);

      final tunnel = Tunnel(
        id: const ObjectId('t1'),
        name: 'pg',
        kind: TunnelKind.local,
        hostId: const ObjectId('h1'),
        bindHost: '127.0.0.1',
        bindPort: 5433,
        targetHost: 'db',
        targetPort: 5432,
        createdAt: DateTime.utc(2026),
        updatedAt: DateTime.utc(2026),
      );
      expect(tunnelFromJson(tunnelToJson(tunnel)).targetPort, 5432);

      const settings = LocalSettings(appLocale: AppLocale.ru, glassMode: GlassMode.solid, terminalFontSize: 15);
      final back = localSettingsFromJson(decodeObject(encodeJson(localSettingsToJson(settings))));
      expect(back.appLocale, AppLocale.ru);
      expect(back.glassMode, GlassMode.solid);
      expect(back.terminalFontSize, 15);
    });

    test('tunnel runtime, sync status and devices', () {
      final running = tunnelRuntimeFromJson({
        'state': {'state': 'running'},
        'bytes_sent': 10,
        'bytes_received': 5,
        'active_connections': 2,
        'started_at_ms': 1,
      });
      expect(running.state, TunnelRunState.running);
      expect(running.bytesTransferred, 15);
      final failed = tunnelRuntimeFromJson({
        'state': {'state': 'failed', 'reason': 'bind'},
        'started_at_ms': 1,
      });
      expect(failed.error, 'bind');

      final now = DateTime.utc(2026, 9, 27);
      final status = syncStatusFromJson({
        'phase': 'offline',
        'pending': 4,
        'last_error': 'dns',
        'next_retry_in_ms': 5000,
        'last_sequence': 9,
      }, now: now);
      expect(status.state, SyncState.offline);
      expect(status.pendingChanges, 4);
      expect(status.nextRetryAt, now.add(const Duration(seconds: 5)));
      expect(status.issues.single.message, 'dns');
      expect(syncStatusFromJson({'phase': 'local_only'}).state, SyncState.localOnly);
      expect(syncStatusFromJson({'phase': 'not_running'}).state, SyncState.paused);

      final devices = devicesFromJson({
        'devices': [
          {
            'device_id': 'd1',
            'name': 'Mac',
            'platform': 'macos',
            'status': 'active',
            'trusted_for_vault': true,
            'is_current': true,
            'created_at_ms': 0,
          },
        ],
        'pending_requests': [
          {
            'request_id': 'r1',
            'device_id': 'd2',
            'device_name': 'PC',
            'platform': 'windows',
            'vault_ids': ['v1'],
            'status': 'pending',
            'created_at_ms': 0,
            'expires_at_ms': 1,
          },
        ],
      }, vaultId: const VaultId('v1'));
      expect(devices.devices.single.isTrustedFor(const VaultId('v1')), isTrue);
      expect(devices.pendingRequests.single.device.platform, DevicePlatform.windows);
      expect(verificationCodeFrom('12345 67890 11111 22222 33333 44444').groups, hasLength(6));
    });

    test('profile and recovery kit', () {
      final p = profileFromJson({
        'id': 'p1',
        'display_name': 'Work',
        'kind': 'synced',
        'active': true,
        'created_at_ms': 0,
        'last_opened_at_ms': null,
        'server_url': 'https://sync.example.org',
        'email': 'a@b.c',
        'device_id': 'd1',
        'vault_id': 'v1',
        'vault_state': 'locked',
      });
      expect(p.isSynced, isTrue);
      expect(p.serverUrl, Uri.parse('https://sync.example.org'));
      final kit = recoveryKitFromJson({
        'vault_id': 'v1',
        'words': List.filled(24, 'abandon'),
        'phrase': '',
        'qr_payload': 'consolecrypt-recovery:v1:v1:k',
        'server_url': null,
        'created_at_ms': 0,
      });
      expect(kit.exposeWords(), hasLength(24));
      expect(kit.toString(), isNot(contains('abandon')));
    });
  });

  test('effective config preview follows the planner rules', () {
    final t = DateTime.utc(2026);
    final group = Group(
      id: const ObjectId('g1'),
      name: 'Prod',
      inheritedUsername: 'deploy',
      inheritedPort: 2200,
      inheritedCredentialId: const ObjectId('c1'),
      createdAt: t,
      updatedAt: t,
    );
    final cred = Credential(
      id: const ObjectId('c1'),
      name: 'deploy key',
      kind: CredentialKind.sshPrivateKey,
      createdAt: t,
      updatedAt: t,
    );
    final host = Host(
      id: const ObjectId('h1'),
      name: 'web',
      address: 'web',
      groupId: group.id,
      createdAt: t,
      updatedAt: t,
    );
    final prompting = Host(
      id: const ObjectId('h2'),
      name: 'db',
      address: 'db',
      groupId: group.id,
      metadata: const {HostMetadataKeys.authPrompt: 'password'},
      createdAt: t,
      updatedAt: t,
    );
    final r = EffectiveConfigResolver(
      hosts: [host, prompting],
      groups: [group],
      credentials: [cred],
      jumpProfiles: const [],
    );
    final e = r.resolve(host);
    expect(e.port.value, 2200);
    expect(e.port.source, ValueSource.group);
    expect(e.username.value, 'deploy');
    expect(e.credentialName, 'deploy key');
    expect(e.problems, isEmpty);
    // `cc.auth.prompt` stops credential inheritance.
    final p = r.resolve(prompting);
    expect(p.credentialId.value, isNull);
    expect(p.problems, isEmpty);
  });

  group('core-gaps wire format', () {
    test('errors: new codes, core reasons carry their details as args', () {
      final big = mapBridgeError(_e('payload_too_large', {'what': 'file', 'size': '60', 'limit': '50'}));
      expect(big.code, AppErrorCode.payloadTooLarge);
      expect(big.args['limit'], '50');
      expect(mapBridgeError(_e('permission_denied', {'path': '/root'})).code, AppErrorCode.forbidden);
      final dir = mapBridgeError(_e('not_found', {'path': '/nope', 'name': 'nope'}, 'directory_not_found'));
      expect(dir.reason, AppErrorReason.directoryNotFound);
      expect(dir.args['path'], '/nope');
      final exists = mapBridgeError(_e('already_exists', {'what': '/home/emails', 'name': 'emails'}, 'already_exists'));
      expect(exists.code, AppErrorCode.conflict, reason: 'a path mentioning "email" is not "email taken"');
      expect(exists.args['name'], 'emails');
      final current = mapBridgeError(_e('wrong_passphrase', const {}, 'current_passphrase_wrong'));
      expect((current.code, current.reason), (AppErrorCode.wrongPassphrase, AppErrorReason.currentPassphraseWrong));
      final pw = mapBridgeError(_e('invalid_credentials', const {}, 'current_password_wrong'));
      expect(pw.reason, AppErrorReason.currentPasswordWrong);
      final editor = mapBridgeError(_e('io', {'detail': 'Application not found'}, 'editor_launch_failed'));
      expect(editor.reason, AppErrorReason.editorLaunchFailed);
      expect(editor.args['detail'], 'Application not found');
    });

    test('remote file info with symlink target', () {
      final f = remoteFileInfoFromJson({
        'name': 'current',
        'path': '/srv/current',
        'kind': 'symlink',
        'size': 12,
        'permissions': 0x1FF,
        'uid': 1000,
        'gid': 1000,
        'owner': 'deploy',
        'group': 'www',
        'modified_at_ms': 1790000000000,
        'link_target': 'releases/42',
        'link_target_kind': 'dir',
      });
      expect(f.kind, RemoteEntryKind.symlink);
      expect(f.linkTargetKind, RemoteEntryKind.directory);
      expect(f.isDirectory, isTrue);
      expect((f.owner, f.group, f.uid), ('deploy', 'www', 1000));
      expect(f.modeString, 'lrwxrwxrwx');
      final dangling = remoteFileInfoFromJson({'name': 'x', 'path': '/x', 'kind': 'symlink', 'link_target_kind': null});
      expect(dangling.linkTargetKind, isNull);
      expect(dangling.isDirectory, isFalse);
    });

    test('edit sessions, statuses, outcomes and leftovers', () {
      final s = editSessionFromJson({
        'id': 'e1',
        'host_id': 'h1',
        'sftp_id': 's1',
        'remote_path': '/etc/app.conf',
        'target_path': '/etc/real.conf',
        'local_path': '/tmp/e1/app.conf',
        'status': {
          'state': 'conflict',
          'remote': {'size': 7, 'modified_at_ms': 1790000000000, 'permissions': 420},
        },
        'opened_at_ms': 1790000000000,
        'last_synced_at_ms': null,
        'uploads': 3,
        'remote_copies': ['/tmp/e1/app.remote-1.conf'],
        'app': {'kind': 'bundle_id', 'value': 'com.apple.TextEdit'},
      });
      expect(s.sftpSession, const SftpSessionId('s1'));
      expect(s.targetPath, '/etc/real.conf');
      expect((s.status as EditStatusConflict).remote!.size, 7);
      expect(s.status.needsAttention, isTrue);
      expect(s.app, const AppRef(AppRefKind.bundleId, 'com.apple.TextEdit'));
      expect(s.uploads, 3);
      final uploading = editStatusFromJson({'state': 'uploading', 'transferred': 5, 'total': 10});
      expect((uploading as EditStatusUploading).fraction, 0.5);
      expect(editStatusFromJson({'state': 'error', 'message': 'x', 'retryable': false}), isA<EditStatusError>());
      expect(editStatusFromJson({'state': 'something_new'}), isA<EditStatusSynced>());
      expect(editStopOutcomeFromJson({'outcome': 'closed', 'uploaded': true}), isA<EditStopClosed>());
      expect(
        (editStopOutcomeFromJson({'outcome': 'kept_files', 'directory': '/d'}) as EditStopKeptFiles).directory,
        '/d',
      );
      expect(editStopOutcomeFromJson({'outcome': 'upload_failed', 'message': 'm'}), isA<EditStopUploadFailed>());
      final l = editLeftoverFromJson({
        'id': 'e2',
        'host_id': 'h1',
        'remote_path': '/a.txt',
        'target_path': '/a.txt',
        'created_at_ms': 1790000000000,
        'working_file': '/tmp/e2/a.txt',
        'locally_modified': true,
      });
      expect(l.canResume, isTrue);
      expect(l.locallyModified, isTrue);
      expect(editLeftoverFromJson({'id': 'e3'}).canResume, isFalse);
      expect(openWithToJson(const OpenWithDefault()), '{"kind":"default"}');
      expect(openWithToJson(const OpenWithChoose()), '{"kind":"choose"}');
      expect(
        openWithToJson(const OpenWithApp(AppRef(AppRefKind.path, '/Applications/TextEdit.app'))),
        '{"kind":"app","app":{"kind":"path","value":"/Applications/TextEdit.app"}}',
      );
    });

    test('core prompts', () {
      final hk =
          corePromptFromJson({
                'HostKey': {
                  'request_id': 'r1',
                  'host': '10.0.0.5',
                  'port': 2222,
                  'host_pattern': '[10.0.0.5]:2222',
                  'host_id': 'h1',
                  'host_name': 'db',
                  'hop_index': 1,
                  'hop_count': 2,
                  'key_type': 'ssh-ed25519',
                  'fingerprint_sha256': 'SHA256:abc',
                  'other_known_key_types': ['ssh-rsa'],
                },
              })!
              as HostKeyCorePrompt;
      expect((hk.requestId, hk.port, hk.hopIndex, hk.hostId), ('r1', 2222, 1, const ObjectId('h1')));
      expect(hk.otherKnownKeyTypes, ['ssh-rsa']);
      final pw =
          corePromptFromJson({
                'Password': {'request_id': 'r2', 'host_id': null, 'host_name': 'web'},
              })!
              as PasswordCorePrompt;
      expect((pw.hostName, pw.hostId), ('web', null));
      final pp =
          corePromptFromJson({
                'Passphrase': {
                  'request_id': 'r3',
                  'credential_id': 'c1',
                  'credential_name': 'deploy key',
                  'attempt': 1,
                },
              })!
              as PassphraseCorePrompt;
      expect(pp.isRetry, isTrue);
      expect(corePromptFromJson({'Unknown': {}}), isNull);
    });

    test('planner preview → effective host config with codes', () {
      final e = effectiveHostFromPlanJson({
        'host_id': 'h1',
        'port': {'value': 2222, 'source': 'group', 'source_id': 'g1', 'source_name': 'prod'},
        'username': {'value': 'postgres', 'source': 'credential', 'source_id': 'c1', 'source_name': 'db pw'},
        'credential_id': {'value': 'c1', 'source': 'group', 'source_id': 'g1', 'source_name': 'prod'},
        'credential_name': 'db pw',
        'prompts_for_password': false,
        'route': [
          {
            'host_id': 'b1',
            'name': 'bastion',
            'username': 'jump',
            'address': 'b',
            'port': 22,
            'label': 'bastion (jump@b)',
          },
        ],
        'route_source': {
          'value': 'via bastion',
          'source': 'jump_profile',
          'source_id': 'j1',
          'source_name': 'via bastion',
        },
        'group_path': ['prod'],
        'proxy_id': null,
        'backend': 'native',
        'route_description': null,
        'diagnostics': [
          {
            'code': 'jump_host_deleted',
            'severity': 'error',
            'args': {'host_id': 'x'},
            'message': 'm',
          },
          {
            'code': 'tunnel_public_bind',
            'severity': 'warning',
            'args': {'name': 't'},
            'message': 'tunnel t',
          },
          {'code': 'credential_prompt', 'severity': 'info', 'args': <String, String>{}, 'message': 'asked'},
        ],
        'ok': false,
      });
      expect((e.port.value, e.port.source, e.port.sourceName), (2222, ValueSource.group, 'prod'));
      expect((e.username.value, e.username.source), ('postgres', ValueSource.host));
      expect(e.routeSource.source, ValueSource.jumpProfile);
      expect(e.route.single.label, 'bastion (jump@b)');
      expect(e.diagnostics.map((d) => d.code), ['jump_host_deleted', 'tunnel_public_bind', 'credential_prompt']);
      expect(e.diagnostics.first.args['host_id'], 'x');
      // Legacy English problems for the current host editor (no info rows).
      expect(e.problems, [planDiagnosticLegacyText['jump_host_deleted'], 'tunnel t']);
    });
  });
}
