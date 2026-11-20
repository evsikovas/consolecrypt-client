import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/automation/automation_portable_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_portable_import_gateway.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:flutter_test/flutter_test.dart';

import '../helpers/portable_import_fixture.dart' as f;

class Transfer implements PortableIssuedPasswordTransfer {
  Transfer(Uint8List bytes) : copy = Uint8List.fromList(bytes);
  final Uint8List copy;
  int cancels = 0, releases = 0;
  @override
  void cancel() {
    cancels++;
  }

  @override
  void release() {
    releases++;
    copy.fillRange(0, copy.length, 0);
  }
}

class Handle implements PortableIssuedImportHandle {
  int starts = 0, accepts = 0, cancels = 0;
  String? review, digest;
  final started = Completer<String>(), accepted = Completer<String>(), retired = Completer<void>();
  @override
  Future<String> start() {
    starts++;
    return started.future;
  }

  @override
  Future<String> acceptImport({required String reviewId, required String reviewDigest}) {
    accepts++;
    review = reviewId;
    digest = reviewDigest;
    return accepted.future;
  }

  @override
  void cancel() {
    cancels++;
  }

  @override
  Future<void> joined() => retired.future;
}

class Port implements PortableImportOpaqueNativePort {
  Transfer? transfer;
  Uint8List? borrowedBytes;
  final ack = Completer<PortableIssuedImportHandle>();
  Map<String, Object?>? request;
  @override
  PortableIssuedPasswordTransfer transferPassword(Uint8List bytes) {
    borrowedBytes = bytes;
    return transfer = Transfer(bytes);
  }

  @override
  Future<PortableIssuedImportHandle> registerImport({
    required String scopeJson,
    required String source,
    required String requestedName,
    required PortableIssuedPasswordTransfer password,
  }) {
    expect(identical(password, transfer), isTrue);
    request = {'scope': jsonDecode(scopeJson), 'source': source, 'requestedName': requestedName};
    return ack.future;
  }
}

Future<void> flush() => Future<void>.delayed(Duration.zero);

void main() {
  late Port port;
  late Handle handle;
  late PortableImportIntent intent;
  late SecretText password;
  late PortableImportRegistration registration;
  setUp(() {
    port = Port();
    handle = Handle();
    password = SecretText('SYNTHETIC_PASSWORD');
    intent = PortableImportIntent.capture(
      scope: f.scope(),
      source: 'D:/owned/input.ccproject',
      requestedName: ' Raw name ',
    );
  });
  void register() =>
      registration = PortableImportHandleGateway(port: port).register(intent: intent, password: password);
  Future<PortableImportReviewSession> acknowledge() async {
    port.ack.complete(handle);
    return registration.registered;
  }

  Future<PortableImportReviewSession> ready() async {
    register();
    final session = await acknowledge();
    final result = session.preview();
    handle.started.complete(jsonEncode(f.readyJson(intent)));
    await result;
    return session;
  }

  tearDown(() {
    password.wipe();
  });

  test('missing capability refuses before transfer or registration', () {
    expect(
      () => const PortableImportHandleGateway().register(intent: intent, password: password),
      throwsA(isA<PortableImportFailure>()),
    );
    expect(port.transfer, isNull);
  });
  test('sync bounded transfer copy precedes return and source/name never enter password JSON', () async {
    register();
    password.wipe();
    expect(port.transfer!.copy.any((v) => v != 0), isTrue);
    expect(port.borrowedBytes!.every((v) => v == 0), isTrue);
    expect(port.request!.keys, ['scope', 'source', 'requestedName']);
    expect(port.request!['requestedName'], ' Raw name ');
    expect(jsonEncode(port.request), isNot(contains('SYNTHETIC_PASSWORD')));
    await acknowledge();
    handle.retired.complete();
    await registration.joined;
  });
  test('normal ACK releases transfer without cancelling legitimate issued owner', () async {
    register();
    await acknowledge();
    expect(port.transfer!.releases, 1);
    expect(port.transfer!.cancels, 0);
    expect(handle.cancels, 0);
    handle.retired.complete();
    await registration.joined;
  });
  test('preACK cancellation targets original transfer and late exact handle before preview', () async {
    register();
    registration.cancel();
    expect(port.transfer!.cancels, 1);
    final session = await acknowledge();
    expect(handle.cancels, 1);
    expect(handle.starts, 0);
    await expectLater(session.preview(), throwsA(isA<PortableImportFailure>()));
    bool joined = false;
    unawaited(registration.joined.then((_) => joined = true));
    await flush();
    expect(joined, isFalse);
    handle.retired.complete();
    await registration.joined;
    expect(handle.starts, 0);
  });
  test('Ready finishes before true joined and only matching refs can be accepted once', () async {
    final session = await ready();
    bool joined = false;
    unawaited(registration.joined.then((_) => joined = true));
    await flush();
    expect(joined, isFalse);
    final reviewed = PortableImportReady.fromJson(f.readyJson(intent), intent: intent);
    await expectLater(
      session.accept(reviewId: 'wrong', reviewDigest: reviewed.reviewDigest),
      throwsA(isA<PortableImportFailure>()),
    );
    expect(handle.accepts, 0);
    final result = session.accept(reviewId: reviewed.reviewId, reviewDigest: reviewed.reviewDigest);
    await expectLater(
      session.accept(reviewId: reviewed.reviewId, reviewDigest: reviewed.reviewDigest),
      throwsA(isA<PortableImportFailure>()),
    );
    expect(handle.accepts, 1);
    expect(handle.review, reviewed.reviewId);
    handle.accepted.complete(jsonEncode(f.installedJson(reviewed)));
    await result;
    handle.retired.complete();
    await registration.joined;
  });
  test('accept while preview still pending sends no Native accept', () async {
    register();
    final session = await acknowledge();
    final preview = session.preview();
    await expectLater(
      session.accept(reviewId: f.reviewId, reviewDigest: 'a' * 64),
      throwsA(isA<PortableImportFailure>()),
    );
    expect(handle.accepts, 0);
    handle.started.complete(jsonEncode(f.readyJson(intent)));
    await preview;
    handle.retired.complete();
    await registration.joined;
  });
  test('start transport failure cancels exact idle owner but never fakes joined', () async {
    register();
    final session = await acknowledge();
    final failed = expectLater(session.preview(), throwsA(isA<PortableImportFailure>()));
    handle.started.completeError(StateError('PRIVATE_TRANSPORT'));
    await failed;
    expect(handle.cancels, 1);
    bool joined = false;
    unawaited(registration.joined.then((_) => joined = true));
    await flush();
    expect(joined, isFalse);
    handle.retired.complete();
    await registration.joined;
  });
  test('invalid/future Ready cancels owned handle without enabling acceptance', () async {
    register();
    final session = await acknowledge();
    final failed = expectLater(session.preview(), throwsA(isA<PortableImportFailure>()));
    handle.started.complete(jsonEncode({...f.readyJson(intent), 'version': 2}));
    await failed;
    expect(handle.cancels, 1);
    expect(handle.accepts, 0);
    handle.retired.complete();
    await registration.joined;
  });
  test('unknown accept transport error is sticky recovery domain and cancels exact owner', () async {
    final session = await ready();
    final reviewed = PortableImportReady.fromJson(f.readyJson(intent), intent: intent);
    final failed = expectLater(
      session.accept(reviewId: reviewed.reviewId, reviewDigest: reviewed.reviewDigest),
      throwsA(isA<PortableImportFailure>().having((e) => e.issue, 'issue', PortableImportIssue.recoveryRequired)),
    );
    handle.accepted.completeError(StateError('PRIVATE_TRANSPORT'));
    await failed;
    expect(handle.cancels, 1);
    handle.retired.complete();
    await registration.joined;
  });
  test('actual joined before accept response delivery does not erase a legitimate response', () async {
    final session = await ready();
    final reviewed = PortableImportReady.fromJson(f.readyJson(intent), intent: intent);
    final result = session.accept(reviewId: reviewed.reviewId, reviewDigest: reviewed.reviewDigest);
    handle.retired.complete();
    await registration.joined;
    final raw = jsonEncode(f.installedJson(reviewed));
    handle.accepted.complete(raw);
    expect(await result, raw);
  });
  test('registration ACK failure cannot fabricate true retirement', () async {
    register();
    final failed = expectLater(registration.registered, throwsA(isA<PortableImportFailure>()));
    final joinFailed = expectLater(registration.joined, throwsA(isA<PortableImportFailure>()));
    port.ack.completeError(StateError('PRIVATE_ACK_FAILURE'));
    await failed;
    await joinFailed;
    expect(port.transfer!.cancels, greaterThanOrEqualTo(1));
    expect(port.transfer!.releases, 1);
  });
  test('join transport failure preserves failure rather than asserting clean retirement', () async {
    register();
    await acknowledge();
    final failed = expectLater(registration.joined, throwsA(isA<PortableImportFailure>()));
    handle.retired.completeError(StateError('PRIVATE_JOIN_FAILURE'));
    await failed;
    expect(handle.cancels, 1);
  });
}
