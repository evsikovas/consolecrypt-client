import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/l10n/app_localizations.dart';
import 'package:consolecrypt/monitoring/agent_reader_dialog.dart';
import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_workspace.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../support/agent_reader_fixture.dart' as f;

Widget host(
  f.Port port,
  f.Files files, {
  bool Function()? original,
  Locale locale = const Locale('en'),
  double scale = 1,
  double keyboard = 0,
}) => MaterialApp(
  locale: locale,
  supportedLocales: AppLocalizations.supportedLocales,
  localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
  builder: (context, child) => MediaQuery(
    data: MediaQuery.of(context).copyWith(
      textScaler: TextScaler.linear(scale),
      viewInsets: EdgeInsets.only(bottom: keyboard),
    ),
    child: child!,
  ),
  home: Scaffold(
    body: AgentReaderDialog(
      key: const ValueKey('owned-reader'),
      port: port,
      profileId: f.profile,
      notificationScopeId: port.selection.notificationScopeId,
      sources: [f.source],
      bindings: [f.binding],
      hosts: [f.host],
      knownHosts: [f.known],
      files: files,
      original: original ?? () => true,
    ),
  ),
);
Future<void> tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.pumpAndSettle();
  await tester.tap(finder.hitTestable());
  await tester.pumpAndSettle();
}

Future<void> select(WidgetTester tester, AgentReaderAction action) async {
  final selectors = find.byType(GlassSelect<String?>);
  tester.widget<GlassSelect<String?>>(selectors.at(0)).onChanged!(f.bindingId);
  await tester.pumpAndSettle();
  tester.widget<GlassSelect<String?>>(selectors.at(1)).onChanged!(f.knownId);
  await tester.pumpAndSettle();
  tester.widget<GlassSelect<AgentReaderAction>>(find.byType(GlassSelect<AgentReaderAction>)).onChanged!(action);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('opening is metadata-only; saved references require explicit selection and exact confirm', (
    tester,
  ) async {
    final port = f.Port(f.selection()), files = f.Files();
    await tester.pumpWidget(host(port, files));
    await tester.pumpAndSettle();
    expect(port.requests, isEmpty);
    expect(port.statuses, isEmpty);
    expect(files.opens, 0);
    expect(tester.widget<FilledButton>(find.byKey(const ValueKey('agent-prepare'))).onPressed, isNull);
    await select(tester, AgentReaderAction.issuerInit);
    await tap(tester, 'agent-prepare');
    expect(port.requests, hasLength(1));
    expect(port.handles.single.confirms, 0);
    expect(find.byKey(const ValueKey('agent-review-details')), findsOneWidget);
    expect(find.textContaining('original opaque unlock'), findsWidgets);
    expect(find.textContaining('PRIVATE'), findsNothing);
    await tap(tester, 'agent-confirm');
    expect(port.handles.single.confirms, 1);
    expect(find.byKey(const ValueKey('agent-confirm')), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Node offer uses only explicit public file pickers; exports never show token or temporary paths', (
    tester,
  ) async {
    final port = f.Port(f.selection()), files = f.Files();
    await tester.pumpWidget(host(port, files));
    await tester.pumpAndSettle();
    await select(tester, AgentReaderAction.nodeOffer);
    await tap(tester, 'agent-file-ownerPinFile');
    await tap(tester, 'agent-file-nodeHelloFile');
    expect(files.opens, 2);
    expect(port.requests, isEmpty);
    await tap(tester, 'agent-prepare');
    expect(port.requests, hasLength(1));
    await tap(tester, 'agent-confirm');
    expect(find.textContaining('PRIVATE_TOKEN'), findsNothing);
    expect(find.textContaining('OWNED_TEMP'), findsNothing);
    expect(port.handles.single.cancels, 0);
    await tap(tester, 'agent-transfer');
    expect(files.saves, 2);
    expect(port.handles.single.cancels, 0);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    expect(port.handles.single.cancels, 1);
  });
  testWidgets('same-key service replacement cancels only late original handle before approval', (tester) async {
    final port = f.Port(f.selection())..pending = Completer(), files = f.Files();
    await tester.pumpWidget(host(port, files));
    await tester.pumpAndSettle();
    await select(tester, AgentReaderAction.issuerInit);
    await tester.tap(find.byKey(const ValueKey('agent-prepare')));
    await tester.pump();
    final original = port.handles.single;
    final replacement = f.Port(f.selection());
    await tester.pumpWidget(host(replacement, files));
    await tester.pump();
    port.pending!.complete(original);
    await tester.pumpAndSettle();
    expect(original.cancels, 1);
    expect(original.confirms, 0);
    expect(replacement.requests, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Native local association choices are selected explicitly; read then consume remains one-use', (
    tester,
  ) async {
    final port = f.Port(f.selection()), files = f.Files();
    final status = f.statusJson(port.selection);
    (status['associationChoices'] as Json)['items'] = [f.choice()];
    port.statusResponse = status;
    await tester.pumpWidget(host(port, files));
    await tester.pumpAndSettle();
    await select(tester, AgentReaderAction.read);
    await tap(tester, 'agent-status');
    expect(port.requests, isEmpty);
    expect(tester.widget<FilledButton>(find.byKey(const ValueKey('agent-prepare'))).onPressed, isNull);
    tester.widget<GlassSelect<int?>>(find.byType(GlassSelect<int?>)).onChanged!(0);
    await tester.pumpAndSettle();
    await tap(tester, 'agent-prepare');
    await tap(tester, 'agent-confirm');
    expect(port.handles.single.reads, 0);
    await tap(tester, 'agent-consume');
    expect(port.handles.single.reads, 1);
    expect(find.textContaining('42.0'), findsOneWidget);
    expect(find.textContaining('clock offset is unknown'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('unknown confirm is sticky and does not invoke another prepare or expose exception prose', (
    tester,
  ) async {
    final port = f.Port(f.selection())..loseConfirm = true;
    await tester.pumpWidget(host(port, f.Files()));
    await tester.pumpAndSettle();
    await select(tester, AgentReaderAction.issuerInit);
    await tap(tester, 'agent-prepare');
    await tap(tester, 'agent-confirm');
    expect(port.requests, hasLength(1));
    expect(port.handles.single.confirms, 1);
    expect(find.byKey(const ValueKey('agent-prepare')), findsNothing);
    expect(find.textContaining('NO_RAW_ERROR'), findsNothing);
    expect(find.textContaining('result is unknown'), findsOneWidget);
  });
  testWidgets('picker pending across original scope retirement never prepares Native work', (tester) async {
    var original = true;
    final port = f.Port(f.selection()), files = f.Files()..pending = Completer();
    await tester.pumpWidget(host(port, files, original: () => original));
    await tester.pumpAndSettle();
    await select(tester, AgentReaderAction.nodeOffer);
    await tester.tap(find.byKey(const ValueKey('agent-file-ownerPinFile')));
    await tester.pump();
    original = false;
    await tester.pumpWidget(const SizedBox.shrink());
    files.pending!.complete('C:/OWNED_PUBLIC/late.json');
    await tester.pumpAndSettle();
    expect(port.requests, isEmpty);
    expect(port.handles, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets(
    'actual Workspace source/binding callback retires after saved revision replacement; ordinary reads unchanged',
    (tester) async {
      final store = MonitoringBindingTestStore(initial: [f.source], rows: [f.binding]),
          service = MonitoringTestService();
      bool Function()? original;
      await tester.pumpWidget(
        MaterialApp(
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(
            body: MonitoringWorkspace(
              service: service,
              store: store,
              openAgentReader: (sources, bindings, current) async {
                expect(sources.single.id, f.sourceId);
                expect(bindings.single.revision, 7);
                original = current;
              },
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tap(tester, 'monitoring-agent-reader-open');
      expect(original!(), isTrue);
      expect(service.queries, 0);
      store.bindings = [
        MonitoringBinding(
          id: f.bindingId,
          name: 'new revision',
          hostId: f.hostId,
          sourceId: f.sourceId,
          revision: 8,
          sourceRevision: 3,
          selector: {'instance': 'owned'},
          state: MonitoringBindingState.current,
        ),
      ];
      expect(original!(), isFalse);
      expect(service.queries, 0);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('RU420 keyboard initial and exact review layouts stay bounded at 1.5 scale', (tester) async {
    tester.view.physicalSize = const Size(420, 760);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final port = f.Port(f.selection());
    await tester.pumpWidget(host(port, f.Files(), locale: const Locale('ru'), scale: 1.5, keyboard: 220));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    await select(tester, AgentReaderAction.issuerInit);
    await tap(tester, 'agent-prepare');
    expect(find.byKey(const ValueKey('agent-confirm')), findsOneWidget);
    expect(tester.takeException(), isNull);
    expect(port.handles.single.confirms, 0);
  });
}
