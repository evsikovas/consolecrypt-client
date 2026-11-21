import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/keepass/kdbx_client.dart';
import 'package:consolecrypt/keepass/kdbx_exchange.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  const row = ExchangeEntry(
    ['服务器, "test"', '0012', '=fixture', 'https://example.invalid', "'первая\nвторая"],
    groups: ['Работа', 'a/b'],
    custom: {'Fixture': 'α'},
  );

  test('CSV preserves strings and neutralizes spreadsheet formulas with reversible escaping', () {
    final bytes = encodeKdbxExchange(KdbxExchangeFormat.csv, [row]);
    expect(utf8.decode(bytes), contains("'=fixture"));
    final restored = decodeKdbxExchange(KdbxExchangeFormat.csv, bytes).single;
    expect(restored.fields, row.fields);
    expect(restored.groups, row.groups);
    expect(restored.custom, isEmpty);
  });

  test('imports common manager CSV headers, UTF-8 BOM, semicolon and multiline notes', () {
    final input = Uint8List.fromList(
      utf8.encode(
        '\ufeffsep=;\r\nname;login_username;login_password;login_uri;notes;folder\r\n'
        'Vault;0001;00;https://example.invalid;"line one\r\nline two";Servers\r\n',
      ),
    );
    final restored = decodeKdbxExchange(KdbxExchangeFormat.csv, input).single;
    expect(restored.fields, ['Vault', '0001', '00', 'https://example.invalid', 'line one\r\nline two']);
    expect(restored.groups, ['Servers']);
  });

  test('rejects a CSV with incomplete rows', () {
    final bytes = Uint8List.fromList(utf8.encode('name,password,notes\nExample,test\n'));
    expect(() => decodeKdbxExchange(KdbxExchangeFormat.csv, bytes), throwsFormatException);
  });

  test('worker freezes the preview, imports once, then reopens JSON entries from KDBX', () async {
    final dir = await Directory.systemTemp.createTemp('cc-password-exchange-');
    final worker = await KdbxClient.start();
    addTearDown(() async {
      worker.close();
      await dir.delete(recursive: true);
    });
    final kdbx = '${dir.path}/test.kdbx';
    final importFile = File('${dir.path}/entries.json');
    final archive = encodeKdbxExchange(KdbxExchangeFormat.json, [row, row]);
    await importFile.writeAsBytes(archive);
    await worker.open(kdbx, SecretText('fixture-master'), create: true);

    final args = {'path': importFile.path, 'format': 'json'};
    expect(await worker.request('previewExchange', args), 2);
    final before = (await worker.request('snapshot')) as KdbxData;
    expect(before['entries'], isEmpty);
    await importFile.writeAsString('changed after preview');
    final result = (await worker.request('commitExchange')) as KdbxData;
    expect(result['added'], 1);
    expect(result['skipped'], 1);
    await worker.request('save');
    final reopened = await worker.open(kdbx, SecretText('fixture-master'));
    final entry = (reopened['entries'] as List).single as KdbxData;
    expect(await worker.request('field', {'id': entry['id'], 'key': 'Password'}), '=fixture');
    final bytes = (await worker.request('exportExchange', {'format': 'json'})) as Uint8List;
    final restored = decodeKdbxExchange(KdbxExchangeFormat.json, bytes).single;
    expect(restored.toJson(), row.toJson());

    await importFile.writeAsBytes(archive);
    expect(await worker.request('previewExchange', args), 2);
    final repeated = (await worker.request('commitExchange')) as KdbxData;
    expect(repeated['added'], 0);
    expect(repeated['skipped'], 2);
    expect(repeated['dirty'], false);
    await worker.request('delete', {'id': entry['id']});
    final empty = (await worker.request('exportExchange', {'format': 'json'})) as Uint8List;
    expect(decodeKdbxExchange(KdbxExchangeFormat.json, empty), isEmpty);
  });
}
