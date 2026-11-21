import 'dart:io';

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/platform.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/keepass/external_vault.dart';
import 'package:consolecrypt/keepass/external_vault_store.dart';
import 'package:consolecrypt/keepass/password_manager.dart';
import 'package:consolecrypt/keepass/vault_connection_dialog.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

class _RealHttp extends HttpOverrides {}

class _PasswordManager extends Fake implements PasswordManager {
  @override
  final validity = ValueNotifier(true);

  @override
  bool get valid => true;
}

void main() {
  testWidgets('Passwords opens Vault folders and masked secrets over HTTP', (tester) async {
    final requests = <(String, String, String?)>[];
    final server = (await tester.runAsync(() async {
      final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      server.listen((request) async {
        final path = request.uri.path;
        requests.add((request.method, path, request.headers.value('X-Vault-Token')));
        request.response.headers.contentType = ContentType.json;
        request.response.write(switch (path) {
          '/v1/sys/internal/ui/mounts' => '{"data":{"secret":{"kv/":{"type":"kv","options":{"version":"2"}}}}}',
          '/v1/kv/metadata' => '{"data":{"keys":["applications/","infrastructure/"]}}',
          '/v1/kv/metadata/applications' => '{"data":{"keys":["vaultwarden","other"]}}',
          '/v1/kv/data/applications/vaultwarden' =>
            '{"data":{"data":{"password":"fixture-secret-value"},"metadata":{"version":2}}}',
          _ => '{}',
        });
        await request.response.close();
      });
      return server;
    }))!;
    addTearDown(() => server.close(force: true));
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final mobile = AppPlatform.isMobile;
    await pumpApp(tester, backend, locale: AppLocale.en, size: mobile ? const Size(360, 800) : const Size(1600, 1050));
    final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
    container.read(routerProvider).go(AppRoutes.passwords);
    await settle(tester);
    await tapKey(tester, 'keepass-external-vault');
    final workspace = find.byKey(const ValueKey('external-vault-workspace'));
    expect(workspace, findsOneWidget);

    Future<void> tapText(String label) async {
      await tester.tap(find.descendant(of: workspace, matching: find.text(label)).first);
      await settle(tester);
    }

    Future<void> waitFor(Finder finder) async {
      for (var attempt = 0; attempt < 80 && finder.evaluate().isEmpty; attempt++) {
        await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
        await tester.pump(const Duration(milliseconds: 30));
      }
      expect(finder, findsOneWidget);
    }

    await tapText('Add connection');
    Finder field(String label) =>
        find.byWidgetPredicate((widget) => widget is TextField && widget.decoration?.labelText == label);
    await tester.enterText(field('Title'), 'Fixture Vault');
    await tester.enterText(field('Server address'), 'http://127.0.0.1:${server.port}');
    await tester.pump();
    expect(find.text('HTTP sends credentials and secret values without encryption.'), findsOneWidget);
    await tester.tap(find.byType(GlassSelect<ExternalVaultAuth>));
    await settle(tester);
    await tester.tap(find.text('Access token').last);
    await settle(tester);
    await tester.enterText(field('Access token'), 'fixture-access-token');
    await HttpOverrides.runWithHttpOverrides(() => tapText('Connect'), _RealHttp());

    final folder = find.byKey(const ValueKey('external-vault-item-applications/'));
    await waitFor(folder);
    expect(requests.map((request) => request.$2), ['/v1/sys/internal/ui/mounts', '/v1/kv/metadata']);
    await tester.tap(folder);
    final item = find.byKey(const ValueKey('external-vault-item-vaultwarden'));
    await waitFor(item);
    await enterKey(tester, 'external-vault-search', 'VAULT');
    if (mobile) {
      tester.view.viewInsets = const FakeViewPadding(bottom: 320);
      await settle(tester);
      expect(tester.takeException(), isNull);
      tester.view.viewInsets = FakeViewPadding.zero;
      await settle(tester);
    }
    expect(item, findsOneWidget);
    expect(find.byKey(const ValueKey('external-vault-item-other')), findsNothing);
    expect(requests.every((request) => !request.$2.contains('/data/')), isTrue);

    await tester.tap(item);
    await waitFor(find.text('••••••••'));
    expect(item, mobile ? findsNothing : findsOneWidget);
    expect(find.text('fixture-secret-value'), findsNothing);
    final show = find.widgetWithIcon(GlassIconButton, Icons.visibility_outlined);
    final copy = find.widgetWithIcon(GlassIconButton, Icons.copy_outlined);
    expect(tester.getSize(show), tester.getSize(copy));
    expect(tester.getRect(copy).left - tester.getRect(show).right, GlassSpacing.toolbarItemGap);
    await tester.tap(show);
    await settle(tester);
    expect(find.text('fixture-secret-value'), findsOneWidget);
    expect(requests.last, ('GET', '/v1/kv/data/applications/vaultwarden', 'fixture-access-token'));

    if (mobile) await tapText('Back');
    await tapText('kv');
    await waitFor(folder);
    expect(find.text('fixture-secret-value'), findsNothing);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  }, variant: const TargetPlatformVariant({TargetPlatform.windows, TargetPlatform.android}));

  testWidgets('Vault connection fields have spacing with either auth method and expanded settings', (tester) async {
    tester.view.physicalSize = const Size(420, 780);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final manager = _PasswordManager();
    addTearDown(manager.validity.dispose);
    final l = lookupAppLocalizations(const Locale('ru'));

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          passwordManagerProvider.overrideWithValue(manager),
          externalVaultConnectionsProvider.overrideWithValue(const []),
        ],
        child: MaterialApp(
          locale: const Locale('ru'),
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
          home: Scaffold(
            body: Builder(
              builder: (context) =>
                  TextButton(onPressed: () => showVaultConnections(context), child: const Text('Vault')),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Vault'));
    await tester.pumpAndSettle();
    await tester.tap(find.text(l.externalVaultAdd));
    await tester.pumpAndSettle();

    Finder field(String label) =>
        find.byWidgetPredicate((widget) => widget is TextField && widget.decoration?.labelText == label);
    void expectSpacing(List<Finder> controls) {
      for (var i = 1; i < controls.length; i++) {
        expect(tester.getTopLeft(controls[i]).dy - tester.getBottomLeft(controls[i - 1]).dy, GlassSpacing.s8);
      }
    }

    final auth = find.byType(GlassSelect<ExternalVaultAuth>);
    final credential = find.byType(SecretField);
    final user = field(l.keepassUsername);
    final endpoint = field(l.externalVaultEndpoint);
    expectSpacing([field(l.keepassEntryTitle), endpoint, auth, user, credential]);

    await tester.tap(find.text(l.externalVaultApiSettings));
    await tester.pumpAndSettle();
    final mount = field(l.externalVaultMount);
    final namespace = field(l.externalVaultNamespace);
    final authMount = field(l.externalVaultAuthMount);
    final version = find.byType(GlassSelect<bool>);
    expectSpacing([mount, namespace, authMount, version]);

    await tester.tap(auth);
    await tester.pumpAndSettle();
    await tester.tap(find.text(l.externalVaultToken).last);
    await tester.pumpAndSettle();
    expect(user, findsNothing);
    expect(authMount, findsNothing);
    expect(tester.widget<SecretField>(credential).label, l.externalVaultToken);
    expectSpacing([endpoint, auth, credential]);
    expectSpacing([mount, namespace, version]);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
