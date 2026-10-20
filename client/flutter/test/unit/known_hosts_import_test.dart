import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/hosts/known_hosts_import.dart';
import 'package:flutter_test/flutter_test.dart';

String key([String algorithm = 'ssh-ed25519']) =>
    base64.encode([0, 0, 0, algorithm.length, ...ascii.encode(algorithm), 0, 0, 0, 32, ...List.filled(32, 1)]);

class _Files extends KnownHostsImportService {
  _Files(this.files);
  final List<String> files;
  @override
  List<String> paths() => files;
}

void main() {
  test('aliases, IPv6 and explicit ports become unique connection candidates', () {
    final result = KnownHostsImportService.parse('''
# comment
example.test,192.0.2.1 ssh-ed25519 ${key()}
EXAMPLE.test ssh-rsa ${key('ssh-rsa')}
[example.test]:2222 ssh-ed25519 ${key()}
[2001:db8::1]:2222 ssh-ed25519 ${key()}
2001:db8::2 ssh-ed25519 ${key()}
''');
    expect(result.hosts.map((h) => h.label), [
      'EXAMPLE.test',
      '192.0.2.1',
      'example.test:2222',
      '[2001:db8::1]:2222',
      '2001:db8::2',
    ]);
    expect(result.skipped, 0);
  });

  test('hashed patterns, revoked/CA lines and malformed keys are never hosts', () {
    final result = KnownHostsImportService.parse('''
|1|hash|hash ssh-ed25519 ${key()}
*.test,!blocked.test ssh-ed25519 ${key()}
@revoked revoked.test ssh-ed25519 ${key()}
@cert-authority *.test ssh-ed25519 ${key()}
[bad.test]:65536 ssh-ed25519 ${key()}
bad.test ssh-rsa ${key()}
missing-key
''');
    expect(result.hosts, isEmpty);
    expect(result.skipped, 8);
  });

  test('oversized input is rejected before parsing', () {
    expect(() => KnownHostsImportService.parse('x' * (KnownHostsImportService.maxBytes + 1)), throwsFormatException);
  });

  test('read is bounded, missing files are optional, malformed source is reported', () async {
    final dir = await Directory.systemTemp.createTemp('cc-known-hosts-test-');
    addTearDown(() => dir.delete(recursive: true));
    final valid = File('${dir.path}/known_hosts');
    final bad = File('${dir.path}/invalid');
    await valid.writeAsString('host.test ssh-ed25519 ${key()}');
    await bad.writeAsBytes([0xff, 0xfe]);
    final result = await _Files([valid.path, '${dir.path}/missing', bad.path]).scan();
    expect(result.hosts.single.address, 'host.test');
    expect(result.sources, [valid.path]);
    expect(result.unreadable, [bad.path]);
  });
}
