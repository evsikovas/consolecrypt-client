import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/automation/automation_portable_native_adapter.dart';
import 'package:consolecrypt/automation/automation_portable_package.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// Adapts ONLY a Native-issued opaque request. No caller operation ID is sent.
abstract interface class PortableIssuedRequestHandle {
  Future<String> start();
  void cancel();
  Future<void> joined();
}

/// Explicit cancellation and normal successful release are deliberately distinct.
/// release drops this port's transfer ownership; it MUST NOT cancel a handed-off
/// request. A register call retains its own borrow until its actual ACK.
abstract interface class PortableIssuedPasswordTransfer {
  void cancel();
  void release();
}

abstract interface class PortableOpaqueNativePort {
  Future<String> captureScope();
  PortableIssuedPasswordTransfer transferPassword(Uint8List passwordBytes);
  Future<PortableIssuedRequestHandle> registerContext(String scopeJson, String projectId, int projectRevision);
  Future<PortableIssuedRequestHandle> registerExport(
    String requestJson,
    PortableIssuedPasswordTransfer password,
    String destination,
  );
  Future<PortableIssuedRequestHandle> registerInspect(
    String scopeJson,
    String source,
    PortableIssuedPasswordTransfer password,
  );
}

const _unavailable = PortablePackageFailure('portable_backend_unavailable');
const _failed = PortablePackageFailure('portable_operation_failed');
const _changed = PortablePackageFailure('portable_context_changed');

Map<String, dynamic> _decode(String raw) {
  if (utf8.encode(raw).length > 524288) throw const PortablePackageFailure('portable_response_invalid');
  final value = jsonDecode(raw);
  if (value is! Map<String, dynamic>) throw const PortablePackageFailure('portable_response_invalid');
  return value;
}

/// One gateway instance owns local cancellation correlation only. Native-issued
/// handles, original scope and existing host lifetime remain the only boundary.
/// Construction opens no request and captures no current Core/profile/nonce.
final class PortableHandleGateway implements PortableNativeGateway {
  PortableHandleGateway({this.port});
  final PortableOpaqueNativePort? port;
  static const maxOperations = 16;
  final _operations = <String, _Invocation>{};
  Future<PortableScope>? _capture;
  PortableScope? _scope;

  @override
  Future<Map<String, dynamic>> captureScope() async {
    if (port == null) throw _unavailable;
    final scope = await (_capture ??= _captureOriginal());
    return scope.toJson(); // fresh DATA map, never a mutable cached authority
  }

  Future<PortableScope> _captureOriginal() async {
    try {
      final raw = await port!.captureScope();
      if (utf8.encode(raw).length > 1024) throw _failed;
      final scope = PortableScope.fromJson(_decode(raw));
      _scope = scope;
      return scope;
    } catch (_) {
      throw _failed;
    }
  }

  void _check(PortableScope scope, String localId) {
    if (port == null) throw _unavailable;
    if (_scope?.same(scope) != true) throw _changed;
    if (localId.isEmpty ||
        localId.length > 128 ||
        _operations.containsKey(localId) ||
        _operations.length >= maxOperations) {
      throw _failed;
    }
  }

  PortableNativeInvocation _register(
    PortableScope scope,
    String id,
    Future<PortableIssuedRequestHandle> Function(PortableIssuedPasswordTransfer?) registration, {
    SecretText? password,
  }) {
    _check(scope, id);
    final op = _Invocation(this, scope, id, registration);
    _operations[id] = op; // cancellation is addressable BEFORE any port call
    op.bind(password);
    return op.invocation;
  }

  @override
  PortableNativeInvocation context({
    required PortableScope scope,
    required String operationId,
    required String projectId,
    required int projectRevision,
  }) {
    final json = jsonEncode(scope.toJson());
    return _register(scope, operationId, (_) => port!.registerContext(json, projectId, projectRevision));
  }

  @override
  PortableNativeInvocation export({
    required PortableScope scope,
    required String operationId,
    required Map<String, dynamic> request,
    required SecretText password,
    required String destination,
  }) {
    final json = jsonEncode(request);
    if (utf8.encode(json).length > portableMaxExportRequestBytes ||
        !PortableScope.fromJson(request['scope']).same(scope) ||
        !portablePackageFilename(destination)) {
      throw _failed;
    }
    return _register(
      scope,
      operationId,
      (transfer) => port!.registerExport(json, transfer!, destination),
      password: password,
    );
  }

  @override
  PortableNativeInvocation inspect({
    required PortableScope scope,
    required String operationId,
    required String source,
    required SecretText password,
  }) {
    if (!portablePackageFilename(source)) throw _failed;
    final json = jsonEncode(scope.toJson());
    return _register(
      scope,
      operationId,
      (transfer) => port!.registerInspect(json, source, transfer!),
      password: password,
    );
  }

  @override
  Future<void> cancel({required PortableScope scope, required String operationId}) {
    final op = _operations[operationId];
    if (op == null || !op.scope.same(scope)) return Future<void>.value();
    op.cancel(); // synchronous exact-transfer/handle cancellation before return
    return op.done.future;
  }
}

final class _Invocation {
  _Invocation(this.owner, this.scope, this.id, this.registration);
  final PortableHandleGateway owner;
  final PortableScope scope;
  final String id;
  final Future<PortableIssuedRequestHandle> Function(PortableIssuedPasswordTransfer?) registration;
  final registered = Completer<void>(), result = Completer<Map<String, dynamic>>(), done = Completer<void>();
  PortableIssuedPasswordTransfer? transfer;
  PortableIssuedRequestHandle? handle;
  Uint8List? bytes;
  bool cancelled = false, started = false, nativeJoined = false;
  PortableNativeInvocation get invocation => PortableNativeInvocation(
    registered: registered.future,
    startBody: start,
    result: result.future,
    joined: done.future,
  );

  void _wipe() {
    bytes?.fillRange(0, bytes!.length, 0);
    bytes = null;
  }

  void _releaseTransfer() {
    final original = transfer;
    transfer = null;
    try {
      original?.release();
    } catch (_) {
      /* cleanup never exposes diagnostics */
    }
    _wipe();
  }

  void _fail() {
    if (!result.isCompleted) result.completeError(_failed);
  }

  void cancel() {
    cancelled = true;
    _wipe();
    try {
      transfer?.cancel();
    } catch (_) {
      /* Native join remains separate */
    }
    try {
      handle?.cancel();
    } catch (_) {
      /* no latest-owner fallback */
    }
    _fail();
    _removeIfComplete();
  }

  void bind(SecretText? password) {
    // Copy + synchronous Native transfer construction precede the first await.
    // The adapter may wipe its input immediately after this method returns.
    try {
      if (password != null) {
        if (password.isWiped || password.byteLength == 0 || password.byteLength > 4096) throw _failed;
        bytes = password.exposeBytes();
        transfer = owner.port!.transferPassword(bytes!);
        if (cancelled) transfer!.cancel(); // reentrant cancel during sync creation
      }
      if (cancelled) {
        _rejectRegistration();
        return;
      }
      final pending = registration(transfer);
      unawaited(_acknowledge(pending));
    } catch (_) {
      _rejectRegistration();
    }
  }

  void _rejectRegistration() {
    cancel();
    _releaseTransfer();
    if (!registered.isCompleted) registered.completeError(_failed);
    _joined();
  }

  Future<void> _acknowledge(Future<PortableIssuedRequestHandle> pending) async {
    try {
      handle = await pending;
      // Success releases transfer ownership WITHOUT canceling a legitimate body.
      _releaseTransfer();
      if (cancelled) {
        try {
          handle!.cancel();
        } catch (_) {
          /* still await ORIGINAL join */
        }
      } // late ACK never starts its body
      registered.complete();
      try {
        await handle!.joined();
        _joined();
      } catch (_) {
        cancel();
        // Transport failure is NOT evidence that the original worker joined.
        // Retain this bounded owned handle for later exact cancellation.
        if (!done.isCompleted) done.completeError(_failed);
      }
    } catch (_) {
      cancel();
      if (!registered.isCompleted) registered.completeError(_failed);
      _joined(); // registration failed without an issued handle
    } finally {
      _releaseTransfer();
    }
  }

  void start() {
    if (started || cancelled) return; // repeated callback cannot start or recancel
    if (handle == null || !registered.isCompleted || done.isCompleted) {
      cancel();
      return;
    }
    started = true; // one-use even when Native transport fails synchronously
    unawaited(_start());
  }

  Future<void> _start() async {
    try {
      final raw = await handle!.start();
      final value = _decode(raw);
      if (!cancelled && !result.isCompleted) result.complete(value);
    } catch (_) {
      // A transport error may precede Native start. Retire this exact idle or
      // running handle promptly; a result error is still not proof of join.
      cancel();
    } finally {
      _removeIfComplete();
    }
  }

  void _removeIfComplete() {
    if (nativeJoined && result.isCompleted && identical(owner._operations[id], this)) {
      owner._operations.remove(id);
    }
  }

  void _joined() {
    nativeJoined = true;
    if (!started) _fail(); // expired idle cannot strand an unstarted result
    if (!done.isCompleted) done.complete();
    // Native retirement may precede delivery of the successful start response.
    // Do not replace a pending legitimate result with a fabricated failure.
    _removeIfComplete();
  }
}
