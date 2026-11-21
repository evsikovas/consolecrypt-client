import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/keepass/external_vault.dart';
import 'package:consolecrypt/keepass/external_vault_store.dart';
import 'package:flutter_test/flutter_test.dart';

ExternalVaultConnection configuration(
  String endpoint, {
  bool v2 = true,
  ExternalVaultAuth auth = ExternalVaultAuth.userpass,
}) => ExternalVaultConnection(
  id: 'demo',
  profile: 'profile',
  vault: 'vault',
  name: 'Test',
  endpoint: endpoint,
  auth: auth,
  username: 'reader',
  authMount: 'staff',
  mount: 'team/kv',
  namespace: 'tenant',
  versioned: v2,
);

class _RealHttp extends HttpOverrides {}

void main() {
  test('saved credentials require a device key and are bound to the connection origin', () async {
    final deviceKeys = <String, Uint8List>{};
    var authorized = true;
    final store = ExternalVaultCredentialStore(
      keyOperation: (action, id, material) async {
        switch (action) {
          case 'enable':
            deviceKeys[id] = Uint8List.fromList(material!);
            return Uint8List(0);
          case 'release' when authorized && deviceKeys.containsKey(id):
            return Uint8List.fromList(deviceKeys[id]!);
          case 'forget':
            deviceKeys.remove(id);
            return Uint8List(0);
          default:
            throw const ExternalVaultFailure('authentication');
        }
      },
    );
    final source = configuration('https://vault.example.invalid');
    final password = SecretText('long synthetic password');
    final wrapped = await store.remember(source, password, () => true);
    password.wipe();
    final settings = source.toStorage(cipherText: wrapped);
    expect(jsonEncode(settings), isNot(contains('long synthetic password')));
    final saved = ExternalVaultConnection.fromStorage(settings);
    final opened = await store.unlock(saved, () => true);
    expect(opened.expose(), 'long synthetic password');
    opened.wipe();
    authorized = false;
    await expectLater(store.unlock(saved, () => true), throwsA(isA<ExternalVaultFailure>()));
    authorized = true;
    final other = ExternalVaultConnection.fromStorage({...settings, 'endpoint': 'https://other.example.invalid'});
    await expectLater(store.unlock(other, () => true), throwsA(isA<ExternalVaultFailure>()));
    await store.forget(saved, wrapped);
    expect(deviceKeys, isEmpty);
  });

  void realTest(String name, Future<void> Function() body) =>
      test(name, () => HttpOverrides.runWithHttpOverrides(body, _RealHttp()));
  realTest('userpass, nested KV v2 paths, guarded writes and version-specific deletion', () async {
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));
    final requests = <(String, List<String>, String?, String?, Object?)>[];
    server.listen((request) async {
      final raw = await utf8.decoder.bind(request).join();
      final segments = request.uri.pathSegments;
      requests.add((
        request.method,
        segments,
        request.headers.value('X-Vault-Token'),
        request.headers.value('X-Vault-Namespace'),
        raw.isEmpty ? null : jsonDecode(raw),
      ));
      request.response.headers.contentType = ContentType.json;
      if (segments.contains('login')) {
        request.response.write('{"auth":{"client_token":"fixture-token"}}');
      } else if (request.method == 'LIST') {
        request.response.write('{"data":{"keys":["nested/","..","a%2fb","item"]}}');
      } else if (request.method == 'GET') {
        request.response.write('{"data":{"data":{"password":"fixture-pass","enabled":true},"metadata":{"version":9}}}');
      } else if (segments.contains('delete')) {
        request.response.statusCode = 204;
      } else if (((jsonDecode(raw) as Map)['options'] as Map)['cas'] == 8) {
        request.response.statusCode = 400;
        request.response.write('{"errors":["check-and-set rejected"]}');
      } else {
        request.response.write('{"data":{"version":10}}');
      }
      await request.response.close();
    });
    final session = ExternalVaultSession(configuration('http://127.0.0.1:${server.port}/proxy'));
    addTearDown(session.close);
    final password = SecretText('fixture-only');
    await session.signIn(password);
    password.wipe();
    expect(await session.list(''), ['item', 'nested/']);
    final record = await session.read('nested/email login');
    expect(record.values, {'password': 'fixture-pass', 'enabled': true});
    expect(record.version, 9);
    await session.write('nested/email login', record.values, version: record.version);
    await expectLater(
      session.write('nested/email login', record.values, version: 8),
      throwsA(isA<ExternalVaultFailure>().having((e) => e.code, 'code', 'conflict')),
    );
    await session.delete('nested/email login', record.version);
    final login = requests.first;
    expect(login.$1, 'POST');
    expect(login.$2, ['proxy', 'v1', 'auth', 'staff', 'login', 'reader']);
    expect(login.$3, isNull);
    expect(login.$4, 'tenant');
    expect(login.$5, {'password': 'fixture-only'});
    expect(requests.skip(1).every((row) => row.$3 == 'fixture-token' && row.$4 == 'tenant'), isTrue);
    expect(requests[1].$2, ['proxy', 'v1', 'team', 'kv', 'metadata']);
    expect(requests[2].$2.last, 'email login');
    expect((requests[3].$5 as Map)['options'], {'cas': 9});
    expect(requests.last.$5, {
      'versions': [9],
    });
  });

  realTest('KV v1 token login avoids credential redirects and uses direct secret paths', () async {
    final primary = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    final other = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => primary.close(force: true));
    addTearDown(() => other.close(force: true));
    var crossOriginRequests = 0;
    other.listen((request) async {
      crossOriginRequests++;
      await request.response.close();
    });
    final seen = <String>[];
    primary.listen((request) async {
      seen.add(request.uri.path);
      if (request.uri.path.endsWith('redirect')) {
        request.response.statusCode = 307;
        request.response.headers.set('Location', 'http://127.0.0.1:${other.port}/');
      } else {
        request.response.write('{"data":{"user":"fixture"}}');
      }
      await request.response.close();
    });
    final session = ExternalVaultSession(
      configuration('http://127.0.0.1:${primary.port}', v2: false, auth: ExternalVaultAuth.token),
    );
    addTearDown(session.close);
    final token = SecretText('fixture-token');
    await session.signIn(token);
    token.wipe();
    expect((await session.read('valid')).values, {'user': 'fixture'});
    await expectLater(
      session.read('redirect'),
      throwsA(isA<ExternalVaultFailure>().having((e) => e.code, 'code', 'redirect')),
    );
    expect(seen, ['/v1/team/kv/valid', '/v1/team/kv/redirect']);
    expect(crossOriginRequests, 0);
  });

  test('rejects insecure endpoints, embedded credentials and traversal before any request', () {
    for (final endpoint in [
      'http://vault.example.invalid',
      'https://u:p@vault.example.invalid',
      'https://vault.example.invalid/?token=test',
    ]) {
      expect(() => configuration(endpoint), throwsA(isA<ExternalVaultFailure>()));
    }
    for (final path in ['../a', 'a/./b', 'a%2fb', 'a\\b']) {
      expect(() => vaultPath(path), throwsA(isA<ExternalVaultFailure>()));
    }
  });
}
