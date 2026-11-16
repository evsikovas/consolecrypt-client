import 'package:consolecrypt/l10n/app_localizations_en.dart';
import 'package:consolecrypt/l10n/app_localizations_ru.dart';
import 'package:consolecrypt/monitoring/monitoring_errors.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  final en = AppLocalizationsEn();
  final ru = AppLocalizationsRu();

  test('all local error codes have localized messages without diagnostic suffixes', () {
    for (final code in MonitoringErrorCode.values) {
      final failure = MonitoringFailure.local(code);
      final english = monitoringFailureMessage(en, failure);
      final russian = monitoringFailureMessage(ru, failure);
      expect(failure.code, code.wire);
      expect(failure.kind, code);
      expect(failure.message, isEmpty);
      expect(english, isNotEmpty);
      expect(russian, isNot(english), reason: code.name);
      expect(russian, isNot(contains(code.wire)));
      expect(russian, isNot(endsWith(': ')));
      expect(russian, isNot(contains('HTTP —')));
    }
  });

  test('Native code diagnostics never leak into localized error text or toString', () {
    for (final code in [...MonitoringErrorCode.values.map((c) => c.wire), 'future_native_code']) {
      final failure = MonitoringFailure.fromJson({
        'code': code,
        'message': 'English diagnostic with synthetic-secret',
        'httpStatus': 401,
      });
      final message = monitoringFailureMessage(ru, failure);
      expect(failure.isServerMessage, isFalse);
      expect(message, contains('HTTP 401'));
      expect(message, isNot(contains('English diagnostic')));
      expect(message, isNot(contains('synthetic-secret')));
      expect(failure.toString(), isNot(contains('synthetic-secret')));
      expect(failure.toString(), isNot(contains('future_native_code')));
    }
  });

  test('Prometheus API type message remains untrusted data with localized context', () {
    final failure = MonitoringFailure.fromJson({
      'type': 'bad_data',
      'message': 'parse error: unexpected end of input <script>fixture</script>',
      'httpStatus': 422,
    });
    expect(failure.code, 'bad_data');
    expect(failure.kind, MonitoringErrorCode.unknown);
    expect(failure.isServerMessage, isTrue);
    final message = monitoringFailureMessage(ru, failure);
    expect(message, contains(ru.monitoringErrorServer));
    expect(message, contains('HTTP 422'));
    expect(message, contains(failure.message));
    expect(failure.toString(), isNot(contains(failure.message)));
  });

  test('code takes precedence over API type and empty server messages add no empty details', () {
    final native = MonitoringFailure.fromJson({'code': 'timeout', 'type': 'bad_data', 'message': 'Native diagnostic'});
    expect(native.isServerMessage, isFalse);
    expect(monitoringFailureMessage(ru, native), ru.monitoringErrorTimeout);
    final empty = MonitoringFailure.fromJson({'type': 'future_type', 'message': ''});
    expect(monitoringFailureMessage(ru, empty), ru.monitoringErrorServer);
  });

  test('invalid error payload is itself a typed invalid response', () {
    for (final json in <Json>[
      {'message': 'No code'},
      {'code': 4, 'message': 'Not a string code'},
      {'code': 'timeout', 'message': null},
      {'code': 'timeout', 'message': 'wrong status', 'httpStatus': '504'},
    ]) {
      expect(
        () => MonitoringFailure.fromJson(json),
        throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.invalidResponse)),
      );
    }
  });

  test('source and query validation throw typed codes instead of English text', () {
    expect(
      () => const MonitoringSource(id: '', name: '', endpoint: '').validate(),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.invalidConfig)),
    );
    expect(
      () => const MonitoringSource(
        id: 'source',
        name: 'Source',
        endpoint: 'http://metrics.test',
        auth: SourceAuth.bearerRef,
        credentialId: 'credential',
      ).validate(),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.authRequiresTls)),
    );
    expect(
      () => const MonitoringQuery(expression: '').validate(),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.invalidQuery)),
    );
  });

  test('HTTP Basic and none are explicit source choices, while Bearer retains TLS requirements', () {
    for (final auth in [SourceAuth.none, SourceAuth.basicRef]) {
      expect(
        () => MonitoringSource(
          id: 'source',
          name: 'Source',
          endpoint: 'http://10.10.10.11:3000/api/datasources/proxy/1',
          auth: auth,
          credentialId: auth == SourceAuth.none ? null : 'credential',
        ).validate(),
        returnsNormally,
      );
    }
    for (final endpoint in [
      'http://admin:password@metrics.test',
      'http://metrics.test?token=example',
      'http://metrics.test#fragment',
      'ftp://metrics.test',
    ]) {
      expect(
        () => MonitoringSource(
          id: 'source',
          name: 'Source',
          endpoint: endpoint,
          auth: SourceAuth.basicRef,
          credentialId: 'credential',
        ).validate(),
        throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.invalidConfig)),
      );
    }
  });

  test('unsupported catalog becomes read-only and rejects writes before RPC', () async {
    var calls = 0;
    final store = AppCoreMonitoringSourceStore((_, _) async {
      calls++;
      return {'schemaVersion': 999};
    });
    await expectLater(
      store.load(),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.unsupportedCatalogVersion)),
    );
    expect(store.readOnly, isTrue);
    expect(
      monitoringFailureMessage(ru, const MonitoringFailure.local(MonitoringErrorCode.unsupportedCatalogVersion)),
      ru.monitoringErrorCatalogVersion,
    );
    const source = MonitoringSource(id: 'source', name: 'Source', endpoint: 'https://metrics.test');
    await expectLater(
      store.upsert(source),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.readOnly)),
    );
    await expectLater(
      store.delete(source),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.readOnly)),
    );
    expect(calls, 1);
  });

  test('invalid response models throw localized typed failures', () {
    expect(
      () => MonitoringSource.fromRecord({
        'id': 'different',
        'configuration': const MonitoringSource(
          id: 'source',
          name: 'Source',
          endpoint: 'https://metrics.test',
        ).toJson(),
      }),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.invalidResponse)),
    );
    expect(
      () => MonitoringSample.fromJson({
        'timestamp': double.nan,
        'value': {'kind': 'finite', 'value': 1},
      }),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.invalidResponse)),
    );
    expect(
      () => QueryData.fromJson({'kind': 'histogram'}),
      throwsA(isA<MonitoringFailure>().having((e) => e.kind, 'kind', MonitoringErrorCode.unsupportedResult)),
    );
  });
}
