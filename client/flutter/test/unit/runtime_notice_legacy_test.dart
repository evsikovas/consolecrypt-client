import 'package:consolecrypt/app/critical_notice_owner.dart';
import 'package:consolecrypt/core/services/runtime_critical_notice_ring.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111';
const nonce = '22222222-2222-4222-8222-222222222222';
const next = '33333333-3333-4333-8333-333333333333';
const record = '44444444-4444-4444-8444-444444444444';
const legacyStates = <String, List<String>>{
  'security': ['integrity_warning', 'server_rollback', 'secret_conflict', 'conflict_copy'],
  'device': ['registered', 'approval_requested', 'revoked', 'recovery_changed', 'key_conflict'],
  'backup': ['failed', 'completed'],
  'team': ['invalid_log', 'log_rollback', 'removed'],
  'edit': ['error', 'conflict', 'recovery_available', 'synced'],
  'transfer': ['failed', 'completed'],
};
Map<String, dynamic> event(String domain, String state, {int revision = 1, String scope = nonce, String id = record}) =>
    {
      'type': 'critical_state_changed',
      'profile_id': profile,
      'scope_id': scope,
      'record_id': id,
      'revision': revision,
      'change': {'domain': domain, 'state': state},
    };
void establish(RuntimeNoticeEventAdapter events, {String scope = nonce}) => events.acceptScope(
  originalEpoch: events.epoch,
  nativeProfile: {'id': profile, 'active': true, 'vault_state': 'unlocked', 'notification_scope_id': scope},
);
Future<void> flush() async {
  for (var i = 0; i < 150; i++) {
    await Future<void>.value();
  }
}

void main() {
  late RuntimeNoticeEventAdapter events;
  late CriticalNoticeOwner owner;
  setUp(() {
    events = RuntimeNoticeEventAdapter();
    establish(events);
    owner = CriticalNoticeOwner(scope: Object(), profileId: profile, isCurrent: () => true, runtimeEvents: events);
  });
  tearDown(() async {
    await owner.dispose();
    await events.dispose();
  });
  test('exact existing producer domains accept fixed states only with no raw payload retention', () async {
    var revision = 0;
    for (final entry in legacyStates.entries) {
      for (final state in entry.value) {
        final value = event(entry.key, state, revision: ++revision);
        final parsed = RuntimeNoticeChange.parse(value);
        expect(parsed, isNotNull);
        expect(parsed!.state.wire, state);
        events.ingest(value);
        await flush();
        final row = owner.state.rows.firstWhere((r) => r.runtime!.value.domain.name == entry.key);
        expect(row.runtime!.value.state.wire, state);
        expect(row.target, isA<CriticalRuntimeTarget>());
      }
    }
    expect(owner.state.runtimeIncomplete, isFalse);
  });
  test('a valid shared state from another domain and extra private fields fail closed', () async {
    expect(RuntimeNoticeChange.parse(event('security', 'failed')), isNull);
    expect(RuntimeNoticeChange.parse(event('device', 'completed')), isNull);
    expect(RuntimeNoticeChange.parse({...event('backup', 'failed'), 'error': 'SYNTHETIC_PRIVATE'}), isNull);
    events.ingest({...event('backup', 'failed'), 'error': 'SYNTHETIC_PRIVATE'});
    await flush();
    expect(owner.state.rows, isEmpty);
    expect(owner.state.runtimeIncomplete, isTrue);
  });
  test('successful first backup/transfer and synced edits do not create unsolicited notices', () async {
    for (final (domain, state) in [('backup', 'completed'), ('transfer', 'completed'), ('edit', 'synced')]) {
      events.ingest(event(domain, state));
    }
    await flush();
    expect(owner.state.rows, isEmpty);
    events.ingest(event('backup', 'failed', revision: 2));
    await flush();
    final failed = owner.state.rows.single;
    expect(failed.runtime!.value.severity, RuntimeCriticalNoticeSeverity.error);
    events.ingest(event('backup', 'completed', revision: 3));
    await flush();
    expect(owner.state.rows.single.runtime!.value.state, RuntimeNoticeState.completed);
    expect(owner.state.rows.single.runtime!.value.severity, RuntimeCriticalNoticeSeverity.info);
  });
  test('old nonce and old revision cannot reopen/dismiss new owner security or transfer notices', () async {
    events.ingest(event('security', 'integrity_warning'));
    await flush();
    final old = owner.state.rows.single;
    events.boundary();
    establish(events, scope: next);
    events.ingest(event('transfer', 'failed', scope: next, revision: 2));
    await flush();
    events.ingest(event('security', 'secret_conflict', revision: 99));
    events.ingest(event('transfer', 'completed', scope: next, revision: 1));
    await flush();
    expect(owner.rowCurrent(old), isFalse);
    owner.dismissRow(old);
    owner.markReadRow(old);
    expect(owner.state.rows, hasLength(1));
    expect(owner.state.rows.single.runtime!.value.state, RuntimeNoticeState.failed);
  });
  test('global stream loss stops stale-owner ingestion; domain gaps remain incompleteness only', () async {
    events.ingest(event('team', 'invalid_log'));
    await flush();
    events.ingest({'type': 'critical_notice_gap', 'profile_id': profile, 'scope_id': nonce});
    await flush();
    expect(owner.state.rows, hasLength(1));
    expect(owner.state.runtimeIncomplete, isTrue);
    events.retireGlobalStream(closed: true);
    await flush();
    events.ingest(event('device', 'registered', revision: 9));
    await flush();
    expect(owner.state.rows, isEmpty);
    expect(owner.state.runtimeFeedUnavailable, isTrue);
  });
  test('new legacy domains share existing64-row bound and strict UUID metadata', () async {
    expect(RuntimeNoticeChange.parse(event('transfer', 'failed', id: 'caller-supplied-path/secret')), isNull);
    for (var i = 1; i <= 80; i++) {
      events.ingest(
        event('transfer', 'failed', revision: i, id: '77777777-7777-4777-8777-${i.toString().padLeft(12, '0')}'),
      );
    }
    await flush();
    expect(owner.state.rows, hasLength(64));
    expect(owner.state.runtimeIncomplete, isTrue);
  });
}
