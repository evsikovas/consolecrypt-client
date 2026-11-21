import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/keepass/external_vault.dart';
import 'package:consolecrypt/keepass/external_vault_store.dart';
import 'package:consolecrypt/keepass/password_manager.dart';
import 'package:consolecrypt/keepass/vault_connection_dialog.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

class _Manager extends Fake implements PasswordManager {
  @override
  final validity = ValueNotifier(true);

  @override
  bool get valid => validity.value;
}

ExternalVaultConnection _saved(int index) => ExternalVaultConnection(
  id: 'server-$index',
  profile: 'test',
  vault: 'vault',
  name: 'Team $index',
  endpoint: 'https://vault-$index.example.org:8200/internal-prefix',
  auth: index.isEven ? ExternalVaultAuth.userpass : ExternalVaultAuth.token,
  username: index.isEven ? 'private-user' : '',
  authMount: 'userpass',
  mount: 'secret',
  namespace: 'private-namespace',
  versioned: true,
  wrapped: 'private-wrapper',
);

Future<void> _show(WidgetTester tester, {required int count, required double width}) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = Size(width, 850);
  addTearDown(tester.view.reset);
  final manager = _Manager();
  addTearDown(manager.validity.dispose);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        passwordManagerProvider.overrideWithValue(manager),
        externalVaultConnectionsProvider.overrideWithValue(List.generate(count, _saved)),
      ],
      child: MaterialApp(
        locale: const Locale('ru'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        home: Scaffold(body: ExternalVaultWorkspace(onClose: () {})),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  final l = lookupAppLocalizations(const Locale('ru'));
  Finder card(int index) => find.byKey(ValueKey('external-vault-server-$index'));

  testWidgets('wide saved cards use the full workspace and expose the server origin and sign-in method', (
    tester,
  ) async {
    await _show(tester, count: 4, width: 1280);
    final first = tester.getRect(card(0));
    final last = tester.getRect(card(2));
    expect(first.top, last.top);
    expect(last.width, closeTo(first.width, .01));
    expect(last.right, greaterThan(1200));
    expect(tester.getRect(card(3)).top, greaterThan(first.bottom));
    expect(find.text('https://vault-0.example.org:8200'), findsOneWidget);
    expect(find.text(l.externalVaultUserpass), findsNWidgets(2));
    expect(find.text(l.externalVaultToken), findsNWidgets(2));
    for (final value in ['internal-prefix', 'private-user', 'private-namespace', 'private-wrapper']) {
      expect(find.textContaining(value), findsNothing);
    }
    final open = find.descendant(of: card(1), matching: find.text(l.commonOpen));
    await tester.tap(open);
    await tester.pumpAndSettle();
    expect(find.text('Team 1'), findsWidgets);
    expect(find.text(l.externalVaultEndpoint), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('narrow workspace stacks the same cards vertically and opens the add form', (tester) async {
    await _show(tester, count: 2, width: 340);
    final first = tester.getRect(card(0)), second = tester.getRect(card(1));
    expect(second.left, first.left);
    expect(second.right, first.right);
    expect(second.top, greaterThan(first.bottom));
    await tester.tap(find.text(l.externalVaultAdd));
    await tester.pumpAndSettle();
    expect(find.text(l.externalVaultEndpoint), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('an empty workspace offers the first connection in its header', (tester) async {
    await _show(tester, count: 0, width: 900);
    expect(find.text(l.externalVaultConnections), findsOneWidget);
    expect(find.text(l.externalVaultNoConnections), findsOneWidget);
    expect(find.text(l.externalVaultAdd), findsOneWidget);
    expect(find.text(l.commonOpen), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('one saved server uses the available row and keeps its action at the far edge', (tester) async {
    await _show(tester, count: 1, width: 1280);
    final rect = tester.getRect(card(0));
    final origin = tester.getRect(find.text('https://vault-0.example.org:8200'));
    final method = tester.getRect(find.text(l.externalVaultUserpass));
    final action = tester.getRect(find.text(l.commonOpen));
    expect(rect.width, greaterThan(1200));
    expect(method.left, greaterThan(origin.right));
    expect(action.left, greaterThan(method.right));
    expect(rect.right - action.right, lessThan(64));
  });
}
