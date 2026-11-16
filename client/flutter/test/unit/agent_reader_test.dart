import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/monitoring/agent_reader_controller.dart';
import 'package:consolecrypt/monitoring/agent_reader_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_reader_fixture.dart' as f;

class PendingTransfer extends f.Handle {
  PendingTransfer(super.selection,super.action);
  final transferResult=Completer<String>();int transfers=0;
  @override Future<String> transferExport({required String publicDestination,String? invitationDestination}){transfers++;return transferResult.future;}
}
void main() {
  test('request contains only saved refs and original opaque nonce; byte/control bounds', () {
    final s = f.selection(nonce: 'opaque original ${String.fromCharCode(0x0430) * 110}');
    final request = readerDecode(AgentReaderRequest.issuer().encode(s));
    expect(request.keys.toSet(), {'notificationScopeId', 'selection', 'action'});
    expect(request['notificationScopeId'], s.notificationScopeId);
    expect(() => f.selection(nonce: String.fromCharCode(0x0430) * 129), throwsFormatException);
    expect(() => f.selection(nonce: 'old\nnew'), throwsFormatException);
    expect(jsonEncode(request), isNot(contains('127.0.0.1')));
    expect(jsonEncode(request), isNot(contains('credential')));
  });
  test('all seven typed reviews require original nonce and matching IDs/revisions', () {
    final s = f.selection();
    for (final action in AgentReaderAction.values) {
      expect(AgentReaderReview.parse(jsonEncode(f.review(s, action)), s, action).kind, action);
    }
    for (final field in ['notificationScopeId', 'sourceRevision', 'bindingRevision', 'nativeHostId']) {
      final m = f.review(s, AgentReaderAction.issuerInit);
      ((m['details'] as Json)['native'] as Json)[field] = field.endsWith('Revision') ? 99 : 'foreign';
      expect(() => AgentReaderReview.parse(jsonEncode(m), s, AgentReaderAction.issuerInit), throwsFormatException);
    }
  });
  test('max u64 checkpoint is preserved exactly and string escapes stay untouched', () {
    const raw = r'{"epoch":18446744073709551615,"quoted":"a\\\"18446744073709551615","small":7}';
    final m = readerDecode(raw);
    expect(m['epoch'], '18446744073709551615');
    expect(m['small'], 7);
    expect(m['quoted'], (jsonDecode(raw) as Json)['quoted']);
    final s = f.selection(), mReview = f.review(f.selection(), AgentReaderAction.nodeOffer);
    final serialized = jsonEncode(mReview).replaceFirst('"ownerEpoch":1', '"ownerEpoch":18446744073709551615');
    expect(
      AgentReaderReview.parse(
        serialized,
        s,
        AgentReaderAction.nodeOffer,
      ).facts.any((v) => v.value == '18446744073709551615'),
      isTrue,
    );
  });
  test('future schema, malformed TTL, raw secrets and fake observed Actor are refused', () {
    final s = f.selection();
    for (final edit in ['ttl', 'renew', 'secret', 'actor']) {
      final m = f.review(s, AgentReaderAction.nodeOffer);
      switch (edit) {
        case 'ttl':
          m['remainingMilliseconds'] = 60001;
        case 'renew':
          m['deadlineRenewable'] = true;
        case 'secret':
          ((m['details'] as Json)['review'] as Json)['privateSeed'] = 'DO_NOT_DISPLAY';
        case 'actor':
          (m['details'] as Json)['actorApply'] = 'observed';
      }
      expect(() => AgentReaderReview.parse(jsonEncode(m), s, AgentReaderAction.nodeOffer), throwsFormatException);
    }
  });
  test('status is metadata and bounded saved association choices, never an ACK', () {
    final s = f.selection(), m = f.statusJson(f.selection());
    final parsed = AgentReaderStatus.parse(jsonEncode(m), s);
    expect(parsed.endpointObserved, isFalse);
    expect(parsed.locator, isNull);
    m['remoteApplyAck'] = 'observedThisOperation';
    expect(() => AgentReaderStatus.parse(jsonEncode(m), s), throwsFormatException);
  });
  test('typed heartbeat/history keep null unknown, UTC, truncation and eviction', () {
    final out = AgentReaderOutcome.parse(jsonEncode(f.result(AgentReaderAction.read)), AgentReaderAction.read);
    final heartbeat = AgentReaderDataset.parse(f.dataset(f.heartbeat()), out);
    expect(heartbeat.uptimeSeconds, 42);
    expect(heartbeat.collectionState, 'unknown');
    expect(heartbeat.observedAt!.isUtc, isTrue);
    final history = AgentReaderDataset.parse(f.dataset(f.history(count: 70)), out);
    expect(history.points, hasLength(70));
    expect(history.points.first.values['agent_collector_running'], isNull);
    expect(history.truncated, isTrue);
    expect(history.evicted, 2);
    final invalid = f.history();
    ((invalid['points'] as List).first as Json)['agent_uptime_seconds'] = -1;
    expect(() => AgentReaderDataset.parse(f.dataset(invalid), out), throwsFormatException);
    final wrong = readerDecode(f.dataset(f.heartbeat()), maxBytes: 270336)..['targetHash'] = 'b' * 64;
    expect(() => AgentReaderDataset.parse(jsonEncode(wrong), out), throwsFormatException);
  });
  test('open and status never prepare or auto-confirm', () async {
    final port = f.Port(f.selection()),
        c = AgentReaderController(port: f.Port(f.selection()), selection: f.selection(), isOriginal: () => true);
    c.dispose();
    expect(port.requests, isEmpty);
    final owner = AgentReaderController(port: port, selection: f.selection(), isOriginal: () => true);
    await owner.readStatus();
    expect(port.requests, isEmpty);
    expect(port.statuses, hasLength(1));
    expect(owner.review, isNull);
    owner.dispose();
  });
  test('cancel before prepare ACK cancels late exact handle without confirm', () async {
    final port = f.Port(f.selection())..pending = Completer();
    final c = AgentReaderController(port: port, selection: f.selection(), isOriginal: () => true);
    final pending = c.prepare(AgentReaderRequest.issuer());
    c.cancel();
    final handle = port.handles.single;
    port.pending!.complete(handle);
    await pending;
    expect(handle.cancels, 1);
    expect(handle.confirms, 0);
    expect(c.review, isNull);
    c.dispose();
  });
  test('original scope retirement during prepare and confirmation denies late output', () async {
    var original = true;
    final port = f.Port(f.selection())..pending = Completer();
    final c = AgentReaderController(port: port, selection: f.selection(), isOriginal: () => original);
    final pending = c.prepare(AgentReaderRequest.issuer());
    original = false;
    port.pending!.complete(port.handles.single);
    await pending;
    expect(port.handles.single.cancels, 1);
    expect(c.review, isNull);
    c.dispose();
    original = true;
    final next = f.Port(f.selection()),
        o = AgentReaderController(port: next, selection: f.selection(), isOriginal: () => original);
    await o.prepare(AgentReaderRequest.issuer());
    next.handles.single.waitingConfirm = Completer();
    final confirm = o.confirm();
    original = false;
    next.handles.single.waitingConfirm!.complete(jsonEncode(f.result(AgentReaderAction.issuerInit)));
    await confirm;
    expect(o.outcome, isNull);
    expect(next.handles.single.cancels, 1);
    o.dispose();
  });
  test('explicit one-use confirm and read consume never retry unknown outcome', () async {
    final port = f.Port(f.selection())..loseConfirm = true,
        c = AgentReaderController(port: f.Port(f.selection()), selection: f.selection(), isOriginal: () => true);
    c.dispose();
    final o = AgentReaderController(port: port, selection: f.selection(), isOriginal: () => true);
    await o.prepare(AgentReaderRequest.issuer());
    await o.confirm();
    await o.confirm();
    await o.prepare(AgentReaderRequest.issuer());
    expect(port.handles.single.confirms, 1);
    expect(port.requests, hasLength(1));
    expect(o.unknown, isTrue);
    o.dispose();
    final read = f.Port(f.selection()),
        r = AgentReaderController(port: read, selection: f.selection(), isOriginal: () => true);
    await r.prepare(AgentReaderRequest.association(AgentReaderAction.read, AgentReaderLocator('a' * 64, 'b' * 64)));
    await r.confirm();
    expect(read.handles.single.reads, 0);
    await r.consumeRead();
    await r.consumeRead();
    expect(read.handles.single.reads, 1);
    expect(r.dataset?.kind, 'heartbeatV1');
    r.dispose();
  });
  test('export transfer uses original held handle, explicit destinations and no Actor ACK', () async {
    final port = f.Port(f.selection()),
        c = AgentReaderController(port: f.Port(f.selection()), selection: f.selection(), isOriginal: () => true);
    c.dispose();
    final o = AgentReaderController(port: port, selection: f.selection(), isOriginal: () => true);
    await o.prepare(
      AgentReaderRequest.file(AgentReaderAction.nodeOffer, {
        'ownerPinFile': 'C:/public/pin.json',
        'nodeHelloFile': 'C:/public/hello.json',
      }, expiresAt: 2000000000),
    );
    await o.confirm();
    expect(port.handles.single.cancels, 0);
    expect(o.outcome?.hasInvitation, isTrue);
    await o.transfer(publicDestination: 'C:/chosen/public.json', invitationDestination: 'C:/chosen/invitation.hex');
    expect(o.transferCompleted, isTrue);
    expect(port.handles.single.cancels, 0);
    o.dispose();
    expect(port.handles.single.cancels, 1);
  });
  test('expired original review cannot be renewed by status or repeated confirm',()async{
    final selected=f.selection(),port=f.Port(f.selection())..pending=Completer();
    final c=AgentReaderController(port:port,selection:selected,isOriginal:()=>true);
    final preparing=c.prepare(AgentReaderRequest.issuer());final handle=port.handles.single;
    handle.customDetails=jsonEncode(f.review(selected,AgentReaderAction.issuerInit)..['remainingMilliseconds']=1);
    port.pending!.complete(handle);await preparing;await Future<void>.delayed(const Duration(milliseconds:10));
    await c.readStatus();await c.confirm();await c.confirm();expect(handle.confirms,0);expect(c.canConfirm,isFalse);expect(port.requests,hasLength(1));c.dispose();
  });
  test('pending transfer retains original handle and late original-scope loss cannot publish a success',()async{
    var original=true;final selected=f.selection(),port=f.Port(f.selection())..pending=Completer();
    final c=AgentReaderController(port:port,selection:selected,isOriginal:()=>original);
    final preparing=c.prepare(AgentReaderRequest.file(AgentReaderAction.nodeCert,{'ownerPinFile':'C:/public/pin.json','signedOfferFile':'C:/public/offer.json','signedClaimFile':'C:/public/claim.json'},expiresAt:2000000000));
    final held=PendingTransfer(selected,AgentReaderAction.nodeCert);port.pending!.complete(held);await preparing;await c.confirm();
    final copying=c.transfer(publicDestination:'C:/chosen/certificate.json');expect(held.transfers,1);expect(held.cancels,0);expect(c.busy,isTrue);
    original=false;c.retire();held.transferResult.complete(jsonEncode({'kind':'nodeExportTransferred','publicDestination':'C:/chosen/certificate.json','invitationDestination':null,'publicSha256':'a'*64,'publicBytes':200,'invitationBytes':null,'actorApply':'notObserved','copiesIrreversible':true}));
    await copying;expect(held.cancels,1);expect(c.transferCompleted,isFalse);expect(c.outcome,isNull);c.dispose();
  });
  test('uncertain transfer never retries or claims rollback of explicit destination files',()async{
    final selected=f.selection(),port=f.Port(f.selection())..pending=Completer();final c=AgentReaderController(port:port,selection:selected,isOriginal:()=>true);
    final preparing=c.prepare(AgentReaderRequest.file(AgentReaderAction.nodeCert,{'ownerPinFile':'C:/public/pin.json','signedOfferFile':'C:/public/offer.json','signedClaimFile':'C:/public/claim.json'},expiresAt:2000000000));
    final held=PendingTransfer(selected,AgentReaderAction.nodeCert);port.pending!.complete(held);await preparing;await c.confirm();
    final copying=c.transfer(publicDestination:'C:/chosen/certificate.json');held.transferResult.completeError(StateError('SYNTHETIC_PARTIAL_TRANSFER'));await copying;
    await c.transfer(publicDestination:'C:/chosen/certificate.json');expect(held.transfers,1);expect(c.unknown,isTrue);expect(c.transferAttempted,isTrue);expect(c.transferCompleted,isFalse);expect(held.cancels,0);c.dispose();
  });

}
