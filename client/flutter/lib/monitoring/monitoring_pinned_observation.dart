import 'dart:async';
import 'dart:math' as math;

import 'package:consolecrypt/monitoring/monitoring_failure_policy.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_observation_plan.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';

export 'package:consolecrypt/monitoring/monitoring_observation_plan.dart';

enum MonitoringPinPhase { idle, checking, reading, waiting, backoff, stopped }

enum MonitoringPinStop {
  manual,
  autoDisabled,
  scopeChanged,
  sourceChanged,
  appPaused,
  appClosed,
  retryLimit,
  invalidRequest,
  rejected,
}

enum MonitoringPinDisposition { retry, stop, scopeChanged }

enum MonitoringPinLifecycle { resumed, inactive, hidden, paused, detached, exit }

abstract interface class MonitoringPinTransport {
  Future<MonitoringSource?> loadSource(String sourceId);
  Future<QueryOutcome> readCurrent(MonitoringSource source, String expression);
  void cancel();
}

/// Optional generalized transport. Old D1 custom-instant transports retain
/// their existing API; Native record/range targets never use that fallback.
abstract interface class MonitoringGeneralPinTransport implements MonitoringPinTransport {
  Future<MonitoringObservationContext?> loadContext(MonitoringPinPlan plan);
  Future<MonitoringObservationValue> readTarget(
    MonitoringPinPlan plan,
    MonitoringObservationContext context,
    MonitoringTimeWindow? window,
  );
}

abstract interface class MonitoringPinTimer {
  void cancel();
}

final class _SystemPinTimer implements MonitoringPinTimer {
  _SystemPinTimer(Duration delay, void Function() callback) : timer = Timer(delay, callback);
  final Timer timer;
  @override
  void cancel() => timer.cancel();
}

/// Memory-only snapshot. No persistence/export or secret-resolution path.
final class MonitoringPinState {
  const MonitoringPinState({
    this.plan,
    this.visibility = MonitoringObservationVisibility.pinned,
    this.value,
    this.phase = MonitoringPinPhase.idle,
    this.outcome,
    this.lastSuccessUtc,
    this.lastAttemptUtc,
    this.nextDueUtc,
    this.error,
    this.failures = 0,
    this.stopReason,
  });
  final MonitoringPinPlan? plan;
  final MonitoringObservationVisibility visibility;
  final MonitoringObservationValue? value;
  bool get hasAcceptedResult => value != null || outcome != null;
  final MonitoringPinPhase phase;
  final QueryOutcome? outcome;
  final DateTime? lastSuccessUtc, lastAttemptUtc, nextDueUtc;
  final Object? error;
  final int failures;
  final MonitoringPinStop? stopReason;
  bool get active => plan != null && phase != MonitoringPinPhase.stopped;
  bool get busy => phase == MonitoringPinPhase.checking || phase == MonitoringPinPhase.reading;
}

/// One explicit observation subscription in one unlocked profile. A separate
/// service instance prevents ordinary hidden-workspace cancel() from killing it.
/// Completion schedules the next tick: no overlap, catch-up loop or app restart.
final class MonitoringPinnedObservation {
  MonitoringPinnedObservation({
    required this.createTransport,
    required this.isCurrent,
    DateTime Function()? now,
    MonitoringPinTimer Function(Duration, void Function())? schedule,
    MonitoringPinDisposition Function(Object)? classify,
  }) : now = now ?? DateTime.now,
       schedule = schedule ?? _SystemPinTimer.new,
       classify = classify ?? classifyFailure;
  final MonitoringPinTransport Function() createTransport;
  final bool Function() isCurrent;
  final DateTime Function() now;
  final MonitoringPinTimer Function(Duration, void Function()) schedule;
  final MonitoringPinDisposition Function(Object) classify;
  final _states = StreamController<MonitoringPinState>.broadcast();
  Stream<MonitoringPinState> get changes => _states.stream;
  MonitoringPinState _state = const MonitoringPinState();
  MonitoringPinState get state => _state;
  MonitoringPinTransport? _transport;
  MonitoringPinTimer? _timer;
  int _generation = 0;
  bool _disposed = false;
  bool _foregroundActive = false;
  bool _appAllowsForeground = true;

  bool _current(int token) => !_disposed && token == _generation && state.active && isCurrent();
  void _emit(MonitoringPinState value) {
    if (_disposed) return;
    _state = value;
    _states.add(value);
  }

  bool pin(MonitoringPinPlan plan) {
    if (_disposed) return false;
    if (!isCurrent()) {
      stop(MonitoringPinStop.scopeChanged);
      return false;
    }
    try {
      plan.validate();
    } catch (error) {
      stop(MonitoringPinStop.invalidRequest);
      _emit(
        MonitoringPinState(
          phase: MonitoringPinPhase.stopped,
          error: error,
          stopReason: MonitoringPinStop.invalidRequest,
        ),
      );
      return false;
    }
    stop(MonitoringPinStop.manual);
    try {
      _transport = createTransport();
    } catch (error) {
      _emit(
        MonitoringPinState(phase: MonitoringPinPhase.stopped, error: error, stopReason: MonitoringPinStop.rejected),
      );
      return false;
    }
    _emit(MonitoringPinState(plan: plan, phase: MonitoringPinPhase.waiting));
    unawaited(_poll(_generation));
    return true;
  }

  /// Page visibility alone never arms a request. Desktop window focus changes
  /// are handled by lifecycle(), separately from switching the workspace route.
  void setForegroundActive(bool visible) {
    _foregroundActive = visible;
    if (!visible && state.plan != null && state.visibility == MonitoringObservationVisibility.foregroundOnly) {
      stop(MonitoringPinStop.manual);
    }
  }

  /// Called only after an explicit successful manual Read whose Auto option was
  /// already visible/enabled. Seed the exact response; do not issue a duplicate.
  bool observeForeground(MonitoringPinPlan plan, MonitoringObservationValue seed) =>
      _acceptSeed(plan, seed, MonitoringObservationVisibility.foregroundOnly);

  /// Explicit pinning can use the last accepted manual result even with Auto
  /// disabled. It still cannot implicitly replace another pinned plan.
  bool pinAccepted(MonitoringPinPlan plan, MonitoringObservationValue seed) =>
      _acceptSeed(plan, seed, MonitoringObservationVisibility.pinned);

  void cancelForeground() {
    if (state.plan != null && state.visibility == MonitoringObservationVisibility.foregroundOnly) {
      stop(MonitoringPinStop.manual);
    }
  }

  /// Explicit Auto OFF preserves only the most recent accepted readonly value.
  /// It revokes timer/transport authority and denies late completion/refresh.
  /// Permanent/schema/source failures never enter this retention transition.
  void disableForeground() {
    if (_disposed || state.visibility != MonitoringObservationVisibility.foregroundOnly || state.plan == null) return;
    if (!isCurrent()) {
      stop(MonitoringPinStop.scopeChanged);
      return;
    }
    final old = state;
    if (!old.hasAcceptedResult) {
      stop(MonitoringPinStop.manual);
      return;
    }
    _generation++;
    _timer?.cancel();
    _timer = null;
    _transport?.cancel();
    _transport = null;
    _emit(
      MonitoringPinState(
        plan: old.plan,
        visibility: old.visibility,
        value: old.value,
        outcome: old.outcome,
        phase: MonitoringPinPhase.stopped,
        lastSuccessUtc: old.lastSuccessUtc,
        lastAttemptUtc: old.lastAttemptUtc,
        error: old.error,
        failures: old.failures,
        stopReason: MonitoringPinStop.autoDisabled,
      ),
    );
  }

  bool _acceptSeed(
    MonitoringPinPlan plan,
    MonitoringObservationValue seed,
    MonitoringObservationVisibility visibility,
  ) {
    if (_disposed) return false;
    if (!isCurrent()) {
      stop(MonitoringPinStop.scopeChanged);
      return false;
    }
    if (!_foregroundActive ||
        !_appAllowsForeground ||
        state.active && state.visibility == MonitoringObservationVisibility.pinned) {
      return false;
    }
    try {
      plan.validate();
      if (seed.failure != null || !monitoringObservationValueMatchesPlan(plan, seed)) {
        return false;
      }
    } on Object {
      return false;
    }
    stop(MonitoringPinStop.manual);
    try {
      _transport = createTransport();
    } on Object {
      return false;
    }
    final completed = now().toUtc();
    _emit(
      MonitoringPinState(
        plan: plan,
        visibility: visibility,
        value: seed,
        outcome: seed.queryOutcome,
        phase: MonitoringPinPhase.waiting,
        lastSuccessUtc: completed,
        lastAttemptUtc: completed,
        nextDueUtc: completed.add(plan.interval.duration),
      ),
    );
    _schedule(_generation, plan.interval.duration);
    return true;
  }

  /// Promotion retains the service, in-flight request, accepted window and timer.
  bool promoteForeground() {
    if (_disposed ||
        !state.active ||
        !isCurrent() ||
        state.visibility != MonitoringObservationVisibility.foregroundOnly) {
      return false;
    }
    _changeVisibility(MonitoringObservationVisibility.pinned);
    return true;
  }

  void unpin() {
    if (!state.active || state.visibility != MonitoringObservationVisibility.pinned) {
      return;
    }
    if (!_foregroundActive || !_appAllowsForeground) {
      stop(MonitoringPinStop.manual);
      return;
    }
    _changeVisibility(MonitoringObservationVisibility.foregroundOnly);
  }

  void _changeVisibility(MonitoringObservationVisibility visibility) {
    final old = state;
    _emit(
      MonitoringPinState(
        plan: old.plan,
        visibility: visibility,
        value: old.value,
        outcome: old.outcome,
        phase: old.phase,
        lastSuccessUtc: old.lastSuccessUtc,
        lastAttemptUtc: old.lastAttemptUtc,
        nextDueUtc: old.nextDueUtc,
        error: old.error,
        failures: old.failures,
        stopReason: old.stopReason,
      ),
    );
  }

  /// Source/binding/saved-panel acknowledgement shares the same catalog owner.
  /// No additional request, resolver or approval is created here.
  void contextChanged(MonitoringObservationContext? context, {required bool readOnly}) {
    final plan = state.plan;
    if (plan != null && (readOnly || context == null || !context.matches(plan))) {
      stop(MonitoringPinStop.sourceChanged);
    }
  }

  /// An authoritative catalog acknowledgement can invalidate immediately;
  /// hidden branches still recheck independently before every scheduled read.
  void sourcesChanged(List<MonitoringSource> sources, {required bool readOnly}) {
    final plan = state.plan;
    if (plan == null) return;
    final current = sources.where((source) => source.id == plan.source.id).firstOrNull;
    if (readOnly || current == null || current.readOnly || current.fingerprint != plan.source.fingerprint) {
      stop(MonitoringPinStop.sourceChanged);
    }
  }

  bool refresh() {
    if (_disposed || !state.active || state.busy) return false;
    if (!isCurrent()) {
      stop(MonitoringPinStop.scopeChanged);
      return false;
    }
    _timer?.cancel();
    _timer = null;
    unawaited(_poll(_generation));
    return true;
  }

  void lifecycle(MonitoringPinLifecycle value) {
    switch (value) {
      case MonitoringPinLifecycle.paused:
        _appAllowsForeground = false;
        stop(MonitoringPinStop.appPaused);
      case MonitoringPinLifecycle.detached || MonitoringPinLifecycle.exit:
        _appAllowsForeground = false;
        stop(MonitoringPinStop.appClosed);
      case MonitoringPinLifecycle.hidden:
        _appAllowsForeground = false;
        cancelForeground();
      case MonitoringPinLifecycle.resumed:
        _appAllowsForeground = true;
      case MonitoringPinLifecycle.inactive:
        break;
    }
  }

  void stop(MonitoringPinStop reason, {Object? error}) {
    if (_disposed) return;
    _generation++;
    _timer?.cancel();
    _timer = null;
    _transport?.cancel();
    _transport = null;
    _emit(
      MonitoringPinState(
        visibility: state.visibility,
        phase: MonitoringPinPhase.stopped,
        stopReason: reason,
        error: error,
      ),
    );
  }

  void dispose() {
    if (_disposed) return;
    stop(MonitoringPinStop.scopeChanged);
    _disposed = true;
    unawaited(_states.close());
  }

  Future<void> _poll(int token) async {
    if (!_current(token)) {
      if (!_disposed && token == _generation) {
        stop(MonitoringPinStop.scopeChanged);
      }
      return;
    }
    if (state.busy) return;
    final plan = state.plan!, transport = _transport!, old = state, attempt = now().toUtc();
    _emit(
      MonitoringPinState(
        plan: plan,
        visibility: state.visibility,
        value: old.value,
        phase: MonitoringPinPhase.checking,
        outcome: old.outcome,
        lastSuccessUtc: old.lastSuccessUtc,
        lastAttemptUtc: attempt,
        failures: old.failures,
        error: old.error,
      ),
    );
    try {
      // One planned window per tick, retained with the accepted value. Fresh
      // pre/post catalog fences may take time but cannot retarget this request.
      final window = plan.windowAt(attempt);
      Future<MonitoringObservationContext?> context() async {
        if (transport is MonitoringGeneralPinTransport) {
          return transport.loadContext(plan);
        }
        // Existing D1 transports are deliberately instant-only.
        if (plan.target is! MonitoringCustomInstantTarget) {
          throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedResult);
        }
        final source = await transport.loadSource(plan.source.id);
        return source == null ? null : MonitoringObservationContext(source: source);
      }

      final fresh = await context();
      if (!_current(token)) return;
      if (fresh == null || !fresh.matches(plan)) {
        stop(MonitoringPinStop.sourceChanged);
        return;
      }
      _emit(
        MonitoringPinState(
          plan: plan,
          visibility: state.visibility,
          phase: MonitoringPinPhase.reading,
          value: old.value,
          outcome: old.outcome,
          lastSuccessUtc: old.lastSuccessUtc,
          lastAttemptUtc: attempt,
          failures: old.failures,
          error: old.error,
        ),
      );
      final value = transport is MonitoringGeneralPinTransport
          ? await transport.readTarget(plan, fresh, window)
          : MonitoringObservedQuery(await transport.readCurrent(fresh.source, plan.expression), null);
      if (!_current(token)) return;
      if (!monitoringObservationValueMatchesPlan(plan, value)) {
        throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
      }
      if (value.failure case final error?) throw error;
      final after = await context();
      if (!_current(token)) return;
      if (after == null || !after.matches(plan)) {
        stop(MonitoringPinStop.sourceChanged);
        return;
      }
      final completed = now().toUtc(), next = completed.add(plan.interval.duration);
      _emit(
        MonitoringPinState(
          plan: plan,
          visibility: state.visibility,
          phase: MonitoringPinPhase.waiting,
          value: value,
          outcome: value.queryOutcome,
          lastSuccessUtc: completed,
          lastAttemptUtc: attempt,
          nextDueUtc: next,
        ),
      );
      _schedule(token, plan.interval.duration);
    } catch (error) {
      if (!_current(token)) return;
      final disposition = classify(error);
      if (disposition == MonitoringPinDisposition.scopeChanged) {
        stop(MonitoringPinStop.scopeChanged);
        return;
      }
      if (disposition == MonitoringPinDisposition.stop) {
        final changed =
            error is MonitoringFailure &&
            (error.kind == MonitoringErrorCode.sourceChanged || error.kind == MonitoringErrorCode.bindingChanged);
        stop(changed ? MonitoringPinStop.sourceChanged : MonitoringPinStop.rejected, error: error);
        return;
      }
      final failures = old.failures + 1;
      if (failures >= 3) {
        _timer?.cancel();
        _timer = null;
        _transport?.cancel();
        _transport = null;
        _generation++;
        _emit(
          MonitoringPinState(
            plan: plan,
            visibility: state.visibility,
            value: old.value,
            phase: MonitoringPinPhase.stopped,
            outcome: old.outcome,
            lastSuccessUtc: old.lastSuccessUtc,
            lastAttemptUtc: attempt,
            error: error,
            failures: failures,
            stopReason: MonitoringPinStop.retryLimit,
          ),
        );
        return;
      }
      final delay = Duration(seconds: math.min(300, plan.interval.duration.inSeconds * (1 << (failures - 1))));
      _emit(
        MonitoringPinState(
          plan: plan,
          visibility: state.visibility,
          value: old.value,
          phase: MonitoringPinPhase.backoff,
          outcome: old.outcome,
          lastSuccessUtc: old.lastSuccessUtc,
          lastAttemptUtc: attempt,
          error: error,
          failures: failures,
          nextDueUtc: now().toUtc().add(delay),
        ),
      );
      _schedule(token, delay);
    } finally {
      if (!_disposed && token == _generation && !isCurrent()) {
        stop(MonitoringPinStop.scopeChanged);
      }
    }
  }

  void _schedule(int token, Duration delay) {
    if (!_current(token)) return;
    _timer?.cancel();
    _timer = schedule(delay, () {
      _timer = null;
      unawaited(_poll(token));
    });
  }

  static MonitoringPinDisposition classifyFailure(Object error) =>
      monitoringTransientFailure(error) ? MonitoringPinDisposition.retry : MonitoringPinDisposition.stop;
}
