import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_flux_cli_service.dart';
import 'package:consolecrypt/kubernetes/kube_kubectl_panel.dart';
import 'package:consolecrypt/kubernetes/kube_kubectl_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

const _scope = KubeScope('fixture', true);
const _review = FluxInstallReview(
  ticket: 'reviewed-install',
  method: 'homebrew',
  operation: 'install',
  version: '',
  source: 'https://kubernetes.io/docs/tasks/tools/install-kubectl-macos/',
  destination: '',
  executable: '/opt/homebrew/bin/brew',
  arguments: ['install', 'kubectl'],
);

class _Service implements KubeService, KubeKubectlService, KubeFluxCliService {
  bool installed = false;
  int probes = 0, approvals = 0;
  final cancelled = <String>[];
  Completer<FluxInstallReview>? heldReview;
  @override
  Future<KubectlDiscovery> discoverKubectl() async => KubectlDiscovery(
    supported: true,
    documentation: _review.source,
    methods: const ['homebrew'],
    candidates: installed ? ['/opt/homebrew/bin/kubectl'] : [],
  );
  @override
  Future<FluxCliVersion> probeKubectl(String path) async {
    probes++;
    return FluxCliVersion(path: path, version: '1.32.4', sha256: 'a' * 64);
  }

  @override
  Future<FluxInstallReview> reviewKubectl(String method) async => heldReview?.future ?? _review;
  @override
  Future<String?> approveFlux(FluxInstallReview review) async {
    expect(review.ticket, _review.ticket);
    approvals++;
    return 'visible-package-terminal';
  }

  @override
  Future<void> cancelFlux(String ticket) async => cancelled.add(ticket);
  @override
  dynamic noSuchMethod(Invocation invocation) => throw UnimplementedError();
}

Future<ProviderContainer> _pump(WidgetTester tester, _Service service, List<String> opened) async {
  await tester.pumpWidget(
    ProviderScope(
      overrides: [kubeScopeProvider.overrideWithValue(_scope), kubeServiceProvider.overrideWithValue(service)],
      child: MaterialApp(
        localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
        supportedLocales: AppLocalizations.supportedLocales,
        home: Scaffold(
          body: SingleChildScrollView(child: KubeKubectlPanel(onTerminalOpened: opened.add)),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
  return ProviderScope.containerOf(tester.element(find.byType(KubeKubectlPanel)));
}

void main() {
  testWidgets('kubectl discovery does not execute; installation shows exact command before approval', (tester) async {
    final service = _Service();
    final opened = <String>[];
    await _pump(tester, service, opened);
    expect(service.probes + service.approvals, 0);
    await tester.tap(find.text('Install kubectl'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 350));
    expect(find.text('["/opt/homebrew/bin/brew","install","kubectl"]'), findsOneWidget);
    expect(service.approvals, 0);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(service.cancelled, [_review.ticket]);
    expect(opened, isEmpty);
    await tester.tap(find.text('Install kubectl'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 350));
    await tester.tap(find.text('Continue'));
    await tester.pumpAndSettle();
    expect(service.approvals, 1);
    expect(opened, ['visible-package-terminal']);
    expect(tester.takeException(), isNull);
  });

  testWidgets('locking while installation review is loading cancels without executing', (tester) async {
    final service = _Service()..heldReview = Completer<FluxInstallReview>();
    final opened = <String>[];
    final container = await _pump(tester, service, opened);
    await tester.tap(find.text('Install kubectl'));
    await tester.pump();
    container.updateOverrides([
      kubeScopeProvider.overrideWithValue(const KubeScope('fixture', false)),
      kubeServiceProvider.overrideWithValue(service),
    ]);
    await tester.pump();
    service.heldReview!.complete(_review);
    await tester.pumpAndSettle();
    expect(service.cancelled, [_review.ticket]);
    expect(service.approvals, 0);
    expect(opened, isEmpty);
  });

  testWidgets('an installed kubectl is checked only by its explicit action', (tester) async {
    final service = _Service()..installed = true;
    await _pump(tester, service, []);
    expect(service.probes, 0);
    await tester.tap(find.text('Check kubectl'));
    await tester.pumpAndSettle();
    expect(service.probes, 1);
    expect(find.text('kubectl 1.32.4'), findsOneWidget);
    expect(service.approvals, 0);
  });
}
