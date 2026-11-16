import 'dart:async';

import 'package:consolecrypt/automation/automation_portable_lifetime.dart';
import 'package:consolecrypt/automation/automation_portable_native_adapter.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:flutter_test/flutter_test.dart';

import 'automation_portable_controller_test.dart' as f;

class Call {
  Call(this.scope, this.id, this.kind);
  final PortableScope scope;
  final String id, kind;
  final result = Completer<Map<String, dynamic>>(), joined = Completer<void>();
  Future<void> registration = Future<void>.value();
  int bodyStarts = 0;
  SecretText? secret;
  PortableNativeInvocation get invocation => PortableNativeInvocation(
    registered: registration,
    startBody: () => bodyStarts++,
    result: result.future,
    joined: joined.future,
  );
}

class Gateway implements PortableNativeGateway {
  int captures = 0;
  PortablePackageFailure? captureError;
  Completer<Map<String, dynamic>>? scopeWait;
  final calls = <Call>[], cancels = <({PortableScope scope, String id})>[];
  void Function()? beforeReturn;
  Completer<void>? registrationWait;
  @override
  Future<Map<String, dynamic>> captureScope() async {
    captures++;
    if (captureError != null) throw captureError!;
    return scopeWait?.future ?? f.scope().toJson();
  }

  Call start(PortableScope scope, String id, String kind) {
    final call = Call(scope, id, kind);
    calls.add(call);
    if (registrationWait != null) call.registration = registrationWait!.future;
    beforeReturn?.call();
    return call;
  }

  @override
  PortableNativeInvocation context({
    required PortableScope scope,
    required String operationId,
    required String projectId,
    required int projectRevision,
  }) => start(scope, operationId, 'context').invocation;
  @override
  PortableNativeInvocation export({
    required PortableScope scope,
    required String operationId,
    required Map<String, dynamic> request,
    required SecretText password,
    required String destination,
  }) {
    expect(password.expose(), 'synthetic package password');
    final call = start(scope, operationId, 'export')..secret = password;
    return call.invocation;
  }

  @override
  PortableNativeInvocation inspect({
    required PortableScope scope,
    required String operationId,
    required String source,
    required SecretText password,
  }) {
    expect(password.expose(), 'synthetic package password');
    final call = start(scope, operationId, 'inspect')..secret = password;
    return call.invocation;
  }

  @override
  Future<void> cancel({required PortableScope scope, required String operationId}) async {
    cancels.add((scope: scope, id: operationId));
  }
}

PortableUiLifetime lifetime(RuntimeNoticeEventAdapter events, {bool Function()? current}) =>
    PortableUiLifetime(profileId: f.profile, events: events, isOriginalUiCurrent: current ?? () => true);
Future<void> flush() async {
  for (var i = 0; i < 20; i++) {
    await Future<void>.value();
  }
}

void main() {
  test('missing Native capability is typed unavailable, with no fallback scope producer', () async {
    final events = RuntimeNoticeEventAdapter(), life = lifetime(events);
    addTearDown(events.dispose);
    addTearDown(life.dispose);
    await expectLater(
      AutomationPortableNativeAdapter.capture(lifetime: life),
      throwsA(isA<PortablePackageFailure>().having((e) => e.code, 'code', 'portable_backend_unavailable')),
    );
  });
  test(
    'original UI epoch is captured before first async scope capture and cannot adopt same-profile new unlock',
    () async {
      final events = RuntimeNoticeEventAdapter(),
          life = lifetime(events),
          gateway = Gateway()..scopeWait = Completer<Map<String, dynamic>>();
      addTearDown(events.dispose);
      addTearDown(life.dispose);
      final pending = AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
      expect(gateway.captures, 1);
      events.boundary();
      expect(life.current, isFalse);
      gateway.scopeWait!.complete(f.scope().toJson()..['notificationScopeId'] = '44444444-4444-4444-8444-444444444444');
      await expectLater(pending, throwsA(isA<PortablePackageFailure>()));
      expect(gateway.calls, isEmpty);
    },
  );
  test('foreign Native profile reply is rejected, never rebound to a latest profile', () async {
    final events = RuntimeNoticeEventAdapter(),
        life = lifetime(events),
        gateway = Gateway()..scopeWait = Completer<Map<String, dynamic>>();
    addTearDown(events.dispose);
    addTearDown(life.dispose);
    final pending = AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
    gateway.scopeWait!.complete(f.scope().toJson()..['profileId'] = '44444444-4444-4444-8444-444444444444');
    await expectLater(pending, throwsA(isA<PortablePackageFailure>()));
    expect(gateway.calls, isEmpty);
  });
  test('cancel is bound before gateway start, carries only ORIGINAL scope and exact new operation ID', () async {
    final events = RuntimeNoticeEventAdapter(), life = lifetime(events), gateway = Gateway();
    addTearDown(events.dispose);
    final adapter = await AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
    addTearDown(adapter.dispose);
    final cancel = PortableCancellation();
    gateway.beforeReturn = cancel.cancel;
    final work = adapter.context(adapter.scope, f.project, 1, cancel);
    final call = gateway.calls.single;
    expect(gateway.cancels.single.scope, same(adapter.scope));
    expect(gateway.cancels.single.id, call.id);
    call.result.complete(f.contextJson());
    call.joined.complete();
    await expectLater(work, throwsA(isA<PortablePackageFailure>()));
  });
  test('early successful result cannot settle or unregister cancellation before actual joined completion', () async {
    final events = RuntimeNoticeEventAdapter(), gateway = Gateway(), life = lifetime(events);
    addTearDown(events.dispose);
    final adapter = await AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
    addTearDown(adapter.dispose);
    final cancel = PortableCancellation();
    var settled = false;
    final work = adapter
        .context(adapter.scope, f.project, 1, cancel)
        .then<void>(
          (_) {
            settled = true;
          },
          onError: (Object _) {
            settled = true;
          },
        );
    final call = gateway.calls.single;
    call.result.complete(f.contextJson());
    await flush();
    expect(settled, isFalse);
    cancel.cancel();
    expect(gateway.cancels.single.id, call.id);
    call.joined.complete();
    await work;
    expect(settled, isTrue);
  });
  test('an early operation error also waits for joined completion before unbinding', () async {
    final events = RuntimeNoticeEventAdapter(), gateway = Gateway(), life = lifetime(events);
    addTearDown(events.dispose);
    final adapter = await AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
    addTearDown(adapter.dispose);
    final cancel = PortableCancellation();
    var settled = false;
    final work = adapter
        .context(adapter.scope, f.project, 1, cancel)
        .then<void>(
          (_) {
            settled = true;
          },
          onError: (Object _) {
            settled = true;
          },
        );
    final call = gateway.calls.single;
    call.result.completeError(StateError('unrendered Native detail'));
    await flush();
    expect(settled, isFalse);
    cancel.cancel();
    expect(gateway.cancels.single.id, call.id);
    call.joined.complete();
    await work;
  });
  test('pre-cancelled operation starts nothing and cannot create an unused Native tombstone', () async {
    final events = RuntimeNoticeEventAdapter(), gateway = Gateway(), life = lifetime(events);
    addTearDown(events.dispose);
    final adapter = await AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
    addTearDown(adapter.dispose);
    final cancel = PortableCancellation()..cancel();
    await expectLater(adapter.context(adapter.scope, f.project, 1, cancel), throwsA(isA<PortablePackageFailure>()));
    expect(gateway.calls, isEmpty);
    expect(gateway.cancels, isEmpty);
  });
  test('a reused cancellation cannot cause this adapter to unbind somebody else original handler', () async {
    final events = RuntimeNoticeEventAdapter(), gateway = Gateway(), life = lifetime(events);
    addTearDown(events.dispose);
    final adapter = await AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
    addTearDown(adapter.dispose);
    var originalCancels = 0;
    final cancel = PortableCancellation()..bind(() => originalCancels++);
    await expectLater(adapter.context(adapter.scope, f.project, 1, cancel), throwsA(isA<StateError>()));
    cancel.cancel();
    expect(originalCancels, 1);
    expect(gateway.calls, isEmpty);
    cancel.unbind();
  });
  test('password is copied before suspension, owned Dart copy wiped immediately; original is caller-owned', () async {
    final events = RuntimeNoticeEventAdapter(), gateway = Gateway(), life = lifetime(events);
    addTearDown(events.dispose);
    final adapter = await AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
    addTearDown(adapter.dispose);
    final password = SecretText('synthetic package password'), cancel = PortableCancellation();
    final work = adapter.export(f.context().selection({'play.yml'}), password, '/owned/new.ccproject', cancel);
    final call = gateway.calls.single;
    expect(call.secret, isNot(same(password)));
    expect(call.secret!.isWiped, isTrue);
    expect(password.isWiped, isFalse);
    password.wipe();
    call.result.complete({'version': 1, 'encryptedSha256': 'c' * 64, 'bytes': 1000});
    call.joined.complete();
    await work;
  });
  test('domain gap preserves original lifetime and unrelated pending operation', () async {
    final events = RuntimeNoticeEventAdapter();
    events.acceptScope(
      originalEpoch: events.epoch,
      nativeProfile: {'id': f.profile, 'active': true, 'vault_state': 'unlocked', 'notification_scope_id': f.nonce},
    );
    final gateway = Gateway(), life = lifetime(events);
    addTearDown(events.dispose);
    final adapter = await AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
    addTearDown(adapter.dispose);
    final cancel = PortableCancellation(), work = adapter.context(adapter.scope, f.project, 1, cancel);
    final call = gateway.calls.single;
    events.ingest({'type': 'critical_notice_gap', 'profile_id': f.profile, 'scope_id': f.nonce});
    await flush();
    expect(life.current, isTrue);
    expect(cancel.cancelled, isFalse);
    expect(gateway.cancels, isEmpty);
    call.result.complete(f.contextJson());
    call.joined.complete();
    expect((await work).projectId, f.project);
  });
  test(
    'same-profile epoch change reacts, wipes caller buffers, cancels original ID and suppresses late result',
    () async {
      final events = RuntimeNoticeEventAdapter(), gateway = Gateway(), life = lifetime(events);
      addTearDown(events.dispose);
      final adapter = await AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
      addTearDown(adapter.dispose);
      final controller = adapter.createController(files: f.Files(), projectId: f.project, projectRevision: 1);
      addTearDown(controller.dispose);
      final load = controller.load();
      gateway.calls.single.result.complete(f.contextJson());
      gateway.calls.single.joined.complete();
      await load;
      controller.select('play.yml', true);
      final password = SecretText('synthetic package password'),
          confirmation = SecretText('synthetic package password');
      final work = controller.export(password, confirmation);
      await flush();
      final call = gateway.calls.last;
      var notices = 0;
      controller.addListener(() => notices++);
      events.boundary();
      expect(life.current, isFalse);
      await flush();
      expect(password.isWiped && confirmation.isWiped, isTrue);
      expect(gateway.cancels.single.id, call.id);
      expect(gateway.cancels.single.scope, same(adapter.scope));
      expect(notices, greaterThan(0));
      call.result.complete({'version': 1, 'encryptedSha256': 'c' * 64, 'bytes': 1000});
      call.joined.complete();
      await work;
      expect(controller.exported, isNull);
    },
  );
  test(
    'delayed registration after an earlier cancel never starts a body, even if Native tombstone had expired',
    () async {
      final events = RuntimeNoticeEventAdapter(),
          life = lifetime(events),
          gateway = Gateway()..registrationWait = Completer<void>();
      addTearDown(events.dispose);
      final adapter = await AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
      addTearDown(adapter.dispose);
      final cancel = PortableCancellation(), work = adapter.context(adapter.scope, f.project, 1, cancel);
      final call = gateway.calls.single;
      expect(call.bodyStarts, 0);
      cancel.cancel();
      expect(gateway.cancels, hasLength(1));
      gateway.registrationWait!.complete();
      await flush();
      expect(call.bodyStarts, 0);
      expect(gateway.cancels, hasLength(2));
      expect(gateway.cancels.every((e) => e.id == call.id && identical(e.scope, adapter.scope)), isTrue);
      call.joined.complete();
      await expectLater(work, throwsA(isA<PortablePackageFailure>()));
    },
  );
  test(
    'registered acknowledgement precedes body start; retired original epoch can never authorize that start',
    () async {
      final events = RuntimeNoticeEventAdapter(),
          life = lifetime(events),
          gateway = Gateway()..registrationWait = Completer<void>();
      addTearDown(events.dispose);
      final adapter = await AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway);
      addTearDown(adapter.dispose);
      final work = adapter.context(adapter.scope, f.project, 1, PortableCancellation());
      final call = gateway.calls.single;
      events.boundary();
      gateway.registrationWait!.complete();
      await flush();
      expect(call.bodyStarts, 0);
      expect(gateway.cancels.single.scope, same(adapter.scope));
      call.joined.complete();
      await expectLater(work, throwsA(isA<PortablePackageFailure>()));
    },
  );

  test('typed unavailable from an actual Native gateway is preserved, never retried through another source', () async {
    final events = RuntimeNoticeEventAdapter(),
        life = lifetime(events),
        gateway = Gateway()..captureError = const PortablePackageFailure('portable_backend_unavailable');
    addTearDown(events.dispose);
    addTearDown(life.dispose);
    await expectLater(
      AutomationPortableNativeAdapter.capture(lifetime: life, gateway: gateway),
      throwsA(isA<PortablePackageFailure>().having((e) => e.code, 'code', 'portable_backend_unavailable')),
    );
    expect(gateway.captures, 1);
    expect(gateway.calls, isEmpty);
  });
}
