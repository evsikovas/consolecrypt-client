import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/app.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/hosts/host_picker.dart';
import 'package:consolecrypt/port_check/port_check_screen.dart';
import 'package:consolecrypt/port_check/port_result_card.dart';
import 'package:consolecrypt/port_check/port_web_preview.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

// Flutter replaces HttpClient with a stub. This test talks only to its own loopback server.
class _LoopbackHttp extends HttpOverrides {}

void main() {
  test(
    'reads actual page metadata and leaves redirects at the chosen origin',
    () => HttpOverrides.runWithHttpOverrides(() async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      addTearDown(() => server.close(force: true));
      var reads = 0;
      server.listen((request) async {
        reads++;
        if (reads == 1) {
          request.response.headers.contentType = ContentType.html;
          request.response.write(
            '<html><head><title>A &amp; B</title><meta name="description" content="Status dashboard"></head></html>',
          );
        } else {
          request.response.statusCode = 302;
          request.response.headers.set('Location', '/unexpected');
        }
        await request.response.close();
      });
      final origin = (sourceId: null, uri: Uri.http('127.0.0.1:${server.port}', '/'));
      final container = ProviderContainer();
      addTearDown(container.dispose);
      final sub = container.listen(portWebPageProvider(origin), (_, _) {});
      addTearDown(sub.close);
      final page = await container.read(portWebPageProvider(origin).future);
      expect((page.status, page.title, page.description), (200, 'A & B', 'Status dashboard'));
      final redirect = await container.refresh(portWebPageProvider(origin).future);
      expect(redirect.status, 302);
      expect(reads, 2);
      expect(describePortPage(200, 'application/json', '', utf8.encode('<title>Wrong</title>')).title, isEmpty);
    }, _LoopbackHttp()),
  );

  test('parses inclusive intervals, pasted dashes and boundary values', () {
    expect(parsePortInterval(' 22 - 10000 '), (low: 22, high: 10000));
    expect(parsePortInterval('1–65535'), (low: 1, high: 65535));
    expect(parsePortInterval('443'), (low: 443, high: 443));
    for (final value in ['0', '1-65536', '90-80', '22-', '-22', '22 25', '1-2-3']) {
      expect(parsePortInterval(value), isNull, reason: value);
    }
  });

  testWidgets('checks one endpoint from this device or the selected SSH server', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final calls = <(String?, String, int, bool)>[];
    final inventories = <(String?, int, int, bool)>[];
    await pumpApp(
      tester,
      backend,
      size: const Size(1600, 900),
      overrides: [
        listeningPortsProvider.overrideWithValue(({hostId, required low, required high, required udp}) async {
          inventories.add((hostId, low, high, udp));
          return [53, 5353];
        }),
        checkPortProvider.overrideWithValue(({sourceId, required host, required port, required udp}) async {
          calls.add((sourceId, host, port, udp));
          return udp ? 'no_reply' : 'connected';
        }),
      ],
    );
    final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
    container.read(routerProvider).go(AppRoutes.portCheck);
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('port-check-host')), '127.0.0.1');
    await tapKey(tester, 'port-check-submit');
    expect(calls.single, (null, '127.0.0.1', 443, false));
    expect(find.text('TCP connection established'), findsOneWidget);
    await tester.tap(find.text('SSH server'));
    await settle(tester);
    await tester.tap(find.text('Choose server'));
    await settle(tester);
    await tester.tap(find.descendant(of: find.byType(HostPickerDialog), matching: find.byType(ListTile)).first);
    await settle(tester);
    await tester.tap(find.text('UDP'));
    await settle(tester);
    await tester.enterText(find.byKey(const ValueKey('port-check-number')), '65536');
    await tapKey(tester, 'port-check-submit');
    expect(calls, hasLength(1));
    await tester.enterText(find.byKey(const ValueKey('port-check-number')), '53');
    await tapKey(tester, 'port-check-submit');
    expect(calls.last.$1, isNotNull);
    expect(calls.last, (calls.last.$1, '127.0.0.1', 53, true));
    expect(find.text('No UDP reply. An open port can also stay silent.'), findsOneWidget);
    await tester.enterText(find.byKey(const ValueKey('port-check-number')), '22-10000');
    await tapKey(tester, 'port-check-submit');
    expect(inventories.single, (calls.last.$1, 22, 10000, true));
    expect(find.text('5353/UDP'), findsOneWidget);
    final row = find.widgetWithText(ListTile, '5353/UDP');
    final header = find.descendant(of: find.byType(PortCheckScreen), matching: find.byType(ContentSurface)).first;
    final card = find.byKey(const ValueKey('port-card-5353'));
    expect(tester.getSize(card).width, greaterThan(760));
    expect(tester.getRect(card).left, closeTo(tester.getRect(header).left, 1));
    expect(tester.getRect(card).right, closeTo(tester.getRect(header).right, 1));
    await tester.ensureVisible(row);
    await tester.tap(row);
    await settle(tester);
    expect(calls.last, (inventories.single.$1, '127.0.0.1', 5353, true));
    expect(find.text('53/UDP'), findsOneWidget);
  });

  testWidgets('one destination field accepts saved or typed hosts and reports actual progress', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final inventory = Completer<List<int>>();
    final probe = Completer<String>();
    final destinations = <String?>[];
    final webOrigins = <WebOrigin>[];
    final browser = <Uri>[];
    await pumpApp(
      tester,
      backend,
      size: const Size(430, 860),
      overrides: [
        portOwnersProvider.overrideWith((ref, target) async => ['service-owner']),
        portWebPageProvider.overrideWith((ref, origin) async {
          webOrigins.add(origin);
          return const PortWebPage(
            status: 200,
            mime: 'text/html',
            title: 'Console status',
            description: 'Service dashboard',
          );
        }),
        portBrowserProvider.overrideWithValue((uri) async {
          browser.add(uri);
          return true;
        }),
        listeningPortsProvider.overrideWithValue(({hostId, required low, required high, required udp}) {
          destinations.add(hostId);
          return inventory.future;
        }),
        checkPortProvider.overrideWithValue(({sourceId, required host, required port, required udp}) => probe.future),
      ],
    );
    final container = ProviderScope.containerOf(tester.element(find.byType(ConsoleCryptApp)));
    container.read(routerProvider).go(AppRoutes.portCheck);
    await settle(tester);
    final field = find.byKey(const ValueKey('port-check-host'));
    final picker = find.byKey(const ValueKey('port-check-pick-destination'));
    expect(find.ancestor(of: picker, matching: find.byType(GlassField)), findsOneWidget);
    await tapKey(tester, 'port-check-pick-destination');
    final option = find.descendant(of: find.byType(HostPickerDialog), matching: find.byType(ListTile)).first;
    final address = (tester.widget<ListTile>(option).subtitle! as Text).data!;
    await tester.tap(option);
    await settle(tester);
    expect(tester.widget<TextField>(field).controller!.text, address);
    final selected = (await container.read(hostsProvider.future)).firstWhere((h) => h.address == address);
    expect(find.widgetWithText(GlassButton, selected.name), findsNothing);
    await tester.enterText(find.byKey(const ValueKey('port-check-number')), '22-1000');
    final submit = find.byKey(const ValueKey('port-check-submit'));
    await tester.ensureVisible(submit);
    await tester.tap(submit);
    await tester.pump();
    expect(destinations, [selected.id.value]);
    expect(find.text('Reading TCP sockets on the destination…'), findsOneWidget);
    expect(tester.widget<LinearProgressIndicator>(find.byType(LinearProgressIndicator)).value, isNull);
    inventory.complete([22, 443]);
    await settle(tester);
    final check22 = find.widgetWithText(ListTile, '22/TCP');
    await tester.ensureVisible(check22);
    await tester.tap(check22);
    await tester.pump();
    expect(find.text('Checking port 22…'), findsOneWidget);
    expect(find.text('Checked 0 of 2 ports'), findsOneWidget);
    probe.complete('connected');
    await settle(tester);
    expect(find.text('Checked 1 of 2 ports'), findsOneWidget);
    expect(find.text('Listening process: service-owner'), findsOneWidget);
    expect(find.text('Common use for this port: SSH'), findsOneWidget);
    expect(webOrigins, isEmpty);
    expect(tester.widget<LinearProgressIndicator>(find.byType(LinearProgressIndicator)).value, 0.5);
    await tester.tap(check22);
    await settle(tester);
    expect(find.text('Checked 1 of 2 ports'), findsOneWidget);
    final check443 = find.widgetWithText(ListTile, '443/TCP');
    await tester.ensureVisible(check443);
    await tester.tap(check443);
    await settle(tester);
    expect(find.text('Checked 2 of 2 ports'), findsOneWidget);
    expect(find.text('Console status'), findsOneWidget);
    expect(find.text('Service dashboard'), findsOneWidget);
    expect(webOrigins.single, (sourceId: null, uri: Uri(scheme: 'https', host: address, path: '/')));
    final link = find.text(webOrigins.single.uri.toString());
    await tester.ensureVisible(link);
    await tester.tap(link);
    expect(browser.single, webOrigins.single.uri);
    await tester.ensureVisible(field);
    await tester.enterText(field, '127.0.0.1');
    await tapKey(tester, 'port-check-submit');
    expect(destinations, [selected.id.value, null]);
    expect(find.text('Checked 2 of 2 ports'), findsNothing);
  });
}
