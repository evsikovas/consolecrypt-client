import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/keepass/external_vault.dart';
import 'package:flutter_test/flutter_test.dart';

class _RealHttp extends HttpOverrides {}

void main() {
  for (final (name, variable) in [('HashiCorp Vault', 'CC_TEST_VAULT'), ('OpenBao', 'CC_TEST_OPENBAO')]) {
    final executable = Platform.environment[variable];
    test(
      '$name native server supports userpass, token, LIST and conditional KV v2 writes',
      () => HttpOverrides.runWithHttpOverrides(() async {
        final temp = await Directory.systemTemp.createTemp('cc-external-vault-');
        addTearDown(() => temp.delete(recursive: true));
        final socket = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
        final authority = '127.0.0.1:${socket.port}';
        final endpoint = 'http://$authority';
        await socket.close();
        final random = Random.secure();
        final rootToken = base64UrlEncode(List.generate(18, (_) => random.nextInt(256)));
        final server = await Process.start(
          executable!,
          ['server', '-dev', '-dev-no-store-token', '-dev-listen-address=$authority', '-dev-root-token-id=$rootToken'],
          workingDirectory: temp.path,
          environment: {'VAULT_ADDR': endpoint, 'BAO_ADDR': endpoint},
        );
        unawaited(server.stdout.drain<void>());
        unawaited(server.stderr.drain<void>());
        addTearDown(() async {
          server.kill();
          await server.exitCode.timeout(const Duration(seconds: 10));
        });

        final fixture = HttpClient()..connectionTimeout = const Duration(seconds: 2);
        addTearDown(() => fixture.close(force: true));
        Future<int> request(String path, {Map<String, Object?>? body}) async {
          final url = Uri.parse('$endpoint/v1/$path');
          final outgoing = await fixture.openUrl(body == null ? 'GET' : 'POST', url);
          outgoing.headers.set('X-Vault-Token', rootToken);
          outgoing.headers.set('X-Vault-Request', 'true');
          if (body != null) {
            outgoing.headers.contentType = ContentType.json;
            outgoing.write(jsonEncode(body));
          }
          final reply = await outgoing.close();
          await reply.drain<void>();
          return reply.statusCode;
        }

        var ready = false;
        for (var attempt = 0; attempt < 100 && !ready; attempt++) {
          try {
            ready = await request('sys/health') == 200;
          } on SocketException {
            /* Still starting. */
          }
          if (!ready) await Future<void>.delayed(const Duration(milliseconds: 150));
        }
        expect(ready, isTrue, reason: '$name did not start on loopback');
        expect(await request('sys/auth/staff', body: {'type': 'userpass'}), 204);
        expect(
          await request(
            'sys/policies/acl/consolecrypt-fixture',
            body: {
              'policy':
                  'path "secret/data/*" { capabilities = ["create", "read", "update"] }\n'
                  'path "secret/metadata/*" { capabilities = ["list"] }\n'
                  'path "secret/delete/*" { capabilities = ["update"] }',
            },
          ),
          204,
        );
        expect(
          await request(
            'auth/staff/users/reader',
            body: {
              'password': 'synthetic-vault-password',
              'token_policies': ['consolecrypt-fixture'],
            },
          ),
          204,
        );

        ExternalVaultSession connect(ExternalVaultAuth auth) => ExternalVaultSession(
          ExternalVaultConnection(
            id: 'id',
            profile: 'profile',
            vault: 'cc-vault',
            name: name,
            endpoint: endpoint,
            auth: auth,
            username: 'reader',
            authMount: 'staff',
            mount: 'secret',
            namespace: '',
            versioned: true,
          ),
        );
        final userpass = connect(ExternalVaultAuth.userpass);
        final token = connect(ExternalVaultAuth.token);
        addTearDown(userpass.close);
        addTearDown(token.close);
        final login = SecretText('synthetic-vault-password');
        final tokenLogin = SecretText(rootToken);
        try {
          await userpass.signIn(login);
          await token.signIn(tokenLogin);
        } finally {
          login.wipe();
          tokenLogin.wipe();
        }
        await token.verifyToken();
        const path = 'nested/demo';
        await userpass.write(path, {'username': '001', 'password': 'fixture', 'port': 8200}, version: null);
        expect(await userpass.list('nested'), ['demo']);
        final first = await token.read(path);
        expect(first.values, {'username': '001', 'password': 'fixture', 'port': 8200});
        await token.write(path, {'password': 'updated'}, version: first.version);
        await expectLater(
          userpass.write(path, {'password': 'stale'}, version: first.version),
          throwsA(isA<ExternalVaultFailure>().having((error) => error.code, 'code', 'conflict')),
        );
        final second = await userpass.read(path);
        expect(second.values, {'password': 'updated'});
        await userpass.delete(path, second.version);
        await expectLater(
          token.read(path),
          throwsA(isA<ExternalVaultFailure>().having((e) => e.code, 'code', 'missing')),
        );
      }, _RealHttp()),
      skip: executable == null ? '$variable points to a native test binary' : false,
    );
  }
}
