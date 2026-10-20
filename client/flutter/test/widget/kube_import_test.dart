import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_import_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kubernetes_screen.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

final _scope = NotifierProvider<_Scope, KubeScope>(_Scope.new);

class _Scope extends Notifier<KubeScope> {
  @override
  KubeScope build() => const KubeScope('profile', true);
  void lock() => state = const KubeScope('profile', false);
  void unlock(String profileId) => state = KubeScope(profileId, true);
}

Future<ProviderContainer> _pump(
  WidgetTester tester,
  _Service service, {
  String language = 'en',
  Size size = const Size(1100, 850),
  double textScale = 1,
  bool inventory = false,
}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        appServicesProvider.overrideWithValue(backend.services),
        kubeServiceProvider.overrideWithValue(service),
        kubeScopeProvider.overrideWith((ref) => ref.watch(_scope)),
      ],
      child: MaterialApp(
        locale: Locale(language),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(textScale)),
          child: child!,
        ),
        home: Scaffold(
          body: inventory
              ? const KubernetesScreen()
              : Builder(
                  builder: (context) => TextButton(
                    key: const ValueKey('open-import'),
                    onPressed: () => showAppDialog<bool>(
                      context,
                      secure: true,
                      barrierDismissible: false,
                      builder: (_) => const KubeImportDialog(),
                    ),
                    child: const Text('Open'),
                  ),
                ),
        ),
      ),
    ),
  );
  await settle(tester);
  if (inventory) return ProviderScope.containerOf(tester.element(find.byType(KubernetesScreen)));
  await tester.tap(find.byKey(const ValueKey('open-import')));
  await settle(tester);
  return ProviderScope.containerOf(tester.element(find.byType(KubeImportDialog)));
}

Future<void> _preview(WidgetTester tester) async {
  await tester.enterText(find.byKey(const ValueKey('kube-import-yaml')), 'synthetic-token-input');
  await tester.tap(find.byKey(const ValueKey('kube-import-submit')));
  await settle(tester);
}

void main() {
  testWidgets('profile changes and locking hide prior inventory while the next scope is loading', (tester) async {
    final service = _Service()..inventoryPending = Completer<List<KubeConnection>>();
    final container = await _pump(tester, service, inventory: true);
    service.inventoryPending!.complete([_connection('first-profile')]);
    await settle(tester);
    expect(find.text('first-profile'), findsOneWidget);

    service.inventoryPending = Completer<List<KubeConnection>>();
    container.read(_scope.notifier).unlock('second-profile');
    await tester.pump();
    expect(find.text('first-profile'), findsNothing);
    final obsolete = service.inventoryPending!;
    container.read(_scope.notifier).lock();
    await tester.pump();
    obsolete.complete([_connection('late-profile')]);
    await settle(tester);
    expect(find.text('first-profile'), findsNothing);
    expect(find.text('late-profile'), findsNothing);

    service.inventoryPending = Completer<List<KubeConnection>>();
    container.read(_scope.notifier).unlock('second-profile');
    await tester.pump();
    expect(find.text('late-profile'), findsNothing);
    service.inventoryPending!.complete([_connection('current-profile')]);
    await settle(tester);
    expect(find.text('current-profile'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  for (final language in ['en', 'ru']) {
    for (final width in [412.0, 1100.0]) {
      testWidgets('import $language at $width requires explicit file approval and saves selected contexts', (
        tester,
      ) async {
        final service = _Service();
        await _pump(tester, service, language: language, size: Size(width, 915));
        await _preview(tester);
        expect(service.received!.isWiped, isTrue);
        expect(find.text('synthetic-token-input'), findsNothing);
        expect(isEnabled(tester, 'kube-import-submit'), isFalse);
        final files = find.byKey(const ValueKey('kube-import-approve-files'));
        await tester.ensureVisible(files);
        await tester.pump();
        await tester.tap(files);
        await settle(tester);
        expect(isEnabled(tester, 'kube-import-submit'), isTrue);
        await tester.tap(find.byKey(const ValueKey('kube-import-submit')));
        await settle(tester);
        expect(service.commits, 1);
        expect(service.saved!.map((choice) => choice.index), [0, 1]);
        expect(service.saved![0].approvedFiles, isEmpty);
        expect(service.saved![1].approvedFiles, ['/selected/path/token']);
        expect(service.saved!.every((choice) => choice.production), isTrue);
        expect(find.byType(KubeImportDialog), findsNothing);
        expect(tester.takeException(), isNull);
      });
    }
  }

  testWidgets('changing the selected set clears referenced file consent', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await _preview(tester);
    final files = find.byKey(const ValueKey('kube-import-approve-files'));
    await tester.ensureVisible(files);
    await tester.tap(files);
    await settle(tester);
    expect(isEnabled(tester, 'kube-import-submit'), isTrue);
    final first = find.byKey(const ValueKey('kube-context-0'));
    await tester.ensureVisible(first);
    await tester.tap(first);
    await settle(tester);
    expect(isEnabled(tester, 'kube-import-submit'), isFalse);
    await tester.pumpWidget(const SizedBox.shrink());
    await settle(tester);
    expect(service.discarded, ['ticket']);
    expect(tester.takeException(), isNull);
  });

  testWidgets('locking during parsing discards the late preview without a save', (tester) async {
    final service = _Service()..pending = Completer<KubeImportPreview>();
    final container = await _pump(tester, service);
    await _preview(tester);
    container.read(_scope.notifier).lock();
    await settle(tester);
    expect(find.byType(KubeImportDialog), findsNothing);
    service.pending!.complete(service.result);
    await settle(tester);
    expect(service.received!.isWiped, isTrue);
    expect(service.discarded, ['ticket']);
    expect(service.commits, 0);
    expect(tester.takeException(), isNull);
  });

  testWidgets('cancelling the native file picker does not show an import failure', (tester) async {
    final service = _Service();
    await _pump(tester, service);
    await tester.tap(find.byKey(const ValueKey('kube-import-file')));
    await settle(tester);
    expect(find.textContaining('Could not import'), findsNothing);
    expect(service.received, isNull);
    expect(tester.takeException(), isNull);
  });

  testWidgets('expired preview cannot be saved at enlarged mobile text size', (tester) async {
    final service = _Service()..expiresAfter = const Duration(seconds: 10);
    await _pump(tester, service, language: 'ru', size: const Size(412, 915), textScale: 1.6);
    await _preview(tester);
    await tester.pump(const Duration(seconds: 12));
    await tester.pump();
    expect(isEnabled(tester, 'kube-import-submit'), isFalse);
    expect(find.textContaining('Время предварительного просмотра истекло'), findsOneWidget);
    expect(service.commits, 0);
    expect(tester.takeException(), isNull);
  });
}

KubeConnection _connection(String name) => KubeConnection(
  id: name,
  name: name,
  clusterName: name,
  server: 'https://example.test',
  namespace: 'default',
  authentication: 'token',
  production: true,
);

class _Service implements KubeService {
  SecretText? received;
  List<KubeImportChoice>? saved;
  int commits = 0;
  final discarded = <String>[];
  Completer<KubeImportPreview>? pending;
  Completer<List<KubeConnection>>? inventoryPending;
  Duration expiresAfter = const Duration(minutes: 10);
  KubeImportPreview get result => KubeImportPreview(
    ticket: 'ticket',
    expiresAt: DateTime.now().toUtc().add(expiresAfter),
    entries: const [
      KubeImportEntry(
        index: 0,
        name: 'first',
        server: 'https://first.example.test',
        namespace: 'default',
        authentication: 'token',
      ),
      KubeImportEntry(
        index: 1,
        name: 'second',
        server: 'https://second.example.test',
        namespace: 'default',
        authentication: 'token',
        pendingFiles: ['/selected/path/token'],
      ),
    ],
  );
  @override
  Stream<void> changes() => const Stream.empty();
  @override
  Future<List<KubeConnection>> inventory() async => inventoryPending == null ? const [] : inventoryPending!.future;
  @override
  Future<KubeImportPreview> preview(SecretText yaml, {String? sourceFile}) async {
    received = yaml;
    return pending == null ? result : pending!.future;
  }

  @override
  Future<KubeImportPreview> previewFile(String path) async => result;
  @override
  Future<void> discard(String ticket) async {
    discarded.add(ticket);
  }

  @override
  Future<void> commit(String ticket, List<KubeImportChoice> choices) async {
    commits++;
    saved = choices;
  }

  @override
  Future<void> deleteContext(String id) async {}
}
