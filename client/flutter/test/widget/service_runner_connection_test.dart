import 'dart:async';

import 'package:consolecrypt/app/theme.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_runners_dialog.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_service_delegation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart' show settle, tapKey;

const _scope = AutomationScope('owned-service-profile', true);
const _identity = 'fd6afef1-3dc9-449c-8d8b-dcaa37477dda';
const _profile = AutomationRunnerProfile(
  id: 'saved-service', name: 'Existing service', revision: 7, readOnly: false,
  configuration: {'kind': 'service_remote', 'host_id': 'saved-host', 'identity_id': _identity},
);

class _Terminal extends Fake implements TerminalService {}

class _Legacy extends Fake implements AutomationRunnerService, AutomationRunnerEditingService,
    AutomationRunnerSetupService {
  final updates = <AutomationJson>[];
  int disconnects = 0;
  @override
  Future<AutomationRunnerCatalog> runners() async =>
      const AutomationRunnerCatalog(readOnly: false, runners: [_profile]);
  @override
  Future<AutomationRunnerProfile> updateRunner(String id, {
    required int revision, required String name, required AutomationJson configuration,
  }) async {
    expect(id, _profile.id); expect(revision, 7);
    updates.add(configuration);
    return _profile;
  }
  @override
  Future<void> disconnectRunner(String id) async { disconnects++; }
  @override
  Future<AutomationJson> setupRunner(AutomationJson target, {required String operationId, bool install = false}) =>
      throw StateError('Service metadata must not install an ordinary helper');
  @override
  Future<AutomationJson> connectRunner(String id) =>
      throw StateError('Service metadata must not use legacy connect');
}

class _Service extends _Legacy implements AutomationServiceDelegationService {
  @override
  Future<AutomationJson> prepareServiceDelegation(AutomationJson request) => throw StateError('No delegation on save');
  @override
  Future<AutomationJson> confirmServiceDelegation(AutomationJson request) => throw StateError('No confirmation on save');
  @override
  Future<AutomationJson> cancelServiceDelegation(AutomationJson request) => throw StateError('No cancellation on save');
  @override
  Future<AutomationJson> serviceDelegationStatus(AutomationJson request) => throw StateError('No status on open');
  @override
  Future<AutomationJson> revokeServiceDelegation(AutomationJson request) => throw StateError('No revocation on save');
}

Future<void> _open(WidgetTester tester, _Legacy service, {Locale locale = const Locale('en')}) async {
  await loadAutomationProofFonts(tester);
  await tester.binding.setSurfaceSize(const Size(820, 760));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(ProviderScope(
    overrides: [
      automationScopeProvider.overrideWithValue(_scope),
      automationRunnerServiceProvider.overrideWithValue(service),
      terminalServiceProvider.overrideWithValue(_Terminal()),
      hostsProvider.overrideWith((ref) => Stream.value([])),
    ],
    child: MaterialApp(
      theme: AppTheme.dark(platform: TargetPlatform.linux), locale: locale,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: const [AppLocalizations.delegate, ...GlobalMaterialLocalizations.delegates],
      home: const Scaffold(body: SizedBox()),
    ),
  ));
  unawaited(showGlassDialog<void>(tester.element(find.byType(Scaffold)),
    builder: (_) => AutomationRunnersDialog(scope: _scope, service: service, initialRunner: _profile)));
  await settle(tester);
}

void main() {
  for (final locale in [const Locale('en'), const Locale('ru')]) {
    testWidgets('service metadata saves fixed connection only: ${locale.languageCode}', (tester) async {
      final service = _Service();
      await _open(tester, service, locale: locale);
      expect(find.byKey(const ValueKey('automation-service-identity')), findsOneWidget);
      expect(find.byKey(const ValueKey('automation-detect-runner')), findsNothing);
      final save = tester.widget<GlassButton>(find.byKey(const ValueKey('automation-runner-save')));
      expect(save.onPressed, isNotNull);
      await tapKey(tester, 'automation-runner-save');
      expect(service.disconnects, 1);
      expect(service.updates, [{'kind': 'service_remote', 'host_id': 'saved-host', 'identity_id': _identity}]);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
    });
  }
  testWidgets('invalid or nil identity cannot save or invoke Native', (tester) async {
    final service = _Service();
    await _open(tester, service);
    for (final value in ['', 'not-a-uuid', '00000000-0000-0000-0000-000000000000', _identity.toUpperCase()]) {
      await tester.enterText(find.byKey(const ValueKey('automation-service-identity')), value);
      await tester.pump();
      expect(tester.widget<GlassButton>(find.byKey(const ValueKey('automation-runner-save'))).onPressed, isNull);
    }
    expect(service.updates, isEmpty); expect(service.disconnects, 0);
    await tester.pumpWidget(const SizedBox.shrink());
  });
  testWidgets('old implementation cannot save service metadata', (tester) async {
    final service = _Legacy();
    await _open(tester, service);
    expect(tester.widget<GlassButton>(find.byKey(const ValueKey('automation-runner-save'))).onPressed, isNull);
    expect(service.updates, isEmpty); expect(service.disconnects, 0);
    await tester.pumpWidget(const SizedBox.shrink());
  });
}
