import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/keepass/kdbx_client.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kdbx/kdbx.dart';

void main() {
  const fixtures = 'test/fixtures/keepass';
  late Directory temp;
  late KdbxClient client;
  setUp(() async {
    temp = await Directory.systemTemp.createTemp('consolecrypt-kdbx-test-');
    client = await KdbxClient.start();
  });
  tearDown(() async {
    client.close();
    await temp.delete(recursive: true);
  });
  List<KdbxData> rows(KdbxData data, String key) => [for (final row in data[key]! as List<Object?>) row! as KdbxData];

  test('remote bytes, custom API fields and composite-key reopening preserve KDBX encryption', () async {
    final encrypted = await File('$fixtures/argon2id.kdbx').readAsBytes();
    var data = await client.openSource(
      'https://dav.example/vault.kdbx',
      password: SecretText('fixture-password'),
      encrypted: encrypted,
    );
    final hash = (await client.request('compositeKey'))! as Uint8List;
    data =
        (await client.request('put', {
              'Title': 'API service',
              'UserName': 'service-user',
              'Password': 'service-secret',
              'URL': '',
              'Notes': 'Visible note\nSecond line',
              'custom': {'API Key': 'api-secret', 'Token': 'api-token'},
            }))!
            as KdbxData;
    final entry = rows(data, 'entries').singleWhere((e) => e['title'] == 'API service');
    expect(entry['fields'], containsAll(['API Key', 'Token']));
    expect(entry['notes'], 'Visible note\nSecond line');
    expect(jsonEncode(data), isNot(contains('service-secret')));
    expect(jsonEncode(data), isNot(contains('api-secret')));
    expect(await client.request('field', {'id': entry['id'], 'key': 'API Key'}), 'api-secret');
    final saved = (await client.request('export'))! as Uint8List;
    expect(utf8.decode(saved, allowMalformed: true), isNot(contains('Visible note')));
    final exported = Directory('build/kdbx-interoperability');
    await exported.create(recursive: true);
    await File('${exported.path}/webdav-export.kdbx').writeAsBytes(saved);
    expect(utf8.decode(saved, allowMalformed: true), isNot(contains('api-secret')));
    final second = await KdbxClient.start();
    addTearDown(second.close);
    final reopened = await second.openSource('https://dav.example/vault.kdbx', encrypted: saved, compositeKey: hash);
    hash.fillRange(0, hash.length, 0);
    final same = rows(reopened, 'entries').singleWhere((e) => e['title'] == 'API service');
    expect(await second.request('field', {'id': same['id'], 'key': 'API Key'}), 'api-secret');
    expect(await second.request('field', {'id': same['id'], 'key': 'Password'}), 'service-secret');
    expect(reopened['dirty'], false);
    await expectLater(second.request('save'), throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'unsupported')));
  });

  for (final name in [
    'v3.kdbx',
    'aes4.kdbx',
    'chacha20.kdbx',
    'argon2d.kdbx',
    'argon2id.kdbx',
    'keyfile.kdbx',
    'xmlkey.kdbx',
    'keyonly.kdbx',
  ]) {
    test('opens, edits and saves $name while preserving encryption and metadata', () async {
      final target = await File('$fixtures/$name').copy('${temp.path}/$name');
      final original = await target.readAsBytes();
      final password = name == 'v3.kdbx'
          ? 'FooBar'
          : name == 'keyonly.kdbx'
          ? ''
          : 'fixture-password';
      final keyFile = name == 'keyfile.kdbx' || name == 'keyonly.kdbx'
          ? '$fixtures/binary.key'
          : name == 'xmlkey.kdbx'
          ? '$fixtures/keyfile.keyx'
          : null;
      final secret = SecretText(password);
      final data = await client.open(target.path, secret, keyFile: keyFile);
      expect(secret.isWiped, isTrue);
      expect(jsonEncode(data), isNot(contains('updated-password')));
      final entry = rows(data, 'entries').first;
      final fields = (await client.request('get', {'id': entry['id']}))! as KdbxData;
      final update = {
        ...fields,
        'id': entry['id'],
        'group': entry['group'],
        'Title': 'Edited title',
        'Password': 'new-password',
      };
      await client.request('put', update);
      final saved = (await client.request('save'))! as KdbxData;
      expect(saved['dirty'], isFalse);
      final backups = await temp.list().where((e) => e.path.endsWith('.bak')).toList();
      expect(backups, hasLength(1));
      expect(await File(backups.single.path).readAsBytes(), original);
      final bytes = await target.readAsBytes();
      expect(bytes, isNot(original));
      expect(utf8.decode(bytes, allowMalformed: true), isNot(contains('new-password')));
      final credentials = Credentials.composite(
        password.isEmpty && keyFile != null ? null : ProtectedValue.fromString(password),
        keyFile == null ? null : await File(keyFile).readAsBytes(),
      );
      final opened = await KdbxFormat().read(bytes, credentials);
      addTearDown(opened.dispose);
      final changed = opened.body.rootGroup.getAllEntries().firstWhere((e) => e.label == 'Edited title');
      expect(changed.getString(KdbxKeyCommon.PASSWORD)!.getText(), 'new-password');
      if (name != 'v3.kdbx') {
        expect(changed.getString(KdbxKey('Custom protected'))!.getText(), 'custom-secret');
        expect(changed.getString(KdbxKey('Custom protected')), isA<ProtectedValue>());
        expect(changed.binaryEntries.single.value.value, utf8.encode('attachment bytes'));
        expect(changed.history.length, greaterThanOrEqualTo(2));
      }
      // An independent Python oracle can read the retained test output too.
      final oracleOutput = Directory('build/kdbx-interoperability');
      await oracleOutput.create(recursive: true);
      await target.copy('${oracleOutput.path}/$name');
    }, timeout: const Timeout(Duration(minutes: 2)));
  }

  test('wrong password and externally modified files never overwrite the source', () async {
    final file = await File('$fixtures/argon2id.kdbx').copy('${temp.path}/conflict.kdbx');
    final original = await file.readAsBytes();
    await expectLater(
      client.open(file.path, SecretText('wrong')),
      throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'key')),
    );
    expect(await file.readAsBytes(), original);
    await client.open(file.path, SecretText('fixture-password'));
    await file.writeAsBytes([...original, 42]);
    await expectLater(client.request('save'), throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'changed')));
    expect(await file.readAsBytes(), [...original, 42]);
  });

  test('creates a database, group and entry; deletion moves the entry to recycle bin', () async {
    final file = '${temp.path}/created.kdbx';
    final created = await client.open(file, SecretText('new-master-password'), create: true);
    final root = rows(created, 'groups').single['id'];
    final grouped = (await client.request('group', {'name': 'Operations', 'parent': root}))! as KdbxData;
    final group = rows(grouped, 'groups').firstWhere((g) => (g['name']! as String).endsWith('Operations'))['id'];
    final withEntry =
        (await client.request('put', {
              'group': group,
              'Title': 'New entry',
              'UserName': 'bob',
              'Password': 'entry-secret',
              'URL': '',
              'Notes': '',
            }))!
            as KdbxData;
    final id = rows(withEntry, 'entries').single['id'];
    final deleted = (await client.request('delete', {'id': id}))! as KdbxData;
    expect(rows(deleted, 'entries').single['group'], isNot(group));
    await client.request('save');
    client.close();
    client = await KdbxClient.start();
    final reopened = await client.open(file, SecretText('new-master-password'));
    expect(rows(reopened, 'entries'), hasLength(1));
    final output = Directory('build/kdbx-interoperability');
    await output.create(recursive: true);
    await File(file).copy('${output.path}/created.kdbx');
  }, timeout: const Timeout(Duration(minutes: 2)));

  test('closing cancels pending work and rejects subsequent requests', () async {
    final pending = client.open('$fixtures/argon2id.kdbx', SecretText('fixture-password'));
    final check = expectLater(pending, throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'closed')));
    client.close();
    await check;
    await expectLater(client.request('snapshot'), throwsA(isA<KdbxFailure>()));
  });

  test('unsafe Argon2 parameters are rejected before key derivation', () async {
    for (final parameters in [(512 * 1024 * 1024, 3), (64 * 1024 * 1024, 0), (1024, 3)]) {
      final file = KdbxFormat().create(Credentials(ProtectedValue.fromString('test-only')), 'Limits');
      addTearDown(file.dispose);
      final kdf = file.header.readKdfParameters;
      KdfField.memory.write(kdf, parameters.$1);
      KdfField.iterations.write(kdf, parameters.$2);
      KdfField.parallelism.write(kdf, 16);
      file.header.writeKdfParameters(kdf);
      await expectLater(file.save(), throwsA(isA<KdbxUnsupportedException>()));
    }
  });

  test('case-colliding custom fields are refused without changing the original', () async {
    for (final name in ['case-collision.kdbx', 'binary-collision.kdbx']) {
      final file = await File('$fixtures/$name').copy('${temp.path}/$name');
      final before = await file.readAsBytes();
      await expectLater(
        client.open(file.path, SecretText('fixture-password')),
        throwsA(isA<KdbxFailure>().having((e) => e.code, 'code', 'unsupported')),
      );
      expect(await file.readAsBytes(), before);
    }
  });

  test('XML v2 key files with an invalid integrity hash are refused', () async {
    final text = await File('$fixtures/keyfile.keyx').readAsString();
    final broken = text.replaceFirst(RegExp('Hash="[A-F0-9]+"'), 'Hash="00000000"');
    expect(() => KeyFileCredentials(Uint8List.fromList(utf8.encode(broken))), throwsA(isA<KdbxInvalidKeyException>()));
  });
}
