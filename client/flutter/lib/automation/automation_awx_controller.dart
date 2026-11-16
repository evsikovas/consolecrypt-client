import 'dart:async';

import 'package:consolecrypt/automation/automation_awx_models.dart';
import 'package:consolecrypt/automation/automation_awx_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_adapter.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart'
    show MonitoringSource;
import 'package:flutter/foundation.dart';

/// Dialog/workspace DATA owner over the existing single hub fan-out. Native
/// alone captures scope and owns credentials, issued registrations and tickets.
final class AwxController extends ChangeNotifier {
  AwxController({
    required this.profileId,
    required this.events,
    required this.isOriginalUiCurrent,
    required this.operationId,
    required this.gateway,
    DateTime Function()? now,
  }) : _epoch = events.currentScope.epoch,
       _now = now ?? DateTime.now {
    _subscription = events.watchRuntimeNotices().listen(
      (_) {
        if (!current) retire();
      },
      onError: (Object _) => retire(),
      onDone: retire,
    );
  }
  final String profileId;
  final RuntimeNoticeEvents events;
  final bool Function() isOriginalUiCurrent;
  final String Function() operationId;
  final AwxNativeGateway? gateway;
  final DateTime Function() _now;
  final int _epoch;
  late final StreamSubscription<Object?> _subscription;
  AwxNativeScope? _scope;
  AwxCatalog? catalog;
  AwxConnection? selected;
  AwxTemplatePage? templates;
  AwxTemplate? template;
  AwxLaunchParameters parameters = AwxLaunchParameters();
  AwxPreparedReview? review;
  AwxLaunchOutcome? outcome;
  AwxJob? job;
  final List<AwxEvent> _events = [];
  List<AwxEvent> get jobEvents => List.unmodifiable(_events);
  int eventsAfter = 0;
  bool eventsHaveNext = false;
  AwxFailureReason? failure;
  bool busy = false, retired = false, _disposed = false;
  bool launchUncertain = false;
  bool parametersReady = true;
  bool _launchBodyStarted = false;
  int _generation = 0;
  String? _pendingId;
  bool get current =>
      !_disposed &&
      !retired &&
      isOriginalUiCurrent() &&
      events.currentScope.epoch == _epoch &&
      !events.currentScope.streamUnavailable;
  bool get canPrepare =>
      parametersReady &&
      current &&
      !busy &&
      catalog?.readOnly == false &&
      selected?.readOnly == false &&
      template != null;
  bool get canLaunch =>
      parametersReady &&
      current &&
      !busy &&
      review != null &&
      review!.expiresUtc.isAfter(_now().toUtc()) &&
      selected?.id == review!.connectionId &&
      selected?.revision == review!.connectionRevision &&
      parameters.sameAs(review!.parameters);

  void _emit() {
    if (!_disposed) notifyListeners();
  }

  bool _same(int generation) => current && generation == _generation;
  Future<void> _bestEffort(Future<void> Function() action) async {
    try {
      await action();
    } catch (_) {
      /* Fixed local retirement, no raw error. */
    }
  }

  void _discard(AwxPreparedReview? value) {
    final scope = _scope, native = gateway;
    if (value != null && scope != null && native != null) {
      unawaited(
        _bestEffort(() => native.discardReview(scope, value.nativeTicket)),
      );
    }
  }

  void _invalidate({bool clearJob = false}) {
    _generation++;
    final id = _pendingId, scope = _scope, native = gateway;
    if (id != null && scope != null && native != null) {
      unawaited(_bestEffort(() => native.cancel(scope, id)));
    }
    if (_launchBodyStarted) {
      launchUncertain = true;
      outcome = const AwxLaunchUnknown();
    }
    _discard(review);
    review = null;
    failure = null;
    if (clearJob) {
      job = null;
      _events.clear();
      eventsAfter = 0;
      eventsHaveNext = false;
    }
    // busy remains until the registered original worker actually joins.
  }

  void cancelRead({bool deferNotification = false}) {
    _invalidate();
    // A cached workspace can hide while a router subtree is being built. The
    // generation/cancellation fence is synchronous; notify modal listeners only
    // after that build, so unrelated owned routes may remove themselves safely.
    if (deferNotification) {
      scheduleMicrotask(_emit);
    } else {
      _emit();
    }
  }

  void retire() {
    if (retired || _disposed) return;
    retired = true;
    _invalidate(clearJob: true);
    catalog = null;
    selected = null;
    templates = null;
    template = null;
    parameters = AwxLaunchParameters();
    parametersReady = true;
    outcome = null;
    _emit();
  }

  void selectConnection(AwxConnection value) {
    if (!current ||
        busy ||
        catalog == null ||
        !catalog!.connections.any((c) => identical(c, value))) {
      return;
    }
    if (selected?.sameRevision(value) == true) return;
    _invalidate(clearJob: true);
    selected = value;
    templates = null;
    template = null;
    parameters = AwxLaunchParameters();
    parametersReady = true;
    outcome = null;
    launchUncertain = false;
    _emit();
  }

  void selectTemplate(AwxTemplate value) {
    if (!current ||
        busy ||
        templates == null ||
        !templates!.templates.any((t) => identical(t, value))) {
      return;
    }
    _invalidate(clearJob: true);
    template = value;
    parameters = AwxLaunchParameters();
    parametersReady = true;
    if (!launchUncertain) outcome = null;
    _emit();
  }

  void editParameters(AwxLaunchParameters value) {
    if (!current || busy || (parametersReady && parameters.sameAs(value))) {
      return;
    }
    _invalidate();
    parameters = value;
    parametersReady = true;
    if (!launchUncertain) outcome = null;
    _emit();
  }

  /// Called on every form edit, including malformed JSON. An invalid draft
  /// cannot leave an older prepared ticket or silently reuse old parameters.
  void invalidateParameterDraft() {
    if (!current || busy) return;
    _invalidate();
    parametersReady = false;
    _emit();
  }

  AwxFailureReason _reason(Object error) => error is AwxFailure
      ? error.reason
      : error is AwxModelException
      ? AwxFailureReason.response
      : AwxFailureReason.unknown;
  Future<AwxNativeScope> _capture(int generation) async {
    final native = gateway;
    if (native == null) throw const AwxFailure(AwxFailureReason.unavailable);
    final existing = _scope;
    if (existing != null) return existing;
    if (!_same(generation)) throw const AwxFailure(AwxFailureReason.changed);
    final scope = await native.captureScope();
    if (!_same(generation) || scope.profileId != profileId) {
      throw const AwxFailure(AwxFailureReason.changed);
    }
    _scope = scope;
    return scope;
  }

  Future<void> _metadata(
    Future<AwxCatalog> Function(AwxNativeGateway, AwxNativeScope) action,
  ) async {
    if (!current || busy) return;
    final generation = ++_generation;
    busy = true;
    failure = null;
    _emit();
    try {
      final scope = await _capture(generation);
      if (!_same(generation)) return;
      final value = await action(gateway!, scope);
      if (!_same(generation)) return;
      final old = selected;
      catalog = value;
      final matches = value.connections.where((c) => c.id == old?.id).toList();
      final next = matches.isEmpty ? null : matches.single;
      if (old != null && (next == null || !old.sameRevision(next))) {
        _discard(review);
        review = null;
        templates = null;
        template = null;
        parameters = AwxLaunchParameters();
        parametersReady = true;
        outcome = null;
        job = null;
        _events.clear();
        eventsAfter = 0;
        eventsHaveNext = false;
      }
      selected = next;
    } catch (error) {
      if (_same(generation)) failure = _reason(error);
    } finally {
      busy = false;
      _emit();
    }
  }

  Future<void> loadCatalog() =>
      _metadata((native, scope) => native.catalog(scope));
  Future<void> saveConnection(
    MonitoringSource config, {
    int? expectedRevision,
  }) async {
    if (!current ||
        busy ||
        catalog == null ||
        catalog!.readOnly ||
        catalog!.revision == null) {
      return;
    }
    _discard(review);
    review = null;
    try {
      validateAwxConfiguration(config, create: expectedRevision == null);
    } catch (_) {
      failure = AwxFailureReason.invalidInput;
      _emit();
      return;
    }
    final revision = catalog!.revision!;
    await _metadata(
      (native, scope) => native.saveConnection(
        scope,
        expectedCatalogRevision: revision,
        expectedRevision: expectedRevision,
        configuration: config,
      ),
    );
  }

  Future<void> deleteConnection(AwxConnection original) async {
    if (!current ||
        busy ||
        catalog == null ||
        catalog!.readOnly ||
        catalog!.revision == null ||
        original.readOnly ||
        !catalog!.connections.any((c) => identical(c, original))) {
      return;
    }
    _discard(review);
    review = null;
    final revision = catalog!.revision!;
    await _metadata(
      (native, scope) => native.deleteConnection(
        scope,
        expectedCatalogRevision: revision,
        connection: original,
      ),
    );
  }

  Future<void> _remote<T>(
    AwxRegisteredOperation<T> Function(AwxNativeGateway, AwxNativeScope, String)
    begin,
    void Function(T) accept, {
    bool launch = false,
    void Function(T)? stale,
  }) async {
    if (!current || busy) return;
    final generation = ++_generation;
    busy = true;
    failure = null;
    _emit();
    AwxRegisteredOperation<T>? invocation;
    AwxNativeScope? original;
    String? id;
    Future<Object?>? observed;
    var staleDelivered = false;
    try {
      original = await _capture(generation);
      if (!_same(generation)) return;
      id = operationId();
      // Binding is synchronous BEFORE registration can suspend. Invalidation
      // cancels this original pair even while Native registration is pending.
      _pendingId = id;
      invocation = begin(gateway!, original, id);
      observed = invocation.result.then<Object?>((v) {
        if (!_same(generation) && stale != null) {
          staleDelivered = true;
          stale(v);
        }
        return v;
      }, onError: (Object e, StackTrace _) => _AwxCaught(e));
      await invocation.registered;
      if (!_same(generation)) {
        await _bestEffort(() => gateway!.cancel(original!, id!));
        return;
      }
      _launchBodyStarted = launch;
      invocation.startBody();
      final value = await observed;
      if (value is _AwxCaught) throw AwxFailure(_reason(value.error));
      if (!_same(generation)) {
        if (!staleDelivered) stale?.call(value as T);
        return;
      }
      accept(value as T);
    } catch (error) {
      if (original != null && id != null) {
        await _bestEffort(() => gateway!.cancel(original!, id!));
      }
      if (_same(generation)) {
        failure = _reason(error);
        if (launch &&
            _launchBodyStarted &&
            (error is! AwxFailure ||
                const {
                  AwxFailureReason.transport,
                  AwxFailureReason.response,
                  AwxFailureReason.unknown,
                  AwxFailureReason.canceled,
                }.contains(error.reason))) {
          launchUncertain = true;
          outcome = const AwxLaunchUnknown();
        }
      }
    } finally {
      if (invocation != null) {
        // Cancellation/result completion does not prove worker join. Keep the
        // original binding until Native's registered operation actually joins.
        try {
          await invocation.joined;
        } catch (_) {
          /* Safe failure above. */
        }
      }
      if (_pendingId == id) _pendingId = null;
      _launchBodyStarted = false;
      busy = false;
      _emit();
    }
  }

  Future<void> loadTemplates({int page = 1, String search = ''}) async {
    final connection = selected;
    if (!current ||
        busy ||
        connection == null ||
        connection.readOnly ||
        page < 1 ||
        page > 1000 ||
        search.length > 256 ||
        search.runes.any((r) => r < 32 || r == 127)) {
      return;
    }
    _discard(review);
    review = null;
    template = null;
    await _remote<AwxTemplatePage>(
      (n, s, id) => n.registerTemplates(s, id, connection, page, search),
      (value) {
        templates = value;
      },
    );
  }

  Future<void> prepareLaunch() async {
    if (!canPrepare) return;
    final connection = selected!, chosen = template!, exact = parameters;
    _discard(review);
    review = null;
    await _remote<AwxPreparedReview>(
      (n, s, id) => n.registerPrepare(s, id, connection, chosen.id, exact),
      (value) {
        if (value.connectionId != connection.id ||
            value.connectionRevision != connection.revision ||
            value.template.id != chosen.id ||
            !value.parameters.sameAs(exact)) {
          _discard(value);
          throw const AwxFailure(AwxFailureReason.response);
        }
        review = value;
      },
      stale: _discard,
    );
  }

  Future<void> launchReviewed() async {
    if (!canLaunch) {
      final expired = review;
      if (expired != null && !expired.expiresUtc.isAfter(_now().toUtc())) {
        _discard(expired);
        review = null;
        failure = AwxFailureReason.expired;
        _emit();
      }
      return;
    }
    final exact = review!;
    review =
        null; // One explicit action consumes local proof BEFORE suspension.
    await _remote<AwxLaunchOutcome>(
      (n, s, id) => n.registerLaunch(s, id, exact.nativeTicket),
      (value) {
        outcome = value;
        launchUncertain = value is AwxLaunchUnknown;
        if (value is AwxLaunchStarted) {
          job = value.job;
          _events.clear();
          eventsAfter = 0;
        }
      },
      launch: true,
    );
    _discard(exact);
  }

  Future<void> refreshJob() async {
    final connection = selected, existing = job;
    if (connection == null || existing == null) return;
    await _remote<AwxJob>(
      (n, s, id) => n.registerJob(s, id, connection, existing.id),
      (value) {
        if (value.id != existing.id) {
          throw const AwxFailure(AwxFailureReason.response);
        }
        job = value;
      },
    );
  }

  Future<void> loadEvents() async {
    final connection = selected, existing = job;
    if (connection == null || existing == null) return;
    final after = eventsAfter;
    await _remote<AwxEventPage>(
      (n, s, id) => n.registerEvents(s, id, connection, existing.id, after),
      (value) {
        if (value.events.any((e) => e.counter <= after) ||
            value.after < after) {
          throw const AwxFailure(AwxFailureReason.response);
        }
        _events.addAll(value.events);
        if (_events.length > 500) _events.removeRange(0, _events.length - 500);
        eventsAfter = value.after;
        eventsHaveNext = value.hasNext;
      },
    );
  }

  @override
  void dispose() {
    if (_disposed) return;
    retire();
    _disposed = true;
    unawaited(_subscription.cancel());
    super.dispose();
  }
}

final class _AwxCaught {
  const _AwxCaught(this.error);
  final Object error;
}
