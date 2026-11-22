import 'dart:convert';
import 'dart:io';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/keepass/external_vault.dart';
import 'package:consolecrypt/keepass/external_vault_store.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _Loopback extends HttpOverrides {}

void main() {
  testWidgets('Vault folder columns fit short listings, show active paths and build long listings lazily', (
    tester,
  ) async {
    final requests = <String>[];
    final server = (await tester.runAsync(() async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((request) async {
        requests.add(request.uri.path);
        final data = switch (request.uri.path) {
          '/v1/sys/internal/ui/mounts' => {
            'secret': {
              'kv/': {
                'type': 'kv',
                'options': {'version': '2'},
              },
            },
          },
          '/v1/kv/metadata' => {
            'keys': ['team/', 'api-token'],
          },
          '/v1/kv/metadata/team' => {'keys': List.generate(500, (i) => 'secret-${i.toString().padLeft(3, '0')}')},
          _ => <String, dynamic>{},
        };
        request.response.headers.contentType = ContentType.json;
        request.response.write(jsonEncode({'data': data}));
        await request.response.close();
      });
      return server;
    }))!;
    addTearDown(() => server.close(force: true));
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final connection = ExternalVaultConnection(
      id: 'layout',
      profile: 'test',
      vault: 'test',
      name: 'Layout',
      endpoint: 'http://127.0.0.1:${server.port}',
      auth: ExternalVaultAuth.token,
      username: '',
      authMount: '',
      mount: 'kv',
      namespace: '',
      versioned: true,
    );
    await pumpApp(
      tester,
      backend,
      overrides: [
        externalVaultConnectionsProvider.overrideWithValue([connection]),
      ],
    );
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.passwords);
    await settle(tester);
    await tapKey(tester, 'keepass-external-vault');
    await tapKey(tester, 'external-vault-layout');
    final token = find.byWidgetPredicate((w) => w is TextField && w.decoration?.labelText == 'Access token');
    await tester.enterText(token, 'fixture-token');
    await tester.pump();
    final connect = find.byKey(const ValueKey('external-vault-connect'));
    expect(tester.widget<GlassButton>(connect).onPressed, isNotNull);
    await tester.ensureVisible(connect);
    await HttpOverrides.runWithHttpOverrides(() async {
      await tester.tap(connect);
      await settle(tester);
    }, _Loopback());

    Finder item(String name) => find.byKey(ValueKey('external-vault-item-$name'));
    Finder column(String path) => find.byKey(PageStorageKey(('external-vault-column', 'kv', path)));
    Future<void> waitFor(Finder finder) async {
      for (var i = 0; i < 80 && finder.evaluate().isEmpty; i++) {
        await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
        await tester.pump(const Duration(milliseconds: 30));
      }
      expect(finder, findsOneWidget);
    }

    final directory = item('team/');
    await waitFor(directory);
    final root = column('');
    expect(tester.getSize(root).height, lessThan(200));
    expect(tester.widget<ListTile>(directory).trailing, isNotNull);
    expect(tester.widget<ListTile>(item('api-token')).trailing, isNull);
    final crumb = find.descendant(
      of: find.byKey(const ValueKey(('external-vault-current-crumb', ''))),
      matching: find.text('kv'),
    );
    expect(tester.widget<Text>(crumb).style?.color, GlassTokens.of(tester.element(crumb)).palette.label);

    await tester.tap(directory);
    await waitFor(item('secret-000'));
    expect(tester.widget<ListTile>(directory).selected, isTrue);
    expect(tester.getRect(column('team')).left, greaterThan(tester.getRect(root).right));
    expect(item('secret-499'), findsNothing);
    await enterKey(tester, 'external-vault-search', 'no-such-entry');
    expect(find.text('Nothing matches your search'), findsOneWidget);
    expect(tester.getSize(column('team')).height, lessThan(150));
    expect(requests.every((path) => !path.contains('/data/')), isTrue);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
