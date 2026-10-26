// Protocol 1.11 quotas and request limits in the UI: a long server-requested
// pause is "resuming at HH:MM", not a sync error; rejected changes over the
// vault object limit say so and offer Retry; quota and rate-limit errors
// carry their limits and times.
import 'dart:async';

import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/app_services.dart';
import 'package:consolecrypt/core/util/formatting.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:intl/date_symbol_data_local.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

import '../helpers/test_app.dart';

/// Sync service with a fixed status; counts "Retry" presses.
final class _FakeSync implements SyncService {
  _FakeSync(this._status);

  SyncStatus _status;
  final _changes = StreamController<SyncStatus>.broadcast();
  int retried = 0;

  @override
  SyncStatus get currentStatus => _status;

  @override
  Stream<SyncStatus> watchStatus() async* {
    yield _status;
    yield* _changes.stream;
  }

  @override
  Future<int> retryFailed() async {
    retried++;
    final n = _status.failedChanges;
    _status = SyncStatus(state: SyncState.syncing, pendingChanges: n, lastSyncAt: _status.lastSyncAt);
    _changes.add(_status);
    return n;
  }

  @override
  Future<void> syncNow() async {}

  @override
  Future<void> disconnect({required bool revokeThisDevice}) async {}

  @override
  Stream<EnableSyncProgress> enableSync({
    required Uri serverUrl,
    required String email,
    required SecretText password,
    required String deviceName,
    required bool createAccount,
  }) => const Stream.empty();

  @override
  Stream<EnableSyncProgress> enableSyncEnterprise({
    required Uri serverUrl,
    required EnterpriseSignIn signIn,
    required String deviceName,
  }) => const Stream.empty();
}

/// Also reports a vault deletion scheduled by another device (1.13).
final class _DeletionSync extends _FakeSync implements VaultDeletionControl {
  _DeletionSync(super.status, this.at);

  DateTime? at;
  int cancelled = 0;

  @override
  Future<DateTime?> scheduledVaultDeletion() async => at;

  @override
  Future<void> cancelVaultDeletion() async {
    cancelled++;
    at = null;
  }
}

Future<_FakeSync> _pumpSync(WidgetTester tester, SyncStatus status, {AppLocale locale = AppLocale.en}) async {
  final backend = testBackend();
  addTearDown(backend.dispose);
  await backend.debugSignInDemoAndUnlock();
  final sync = _FakeSync(status);
  await pumpApp(tester, backend, locale: locale, overrides: [syncServiceProvider.overrideWithValue(sync)]);
  await tapKey(tester, 'nav-sync');
  return sync;
}

void main() {
  final lastSync = DateTime.now().toUtc().subtract(const Duration(minutes: 3));

  testWidgets('a long rate-limit pause says when sync resumes instead of an error', (tester) async {
    final resume = DateTime.now().add(const Duration(minutes: 20));
    await _pumpSync(
      tester,
      SyncStatus(state: SyncState.throttled, pendingChanges: 2, lastSyncAt: lastSync, nextRetryAt: resume),
    );
    final l = lookupAppLocalizations(const Locale('en'));
    final label = l.syncStateThrottled(formatClock(l, resume));
    expect(label, contains(DateFormat.Hm('en').format(resume)));
    expect(find.text(label), findsWidgets);
    expect(find.byKey(const ValueKey('sync-throttled')), findsOneWidget);
    expect(find.text(l.syncScreenThrottledBody), findsOneWidget);
    expect(find.text(l.syncStateError), findsNothing);
    expect(find.text(l.syncScreenIssueRetryable), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('the Russian pause notice', (tester) async {
    final resume = DateTime.now().add(const Duration(hours: 1));
    await _pumpSync(
      tester,
      SyncStatus(state: SyncState.throttled, lastSyncAt: lastSync, nextRetryAt: resume),
      locale: AppLocale.ru,
    );
    final ru = lookupAppLocalizations(const Locale('ru'));
    final when = formatClock(ru, resume);
    expect(when, contains(DateFormat.Hm('ru').format(resume)));
    expect(find.text('Сервер временно ограничил синхронизацию, продолжим в $when'), findsWidgets);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('changes over the vault object limit are explained and can be retried', (tester) async {
    final sync = await _pumpSync(
      tester,
      SyncStatus(
        state: SyncState.idle,
        failedChanges: 3,
        lastSyncAt: lastSync,
        pushQuota: const SyncQuota(reason: SyncQuota.objectLimitReason, limit: 5000),
      ),
    );
    final l = lookupAppLocalizations(const Locale('en'));
    expect(find.byKey(const ValueKey('sync-rejected')), findsOneWidget);
    final message = tester.widget<Text>(find.byKey(const ValueKey('sync-rejected-message'))).data!;
    expect(message, l.errorObjectLimit('5,000'));
    expect(message, contains('too many objects (limit 5,000)'));
    await tapKey(tester, 'sync-retry-failed');
    expect(sync.retried, 1);
    expect(find.byKey(const ValueKey('sync-rejected')), findsNothing);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  testWidgets('other rejected changes get a generic explanation', (tester) async {
    await _pumpSync(tester, SyncStatus(state: SyncState.idle, failedChanges: 2, lastSyncAt: lastSync));
    final l = lookupAppLocalizations(const Locale('en'));
    expect(find.text(l.syncScreenRejectedBody(2)), findsOneWidget);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));

  test('quota and rate-limit errors name their limits and times', () async {
    await initializeDateFormatting('en');
    await initializeDateFormatting('ru');
    final until = DateTime.utc(2026, 10, 24, 8, 30);
    for (final locale in const [Locale('en'), Locale('ru')]) {
      final l = lookupAppLocalizations(locale);
      String quota(AppErrorCode code, AppErrorReason reason, Map<String, String> args) =>
          errorMessage(l, AppException(code, 'quota', reason: reason, args: args));

      expect(
        quota(AppErrorCode.payloadTooLarge, AppErrorReason.objectLimit, {'limit': '5000'}),
        l.errorObjectLimit(formatCount(l, 5000)),
      );
      expect(quota(AppErrorCode.forbidden, AppErrorReason.deviceLimit, {'limit': '10'}), l.errorDeviceLimit('10'));
      final daily = quota(AppErrorCode.forbidden, AppErrorReason.deviceDailyLimit, {
        'limit': '3',
        'until_ms': until.millisecondsSinceEpoch.toString(),
      });
      expect(daily, l.errorDeviceDailyLimit(formatDateTime(l, until)));
      expect(daily, contains('2026'));
      // Without the numbers: the code-level text.
      expect(quota(AppErrorCode.forbidden, AppErrorReason.deviceLimit, const {}), l.errorForbidden);

      // Dialogs with their own failure text show the retry delay instead.
      const limited = AppException(
        AppErrorCode.rateLimited,
        'slow down',
        args: {'retry_after_seconds': '42', 'limit': 'recipient_lookup_account'},
      );
      expect(rateLimitedMessage(l, limited), l.errorRateLimitedRetry(42));
      expect(rateLimitedMessage(l, const AppException(AppErrorCode.rateLimited, 'x')), l.errorRateLimited);
      expect(rateLimitedMessage(l, const AppException(AppErrorCode.forbidden, 'x')), isNull);
      expect(rateLimitedMessage(l, StateError('x')), isNull);
    }
    final ru = lookupAppLocalizations(const Locale('ru'));
    expect(
      errorMessage(
        ru,
        const AppException(
          AppErrorCode.payloadTooLarge,
          'q',
          reason: AppErrorReason.objectLimit,
          args: {'limit': '200'},
        ),
      ),
      startsWith('В хранилище слишком много объектов (предел 200)'),
    );
  });

  testWidgets('a deletion scheduled by another device is shown and can be cancelled', (tester) async {
    final backend = testBackend();
    addTearDown(backend.dispose);
    await backend.debugSignInDemoAndUnlock();
    final at = DateTime.now().toUtc().add(const Duration(days: 7));
    final sync = _DeletionSync(SyncStatus(state: SyncState.idle, lastSyncAt: lastSync), at);
    await pumpApp(tester, backend, overrides: [syncServiceProvider.overrideWithValue(sync)]);
    await tapKey(tester, 'nav-sync');
    final l = lookupAppLocalizations(const Locale('en'));
    expect(find.byKey(const ValueKey('sync-pending-deletion')), findsOneWidget);
    expect(find.text(l.syncVaultDeletionTitle(formatDateTime(l, at))), findsOneWidget);
    await tapKey(tester, 'sync-cancel-deletion');
    expect(sync.cancelled, 1);
    expect(find.text(l.syncVaultDeletionCancelled), findsOneWidget);
    expect(find.byKey(const ValueKey('sync-pending-deletion')), findsNothing);
    expect(tester.takeException(), isNull);
  }, variant: TargetPlatformVariant.only(TargetPlatform.macOS));
}
