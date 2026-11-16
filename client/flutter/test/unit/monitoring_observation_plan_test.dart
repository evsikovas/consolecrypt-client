import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_observation_plan.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:flutter_test/flutter_test.dart';

void check(bool value) {
  if (!value) throw StateError('Test expectation failed');
}

void denied(void Function() body) {
  try {
    body();
  } on MonitoringFailure {
    return;
  }
  throw StateError('Expected invalid immutable observation plan');
}

const source = MonitoringSource(
  id: '11111111-1111-4111-8111-111111111111',
  name: 'Owned observation source',
  endpoint: 'http://metrics.invalid',
  revision: 7,
);
const definition = SavedPanelDefinition(
  title: 'Owned saved observation',
  sourceId: '11111111-1111-4111-8111-111111111111',
  sourceRevision: 7,
  query: 'rate(owned_total[5m])',
  unitHint: SavedPanelUnitHint.bytes,
);
const saved = SavedMonitoringPanel(
  id: '33333333-3333-4333-8333-333333333333',
  revision: 4,
  definition: definition,
  state: SavedPanelState.current,
);
MonitoringBinding binding({
  int revision = 2,
  int sourceRevision = 7,
  MonitoringBindingState state = MonitoringBindingState.current,
  Map<String, String> selector = const {'site': 'east', 'instance': 'owned'},
}) => MonitoringBinding(
  id: '22222222-2222-4222-8222-222222222222',
  name: 'Owned mapping',
  hostId: '44444444-4444-4444-8444-444444444444',
  sourceId: source.id,
  revision: revision,
  sourceRevision: sourceRevision,
  selector: selector,
  state: state,
);
MonitoringTimeWindow fixed() =>
    MonitoringTimeWindow(startUtc: DateTime.utc(2020, 1, 1), endUtc: DateTime.utc(2020, 1, 2), stepSeconds: 120);

void main() {
  test('existing const D1 constructor and expression/interval API stay compatible', () {
    const plan = MonitoringPinPlan(source: source, expression: 'up', interval: MonitoringPinInterval.minute1);
    plan.validate();
    check(plan.expression == 'up' && plan.interval == MonitoringPinInterval.minute1);
    check(plan.target is MonitoringCustomInstantTarget && plan.windowAt(DateTime.utc(2040)) == null);
  });
  test('generalized plan default is fifteen-second cadence, not query step', () {
    const plan = MonitoringPinPlan.forTarget(source: source, target: MonitoringCustomInstantTarget('up'));
    plan.validate();
    check(plan.interval.duration == const Duration(seconds: 15));
  });
  test('fixed custom period stays exact while clock advances years', () {
    final window = fixed();
    final plan = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringCustomRangeTarget(expression: 'up', windowPolicy: MonitoringFixedObservationWindow(window)),
    );
    plan.validate();
    check(identical(plan.windowAt(DateTime.utc(2026)), window));
    check(identical(plan.windowAt(DateTime.utc(2040)), window));
  });
  test('rolling preset moves only because policy was explicitly selected', () {
    const policy = MonitoringRollingObservationWindow(preset: MonitoringTimePreset.minutes15, stepSeconds: 20);
    final one = policy.at(DateTime.utc(2026, 11, 12)), two = policy.at(DateTime.utc(2026, 11, 12, 0, 0, 45));
    check(one.endUtc.difference(one.startUtc) == const Duration(minutes: 15));
    check(two.startUtc.difference(one.startUtc) == const Duration(seconds: 45));
    check(two.stepSeconds == 20);
    final fixedPreset = MonitoringFixedObservationWindow(one);
    check(fixedPreset.at(two.endUtc) == one);
  });
  test('custom pseudo-preset and invalid rolling step rejected', () {
    for (final policy in [
      const MonitoringRollingObservationWindow(preset: MonitoringTimePreset.custom, stepSeconds: 15),
      const MonitoringRollingObservationWindow(preset: MonitoringTimePreset.hour1, stepSeconds: double.nan),
      const MonitoringRollingObservationWindow(preset: MonitoringTimePreset.hour1, stepSeconds: 0),
    ]) {
      denied(
        () => MonitoringPinPlan.forTarget(
          source: source,
          target: MonitoringCustomRangeTarget(expression: 'up', windowPolicy: policy),
        ).validate(),
      );
    }
  });
  test('sampling step participates in immutable request identity', () {
    final one = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringCustomRangeTarget(expression: 'up', windowPolicy: MonitoringFixedObservationWindow(fixed())),
    );
    final old = fixed();
    final two = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringCustomRangeTarget(
        expression: 'up',
        windowPolicy: MonitoringFixedObservationWindow(
          MonitoringTimeWindow(startUtc: old.startUtc, endUtc: old.endUtc, stepSeconds: 300),
        ),
      ),
    );
    check(one.requestIdentity != two.requestIdentity);
  });
  test('cadence change does not silently alter the requested query or period', () {
    final target = MonitoringCustomRangeTarget(
      expression: 'up',
      windowPolicy: MonitoringFixedObservationWindow(fixed()),
    );
    final one = MonitoringPinPlan.forTarget(source: source, target: target);
    final two = MonitoringPinPlan.forTarget(source: source, target: target, interval: MonitoringPinInterval.minutes5);
    check(one.requestIdentity == two.requestIdentity && one.fingerprint != two.fingerprint);
  });
  test('empty/control/multibyte oversized custom expression is not accepted', () {
    for (final expression in [' ', 'up\u0000', '\u041c' * 9000]) {
      denied(
        () => MonitoringPinPlan.forTarget(source: source, target: MonitoringCustomInstantTarget(expression)).validate(),
      );
    }
  });
  test('binding plan is explicit Native identity and has no legacy expression fallback', () {
    final plan = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringBindingPanelTarget(
        binding: binding(),
        panel: MonitoringPanel.cpu,
        windowPolicy: MonitoringFixedObservationWindow(fixed()),
      ),
    );
    plan.validate();
    check(plan.customExpression == null);
    expect(() => plan.expression, throwsStateError);
  });
  test('stale source revision or missing host binding cannot be pinned', () {
    for (final value in [binding(sourceRevision: 6), binding(state: MonitoringBindingState.missingHost)]) {
      denied(
        () => MonitoringPinPlan.forTarget(
          source: source,
          target: MonitoringBindingPanelTarget(
            binding: value,
            panel: MonitoringPanel.cpu,
            windowPolicy: MonitoringFixedObservationWindow(fixed()),
          ),
        ).validate(),
      );
    }
  });
  test('fresh binding fingerprint checks exact selector but ignores map iteration order', () {
    final plan = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringBindingPanelTarget(
        binding: binding(),
        panel: MonitoringPanel.cpu,
        windowPolicy: MonitoringFixedObservationWindow(fixed()),
      ),
    );
    check(
      MonitoringObservationContext(
        source: source,
        binding: binding(selector: {'instance': 'owned', 'site': 'east'}),
      ).matches(plan),
    );
    check(
      !MonitoringObservationContext(
        source: source,
        binding: binding(selector: {'site': 'west', 'instance': 'owned'}),
      ).matches(plan),
    );
  });
  test('binding revision and source removal/change cannot match old plan', () {
    final plan = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringBindingPanelTarget(
        binding: binding(),
        panel: MonitoringPanel.cpu,
        windowPolicy: MonitoringFixedObservationWindow(fixed()),
      ),
    );
    check(!const MonitoringObservationContext(source: source).matches(plan));
    check(!MonitoringObservationContext(source: source, binding: binding(revision: 3)).matches(plan));
    check(!MonitoringObservationContext(source: source.withRecord(revision: 8), binding: binding()).matches(plan));
  });
  test('saved instant and saved range retain ID/revision and user unit hint without substituting expression', () {
    const instant = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringSavedPanelTarget(panel: saved),
    );
    instant.validate();
    check(instant.customExpression == null && instant.windowAt(DateTime.utc(2026)) == null);
    final range = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringSavedPanelTarget(panel: saved, windowPolicy: MonitoringFixedObservationWindow(fixed())),
    );
    range.validate();
    check(range.windowAt(DateTime.utc(2030)) == fixed());
    check(instant.requestIdentity != range.requestIdentity);
  });
  test('saved record revision/deletion stops exact context matching', () {
    const plan = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringSavedPanelTarget(panel: saved),
    );
    check(const MonitoringObservationContext(source: source, panel: saved).matches(plan));
    check(!const MonitoringObservationContext(source: source).matches(plan));
    final newer = SavedMonitoringPanel(
      id: saved.id,
      revision: 5,
      definition: definition,
      state: SavedPanelState.current,
    );
    check(!MonitoringObservationContext(source: source, panel: newer).matches(plan));
  });
  test('same-revision changed saved definition is not silently accepted', () {
    const plan = MonitoringPinPlan.forTarget(
      source: source,
      target: MonitoringSavedPanelTarget(panel: saved),
    );
    final changed = SavedMonitoringPanel(
      id: saved.id,
      revision: saved.revision,
      definition: SavedPanelDefinition(
        title: 'Owned saved observation',
        sourceId: source.id,
        sourceRevision: 7,
        query: 'different_literal',
        unitHint: SavedPanelUnitHint.bytes,
      ),
      state: SavedPanelState.current,
    );
    check(!MonitoringObservationContext(source: source, panel: changed).matches(plan));
  });
  test('accepted partial/NaN/zero keeps actual old window rather than next rolling plan', () {
    const outcome = QueryOutcome(
      status: 'success',
      httpStatus: 200,
      partial: true,
      warnings: ['owned warning'],
      data: QueryData(
        'matrix',
        series: [
          MonitoringSeries(
            {'owned': 'series'},
            [MonitoringSample(1, SampleKind.finite, 0), MonitoringSample(2, SampleKind.nan)],
          ),
        ],
      ),
    );
    final oldWindow = fixed(), value = MonitoringObservedQuery(outcome, fixed());
    const rolling = MonitoringRollingObservationWindow(preset: MonitoringTimePreset.hour1, stepSeconds: 15);
    check(value.window == oldWindow && value.window != rolling.at(DateTime.utc(2030)));
    check(value.queryOutcome.partial && value.queryOutcome.data!.series.first.samples[0].value == 0);
    check(value.queryOutcome.data!.series.first.samples[1].kind == SampleKind.nan);
  });
  test('failed native outcome remains failure rather than fabricated accepted success', () {
    const failure = MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
    const value = MonitoringObservedQuery(QueryOutcome(status: 'error', httpStatus: 500, error: failure), null);
    check(value.failure == failure);
  });
}
