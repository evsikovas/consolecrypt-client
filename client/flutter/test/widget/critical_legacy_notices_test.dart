import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/app/critical_notice_view.dart';
import 'package:consolecrypt/app/gate.dart';
import 'package:consolecrypt/app/navigation_customization.dart';
import 'package:consolecrypt/app/router.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/models/navigation.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/services/sftp_browser_service.dart';
import 'package:consolecrypt/core/services/sftp_service.dart';
import 'package:consolecrypt/core/services/terminal_service.dart';
import 'package:consolecrypt/sftp/sftp_controller.dart';
import 'package:consolecrypt/sftp/sftp_providers.dart';
import 'package:consolecrypt/sftp/widgets/sftp_activity.dart';
import 'package:consolecrypt/terminal/terminal_minimal_mode.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

const nonce = '10000000-0000-4000-8000-000000000001';
const record = '30000000-0000-4000-8000-000000000003';

class NoTerminal extends Fake implements TerminalService {
  final commands = <Symbol>[];
  @override
  dynamic noSuchMethod(Invocation invocation) {
    commands.add(invocation.memberName);
    throw StateError('No terminal execution');
  }
}

class NoSftp extends Fake implements SftpService {
  final commands = <Symbol>[];
  @override
  Future<String> localHome() async => '/owned/local';
  @override
  Future<List<FileEntry>> listLocal(String path) async => const [];
  @override
  Stream<List<TransferJob>> watchTransfers() => Stream.value(const []);
  @override
  dynamic noSuchMethod(Invocation invocation) {
    commands.add(invocation.memberName);
    throw StateError('No connection/transfer execution');
  }
}

class NoEdit extends Fake implements SftpBrowserService {
  final commands = <Symbol>[];
  @override
  Stream<List<EditSessionInfo>> watchEditSessions() => Stream.value(const []);
  @override
  Future<List<EditLeftover>> listEditLeftovers() async => const [];
  @override
  dynamic noSuchMethod(Invocation invocation) {
    commands.add(invocation.memberName);
    throw StateError('No edit execution');
  }
}

Future<(ProviderContainer, RuntimeNoticeEventAdapter, String, NoTerminal, NoSftp, NoEdit)> open(
  WidgetTester tester,
  String hiddenSection, {
  double width = 1400,
  bool ru = false,
}) async {
  final app = testBackend();
  addTearDown(app.dispose);
  await app.debugCreateUnlockedLocalProfile(name: 'Owned critical legacy fixture');
  final profile = app.services.profiles.currentProfiles.active!.id.value;
  final current = app.services.settings.currentLocal;
  await app.services.settings.updateLocal(
    current.copyWith(
      navigationByProfile: {
        ...current.navigationByProfile,
        profile: NavigationPreferences(visibility: {hiddenSection: false}),
      },
    ),
  );
  final events = RuntimeNoticeEventAdapter();
  addTearDown(events.dispose);
  events.acceptScope(
    originalEpoch: events.epoch,
    nativeProfile: {'id': profile, 'active': true, 'vault_state': 'unlocked', 'notification_scope_id': nonce},
  );
  final terminal = NoTerminal(), sftp = NoSftp(), edit = NoEdit();
  await pumpApp(
    tester,
    app,
    size: Size(width, 1000),
    locale: ru ? AppLocale.ru : AppLocale.en,
    overrides: [
      criticalRuntimeEventsProvider.overrideWithValue(events),
      terminalServiceProvider.overrideWithValue(terminal),
      sftpServiceProvider.overrideWithValue(sftp),
      sftpBrowserServiceProvider.overrideWithValue(edit),
    ],
  );
  final container = ProviderScope.containerOf(tester.element(find.byType(Navigator).first));
  container.read(routerProvider).go(AppRoutes.terminal);
  await settle(tester);
  return (container, events, profile, terminal, sftp, edit);
}

Map<String, dynamic> event(String profile, String domain, String state, {String scope = nonce, int revision = 1}) => {
  'type': 'critical_state_changed',
  'profile_id': profile,
  'scope_id': scope,
  'record_id': record,
  'revision': revision,
  'change': {'domain': domain, 'state': state},
};
Future<void> tap(WidgetTester tester, String key) async {
  final finder = find.byKey(ValueKey(key));
  await tester.ensureVisible(finder);
  await tester.tap(finder);
  await settle(tester);
}

void main() {
  for (final (domain, state, section, route) in [
    ('security', 'integrity_warning', 'sync', AppRoutes.sync),
    ('device', 'approval_requested', 'devices', AppRoutes.devices),
    ('backup', 'failed', 'backups', AppRoutes.backups),
    ('team', 'invalid_log', 'sharing', AppRoutes.sharing),
    ('edit', 'conflict', 'sftp', AppRoutes.sftp),
    ('transfer', 'failed', 'sftp', AppRoutes.sftp),
  ]) {
    testWidgets('hidden $domain critical notice opens existing read surface without repin/execution', (tester) async {
      final (container, events, profile, terminal, sftp, edit) = await open(tester, section);
      events.ingest(event(profile, domain, state));
      await settle(tester);
      final owner = container.read(criticalNoticeOwnerProvider)!;
      expect(owner.state.rows.single.runtime!.value.domain.name, domain);
      final l = tester.element(find.byType(Navigator).first).l10n;
      if (domain == 'backup') {
        expect(criticalNoticeTitle(l, owner.state.rows.single), contains(l.noticesLegacyBackupFailed));
      } else if (domain == 'transfer') {
        expect(criticalNoticeTitle(l, owner.state.rows.single), contains(l.noticesLegacyTransferFailed));
      } else if (domain == 'edit') {
        expect(criticalNoticeTitle(l, owner.state.rows.single), contains(l.noticesLegacyEditConflict));
        events.ingest(event(profile, domain, 'error', revision: 2));
        await settle(tester);
        expect(criticalNoticeTitle(l, owner.state.rows.single), contains(l.noticesLegacyEditError));
      }

      await tap(tester, 'critical-notices-open');
      await tap(tester, 'critical-notice-open-runtime:$domain:$record');
      expect(container.read(routerProvider).routeInformationProvider.value.uri.path, route);
      expect(container.read(navigationPreferencesProvider)!.visibility[section], isFalse);
      expect(terminal.commands, isEmpty);
      expect(sftp.commands, isEmpty);
      expect(edit.commands, isEmpty);
      if (domain == 'edit' || domain == 'transfer') {
        expect(container.read(sftpControllerProvider).session, isNull);
        expect(find.byKey(const ValueKey('sftp-activity')), findsOneWidget);
        expect(
          container.read(sftpActivityProvider).tab,
          domain == 'edit' ? SftpActivityTab.editing : SftpActivityTab.transfers,
        );
      }
      expect(tester.takeException(), isNull);
    });
  }
  testWidgets('minimal mode exposes hidden security warning while legacy banners remain offstage', (tester) async {
    tester.platformDispatcher.textScaleFactorTestValue = 1.6;
    addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
    final (container, events, profile, terminal, sftp, edit) = await open(tester, 'sync', width: 420, ru: true);
    container.read(terminalMinimalModeProvider.notifier).toggle();
    events.ingest(event(profile, 'security', 'server_rollback'));
    await settle(tester);
    expect(find.byKey(const ValueKey('critical-notices-attention')), findsOneWidget);
    expect(container.read(criticalNoticeOwnerProvider)!.state.unread, 1);
    await tester.tap(
      find.descendant(of: find.byKey(const ValueKey('critical-notices-attention')), matching: find.byType(GlassButton)),
    );
    await settle(tester);
    expect(find.byType(CriticalNoticePanel), findsOneWidget);
    expect(find.byKey(const ValueKey('critical-notice-open-runtime:security:$record')).hitTestable(), findsOneWidget);
    expect(terminal.commands, isEmpty);
    expect(sftp.commands, isEmpty);
    expect(edit.commands, isEmpty);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.windows));
  testWidgets('old captured legacy notice cannot navigate or open SFTP after original nonce changed', (tester) async {
    final (container, events, profile, terminal, sftp, edit) = await open(tester, 'sftp');
    events.ingest(event(profile, 'transfer', 'failed'));
    await settle(tester);
    await tap(tester, 'critical-notices-open');
    final callback = tester
        .widget<GlassButton>(find.byKey(const ValueKey('critical-notice-open-runtime:transfer:$record')))
        .onPressed!;
    events.boundary();
    events.acceptScope(
      originalEpoch: events.epoch,
      nativeProfile: {
        'id': profile,
        'active': true,
        'vault_state': 'unlocked',
        'notification_scope_id': '20000000-0000-4000-8000-000000000002',
      },
    );
    events.ingest(event(profile, 'transfer', 'failed', scope: '20000000-0000-4000-8000-000000000002', revision: 2));
    await settle(tester);
    callback();
    await settle(tester);
    expect(container.read(routerProvider).routeInformationProvider.value.uri.path, AppRoutes.terminal);
    expect(container.read(sftpActivityProvider).open, isFalse);
    expect(find.byType(CriticalNoticePanel), findsOneWidget);
    expect(terminal.commands, isEmpty);
    expect(sftp.commands, isEmpty);
    expect(edit.commands, isEmpty);
  });
}
