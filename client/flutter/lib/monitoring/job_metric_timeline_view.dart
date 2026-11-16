import 'package:consolecrypt/automation/automation_event_view.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_flow.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:material_ui/material_ui.dart';

/// Stateless bounded metadata view. The original owner checks current flow and
/// Native context on selection; this cannot fetch, navigate or execute work.
typedef MonitoringCorrelationTimeline = ({
  JobMetricTimelineFlow owner,
  JobMetricTimelineSnapshot snapshot,
  MonitoringTimeWindow window,
  ValueChanged<JobMetricMarker> openProject,
  bool localTime,
});

String jobMetricDisplayTime(DateTime value, bool local) {
  if (!local) return value.toUtc().toIso8601String();
  final time = value.toLocal(), offset = value.toLocal().timeZoneOffset.inMinutes;
  final hours = (offset.abs() ~/ 60).toString().padLeft(2, '0');
  final minutes = (offset.abs() % 60).toString().padLeft(2, '0');
  return '${time.toIso8601String()}${offset < 0 ? '-' : '+'}$hours:$minutes';
}

class JobMetricTimelineView extends StatelessWidget {
  const JobMetricTimelineView({
    super.key,
    required this.snapshot,
    required this.busy,
    required this.failure,
    required this.onRead,
    required this.onCancel,
    required this.onChoose,
    required this.formatTime,
    required this.timeZoneLabel,
    this.enabled = true,
  });
  final JobMetricTimelineSnapshot? snapshot;
  final bool busy;
  final bool enabled;
  final JobMetricTimelineFailure? failure;
  final VoidCallback onRead, onCancel;
  final ValueChanged<JobMetricMarker> onChoose;

  /// The host supplies its chosen display zone. UTC data stays unchanged;
  /// neither system-local timezone nor clock alignment is inferred here.
  final String Function(DateTime) formatTime;
  final String timeZoneLabel;


  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final value = snapshot;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.correlationTimelineTitle),
        Text(l.correlationTimelineWholeJob),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            GlassButton(
              key: const ValueKey('job-timeline-read'),
              label: l.correlationTimelineRead,
              onPressed: !enabled || busy ? null : onRead,
            ),
            if (busy)
              TextButton(key: const ValueKey('job-timeline-cancel'), onPressed: onCancel, child: Text(l.commonCancel)),
          ],
        ),
        if (busy) const LinearProgressIndicator(),
        if (failure != null)
          InfoBanner(key: const ValueKey('job-timeline-failure'),
            message: failure == JobMetricTimelineFailure.changed ? l.correlationContextChanged : l.correlationTimelineFailed,
            tone: BannerTone.danger),
        if (value == null && !busy && failure == null) Text(l.correlationTimelineNotRead),
        if (value != null) ...[
          Text(
            l.correlationTimelineWindow(
              formatTime(value.selection.startUtc),
              formatTime(value.selection.endUtc),
              timeZoneLabel,
            ),
          ),
          if (value.jobs.markers.isEmpty) Text(l.correlationTimelineEmpty),
          if (value.jobs.truncated) Text(l.correlationTimelineTruncated),
          ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 360),
            child: ListView.builder(
              key: const ValueKey('job-timeline-rows'),
              shrinkWrap: true,
              primary: false,
              itemCount: value.jobs.markers.length,
              itemBuilder: (context, index) => JobMetricTimelineEntry(marker: value.jobs.markers[index], enabled: enabled,
                  busy: busy, formatTime: formatTime, onChoose: onChoose),
            ),
          ),
        ],
      ],
    );
  }
}

/// The same read-only job details are used by host history and graph annotations.
class JobMetricTimelineEntry extends StatelessWidget {
  const JobMetricTimelineEntry({super.key, required this.marker, required this.formatTime,
    required this.onChoose, this.enabled = true, this.busy = false, this.initiallyExpanded = false, this.nominalClientTime = false});
  final JobMetricMarker marker;
  final String Function(DateTime) formatTime;
  final ValueChanged<JobMetricMarker> onChoose;
  final bool enabled, busy, initiallyExpanded, nominalClientTime;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final position = marker.position, provenance = marker.provenance, process = marker.runnerObservation;
    final state = marker.state == JobMetricState.cancelling
        ? l.noticesCancelling
        : automationJobStateLabel(l, marker.state.name);
    return ExpansionTile(
      initiallyExpanded: initiallyExpanded,
      key: ValueKey('job-timeline-row-${marker.jobId}'),
      title: Text(l.correlationTimelineJob(marker.jobId)),
      subtitle: Text(state),
      children: [
        Padding(
          padding: const EdgeInsets.all(8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(l.correlationTimelineRevision(marker.revisionDigest)),
              Text(
                l.correlationTimelineAdmitted(
                  marker.admitted == null ? l.correlationTimelineTimeUnknown : formatTime(marker.admitted!.utc),
                ),
              ),
              if (marker.updated != null) Text(l.correlationTimelineUpdated(formatTime(marker.updated!.utc))),
              if (marker.runnerQueued != null)
                Text(l.correlationTimelineRunnerQueued(formatTime(marker.runnerQueued!.utc))),
              if (marker.runnerUpdated != null)
                Text(l.correlationTimelineRunnerUpdated(formatTime(marker.runnerUpdated!.utc))),
              Text(l.correlationTimelineReceived(formatTime(marker.received.utc))),
              if (provenance == null) ...[
                Text(l.correlationTimelineInitiatorUnknown),
                if (marker.provenanceStatus == JobProvenanceStatus.unavailable)
                  Text(l.correlationTimelineProvenanceUnavailable),
              ] else ...[
                Text(l.correlationTimelineAdmissionFacts),
                SelectableText(l.correlationTimelineInitiatorDevice(provenance.deviceId),
                  key: ValueKey('job-timeline-device-${marker.jobId}')),
                SelectableText(l.correlationTimelineUploadedSnapshot(provenance.revisionObjectId),
                  key: ValueKey('job-timeline-snapshot-${marker.jobId}')),
                Text(l.correlationTimelineProjectRevision(provenance.projectRevision)),
              ],
              if (process == null) ...[
                Text(l.correlationTimelineExecutionUnknown),
                if (marker.runnerObservationStatus == JobRunnerObservationStatus.unavailable)
                  Text(l.correlationTimelineRunnerObservationUnavailable),
              ] else ...[
                const SizedBox(height: 8),
                Text(l.correlationTimelineRunnerProcess),
                Text(l.correlationTimelineRunnerWallClock),
                Text(l.correlationTimelineRunnerSpawnObserved(formatTime(process.spawnObservedUtc))),
                Text(l.correlationTimelineRunnerExitObserved(formatTime(process.exitObservedUtc))),
                Text(l.correlationTimelineRunnerEnvelopeMs(process.elapsedMonotonicMs)),
                Text(l.correlationTimelineTargetExecutionUnknown),
                if (process.wallClockReversed)
                  InfoBanner(key: ValueKey('job-timeline-clock-reversal-${marker.jobId}'),
                    message: l.correlationTimelineRunnerClockReversed, tone: BannerTone.warning)
                else if (!process.hasApplicableClockMeasurement)
                  Text(l.correlationTimelineRunnerClockUnknown),
              ],
              if (position != null)
                Text(l.correlationTimelinePosition(formatTime(position.earliestUtc), formatTime(position.latestUtc)))
              else
                Text(switch (marker.unpositioned) {
                  JobMarkerUnpositioned.outsidePeriod => l.correlationTimelineOutside,
                  JobMarkerUnpositioned.missingAdmissionTime => l.correlationTimelineMissingTime,
                  _ => nominalClientTime ? l.correlationChartNominal : l.correlationTimelineClockUnknown,
                }),
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: TextButton(
                  key: ValueKey('job-timeline-choose-${marker.jobId}'),
                  onPressed: !enabled || busy || marker.notificationRevision == 0 ? null : () => onChoose(marker),
                  child: Text(l.correlationTimelineChoose),
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

}

/// Presentation of an already accepted host history, never an execution handle.
class JobMetricChartAnnotations extends StatelessWidget {
  const JobMetricChartAnnotations({super.key, required this.timeline, required this.startUtc,
    required this.endUtc, this.active = true, this.selectedJobId});
  final MonitoringCorrelationTimeline timeline;
  final DateTime startUtc, endUtc;
  final String? selectedJobId;
  final bool active;
  @override
  Widget build(BuildContext context) {
    if (!identical(timeline.owner.snapshot, timeline.snapshot)) return const SizedBox.shrink();
    final l = context.l10n;
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      Text(l.correlationChartNominal, key: const ValueKey('job-chart-clock-notice')),
      for (final marker in timeline.snapshot.jobs.markers)
        if (marker.jobId == selectedJobId)
          JobMetricTimelineEntry(key: ValueKey('job-chart-selected-${marker.jobId}'), marker: marker,
            initiallyExpanded: true, enabled: active, nominalClientTime: true,
            formatTime: (time) => jobMetricDisplayTime(time, timeline.localTime),
            onChoose: (value) {
              if (active && identical(timeline.owner.snapshot, timeline.snapshot)) timeline.openProject(value);
            }),
      ExpansionTile(key: const ValueKey('job-chart-jobs'),
        title: Text('${l.correlationTimelineTitle} (${timeline.snapshot.jobs.markers.length})'),
        children: [for (final marker in timeline.snapshot.jobs.markers) if (marker.jobId != selectedJobId) ...[
          if (marker.admitted == null)
            Text(l.correlationTimelineMissingTime)
          else if (marker.admitted!.utc.isBefore(startUtc) || marker.admitted!.utc.isAfter(endUtc))
            Text(l.correlationTimelineOutside),
          JobMetricTimelineEntry(key: ValueKey('job-chart-entry-${marker.jobId}'), marker: marker,
            enabled: active, nominalClientTime: true, formatTime: (time) => jobMetricDisplayTime(time, timeline.localTime),
            onChoose: (value) {
              if (active && identical(timeline.owner.snapshot, timeline.snapshot)) timeline.openProject(value);
            }),
        ]],
      ),
    ]);
  }
}
