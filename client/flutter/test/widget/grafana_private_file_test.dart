import 'dart:io';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_input.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_worker.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart' show settle;

class _PrivateProbe implements GrafanaDashboardInspector {
  final delegate = IsolatedGrafanaDashboardInspector();
  Future<GrafanaInspection>? result;
  @override
  Future<GrafanaInspection> inspect(String text) => result = delegate.inspect(text);
  @override
  void cancel() => delegate.cancel();
  @override
  void dispose() => delegate.dispose();
}

void main() {
  final path = Platform.environment['CONSOLECRYPT_GRAFANA_PRIVATE_FIXTURE'];
  testWidgets(
    'opt-in private file is read-only, isolated, exact and bounded; no raw values are logged',
    (tester) async {
      // A failed layout must still fail, without dumping a private JSON preview
      // through framework diagnostics. No exception is ignored or marked expected.
      final previous = FlutterError.onError, errors = <String>[];
      FlutterError.onError = (details) => errors.add(details.exception.runtimeType.toString());
      var alive = true;
      final probe = _PrivateProbe();
      Future<String?>? read;
      try {
        await tester.pumpWidget(
          MaterialApp(
            theme: AppTheme.dark(platform: TargetPlatform.windows),
            localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
            supportedLocales: AppLocalizations.supportedLocales,
            home: Scaffold(
              body: MonitoringGrafanaImport(
                sources: const [monitoringFixture],
                isCurrent: () => alive,
                inspectorFactory: () => probe,
                loadJson: () => read = readGrafanaInput(File(path!).openRead(), isCurrent: () => alive),
                onContinue: (_) => fail('Private inspection must not prefill or save automatically'),
                onCancel: () {},
              ),
            ),
          ),
        );
        final raw = await tester.runAsync(() async {
          await tester.tap(find.byKey(const ValueKey('grafana-import-file')));
          return read!;
        });
        await settle(tester);
        expect(raw != null, isTrue);
        final field = tester.widget<TextField>(find.byKey(const ValueKey('grafana-import-json')));
        expect(field.readOnly, isTrue);
        expect(field.controller!.text.length <= 8192, isTrue);
        final inspect = find.byKey(const ValueKey('grafana-import-inspect'));
        await Scrollable.ensureVisible(tester.element(inspect), alignment: .5);
        await settle(tester);
        final inspected = await tester.runAsync(() async {
          await tester.tap(inspect);
          return probe.result!.timeout(const Duration(seconds: 5));
        });
        await settle(tester);
        expect(inspected!.report.originalJson == raw, isTrue);
        expect(inspected.report.panels.length, 62);
        expect(inspected.report.supported.length, 35);
        expect(inspected.report.unsupported.length, 27);
      } finally {
        alive = false;
        probe.dispose();
        await tester.pumpWidget(const SizedBox.shrink());
        FlutterError.onError = previous;
      }
      expect(errors, isEmpty, reason: 'Any framework error fails; private payload diagnostics are never emitted.');
    },
    skip: path == null,
    timeout: const Timeout(Duration(seconds: 30)),
  );
}
