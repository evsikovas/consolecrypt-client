import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/tcpdump/capture.dart';
import 'package:consolecrypt/tcpdump/capture_run.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _Shell implements TerminalService {
  final pending = Completer<TerminalSessionHandle>();
  final output = StreamController<Uint8List>();
  final events = StreamController<TerminalEvent>();
  final id = TerminalSessionId.generate();
  final commands = <String>[];
  int closed = 0;

  void connect() => pending.complete(TerminalSessionHandle(id: id, output: output.stream, events: events.stream));
  void line(String s) => output.add(Uint8List.fromList(utf8.encode('$s\r\n')));
  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) => pending.future;
  @override
  Future<void> write(TerminalSessionId id, Uint8List data) async => commands.add(utf8.decode(data));
  @override
  Future<void> close(TerminalSessionId id) async {
    closed++;
    unawaited(output.close());
    unawaited(events.close());
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test('decodes directional endpoints, TCP flags, DNS and IP-only ICMP', () {
    final p = CapturePacket.parse(
      '1795500000.123456 eth0 Out IP 192.0.2.1.51111 > 192.0.2.2.443: Flags [S.], seq 0, ack 1, length 0',
    )!;
    expect(p.protocol, 'TCP');
    expect(p.source, '192.0.2.1:51111');
    expect(p.destination, '192.0.2.2:443');
    expect(p.summary, contains('SYN'));
    expect(p.summary, contains('ACK'));
    final d = CapturePacket.parse(
      '1795500000.100001 IP6 2001:db8::1.50001 > 2001:db8::2.53: 42+ AAAA? example.test. (30)',
    )!;
    expect(d.source, '[2001:db8::1]:50001');
    expect(d.protocol, 'DNS');
    expect(d.summary, contains('AAAA?'));
    final ping = CapturePacket.parse(
      '1795500000.100001 lo In IP 127.0.0.1 > 127.0.0.2: ICMP echo request, id 21, seq 1, length 64',
    )!;
    expect(ping.destination, '127.0.0.2');
    expect(ping.protocol, 'ICMP');
    expect(CapturePacket.parse('Last login: 12:00'), isNull);
  });

  test('limits both endpoints; host text cannot introduce commands or filters', () {
    for (final invalid in ['x;id', '10.0.0.1 or tcp', 'host/24', 'a\nid', '-i', r'$(id)', "a'b"]) {
      expect(() => remoteCaptureCommand(invalid), throwsFormatException);
    }
    expect(checkedPeer('[2001:db8::1]'), '2001:db8::1');
    expect(remoteCaptureCommand('db.example.test'), contains(r'host db.example.test and ($localfilter)'));
    final receiver = remoteCaptureCommand(null);
    expect(receiver, contains(r'host $1 and host $3'));
    expect(receiver, contains(r'port $2 and port $4'));
    expect(receiver, isNot(contains('sudo -n')));
  });

  final host = Host.create(name: 'lab', address: '192.0.2.10');
  test('stop before SSH finishes closes the late handle without writing', () async {
    final shell = _Shell(), run = CaptureRun(shell);
    final start = run.startRemote(host, null);
    run.stop();
    shell.connect();
    await start;
    expect(shell.closed, 1);
    expect(shell.commands, isEmpty);
    run.dispose();
  });

  test('missing package offers only the detected manager command, as the actual user', () async {
    final shell = _Shell()..connect(), run = CaptureRun(shell);
    await run.startRemote(host, null);
    expect(run.status, CaptureStatus.connecting, reason: run.error);
    shell.events.add(const TerminalStateChanged(SessionConnectionState.connected));
    await Future<void>.delayed(Duration.zero);
    expect(shell.commands.single, contains('exec sh -c'));
    shell.line('CC_MISSING apk 0');
    await Future<void>.delayed(Duration.zero);
    expect(run.status, CaptureStatus.missing);
    expect(run.installCommand, 'apk add tcpdump');
    expect(shell.closed, 1);
    run.dispose();
  });

  test('streams packets, bounds visible rows and closes the capture on stop', () async {
    final shell = _Shell()..connect(), run = CaptureRun(shell);
    await run.startRemote(host, '192.0.2.20');
    expect(run.status, CaptureStatus.connecting, reason: run.error);
    shell.line('tcpdump: listening on any, link-type LINUX_SLL2');
    for (int i = 0; i < displayLimit + 2; i++) {
      shell.line('1795500000.000001 eth0 In IP 192.0.2.10.51235 > 192.0.2.20.53: UDP, length 6');
    }
    await Future<void>.delayed(Duration.zero);
    expect(run.status, CaptureStatus.listening);
    expect(run.packets.length, displayLimit);
    expect(run.count, displayLimit + 2);
    run.stop();
    expect(shell.closed, 1);
    run.dispose();
  });
  testWidgets('choose a saved server, read a live packet, leave the workspace', (tester) async {
    final backend = testBackend(), shell = _Shell()..connect();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, overrides: [terminalServiceProvider.overrideWithValue(shell)]);
    final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
    container.read(routerProvider).go(AppRoutes.tcpdump);
    await settle(tester);
    await tester.tap(find.text('Choose server'));
    await settle(tester);
    await tester.tap(find.descendant(of: find.byType(HostPickerDialog), matching: find.byType(ListTile)).first);
    await settle(tester);
    await tester.tap(find.text('Start capture'));
    await settle(tester);
    shell.events.add(const TerminalStateChanged(SessionConnectionState.connected));
    await settle(tester);
    expect(shell.commands, hasLength(1));
    shell.line('tcpdump: listening on eth0');
    shell.line('1795500000.000001 IP 192.0.2.1.51234 > 192.0.2.2.443: Flags [S], seq 2, length 0');
    await settle(tester);
    expect(find.text('192.0.2.1:51234 → 192.0.2.2:443'), findsOneWidget);
    container.read(routerProvider).go(AppRoutes.hosts);
    await settle(tester);
    expect(shell.closed, 1);
    expect(tester.takeException(), isNull);
  });
}
