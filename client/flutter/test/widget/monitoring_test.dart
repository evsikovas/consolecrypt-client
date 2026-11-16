import 'dart:async';
import 'dart:io' as io;

import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/navigation_customization.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/secret_field.dart';
import 'package:consolecrypt/credentials/credential_dialogs.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_providers.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/monitoring_binding_fakes.dart';
import '../helpers/monitoring_binding_reads_fakes.dart';
import '../helpers/monitoring_fakes.dart';
import '../helpers/test_app.dart';

const _credentialId = 'cc111111-1111-4111-8111-111111111111';
Future<ProviderContainer> _open(
  WidgetTester tester,
  MonitoringTestBackend service, {
  Size size = const Size(1400, 1000),
  AppLocale locale = AppLocale.en,
  List<Host> hosts = const [],
  bool realCredentials = false,
}) async {
  await loadAutomationProofFonts(tester);
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugCreateUnlockedLocalProfile(name: 'Monitoring test');
  if (io.Platform.environment['CONSOLECRYPT_UI_PROOF_DIR'] != null) {
    final settings = backend.services.settings;
    await settings.updateLocal(settings.currentLocal.copyWith(themeMode: AppThemeMode.dark));
  }
  await pumpApp(
    tester,
    backend,
    size: size,
    locale: locale,
    captureBoundary: true,
    overrides: [
      monitoringBackendProvider.overrideWithValue(service),
      hostsProvider.overrideWith((ref) => Stream.value(hosts)),
      if (!realCredentials)
        credentialsProvider.overrideWith(
          (ref) => Stream.value([
            Credential(
              id: const ObjectId(_credentialId),
              name: 'Saved metrics credential',
              kind: CredentialKind.password,
              username: 'metrics-user',
              secretId: const ObjectId('cc222222-2222-4222-8222-222222222222'),
              createdAt: DateTime(2026),
              updatedAt: DateTime(2026),
            ),
            Credential(
              id: const ObjectId('cc333333-3333-4333-8333-333333333333'),
              name: 'SSH key is not eligible',
              kind: CredentialKind.sshPrivateKey,
              createdAt: DateTime(2026),
              updatedAt: DateTime(2026),
            ),
          ]),
        ),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.monitoring);
  await settle(tester);
  return container;
}

class _SshRouteReader extends MonitoringTestService {
  int probes = 0;
  MonitoringSource? probedSource;
  @override
  Future<SourceCapabilities> probe(MonitoringSource source) async {
    probes++; probedSource = source; return super.probe(source);
  }
}
Host _routeHost(String id, String name, {HostProtocol protocol = HostProtocol.ssh, SshBackend backend = SshBackend.native}) =>
  Host(id: ObjectId(id), name: name, address: '192.0.2.17', createdAt: DateTime(2026), updatedAt: DateTime(2026),
    protocol: protocol, backend: backend);

class _AccessPolicyReader extends MonitoringTestService {
  int probes = 0, metadataReads = 0;
  @override
  Future<SourceCapabilities> probe(MonitoringSource source) async {
    probes++;
    return super.probe(source);
  }
  @override
  Future<LabelValues> labelValues(MonitoringSource source, String label) async {
    metadataReads++;
    return super.labelValues(source, label);
  }
}

void main() {
  for (final locale in [AppLocale.en, AppLocale.ru]) {
    testWidgets('source access policy ${locale.name} is explicit without probing and preserves configured tenant endpoint and credential reference', (tester) async {
      final reader = _AccessPolicyReader();
      final backend = MonitoringTestBackend(store: MonitoringTestStore(initial: []), service: reader);
      await _open(tester, backend, locale: locale, size: const Size(520, 820));
      final notice = find.byKey(const ValueKey('monitoring-source-access-policy'));
      await tester.ensureVisible(notice);
      await settle(tester);
      final l = tester.element(notice).l10n;
      expect(find.text(l.monitoringSourceAccessPolicy), findsOneWidget);
      expect(reader.probes, 0);
      expect(reader.queries, 0);
      expect(reader.metadataReads, 0);
      expect(backend.store.sources, isEmpty);
      const endpoint = 'https://metrics.example.test/select/team-a/prometheus';
      await enterKey(tester, 'monitoring-source-name', 'Backend-scoped metrics');
      await enterKey(tester, 'monitoring-source-endpoint', endpoint);
      tester.widget<GlassSelect<SourceAuth>>(find.byType(GlassSelect<SourceAuth>)).onChanged!(SourceAuth.bearerRef);
      await settle(tester);
      tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('monitoring-credential-ref'))).onChanged!(_credentialId);
      await settle(tester);
      expect(find.text(l.monitoringSourceAccessPolicy), findsOneWidget);
      await tapKey(tester, 'monitoring-save-source');
      final saved = backend.store.sources.single;
      expect(saved.endpoint, endpoint);
      expect(saved.auth, SourceAuth.bearerRef);
      expect(saved.credentialId, _credentialId);
      expect(reader.probes, 0);
      expect(reader.queries, 0);
      expect(reader.metadataReads, 0);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('SSH route uses only explicit saved native SSH host and keeps URL Basic and custom CA on Save', (tester) async {
    final ssh = _routeHost('a1111111-1111-4111-8111-111111111111', 'Native SSH route');
    final rdp = _routeHost('a2222222-2222-4222-8222-222222222222', 'RDP not a route', protocol: HostProtocol.rdp);
    final system = _routeHost('a3333333-3333-4333-8333-333333333333', 'OpenSSH compatibility route', backend: SshBackend.openSsh);
    final reader = _SshRouteReader();
    final backend = MonitoringTestBackend(store: MonitoringTestStore(initial: []), service: reader);
    await _open(tester, backend, hosts: [ssh, rdp, system]);
    const url = 'http://10.10.10.11:3000/api/datasources/proxy/1';
    await enterKey(tester, 'monitoring-source-name', 'Explicit SSH source');
    await enterKey(tester, 'monitoring-source-endpoint', url);
    tester.widget<GlassSelect<SourceRoute>>(find.byKey(const ValueKey('monitoring-source-route'))).onChanged!(SourceRoute.sshHost);
    await settle(tester);
    await tapKey(tester, 'monitoring-source-ssh-host');
    expect(find.byKey(const ValueKey('pick-Native SSH route')), findsOneWidget);
    expect(find.byKey(const ValueKey('pick-RDP not a route')), findsNothing);
    expect(find.byKey(const ValueKey('pick-OpenSSH compatibility route')), findsNothing);
    await tapKey(tester, 'pick-Native SSH route');
    expect(backend.store.sources, isEmpty);
    expect(reader.queries, 0); expect(reader.probes, 0);
    tester.widget<GlassSelect<SourceAuth>>(find.byType(GlassSelect<SourceAuth>)).onChanged!(SourceAuth.basicRef);
    await settle(tester);
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('monitoring-credential-ref'))).onChanged!(_credentialId);
    await settle(tester);
    await tester.ensureVisible(find.text('Custom TLS CA certificate'));
    await tester.tap(find.text('Custom TLS CA certificate'));
    await settle(tester);
    final ca = find.byWidgetPredicate((w) => w is TextField && w.decoration?.labelText == 'PEM certificate (optional)');
    await tester.ensureVisible(ca);
    await tester.enterText(ca, 'opaque CA fixture');
    await tapKey(tester, 'monitoring-save-source');
    final saved = backend.store.sources.single;
    expect(saved.name, 'Explicit SSH source'); expect(saved.endpoint, url);
    expect(saved.toJson()['route'], {'kind': 'sshHost', 'hostId': ssh.id.value});
    expect(saved.routeId, isNull); expect(saved.auth, SourceAuth.basicRef);
    expect(saved.credentialId, _credentialId); expect(saved.customCaPem, 'opaque CA fixture');
    expect(reader.probes, 0); expect(reader.queries, 0);
    expect(tester.widget<Text>(find.byKey(const ValueKey('monitoring-source-ssh-host-name'))).data, ssh.name);
    await tapKey(tester, 'monitoring-probe');
    expect(reader.probes, 1); expect(reader.probedSource!.hostId, ssh.id.value);
    expect(reader.queries, 0);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('legacy tunnel source stays a legacy reference after an ordinary name edit', (tester) async {
    const legacy = MonitoringSource(id: 'legacy-source', name: 'Legacy', endpoint: 'https://metrics.test/base',
      route: SourceRoute.sshTunnel, routeId: 'old-route-reference', revision: 3);
    final reader = _SshRouteReader();
    final backend = MonitoringTestBackend(store: MonitoringTestStore(initial: [legacy]), service: reader);
    await _open(tester, backend);
    await tester.tap(find.text('Edit source')); await settle(tester);
    expect(find.text('Via saved SSH host'), findsNothing);
    expect(find.text('old-route-reference'), findsOneWidget);
    await enterKey(tester, 'monitoring-source-name', 'Renamed legacy');
    await tapKey(tester, 'monitoring-save-source');
    final saved = backend.store.sources.single;
    expect(saved.toJson()['route'], {'kind': 'sshTunnel', 'routeId': 'old-route-reference'});
    expect(saved.hostId, isNull); expect(reader.probes, 0); expect(reader.queries, 0);
    expect(tester.takeException(), isNull);
  });
  testWidgets('SSH host picker retires on hiding and cannot choose a route or query after returning', (tester) async {
    final ssh = _routeHost('a1111111-1111-4111-8111-111111111111', 'Pending SSH route');
    final reader = _SshRouteReader();
    final backend = MonitoringTestBackend(store: MonitoringTestStore(initial: []), service: reader);
    final container = await _open(tester, backend, hosts: [ssh], size: const Size(620, 950));
    await enterKey(tester, 'monitoring-source-name', 'Keep route draft');
    await enterKey(tester, 'monitoring-source-endpoint', 'https://metrics.test/base');
    tester.widget<GlassSelect<SourceRoute>>(find.byKey(const ValueKey('monitoring-source-route'))).onChanged!(SourceRoute.sshHost);
    await settle(tester); await tapKey(tester, 'monitoring-source-ssh-host');
    expect(find.byKey(const ValueKey('pick-Pending SSH route')), findsOneWidget);
    container.read(routerProvider).go(AppRoutes.terminal); await settle(tester);
    container.read(routerProvider).go(AppRoutes.monitoring); await settle(tester);
    expect(find.byKey(const ValueKey('pick-Pending SSH route')), findsNothing);
    expect(find.text('Choose SSH host'), findsOneWidget);
    expect(backend.store.sources, isEmpty); expect(reader.probes, 0); expect(reader.queries, 0);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('monitoring-source-name'))).controller!.text, 'Keep route draft');
    expect(tester.takeException(), isNull);
  });
  testWidgets('changing source route rejects a late old direct query without automatic replacement read', (tester) async {
    final ssh = _routeHost('a1111111-1111-4111-8111-111111111111', 'New SSH route');
    final reader = _SshRouteReader()..pending = Completer<QueryOutcome>();
    final backend = MonitoringTestBackend(service: reader);
    await _open(tester, backend, hosts: [ssh]);
    await tapKey(tester, 'monitoring-query');
    expect(reader.queries, 1);
    await tapKey(tester, 'monitoring-new-source');
    await enterKey(tester, 'monitoring-source-name', 'Changed route source');
    await enterKey(tester, 'monitoring-source-endpoint', 'https://metrics.test/base');
    tester.widget<GlassSelect<SourceRoute>>(find.byKey(const ValueKey('monitoring-source-route'))).onChanged!(SourceRoute.sshHost);
    await settle(tester); await tapKey(tester, 'monitoring-source-ssh-host'); await tapKey(tester, 'pick-New SSH route');
    reader.pending!.complete(reader.result); await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-result-status')), findsNothing);
    await tapKey(tester, 'monitoring-save-source');
    expect(reader.queries, 1); expect(reader.probes, 0);
    expect(backend.store.sources.last.hostId, ssh.id.value);
    expect(find.byKey(const ValueKey('monitoring-result-status')), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets('explicit saved binding panel uses native typed metadata and cancels when leaving the workspace', (
    tester,
  ) async {
    final store = MonitoringBindingTestStore(rows: [bindingFixture()]);
    final reader = MonitoringBindingReadTestService();
    final container = await _open(
      tester,
      MonitoringTestBackend(store: store, service: reader),
      locale: AppLocale.ru,
      size: const Size(1500, 1200),
    );
    expect(reader.panels, isEmpty);
    expect(reader.discoveries, isEmpty);
    expect(find.byKey(const ValueKey('monitoring-binding-reader')), findsNothing);
    await tapKey(tester, 'monitoring-binding-inspect-${store.bindings.single.id}');
    expect(reader.panels, isEmpty);
    tester.widget<GlassSelect<MonitoringPanel>>(find.byKey(const ValueKey('monitoring-binding-panel'))).onChanged!(
      MonitoringPanel.docker,
    );
    await settle(tester);
    await enterKey(tester, 'monitoring-binding-start', '1970-01-01T00:00:00Z');
    await enterKey(tester, 'monitoring-binding-end', '1970-01-01T00:16:40Z');
    await tapKey(tester, 'monitoring-binding-apply-window');
    await tapKey(tester, 'monitoring-binding-load-panel');
    expect(reader.panels.single.$3, MonitoringPanel.docker);
    final l = tester.element(find.byKey(const ValueKey('monitoring-binding-reader'))).l10n;
    expect(find.text(l.monitoringUnitCpuCores), findsOneWidget);
    expect(find.text(l.monitoringMetricContainerCpu), findsOneWidget);
    await tester.ensureVisible(find.byKey(const ValueKey('monitoring-panel-query-containerCpu')));
    await settle(tester);
    await captureAutomationWorkspace(tester, 'monitoring-binding-docker-panel-desktop-ru');
    final pending = Completer<BindingReadResult>();
    reader.pendingBinding = pending;
    await tapKey(tester, 'monitoring-binding-discover');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    pending.complete(BindingReadResult.fromJson(bindingDiscoveryJson()));
    await settle(tester);
    expect(reader.bindingCancellations, greaterThan(0));
    container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-binding-reader')), findsNothing);
    expect(reader.panels.length, 1);
    expect(reader.discoveries.length, 1);
    expect(reader.queries, 0);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  testWidgets(
    'binding host chooser uses saved SSH/RDP records and closes on lock without assigning by label or address',
    (tester) async {
      final store = MonitoringBindingTestStore();
      final rdp = Host(
        id: ObjectId(bindingHost.id),
        name: 'Explicit RDP fixture',
        address: 'manual:9100',
        protocol: HostProtocol.rdp,
        createdAt: DateTime(2026),
        updatedAt: DateTime(2026),
      );
      final container = await _open(
        tester,
        MonitoringTestBackend(store: store),
        hosts: [rdp],
        locale: AppLocale.ru,
        size: const Size(1500, 1050),
      );
      await tapKey(tester, 'monitoring-binding-add');
      await tapKey(tester, 'monitoring-binding-host');
      expect(find.byKey(const ValueKey('host-picker')), findsOneWidget);
      await tapKey(tester, 'pick-Explicit RDP fixture');
      expect(find.text('Explicit RDP fixture'), findsOneWidget);
      expect(
        tester.widget<TextField>(find.byKey(const ValueKey('monitoring-binding-value-0'))).controller!.text,
        isEmpty,
      );
      expect(store.saves, isEmpty);
      await enterKey(tester, 'monitoring-binding-name', 'Node exporter');
      await enterKey(tester, 'monitoring-binding-label-0', 'job');
      await enterKey(tester, 'monitoring-binding-value-0', 'node');
      await tapKey(tester, 'monitoring-binding-add-label');
      await enterKey(tester, 'monitoring-binding-label-1', 'instance');
      await enterKey(tester, 'monitoring-binding-value-1', 'manual:9100');
      await tester.ensureVisible(find.byKey(const ValueKey('monitoring-bindings')));
      await settle(tester);
      await captureAutomationWorkspace(tester, 'monitoring-host-binding-desktop-ru');
      await tapKey(tester, 'monitoring-binding-host');
      await container.read(vaultServiceProvider).lock();
      await settle(tester);
      expect(find.byKey(const ValueKey('host-picker')), findsNothing);
      expect(find.byKey(const ValueKey('monitoring-binding-name')), findsNothing);
      expect(store.saves, isEmpty);
      expect(tester.takeException(), isNull);
    },
    variant: const TargetPlatformVariant({TargetPlatform.windows}),
  );

  testWidgets('changing a source invalidates an in-flight query response', (tester) async {
    final store = MonitoringTestStore(
      initial: [
        monitoringFixture,
        monitoringFixture.withRecord(id: 'c0a26470-e5af-4dcc-b37f-d71a65fd7899', revision: 2),
      ],
    );
    final service = MonitoringTestBackend(
      store: store,
      service: MonitoringTestService()..pending = Completer<QueryOutcome>(),
    );
    await _open(tester, service);
    await tapKey(tester, 'monitoring-query');
    tester.widget<GlassSelect<String?>>(find.byType(GlassSelect<String?>).first).onChanged!(store.sources.last.id);
    await settle(tester);
    service.service.pending!.complete(service.service.result);
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-result-status')), findsNothing);
    expect(service.service.cancellations, greaterThan(0));
    expect(tester.takeException(), isNull);
  });
  testWidgets('profile change disposes query state and a late old-profile response', (tester) async {
    final service = MonitoringTestBackend(service: MonitoringTestService()..pending = Completer<QueryOutcome>());
    final container = await _open(tester, service);
    await enterKey(tester, 'monitoring-expression', 'fixture_profile_expression');
    await tapKey(tester, 'monitoring-query');
    await container.read(profileServiceProvider).createLocalProfile(name: 'Other monitoring profile');
    await container
        .read(vaultServiceProvider)
        .createVault(name: 'Other monitoring profile', passphrase: SecretText('fixture-only-passphrase-123'));
    await container.read(vaultServiceProvider).confirmRecoveryKitSaved();
    await settle(tester);
    service.service.pending!.complete(service.service.result);
    await settle(tester);
    container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    expect(tester.widget<TextField>(find.byKey(const ValueKey('monitoring-expression'))).controller!.text, 'up');
    expect(find.byKey(const ValueKey('monitoring-result-status')), findsNothing);
    expect(find.textContaining('fixture_profile_expression'), findsNothing);
    expect(tester.takeException(), isNull);
  });
  testWidgets('typed error and empty data remain visible and session history never auto-runs', (tester) async {
    final service = MonitoringTestBackend(service: MonitoringTestService()..pending = Completer<QueryOutcome>());
    await _open(tester, service);
    await tapKey(tester, 'monitoring-query');
    service.service.pending!.completeError(const MonitoringFailure('timeout', 'Fixture timeout'));
    await settle(tester);
    expect(find.textContaining('The source request timed out.'), findsOneWidget);
    service.service.pending = null;
    service.service.result = const QueryOutcome(status: 'success', httpStatus: 200, data: QueryData('matrix'));
    await tapKey(tester, 'monitoring-query');
    expect(find.byKey(const ValueKey('monitoring-no-data')), findsOneWidget);
    final previous = find.textContaining('Fixture metrics · current · up');
    await tester.ensureVisible(previous);
    await settle(tester);
    await tester.tap(previous);
    await settle(tester);
    expect(service.service.queries, 2);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Monitoring is optional and absent when a backend is unavailable', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugCreateUnlockedLocalProfile();
    await pumpApp(tester, backend);
    await tapKey(tester, 'nav-all-sections');
    expect(find.byKey(const ValueKey('all-nav-monitoring')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('first native source can be saved and deleted only after explicit confirmation', (tester) async {
    final service = MonitoringTestBackend(store: MonitoringTestStore(initial: []));
    await _open(tester, service);
    await enterKey(tester, 'monitoring-source-name', 'New metrics');
    await enterKey(tester, 'monitoring-source-endpoint', 'http://127.0.0.1:9090');
    await tapKey(tester, 'monitoring-save-source');
    expect(service.store.sources.single.name, 'New metrics');
    expect(service.store.sources.single.id, endsWith('7890'));
    await tapKey(tester, 'monitoring-delete-source');
    expect(service.store.sources.length, 1);
    await tapKey(tester, 'monitoring-delete-confirm');
    expect(service.store.sources, isEmpty);
    expect(tester.takeException(), isNull);
  });
  testWidgets('credential metadata picker exposes references only and Bearer authentication requires HTTPS', (
    tester,
  ) async {
    final service = MonitoringTestBackend();
    await _open(tester, service);
    await tester.tap(find.text('Edit source'));
    await settle(tester);
    tester.widget<GlassSelect<SourceAuth>>(find.byType(GlassSelect<SourceAuth>)).onChanged!(SourceAuth.bearerRef);
    await settle(tester);
    final picker = tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('monitoring-credential-ref')));
    expect(picker.items.map((i) => i.label).join(), contains('Saved metrics credential'));
    expect(picker.items.map((i) => i.label).join(), isNot(contains('SSH key')));
    expect(find.widgetWithText(TextField, 'Credential reference ID'), findsNothing);
    picker.onChanged!(_credentialId);
    await settle(tester);
    await tapKey(tester, 'monitoring-save-source');
    final l = tester.element(find.byKey(const ValueKey('monitoring-save-source'))).l10n;
    expect(find.text(l.monitoringHttpAuth), findsOneWidget);
    await enterKey(tester, 'monitoring-source-endpoint', 'https://metrics.example.test/prometheus');
    await tapKey(tester, 'monitoring-save-source');
    expect(service.store.sources.single.auth, SourceAuth.bearerRef);
    expect(service.store.sources.single.credentialId, _credentialId);
    expect(tester.takeException(), isNull);
  });
  testWidgets('explicit HTTP Basic keeps the configured endpoint and shows a plaintext notice without approval', (
    tester,
  ) async {
    final backend = MonitoringTestBackend();
    await _open(tester, backend);
    await tester.tap(find.text('Edit source'));
    await settle(tester);
    const endpoint = 'http://10.10.10.11:3000/api/datasources/proxy/1';
    await enterKey(tester, 'monitoring-source-endpoint', endpoint);
    tester.widget<GlassSelect<SourceAuth>>(find.byType(GlassSelect<SourceAuth>)).onChanged!(SourceAuth.basicRef);
    await settle(tester);
    tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('monitoring-credential-ref'))).onChanged!(
      _credentialId,
    );
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-http-basic-notice')), findsOneWidget);
    expect(find.byType(PasswordCredentialDialog), findsNothing);
    await enterKey(tester, 'monitoring-source-endpoint', 'https://metrics.test/prometheus');
    expect(find.byKey(const ValueKey('monitoring-http-basic-notice')), findsNothing);
    await enterKey(tester, 'monitoring-source-endpoint', endpoint);
    expect(find.byKey(const ValueKey('monitoring-http-basic-notice')), findsOneWidget);
    await tapKey(tester, 'monitoring-save-source');
    expect(backend.store.sources.single.endpoint, endpoint);
    expect(backend.store.sources.single.auth, SourceAuth.basicRef);
    expect(backend.store.sources.single.credentialId, _credentialId);
    expect(tester.takeException(), isNull);
  });
  testWidgets('Add credentials uses the secure existing editor, refreshes and selects the new password reference', (
    tester,
  ) async {
    final backend = MonitoringTestBackend();
    final container = await _open(tester, backend, realCredentials: true);
    await tester.tap(find.text('Edit source'));
    await settle(tester);
    await enterKey(tester, 'monitoring-source-name', 'Unsaved Basic source');
    tester.widget<GlassSelect<SourceAuth>>(find.byType(GlassSelect<SourceAuth>)).onChanged!(SourceAuth.basicRef);
    await settle(tester);
    await tapKey(tester, 'monitoring-add-credential');
    expect(find.byType(PasswordCredentialDialog), findsOneWidget);
    await enterKey(tester, 'password-credential-name', 'New monitoring login');
    await enterKey(tester, 'password-credential-username', 'admin');
    final passwordField = find.descendant(of: find.byType(SecretField), matching: find.byType(TextField));
    expect(tester.widget<TextField>(passwordField).obscureText, isTrue);
    await tester.enterText(passwordField, 'fixture-only-password');
    await tapKey(tester, 'credential-save');
    expect(find.byType(PasswordCredentialDialog), findsNothing);
    final created = container
        .read(credentialsProvider)
        .requireValue
        .singleWhere((c) => c.name == 'New monitoring login');
    final picker = tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('monitoring-credential-ref')));
    expect(picker.value, created.id.value);
    expect(picker.items.where((item) => item.value == created.id.value).single.label, contains('admin'));
    expect(find.byType(SecretField), findsNothing);
    expect(backend.store.sources.single.name, 'Fixture metrics'); // Adding a credential does not save the source.
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('monitoring-source-name'))).controller!.text,
      'Unsaved Basic source',
    );
    await tapKey(tester, 'monitoring-save-source');
    expect(backend.store.sources.single.auth, SourceAuth.basicRef);
    expect(backend.store.sources.single.credentialId, created.id.value);
    expect(backend.store.sources.single.toJson().toString(), isNot(contains('fixture-only-password')));
    expect(tester.takeException(), isNull);
  });
  testWidgets('credential editor cancel preserves the source draft and profile lock closes the secure route', (
    tester,
  ) async {
    final backend = MonitoringTestBackend();
    final container = await _open(tester, backend, realCredentials: true);
    await tester.tap(find.text('Edit source'));
    await settle(tester);
    await enterKey(tester, 'monitoring-source-name', 'Keep draft');
    tester.widget<GlassSelect<SourceAuth>>(find.byType(GlassSelect<SourceAuth>)).onChanged!(SourceAuth.basicRef);
    await settle(tester);
    await tapKey(tester, 'monitoring-add-credential');
    final l = tester.element(find.byType(PasswordCredentialDialog)).l10n;
    await tester.tap(find.text(l.commonCancel));
    await settle(tester);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('monitoring-source-name'))).controller!.text,
      'Keep draft',
    );
    await tapKey(tester, 'monitoring-add-credential');
    await enterKey(tester, 'password-credential-name', 'Discard editor');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    expect(find.byType(PasswordCredentialDialog), findsNothing);
    expect(find.byKey(const ValueKey('monitoring-credential-ref')), findsNothing);
    expect(backend.store.sources.single.name, 'Fixture metrics');
    expect(tester.takeException(), isNull);
  });
  testWidgets('Basic credential creation without a username stays unselected and cannot save the source', (
    tester,
  ) async {
    final backend = MonitoringTestBackend();
    final container = await _open(tester, backend, realCredentials: true);
    await tester.tap(find.text('Edit source'));
    await settle(tester);
    tester.widget<GlassSelect<SourceAuth>>(find.byType(GlassSelect<SourceAuth>)).onChanged!(SourceAuth.basicRef);
    await settle(tester);
    await tapKey(tester, 'monitoring-add-credential');
    await enterKey(tester, 'password-credential-name', 'Token without username');
    await tester.enterText(
      find.descendant(of: find.byType(SecretField), matching: find.byType(TextField)),
      'fixture-only-token',
    );
    await tapKey(tester, 'credential-save');
    expect(find.byType(PasswordCredentialDialog), findsOneWidget);
    final l = tester.element(find.byType(PasswordCredentialDialog)).l10n;
    expect(find.text(l.validationRequired(l.credentialsDetailsUsername)), findsOneWidget);
    expect(container.read(credentialsProvider).requireValue.any((c) => c.name == 'Token without username'), isFalse);
    await tester.tap(find.text(l.commonCancel));
    await settle(tester);
    final picker = tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('monitoring-credential-ref')));
    expect(picker.value, isNull);
    expect(picker.items.map((item) => item.label), isNot(contains('Token without username')));
    await tapKey(tester, 'monitoring-save-source');
    expect(backend.store.sources.single.auth, SourceAuth.none);
    expect(find.byKey(const ValueKey('monitoring-error')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('leaving Monitoring dismisses credential creation without selecting or saving a late result', (
    tester,
  ) async {
    final backend = MonitoringTestBackend();
    final container = await _open(tester, backend, realCredentials: true);
    await tester.tap(find.text('Edit source'));
    await settle(tester);
    await enterKey(tester, 'monitoring-source-name', 'Keep on return');
    tester.widget<GlassSelect<SourceAuth>>(find.byType(GlassSelect<SourceAuth>)).onChanged!(SourceAuth.basicRef);
    await settle(tester);
    await tapKey(tester, 'monitoring-add-credential');
    await enterKey(tester, 'password-credential-name', 'Not saved');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(find.byType(PasswordCredentialDialog), findsNothing);
    container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    expect(
      tester.widget<TextField>(find.byKey(const ValueKey('monitoring-source-name'))).controller!.text,
      'Keep on return',
    );
    expect(tester.widget<GlassSelect<String?>>(find.byKey(const ValueKey('monitoring-credential-ref'))).value, isNull);
    expect(container.read(credentialsProvider).requireValue.any((c) => c.name == 'Not saved'), isFalse);
    expect(backend.store.sources.single.name, 'Fixture metrics');
    expect(tester.takeException(), isNull);
  });
  for (final width in [580.0, 1400.0]) {
    testWidgets('HTTP Basic source form shows the warning and Add credentials at width $width', (tester) async {
      await _open(tester, MonitoringTestBackend(), size: Size(width, 1100), locale: AppLocale.ru);
      final l = tester.element(find.byKey(const ValueKey('monitoring-probe'))).l10n;
      await tester.tap(find.text(l.monitoringEditSource));
      await settle(tester);
      await enterKey(tester, 'monitoring-source-endpoint', 'http://10.10.10.11:3000/api/datasources/proxy/1');
      tester.widget<GlassSelect<SourceAuth>>(find.byType(GlassSelect<SourceAuth>)).onChanged!(SourceAuth.basicRef);
      await settle(tester);
      expect(find.text(l.monitoringAddCredential), findsOneWidget);
      expect(find.text(l.monitoringHttpBasicPlaintext), findsOneWidget);
      await tester.ensureVisible(find.byKey(const ValueKey('monitoring-http-basic-notice')));
      await settle(tester);
      await captureAutomationWorkspace(tester, 'monitoring-http-basic-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
  testWidgets('per-endpoint capability and warnings partial nonfinite data remain explicit', (tester) async {
    final service = MonitoringTestBackend();
    await _open(tester, service);
    await tapKey(tester, 'monitoring-probe');
    expect(find.text('Compatibility not confirmed'), findsOneWidget);
    expect(find.textContaining('The source returned an unsupported or invalid response.'), findsOneWidget);
    expect(find.textContaining('Fixture range unavailable'), findsNothing);
    await tapKey(tester, 'monitoring-query');
    await tester.ensureVisible(find.byKey(const ValueKey('monitoring-result-status')));
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-partial')), findsOneWidget);
    expect(find.text('Warning: fixture partial warning'), findsOneWidget);
    expect(find.text('NaN'), findsOneWidget);
    expect(find.text('+Inf'), findsOneWidget);
    expect(find.text('-Inf'), findsOneWidget);
    await captureAutomationWorkspace(tester, 'monitoring-native-surface');
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('switching to Terminal cancels the hidden request and late output cannot restore it', (tester) async {
    final service = MonitoringTestBackend(service: MonitoringTestService()..pending = Completer<QueryOutcome>());
    final container = await _open(tester, service);
    await tapKey(tester, 'monitoring-query');
    container.read(routerProvider).go(AppRoutes.terminal);
    await settle(tester);
    expect(service.service.cancellations, greaterThan(0));
    service.service.pending!.complete(service.service.result);
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-result-status')), findsNothing);
    container.read(routerProvider).go(AppRoutes.monitoring);
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-result-status')), findsNothing);
    expect(service.service.queries, 1);
    expect(tester.takeException(), isNull);
  });
  testWidgets('locking removes query state and rejects a late response', (tester) async {
    final service = MonitoringTestBackend(service: MonitoringTestService()..pending = Completer<QueryOutcome>());
    final container = await _open(tester, service);
    await tapKey(tester, 'monitoring-query');
    await container.read(vaultServiceProvider).lock();
    await settle(tester);
    service.service.pending!.complete(service.service.result);
    await settle(tester);
    expect(find.byKey(const ValueKey('monitoring-expression')), findsNothing);
    expect(find.text('fixture partial warning'), findsNothing);
    expect(service.service.cancellations, greaterThan(0));
    expect(tester.takeException(), isNull);
  });
  testWidgets('hiding the navigation item preserves a working workspace and command palette', (tester) async {
    final service = MonitoringTestBackend();
    final container = await _open(tester, service);
    final profile = container.read(activeProfileProvider)!.id.value;
    await container.read(navigationPreferencesControllerProvider).setVisible(profile, 'monitoring', true);
    await settle(tester);
    expect(find.byKey(const ValueKey('nav-monitoring')), findsOneWidget);
    await container.read(navigationPreferencesControllerProvider).setVisible(profile, 'monitoring', false);
    await settle(tester);
    expect(find.byKey(const ValueKey('nav-monitoring')), findsNothing);
    await tapKey(tester, 'monitoring-query');
    expect(service.service.queries, 1);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyK);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
    await settle(tester);
    expect(find.byKey(const ValueKey('command-palette')), findsOneWidget);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  testWidgets('Russian narrow connection and query controls are localized and scroll without overlap', (tester) async {
    await _open(tester, MonitoringTestBackend(), size: const Size(580, 850), locale: AppLocale.ru);
    expect(find.text('Мониторинг'), findsWidgets);
    await tapKey(tester, 'monitoring-probe');
    expect(find.text('Совместимость не подтверждена'), findsOneWidget);
    await tapKey(tester, 'monitoring-query');
    await tester.ensureVisible(find.byKey(const ValueKey('monitoring-result-status')));
    await settle(tester);
    expect(find.text('Предупреждение: fixture partial warning'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
