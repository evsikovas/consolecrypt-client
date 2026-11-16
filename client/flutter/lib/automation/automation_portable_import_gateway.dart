import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:consolecrypt/automation/automation_portable_handle_gateway.dart';
import 'package:consolecrypt/automation/automation_portable_import_session.dart';
import 'package:consolecrypt/core/security/secret_text.dart';

/// The original issued import owner. Ready does not retire this handle.
abstract interface class PortableIssuedImportHandle {
  Future<String> start();
  Future<String> acceptImport({required String reviewId, required String reviewDigest});
  void cancel();
  Future<void> joined();
}

abstract interface class PortableImportOpaqueNativePort {
  PortableIssuedPasswordTransfer transferPassword(Uint8List bytes);
  Future<PortableIssuedImportHandle> registerImport({
    required String scopeJson,
    required String source,
    required String requestedName,
    required PortableIssuedPasswordTransfer password,
  });
}

/// Instance-owned registrations; no Native ID, global registry, RPC at construction
/// or reuse of export/inspect's await-joined result path.
final class PortableImportHandleGateway implements PortableImportReviewGateway {
  const PortableImportHandleGateway({this.port});
  final PortableImportOpaqueNativePort? port;
  @override
  PortableImportRegistration register({required PortableImportIntent intent, required SecretText password}) {
    final original = port;
    if (original == null) {
      throw const PortableImportFailure(PortableImportIssue.unavailable);
    }
    if (password.isWiped || password.byteLength == 0 || password.byteLength > 4096) {
      throw const PortableImportFailure(PortableImportIssue.invalidInput);
    }
    final registration = _Registration(original, intent);
    registration.bind(password);
    return registration;
  }
}

const _failed = PortableImportFailure(PortableImportIssue.failed);
const _cancelled = PortableImportFailure(PortableImportIssue.cancelled);

PortableImportFailure _safe(Object error, {bool accepting = false}) => error is PortableImportFailure
    ? error
    : accepting
    ? const PortableImportFailure(PortableImportIssue.recoveryRequired)
    : _failed;

final class _Registration implements PortableImportRegistration {
  _Registration(this.port, this.intent);
  final PortableImportOpaqueNativePort port;
  final PortableImportIntent intent;
  final _registered = Completer<PortableImportReviewSession>();
  final _joined = Completer<void>();
  PortableIssuedPasswordTransfer? _transfer;
  PortableIssuedImportHandle? _handle;
  bool _cancelled = false, _retired = false;
  @override
  Future<PortableImportReviewSession> get registered => _registered.future;
  @override
  Future<void> get joined => _joined.future;

  void bind(SecretText password) {
    Uint8List? bytes;
    try {
      // The Native transfer construction synchronously owns a separate copy.
      // Its registration borrow remains retained through the actual ACK.
      bytes = password.exposeBytes();
      _transfer = port.transferPassword(bytes);
      final pending = port.registerImport(
        scopeJson: jsonEncode(intent.scope.toJson()),
        source: intent.source,
        requestedName: intent.requestedName,
        password: _transfer!,
      );
      unawaited(_acknowledge(pending));
    } catch (error) {
      _registrationFailed(error);
    } finally {
      bytes?.fillRange(0, bytes.length, 0);
    }
  }

  void _release() {
    final original = _transfer;
    _transfer = null;
    try {
      original?.release(); // successful ACK is NOT cancellation
    } catch (_) {
      /* no diagnostics, join is independently observed */
    }
  }

  void _registrationFailed(Object error) {
    cancel();
    _release();
    if (!_registered.isCompleted) _registered.completeError(_safe(error));
    // A missing ACK is not proof that a Native owner retired. Surface failed
    // cleanup instead of manufacturing joined; controller blocks another attempt.
    if (!_joined.isCompleted) _joined.completeError(_failed);
  }

  Future<void> _acknowledge(Future<PortableIssuedImportHandle> pending) async {
    try {
      final handle = await pending;
      _handle = handle;
      _release();
      if (_cancelled) {
        try {
          handle.cancel();
        } catch (_) {
          /* await this same owner */
        }
      }
      _registered.complete(_Session(this, handle));
      try {
        await handle.joined();
        _retired = true;
        _joined.complete();
      } catch (_) {
        cancel();
        if (!_joined.isCompleted) _joined.completeError(_failed);
      }
    } catch (error) {
      _registrationFailed(error);
    }
  }

  @override
  void cancel() {
    _cancelled = true;
    try {
      _transfer?.cancel();
    } catch (_) {
      /* no replacement owner */
    }
    try {
      _handle?.cancel();
    } catch (_) {
      /* no fabricated join */
    }
  }
}

final class _Session implements PortableImportReviewSession {
  _Session(this.owner, this.handle);
  final _Registration owner;
  final PortableIssuedImportHandle handle;
  bool _started = false, _accepted = false;
  PortableImportReady? _reviewed;
  @override
  Future<String> preview() async {
    if (_started || owner._cancelled || owner._retired) throw _cancelled;
    _started = true;
    try {
      final raw = await handle.start();
      if (owner._cancelled || owner._retired) throw _cancelled;
      _reviewed = PortableImportReady.fromJson(portableImportDecode(raw), intent: owner.intent);
      return raw;
    } catch (error) {
      owner.cancel(); // transport may have failed before Native start
      throw _safe(error);
    }
  }

  @override
  Future<String> accept({required String reviewId, required String reviewDigest}) async {
    final reviewed = _reviewed;
    if (reviewed == null || _accepted || owner._cancelled || owner._retired) throw _cancelled;
    if (!reviewed.publicationSupported || reviewId != reviewed.reviewId || reviewDigest != reviewed.reviewDigest) {
      throw const PortableImportFailure(PortableImportIssue.response);
    }
    _accepted = true; // one-use before ANY asynchronous suspension
    try {
      // These refs are data from the same retained review, not bearer approval.
      return await handle.acceptImport(reviewId: reviewId, reviewDigest: reviewDigest);
    } catch (error) {
      owner.cancel();
      throw _safe(error, accepting: true);
    }
  }

  @override
  void cancel() => owner.cancel();
}
