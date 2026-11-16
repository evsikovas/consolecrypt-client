import 'dart:convert';

import 'package:consolecrypt/monitoring/grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_buffer.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_input.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_worker.dart';
import 'package:flutter/foundation.dart' show debugPrint;
import 'package:flutter_test/flutter_test.dart';

String dashboard(int padding, {bool pretty = false}) {
  final value = {
    'schemaVersion': 42,
    'title': 'Owned synthetic dashboard',
    'panels': [
      for (var i = 0; i < 128; i++)
        {
          'title': 'Owned $i',
          'type': 'timeseries',
          'datasource': {'type': 'prometheus', 'uid': 'synthetic'},
          'targets': [
            {'expr': 'metric{owned="${'a' * padding}"}'},
          ],
        },
    ],
  };
  return (pretty ? const JsonEncoder.withIndent('  ') : const JsonEncoder()).convert(value);
}

void main() {
  test('small JSON is editable and exact; large JSON is a bounded projection of the exact original', () {
    const small = '{"owned":"exact"}';
    expect(GrafanaInputBuffer.accept(small).display, small);
    expect(GrafanaInputBuffer.accept(small).previewOnly, isFalse);
    final raw = dashboard(2300), value = GrafanaInputBuffer.accept(dashboard(2300));
    expect(value.original == raw, isTrue);
    expect(value.previewOnly, isTrue);
    expect(value.display.length, lessThanOrEqualTo(GrafanaInputBuffer.previewLimit));
  });
  test('preview never cuts a surrogate pair; original Unicode remains exact', () {
    final raw = '${'x' * (GrafanaInputBuffer.previewLimit - 1)}😀${'z' * 20000}',
        value = GrafanaInputBuffer.accept(raw);
    expect(value.original == raw, isTrue);
    expect(value.display.length, GrafanaInputBuffer.previewLimit - 1);
  });
  test('oversize clipboard cannot remain in the buffer', () {
    expect(
      () => GrafanaInputBuffer.accept('x' * (grafanaInputByteLimit + 1)),
      throwsA(isA<GrafanaInputException>().having((e) => e.reason, 'reason', GrafanaInputFailure.tooLarge)),
    );
  });
  for (final pretty in [false, true]) {
    for (final padding in [2300, 14500]) {
      test('real isolated inspection preserves complete ${pretty ? 'multiline' : 'one-line'} input padding=$padding', () async {
        final raw = dashboard(padding, pretty: pretty), worker = IsolatedGrafanaDashboardInspector();
        addTearDown(worker.dispose);
        final watch = Stopwatch()..start();
        final value = await worker.inspect(raw).timeout(const Duration(seconds: 15));
        expect(value.report.originalJson == raw, isTrue);
        expect(value.report.supported.length, 128);
        debugPrint(
          'OWNED_ISOLATE_METRICS ${jsonEncode({'bytes': utf8.encode(raw).length, 'pretty': pretty, 'elapsedMs': watch.elapsedMilliseconds, 'panels': value.report.panels.length})}',
        );
      });
    }
  }
  test('isolate keeps the exact 2MiB UTF8 budget for multibyte pasted input', () async {
    final worker = IsolatedGrafanaDashboardInspector();
    addTearDown(worker.dispose);
    await expectLater(
      worker.inspect('{"owned":"${'я' * 1100000}"}'),
      throwsA(isA<GrafanaImportException>().having((e) => e.problem, 'problem', GrafanaImportProblem.inputTooLarge)),
    );
  });
  test('cancellation before spawn completes denies the old result and allows a fresh explicit inspection', () async {
    final worker = IsolatedGrafanaDashboardInspector();
    addTearDown(worker.dispose);
    final old = worker.inspect(dashboard(14500));
    final denied = expectLater(old, throwsA(isA<GrafanaInspectionCancelled>()));
    worker.cancel();
    await denied;
    final fresh = await worker.inspect(dashboard(8));
    expect(fresh.report.supported.length, 128);
  });
  test('disposed worker cannot start another inspection', () async {
    final worker = IsolatedGrafanaDashboardInspector()..dispose();
    await expectLater(worker.inspect(dashboard(8)), throwsA(isA<GrafanaInspectionCancelled>()));
  });
  test('unsupported resource exposes bounded identifiers without pretending its panels were inspected', () async {
    final worker = IsolatedGrafanaDashboardInspector();
    addTearDown(worker.dispose);
    final raw = jsonEncode({
      'apiVersion': 'dashboard.grafana.app/v999',
      'kind': 'Dashboard',
      'spec': {
        'elements': {'owned': <String, Object?>{}},
      },
    });
    final value = await worker.inspect(raw);
    expect(value.report.problems, contains(GrafanaImportProblem.unsupportedSchema));
    expect(value.apiVersion, 'dashboard.grafana.app/v999');
    expect(value.kind, 'Dashboard');
    expect(value.report.originalJson == raw, isTrue);
  });
}
