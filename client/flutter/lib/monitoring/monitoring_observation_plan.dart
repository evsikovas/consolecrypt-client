import 'dart:convert';

import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';

enum MonitoringPinInterval {
  seconds15(Duration(seconds: 15)),
  seconds30(Duration(seconds: 30)),
  minute1(Duration(minutes: 1)),
  minutes5(Duration(minutes: 5));

  const MonitoringPinInterval(this.duration);
  final Duration duration;
}

enum MonitoringObservationVisibility { foregroundOnly, pinned }

sealed class MonitoringObservationWindow {
  const MonitoringObservationWindow();
  MonitoringTimeWindow at(DateTime tickUtc);
  void validate();
  Object get identity;
}

final class MonitoringFixedObservationWindow extends MonitoringObservationWindow {
  const MonitoringFixedObservationWindow(this.window);
  final MonitoringTimeWindow window;
  @override
  MonitoringTimeWindow at(DateTime tickUtc) => window;
  @override
  void validate() {}
  @override
  Object get identity => ['fixed', window.startText, window.endText, window.stepSeconds];
}

final class MonitoringRollingObservationWindow extends MonitoringObservationWindow {
  const MonitoringRollingObservationWindow({required this.preset, required this.stepSeconds});
  final MonitoringTimePreset preset;
  final double stepSeconds;
  @override
  MonitoringTimeWindow at(DateTime tickUtc) {
    validate();
    return MonitoringTimeWindow.preset(preset, tickUtc.toUtc(), stepSeconds: stepSeconds);
  }

  @override
  void validate() {
    if (preset == MonitoringTimePreset.custom || !stepSeconds.isFinite || stepSeconds <= 0) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidQuery);
    }
  }

  @override
  Object get identity => ['rollingPreset', preset.name, stepSeconds];
}

sealed class MonitoringObservationTarget {
  const MonitoringObservationTarget();
  MonitoringObservationWindow? get windowPolicy;
  Object get identity;
}

final class MonitoringCustomInstantTarget extends MonitoringObservationTarget {
  const MonitoringCustomInstantTarget(this.expression);
  final String expression;
  @override
  MonitoringObservationWindow? get windowPolicy => null;
  @override
  Object get identity => ['customInstant', expression];
}

final class MonitoringCustomRangeTarget extends MonitoringObservationTarget {
  const MonitoringCustomRangeTarget({required this.expression, required this.windowPolicy});
  final String expression;
  @override
  final MonitoringObservationWindow windowPolicy;
  @override
  Object get identity => ['customRange', expression, windowPolicy.identity];
}

final class MonitoringBindingPanelTarget extends MonitoringObservationTarget {
  const MonitoringBindingPanelTarget({required this.binding, required this.panel, required this.windowPolicy});
  final MonitoringBinding binding;
  final MonitoringPanel panel;
  @override
  final MonitoringObservationWindow windowPolicy;
  @override
  Object get identity => [
    'bindingPanel',
    monitoringBindingObservationIdentity(binding),
    panel.name,
    windowPolicy.identity,
  ];
}

final class MonitoringSavedPanelTarget extends MonitoringObservationTarget {
  const MonitoringSavedPanelTarget({required this.panel, this.windowPolicy});
  final SavedMonitoringPanel panel;
  @override
  final MonitoringObservationWindow? windowPolicy;
  @override
  Object get identity => ['savedPanel', monitoringSavedObservationIdentity(panel), windowPolicy?.identity];
}

Object monitoringBindingObservationIdentity(MonitoringBinding binding) {
  final keys = binding.selector.keys.toList()..sort();
  return [
    binding.id,
    binding.name,
    binding.revision,
    binding.sourceId,
    binding.sourceRevision,
    binding.hostId,
    binding.state.name,
    [
      for (final key in keys) [key, binding.selector[key]],
    ],
  ];
}

Object monitoringSavedObservationIdentity(SavedMonitoringPanel panel) => [
  panel.id,
  panel.revision,
  panel.state.name,
  panel.definition.toJson(),
];

/// Immutable UI request snapshot. This is never an execution approval or a
/// replacement for Native source/record revision and credential authorization.
final class MonitoringPinPlan {
  /// Existing D1 instant API stays const-compatible. Its old explicit interval
  /// remains required; the generalized constructor/default UI use fifteen seconds.
  const MonitoringPinPlan({required this.source, required String expression, required this.interval})
    : _legacyExpression = expression,
      _target = null;
  const MonitoringPinPlan.forTarget({
    required MonitoringSource source,
    required MonitoringObservationTarget target,
    MonitoringPinInterval interval = MonitoringPinInterval.seconds15,
  }) : this._(source, target, interval);
  const MonitoringPinPlan._(this.source, this._target, this.interval) : _legacyExpression = null;
  final MonitoringSource source;
  final MonitoringPinInterval interval;
  final String? _legacyExpression;
  final MonitoringObservationTarget? _target;
  MonitoringObservationTarget get target => _target ?? MonitoringCustomInstantTarget(_legacyExpression!);
  String? get customExpression => switch (target) {
    MonitoringCustomInstantTarget(:final expression) || MonitoringCustomRangeTarget(:final expression) => expression,
    _ => null,
  };

  /// Legacy custom-only accessor. Native record targets cannot silently become
  /// caller-expression requests through a convenience fallback.
  String get expression => customExpression ?? (throw StateError('Native record target has no custom expression'));
  String get requestIdentity => jsonEncode([source.fingerprint, source.readOnly, target.identity]);
  String get fingerprint => jsonEncode([requestIdentity, interval.name]);
  MonitoringTimeWindow? windowAt(DateTime tickUtc) => target.windowPolicy?.at(tickUtc);

  void validate() {
    source.validate();
    if (!source.allowsBackgroundObservation) {
      throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedRoute);
    }
    if (source.revision <= 0 || source.readOnly) {
      throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
    }
    final expression = customExpression;
    if (expression != null &&
        (expression.trim().isEmpty ||
            utf8.encode(expression).length > 16384 ||
            expression.runes.any((r) => r < 32 && r != 9 && r != 10 && r != 13 || r >= 127 && r <= 159))) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidQuery);
    }
    switch (target) {
      case MonitoringBindingPanelTarget(:final binding):
        binding.validate();
        if (binding.state != MonitoringBindingState.current ||
            binding.sourceId != source.id ||
            binding.sourceRevision != source.revision) {
          throw const MonitoringFailure.local(MonitoringErrorCode.bindingChanged);
        }
      case MonitoringSavedPanelTarget(:final panel):
        panel.definition.validate();
        if (!RegExp(r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$').hasMatch(panel.id) ||
            panel.id == '00000000-0000-0000-0000-000000000000' ||
            panel.revision <= 0 ||
            !panel.currentFor(source)) {
          throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
        }
      case MonitoringCustomInstantTarget() || MonitoringCustomRangeTarget():
        break;
    }
    target.windowPolicy?.validate();
  }
}

/// One fresh catalog read, not a stored approval. Compare before AND after the
/// Native read. The transport never resolves a host or substitutes an expression.
final class MonitoringObservationContext {
  const MonitoringObservationContext({required this.source, this.binding, this.panel});
  final MonitoringSource source;
  final MonitoringBinding? binding;
  final SavedMonitoringPanel? panel;
  bool matches(MonitoringPinPlan plan) {
    if (source.readOnly || source.id != plan.source.id || source.fingerprint != plan.source.fingerprint) {
      return false;
    }
    return switch (plan.target) {
      MonitoringBindingPanelTarget(:final binding) =>
        this.binding != null &&
            this.binding!.state == MonitoringBindingState.current &&
            jsonEncode(monitoringBindingObservationIdentity(this.binding!)) ==
                jsonEncode(monitoringBindingObservationIdentity(binding)),
      MonitoringSavedPanelTarget(:final panel) =>
        this.panel != null &&
            this.panel!.currentFor(source) &&
            jsonEncode(monitoringSavedObservationIdentity(this.panel!)) ==
                jsonEncode(monitoringSavedObservationIdentity(panel)),
      MonitoringCustomInstantTarget() || MonitoringCustomRangeTarget() => true,
    };
  }
}

/// Accepted data carries the ACTUAL requested window. Stale retention must keep
/// this object/window, not relabel it with a newly planned rolling tick.
sealed class MonitoringObservationValue {
  const MonitoringObservationValue(this.window);
  final MonitoringTimeWindow? window;
  QueryOutcome? get queryOutcome;
  MonitoringFailure? get failure;
}

final class MonitoringObservedQuery extends MonitoringObservationValue {
  const MonitoringObservedQuery(this.outcome, super.window);
  final QueryOutcome outcome;
  @override
  QueryOutcome get queryOutcome => outcome;
  @override
  MonitoringFailure? get failure =>
      outcome.error ??
      (outcome.status == 'success' ? null : const MonitoringFailure.local(MonitoringErrorCode.invalidResponse));
}

final class MonitoringObservedSavedPanel extends MonitoringObservationValue {
  const MonitoringObservedSavedPanel(this.read, super.window);
  final SavedPanelRead read;
  @override
  QueryOutcome get queryOutcome => read.outcome;
  @override
  MonitoringFailure? get failure =>
      read.outcome.error ??
      (read.outcome.status == 'success' ? null : const MonitoringFailure.local(MonitoringErrorCode.invalidResponse));
}

final class MonitoringObservedBindingPanel extends MonitoringObservationValue {
  const MonitoringObservedBindingPanel(this.read, MonitoringTimeWindow window) : super(window);
  final BindingReadResult read;
  @override
  QueryOutcome? get queryOutcome => null;
  @override
  MonitoringFailure? get failure => read.error ?? read.preparationError;
}

bool monitoringObservationValueMatchesPlan(MonitoringPinPlan plan, MonitoringObservationValue value) {
  bool windowMatches() => switch ((plan.target.windowPolicy, value.window)) {
    (null, null) => true,
    (MonitoringFixedObservationWindow(:final window), final MonitoringTimeWindow actual) => window == actual,
    (MonitoringRollingObservationWindow(:final preset, :final stepSeconds), final MonitoringTimeWindow actual) =>
      preset.duration != null &&
          actual.endUtc.difference(actual.startUtc) == preset.duration &&
          actual.stepSeconds == stepSeconds,
    _ => false,
  };
  if (!windowMatches()) return false;
  return switch ((plan.target, value)) {
    (MonitoringCustomInstantTarget() || MonitoringCustomRangeTarget(), MonitoringObservedQuery()) => true,
    (MonitoringSavedPanelTarget(:final panel), MonitoringObservedSavedPanel(:final read)) =>
      panel.definition.unitHint == read.unitHint,
    (MonitoringBindingPanelTarget(:final binding, :final panel), MonitoringObservedBindingPanel(:final read)) =>
      read.kind == 'bindingPanel' &&
          read.bindingId == binding.id &&
          read.bindingRevision == binding.revision &&
          read.panel == panel,
    _ => false,
  };
}
