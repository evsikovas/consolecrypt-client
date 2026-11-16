import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:consolecrypt/monitoring/monitoring_service.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';

/// One owner creates one private read service and reuses the existing catalog
/// store. Record reads always remain Native ID/revision operations.
final class ScopedMonitoringPinTransport implements MonitoringGeneralPinTransport {
  ScopedMonitoringPinTransport(MonitoringBackend backend)
    : service = backend.createService(),
      sources = backend.createSourceStore();
  final MonitoringService service;
  final MonitoringSourceStore sources;

  @override
  Future<MonitoringSource?> loadSource(String sourceId) async {
    final values = await sources.load();
    if (sources.readOnly) {
      throw const MonitoringFailure.local(MonitoringErrorCode.readOnly);
    }
    return values.where((source) => source.id == sourceId).firstOrNull;
  }

  @override
  Future<MonitoringObservationContext?> loadContext(MonitoringPinPlan plan) async {
    final source = await loadSource(plan.source.id);
    if (source == null) return null;
    final target = plan.target;
    final bindingStore = sources is MonitoringBindingStore ? sources as MonitoringBindingStore : null;
    final panelStore = sources is MonitoringSavedPanelStore ? sources as MonitoringSavedPanelStore : null;
    return MonitoringObservationContext(
      source: source,
      binding: target is MonitoringBindingPanelTarget
          ? bindingStore?.bindings.where((binding) => binding.id == target.binding.id).firstOrNull
          : null,
      panel: target is MonitoringSavedPanelTarget
          ? panelStore?.panels.where((panel) => panel.id == target.panel.id).firstOrNull
          : null,
    );
  }

  @override
  Future<QueryOutcome> readCurrent(MonitoringSource source, String expression) =>
      service.query(source, MonitoringQuery(expression: expression));

  @override
  Future<MonitoringObservationValue> readTarget(
    MonitoringPinPlan plan,
    MonitoringObservationContext context,
    MonitoringTimeWindow? window,
  ) async {
    if (!context.matches(plan)) {
      throw const MonitoringFailure.local(MonitoringErrorCode.sourceChanged);
    }
    final MonitoringObservationValue value;
    switch (plan.target) {
      case MonitoringCustomInstantTarget(:final expression):
        if (window != null) {
          throw const MonitoringFailure.local(MonitoringErrorCode.invalidQuery);
        }
        value = MonitoringObservedQuery(await readCurrent(context.source, expression), null);
      case MonitoringCustomRangeTarget(:final expression):
        if (window == null) {
          throw const MonitoringFailure.local(MonitoringErrorCode.invalidQuery);
        }
        value = MonitoringObservedQuery(
          await service.query(
            context.source,
            MonitoringQuery(
              expression: expression,
              start: window.startSeconds,
              end: window.endSeconds,
              stepSeconds: window.stepSeconds,
            ),
          ),
          window,
        );
      case MonitoringBindingPanelTarget(:final panel):
        if (window == null || service is! MonitoringBindingReader || context.binding == null) {
          throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedResult);
        }
        value = MonitoringObservedBindingPanel(
          await (service as MonitoringBindingReader).readPanel(
            context.source,
            context.binding!,
            panel,
            start: window.startSeconds,
            end: window.endSeconds,
            stepSeconds: window.stepSeconds,
          ),
          window,
        );
      case MonitoringSavedPanelTarget():
        if (service is! MonitoringSavedPanelReader || context.panel == null) {
          throw const MonitoringFailure.local(MonitoringErrorCode.unsupportedResult);
        }
        value = MonitoringObservedSavedPanel(
          await (service as MonitoringSavedPanelReader).readSavedPanel(
            context.source,
            context.panel!,
            window: window == null
                ? null
                : SavedPanelRange(start: window.startSeconds, end: window.endSeconds, stepSeconds: window.stepSeconds),
          ),
          window,
        );
    }
    if (!monitoringObservationValueMatchesPlan(plan, value)) {
      throw const MonitoringFailure.local(MonitoringErrorCode.invalidResponse);
    }
    return value;
  }

  @override
  void cancel() => service.cancel();
}
