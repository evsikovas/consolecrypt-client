import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/critical_notice_view.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/job_notice_events.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/terminal/terminal_minimal_mode.dart';
import 'package:consolecrypt/terminal/terminal_tabs_controller.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/automation_ui_proof.dart';
import '../helpers/test_app.dart';

const _nonceA = '10000000-0000-4000-8000-000000000001';
const _nonceB = '20000000-0000-4000-8000-000000000002';
const _record = '30000000-0000-4000-8000-000000000003';

class _NoTerminalCommands implements TerminalService {
  final calls = <Symbol>[];
  @override
  dynamic noSuchMethod(Invocation invocation) {
    calls.add(invocation.memberName);
    throw StateError('Unexpected terminal operation');
  }
}

class _Harness {
  _Harness(this.container, this.events, this.terminal, this.profile);
  final ProviderContainer container;
  final RuntimeNoticeEventAdapter events;
  final _NoTerminalCommands terminal;
  final String profile;
  CriticalNoticeOwner get owner => container.read(criticalNoticeOwnerProvider)!;
  String id(String domain) => 'runtime:$domain:${domain == 'sync' ? profile : _record}';
  void establish(String nonce) => events.acceptScope(
    originalEpoch: events.epoch,
    nativeProfile: {'id': profile, 'active': true, 'vault_state': 'unlocked', 'notification_scope_id': nonce},
  );
  Future<void> emit(
    WidgetTester tester, {
    String domain = 'terminal',
    String state = 'closed',
    int revision = 1,
    String nonce = _nonceA,
  }) async {
    events.ingest({
      'type': 'critical_state_changed',
      'profile_id': profile,
      'scope_id': nonce,
      'record_id': domain == 'sync' ? profile : _record,
      'revision': revision,
      'change': {'domain': domain, 'state': state},
    });
    await settle(tester);
  }
}

Future<_Harness> _open(WidgetTester tester, {double width = 1500, bool ru = false}) async {
  final app = testBackend();
  addTearDown(app.dispose);
  await app.debugCreateUnlockedLocalProfile(name: 'Owned runtime notice fixture');
  final profile = app.services.profiles.currentProfiles.active!.id.value;
  final events = RuntimeNoticeEventAdapter();
  final jobs = JobNoticeEventAdapter();
  final terminal = _NoTerminalCommands();
  jobs.acceptScope(originalEpoch: jobs.epoch, profileId: profile, unlocked: true);
  events.acceptScope(
    originalEpoch: events.epoch,
    nativeProfile: {'id': profile, 'active': true, 'vault_state': 'unlocked', 'notification_scope_id': _nonceA},
  );
  addTearDown(events.dispose);
  addTearDown(jobs.dispose);
  await pumpApp(
    tester,
    app,
    size: Size(width, 1100),
    locale: ru ? AppLocale.ru : AppLocale.en,
    captureBoundary: true,
    overrides: [
      criticalRuntimeEventsProvider.overrideWithValue(events),
      criticalJobEventsProvider.overrideWithValue(jobs),
      terminalServiceProvider.overrideWithValue(terminal),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.terminal);
  await settle(tester);
  expect(container.read(appStageProvider), AppStage.unlocked);
  return _Harness(container, events, terminal, profile);
}

Future<void> _tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.tap(finder);
  await settle(tester);
}

void main() {
  testWidgets('closed runtime feed with no unread rows remains visible in minimal mode', (tester) async {
    final h = await _open(tester);
    h.events.streamLost(closed: true);
    await settle(tester);
    expect(h.owner.state.rows, isEmpty);
    final bell = tester.widget<GlassIconButton>(find.byKey(const ValueKey('critical-notices-open')));
    expect(bell.tooltip, contains('connection event stream is closed'));
    h.container.read(terminalMinimalModeProvider.notifier).toggle();
    await settle(tester);
    expect(find.byKey(const ValueKey('critical-notices-attention')), findsOneWidget);
    final l = tester.element(find.byKey(const ValueKey('critical-notices-attention'))).l10n;
    expect(find.text(l.noticesRuntimeUnavailable), findsOneWidget);
    await tester.tap(find.widgetWithText(GlassButton, l.noticesTitle));
    await settle(tester);
    expect(find.byKey(const ValueKey('critical-notices-runtime-gap')), findsOneWidget);
    expect(h.terminal.calls, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: const TargetPlatformVariant({TargetPlatform.windows}));

  for (final (domain, state, route) in [
    ('terminal', 'closed', AppRoutes.terminal),
    ('tunnel', 'failed', AppRoutes.tunnels),
    ('sync', 'offline', AppRoutes.sync),
  ]) {
    testWidgets('runtime $domain row opens only section with no reconnect or deferred intent', (tester) async {
      final h = await _open(tester);
      await h.emit(tester, domain: domain, state: state);
      expect(h.owner.state.rows.single.runtime, isNotNull);
      await _tap(tester, 'critical-notices-open');
      final l = tester.element(find.byType(CriticalNoticePanel)).l10n;
      final label = criticalNoticeTitle(l, h.owner.state.rows.single);
      expect(find.text(label), findsOneWidget);
      if (domain == 'terminal') {
        expect(label, contains('Session closed'));
        final open = find.byKey(ValueKey('critical-notice-open-${h.id(domain)}'));
        final dismiss = find.byKey(ValueKey('critical-notice-dismiss-${h.id(domain)}'));
        expect(tester.getSize(dismiss).height, tester.getSize(open).height);
        expect(tester.getCenter(dismiss).dy, tester.getCenter(open).dy);
        expect(tester.widget<GlassIconButton>(dismiss).size, tester.widget<GlassButton>(open).size.height);
      }
      await _tap(tester, 'critical-notice-open-${h.id(domain)}');
      expect(h.container.read(routerProvider).routeInformationProvider.value.uri.path, route);
      expect(h.container.read(criticalNoticeOpenIntentProvider), isNull);
      expect(h.container.read(terminalTabsProvider).tabs, isEmpty);
      expect(h.terminal.calls, isEmpty);
      expect(tester.takeException(), isNull);
    });
  }
  for (final replaceNonce in [false, true]) {
    testWidgets('real captured callbacks cannot touch ${replaceNonce ? 'new Native nonce' : 'new revision'} row', (
      tester,
    ) async {
      final h = await _open(tester);
      await h.emit(tester, domain: 'sync', state: 'offline');
      await _tap(tester, 'critical-notices-open');
      final id = h.id('sync');
      final read = tester.widget<GlassButton>(find.byKey(ValueKey('critical-notice-read-$id'))).onPressed!;
      final dismiss = tester.widget<GlassIconButton>(find.byKey(ValueKey('critical-notice-dismiss-$id'))).onPressed!;
      final open = tester.widget<GlassButton>(find.byKey(ValueKey('critical-notice-open-$id'))).onPressed!;
      if (replaceNonce) {
        h.events.boundary();
        h.establish(_nonceB);
      }
      await h.emit(tester, domain: 'sync', state: 'error', revision: 2, nonce: replaceNonce ? _nonceB : _nonceA);
      read();
      dismiss();
      open();
      await settle(tester);
      expect(find.byType(CriticalNoticePanel), findsOneWidget);
      expect(h.owner.state.rows.single.runtime!.value.revision, 2);
      expect(h.owner.state.rows.single.unread, isTrue);
      expect(h.container.read(routerProvider).routeInformationProvider.value.uri.path, AppRoutes.terminal);
      expect(h.container.read(criticalNoticeOpenIntentProvider), isNull);
      expect(h.terminal.calls, isEmpty);
      expect(tester.takeException(), isNull);
    });
  }
  for (final width in [360.0, 1500.0]) {
    testWidgets('Russian runtime notice at width $width uses literal localized state', (tester) async {
      await loadAutomationProofFonts(tester);
      tester.platformDispatcher.textScaleFactorTestValue = 1.6;
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      final h = await _open(tester, width: width, ru: true);
      await h.emit(tester);
      await _tap(tester, 'critical-notices-open');
      final l = tester.element(find.byType(CriticalNoticePanel)).l10n;
      expect(find.text(criticalNoticeTitle(l, h.owner.state.rows.single)), findsOneWidget);
      expect(find.textContaining('Session closed'), findsNothing);
      expect(find.byKey(ValueKey('critical-notice-open-${h.id('terminal')}')), findsOneWidget);
      await captureAutomationWorkspace(tester, 'critical-runtime-${width < 800 ? 'narrow' : 'wide'}-ru');
      expect(h.terminal.calls, isEmpty);
      expect(tester.takeException(), isNull);
    }, variant: const TargetPlatformVariant({TargetPlatform.windows}));
  }
}
