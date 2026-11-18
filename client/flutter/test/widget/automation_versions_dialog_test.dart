import 'dart:async';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_versions_dialog.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const project = AutomationProject(id: 'original', name: 'Deploy', readOnly: false, revision: 1);

class Api extends Fake implements AutomationProjectVersionService {
  final rows = <AutomationJson>[
    {'id': 'v1', 'name': 'Stable', 'digest': 'a' * 64, 'created_at_ms': 1700000000000, 'bytes': 128},
  ];
  final saves = <(String, String, String)>[], opens = <(String, String, String)>[];
  Completer<List<AutomationJson>>? pending;
  @override
  Future<List<AutomationJson>> projectVersions(String projectId) async => pending?.future ?? rows;
  @override
  Future<void> saveProjectVersion(String projectId, {required String digest, required String name}) async {
    saves.add((projectId, digest, name));
    rows.add({'id': 'v2', 'name': name, 'digest': digest, 'created_at_ms': 1700000000001, 'bytes': 256});
  }

  @override
  Future<AutomationProject> openProjectVersion(String projectId, String versionId, {required String name}) async {
    opens.add((projectId, versionId, name));
    return AutomationProject(id: 'separate', name: name, readOnly: false, revision: 1);
  }
}

Future<void> show(
  WidgetTester tester,
  Api api, {
  bool Function()? current,
  ValueChanged<AutomationProject?>? result,
  Size size = const Size(900, 700),
  double scale = 1,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    MaterialApp(
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
        child: child!,
      ),
      home: Scaffold(
        body: Builder(
          builder: (context) => TextButton(
            onPressed: () async {
              final value = await showDialog<AutomationProject>(
                context: context,
                builder: (_) => AutomationVersionsDialog(
                  project: project,
                  digest: 'b' * 64,
                  service: api,
                  originalCurrent: current ?? () => true,
                ),
              );
              result?.call(value);
            },
            child: const Text('Versions'),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.text('Versions'));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 400));
  await tester.pump();
}

Future<void> tap(WidgetTester tester, String key) async {
  final f = find.byKey(ValueKey(key));
  await tester.ensureVisible(f);
  await tester.tap(f);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('opening only lists versions; save freezes the exact current tree digest', (tester) async {
    final api = Api();
    await show(tester, api);
    expect(find.text('Stable'), findsOneWidget);
    expect(api.saves, isEmpty);
    expect(api.opens, isEmpty);
    await tester.enterText(find.byKey(const ValueKey('automation-version-name')), 'Release');
    await tester.pump();
    await tap(tester, 'automation-version-save');
    expect(api.saves, [('original', 'b' * 64, 'Release')]);
    expect(find.text('Release'), findsOneWidget);
    expect(api.opens, isEmpty);
  });
  testWidgets('selected saved version opens a distinct project only on explicit action', (tester) async {
    final api = Api();
    AutomationProject? copy;
    await show(tester, api, result: (value) => copy = value);
    await tap(tester, 'automation-version-v1');
    expect(api.opens, isEmpty);
    await tester.enterText(find.byKey(const ValueKey('automation-version-project-name')), 'Reviewed copy');
    await tester.pump();
    await tap(tester, 'automation-version-open');
    expect(api.opens, [('original', 'v1', 'Reviewed copy')]);
    expect(copy?.id, 'separate');
    expect(project.id, 'original');
    expect(api.saves, isEmpty);
  });
  testWidgets('stale original scope discards a pending version listing', (tester) async {
    final api = Api()..pending = Completer<List<AutomationJson>>();
    var current = true;
    await show(tester, api, current: () => current);
    current = false;
    api.pending!.complete(api.rows);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 50));
    expect(find.byKey(const ValueKey('automation-version-v1')), findsNothing);
    expect(api.saves, isEmpty);
    expect(api.opens, isEmpty);
  });
  testWidgets('malformed record is inert and scaled small version dialog remains scrollable', (tester) async {
    final api = Api();
    api.rows.single['digest'] = 'RAW-DIAGNOSTIC-SECRET';
    await show(tester, api, size: const Size(640, 480), scale: 1.4);
    expect(find.text('RAW-DIAGNOSTIC-SECRET'), findsNothing);
    expect(find.byKey(const ValueKey('automation-version-v1')), findsNothing);
    expect(api.saves, isEmpty);
    expect(api.opens, isEmpty);
    expect(tester.takeException(), isNull);
  });
}
