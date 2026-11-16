import 'dart:async';

import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:flutter_test/flutter_test.dart';

void check(bool value) {
  if (!value) throw StateError('Observation expectation failed');
}

Future<void> flush() async {
  for (var i = 0; i < 30; i++) {
    await Future<void>.value();
  }
}

class Ticket implements MonitoringPinTimer {
  Ticket(this.due, this.callback);
  final DateTime due;
  final void Function() callback;
  bool cancelled = false, fired = false;
  @override
  void cancel() {
    cancelled = true;
  }
}

class Clock {
  DateTime now = DateTime.utc(2026, 11, 12);
  final tasks = <Ticket>[];
  MonitoringPinTimer schedule(Duration delay, void Function() callback) {
    final ticket = Ticket(now.add(delay), callback);
    tasks.add(ticket);
    return ticket;
  }

  int get pending => tasks.where((t) => !t.cancelled && !t.fired).length;
  Future<void> advance(int seconds) async {
    now = now.add(Duration(seconds: seconds));
    for (final ticket in tasks.where((t) => !t.cancelled && !t.fired && !t.due.isAfter(now)).toList()) {
      ticket.fired = true;
      ticket.callback();
      await flush();
    }
  }
}

const source = MonitoringSource(
  id: '11111111-1111-4111-8111-111111111111',
  name: 'Owned source',
  endpoint: 'http://metrics.invalid',
  revision: 1,
);
const outcome = QueryOutcome(
  status: 'success',
  httpStatus: 200,
  partial: true,
  data: QueryData(
    'matrix',
    series: [
      MonitoringSeries(
        {'job': 'owned'},
        [MonitoringSample(1000, SampleKind.finite, 0), MonitoringSample(1015, SampleKind.nan)],
      ),
    ],
  ),
);
MonitoringPinPlan plan({bool rolling = false}) => MonitoringPinPlan.forTarget(
  source: source,
  target: MonitoringCustomRangeTarget(
    expression: 'rate(owned_total[5m])',
    windowPolicy: rolling
        ? const MonitoringRollingObservationWindow(preset: MonitoringTimePreset.hour1, stepSeconds: 15)
        : MonitoringFixedObservationWindow(
            MonitoringTimeWindow.preset(MonitoringTimePreset.hour1, DateTime.utc(2026, 11, 12)),
          ),
  ),
);

class Transport implements MonitoringGeneralPinTransport {
  MonitoringObservationContext? context = const MonitoringObservationContext(source: source);
  int reads = 0, loads = 0, cancels = 0, legacy = 0;
  Exception? error;
  Completer<MonitoringObservationValue>? pending;
  bool changeAfterRead = false;
  MonitoringObservationValue? Function(MonitoringPinPlan, MonitoringTimeWindow?)? result;
  final windows = <MonitoringTimeWindow?>[];
  @override
  Future<MonitoringObservationContext?> loadContext(MonitoringPinPlan plan) async {
    loads++;
    return context;
  }

  @override
  Future<MonitoringSource?> loadSource(String id) async => context?.source;
  @override
  Future<QueryOutcome> readCurrent(MonitoringSource source, String expression) async {
    legacy++;
    return outcome;
  }

  @override
  Future<MonitoringObservationValue> readTarget(
    MonitoringPinPlan plan,
    MonitoringObservationContext context,
    MonitoringTimeWindow? window,
  ) async {
    reads++;
    windows.add(window);
    if (error case final e?) throw e;
    if (changeAfterRead) this.context = null;
    return pending?.future ?? result?.call(plan, window) ?? MonitoringObservedQuery(outcome, window);
  }

  @override
  void cancel() {
    cancels++;
  }
}

class LegacyTransport implements MonitoringPinTransport {
  int reads = 0;
  @override
  Future<MonitoringSource?> loadSource(String id) async => source;
  @override
  Future<QueryOutcome> readCurrent(MonitoringSource source, String expression) async {
    reads++;
    return outcome;
  }

  @override
  void cancel() {}
}

class Fixture {
  final clock = Clock(), transport = Transport();
  bool current = true;
  int created = 0;
  late final owner = MonitoringPinnedObservation(
    createTransport: () {
      created++;
      return transport;
    },
    isCurrent: () => current,
    now: () => clock.now,
    schedule: clock.schedule,
  );
  bool seed(MonitoringPinPlan plan) =>
      owner.observeForeground(plan, MonitoringObservedQuery(outcome, plan.windowAt(clock.now)));
}

void main() {
  test('page open/visibility alone creates no service and sends no read', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    await f.clock.advance(300);
    check(f.created == 0 && f.transport.reads == 0 && f.clock.pending == 0);
    f.owner.dispose();
  });
  test('explicit accepted Auto seed starts 15 seconds after completion without duplicate request', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    check(f.seed(plan()));
    check(f.created == 1 && f.transport.reads == 0 && f.clock.pending == 1);
    await f.clock.advance(14);
    check(f.transport.reads == 0);
    await f.clock.advance(1);
    check(f.transport.reads == 1 && f.transport.loads == 2 && f.transport.legacy == 0);
    check(f.owner.state.visibility == MonitoringObservationVisibility.foregroundOnly);
    f.owner.dispose();
  });
  test('hidden workspace stops foreground but never arms a replacement when shown', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.owner.setForegroundActive(false);
    check(!f.owner.state.active && f.clock.pending == 0 && f.transport.cancels == 1);
    f.owner.setForegroundActive(true);
    await f.clock.advance(300);
    check(f.transport.reads == 0);
    f.owner.dispose();
  });
  test('promotion reuses one service/timer and stays active while workspace hidden', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    final due = f.owner.state.nextDueUtc;
    check(f.owner.promoteForeground());
    f.owner.setForegroundActive(false);
    check(f.created == 1 && f.clock.pending == 1 && f.owner.state.nextDueUtc == due);
    await f.clock.advance(15);
    check(f.transport.reads == 1 && f.owner.state.active);
    f.owner.dispose();
  });
  test('another manual accepted read cannot replace a pinned plan or timer', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.owner.promoteForeground();
    final original = f.owner.state.plan, due = f.owner.state.nextDueUtc;
    check(!f.seed(plan(rolling: true)) && identical(original, f.owner.state.plan));
    check(f.created == 1 && f.clock.pending == 1 && f.owner.state.nextDueUtc == due);
    f.owner.dispose();
  });
  test('promotion while in flight survives completion without service restart', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan();
    f.seed(p);
    f.transport.pending = Completer<MonitoringObservationValue>();
    await f.clock.advance(15);
    check(f.owner.state.busy && f.owner.promoteForeground());
    f.owner.setForegroundActive(false);
    f.transport.pending!.complete(MonitoringObservedQuery(outcome, p.windowAt(f.clock.now)));
    await flush();
    check(f.owner.state.visibility == MonitoringObservationVisibility.pinned && f.owner.state.active);
    check(f.created == 1 && f.clock.pending == 1);
    f.owner.dispose();
  });
  test('fixed range remains exact across scheduled ticks', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan();
    f.seed(p);
    await f.clock.advance(15);
    await f.clock.advance(15);
    check(f.transport.windows.length == 2 && f.transport.windows.every((w) => w == p.windowAt(f.clock.now)));
    check(f.owner.state.value!.window == f.transport.windows.last);
    f.owner.dispose();
  });
  test('explicit rolling result carries actual window; transient failure retains old window and NaN/zero', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan(rolling: true);
    f.seed(p);
    final accepted = f.owner.state.value!, success = f.owner.state.lastSuccessUtc;
    f.transport.error = const MonitoringFailure.local(MonitoringErrorCode.timeout);
    await f.clock.advance(15);
    check(identical(f.owner.state.value, accepted) && f.owner.state.lastSuccessUtc == success);
    check(f.owner.state.value!.window != p.windowAt(f.clock.now) && f.owner.state.phase == MonitoringPinPhase.backoff);
    check(f.owner.state.outcome!.partial && f.owner.state.outcome!.data!.series.single.samples.first.value == 0);
    check(f.owner.state.outcome!.data!.series.single.samples.last.kind == SampleKind.nan);
    f.owner.dispose();
  });
  test('no overlap and next tick begins after delayed completion', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan();
    f.seed(p);
    f.transport.pending = Completer<MonitoringObservationValue>();
    await f.clock.advance(15);
    await f.clock.advance(200);
    check(f.transport.reads == 1 && f.clock.pending == 0 && !f.owner.refresh());
    f.transport.pending!.complete(MonitoringObservedQuery(outcome, p.windowAt(f.clock.now)));
    await flush();
    check(f.owner.state.nextDueUtc == f.clock.now.add(const Duration(seconds: 15)) && f.clock.pending == 1);
    f.owner.dispose();
  });
  test('catalog removal before tick sends no hidden read', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.transport.context = null;
    await f.clock.advance(15);
    check(f.transport.reads == 0 && f.owner.state.stopReason == MonitoringPinStop.sourceChanged);
    check(!f.owner.state.hasAcceptedResult && f.clock.pending == 0);
    f.owner.dispose();
  });
  test('post-read catalog change denies result and clears accepted values', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.transport.changeAfterRead = true;
    await f.clock.advance(15);
    check(f.transport.reads == 1 && f.owner.state.stopReason == MonitoringPinStop.sourceChanged);
    check(!f.owner.state.hasAcceptedResult);
    f.owner.dispose();
  });
  test('lock mid-read clears data and rejects late completion', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan();
    f.seed(p);
    f.transport.pending = Completer<MonitoringObservationValue>();
    await f.clock.advance(15);
    f.current = false;
    f.owner.stop(MonitoringPinStop.scopeChanged);
    f.transport.pending!.complete(MonitoringObservedQuery(outcome, p.windowAt(f.clock.now)));
    await flush();
    check(!f.owner.state.hasAcceptedResult && !f.owner.state.active && f.clock.pending == 0);
    f.owner.dispose();
  });
  test('desktop inactive keeps pin; paused stops it; resume cannot restart', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.owner.promoteForeground();
    f.owner.lifecycle(MonitoringPinLifecycle.inactive);
    await f.clock.advance(15);
    check(f.transport.reads == 1);
    f.owner.lifecycle(MonitoringPinLifecycle.paused);
    f.owner.lifecycle(MonitoringPinLifecycle.resumed);
    await f.clock.advance(300);
    check(f.transport.reads == 1 && !f.owner.state.active);
    f.owner.dispose();
  });
  test('permanent unknown schema error cannot retry just because HTTP503', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.transport.error = const MonitoringFailure('unknown_native', 'private', httpStatus: 503);
    await f.clock.advance(15);
    check(f.owner.state.stopReason == MonitoringPinStop.rejected && f.clock.pending == 0);
    check(!f.owner.state.hasAcceptedResult);
    f.owner.dispose();
  });
  test('three retryable failures stop without overlap and retain actual last accepted snapshot', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    final value = f.owner.state.value;
    f.transport.error = const MonitoringFailure.local(MonitoringErrorCode.timeout);
    await f.clock.advance(15);
    await f.clock.advance(15);
    await f.clock.advance(30);
    check(f.transport.reads == 3 && f.owner.state.stopReason == MonitoringPinStop.retryLimit);
    check(identical(value, f.owner.state.value) && f.clock.pending == 0);
    f.owner.dispose();
  });
  test('D1 instant transport API remains immediately readable with explicit old cadence', () async {
    final f = Fixture();
    check(
      f.owner.pin(const MonitoringPinPlan(source: source, expression: 'up', interval: MonitoringPinInterval.seconds30)),
    );
    await flush();
    check(f.transport.reads == 1 && f.owner.state.nextDueUtc == f.clock.now.add(const Duration(seconds: 30)));
    check(f.owner.state.visibility == MonitoringObservationVisibility.pinned);
    f.owner.dispose();
  });
  test('fixed-window seed mismatch and failed read never arm an owner', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan();
    check(
      !f.owner.observeForeground(
        p,
        MonitoringObservedQuery(
          outcome,
          p.windowAt(f.clock.now) == null
              ? null
              : MonitoringTimeWindow.preset(MonitoringTimePreset.hour1, f.clock.now.add(const Duration(seconds: 1))),
        ),
      ),
    );
    check(
      !f.owner.observeForeground(
        p,
        MonitoringObservedQuery(
          const QueryOutcome(
            status: 'error',
            httpStatus: 503,
            error: MonitoringFailure.local(MonitoringErrorCode.timeout),
          ),
          p.windowAt(f.clock.now),
        ),
      ),
    );
    check(f.created == 0 && f.clock.pending == 0 && f.transport.reads == 0);
    f.owner.dispose();
  });
  test('binding revision acknowledgment stops pinned observation without a hidden read', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final binding = MonitoringBinding(
      id: '22222222-2222-4222-8222-222222222222',
      name: 'Owned binding',
      revision: 2,
      hostId: '44444444-4444-4444-8444-444444444444',
      sourceId: source.id,
      sourceRevision: 1,
      selector: const {'instance': 'owned'},
      state: MonitoringBindingState.current,
    );
    final p = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringBindingPanelTarget(
        binding: binding,
        panel: MonitoringPanel.cpu,
        windowPolicy: MonitoringFixedObservationWindow(plan().windowAt(f.clock.now)!),
      ),
    );
    final seed = MonitoringObservedBindingPanel(
      BindingReadResult(
        bindingId: binding.id,
        bindingRevision: 2,
        state: BindingReadState.missing,
        kind: 'bindingPanel',
        panel: MonitoringPanel.cpu,
      ),
      p.windowAt(f.clock.now)!,
    );
    check(f.owner.observeForeground(p, seed));
    f.owner.promoteForeground();
    f.owner.contextChanged(
      MonitoringObservationContext(
        source: source,
        binding: MonitoringBinding(
          id: binding.id,
          name: binding.name,
          revision: 3,
          hostId: binding.hostId,
          sourceId: source.id,
          sourceRevision: 1,
          selector: binding.selector,
          state: MonitoringBindingState.current,
        ),
      ),
      readOnly: false,
    );
    await f.clock.advance(60);
    check(f.transport.reads == 0 && f.owner.state.stopReason == MonitoringPinStop.sourceChanged);
    check(!f.owner.state.hasAcceptedResult);
    f.owner.dispose();
  });
  test('Native saved record removal before scheduled read sends no query', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final saved = SavedMonitoringPanel(
      id: '33333333-3333-4333-8333-333333333333',
      revision: 4,
      definition: SavedPanelDefinition(
        title: 'Owned saved panel',
        sourceId: source.id,
        sourceRevision: 1,
        query: 'native_saved_query',
        unitHint: SavedPanelUnitHint.unknown,
      ),
      state: SavedPanelState.current,
    );
    final p = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringSavedPanelTarget(panel: saved),
    );
    check(
      f.owner.observeForeground(
        p,
        const MonitoringObservedSavedPanel(SavedPanelRead(outcome, SavedPanelUnitHint.unknown), null),
      ),
    );
    // Source remains current; absence of the selected Native record still denies.
    await f.clock.advance(15);
    check(f.transport.reads == 0 && f.owner.state.stopReason == MonitoringPinStop.sourceChanged);
    check(!f.owner.state.hasAcceptedResult);
    f.owner.dispose();
  });
  test('wrong Native response target/window is permanent and clears last accepted data', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan();
    f.seed(p);
    f.transport.result = (plan, window) => const MonitoringObservedQuery(outcome, null);
    await f.clock.advance(15);
    check(f.owner.state.stopReason == MonitoringPinStop.rejected && !f.owner.state.hasAcceptedResult);
    check(f.clock.pending == 0);
    f.owner.dispose();
  });
  test('global Stop clears a hidden pin and late ticks cannot restart it', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.owner.promoteForeground();
    f.owner.setForegroundActive(false);
    f.owner.stop(MonitoringPinStop.manual);
    await f.clock.advance(300);
    check(f.transport.reads == 0 && !f.owner.state.hasAcceptedResult && f.clock.pending == 0);
    f.owner.setForegroundActive(true);
    check(!f.owner.promoteForeground());
    await f.clock.advance(300);
    check(f.transport.reads == 0);
    f.owner.dispose();
  });
  test('unpin keeps foreground cadence but hidden unpin stops instead of silently repinning', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.owner.promoteForeground();
    final due = f.owner.state.nextDueUtc;
    f.owner.unpin();
    check(
      f.owner.state.visibility == MonitoringObservationVisibility.foregroundOnly && f.owner.state.nextDueUtc == due,
    );
    check(f.created == 1 && f.clock.pending == 1);
    f.owner.promoteForeground();
    f.owner.setForegroundActive(false);
    f.owner.unpin();
    check(!f.owner.state.active && f.clock.pending == 0);
    f.owner.dispose();
  });
  test('old instant-only transport still works without implementing optional generalized capability', () async {
    final transport = LegacyTransport(), clock = Clock();
    final owner = MonitoringPinnedObservation(
      createTransport: () => transport,
      isCurrent: () => true,
      now: () => clock.now,
      schedule: clock.schedule,
    );
    check(
      owner.pin(const MonitoringPinPlan(source: source, expression: 'up', interval: MonitoringPinInterval.seconds30)),
    );
    await flush();
    check(transport.reads == 1 && owner.state.outcome == outcome);
    await clock.advance(30);
    check(transport.reads == 2 && clock.pending == 1);
    owner.dispose();
  });
  test('old instant-only transport cannot silently execute a custom range as instant', () async {
    final transport = LegacyTransport(), clock = Clock();
    final owner = MonitoringPinnedObservation(
      createTransport: () => transport,
      isCurrent: () => true,
      now: () => clock.now,
      schedule: clock.schedule,
    );
    check(owner.pin(plan()));
    await flush();
    check(transport.reads == 0 && owner.state.stopReason == MonitoringPinStop.rejected && clock.pending == 0);
    owner.dispose();
  });
  test('explicit Pin with Auto disabled seeds last manual value without immediate duplicate request', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan();
    check(f.owner.pinAccepted(p, MonitoringObservedQuery(outcome, p.windowAt(f.clock.now))));
    check(f.owner.state.visibility == MonitoringObservationVisibility.pinned && f.transport.reads == 0);
    f.owner.setForegroundActive(false);
    await f.clock.advance(15);
    check(f.transport.reads == 1 && f.owner.state.active && f.created == 1);
    f.owner.dispose();
  });
  test('ordinary manual cancellation only cancels foreground; pinned owner remains untouched', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.owner.cancelForeground();
    check(!f.owner.state.active && f.transport.cancels == 1);
    final p = plan();
    check(f.owner.pinAccepted(p, MonitoringObservedQuery(outcome, p.windowAt(f.clock.now))));
    final cancels = f.transport.cancels;
    f.owner.cancelForeground();
    check(f.owner.state.active && f.transport.cancels == cancels && f.clock.pending == 1);
    f.owner.dispose();
  });
  test('Auto OFF revokes authority and preserves the latest accepted actual-window snapshot', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan(rolling: true);
    f.seed(p);
    await f.clock.advance(15);
    final accepted = f.owner.state.value, success = f.owner.state.lastSuccessUtc;
    f.owner.disableForeground();
    check(!f.owner.state.active && f.clock.pending == 0 && f.transport.cancels == 1 && !f.owner.refresh());
    check(f.owner.state.stopReason == MonitoringPinStop.autoDisabled && identical(f.owner.state.value, accepted));
    check(f.owner.state.lastSuccessUtc == success && f.owner.state.nextDueUtc == null);
    await f.clock.advance(300);
    check(f.transport.reads == 1 && identical(f.owner.state.value, accepted));
    f.owner.setForegroundActive(false);
    check(!f.owner.state.hasAcceptedResult);
    f.owner.dispose();
  });
  test('Auto OFF during a pending read denies late response and retains only previous accepted UTC', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan(rolling: true);
    f.seed(p);
    final old = f.owner.state.value!;
    f.transport.pending = Completer<MonitoringObservationValue>();
    await f.clock.advance(15);
    f.owner.disableForeground();
    f.transport.pending!.complete(MonitoringObservedQuery(outcome, p.windowAt(f.clock.now)));
    await flush();
    check(identical(f.owner.state.value, old) && f.clock.pending == 0 && !f.owner.state.active);
    f.owner.dispose();
  });
  test('source catalog change clears readonly AutoOFF snapshot and cannot repin old Native authority', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.owner.disableForeground();
    f.owner.contextChanged(null, readOnly: false);
    check(f.owner.state.stopReason == MonitoringPinStop.sourceChanged && !f.owner.state.hasAcceptedResult);
    await f.clock.advance(300);
    check(f.transport.reads == 0);
    f.owner.dispose();
  });

  test('hidden app stops only foreground observation; visibility/resume cannot arm again', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.owner.lifecycle(MonitoringPinLifecycle.inactive);
    check(f.owner.state.active);
    f.owner.lifecycle(MonitoringPinLifecycle.hidden);
    check(!f.owner.state.active && f.clock.pending == 0);
    f.owner.lifecycle(MonitoringPinLifecycle.resumed);
    await f.clock.advance(300);
    check(f.transport.reads == 0 && !f.owner.state.hasAcceptedResult);
    f.owner.dispose();
  });

  test('late manual read seed cannot arm while app hidden even if selected route stays visible', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    final p = plan();
    f.owner.lifecycle(MonitoringPinLifecycle.hidden);
    check(!f.seed(p) && f.created == 0 && f.clock.pending == 0);
    f.owner.setForegroundActive(true); // A rebuild cannot override hidden state.
    check(!f.seed(p) && f.created == 0);
    f.owner.lifecycle(MonitoringPinLifecycle.resumed);
    await f.clock.advance(300);
    check(f.created == 0 && f.transport.reads == 0);
    f.owner.dispose();
  });
  test('unpin in a hidden app cannot turn into a background foreground-only timer', () async {
    final f = Fixture();
    f.owner.setForegroundActive(true);
    f.seed(plan());
    f.owner.promoteForeground();
    f.owner.lifecycle(MonitoringPinLifecycle.hidden);
    check(f.owner.state.active);
    f.owner.unpin();
    await f.clock.advance(300);
    check(!f.owner.state.active && f.clock.pending == 0 && f.transport.reads == 0);
    f.owner.dispose();
  });
}
