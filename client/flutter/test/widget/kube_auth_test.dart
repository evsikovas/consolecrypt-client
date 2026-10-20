import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_auth_browser.dart';
import 'package:consolecrypt/kubernetes/kube_auth_dialog.dart';
import 'package:consolecrypt/kubernetes/kube_auth_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

final _scope = NotifierProvider<_Scope, KubeScope>(_Scope.new);

class _Scope extends Notifier<KubeScope> {
  @override
  KubeScope build() => const KubeScope('profile', true);
  void lock() => state = const KubeScope('profile', false);
}

class _Service implements KubeAuthService {
  final approved = <String>[], cancelled = <String>[], completed = <String?>[];
  int reviews = 0;
  Completer<KubeExecReview>? pendingReview;
  KubeExecReview? lastReview;
  bool loopback = true;
  KubeExecReview review() => lastReview = KubeExecReview(
    ticket: 'exec-ticket',
    contextName: 'Production',
    binaryPath: '/usr/local/bin/fixture',
    binarySha256: 'a' * 64,
    invocationSha256: 'b' * 64,
    expiresAt: DateTime.now().toUtc().add(const Duration(minutes: 10)),
    arguments: ['synthetic-secret-argument', 'line\nbreak'],
    environment: {'SYNTHETIC_TOKEN': 'synthetic-secret-environment'},
  );
  @override
  Future<KubeExecReview> reviewExec(String contextId) async {
    reviews++;
    return pendingReview?.future ?? review();
  }

  @override
  Future<void> approveExec(String ticket) async {
    approved.add(ticket);
  }

  @override
  Future<KubeOidcChallenge> beginOidc(String contextId) async => KubeOidcChallenge(
    ticket: 'oidc-ticket',
    authorizationUrl: Uri.parse('https://issuer.example.test/authorize?state=fixture'),
    redirectUri: Uri.parse(loopback ? 'http://127.0.0.1:32123/callback' : 'consolecrypt://kubernetes/oauth/callback'),
    expiresAt: DateTime.now().toUtc().add(const Duration(minutes: 10)),
    loopback: loopback,
  );
  @override
  Future<void> completeOidc(String ticket, {String? callbackUrl}) async {
    completed.add(callbackUrl);
  }

  @override
  Future<void> cancelAuth(String ticket) async {
    cancelled.add(ticket);
  }
}

class _Browser implements KubeAuthBrowser {
  int opened = 0;
  final cancelled = <String>[];
  String? callback;
  Completer<String?>? pending;
  @override
  Future<String?> open(KubeOidcChallenge challenge) async {
    opened++;
    return pending?.future ?? callback;
  }

  @override
  Future<void> cancel(String ticket) async {
    cancelled.add(ticket);
  }
}

Future<ProviderContainer> _pump(
  WidgetTester tester,
  _Service service,
  _Browser browser, {
  String kind = 'exec',
  String language = 'en',
  TargetPlatform platform = TargetPlatform.macOS,
  double scale = 1,
  Size size = const Size(1100, 850),
}) async {
  debugDefaultTargetPlatformOverride = platform;
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        kubeAuthServiceProvider.overrideWithValue(service),
        kubeAuthBrowserProvider.overrideWithValue(browser),
        kubeScopeProvider.overrideWith((ref) => ref.watch(_scope)),
      ],
      child: MaterialApp(
        locale: Locale(language),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: TextScaler.linear(scale)),
          child: child!,
        ),
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              key: const ValueKey('open-auth'),
              onPressed: () => authenticateKubeConnection(
                context,
                KubeConnection(
                  id: 'context',
                  name: 'Production',
                  clusterName: 'Cluster',
                  server: 'https://api.example.test',
                  namespace: 'default',
                  authentication: kind,
                  production: true,
                ),
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    ),
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Scaffold).first));
  await tester.tap(find.byKey(const ValueKey('open-auth')));
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 300));
  return container;
}

void authTestWidgets(String description, Future<void> Function(WidgetTester) body) {
  testWidgets(description, (tester) async {
    try {
      await body(tester);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}

void main() {
  for (final language in ['en', 'ru']) {
    authTestWidgets('exec review requires explicit consent and clears secrets ($language)', (tester) async {
      final service = _Service(), browser = _Browser();
      await _pump(tester, service, browser, language: language, size: const Size(412, 900), scale: 1.5);
      expect(service.approved, isEmpty);
      expect(find.text('/usr/local/bin/fixture'), findsOneWidget);
      expect(find.text('1: "synthetic-secret-argument"'), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tester.tap(find.byKey(const ValueKey('kube-auth-submit')));
      await tester.pumpAndSettle();
      expect(service.approved, ['exec-ticket']);
      expect(service.lastReview!.arguments, isEmpty);
      expect(service.lastReview!.environment, isEmpty);
      expect(find.byKey(const ValueKey('kube-auth-dialog')), findsNothing);
    });
  }
  authTestWidgets('mobile exec never requests a command approval', (tester) async {
    final service = _Service();
    await _pump(tester, service, _Browser(), platform: TargetPlatform.android, size: const Size(412, 850));
    expect(service.reviews, 0);
    expect(find.byKey(const ValueKey('kube-auth-submit')), findsNothing);
    expect(find.textContaining('External authentication commands are unavailable'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('kube-auth-cancel')));
    await tester.pumpAndSettle();
  });
  for (final mobile in [false, true]) {
    authTestWidgets('OIDC opens only on click and passes ${mobile ? 'mobile' : 'native loopback'} callback', (
      tester,
    ) async {
      final service = _Service()..loopback = !mobile;
      final browser = _Browser()
        ..callback = mobile ? 'consolecrypt://kubernetes/oauth/callback?code=fixture&state=fixture' : null;
      await _pump(
        tester,
        service,
        browser,
        kind: 'oidc',
        platform: mobile ? TargetPlatform.android : TargetPlatform.macOS,
      );
      expect(browser.opened, 0);
      await tester.tap(find.byKey(const ValueKey('kube-auth-submit')));
      await tester.pumpAndSettle();
      expect(browser.opened, 1);
      expect(service.completed, [browser.callback]);
      expect(find.byKey(const ValueKey('kube-auth-dialog')), findsNothing);
    });
  }
  authTestWidgets('cancel during browser wait cancels native and platform requests', (tester) async {
    final service = _Service()..loopback = false;
    final browser = _Browser()..pending = Completer<String?>();
    await _pump(tester, service, browser, kind: 'oidc', platform: TargetPlatform.android);
    await tester.tap(find.byKey(const ValueKey('kube-auth-submit')));
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('kube-auth-cancel')));
    await tester.pumpAndSettle();
    expect(service.cancelled, contains('oidc-ticket'));
    expect(browser.cancelled, contains('oidc-ticket'));
    browser.pending!.complete('consolecrypt://kubernetes/oauth/callback?code=late');
    await tester.pumpAndSettle();
    expect(service.completed, isEmpty);
  });
  authTestWidgets('late secret review is discarded after a profile lock', (tester) async {
    final service = _Service()..pendingReview = Completer<KubeExecReview>();
    final container = await _pump(tester, service, _Browser());
    container.read(_scope.notifier).lock();
    await tester.pump();
    final review = service.review();
    service.pendingReview!.complete(review);
    await tester.pumpAndSettle();
    expect(service.approved, isEmpty);
    expect(service.cancelled, contains('exec-ticket'));
    expect(review.arguments, isEmpty);
    expect(review.environment, isEmpty);
    expect(find.byKey(const ValueKey('kube-auth-dialog')), findsNothing);
  });
  test('bridge byte buffers are wiped on valid and invalid JSON', () {
    final bytes = Uint8List.fromList(
      utf8.encode(
        jsonEncode({
          'ticket': 'id',
          'context_name': 'context',
          'binary_path': 'fixture',
          'binary_sha256': 'hash',
          'invocation_sha256': 'hash',
          'expires_at_ms': 1,
          'arguments': ['synthetic'],
          'environment': {'TOKEN': 'synthetic'},
        }),
      ),
    );
    final review = KubeExecReview.fromBytes(bytes);
    expect(bytes.every((value) => value == 0), isTrue);
    review.dispose();
    final invalid = Uint8List.fromList([1, 2, 3]);
    expect(() => KubeExecReview.fromBytes(invalid), throwsFormatException);
    expect(invalid, [0, 0, 0]);
  });
}
