import 'dart:async';

import 'package:consolecrypt/monitoring/metrics_ai_context.dart';
import 'package:consolecrypt/monitoring/metrics_ai_review_flow.dart';

/// Hold the already-selected built-in endpoint throughout the exact review.
/// Starting inference, downloading a model and falling back to another provider
/// are not capabilities of this adapter. The existing local service owns startup.
final class MetricsAiLeasedTransport implements MetricsAiReviewTransport {
  MetricsAiLeasedTransport({required this.delegate, required this.withProvider, required this.isCurrent});

  final MetricsAiReviewTransport delegate;
  final Future<void> Function(Future<void> Function() body) withProvider;
  final bool Function() isCurrent;
  Completer<void>? _release;
  Completer<void>? _enteredSignal;
  Future<void>? _ready;
  bool _closed = false, _entered = false, _broken = false;

  void _check() {
    if (_closed || _broken || !isCurrent()) {
      throw const MetricsAiFlowException(MetricsAiFlowIssue.contextChanged);
    }
  }

  Future<void> _ensure() {
    _check();
    if (_ready case final ready?) return ready;
    final entered = Completer<void>(), release = Completer<void>();
    _release = release;
    _enteredSignal = entered;
    _ready = entered.future;
    // Observe every holder failure, including one arriving after the preview.
    // No background future is left with an unhandled error or pending ready.
    unawaited(
      Future<void>.sync(
        () => withProvider(() async {
          _check();
          _entered = true;
          entered.complete();
          await release.future;
          _entered = false;
        }),
      ).then<void>(
        (_) {
          _entered = false;
          if (!_closed) _broken = true;
          if (!entered.isCompleted) {
            entered.completeError(const MetricsAiFlowException(MetricsAiFlowIssue.contextChanged));
          }
        },
        onError: (Object _, StackTrace _) {
          _entered = false;
          _broken = true;
          if (!entered.isCompleted) {
            entered.completeError(const MetricsAiFlowException(MetricsAiFlowIssue.transport));
          }
        },
      ),
    );
    return entered.future;
  }

  @override
  Future<MetricsAiPreparedReview> prepare(MetricsAiFence original, MetricsAiRequest request) async {
    await _ensure();
    _check();
    if (!_entered) throw const MetricsAiFlowException(MetricsAiFlowIssue.contextChanged);
    return delegate.prepare(original, request);
  }

  @override
  Future<MetricsAiReadonlyProposal> send(String ticket, {required bool approveExternal}) async {
    _check();
    // Never silently register/restart a different endpoint behind a frozen preview.
    if (!_entered) throw const MetricsAiFlowException(MetricsAiFlowIssue.contextChanged);
    return delegate.send(ticket, approveExternal: approveExternal);
  }

  @override
  Future<void> discard(String ticket) => delegate.discard(ticket);

  void dispose() {
    _closed = true;
    final entered = _enteredSignal;
    if (entered != null && !entered.isCompleted) {
      entered.completeError(const MetricsAiFlowException(MetricsAiFlowIssue.contextChanged));
    }
    final release = _release;
    if (release != null && !release.isCompleted) release.complete();
  }
}
