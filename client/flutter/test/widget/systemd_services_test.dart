import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/app/workspace_tools_controller.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/hosts/systemd_journal.dart';
import 'package:consolecrypt/hosts/systemd_services.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

void main() {
  test('parses journald metadata and Docker messages without losing the original record', () {
    const raw = r'level=error msg="[resolver] failed to query external DNS server" error="read udp: i/o timeout"';
    final entry = ServiceLogEntry.parse(
      jsonEncode({
        '__REALTIME_TIMESTAMP': '1600000000123456',
        'PRIORITY': '6',
        'SYSLOG_IDENTIFIER': 'dockerd',
        '_PID': '1522',
        'MESSAGE': raw,
      }),
    );
    expect(entry.level, 3);
    expect(entry.summary, '[resolver] failed to query external DNS server');
    expect(entry.process, 'dockerd[1522]');
    expect(entry.time!.microsecondsSinceEpoch, 1600000000123456);
    expect(entry.raw, raw);
    expect(entry.fields['error'], 'read udp: i/o timeout');
    expect(ServiceLogEntry.parse('{"PRIORITY":"4","MESSAGE":[65,10,66]}').summary, 'A\nB');
    expect(ServiceLogEntry.parse('plain text').summary, 'plain text');
    expect(ServiceLogEntry.parse(jsonEncode({'MESSAGE': '{"level":"debug","message":"result"}'})).level, 7);
  });
  testWidgets('SSH session tools follow the selected host and explain when systemd is unavailable', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(
      tester,
      backend,
      size: AppPlatform.isMobile ? const Size(390, 800) : const Size(1440, 900),
      overrides: [
        systemdRequestProvider.overrideWithValue((host, operation) async {
          expect(operation, {'action': 'list'});
          if (host.name == 'raspberry-pi') {
            return {'status': 0, 'output': 'pi.service loaded active running Pi', 'diagnostic': ''};
          }
          return {'status': 69, 'output': '', 'diagnostic': ''};
        }),
      ],
    );
    await enterKey(tester, 'hosts-search', 'prod-web');
    final entry = AppPlatform.isMobile ? 'terminal-tool-systemd' : 'nav-systemd';
    expect(find.byKey(ValueKey(entry)), findsNothing);
    await tapKey(tester, 'connect-prod-web-1');
    await tapKey(tester, entry);
    expect(find.text('This host does not have a running systemd system manager.'), findsOneWidget);
    expect(find.text('Restart'), findsNothing);
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    final tabs = container.read(terminalTabsProvider.notifier);
    await tabs.open(backend.inventory.currentHosts.firstWhere((h) => h.name == 'raspberry-pi'));
    await settle(tester);
    expect(find.text('pi.service'), findsOneWidget);
    expect(find.text('This host does not have a running systemd system manager.'), findsNothing);
    container.read(routerProvider).go(AppRoutes.hosts);
    await settle(tester);
    expect(container.read(workspaceToolsProvider).selected, isNull);
    expect(find.byType(SystemServicesPanel), findsNothing);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.android, TargetPlatform.iOS}));

  testWidgets('journal follow stays beside its heading while reading a long log', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    await pumpApp(
      tester,
      backend,
      size: AppPlatform.isMobile ? const Size(360, 800) : const Size(1440, 900),
      overrides: [
        systemdRequestProvider.overrideWithValue(
          (_, request) async => {
            'status': 0,
            'output': switch (request['action']) {
              'list' => 'demo.service loaded active running Example service',
              'journal' => List.generate(200, (i) => 'Entry $i').join('\n'),
              _ => '/etc/systemd/system/demo.service',
            },
          },
        ),
      ],
    );
    await enterKey(tester, 'hosts-search', 'prod-web');
    await tapKey(tester, 'connect-prod-web-1');
    await tapKey(tester, AppPlatform.isMobile ? 'terminal-tool-systemd' : 'nav-systemd');
    await tester.tap(find.text('demo.service'));
    await settle(tester);
    final panel = find.byType(SystemServicesPanel);
    final back = find.descendant(of: panel, matching: find.byTooltip('Back'));
    expect(tester.getTopLeft(find.text('demo.service')).dx - tester.getTopRight(back).dx, greaterThanOrEqualTo(12));
    final follow = find.byKey(const ValueKey('systemd-journal-follow'));
    expect(find.descendant(of: find.byType(PinnedHeaderSliver), matching: follow), findsOneWidget);
    expect(find.byTooltip('Show new journal entries for demo.service as they arrive.'), findsOneWidget);
    final scroll = tester.widget<CustomScrollView>(find.descendant(of: panel, matching: find.byType(CustomScrollView)));
    scroll.controller!.jumpTo(1600);
    await settle(tester);
    expect(follow.hitTestable(), findsOneWidget);
    await tester.tap(follow);
    await settle(tester);
    expect(tester.widget<FilterChip>(follow).selected, isTrue);
    await tester.tap(follow);
    await settle(tester);
    expect(tester.widget<FilterChip>(follow).selected, isFalse);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.android}));

  testWidgets('follow refreshes only the selected journal and ignores late replies after pause', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    var reads = 0;
    final delayed = Completer<Map<String, dynamic>>();
    Map<String, dynamic> reply(String output) => {'status': 0, 'output': output, 'diagnostic': ''};
    final first = jsonEncode({'__CURSOR': 'a', 'MESSAGE': 'level=error msg="first" error="details"'});
    final second = jsonEncode({'__CURSOR': 'b', 'MESSAGE': 'second'});
    await pumpApp(
      tester,
      backend,
      overrides: [
        systemdRequestProvider.overrideWithValue((host, request) async {
          if (request['action'] == 'list') return reply('demo.service loaded active running Example');
          if (request['action'] == 'unit_path') return reply('/etc/systemd/system/demo.service');
          expect(request, {
            'action': 'journal',
            'unit': 'demo.service',
            if (reads > 0) 'after_cursor': reads == 1 ? 'a' : 'b',
          });
          reads++;
          return switch (reads) {
            1 => reply(first),
            2 => reply(second),
            3 => delayed.future,
            _ => reply(second),
          };
        }),
      ],
    );
    await enterKey(tester, 'hosts-search', 'prod-web');
    await tapKey(tester, 'connect-prod-web-1');
    await tapKey(tester, 'nav-systemd');
    await tester.tap(find.text('demo.service'));
    await settle(tester);
    await tester.pump(const Duration(seconds: 6));
    expect(reads, 1);
    await tester.tap(find.text('first'));
    await settle(tester);
    await tapKey(tester, 'systemd-journal-follow');
    await tester.pump(const Duration(seconds: 3));
    await tester.pump();
    expect(find.text('second'), findsOneWidget);
    expect(find.textContaining('msg="first"'), findsOneWidget);
    await tester.pump(const Duration(seconds: 4));
    await tester.pump(const Duration(seconds: 6));
    expect(reads, 3);
    await tapKey(tester, 'systemd-journal-follow');
    delayed.complete(reply('stale'));
    await tester.pump(const Duration(seconds: 6));
    expect(find.text('stale'), findsNothing);
    expect(find.text('second'), findsOneWidget);
    expect(reads, 3);
    await tapKey(tester, 'systemd-journal-follow');
    await tester.tap(find.byTooltip('Back').last);
    await settle(tester);
    await tester.pump(const Duration(seconds: 6));
    expect(reads, 3);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));

  testWidgets('host services open the selected journal and restart only after confirmation', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final operations = <Map<String, Object?>>[];
    var journalReads = 0;
    await pumpApp(
      tester,
      backend,
      overrides: [
        systemdRequestProvider.overrideWithValue((host, request) async {
          expect(host.name, 'prod-web-1');
          operations.add(request);
          final output = switch (request['action']) {
            'list' => 'demo.service loaded active running Example\nother.service loaded inactive dead Other unit\nzeta.service loaded failed failed Failed unit\nonce.service loaded active exited Completed\nstart.service loaded activating start Starting\n',
            'unit_path' => '/etc/systemd/system/demo.service',
            'unit_text' => '# /etc/systemd/system/demo.service\n[Service]\nExecStart=/opt/demo\n# /etc/systemd/system/demo.service.d/override.conf',
            'journal' => [
              jsonEncode({'PRIORITY': '6', 'MESSAGE': 'log entry #${++journalReads}'}),
              jsonEncode({'PRIORITY': '6', 'MESSAGE': 'level=error msg="DNS timeout" error="resolver unreachable"'}),
            ].join('\n'),
            'restart' => '',
            _ => throw StateError('Unexpected action'),
          };
          return {'status': 0, 'output': output, 'diagnostic': ''};
        }),
      ],
    );
    await enterKey(tester, 'hosts-search', 'prod-web');
    await tapKey(tester, 'connect-prod-web-1');
    await tapKey(tester, 'nav-systemd');
    final tokens = GlassTokens.of(tester.element(find.byType(SystemServicesPanel)));
    Color? statusColor(String status) => tester.widget<Text>(find.text(status)).style?.color;
    expect(statusColor('active · running'), tokens.palette.success);
    expect(statusColor('active · exited'), tokens.palette.success);
    expect(statusColor('inactive · dead'), tokens.palette.danger);
    expect(statusColor('failed · failed'), tokens.palette.danger);
    expect(statusColor('activating · start'), tokens.palette.warning);
    await tapKey(tester, 'systemd-sort');
    await tester.tap(find.text('State · failed first'));
    await settle(tester);
    expect(tester.getTopLeft(find.text('zeta.service')).dy, lessThan(tester.getTopLeft(find.text('demo.service')).dy));
    await tapKey(tester, 'systemd-filter');
    await tester.tap(find.text('Running'));
    await settle(tester);
    expect(find.text('other.service'), findsNothing);
    expect(find.text('zeta.service'), findsNothing);
    await tester.tap(find.text('demo.service'));
    await settle(tester);
    expect(statusColor('active · running'), tokens.palette.success);
    expect(find.text('log entry #1'), findsOneWidget);
    expect(operations.last, {'action': 'journal', 'unit': 'demo.service'});
    expect(find.text('/etc/systemd/system/demo.service'), findsOneWidget);
    expect(operations.where((op) => op['action'] == 'unit_text'), isEmpty);
    await tester.tap(find.text('Read unit file'));
    await settle(tester);
    expect(find.textContaining('demo.service.d/override.conf'), findsOneWidget);
    await tester.tap(find.text('Hide unit file'));
    await settle(tester);
    await tapKey(tester, 'systemd-log-level');
    await tester.tap(find.text('Errors'));
    await settle(tester);
    expect(find.text('log entry #1'), findsNothing);
    await tester.tap(find.text('DNS timeout'));
    await settle(tester);
    expect(find.text('level=error msg="DNS timeout" error="resolver unreachable"'), findsOneWidget);
    await enterKey(tester, 'systemd-log-search', 'no such message');
    expect(find.text('No matching journal entries'), findsOneWidget);
    await enterKey(tester, 'systemd-log-search', '');
    await tapKey(tester, 'systemd-log-level');
    await tester.tap(find.text('All levels').last);
    await settle(tester);

    await tester.tap(find.text('Restart'));
    await settle(tester);
    expect(find.textContaining('Restart demo.service on prod-web-1'), findsOneWidget);
    await tapKey(tester, 'confirm-cancel');
    expect(operations.where((op) => op['action'] == 'restart'), isEmpty);

    await tester.tap(find.text('Restart'));
    await settle(tester);
    await tester.tap(find.text('Use sudo (passwordless)'));
    await tapKey(tester, 'confirm-ok');
    expect(operations.where((op) => op['action'] == 'restart').single, {
      'action': 'restart',
      'unit': 'demo.service',
      'sudo': true,
    });
    expect(find.text('Service restarted.'), findsOneWidget);
    expect(find.text('log entry #2'), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
}
