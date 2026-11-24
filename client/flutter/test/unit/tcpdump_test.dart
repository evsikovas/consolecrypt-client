import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
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

class _Shell implements TerminalService, TcpdumpService {
  final pending = Completer<TerminalSessionHandle>();
  final output = StreamController<Uint8List>();
  final events = StreamController<TerminalEvent>();
  final id = TerminalSessionId.generate();
  final commands = <String>[];
  int closed = 0;

  void connect() => pending.complete(TerminalSessionHandle(id: id, output: output.stream, events: events.stream));
  void line(String s) => output.add(Uint8List.fromList(utf8.encode('$s\r\n')));
  final requests = <({ObjectId source, ObjectId? peer, bool sudo})>[];
  @override
  Future<TerminalSessionHandle> openTcpdump({required ObjectId hostId, ObjectId? peerId, required bool sudo}) {
    requests.add((source: hostId, peer: peerId, sudo: sudo));
    return pending.future;
  }

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

class _InstallHost extends Fake implements TerminalService, TcpdumpService {
  final check = _Shell()..connect(), install = _Shell()..connect();
  final hosts = <ObjectId>[];
  _Shell shell(TerminalSessionId id) => id == check.id ? check : install;
  @override
  Future<TerminalSessionHandle> open({required ObjectId hostId, required TerminalSize size}) {
    hosts.add(hostId);
    return install.pending.future;
  }

  @override
  Future<TerminalSessionHandle> openTcpdump({required ObjectId hostId, ObjectId? peerId, required bool sudo}) {
    hosts.add(hostId);
    return check.openTcpdump(hostId: hostId, peerId: peerId, sudo: sudo);
  }

  @override
  Future<void> write(TerminalSessionId id, Uint8List data) => shell(id).write(id, data);
  @override
  Future<void> close(TerminalSessionId id) => shell(id).close(id);
  @override
  Future<void> resize(TerminalSessionId id, TerminalSize size) async {}
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
    expect((p.interface, p.direction, p.sequence, p.ack, p.length), ('eth0', 'Out', '0', '1', 0));
    final reverse = CapturePacket.parse(
      '1795500001.1 IP 192.0.2.2.443 > 192.0.2.1.51111: Flags [.], ack 1, win 1234, length 0',
    )!;
    expect(p.sharesEndpoints(reverse), isTrue);
    expect(reverse.window, '1234');
    expect(
      p.sharesEndpoints(CapturePacket.parse('1795500001.2 IP 192.0.2.2.442 > 192.0.2.1.51111: Flags [.], length 0')!),
      isFalse,
    );
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

  test('keeps Ethernet and cooked summaries, ARP targets and bounded actual bytes', () {
    final reply = CapturePacket.parse(
      '1795500000.123456 aa:bb:cc:dd:ee:ff > 12:34:56:78:9a:bc, ethertype ARP (0x0806), length 60: Reply 192.0.2.2 is-at aa:bb:cc:dd:ee:ff, length 28',
    )!;
    expect(reply.source, '192.0.2.2');
    expect(reply.appendHex('0x0000:  0001 0800 0604 0002 aabb ccdd eeff c000  ................'), isTrue);
    expect(reply.appendHex('0x0010:  0202 1234 5678 9abc c000 0201            ...4Vx......'), isTrue);
    expect(reply.destination, '192.0.2.1');
    expect(reply.hexDump, contains('12 34 56 78'));
    expect(reply.hexDump, contains('...4Vx'));
    expect(reply.appendHex('0x0000:  ffff'), isFalse);
    final icmp = CapturePacket.parse(
      '1795521787.314386 lo    In  ifindex 1 00:00:00:00:00:00 ethertype IPv4 (0x0800), length 104: 127.0.0.1 > 127.0.0.2: ICMP echo request, id 12, seq 0, length 64',
    )!;
    expect((icmp.interface, icmp.protocol, icmp.source), ('lo', 'ICMP', '127.0.0.1'));
  });
  final host = Host.create(name: 'lab', address: '192.0.2.10');
  final peer = Host.create(name: 'peer', address: '192.0.2.20');
  test('stop before SSH finishes closes the late handle without writing', () async {
    final shell = _Shell(), run = CaptureRun(shell);
    final start = run.startRemote(host, peer.id);
    run.stop();
    shell.connect();
    await start;
    expect(shell.closed, 1);
    expect(shell.commands, isEmpty);
    run.dispose();
  });

  test('missing package offers only the detected manager command, as the actual user', () async {
    final shell = _Shell()..connect(), run = CaptureRun(shell);
    await run.startRemote(host, peer.id);
    expect(run.status, CaptureStatus.connecting, reason: run.error);
    shell.events.add(const TerminalStateChanged(SessionConnectionState.connected));
    await Future<void>.delayed(Duration.zero);
    expect(shell.requests.single, (source: host.id, peer: peer.id, sudo: false));
    expect(shell.commands, isEmpty);
    shell.line('CC_MISSING apk 0');
    await Future<void>.delayed(Duration.zero);
    expect(run.status, CaptureStatus.missing);
    expect(run.installCommand, 'apk add tcpdump');
    expect(shell.closed, 1);
    run.dispose();
  });

  test('checks the selected host without starting a capture', () async {
    final shell = _Shell()..connect(), run = CaptureRun(shell);
    await run.checkRemote(host);
    shell.events.add(const TerminalStateChanged(SessionConnectionState.connected));
    await Future<void>.delayed(Duration.zero);
    expect(shell.requests.single, (source: host.id, peer: null, sudo: false));
    expect(shell.commands, isEmpty);
    shell.line('CC_TCPDUMP_READY');
    await Future<void>.delayed(Duration.zero);
    expect(run.status, CaptureStatus.ready);
    expect(shell.closed, 1);
    run.dispose();
  });

  test('keeps the final permission diagnostic even when the exit event arrives first', () async {
    final shell = _Shell()..connect(), run = CaptureRun(shell);
    await run.startRemote(host, peer.id, privileged: true);
    shell.events.add(const TerminalStateChanged(SessionConnectionState.connected));
    shell.events.add(const TerminalExited(1));
    shell.events.add(const TerminalStateChanged(SessionConnectionState.disconnected));
    await Future<void>.delayed(Duration.zero);
    expect(shell.closed, 0);
    shell.output.add(Uint8List.fromList(utf8.encode('sudo: a pass')));
    shell.output.add(Uint8List.fromList(utf8.encode('word is required')));
    await shell.output.close();
    expect(run.status, CaptureStatus.denied);
    expect(run.error, 'sudo: a password is required');
    expect(shell.closed, 1);
    run.dispose();
  });

  test('retains the exit code and unprefixed shell diagnostic', () async {
    final shell = _Shell()..connect(), run = CaptureRun(shell);
    await run.startRemote(host, peer.id);
    shell.events.add(const TerminalExited(127));
    shell.events.add(const TerminalStateChanged(SessionConnectionState.disconnected));
    shell.line('sh: tcpdump: not found');
    await shell.output.close();
    await Future<void>.delayed(Duration.zero);
    expect(run.status, CaptureStatus.failed);
    expect(run.error, 'sh: tcpdump: not found');
    expect(run.exitCode, 127);
    run.dispose();
  });

  test('streams packets, bounds visible rows and closes the capture on stop', () async {
    final shell = _Shell()..connect(), run = CaptureRun(shell);
    await run.startRemote(host, peer.id);
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
  testWidgets('installs on the selected capture host only after confirmation', (tester) async {
    final backend = testBackend(), service = _InstallHost();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(tester, backend, overrides: [terminalServiceProvider.overrideWithValue(service)]);
    final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
    container.read(routerProvider).go(AppRoutes.tcpdump);
    await settle(tester);
    await tester.tap(find.text('Capture server'));
    await settle(tester);
    await tester.tap(find.descendant(of: find.byType(HostPickerDialog), matching: find.byType(ListTile)).first);
    await settle(tester);
    await tester.tap(find.text('Check tcpdump'));
    await settle(tester);
    service.check.events.add(const TerminalStateChanged(SessionConnectionState.connected));
    service.check.line('CC_MISSING apt-get 1001');
    await settle(tester);
    await tester.tap(find.text('Install tcpdump'));
    await settle(tester);
    expect(find.text('sudo apt-get install tcpdump'), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(service.hosts, hasLength(1));
    await tester.tap(find.text('Install tcpdump'));
    await settle(tester);
    await tapKey(tester, 'confirm-ok');
    service.install.events.add(const TerminalStateChanged(SessionConnectionState.connected));
    await settle(tester);
    expect(service.hosts, [service.hosts.first, service.hosts.first]);
    expect(service.install.commands, ['sudo apt-get install tcpdump\r']);
    expect(container.read(routerProvider).state.uri.path, AppRoutes.terminal);
    expect(tester.takeException(), isNull);
  });

  testWidgets('choose a saved server, read a live packet, leave the workspace', (tester) async {
    final backend = testBackend(), shell = _Shell()..connect();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(
      tester,
      backend,
      size: AppPlatform.isMobile ? const Size(390, 844) : const Size(1200, 900),
      overrides: [terminalServiceProvider.overrideWithValue(shell)],
    );
    final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
    container.read(routerProvider).go(AppRoutes.tcpdump);
    await settle(tester);
    expect(find.textContaining('Computer ↔ server'), findsNothing);
    await tester.tap(find.text('Capture server'));
    await settle(tester);
    await tester.tap(find.descendant(of: find.byType(HostPickerDialog), matching: find.byType(ListTile)).first);
    await settle(tester);
    await tester.tap(find.text('Peer server'));
    await settle(tester);
    await tester.tap(find.descendant(of: find.byType(HostPickerDialog), matching: find.byType(ListTile)).first);
    await settle(tester);
    final swap = find.byKey(const ValueKey('tcpdump-swap'));
    String hostLabel(String key) =>
        tester.widget<Text>(find.descendant(of: find.byKey(ValueKey(key)), matching: find.byType(Text))).data!;
    final originalSource = hostLabel('tcpdump-source'), originalPeer = hostLabel('tcpdump-peer');
    await tester.tap(swap);
    await settle(tester);
    expect(hostLabel('tcpdump-source'), originalPeer);
    expect(hostLabel('tcpdump-peer'), originalSource);
    await tester.tap(find.byTooltip('Run capture with sudo'));
    await tester.tap(find.text('Start capture'));
    await settle(tester);
    shell.events.add(const TerminalStateChanged(SessionConnectionState.connected));
    await settle(tester);
    expect(shell.requests.single.peer, isNotNull);
    expect(shell.commands, isEmpty);
    shell.line('CC_TCPDUMP_SUDO_PASSWORD');
    await settle(tester);
    expect(find.text('sudo password'), findsOneWidget);
    final field = find.byWidgetPredicate((w) => w is TextField && w.obscureText);
    await tester.enterText(field, 'fixture input');
    await tester.tap(find.text('Continue').last);
    await settle(tester);
    expect(shell.commands, ['fixture input\n']);
    shell.line('CC_TCPDUMP_SUDO_PASSWORD');
    await settle(tester);
    expect(find.text('sudo password'), findsOneWidget);
    await tester.enterText(field, 'fixture retry');
    await tester.tap(find.text('Continue').last);
    await settle(tester);
    expect(shell.commands.last, 'fixture retry\n');
    shell.line('tcpdump: listening on eth0');
    for (var i = 0; i < 100; i++) {
      shell.line('1795500000.000001 IP 192.0.2.3.40000 > 192.0.2.4.80: Flags [.], win 2, length 0');
    }
    shell.line('1795500000.000001 IP 192.0.2.2.443 > 192.0.2.1.51234: Flags [.], ack 3, length 0');
    shell.line('1795500000.000001 IP 192.0.2.1.51234 > 192.0.2.2.443: Flags [S], seq 2, length 0');
    await settle(tester);
    expect(find.text('192.0.2.1:51234 → 192.0.2.2:443'), findsOneWidget);
    final list = find.byKey(const PageStorageKey('tcpdump-packets'));
    final controls = find.text('Check tcpdump');
    final anchor = tester.getTopLeft(controls);
    await tester.tap(find.text('192.0.2.1:51234 → 192.0.2.2:443'));
    await settle(tester);
    expect(find.text('Connection details'), findsOneWidget);
    expect(find.text('seq 2'), findsOneWidget);
    expect(find.text('Packets between these endpoints when selected: 2'), findsOneWidget);
    final pinned = tester.getTopLeft(find.text('Connection details'));
    shell.line('1795500000.000002 IP 192.0.2.7.40000 > 192.0.2.4.80: Flags [.], win 2, length 0');
    await settle(tester);
    expect(find.textContaining('192.0.2.7:40000'), findsNothing);
    await tester.drag(list, const Offset(0, -260));
    await settle(tester);
    expect(tester.getTopLeft(find.text('Connection details')), pinned);
    await tester.tap(find.byTooltip('Back'));
    await settle(tester);
    await tester.drag(list, const Offset(0, -420));
    await settle(tester);
    expect(tester.getTopLeft(controls), anchor);
    container.read(routerProvider).go(AppRoutes.hosts);
    await settle(tester);
    expect(shell.closed, 1);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.android, TargetPlatform.iOS}));
}
