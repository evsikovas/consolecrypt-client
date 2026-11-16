import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/automation/automation_awx_service.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart'
    show MonitoringSource;

/// Adapt ONLY the future generated opaque Native handle to this interface.
/// There is no caller-supplied Native ID, endpoint, token or action on start.
abstract interface class AwxIssuedRequestHandle {
  Future<String> start();
  void cancel();
  Future<void> joined();
}

/// Optional injected port. This file guesses no generated function/class names,
/// creates no Native subscription and never falls back to Dart HTTP.
abstract interface class AwxOpaqueNativePort {
  Future<String> captureScope();
  Future<String> catalogRequest(String requestJson);
  Future<AwxIssuedRequestHandle> registerRequest(String requestJson);
  Future<void> discardReview(String requestJson);
}

/// Optional production service capability; missing support exposes no AWX tab.
/// Creating the port opens no request/session and resolves no credentials.
abstract interface class AutomationAwxPortCapability {
  AwxOpaqueNativePort createAwxPort();
}

AwxFailure awxSafeFailure(Object error) {
  if (error is AwxFailure) return error;
  if (error is AwxModelException ||
      error is FormatException ||
      error is TypeError) {
    return const AwxFailure(AwxFailureReason.response);
  }
  if (error is AppException &&
      error.code == AppErrorCode.validation &&
      error.reason == AppErrorReason.invalidField &&
      error.args['field'] == 'automation_awx') {
    final reason = switch (error.args['rule']) {
      'awx_configuration' ||
      'awx_invalid_configuration' ||
      'awx_credential_unavailable' => AwxFailureReason.configuration,
      'awx_invalid_input' => AwxFailureReason.invalidInput,
      'awx_canceled' => AwxFailureReason.canceled,
      'awx_expired' ||
      'awx_review_unavailable' ||
      'awx_registration_unavailable' => AwxFailureReason.expired,
      'awx_changed' ||
      'awx_context_changed' ||
      'awx_connection_changed' ||
      'awx_catalog_changed' ||
      'awx_credential_changed' ||
      'awx_connection_unavailable' ||
      'awx_catalog_read_only' ||
      'awx_runtime_closed' ||
      'awx_revision_limit' => AwxFailureReason.changed,
      'awx_transport' => AwxFailureReason.transport,
      'awx_response' || 'awx_response_encoding' => AwxFailureReason.response,
      'awx_budget' ||
      'awx_operation_budget' ||
      'awx_review_budget' ||
      'awx_input_budget' ||
      'awx_catalog_budget' => AwxFailureReason.budget,
      'awx_forbidden' => AwxFailureReason.forbidden,
      'awx_unauthorized' => AwxFailureReason.unauthorized,
      'awx_needs_awx_input' => AwxFailureReason.needsAwxInput,
      'awx_unsupported_prompts' => AwxFailureReason.unsupportedPrompts,
      _ => AwxFailureReason.unknown,
    };
    return AwxFailure(reason);
  }
  return const AwxFailure(AwxFailureReason.unknown);
}

Object? _decode(String json) {
  if (utf8.encode(json).length > 512 * 1024) {
    throw const AwxFailure(AwxFailureReason.budget);
  }
  return jsonDecode(json);
}

String _request(Map<String, Object?> json) {
  final encoded = jsonEncode(json);
  if (utf8.encode(encoded).length > 64 * 1024) {
    throw const AwxFailure(AwxFailureReason.budget);
  }
  return encoded;
}

void _number(int value, {bool zero = false}) {
  if (value < (zero ? 0 : 1) || value > awxMaxId) {
    throw const AwxFailure(AwxFailureReason.invalidInput);
  }
}

/// One frontend owner's bounded correlation map, NOT a Native ID registry or
/// source of authority. Every cancellation targets its original issued handle.
final class AwxHandleGateway implements AwxNativeGateway {
  AwxHandleGateway({
    required this.profileId,
    required this.isOriginalCurrent,
    this.port,
    DateTime Function()? now,
  }) : _now = now ?? DateTime.now;
  final String profileId;
  final bool Function() isOriginalCurrent;
  final AwxOpaqueNativePort? port;
  final DateTime Function() _now;
  final _operations = <String, _OwnedInvocation<Object?>>{};
  final _reviews = <String, ({AwxConnection connection, DateTime expires})>{};
  AwxNativeScope? _scope;
  Future<AwxNativeScope>? _captureFuture;
  bool _closed = false, _broken = false, _retired = false;
  bool get current {
    if (_closed || _broken || _retired) return false;
    if (!isOriginalCurrent()) {
      _retired = true;
      return false;
    }
    return true;
  }

  bool _matches(AwxNativeScope scope) =>
      _scope?.profileId == scope.profileId &&
      _scope?.notificationScopeId == scope.notificationScopeId;
  void _check(AwxNativeScope scope) {
    if (port == null) throw const AwxFailure(AwxFailureReason.unavailable);
    if (!current || !_matches(scope)) {
      throw const AwxFailure(AwxFailureReason.changed);
    }
  }

  @override
  Future<AwxNativeScope> captureScope() {
    if (port == null) {
      return Future.error(const AwxFailure(AwxFailureReason.unavailable));
    }
    if (!current) {
      return Future.error(const AwxFailure(AwxFailureReason.changed));
    }
    return _captureFuture ??= _capture();
  }

  Future<AwxNativeScope> _capture() async {
    try {
      final value = AwxNativeScope.fromJson(
        _decode(await port!.captureScope()),
      );
      if (!current || value.profileId != profileId) {
        throw const AwxFailure(AwxFailureReason.changed);
      }
      _scope = value;
      return value;
    } catch (error) {
      throw awxSafeFailure(error);
    }
  }

  Future<AwxCatalog> _catalog(
    AwxNativeScope scope,
    Map<String, Object?> payload,
  ) async {
    _check(scope);
    final request = _request({...scope.toJson(), ...payload});
    try {
      final result = AwxCatalog.fromJson(
        _decode(await port!.catalogRequest(request)),
      );
      _check(scope);
      return result;
    } catch (error) {
      throw awxSafeFailure(error);
    }
  }

  @override
  Future<AwxCatalog> catalog(AwxNativeScope original) =>
      _catalog(original, {'kind': 'list'});
  @override
  Future<AwxCatalog> saveConnection(
    AwxNativeScope original, {
    required int expectedCatalogRevision,
    required int? expectedRevision,
    required MonitoringSource configuration,
  }) {
    _number(expectedCatalogRevision, zero: true);
    if (expectedRevision != null) _number(expectedRevision);
    validateAwxConfiguration(configuration, create: expectedRevision == null);
    return _catalog(original, {
      'kind': 'save',
      'expectedCatalogRevision': expectedCatalogRevision,
      'expectedRevision': expectedRevision,
      'configuration': configuration.toJson(),
    });
  }

  @override
  Future<AwxCatalog> deleteConnection(
    AwxNativeScope original, {
    required int expectedCatalogRevision,
    required AwxConnection connection,
  }) {
    _number(expectedCatalogRevision, zero: true);
    if (connection.readOnly || connection.revision == null) {
      throw const AwxFailure(AwxFailureReason.changed);
    }
    return _catalog(original, {
      'kind': 'delete',
      'expectedCatalogRevision': expectedCatalogRevision,
      'connectionId': connection.id,
      'expectedRevision': connection.revision,
    });
  }

  Map<String, Object?> _connection(AwxConnection c) {
    if (c.readOnly || c.revision == null) {
      throw const AwxFailure(AwxFailureReason.changed);
    }
    return {'connectionId': c.id, 'connectionRevision': c.revision};
  }

  AwxRegisteredOperation<T> _register<T>(
    AwxNativeScope scope,
    String localId,
    Map<String, Object?> action,
    T Function(Object?) decode, {
    void Function(T)? stale,
    void Function()? registrationFailed,
  }) {
    _check(scope);
    if (localId.isEmpty ||
        localId.length > 128 ||
        _operations.containsKey(localId)) {
      throw const AwxFailure(AwxFailureReason.invalidInput);
    }
    if (_operations.length >= 32) {
      throw const AwxFailure(AwxFailureReason.budget);
    }
    // Serialize the immutable scope/action BEFORE the first suspension. The
    // local correlation ID is deliberately absent from the Native request.
    final request = _request({...scope.toJson(), 'action': action});
    final op = _OwnedInvocation<T>(
      owner: this,
      scope: scope,
      localId: localId,
      request: request,
      decode: decode,
      stale: stale,
      onRegistrationFailed: registrationFailed,
    );
    _operations[localId] = op;
    op.bind();
    return op;
  }

  @override
  AwxRegisteredOperation<AwxTemplatePage> registerTemplates(
    AwxNativeScope original,
    String operationId,
    AwxConnection connection,
    int page,
    String search,
  ) {
    if (page < 1 ||
        page > 1000 ||
        utf8.encode(search).length > 256 ||
        search.runes.any((c) => c < 32 || (c >= 127 && c <= 159))) {
      throw const AwxFailure(AwxFailureReason.invalidInput);
    }
    return _register(
      original,
      operationId,
      {
        'kind': 'templates',
        ..._connection(connection),
        'page': page,
        'search': search,
      },
      (raw) => AwxTemplatePage.fromJson(
        awxEnvelopeData(raw, 'templates'),
        expectedPage: page,
      ),
    );
  }

  @override
  AwxRegisteredOperation<AwxPreparedReview> registerPrepare(
    AwxNativeScope original,
    String operationId,
    AwxConnection connection,
    int templateId,
    AwxLaunchParameters parameters,
  ) {
    _number(templateId);
    return _register(
      original,
      operationId,
      {
        'kind': 'prepare',
        ..._connection(connection),
        'templateId': templateId,
        'parameters': parameters.toJson(),
      },
      (raw) {
        final value = AwxPreparedReview.fromJson(
          awxEnvelopeData(raw, 'prepare'),
          connection: connection,
          templateId: templateId,
          parameters: parameters,
          now: _now(),
        );
        _reviews.removeWhere(
          (_, entry) => !entry.expires.isAfter(_now().toUtc()),
        );
        if (_reviews.length >= 4) {
          unawaited(_discardOriginal(original, value.nativeTicket));
          throw const AwxFailure(AwxFailureReason.budget);
        }
        _reviews[value.nativeTicket] = (
          connection: connection,
          expires: value.expiresUtc,
        );
        return value;
      },
      stale: (review) {
        unawaited(discardReview(original, review.nativeTicket));
      },
    );
  }

  @override
  AwxRegisteredOperation<AwxLaunchOutcome> registerLaunch(
    AwxNativeScope original,
    String operationId,
    String reviewTicket,
  ) {
    _check(original);
    final reviewed = _reviews.remove(reviewTicket);
    if (reviewed == null) throw const AwxFailure(AwxFailureReason.changed);
    try {
      return _register(
        original,
        operationId,
        {'kind': 'launch', 'reviewTicket': reviewTicket},
        (raw) => AwxLaunchOutcome.fromJson(
          awxEnvelopeData(raw, 'launch'),
          connection: reviewed.connection,
        ),
        registrationFailed: () {
          unawaited(_discardOriginal(original, reviewTicket));
        },
      );
    } catch (error) {
      unawaited(_discardOriginal(original, reviewTicket));
      rethrow;
    }
  }

  @override
  AwxRegisteredOperation<AwxJob> registerJob(
    AwxNativeScope original,
    String operationId,
    AwxConnection connection,
    int jobId,
  ) {
    _number(jobId);
    return _register(
      original,
      operationId,
      {'kind': 'job', ..._connection(connection), 'jobId': jobId},
      (raw) => AwxJob.fromJson(awxEnvelopeData(raw, 'job'), expectedId: jobId),
    );
  }

  @override
  AwxRegisteredOperation<AwxEventPage> registerEvents(
    AwxNativeScope original,
    String operationId,
    AwxConnection connection,
    int jobId,
    int after,
  ) {
    _number(jobId);
    _number(after, zero: true);
    return _register(
      original,
      operationId,
      {
        'kind': 'events',
        ..._connection(connection),
        'jobId': jobId,
        'after': after,
      },
      (raw) => AwxEventPage.fromJson(
        awxEnvelopeData(raw, 'events'),
        requestedAfter: after,
      ),
    );
  }

  @override
  Future<void> cancel(AwxNativeScope original, String operationId) async {
    if (!_matches(original)) return;
    _operations[operationId]?.cancel();
  }

  @override
  Future<void> discardReview(
    AwxNativeScope original,
    String reviewTicket,
  ) async {
    if (!_matches(original) || _reviews.remove(reviewTicket) == null) return;
    await _discardOriginal(original, reviewTicket);
  }

  Future<void> _discardOriginal(
    AwxNativeScope original,
    String reviewTicket,
  ) async {
    if (!_matches(original) || port == null) return;
    try {
      await port!.discardReview(
        _request({...original.toJson(), 'reviewTicket': reviewTicket}),
      );
    } catch (_) {
      /* Original scope only; no latest-profile retry or raw diagnostics. */
    }
  }

  void _retire(String id, _OwnedInvocation<Object?> op) {
    if (identical(_operations[id], op)) _operations.remove(id);
  }

  /// Retire this owner and discard its known tickets. Each invocation.joined
  /// remains the sole proof of actual Native worker completion; close does not
  /// synthesize a Joined value for a pending registration or request.
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    for (final op in _operations.values.toList(growable: false)) {
      op.cancel();
    }
    final scope = _scope;
    if (scope != null) {
      await Future.wait([
        for (final ticket in _reviews.keys.toList(growable: false))
          discardReview(scope, ticket),
      ]);
    }
  }
}

final class _OwnedInvocation<T> implements AwxRegisteredOperation<T> {
  _OwnedInvocation({
    required this.owner,
    required this.scope,
    required this.localId,
    required this.request,
    required this.decode,
    this.stale,
    this.onRegistrationFailed,
  }) {
    // These are independent result channels. An owner may retire after only
    // awaiting registered; suppress unhandled errors without altering awaits.
    _registered.future.ignore();
    _result.future.ignore();
    _joined.future.ignore();
  }
  final AwxHandleGateway owner;
  final AwxNativeScope scope;
  final String localId, request;
  final T Function(Object?) decode;
  final void Function(T)? stale;
  final void Function()? onRegistrationFailed;
  final _registered = Completer<void>(),
      _result = Completer<T>(),
      _joined = Completer<void>();
  AwxIssuedRequestHandle? _handle;
  bool _canceled = false, _started = false, _nativeJoined = false;
  @override
  Future<void> get registered => _registered.future;
  @override
  Future<T> get result => _result.future;
  @override
  Future<void> get joined => _joined.future;
  void bind() {
    unawaited(_bind());
  }

  Future<void> _bind() async {
    try {
      final handle = await owner.port!.registerRequest(request);
      _handle = handle;
      unawaited(_join(handle));
      if (_canceled || !owner.current) {
        cancel();
      }
      _registered.complete();
    } catch (error) {
      final safe = awxSafeFailure(error);
      onRegistrationFailed?.call();
      if (_handle != null) cancel();
      if (!_registered.isCompleted) _registered.completeError(safe);
      if (!_result.isCompleted) _result.completeError(safe);
      if (_handle == null && !_joined.isCompleted) _joined.complete();
      if (_handle == null) owner._retire(localId, this);
    }
  }

  void cancel() {
    _canceled = true;
    final handle = _handle;
    if (handle != null) {
      try {
        handle.cancel();
      } catch (_) {
        owner._broken = true;
      }
    }
    if (handle != null && !_started && !_result.isCompleted) {
      _result.completeError(const AwxFailure(AwxFailureReason.canceled));
    }
  }

  @override
  void startBody() {
    final handle = _handle;
    if (handle == null || !_registered.isCompleted || _started) {
      throw const AwxFailure(AwxFailureReason.invalidInput);
    }
    _started = true;
    if (_canceled || _nativeJoined || !owner.current) {
      cancel();
      if (!_result.isCompleted) {
        _result.completeError(const AwxFailure(AwxFailureReason.changed));
      }
      return;
    }
    unawaited(_run(handle));
  }

  Future<void> _run(AwxIssuedRequestHandle handle) async {
    try {
      final value = decode(_decode(await handle.start()));
      if (!owner.current) {
        stale?.call(value);
        throw const AwxFailure(AwxFailureReason.changed);
      }
      if (!_result.isCompleted) _result.complete(value);
    } catch (error) {
      if (!_result.isCompleted) _result.completeError(awxSafeFailure(error));
    }
  }

  Future<void> _join(AwxIssuedRequestHandle handle) async {
    try {
      await handle.joined();
      _nativeJoined = true;
      if (!_joined.isCompleted) _joined.complete();
    } catch (error) {
      owner._broken = true;
      final safe = awxSafeFailure(error);
      if (!_result.isCompleted) _result.completeError(safe);
      if (!_joined.isCompleted) _joined.completeError(safe);
    } finally {
      owner._retire(localId, this);
    }
  }
}
