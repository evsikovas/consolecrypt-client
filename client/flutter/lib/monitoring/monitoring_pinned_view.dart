import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_view.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_reader.dart';
import 'package:consolecrypt/monitoring/monitoring_errors.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_observation_result.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_providers.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Exact presentation identity without putting a raw query/configuration in
/// Widget diagnostics. This object conveys no read or execution authority.
final class _ObservationViewIdentity {
  _ObservationViewIdentity(MonitoringPinPlan plan) : _value = plan.requestIdentity;
  final String _value;
  @override
  bool operator ==(Object other) => other is _ObservationViewIdentity && _value == other._value;
  @override
  int get hashCode => _value.hashCode;
  @override
  String toString() => 'MonitoringObservationIdentity';
}

String monitoringPinIntervalLabel(AppLocalizations l, MonitoringPinInterval interval) => switch (interval) {
  MonitoringPinInterval.seconds15 => l.monitoringPin15Seconds,
  MonitoringPinInterval.seconds30 => l.monitoringPin30Seconds,
  MonitoringPinInterval.minute1 => l.monitoringPin1Minute,
  MonitoringPinInterval.minutes5 => l.monitoringPin5Minutes,
};
String monitoringPinStatus(AppLocalizations l, MonitoringPinState value) => switch (value.phase) {
  MonitoringPinPhase.idle => l.monitoringPinIdle,
  MonitoringPinPhase.checking => l.monitoringPinChecking,
  MonitoringPinPhase.reading => l.monitoringPinReading,
  MonitoringPinPhase.waiting => l.monitoringPinWaiting,
  MonitoringPinPhase.backoff => l.monitoringPinBackoff,
  MonitoringPinPhase.stopped => switch (value.stopReason) {
    MonitoringPinStop.autoDisabled => l.monitoringObservationAutoStopped,
    MonitoringPinStop.scopeChanged => l.monitoringPinScopeStopped,
    MonitoringPinStop.sourceChanged => l.monitoringPinSourceStopped,
    MonitoringPinStop.appPaused => l.monitoringPinPausedStopped,
    MonitoringPinStop.appClosed => l.monitoringPinClosedStopped,
    MonitoringPinStop.retryLimit => l.monitoringPinRetryStopped,
    MonitoringPinStop.invalidRequest => l.monitoringPinInvalid,
    MonitoringPinStop.rejected => l.monitoringPinRejected,
    _ => l.monitoringPinStopped,
  },
};

class MonitoringPinControls extends ConsumerStatefulWidget {
  const MonitoringPinControls({
    super.key,
    required this.source,
    required this.expression,
    required this.instant,
    required this.enabled,
  });
  final MonitoringSource? source;
  final String expression;
  final bool instant, enabled;
  @override
  ConsumerState<MonitoringPinControls> createState() => _MonitoringPinControlsState();
}

class _MonitoringPinControlsState extends ConsumerState<MonitoringPinControls> {
  MonitoringPinInterval interval = MonitoringPinInterval.seconds15;
  @override
  Widget build(BuildContext context) {
    final owner = ref.watch(monitoringPinnedObservationProvider), l = context.l10n;
    if (owner == null) return const SizedBox.shrink();
    return StreamBuilder<MonitoringPinState>(
      key: ObjectKey(owner),
      stream: owner.changes,
      initialData: owner.state,
      builder: (context, snapshot) {
        final state = snapshot.data ?? owner.state;
        final source = widget.source,
            canPin =
                widget.enabled &&
                widget.instant &&
                source != null &&
                !source.readOnly &&
                source.revision > 0 &&
                widget.expression.trim().isNotEmpty;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const SizedBox(height: 12),
            Text(l.monitoringPinInterval),
            GlassSelect<MonitoringPinInterval>(
              key: const ValueKey('monitoring-pin-interval'),
              value: interval,
              expand: true,
              items: [
                for (final value in MonitoringPinInterval.values)
                  GlassSelectItem(value: value, label: monitoringPinIntervalLabel(l, value)),
              ],
              onChanged: state.busy ? null : (value) => setState(() => interval = value),
            ),
            const SizedBox(height: 8),
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: GlassButton(
                key: const ValueKey('monitoring-pin-current'),
                icon: Icons.push_pin_outlined,
                label: state.active ? l.monitoringPinReplace : l.monitoringPinCurrent,
                onPressed: !canPin || state.busy
                    ? null
                    : () {
                        owner.pin(
                          MonitoringPinPlan(source: source, expression: widget.expression.trim(), interval: interval),
                        );
                      },
              ),
            ),
            if (!widget.instant) Text(l.monitoringPinInstantOnly),
            Text(l.monitoringPinMemoryOnly),
          ],
        );
      },
    );
  }
}

/// A small global status/Stop row outside hidden navigation branches. Building
/// this widget never creates a transport; only the explicit Pin action does.
class MonitoringPinGlobalBar extends ConsumerWidget {
  const MonitoringPinGlobalBar({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final owner = ref.watch(monitoringPinnedObservationProvider), l = context.l10n;
    if (owner == null) return const SizedBox.shrink();
    return StreamBuilder<MonitoringPinState>(
      key: ObjectKey(owner),
      stream: owner.changes,
      initialData: owner.state,
      builder: (context, snapshot) {
        final value = snapshot.data ?? owner.state;
        if (value.plan == null || value.visibility != MonitoringObservationVisibility.pinned) {
          return const SizedBox.shrink();
        }
        return Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
          child: ContentSurface(
            child: LayoutBuilder(
              builder: (context, bounds) => Row(
                children: [
                  const Icon(Icons.push_pin, size: 16),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      '${l.monitoringPinGlobal(value.plan!.source.name)} · ${monitoringPinStatus(l, value)}',
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  const SizedBox(width: 8),
                  if (bounds.maxWidth >= 380)
                    GlassButton(
                      key: const ValueKey('monitoring-pin-global-stop'),
                      icon: Icons.stop_circle_outlined,
                      label: l.monitoringPinStop,
                      onPressed: () => owner.stop(MonitoringPinStop.manual),
                    )
                  else
                    GlassIconButton(
                      key: const ValueKey('monitoring-pin-global-stop'),
                      icon: Icons.stop_circle_outlined,
                      tooltip: l.monitoringPinStop,
                      onPressed: () => owner.stop(MonitoringPinStop.manual),
                    ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }
}

class MonitoringPinnedView extends ConsumerWidget {
  const MonitoringPinnedView({super.key, this.active = true, this.jobTimeline});
  final MonitoringCorrelationTimeline? jobTimeline;
  final bool active;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final owner = ref.watch(monitoringPinnedObservationProvider), l = context.l10n;
    if (owner == null) return const SizedBox.shrink();
    return StreamBuilder<MonitoringPinState>(
      key: ObjectKey(owner),
      stream: owner.changes,
      initialData: owner.state,
      builder: (context, snapshot) {
        final value = snapshot.data ?? owner.state;
        if (value.phase == MonitoringPinPhase.idle || value.stopReason == MonitoringPinStop.manual) {
          return const SizedBox.shrink();
        }
        return ContentSurface(
          key: const ValueKey('monitoring-pin-view'),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                value.plan == null
                    ? l.monitoringObservationTitle
                    : l.monitoringObservationSource(value.plan!.source.name),
                style: Theme.of(context).textTheme.titleSmall,
              ),
              Text(monitoringPinStatus(l, value), key: const ValueKey('monitoring-pin-status')),
              if (value.busy) const LinearProgressIndicator(),
              if (value.plan case final plan?) ...[
                Text(l.monitoringPinActiveInterval(monitoringPinIntervalLabel(l, plan.interval))),
                if (plan.target.windowPolicy case MonitoringFixedObservationWindow())
                  Text(l.monitoringObservationFixed),
                if (plan.target.windowPolicy case MonitoringRollingObservationWindow(:final preset))
                  Text('${l.monitoringObservationRolling}: ${monitoringTimePresetLabel(l, preset)}'),
                ExpansionTile(
                  title: Text(l.monitoringObservationSnapshot),
                  children: [
                    switch (plan.target) {
                      MonitoringCustomInstantTarget(:final expression) ||
                      MonitoringCustomRangeTarget(:final expression) => SelectableText(expression),
                      MonitoringBindingPanelTarget(:final binding, :final panel) => SelectableText(
                        '${binding.name} · ${monitoringPanelLabel(l, panel)}\n${binding.id} · ${binding.revision}',
                      ),
                      MonitoringSavedPanelTarget(:final panel) => SelectableText(
                        '${panel.definition.title}\n${panel.id} · ${panel.revision}',
                      ),
                    },
                  ],
                ),
              ],
              if (value.lastSuccessUtc case final time?) Text(l.monitoringPinLastSuccess(time.toIso8601String())),
              if (value.nextDueUtc case final time?) Text(l.monitoringPinNext(time.toIso8601String())),
              if (value.error case final error?)
                InfoBanner(
                  key: const ValueKey('monitoring-pin-error'),
                  message: error is MonitoringFailure ? monitoringFailureMessage(l, error) : errorMessage(l, error),
                  tone: BannerTone.danger,
                ),
              if (value.hasAcceptedResult) ...[
                if (value.error != null ||
                    value.phase == MonitoringPinPhase.stopped && value.stopReason != MonitoringPinStop.autoDisabled)
                  Text(l.monitoringPinStale),
                MonitoringObservationResultView(
                  key: value.plan == null ? null : ValueKey(_ObservationViewIdentity(value.plan!)),
                  value: value.value ?? MonitoringObservedQuery(value.outcome!, null),
                  jobTimeline: jobTimeline,
                  active: active,
                ),
              ],
              Wrap(
                spacing: 8,
                children: [
                  if (value.active)
                    GlassButton(
                      key: const ValueKey('monitoring-pin-refresh'),
                      label: l.monitoringPinRefresh,
                      onPressed: value.busy ? null : owner.refresh,
                    ),
                  GlassButton(
                    key: const ValueKey('monitoring-pin-stop'),
                    label: l.monitoringPinStop,
                    onPressed: () => owner.stop(MonitoringPinStop.manual),
                  ),
                ],
              ),
            ],
          ),
        );
      },
    );
  }
}
