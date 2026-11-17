import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/agent_setup_controller.dart';
import 'package:consolecrypt/monitoring/agent_setup_models.dart';
import 'package:consolecrypt/monitoring/agent_setup_wizard.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../support/agent_setup_fixture.dart' as f;

Future<void> pumpWizard(
  WidgetTester tester,
  f.SetupPort port, {
  double scale = 1,
  ValueChanged<AgentSetupVerification>? onGraphs,
}) async {
  await loadAutomationProofFonts(tester);
  await tester.pumpWidget(
    MaterialApp(
      locale: const Locale('ru'),
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
        child: child!,
      ),
      home: RepaintBoundary(
        key: const ValueKey('agent-setup-proof'),
        child: Scaffold(
          body: AgentSetupWizard(port: port, isCurrent: () => true, onOpenGraphs: onGraphs ?? (_) {}),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> capture(WidgetTester tester, String name) async {
  final directory = Platform.environment['CONSOLECRYPT_AGENT_SETUP_PROOF_DIR'];
  if (directory == null) return;
  final boundary = tester.renderObject<RenderRepaintBoundary>(find.byKey(const ValueKey('agent-setup-proof')));
  await tester.runAsync(() async {
    final image = await boundary.toImage();
    try {
      final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
      final file = File('$directory/$name.png');
      await file.parent.create(recursive: true);
      await file.writeAsBytes(bytes!.buffer.asUint8List());
    } finally {
      image.dispose();
    }
  });
}

void main() {
  for (final (size, scale) in [(const Size(900, 700), 1.0), (const Size(640, 480), 1.4)]) {
    testWidgets('actual Agent preflight is understandable and truthful at $size scale$scale', (tester) async {
      tester.view.physicalSize = size;
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      final port = f.SetupPort()
        ..releaseAvailable = false
        ..destinationAvailable = false;
      await pumpWizard(tester, port, scale: scale);
      expect(find.text('Настройка ConsoleCrypt Agent'), findsOneWidget);
      expect(find.text('Показатели Linux-сервера'), findsOneWidget);
      expect(find.byType(TextField), findsNothing, reason: 'No ZIP/key/UID/JSON entry');
      await capture(tester, 'selection-${size.width.toInt()}');
      await tester.tap(find.byKey(const ValueKey('agent-setup-prepare')));
      await tester.pumpAndSettle();
      expect(find.textContaining('В этой поставке ConsoleCrypt нет проверенного пакета Linux Agent.'), findsOneWidget);
      expect(tester.widget<FilledButton>(find.byKey(const ValueKey('agent-setup-install'))).onPressed, isNull);
      expect(find.byKey(const ValueKey('agent-setup-open-graphs')), findsNothing);
      expect(port.operations.single.calls, isEmpty);
      expect(find.textContaining('playbook'), findsNothing);
      expect(find.textContaining('Prometheus'), findsNothing);
      expect(tester.takeException(), isNull);
      await capture(tester, 'blocked-review-${size.width.toInt()}');
    });
  }

  testWidgets('owner and collection steps require explicit consent and graphs require observed data', (tester) async {
    tester.view.physicalSize = const Size(1100, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final port = f.SetupPort();
    AgentSetupVerification? opened;
    await pumpWizard(tester, port, onGraphs: (value) => opened = value);
    Future<void> tap(String key) async {
      await tester.tap(find.byKey(ValueKey(key)));
      await tester.pumpAndSettle();
    }

    await tap('agent-setup-prepare');
    await tap('agent-setup-install');
    final operation = port.operations.single;
    expect(operation.pairing.offers, 0);
    await tap('agent-setup-connect');
    expect(operation.pairing.certificates, 0);
    await tap('agent-setup-confirm-connection');
    expect(operation.collection.confirms, 0);
    await tap('agent-setup-enable-collection');
    expect(find.byKey(const ValueKey('agent-setup-open-graphs')), findsNothing);
    expect(opened, isNull);
    operation.ready = true;
    await tap('agent-setup-verify');
    expect(find.byKey(const ValueKey('agent-setup-open-graphs')), findsOneWidget);
    expect(opened, isNull);
    await tap('agent-setup-open-graphs');
    expect(opened?.graphs?.querySourceId, f.destination.querySourceId);
    expect(tester.takeException(), isNull);
  });

  testWidgets('busy installation can be stopped and unknown cleanup does not close the wizard', (tester) async {
    final port = f.SetupPort();
    await pumpWizard(tester, port);
    await tester.tap(find.byKey(const ValueKey('agent-setup-prepare')));
    await tester.pumpAndSettle();
    final operation = port.operations.single
      ..pendingInstall = Completer<AgentSoftwareEvidence>()
      ..pendingCleanup = Completer<AgentSetupCleanup>();
    await tester.tap(find.byKey(const ValueKey('agent-setup-install')));
    await tester.pump();
    expect(tester.widget<TextButton>(find.byKey(const ValueKey('agent-setup-close'))).onPressed, isNotNull);
    await tester.tap(find.byKey(const ValueKey('agent-setup-close')));
    await tester.pump();
    expect(find.text('Останавливаем…'), findsWidgets);
    operation.pendingCleanup!.complete(const AgentSetupCleanup(closedObserved: false));
    operation.pendingInstall!.complete(operation.software());
    for (var i = 0; i < 10; i++) {
      await tester.pump(const Duration(milliseconds: 100));
    }
    final owner =
        tester
                .widget<AnimatedBuilder>(
                  find.byWidgetPredicate(
                    (widget) => widget is AnimatedBuilder && widget.animation is AgentSetupController,
                  ),
                )
                .animation
            as AgentSetupController;
    expect(owner.stopping, false, reason: operation.calls.join(','));
    expect(owner.busy, false);
    expect(find.textContaining('Не удалось подтвердить завершение операции.'), findsOneWidget);
    expect(find.byKey(const ValueKey('agent-setup-wizard')), findsOneWidget);
    expect(operation.pairing.offers, 0);
    expect(find.byKey(const ValueKey('agent-setup-open-graphs')), findsNothing);
  });
}
