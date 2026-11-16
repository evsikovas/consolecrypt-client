import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_observation.dart';
import 'package:consolecrypt/monitoring/monitoring_pinned_view.dart'
    show monitoringPinIntervalLabel, monitoringPinStatus;
import 'package:material_ui/material_ui.dart';

/// Passive controls. Edits never start a service or a read. The caller captures
/// these choices before an explicit manual Read and may then seed its result.
class MonitoringObservationControls extends StatelessWidget {
  const MonitoringObservationControls({
    super.key,
    required this.owner,
    required this.auto,
    required this.interval,
    required this.rolling,
    required this.enabled,
    required this.rollingAvailable,
    required this.onAuto,
    required this.onInterval,
    required this.onRolling,
    this.acceptedPlan,
    this.acceptedValue,
    required this.isAcceptedCurrent,
  });
  final MonitoringPinnedObservation owner;
  final bool auto, rolling, enabled, rollingAvailable;
  final MonitoringPinInterval interval;
  final ValueChanged<bool> onAuto, onRolling;
  final ValueChanged<MonitoringPinInterval> onInterval;
  final MonitoringPinPlan? acceptedPlan;
  final MonitoringObservationValue? acceptedValue;
  final bool Function(MonitoringPinPlan plan, MonitoringObservationValue value) isAcceptedCurrent;

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return StreamBuilder<MonitoringPinState>(
      key: ObjectKey(owner),
      stream: owner.changes,
      initialData: owner.state,
      builder: (context, snapshot) {
        final state = snapshot.data ?? owner.state;
        final pinned = state.active && state.visibility == MonitoringObservationVisibility.pinned;
        final foreground = state.active && state.visibility == MonitoringObservationVisibility.foregroundOnly;
        return ContentSurface(
          key: const ValueKey('monitoring-auto-controls'),
          child: ExpansionTile(
            key: const ValueKey('monitoring-auto-settings'),
            tilePadding: EdgeInsets.zero,
            title: Wrap(
              spacing: 12,
              runSpacing: 4,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                Text(l.monitoringObservationTitle, style: Theme.of(context).textTheme.titleSmall),
                Text(
                  '${state.phase != MonitoringPinPhase.idle ? monitoringPinStatus(l, state)
                      : auto
                      ? l.monitoringObservationAuto
                      : l.monitoringObservationAutoOff} · ${monitoringPinIntervalLabel(l, state.active ? state.plan!.interval : interval)}',
                  key: const ValueKey('monitoring-auto-summary'),
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ],
            ),
            children: [
              Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  CheckboxListTile(
                    key: const ValueKey('monitoring-auto-enabled'),
                    contentPadding: EdgeInsets.zero,
                    value: auto,
                    title: Text(l.monitoringObservationAuto),
                    onChanged: enabled ? (value) => onAuto(value ?? false) : null,
                  ),
                  Text(l.monitoringObservationAutoHelp),
                  const SizedBox(height: 8),
                  GlassSelect<MonitoringPinInterval>(
                    key: const ValueKey('monitoring-auto-interval'),
                    value: interval,
                    expand: true,
                    semanticLabel: l.monitoringPinInterval,
                    items: [
                      for (final value in MonitoringPinInterval.values)
                        GlassSelectItem(value: value, label: monitoringPinIntervalLabel(l, value)),
                    ],
                    onChanged: enabled ? onInterval : null,
                  ),
                  CheckboxListTile(
                    key: const ValueKey('monitoring-observation-rolling'),
                    contentPadding: EdgeInsets.zero,
                    value: rolling,
                    title: Text(l.monitoringObservationRolling),
                    subtitle: Text(rolling ? l.monitoringObservationRollingHelp : l.monitoringObservationFixed),
                    onChanged: enabled && (rollingAvailable || rolling) ? (value) => onRolling(value ?? false) : null,
                  ),
                  if (pinned && state.plan?.requestIdentity != acceptedPlan?.requestIdentity)
                    Text(l.monitoringObservationManualLimit),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      GlassButton(
                        key: const ValueKey('monitoring-observation-pin'),
                        icon: Icons.push_pin_outlined,
                        label: l.monitoringObservationPinAccepted,
                        onPressed: !enabled || pinned || acceptedPlan == null || acceptedValue == null
                            ? null
                            : () {
                                if (!isAcceptedCurrent(acceptedPlan!, acceptedValue!)) return;
                                if (foreground && state.plan?.requestIdentity == acceptedPlan!.requestIdentity) {
                                  owner.promoteForeground();
                                } else {
                                  owner.pinAccepted(acceptedPlan!, acceptedValue!);
                                }
                              },
                      ),
                      if (pinned)
                        GlassButton(
                          key: const ValueKey('monitoring-observation-unpin'),
                          label: l.monitoringObservationUnpin,
                          onPressed: owner.unpin,
                        ),
                    ],
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
