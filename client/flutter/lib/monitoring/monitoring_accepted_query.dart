import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_failure_policy.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';

/// An exact read identity, not a source of authorization. The existing
/// workspace service/generation must still admit and fence every Native read.
final class MonitoringQueryIdentity {
  MonitoringQueryIdentity({required this.scope, required MonitoringSource source, required MonitoringQuery query})
    : expression = query.expression,
      sourceFingerprint = source.fingerprint,
      operation = jsonEncode(query.toOperation()) {
    query.validate();
  }
  final Object scope;
  final String expression, sourceFingerprint, operation;

  bool matches(MonitoringQueryIdentity other) =>
      identical(scope, other.scope) && sourceFingerprint == other.sourceFingerprint && operation == other.operation;
}

final class MonitoringAcceptedQuery {
  const MonitoringAcceptedQuery(this.identity, this.outcome, this.completedUtc);
  final MonitoringQueryIdentity identity;
  final QueryOutcome outcome;
  final DateTime completedUtc;
}

enum MonitoringAcceptedQueryPhase { empty, refreshing, fresh, stale, failed }

/// Immutable presentation bookkeeping owned by the existing workspace. No
/// service, listener, timer, persistence or query scheduling lives here.
final class MonitoringAcceptedQueryState {
  const MonitoringAcceptedQueryState({this.identity, this.accepted, this.latestAttempt, this.error, this.pendingToken});
  final MonitoringQueryIdentity? identity;
  final MonitoringAcceptedQuery? accepted;
  final QueryOutcome? latestAttempt;
  final Object? error;
  final int? pendingToken;

  MonitoringAcceptedQueryPhase get phase {
    if (pendingToken != null) return MonitoringAcceptedQueryPhase.refreshing;
    if (error != null) {
      return accepted == null ? MonitoringAcceptedQueryPhase.failed : MonitoringAcceptedQueryPhase.stale;
    }
    if (accepted != null) return MonitoringAcceptedQueryPhase.fresh;
    return MonitoringAcceptedQueryPhase.empty;
  }

  MonitoringAcceptedQueryState begin(MonitoringQueryIdentity next, int token) => MonitoringAcceptedQueryState(
    identity: next,
    accepted: identity?.matches(next) == true ? accepted : null,
    pendingToken: token,
  );

  MonitoringAcceptedQueryState complete(int token, QueryOutcome outcome, DateTime completed) {
    if (pendingToken != token || identity == null) return this;
    final success = outcome.status == 'success' && outcome.error == null;
    return MonitoringAcceptedQueryState(
      identity: identity,
      accepted: success
          ? MonitoringAcceptedQuery(identity!, outcome, completed.toUtc())
          : monitoringMayRetainAcceptedQuery(outcome.error)
          ? accepted
          : null,
      latestAttempt: outcome,
      error: success ? null : outcome.error ?? const MonitoringFailure.local(MonitoringErrorCode.invalidResponse),
    );
  }

  MonitoringAcceptedQueryState fail(int token, Object failure) => pendingToken != token
      ? this
      : MonitoringAcceptedQueryState(
          identity: identity,
          accepted: monitoringMayRetainAcceptedQuery(failure) ? accepted : null,
          error: failure,
        );

  /// The workspace owns context reset/cancel and increments its existing read
  /// generation. Dropping this value prevents any late completion reappearing.
  static const empty = MonitoringAcceptedQueryState();
}

/// Only a known transient failure retains previous measurements. Unrecognized,
/// authorization, source/scope and schema failures conservatively clear them.
/// This is presentation policy, not a retry policy: no read is scheduled here.
bool monitoringMayRetainAcceptedQuery(Object? failure) => monitoringTransientFailure(failure);
