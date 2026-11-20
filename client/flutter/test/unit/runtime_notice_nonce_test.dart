import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:flutter_test/flutter_test.dart';

const profile = '11111111-1111-4111-8111-111111111111';
const record = '22222222-2222-4222-8222-222222222222';
Map<String, dynamic> owner(Object? nonce, {String id = profile}) => {
  'id': id, 'active': true, 'vault_state': 'unlocked', 'notification_scope_id': nonce,
};
Map<String, dynamic> event(Object? nonce, {String id = profile}) => {
  'type': 'critical_state_changed', 'profile_id': id, 'scope_id': nonce,
  'record_id': record, 'revision': 1,
  'change': {'domain': 'terminal', 'state': 'failed'},
};

void main() {
  test('opaque original nonce is preserved byte-exact while object IDs remain UUIDs', () {
    for (final nonce in ['opaque-origin:é', 'é' * 128, 'x' * 256, ' ', '00000000-0000-0000-0000-000000000000']) {
      expect(RuntimeNoticeScope.fromProfile(owner(nonce))?.nonce, nonce);
      expect(RuntimeNoticeChange.parse(event(nonce))?.scopeId, nonce);
    }
    expect(RuntimeNoticeScope.fromProfile(owner('opaque', id: 'opaque-profile')), isNull);
    expect(RuntimeNoticeChange.parse(event('opaque', id: 'opaque-profile')), isNull);
    final badRecord = event('opaque')..['record_id'] = 'opaque-record';
    expect(RuntimeNoticeChange.parse(badRecord), isNull);
  });
  test('nonces reject controls malformed unicode and byte overflow on every input', () {
    for (final nonce in [null, 7, '', 'é' * 129, 'x' * 257, 'a\n', '\u0000', '\u007f', '\u0085', String.fromCharCode(0xd800)]) {
      expect(RuntimeNoticeScope.fromProfile(owner(nonce)), isNull);
      expect(RuntimeNoticeChange.parse(event(nonce)), isNull);
    }
  });
  test('startup replay requires matching captured opaque origin and original epoch', () {
    final projection = RuntimeNoticeProjection();
    final boundary = projection.boundary(unlockPending: true);
    expect(projection.consume(event('original:é')), isNull);
    expect(projection.consume(event('foreign:é')), isNull);
    expect(projection.establish(boundary.epoch - 1, owner('original:é')), isNull);
    expect(projection.currentScope.scope, isNull);
    projection.establish(boundary.epoch, owner('original:é'));
    expect(projection.currentScope.scope?.nonce, 'original:é');
    final replay = projection.takeUnlockReplay(projection.currentScope, profile);
    expect(replay, hasLength(1));
    expect(replay.single.change?.scopeId, 'original:é');
    expect(projection.takeUnlockReplay(projection.currentScope, profile), isEmpty);
    expect(projection.consume(event('foreign:é')), isNull);
    expect(projection.consume(event('original:é'))?.change?.recordId, record);
    projection.boundary();
    expect(projection.currentScope.scope, isNull);
    expect(projection.consume(event('original:é')), isNull);
    projection.dispose();
  });
}
