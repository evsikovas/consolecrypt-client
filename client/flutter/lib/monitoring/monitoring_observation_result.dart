import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_view.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panels.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:material_ui/material_ui.dart';

/// Only accepted memory data. One existing time/viewport controller serves all
/// charts in a Native panel. No service, timer, resolver or raw-query fallback.
class MonitoringObservationResultView extends StatefulWidget {
  const MonitoringObservationResultView({super.key, required this.value, this.active = true, this.jobTimeline});
  final MonitoringObservationValue value;
  final bool active;
  final MonitoringCorrelationTimeline? jobTimeline;
  @override
  State<MonitoringObservationResultView> createState() => _MonitoringObservationResultViewState();
}

class _MonitoringObservationResultViewState extends State<MonitoringObservationResultView> {
  MonitoringTimeWindowController? timeWindow;
  @override
  void initState() {
    super.initState();
    updateWindow();
  }

  void updateWindow() {
    final window = widget.value.window;
    if (window == null) {
      timeWindow?.dispose();
      timeWindow = null;
      return;
    }
    if (timeWindow == null) {
      timeWindow = MonitoringTimeWindowController(initial: window);
    } else if (timeWindow!.applied != window) {
      timeWindow!.presentAcceptedWindow(window);
    }
  }

  @override
  void didUpdateWidget(MonitoringObservationResultView oldWidget) {
    super.didUpdateWidget(oldWidget);
    updateWindow();
  }

  @override
  void dispose() {
    timeWindow?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, value = widget.value;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(value.window == null ? l.monitoringObservationCurrentValues : l.monitoringObservationRangeValues),
        if (value.window case final window?)
          Text('${window.startText} — ${window.endText} · ${l.monitoringStep}: ${window.stepSeconds}'),
        switch (value) {
          MonitoringObservedQuery(:final outcome) => MonitoringResultView(
            result: outcome,
            requestedWindow: value.window,
            timeWindow: timeWindow,
            active: widget.active,
          ),
          MonitoringObservedSavedPanel(:final read) => Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('${l.monitoringSavedPanelHint}: ${savedPanelUnitLabel(l, read.unitHint)}'),
              MonitoringResultView(
                result: read.outcome,
                requestedWindow: value.window,
                timeWindow: timeWindow,
                active: widget.active,
              ),
            ],
          ),
          MonitoringObservedBindingPanel(:final read) => MonitoringBindingResultView(
            result: read,
            jobTimeline: widget.jobTimeline,
            requestedWindow: value.window,
            timeWindow: timeWindow,
            active: widget.active,
          ),
        },
      ],
    );
  }
}
