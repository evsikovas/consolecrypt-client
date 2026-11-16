import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:consolecrypt/monitoring/agent_reader_port.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../support/agent_reader_fixture.dart' as f;
import '../support/agent_reader_history_fixture.dart';
import 'agent_reader_test.dart' as old;

Future<void> ready(WidgetTester tester) async {
  await old.select(tester, AgentReaderAction.read);
  await old.tap(tester, 'agent-status');
  tester.widget<GlassSelect<int?>>(find.byType(GlassSelect<int?>)).onChanged!(0);
  await tester.pumpAndSettle();
  await old.tap(tester, 'agent-prepare');
  await old.tap(tester, 'agent-confirm');
}

void main() {
  testWidgets(
    'availability boundary appears before any RPC and typed read failure has explicit separate-review recovery',
    (tester) async {
      final port = ReadPort()..readFailure = AgentReaderException.fromCode('reader_unavailable');
      await tester.pumpWidget(old.host(port, f.Files()));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('agent-history-availability')), findsOneWidget);
      expect(port.requests, isEmpty);
      expect(port.statuses, isEmpty);
      await ready(tester);
      await old.tap(tester, 'agent-consume');
      expect(find.textContaining('The Agent may be offline'), findsOneWidget);
      expect(find.textContaining('No new dataset was loaded'), findsOneWidget);
      expect(find.byKey(const ValueKey('agent-consume')), findsNothing);
      expect(find.byKey(const ValueKey('agent-new-action')), findsOneWidget);
      await tester.pump(const Duration(seconds: 5));
      await tester.pump();
      expect(port.handles.single.reads, 1);
      expect(port.requests, hasLength(1));
      expect(port.statuses, hasLength(1));
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    },
  );
  testWidgets(
    'latest bounded history page opens once and earlier/later browsing survives timer repaint without another RPC',
    (tester) async {
      final port = ReadPort();
      await tester.pumpWidget(old.host(port, f.Files()));
      await tester.pumpAndSettle();
      await ready(tester);
      await old.tap(tester, 'agent-consume');
      expect(find.text('agent_uptime_seconds: 69.0'), findsOneWidget);
      expect(find.text('agent_uptime_seconds: 0.0'), findsNothing);
      expect(tester.widget<TextButton>(find.byKey(const ValueKey('agent-history-later'))).onPressed, isNull);
      await old.tap(tester, 'agent-history-earlier');
      expect(find.text('agent_uptime_seconds: 32.0'), findsOneWidget);
      expect(find.text('agent_uptime_seconds: 69.0'), findsNothing);
      await tester.pump(const Duration(seconds: 2));
      await tester.pump();
      expect(find.text('agent_uptime_seconds: 32.0'), findsOneWidget);
      await old.tap(tester, 'agent-history-later');
      expect(find.text('agent_uptime_seconds: 69.0'), findsOneWidget);
      expect(port.handles.single.reads, 1);
      expect(port.requests, hasLength(1));
      expect(port.statuses, hasLength(1));
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await tester.pump();
    },
  );
  testWidgets('original scope loss while a read waits hides late history and cancels only the original owner', (
    tester,
  ) async {
    var original = true;
    final port = ReadPort()..pendingRead = Completer<String>();
    await tester.pumpWidget(old.host(port, f.Files(), original: () => original));
    await tester.pumpAndSettle();
    await ready(tester);
    await tester.ensureVisible(find.byKey(const ValueKey('agent-consume')));
    await tester.tap(find.byKey(const ValueKey('agent-consume')));
    await tester.pump();
    original = false;
    port.pendingRead!.complete(port.body);
    await tester.pump();
    await tester.pump();
    expect(find.text('agent_uptime_seconds: 69.0'), findsNothing);
    expect(port.handles.single.cancels, 1);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });
}
