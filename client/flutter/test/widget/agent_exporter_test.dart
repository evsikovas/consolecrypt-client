import 'dart:async';

import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_association_dialog.dart';
import 'package:consolecrypt/monitoring/agent_exporter_dialog.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_association_fixture.dart' as old;
import '../support/agent_exporter_fixture.dart' as f;

Widget host(Widget child, {Locale locale = const Locale('en'), double scale = 1}) => MaterialApp(
  locale: locale,
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
    child: child!,
  ),
  home: Scaffold(body: child),
);
Widget dialog(f.Calls c, ValueNotifier<bool> live) => AgentExporterDialog(
  port: c.port,
  scope: f.scope,
  record: f.record(),
  original: () => live.value,
  originalChanges: live,
  now: () => f.now,
);
Future<void> tap(WidgetTester t, String key) async {
  final finder = find.byKey(ValueKey(key));
  await t.ensureVisible(finder);
  await t.tap(finder);
  await t.pump();
  await t.pump(const Duration(milliseconds: 30));
}

void main() {
  testWidgets('mount is inert; prerequisites and review are separate from one-use start; Stop never claims stopped', (
    t,
  ) async {
    final c = f.Calls(), live = ValueNotifier(true);
    await t.pumpWidget(host(dialog(c, live)));
    expect(c.requests, isEmpty);
    await tap(t, 'agent-exporter-query');
    expect(c.requests.keys, ['catalog']);
    expect(c.raw.calls, isEmpty);
    await tap(t, 'agent-exporter-prepare');
    expect(c.raw.calls, ['details']);
    await tap(t, 'agent-exporter-approve');
    expect(c.raw.calls, contains('approve'));
    await tap(t, 'agent-exporter-stop');
    expect(c.raw.calls.where((x) => x == 'stop').length, 1);
    final l = AppLocalizations.of(t.element(find.byType(AgentExporterDialog)));
    expect(find.text(l.agentExporterStopRequested), findsOneWidget);
    expect(find.text(l.agentExporterStopped), findsNothing);
    await t.pumpWidget(const SizedBox());
    await t.pump();
    live.dispose();
  });
  testWidgets('RU narrow review keeps original expiry and finite nonroot disclosure visible without raw scope', (
    t,
  ) async {
    t.view.physicalSize = const Size(600, 800);
    t.view.devicePixelRatio = 1;
    addTearDown(t.view.reset);
    final c = f.Calls(), live = ValueNotifier(true);
    await t.pumpWidget(host(dialog(c, live), locale: const Locale('ru'), scale: 1.3));
    await tap(t, 'agent-exporter-query');
    await tap(t, 'agent-exporter-prepare');
    final l = AppLocalizations.of(t.element(find.byType(AgentExporterDialog)));
    expect(find.textContaining('127.0.0.1:19100'), findsOneWidget);
    expect(find.text(l.agentExporterFiniteDisclosure), findsOneWidget);
    expect(find.text('exporters.manage'), findsNothing);
    expect(c.raw.calls, ['details']);
    expect(t.takeException(), isNull);
    await t.pumpWidget(const SizedBox());
    await t.pump();
    live.dispose();
  });
  testWidgets('owner change while prepare is pending joins only late original and never starts', (t) async {
    final c = f.Calls(), live = ValueNotifier(true);
    c.pending = Completer();
    await t.pumpWidget(host(dialog(c, live)));
    await tap(t, 'agent-exporter-query');
    await tap(t, 'agent-exporter-prepare');
    live.value = false;
    c.pending!.complete(c.raw);
    await t.pump();
    await t.pump(const Duration(milliseconds: 30));
    expect(c.raw.calls, ['cancel', 'joined', 'dispose']);
    expect(c.requests['prepare']!.length, 1);
    await t.pumpWidget(const SizedBox());
    await t.pump();
    live.dispose();
  });
  testWidgets('existing Agent entry exposes only explicit optional Exporter enrollment without making exporter RPC', (
    t,
  ) async {
    final c = f.Calls(), base = old.Port();
    await t.pumpWidget(
      host(AgentAssociationDialog(port: base, exporterPort: c.port, scope: old.scope, original: () => true)),
    );
    expect(find.byKey(const ValueKey('agent-exporter-enrollment')), findsOneWidget);
    expect(c.requests, isEmpty);
    await t.pumpWidget(const SizedBox());
    await t.pump();
  });
}
