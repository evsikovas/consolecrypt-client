import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_accepted_query.dart';
import 'package:consolecrypt/monitoring/monitoring_errors.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_result_view.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:material_ui/material_ui.dart';

/// Read-only projection. It never changes the query window or issues a read.
class MonitoringAcceptedQueryView extends StatelessWidget {
  const MonitoringAcceptedQueryView({
    super.key,
    required this.state,
    this.requestedWindow,
    this.timeWindow,
    this.active = true,
  });
  final MonitoringAcceptedQueryState state;
  final MonitoringTimeWindow? requestedWindow;
  final MonitoringTimeWindowController? timeWindow;
  final bool active;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, accepted = state.accepted, latest = state.latestAttempt;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (state.phase == MonitoringAcceptedQueryPhase.refreshing && accepted != null)
          InfoBanner(key: const ValueKey('monitoring-previous-refreshing'), message: l.monitoringPreviousRefreshing),
        if (state.phase == MonitoringAcceptedQueryPhase.stale)
          InfoBanner(
            key: const ValueKey('monitoring-previous-stale'),
            message: l.monitoringPreviousStale,
            tone: BannerTone.warning,
          ),
        if (state.error case final failure?)
          InfoBanner(
            key: const ValueKey('monitoring-refresh-error'),
            tone: BannerTone.danger,
            message: failure is MonitoringFailure ? monitoringFailureMessage(l, failure) : errorMessage(l, failure),
          ),
        if (accepted != null) ...[
          Text(
            l.monitoringReadLastSuccess(accepted.completedUtc.toIso8601String()),
            key: const ValueKey('monitoring-read-last-success'),
          ),
          MonitoringResultView(
            result: accepted.outcome,
            requestedWindow: requestedWindow,
            timeWindow: timeWindow,
            active: active,
          ),
        ],
        if (latest != null && (latest.status != 'success' || latest.error != null))
          if (accepted == null)
            MonitoringResultView(
              result: latest,
              requestedWindow: requestedWindow,
              timeWindow: timeWindow,
              active: active,
            )
          else
            ExpansionTile(
              key: const ValueKey('monitoring-failed-attempt'),
              title: Text(l.monitoringFailedAttempt),
              children: [
                Text(l.monitoringFailedAttemptHelp),
                MonitoringResultView(
                  result: latest,
                  requestedWindow: requestedWindow,
                  timeWindow: timeWindow,
                  active: active,
                ),
              ],
            ),
      ],
    );
  }
}
