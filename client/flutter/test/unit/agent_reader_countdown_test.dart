import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_reader_controller.dart';
import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_reader_fixture.dart' as f;

void main() {
  testWidgets('original local deadline ticks, expires and cannot confirm or perform hidden RPC', (tester) async {
    final start = tester.binding.clock.now(), port = f.Port(f.selection());
    final owner = AgentReaderController(
      port: port,
      selection: f.selection(),
      isOriginal: () => true,
      elapsedForReview: () => tester.binding.clock.now().difference(start),
    );
    await owner.prepare(AgentReaderRequest.issuer());
    expect(owner.remainingSeconds, 59);
    await tester.pump(const Duration(seconds: 1));
    expect(owner.remainingSeconds, 58);
    expect(owner.canConfirm, isTrue);
    await tester.pump(const Duration(seconds: 58));
    expect(owner.review, isNull);
    expect(owner.failure, 'reader_expired');
    expect(owner.canConfirm, isFalse);
    await owner.confirm();
    expect(port.handles.single.confirms, 0);
    expect(port.handles.single.cancels, 1);
    expect(port.requests, hasLength(1));
    expect(port.statuses, isEmpty);
    owner.dispose();
  });
  testWidgets('deadline while confirm awaits cannot cancel original or replace its actual result', (tester) async {
    final start = tester.binding.clock.now(), port = f.Port(f.selection());
    final owner = AgentReaderController(
      port: port,
      selection: f.selection(),
      isOriginal: () => true,
      elapsedForReview: () => tester.binding.clock.now().difference(start),
    );
    await owner.prepare(AgentReaderRequest.issuer());
    final handle = port.handles.single..waitingConfirm = Completer<String>();
    final pending = owner.confirm();
    expect(handle.confirms, 1);
    await tester.pump(const Duration(seconds: 60));
    expect(owner.busy, isTrue);
    expect(owner.remainingSeconds, 0);
    expect(handle.cancels, 0);
    expect(owner.failure, isNull);
    handle.waitingConfirm!.complete(jsonEncode(f.result(AgentReaderAction.issuerInit)));
    await pending;
    expect(owner.outcome?.kind, 'issuerReady');
    expect(owner.failure, isNull);
    await owner.confirm();
    expect(handle.confirms, 1);
    owner.dispose();
  });
  testWidgets('cancel retirement and disposal remove their timers without late notifications or RPC', (tester) async {
    for (final action in ['cancel', 'retire', 'dispose']) {
      final start = tester.binding.clock.now(), port = f.Port(f.selection());
      final owner = AgentReaderController(
        port: port,
        selection: f.selection(),
        isOriginal: () => true,
        elapsedForReview: () => tester.binding.clock.now().difference(start),
      );
      var notifications = 0;
      owner.addListener(() => notifications++);
      await owner.prepare(AgentReaderRequest.issuer());
      switch (action) {
        case 'cancel':
          owner.cancel();
        case 'retire':
          owner.retire();
        case 'dispose':
          owner.dispose();
      }
      final stopped = notifications;
      await tester.pump(const Duration(seconds: 61));
      expect(notifications, stopped);
      expect(port.handles.single.cancels, 1);
      expect(port.handles.single.confirms, 0);
      expect(port.requests, hasLength(1));
      expect(port.statuses, isEmpty);
      if (action != 'dispose') owner.dispose();
    }
  });
}
