import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_awx_controller.dart';
import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/automation/automation_awx_workspace.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/awx_copy.dart';
import '../unit/automation_awx_controller_test.dart' as f;
import '../unit/automation_awx_models_test.dart' as m;

Future<void> open(
  WidgetTester tester,
  AwxController c, {
  double width = 1200,
  bool russian = false,
  double scale = 1,
}) async {
  tester.view.physicalSize = Size(width, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.dark(platform: TargetPlatform.windows),
      locale: Locale(russian ? 'ru' : 'en'),
      supportedLocales: const [Locale('en'), Locale('ru')],
      localizationsDelegates: GlobalMaterialLocalizations.delegates,
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context)
            .copyWith(textScaler: TextScaler.linear(scale)),
        child: child!,
      ),
      home: Scaffold(
        body: RepaintBoundary(
          key: const ValueKey('automation-proof-root'),
          child: AwxWorkspace(
            controller: c,
            copy: (key) => awxCopy(key, russian: russian),
            onEditConnection: (_) async => null,
            onOpenResource: (_) async {},
          ),
        ),
      ),
    ),
  );
  await settle(tester);
}

Future<void> settle(WidgetTester tester) async {
  for (var i = 0; i < 12; i++) {
    await tester.pump(const Duration(milliseconds: 40));
  }
}

Future<void> tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await settle(tester);
  expect(finder.hitTestable(), findsOneWidget);
  await tester.tap(finder);
  await settle(tester);
}

Future<void> prepared(WidgetTester tester) async {
  await tap(tester, 'awx-catalog-refresh');
  await tap(tester, 'awx-connection-${m.connectionId}');
  await tap(tester, 'awx-read-templates');
  await tap(tester, 'awx-template-9');
  await tap(tester, 'awx-prepare');
}

void main() {
  testWidgets(
    'open/catalog/template reads have no launch and only explicit review enables launch',
    (tester) async {
      final hub = RuntimeNoticeEventAdapter(),
          native = f.Native(),
          c = f.controller(hub, native);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await open(tester, c);
      expect(native.calls, isEmpty);
      expect(find.byKey(const ValueKey('awx-launch')), findsNothing);
      await prepared(tester);
      expect(native.calls, ['catalog', 'templates', 'prepare']);
      expect(find.text('{}'), findsOneWidget);
      expect(find.text('Synthetic template · #9'), findsNWidgets(2));
      expect(find.text('https://awx.example.test/controller'), findsOneWidget);
      await tap(tester, 'awx-launch');
      expect(native.calls.where((v) => v == 'launch').length, 1);
      expect(find.byKey(const ValueKey('awx-unknown-outcome')), findsOneWidget);
      expect(find.byKey(const ValueKey('awx-launch')), findsNothing);
    },
  );
  testWidgets(
    'invalid manual JSON immediately clears review and cannot reuse old parameters',
    (tester) async {
      final hub = RuntimeNoticeEventAdapter(),
          native = f.Native(),
          c = f.controller(hub, native);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await open(tester, c);
      await prepared(tester);
      final field = find.widgetWithText(TextField, awxCopy(AwxCopy.extraVars));
      await tester.ensureVisible(field);
      await tester.enterText(field, r'{"x":1,"x":2}');
      await settle(tester);
      expect(c.review, isNull);
      expect(c.canPrepare, false);
      expect(c.canLaunch, false);
      expect(
        find.byKey(const ValueKey('awx-invalid-parameters')),
        findsOneWidget,
      );
      expect(native.calls.contains('launch'), false);
      await tester.enterText(field, '{}');
      await settle(tester);
      expect(c.canPrepare, true);
    },
  );
  testWidgets(
    'pending prepare remains cancelable and oldscope result is discarded after epoch loss',
    (tester) async {
      final hub = RuntimeNoticeEventAdapter(),
          native = f.Native(),
          c = f.controller(hub, native);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await open(tester, c);
      await tap(tester, 'awx-catalog-refresh');
      await tap(tester, 'awx-connection-${m.connectionId}');
      await tap(tester, 'awx-read-templates');
      await tap(tester, 'awx-template-9');
      final op = f.Op(m.review(), auto: false)..ack.complete();
      native.prepareWait = op;
      await tap(tester, 'awx-prepare');
      expect(c.busy, true);
      expect(op.starts, 1);
      hub.boundary();
      await settle(tester);
      op.done.complete(op.value);
      op.join.complete();
      await settle(tester);
      expect(c.current, false);
      expect(find.byKey(const ValueKey('awx-launch')), findsNothing);
      expect(native.discards, ['synthetic-one-use']);
      expect(native.calls.contains('launch'), false);
    },
  );
  testWidgets(
    'delete cancel keeps connection and explicit confirmation uses captured row',
    (tester) async {
      final hub = RuntimeNoticeEventAdapter(),
          native = f.Native(),
          c = f.controller(hub, native);
      addTearDown(hub.dispose);
      addTearDown(c.dispose);
      await open(tester, c);
      await tap(tester, 'awx-catalog-refresh');
      await tap(tester, 'awx-connection-${m.connectionId}');
      final delete = find.widgetWithText(
        GlassButton,
        awxCopy(AwxCopy.deleteConnection),
      );
      await tester.ensureVisible(delete);
      await tester.tap(delete);
      await settle(tester);
      await tester.tap(
        find.widgetWithText(GlassButton, awxCopy(AwxCopy.cancel)),
      );
      await settle(tester);
      expect(native.calls.contains('delete'), false);
      await tester.tap(delete);
      await settle(tester);
      await tester.tap(
        find.widgetWithText(GlassButton, awxCopy(AwxCopy.confirmDelete)),
      );
      await settle(tester);
      expect(native.calls.where((v) => v == 'delete').length, 1);
    },
  );
  testWidgets('profile retirement while delete dialog open sends no delete', (
    tester,
  ) async {
    final hub = RuntimeNoticeEventAdapter(),
        native = f.Native(),
        c = f.controller(hub, native);
    addTearDown(hub.dispose);
    addTearDown(c.dispose);
    await open(tester, c);
    await tap(tester, 'awx-catalog-refresh');
    await tap(tester, 'awx-connection-${m.connectionId}');
    final delete = find.widgetWithText(
      GlassButton,
      awxCopy(AwxCopy.deleteConnection),
    );
    await tester.ensureVisible(delete);
    await tester.tap(delete);
    await settle(tester);
    hub.boundary();
    await settle(tester);
    await tester.tap(
      find.widgetWithText(GlassButton, awxCopy(AwxCopy.confirmDelete)),
    );
    await settle(tester);
    expect(native.calls.contains('delete'), false);
    expect(c.catalog, isNull);
  });
  for (final width in [1200.0, 420.0]) {
    testWidgets(
      'RU exact review layout ${width.toInt()} preserves readable launch and bounded lists',
      (tester) async {
        final hub = RuntimeNoticeEventAdapter(),
            native = f.Native(),
            c = f.controller(hub, native);
        addTearDown(hub.dispose);
        addTearDown(c.dispose);
        await f.selectTemplate(c);
        c.editParameters(
          AwxLaunchParameters(
            jobType: 'check',
            limit: 'synthetic-group',
            extraVars: {'synthetic': true},
          ),
        );
        await c.prepareLaunch();
        await open(
          tester,
          c,
          width: width,
          russian: true,
          scale: width < 500 ? 1.5 : 1,
        );
        final limitField = find.widgetWithText(
          TextField,
          awxCopy(AwxCopy.limit, russian: true),
        );
        expect(
          tester.widget<TextField>(limitField).controller!.text,
          'synthetic-group',
        );
        final varsField = find.widgetWithText(
          TextField,
          awxCopy(AwxCopy.extraVars, russian: true),
        );
        expect(
          tester.widget<TextField>(varsField).controller!.text,
          '{"synthetic":true}',
        );
        expect(
          find.text(awxCopy(AwxCopy.check, russian: true)),
          findsOneWidget,
        );
        expect(tester.takeException(), isNull);
        await tester.ensureVisible(find.byKey(const ValueKey('awx-launch')));
        await settle(tester);
        expect(
          find.byKey(const ValueKey('awx-launch')).hitTestable(),
          findsOneWidget,
        );
        expect(tester.takeException(), isNull);
        expect(native.calls.contains('launch'), false);
        await captureAutomationWorkspace(
          tester,
          'awx-review-ru-${width.toInt()}',
        );
      },
    );
  }
}
