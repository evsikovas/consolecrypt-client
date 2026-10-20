import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_flux_cli_panel.dart';
import 'package:consolecrypt/kubernetes/kube_flux_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_flux_preferences_service.dart';
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

class _Flux implements KubeFluxCliService, KubeFluxPreferencesService {
  int discoveries = 0, probes = 0, approvals = 0, runs = 0;
  final cancelled = <String>[];
  Completer<FluxCliOutput>? pending;
  FluxCliOutput? lastOutput;
  FluxPreferences preferences = FluxPreferences();
  String? lastManual;
  int preferencesSaved = 0, pathWrites = 0;
  @override
  Future<FluxPreferencesView> fluxPreferences({String? sessionId}) async => FluxPreferencesView(
    preferences: preferences,
    controllerVersion: '2.9.6',
    path: const FluxPathAdvice(
      directory: '/Users/fixture/.local/bin',
      present: false,
      canAdd: false,
      profileFile: '/Users/fixture/.zprofile',
      line: r'export PATH="$HOME/.local/bin:$PATH"',
    ),
  );
  @override
  Future<void> saveFluxPreferences(FluxPreferences value) async {
    preferencesSaved++;
    preferences = FluxPreferences.fromJson(value.toJson());
  }

  @override
  Future<FluxPathAdvice> addFluxUserPath() async {
    pathWrites++;
    return const FluxPathAdvice(directory: '/Users/fixture/.local/bin', present: true, canAdd: true);
  }

  @override
  Future<FluxCliDiscovery> discoverFlux({String? manualPath}) async {
    discoveries++;
    lastManual = manualPath;
    return const FluxCliDiscovery(supported: true, candidates: ['/usr/local/bin/flux'], methods: ['homebrew']);
  }

  @override
  Future<FluxCliVersion> probeFlux(String path) async {
    probes++;
    return FluxCliVersion(path: path, version: '2.9.6', sha256: 'a' * 64);
  }

  @override
  Future<FluxInstallReview> reviewFlux(String version, String method, String operation) async => FluxInstallReview(
    ticket: 'ticket',
    method: method,
    operation: operation,
    version: version,
    source: 'https://fluxcd.io/flux/installation/',
    destination: '/Users/fixture/.local/bin/flux',
    executable: '/opt/homebrew/bin/brew',
    arguments: ['install', 'fluxcd/tap/flux'],
  );
  @override
  Future<String?> approveFlux(FluxInstallReview review) async {
    approvals++;
    return null;
  }

  @override
  Future<void> cancelFlux(String ticket) async {
    cancelled.add(ticket);
  }

  @override
  Future<FluxCliOutput> runFlux(String sessionId, String path, KubeJson action) async {
    runs++;
    return pending?.future ?? (lastOutput = FluxCliOutput(exitCode: 0, stdout: 'synthetic output', stderr: ''));
  }

  @override
  Future<String> openFluxTerminal(String sessionId, String path) async => 'terminal';
}

Future<ProviderContainer> _pump(
  WidgetTester tester,
  _Flux service, {
  TargetPlatform platform = TargetPlatform.macOS,
  String language = 'en',
}) async {
  debugDefaultTargetPlatformOverride = platform;
  tester.view.physicalSize = const Size(412, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(
    ProviderScope(
      overrides: [
        kubeFluxCliServiceProvider.overrideWithValue(service),
        kubeScopeProvider.overrideWith((ref) => ref.watch(_scope)),
      ],
      child: MaterialApp(
        locale: Locale(language),
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context).copyWith(textScaler: const TextScaler.linear(1.5)),
          child: child!,
        ),
        home: const Scaffold(
          body: SingleChildScrollView(
            child: KubeFluxCliPanel(
              session: KubeSession(id: 'session', contextId: 'context', serverVersion: 'v1', production: false),
            ),
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
  return ProviderScope.containerOf(tester.element(find.byType(Scaffold)));
}

void _test(String name, Future<void> Function(WidgetTester) body) {
  testWidgets(name, (tester) async {
    try {
      await body(tester);
    } finally {
      debugDefaultTargetPlatformOverride = null;
    }
  });
}

Future<void> _tap(WidgetTester tester, String key) async {
  final target = find.byKey(ValueKey(key));
  await tester.ensureVisible(target);
  await tester.tap(target);
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 350));
}

void main() {
  _test('device preferences restore manual path without granting execution or changing PATH', (tester) async {
    final service = _Flux()
      ..preferences = FluxPreferences(manualPath: '/fixture/custom/flux', method: 'homebrew', dismissAdvice: true);
    await _pump(tester, service);
    expect(service.lastManual, '/fixture/custom/flux');
    expect(service.probes + service.approvals + service.runs + service.pathWrites, 0);
    await tester.enterText(find.byKey(const ValueKey('flux-manual-path')), '/fixture/new/flux');
    await _tap(tester, 'flux-find');
    expect(service.preferences.manualPath, '/fixture/new/flux');
    expect(service.preferences.dismissAdvice, isTrue);
    expect(service.preferencesSaved, 1);
  });
  for (final language in ['en', 'ru']) {
    _test('Flux discovery never executes, install requires explicit confirmation ($language)', (tester) async {
      final service = _Flux();
      await _pump(tester, service, language: language);
      expect(service.discoveries, 1);
      expect(service.probes, 0);
      expect(service.approvals, 0);
      expect(service.runs, 0);
      await _tap(tester, 'flux-install');
      expect(service.approvals, 0);
      expect(find.textContaining('fluxcd/tap/flux'), findsOneWidget);
      expect(tester.takeException(), isNull);
      await _tap(tester, 'flux-confirm');
      expect(service.approvals, 1);
      expect(find.byKey(const ValueKey('flux-cli-dialog')), findsNothing);
    });
  }
  _test('mobile never discovers or offers Flux CLI', (tester) async {
    final service = _Flux();
    await _pump(tester, service, platform: TargetPlatform.android);
    expect(service.discoveries, 0);
    expect(find.byKey(const ValueKey('flux-install')), findsNothing);
  });
  _test('profile lock cancels an open install review', (tester) async {
    final service = _Flux();
    final container = await _pump(tester, service);
    await _tap(tester, 'flux-install');
    container.read(_scope.notifier).lock();
    await tester.pumpAndSettle();
    expect(service.approvals, 0);
    expect(service.cancelled, contains('ticket'));
    expect(find.byKey(const ValueKey('flux-cli-dialog')), findsNothing);
  });
  _test('late CLI output is cleared after lock without opening a dialog', (tester) async {
    final service = _Flux()..pending = Completer<FluxCliOutput>();
    final container = await _pump(tester, service);
    final run = find.byKey(const ValueKey('flux-run'));
    await tester.ensureVisible(run);
    await tester.tap(run);
    await tester.pump();
    container.read(_scope.notifier).lock();
    await tester.pump();
    final output = FluxCliOutput(exitCode: 0, stdout: 'synthetic secret', stderr: 'synthetic error');
    service.pending!.complete(output);
    await tester.pumpAndSettle();
    expect(output.stdout, '');
    expect(output.stderr, '');
    expect(find.byKey(const ValueKey('flux-cli-dialog')), findsNothing);
  });
}
