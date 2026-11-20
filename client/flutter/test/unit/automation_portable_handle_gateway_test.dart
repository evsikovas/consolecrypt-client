import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/automation/automation_portable_controller.dart';
import 'package:consolecrypt/automation/automation_portable_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_portable_lifetime.dart';
import 'package:consolecrypt/automation/automation_portable_native_adapter.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_test/flutter_test.dart';

import 'automation_portable_controller_test.dart' as f;

class Handle implements PortableIssuedRequestHandle {
  int starts = 0, cancels = 0;
  bool failStart = false;
  final reply = Completer<String>(), retired = Completer<void>();
  @override
  Future<String> start() {
    starts++;
    if (failStart) throw StateError('SYNTHETIC_PRIVATE_ERROR');
    return reply.future;
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<void> joined() => retired.future;
}

class Transfer implements PortableIssuedPasswordTransfer {
  Transfer(this.bytes);
  final Uint8List bytes;
  int cancels = 0, releases = 0;
  Handle? taken;
  @override
  void cancel() {
    cancels++;
    taken?.cancel();
  }

  @override
  void release() {
    releases++;
  } // NOT cancel: Native normal Drop semantics
}

class Call {
  Call(this.kind, this.json, this.password);
  final String kind, json;
  final Transfer? password;
  final ack = Completer<PortableIssuedRequestHandle>();
}

class Port implements PortableOpaqueNativePort {
  int captures = 0;
  final calls = <Call>[], transfers = <Transfer>[];
  Completer<String>? capture;
  bool badTransfer = false;
  @override
  Future<String> captureScope() {
    captures++;
    return capture?.future ?? Future.value(jsonEncode(f.scope().toJson()));
  }

  @override
  Transfer transferPassword(Uint8List bytes) {
    if (badTransfer) throw StateError('SYNTHETIC_PRIVATE_ERROR');
    final value = Transfer(bytes);
    transfers.add(value);
    return value;
  }

  Future<PortableIssuedRequestHandle> add(String kind, String json, Transfer? secret) {
    final call = Call(kind, json, secret);
    calls.add(call);
    return call.ack.future;
  }

  @override
  Future<PortableIssuedRequestHandle> registerContext(String scope, String project, int revision) =>
      add('context', scope, null);
  @override
  Future<PortableIssuedRequestHandle> registerExport(String json, PortableIssuedPasswordTransfer secret, String path) =>
      add('export', json, secret as Transfer);
  @override
  Future<PortableIssuedRequestHandle> registerInspect(
    String scope,
    String path,
    PortableIssuedPasswordTransfer secret,
  ) => add('inspect', scope, secret as Transfer);
}

Future<void> flush() => Future<void>.delayed(Duration.zero);

class Observed {
  Observed(this.invocation) {
    registered = invocation.registered.then((_) => true, onError: (Object _) => false);
    result = invocation.result.then<Map<String, dynamic>?>((v) => v, onError: (Object _) => null);
    joined = invocation.joined.then((_) => true, onError: (Object _) => false);
  }
  final PortableNativeInvocation invocation;
  late final Future<bool> registered, joined;
  late final Future<Map<String, dynamic>?> result;
}

Future<(PortableHandleGateway, Port)> setup() async {
  final port = Port();
  final gateway = PortableHandleGateway(port: port);
  await gateway.captureScope();
  return (gateway, port);
}

Observed context(PortableHandleGateway g, [String id = 'local-only']) =>
    Observed(g.context(scope: f.scope(), operationId: id, projectId: f.project, projectRevision: 1));
Observed exportCall(PortableHandleGateway g, SecretText secret, [String id = 'local-only']) => Observed(
  g.export(
    scope: f.scope(),
    operationId: id,
    request: {'scope': f.scope().toJson()},
    password: secret,
    destination: 'own.ccproject',
  ),
);
Future<void> finish(Call c, Observed op, {bool start = false}) async {
  final handle = Handle();
  c.ack.complete(handle);
  await op.registered;
  if (start) {
    op.invocation.startBody();
    handle.reply.complete(jsonEncode(f.contextJson()));
    await op.result;
  } else {
    handle.retired.complete();
  }
  if (start) handle.retired.complete();
  await op.joined;
}

void main() {
  test('construction and missing optional port perform zero Native RPC', () async {
    final p = Port();
    PortableHandleGateway(port: p);
    expect(p.captures, 0);
    expect(p.calls, isEmpty);
    await expectLater(PortableHandleGateway().captureScope(), throwsA(isA<PortablePackageFailure>()));
  });
  test('single actual scope capture returns immutable independent data copies', () async {
    final p = Port()..capture = Completer<String>(), g = PortableHandleGateway(port: null);
    await expectLater(g.captureScope(), throwsA(isA<PortablePackageFailure>()));
    final real = PortableHandleGateway(port: p), a = real.captureScope(), b = real.captureScope();
    expect(p.captures, 1);
    p.capture!.complete(jsonEncode(f.scope().toJson()));
    (await a)['profileId'] = 'foreign';
    expect((await b)['profileId'], f.profile);
    expect((await real.captureScope())['profileId'], f.profile);
    expect(p.captures, 1);
  });
  test('scope failure stays failed rather than adopting a later latest unlock', () async {
    final p = Port()..capture = Completer<String>(), g = PortableHandleGateway(port: null);
    await expectLater(g.captureScope(), throwsA(isA<PortablePackageFailure>()));
    final real = PortableHandleGateway(port: p), first = real.captureScope();
    p.capture!.complete('{}');
    await expectLater(first, throwsA(isA<PortablePackageFailure>()));
    p.capture = null;
    await expectLater(real.captureScope(), throwsA(isA<PortablePackageFailure>()));
    expect(p.captures, 1);
  });
  test('foreign scope and unknown cancellation never begin a Native request', () async {
    final (g, p) = await setup();
    final other = PortableScope.fromJson({...f.scope().toJson(), 'notificationScopeId': f.profile});
    expect(
      () => g.context(scope: other, operationId: 'x', projectId: f.project, projectRevision: 1),
      throwsA(isA<PortablePackageFailure>()),
    );
    await g.cancel(scope: other, operationId: 'x');
    expect(p.calls, isEmpty);
  });
  test('registration only ACK does not start body or send local correlation ID', () async {
    final (g, p) = await setup();
    final op = context(g);
    final h = Handle();
    p.calls.single.ack.complete(h);
    expect(await op.registered, isTrue);
    expect(h.starts, 0);
    expect(p.calls.single.json, jsonEncode(f.scope().toJson()));
    expect(p.calls.single.json, isNot(contains('local-only')));
    h.retired.complete();
    expect(await op.joined, isTrue);
    expect(await op.result, isNull);
  });
  test('sync transfer is copied before adapter input wipe and wiped only at actual ACK', () async {
    final (g, p) = await setup();
    final input = SecretText('SYNTHETIC_PASSWORD'), op = exportCall(g, input);
    input.wipe();
    final bytes = p.transfers.single.bytes;
    expect(utf8.decode(bytes), 'SYNTHETIC_PASSWORD');
    expect(p.calls.single.json, isNot(contains('SYNTHETIC_PASSWORD')));
    final h = Handle();
    p.calls.single.ack.complete(h);
    await op.registered;
    expect(bytes.every((v) => v == 0), isTrue);
    expect(p.transfers.single.releases, 1);
    expect(p.transfers.single.cancels, 0);
    op.invocation.startBody();
    expect(h.starts, 1);
    h.reply.complete(jsonEncode({'version': 1, 'encryptedSha256': 'c' * 64, 'bytes': 1000}));
    await op.result;
    h.retired.complete();
    await op.joined;
  });
  test('cancel before ACK wipes transfer and late exact handle joins without start', () async {
    final (g, p) = await setup();
    final op = exportCall(g, SecretText('SYNTHETIC_PASSWORD'));
    var done = false;
    final cancel = g.cancel(scope: f.scope(), operationId: 'local-only').then((_) => done = true);
    expect(p.transfers.single.cancels, 1);
    expect(p.transfers.single.bytes.every((v) => v == 0), isTrue);
    await flush();
    expect(done, isFalse);
    final h = Handle();
    p.calls.single.ack.complete(h);
    await op.registered;
    op.invocation.startBody();
    expect(h.starts, 0);
    expect(h.cancels, 1);
    expect(await op.result, isNull);
    h.retired.complete();
    await cancel;
    expect(await op.joined, isTrue);
  });
  test('post take pre ACK cancellation targets transfer weak original Control and late handle', () async {
    final (g, p) = await setup();
    final op = exportCall(g, SecretText('SYNTHETIC_PASSWORD')), h = Handle();
    p.transfers.single.taken = h; // models actual Native transfer already consumed, ACK held
    final cancel = g.cancel(scope: f.scope(), operationId: 'local-only');
    expect(h.cancels, 1);
    p.calls.single.ack.complete(h);
    await op.registered;
    expect(h.cancels, 2);
    op.invocation.startBody();
    expect(h.starts, 0);
    h.retired.complete();
    await cancel;
    await op.joined;
  });
  test('successful ACK release never explicitly cancels handed off request', () async {
    final (g, p) = await setup();
    final op = exportCall(g, SecretText('SYNTHETIC_PASSWORD')), h = Handle();
    p.transfers.single.taken = h;
    p.calls.single.ack.complete(h);
    await op.registered;
    expect(p.transfers.single.releases, 1);
    expect(p.transfers.single.cancels, 0);
    expect(h.cancels, 0);
    op.invocation.startBody();
    op.invocation.startBody();
    expect(h.starts, 1);
    h.reply.complete(jsonEncode({'version': 1, 'encryptedSha256': 'c' * 64, 'bytes': 1000}));
    await op.result;
    h.retired.complete();
    await op.joined;
    expect(h.cancels, 0);
  });
  test('Native joined may precede start response delivery without losing success', () async {
    final (g, p) = await setup();
    final op = context(g), h = Handle();
    p.calls.single.ack.complete(h);
    await op.registered;
    op.invocation.startBody();
    h.retired.complete();
    expect(await op.joined, isTrue);
    h.reply.complete(jsonEncode(f.contextJson()));
    expect(await op.result, f.contextJson());
  });
  test('result arrival does not fake join and cancellation retains exact running handle', () async {
    final (g, p) = await setup();
    final op = context(g), h = Handle();
    p.calls.single.ack.complete(h);
    await op.registered;
    op.invocation.startBody();
    h.reply.complete(jsonEncode(f.contextJson()));
    await op.result;
    var joined = false;
    unawaited(
      op.joined.then<void>((_) {
        joined = true;
      }),
    );
    await flush();
    expect(joined, isFalse);
    final cancel = g.cancel(scope: f.scope(), operationId: 'local-only');
    expect(h.cancels, 1);
    h.retired.complete();
    await cancel;
    expect(await op.joined, isTrue);
  });
  test('failed registration clears owned bytes and exposes no private diagnostic', () async {
    final (g, p) = await setup();
    final op = exportCall(g, SecretText('SYNTHETIC_PASSWORD'));
    p.calls.single.ack.completeError(StateError('SYNTHETIC_PRIVATE_ERROR'));
    expect(await op.registered, isFalse);
    expect(await op.result, isNull);
    expect(await op.joined, isTrue);
    expect(p.transfers.single.bytes.every((v) => v == 0), isTrue);
    expect(p.transfers.single.cancels, 1);
  });
  test('sync transfer failure registers no body and retires safely', () async {
    final (g, p) = await setup();
    p.badTransfer = true;
    final op = exportCall(g, SecretText('SYNTHETIC_PASSWORD'));
    expect(await op.registered, isFalse);
    expect(await op.result, isNull);
    expect(await op.joined, isTrue);
    expect(p.calls, isEmpty);
  });
  test('password byte bounds reject before transfer or request registration', () async {
    final (g, p) = await setup();
    for (final bytes in [0, 4097]) {
      final op = exportCall(g, SecretText('x' * bytes), 'id-$bytes');
      expect(await op.registered, isFalse);
      expect(await op.result, isNull);
      expect(await op.joined, isTrue);
    }
    expect(p.transfers, isEmpty);
    expect(p.calls, isEmpty);
  });
  test('duplicate and quota are bounded local correlation not Native authorization', () async {
    final (g, p) = await setup();
    final ops = <Observed>[];
    for (var i = 0; i < 16; i++) {
      ops.add(context(g, 'local-$i'));
    }
    expect(() => context(g, 'local-0'), throwsA(isA<PortablePackageFailure>()));
    expect(() => context(g, 'over-quota'), throwsA(isA<PortablePackageFailure>()));
    expect(p.calls.length, 16);
    for (var i = 0; i < 16; i++) {
      await finish(p.calls[i], ops[i]);
    }
    final next = context(g, 'after-joined');
    await finish(p.calls.last, next);
  });
  test('join transport failure is not reported as actual Native join', () async {
    final (g, p) = await setup();
    final op = context(g), h = Handle();
    p.calls.single.ack.complete(h);
    await op.registered;
    h.retired.completeError(StateError('SYNTHETIC_PRIVATE_ERROR'));
    expect(await op.joined, isFalse);
    expect(await op.result, isNull);
    expect(h.cancels, 1);
    // Failed join keeps the original owned handle available for exact cancel.
    await expectLater(g.cancel(scope: f.scope(), operationId: 'local-only'), throwsA(isA<PortablePackageFailure>()));
    expect(h.cancels, 2);
  });
  test('malformed or oversized Native response remains failure while real join is awaited', () async {
    final (g, p) = await setup();
    final oversized = 'x' * 524288;
    for (final reply in ['[]', '{$oversized']) {
      final op = context(g, 'r-${reply.length}'), h = Handle();
      p.calls.last.ack.complete(h);
      await op.registered;
      op.invocation.startBody();
      h.reply.complete(reply);
      expect(await op.result, isNull);
      h.retired.complete();
      expect(await op.joined, isTrue);
    }
  });
  test('actual adapter rejects late initial scope after original hub epoch retires', () async {
    final p = Port()..capture = Completer<String>(), events = RuntimeNoticeEventAdapter();
    final life = PortableUiLifetime(profileId: f.profile, events: events, isOriginalUiCurrent: () => true);
    addTearDown(events.dispose);
    addTearDown(life.dispose);
    final pending = AutomationPortableNativeAdapter.capture(
      lifetime: life,
      gateway: PortableHandleGateway(port: p),
    );
    events.boundary();
    expect(life.current, isFalse);
    p.capture!.complete(jsonEncode({...f.scope().toJson(), 'notificationScopeId': f.project}));
    await expectLater(pending, throwsA(isA<PortablePackageFailure>()));
    expect(p.calls, isEmpty);
  });
  for (final boundary in ['AppServices replacement', 'same service replacement', 'route hide', 'hub lock/reopen']) {
    test('composed original controller $boundary cancels transfer before ACK and cannot revive', () async {
      final p = Port(), events = RuntimeNoticeEventAdapter(), files = f.Files();
      var original = true;
      final life = PortableUiLifetime(profileId: f.profile, events: events, isOriginalUiCurrent: () => original);
      addTearDown(events.dispose);
      final adapter = await AutomationPortableNativeAdapter.capture(
        lifetime: life,
        gateway: PortableHandleGateway(port: p),
      );
      final c = adapter.createController(files: files, projectId: f.project, projectRevision: 1);
      addTearDown(adapter.dispose);
      addTearDown(c.dispose);
      final load = c.load(), contextHandle = Handle();
      p.calls.single.ack.complete(contextHandle);
      await flush();
      expect(contextHandle.starts, 1);
      contextHandle.reply.complete(jsonEncode(f.contextJson()));
      contextHandle.retired.complete();
      await load;
      expect(c.phase, PortablePhase.selecting);
      c.select('play.yml', true);
      final password = SecretText('SYNTHETIC_PASSWORD'), confirmation = SecretText('SYNTHETIC_PASSWORD');
      final work = c.export(password, confirmation);
      await flush();
      expect(p.calls.last.kind, 'export');
      if (boundary == 'hub lock/reopen') {
        events.boundary();
        await flush();
      } else {
        original = false;
        life.retire();
      }
      expect(c.phase, PortablePhase.cancelled);
      expect(p.transfers.single.cancels, 1);
      expect(password.isWiped, isTrue);
      expect(confirmation.isWiped, isTrue);
      // Even if the old predicate becomes true, the original lifetime stays retired.
      original = true;
      final late = Handle();
      p.calls.last.ack.complete(late);
      await flush();
      expect(late.starts, 0);
      expect(late.cancels, greaterThanOrEqualTo(1));
      late.retired.complete();
      await work;
      expect(c.exported, isNull);
      expect(c.current, isFalse);
      expect(files.finishes, 0);
      expect(p.captures, 1);
    });
  }

  test('start transport failure cancels exact idle handle promptly but cannot fake join', () async {
    final (g, p) = await setup();
    final op = context(g), h = Handle()..failStart = true;
    p.calls.single.ack.complete(h);
    await op.registered;
    op.invocation.startBody();
    expect(await op.result, isNull);
    expect(h.starts, 1);
    expect(h.cancels, 1);
    var joined = false;
    unawaited(
      op.joined.then<void>((_) {
        joined = true;
      }),
    );
    await flush();
    expect(joined, isFalse);
    // Original request remains correlated while its worker/idle retirement is unproven.
    final cancel = g.cancel(scope: f.scope(), operationId: 'local-only');
    expect(h.cancels, 2);
    h.retired.complete();
    await cancel;
    expect(await op.joined, isTrue);
  });

  test('foreign original scope cannot cancel an existing other-scope handle', () async {
    final (g, p) = await setup();
    final op = context(g), h = Handle();
    p.calls.single.ack.complete(h);
    await op.registered;
    final other = PortableScope.fromJson({...f.scope().toJson(), 'notificationScopeId': f.profile});
    await g.cancel(scope: other, operationId: 'local-only');
    expect(h.cancels, 0);
    h.retired.complete();
    await op.joined;
  });
  test('premature start before ACK retires only original registration and never starts late body', () async {
    final (g, p) = await setup();
    final op = exportCall(g, SecretText('SYNTHETIC_PASSWORD'));
    op.invocation.startBody();
    expect(p.transfers.single.cancels, 1);
    final h = Handle();
    p.calls.single.ack.complete(h);
    await op.registered;
    op.invocation.startBody();
    expect(h.starts, 0);
    expect(h.cancels, greaterThanOrEqualTo(1));
    h.retired.complete();
    await op.joined;
    expect(await op.result, isNull);
  });
  test('inspect sends only original scope JSON and explicit source with separate secret transfer', () async {
    final (g, p) = await setup();
    final input = SecretText('SYNTHETIC_PASSWORD');
    final op = Observed(
      g.inspect(scope: f.scope(), operationId: 'local-inspect', source: 'own.ccproject', password: input),
    );
    expect(p.calls.single.kind, 'inspect');
    expect(p.calls.single.json, jsonEncode(f.scope().toJson()));
    expect(p.calls.single.json, isNot(contains('SYNTHETIC_PASSWORD')));
    final h = Handle();
    p.calls.single.ack.complete(h);
    await op.registered;
    op.invocation.startBody();
    h.reply.complete(
      jsonEncode({
        'version': 1,
        'encryptedSha256': 'c' * 64,
        'publicationSupported': false,
        'reason': 'managed_publication_required',
        'summary': {'sourceDigest': 'a' * 64, 'portableDigest': 'd' * 64, 'files': 1, 'bytes': 24, 'exclusions': 1},
      }),
    );
    expect((await op.result)!['publicationSupported'], isFalse);
    h.retired.complete();
    await op.joined;
    expect(p.transfers.single.cancels, 0);
  });
}
