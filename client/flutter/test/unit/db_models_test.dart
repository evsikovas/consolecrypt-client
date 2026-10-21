import 'dart:convert';

import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('integer and decimal result values preserve every wire digit', () {
    for (final entry in {
      'int': '-9223372036854775808',
      'uint': '18446744073709551615',
      'decimal': '12345678901234567890.12345678901234567890',
      'float': '1.234567890123456789e-300',
    }.entries) {
      final wire = {'kind': entry.key, 'value': entry.value};
      final cell = DbCell.fromJson(wire);
      expect(cell.value, isA<String>());
      expect(cell.toJson(), wire);
      expect(cell.toString(), isNot(contains(entry.value)));
    }
    expect(() => DbCell.fromJson({'kind': 'int', 'value': 123}), throwsFormatException);
    expect(() => DbCell.fromJson({'kind': 'bool', 'value': 'false'}), throwsFormatException);
    expect(DbCell.fromJson({'kind': 'null'}).value, isNull);
  });

  test('query requests expose only native input fields and redact diagnostics', () {
    const request = DbQueryRequest(
      text: "SELECT '{{canary}}'",
      parameters: [DbParameter(name: 'canary', value: DbCell(DbCellKind.text, 'canary-secret'))],
      approvalTicket: 'approval-secret',
    );
    expect(request.toJson().keys, ['text', 'parameters', 'approval_ticket']);
    expect(request.toString(), isNot(contains('canary')));
    expect(request.parameters.single.toString(), isNot(contains('canary')));
  });

  test('same-name metadata objects in different schemas and kinds stay distinct', () {
    const refs = [
      DbObjectRef(database: 'db', schema: 'one', kind: DbObjectKind.table, name: 'x'),
      DbObjectRef(database: 'db', schema: 'two', kind: DbObjectKind.table, name: 'x'),
      DbObjectRef(database: 'db', schema: 'one', kind: DbObjectKind.view, name: 'x'),
      DbObjectRef(database: 'db:one', kind: DbObjectKind.table, name: 'x'),
    ];
    expect(refs.map((r) => r.id).toSet(), hasLength(4));
    for (final ref in refs) {
      expect(DbObjectRef.fromJson(ref.toJson()).id, ref.id);
    }
  });

  test('engine options emit only typed fields, not arbitrary driver overrides', () {
    for (final engine in DbEngine.values) {
      final options = DbDriverOptions(engine: engine);
      final wire = options.toJson();
      expect(wire['kind'], engine.wireName);
      expect(DbDriverOptions.fromJson(wire).toJson(), wire);
      expect(wire, isNot(contains('password')));
      expect(wire, isNot(contains('tlsAllowInvalidCertificates')));
    }
  });

  test('connection metadata includes secret references but never secret buffers', () {
    final secret = SecretText('canary-password');
    const metadata = DbConnection(
      id: 'connection',
      name: 'fixture',
      engine: DbEngine.postgresql,
      host: 'db.example.test',
      port: 5432,
      options: DbDriverOptions(engine: DbEngine.postgresql),
      auth: DbAuth(passwordSecretId: 'password-object-id'),
    );
    final draft = DbConnectionDraft(metadata: metadata, password: secret);
    expect(metadata.production, isTrue);
    expect(metadata.tls.mode, DbTlsMode.verify);
    final wire = jsonEncode(metadata.toJson());
    expect(wire, contains('password-object-id'));
    expect(wire, isNot(contains('canary-password')));
    expect(DbConnection.fromJson(metadata.toJson()).auth.passwordSecretId, 'password-object-id');
    expect(draft.toString(), isNot(contains('canary-password')));
    draft.wipe();
    expect(secret.isWiped, isTrue);
  });

  test('query events retain numeric values and redact diagnostic string output', () {
    final event = DbQueryEvent.fromJson({
      'kind': 'rows',
      'result_index': 2,
      'rows': [
        [
          {'kind': 'uint', 'value': '18446744073709551615'},
          {'kind': 'text', 'value': 'canary'},
        ],
      ],
    });
    expect(event.resultIndex, 2);
    expect(event.rows.single.first.value, '18446744073709551615');
    expect(event.toString(), isNot(contains('canary')));
    expect(DbQueryEvent.fromJson({'kind': 'paused', 'result_index': 2, 'has_more': true}).hasMore, isTrue);
  });

  test('TLS settings omit the channel-binding opt-out unless it is on; sessions report their protection', () {
    expect(const DbTls(mode: DbTlsMode.require).toJson().containsKey('allow_unbound_auth'), isFalse);
    final unbound = DbTls.fromJson(const DbTls(mode: DbTlsMode.require, allowUnboundAuth: true).toJson());
    expect(unbound.allowUnboundAuth, isTrue);
    final info = DbSessionInfo.fromJson({
      'id': 's',
      'connection_id': 'c',
      'engine': 'postgresql',
      'server_version': '17',
      'production': true,
      'read_only': true,
      'server_read_only': 'enforced',
      'transport_security': 'tunnel',
    });
    expect(info.transportSecurity, DbTransportSecurity.tunnel);
    expect(DbTransportSecurity.parse('something_new'), isNull);
  });

  test('cards expect what the TLS mode and route guarantee', () {
    DbConnection connection(DbTlsMode mode, DbRouteKind route) => DbConnection(
      id: 'c',
      name: 'c',
      engine: DbEngine.postgresql,
      host: 'db.example.test',
      port: 5432,
      tls: DbTls(mode: mode),
      route: DbRoute(kind: route, reference: route == DbRouteKind.direct ? null : 'r'),
      options: const DbDriverOptions(engine: DbEngine.postgresql),
    );
    DbTransportSecurity? expected(DbTlsMode mode, DbRouteKind route, {bool loopback = false}) =>
        DbTransportSecurity.expected(connection(mode, route), loopback: loopback);
    expect(expected(DbTlsMode.verify, DbRouteKind.direct), DbTransportSecurity.verified);
    expect(expected(DbTlsMode.require, DbRouteKind.kubernetes), DbTransportSecurity.unverified);
    // Over the network "prefer" never continues without TLS.
    expect(expected(DbTlsMode.prefer, DbRouteKind.direct), DbTransportSecurity.verified);
    expect(expected(DbTlsMode.prefer, DbRouteKind.proxy), DbTransportSecurity.verified);
    expect(expected(DbTlsMode.prefer, DbRouteKind.ssh), isNull);
    expect(expected(DbTlsMode.prefer, DbRouteKind.direct, loopback: true), isNull);
    expect(expected(DbTlsMode.disable, DbRouteKind.docker), DbTransportSecurity.tunnel);
    expect(expected(DbTlsMode.disable, DbRouteKind.proxy, loopback: true), DbTransportSecurity.plaintext);
    expect(expected(DbTlsMode.disable, DbRouteKind.direct), DbTransportSecurity.plaintext);
  });
}
