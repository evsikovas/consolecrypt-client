import 'package:consolecrypt/core/models/models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('moving a host preserves all explicit connection, auth and display fields', () {
    final now = DateTime.now().toUtc();
    final original = Host(
      id: ObjectId.generate(),
      name: 'database',
      address: 'db.example.test',
      createdAt: now.subtract(const Duration(days: 2)),
      updatedAt: now.subtract(const Duration(days: 1)),
      port: 2222,
      username: 'ops',
      credentialId: ObjectId.generate(),
      groupId: ObjectId.generate(),
      jumpChain: [ObjectId.generate()],
      jumpProfileId: ObjectId.generate(),
      proxyId: ObjectId.generate(),
      hostKeyPolicy: HostKeyPolicy.strict,
      backend: SshBackend.openSsh,
      keepaliveSecs: 45,
      agentForwarding: true,
      tags: const ['database', 'production'],
      notes: 'Preserved note',
      metadata: const {HostMetadataKeys.authPrompt: 'password', 'custom': 'kept'},
    );
    for (final group in [ObjectId.generate(), null]) {
      final moved = original.withGroup(group);
      expect(moved.groupId, group);
      expect(moved.updatedAt.isAfter(original.updatedAt), isTrue);
      expect(moved.id, original.id);
      expect(moved.name, original.name);
      expect(moved.address, original.address);
      expect(moved.createdAt, original.createdAt);
      expect(moved.port, original.port);
      expect(moved.username, original.username);
      expect(moved.credentialId, original.credentialId);
      expect(moved.jumpChain, original.jumpChain);
      expect(moved.jumpProfileId, original.jumpProfileId);
      expect(moved.proxyId, original.proxyId);
      expect(moved.hostKeyPolicy, original.hostKeyPolicy);
      expect(moved.backend, original.backend);
      expect(moved.keepaliveSecs, original.keepaliveSecs);
      expect(moved.agentForwarding, original.agentForwarding);
      expect(moved.tags, original.tags);
      expect(moved.notes, original.notes);
      expect(moved.metadata, original.metadata);
    }
  });

  group('snippet templates (mirror cc_models::snippet)', () {
    test('extracts variables in order, deduplicated, like template_variables', () {
      expect(templateVariables('kubectl logs -n {{namespace}} {{ pod }} --tail={{lines}} {{pod}}'), [
        'namespace',
        'pod',
        'lines',
      ]);
      expect(templateVariables('echo {{}} {{1bad}} {{ unterminated'), isEmpty);
      expect(templateVariables('{{a.b-c_d}}'), ['a.b-c_d']);
    });

    test('renders known variables and leaves others untouched', () {
      expect(
        renderTemplate('kubectl logs -n {{namespace}} {{ pod }} {{missing}}', {'namespace': 'prod', 'pod': 'api'}),
        'kubectl logs -n prod api {{missing}}',
      );
    });

    test('effectiveVariables merges declared metadata', () {
      final now = DateTime.now();
      final s = Snippet(
        id: ObjectId.generate(),
        name: 'x',
        snippetType: SnippetType.bash,
        template: 'du -ah {{path}} | head -n {{count}}',
        variables: const [SnippetVariable(name: 'count', defaultValue: '20')],
        createdAt: now,
        updatedAt: now,
      );
      expect(s.effectiveVariables.map((v) => v.name), ['path', 'count']);
      expect(s.effectiveVariables.last.defaultValue, '20');
    });
  });

  group('risk', () {
    test('requiresConfirmation mirrors RiskLevel::requires_confirmation', () {
      expect(RiskLevel.readOnly.requiresConfirmation, isFalse);
      expect(RiskLevel.modifying.requiresConfirmation, isTrue);
      expect(RiskLevel.destructive.requiresConfirmation, isTrue);
      expect(RiskLevel.unknown.requiresConfirmation, isTrue);
    });

    test('local rules win when they know the command', () {
      expect(
        combineRisk(declared: RiskLevel.readOnly, local: RiskLevel.destructive, source: SnippetSource.ai),
        RiskLevel.destructive,
      );
      expect(
        combineRisk(declared: RiskLevel.destructive, local: RiskLevel.readOnly, source: SnippetSource.user),
        RiskLevel.destructive,
      );
    });

    test('AI hints are not trusted when local rules do not know the command', () {
      expect(
        combineRisk(declared: RiskLevel.readOnly, local: RiskLevel.unknown, source: SnippetSource.ai),
        RiskLevel.unknown,
      );
      expect(
        combineRisk(declared: RiskLevel.readOnly, local: RiskLevel.unknown, source: SnippetSource.user),
        RiskLevel.readOnly,
      );
    });
  });

  group('validation (mirror cc_models)', () {
    test('Host.validate', () {
      final h = Host.create(name: 'db', address: '10.0.0.1');
      expect(h.validate(), isNull);
      expect(Host.create(name: 'db', address: 'bad host').validate()?.field, 'address');
      expect(Host.create(name: ' ', address: 'x').validate()?.field, 'name');
      final selfJump = Host(
        id: h.id,
        name: 'db',
        address: 'x',
        jumpChain: [h.id],
        createdAt: h.createdAt,
        updatedAt: h.updatedAt,
      );
      expect(selfJump.validate()?.field, 'jump_chain');
    });

    Tunnel tunnel(TunnelKind kind, {String bind = '127.0.0.1', String? target, int? targetPort}) => Tunnel(
      id: ObjectId.generate(),
      name: 't',
      kind: kind,
      hostId: ObjectId.generate(),
      bindHost: bind,
      bindPort: 15432,
      targetHost: target,
      targetPort: targetPort,
      createdAt: DateTime.now(),
      updatedAt: DateTime.now(),
    );

    test('Tunnel.validate and binds_publicly', () {
      expect(tunnel(TunnelKind.local).validate()?.field, 'target_host');
      expect(tunnel(TunnelKind.local, target: 'db', targetPort: 0).validate()?.field, 'target_port');
      expect(tunnel(TunnelKind.local, target: 'db', targetPort: 5432).validate(), isNull);
      expect(tunnel(TunnelKind.dynamic).validate(), isNull);
      expect(tunnel(TunnelKind.dynamic).bindsPublicly, isFalse);
      expect(tunnel(TunnelKind.dynamic, bind: 'localhost').bindsPublicly, isFalse);
      expect(tunnel(TunnelKind.dynamic, bind: '::1').bindsPublicly, isFalse);
      expect(tunnel(TunnelKind.dynamic, bind: '0.0.0.0').bindsPublicly, isTrue);
    });

    test('hostPattern follows OpenSSH', () {
      expect(hostPattern('Example.org', 22), 'example.org');
      expect(hostPattern('10.0.0.1', 2222), '[10.0.0.1]:2222');
    });

    test('server URL: no default, https required except loopback', () {
      expect(validateServerUrl(''), isNotNull);
      expect(validateServerUrl('sync.example.org'), isNotNull);
      expect(validateServerUrl('http://sync.example.org'), isNotNull);
      expect(validateServerUrl('https://sync.example.org'), isNull);
      expect(validateServerUrl('http://localhost:8080'), isNull);
    });
  });

  group('device verification code (ADR-0004)', () {
    test('formats SHA-256 digest as 6 groups of 5 digits (u40 BE mod 100000)', () {
      // SHA-256("consolecrypt test vector"), computed independently.
      const digest = [
        26, 64, 98, 84, 134, 200, 163, 168, 247, 16, 144, 209, 182, 105, 239, 246, //
        106, 126, 55, 208, 52, 78, 209, 48, 152, 183, 229, 39, 142, 23, 57, 230,
      ];
      expect(VerificationCode.fromDigest(digest).display, '35686 18704 83439 11536 31704 89911');
    });

    test('parses user input and compares', () {
      final a = VerificationCode.tryParse('35686-18704 83439 11536 31704 89911')!;
      expect(a.display, '35686 18704 83439 11536 31704 89911');
      expect(VerificationCode.tryParse('123'), isNull);
      expect(a.matches(VerificationCode.tryParse('356861870483439115363170489911')!), isTrue);
      expect(a.matches(VerificationCode.tryParse('356861870483439115363170489912')!), isFalse);
    });
  });

  group('recovery input (ADR-0002)', () {
    final words = List.filled(24, 'abandon').join(' ');

    test('accepts 24 words, with numbering and punctuation', () {
      expect(RecoveryInput.parse(words), isA<RecoveryMnemonicInput>());
      final numbered = [for (var i = 0; i < 24; i++) '${i + 1}. abandon'].join('\n');
      expect(RecoveryInput.parse(numbered), isA<RecoveryMnemonicInput>());
      expect(RecoveryInput.countWords('a b c'), 3);
    });

    test('rejects wrong word counts', () {
      expect(RecoveryInput.parse(List.filled(12, 'abandon').join(' ')), isNull);
      expect(RecoveryInput.parse('$words extra'), isNull);
    });

    test('accepts the QR payload format', () {
      const key = 'AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8';
      const vault = '0f8fad5b-d9cb-469f-a165-70867728950e';
      final parsed = RecoveryInput.parse('consolecrypt-recovery:v1:$vault:$key');
      expect(parsed, isA<RecoveryQrInput>());
      expect((parsed! as RecoveryQrInput).vaultId.value, vault);
      expect(RecoveryInput.parse('consolecrypt-recovery:v1:$vault:short'), isNull);
      expect(RecoveryInput.parse('consolecrypt-recovery:v1:not-a-uuid:$key'), isNull);
    });
  });
}
