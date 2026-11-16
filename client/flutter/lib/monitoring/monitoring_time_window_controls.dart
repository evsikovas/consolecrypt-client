import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/monitoring_chart_viewport.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';

/// One controller belongs to the current profile's workspace. Draft edits
/// notify consumers to cancel stale reads; neither editing nor Apply queries.
final class MonitoringTimeWindowController extends ChangeNotifier {
  MonitoringTimeWindowController({MonitoringTimeWindow? initial, DateTime Function()? now})
    : now = now ?? DateTime.now {
    _applied = initial ?? MonitoringTimeWindow.preset(MonitoringTimePreset.hour1, this.now().toUtc());
    _viewport = ValueNotifier(_applied);
    if (initial != null) preset = MonitoringTimePreset.custom;
    _copyApplied();
  }
  final DateTime Function() now;
  late MonitoringTimeWindow _applied;
  MonitoringTimeWindow get applied => _applied;
  late final ValueNotifier<MonitoringTimeWindow> _viewport;

  /// A separate presentation channel. It must not invalidate reads or mutate
  /// applied/draft/revision when a chart is zoomed or panned.
  ValueListenable<MonitoringTimeWindow> get viewport => _viewport;
  bool _disposed = false;
  bool get canView => !_disposed && !dirty;
  bool get canZoomIn => canView && MonitoringChartViewport.zoom(applied, viewport.value, .5) != viewport.value;
  bool get canZoomOut => canView && viewport.value != applied;
  bool get canPanEarlier => canView && viewport.value.startUtc.isAfter(applied.startUtc);
  bool get canPanLater => canView && viewport.value.endUtc.isBefore(applied.endUtc);
  bool zoom(double factor, {double anchor = .5}) =>
      _view(MonitoringChartViewport.zoom(applied, viewport.value, factor, anchor: anchor));
  bool pan(double fraction) => _view(MonitoringChartViewport.pan(applied, viewport.value, fraction));
  bool resetViewport() => _view(applied);
  bool _view(MonitoringTimeWindow? next) {
    if (!canView || next == null || next == viewport.value) return false;
    _viewport.value = next;
    return true;
  }

  late String draftStart, draftEnd, draftStep;
  MonitoringTimePreset preset = MonitoringTimePreset.hour1;
  MonitoringTimeWindowError? error;
  bool dirty = false;
  int revision = 0;
  MonitoringTimeWindow get requested {
    if (dirty) throw const MonitoringTimeWindowFailure(MonitoringTimeWindowError.notApplied);
    return applied;
  }

  void _copyApplied() {
    draftStart = applied.startText;
    draftEnd = applied.endText;
    draftStep = applied.stepSeconds.toString();
  }

  void edit({String? start, String? end, String? step}) {
    _viewport.value = applied;
    draftStart = start ?? draftStart;
    draftEnd = end ?? draftEnd;
    draftStep = step ?? draftStep;
    preset = MonitoringTimePreset.custom;
    dirty = true;
    error = null;
    revision++;
    notifyListeners();
  }

  void select(MonitoringTimePreset value) {
    _viewport.value = applied;
    if (value != MonitoringTimePreset.custom) {
      final next = MonitoringTimeWindow.preset(value, now().toUtc(), stepSeconds: applied.stepSeconds);
      draftStart = next.startText;
      draftEnd = next.endText;
    }
    preset = value;
    dirty = true;
    error = null;
    revision++;
    notifyListeners();
  }

  bool apply() {
    try {
      _applied = MonitoringTimeWindow.parse(start: draftStart, end: draftEnd, step: draftStep);
      dirty = false;
      error = null;
      _copyApplied();
    } on MonitoringTimeWindowFailure catch (failure) {
      dirty = true;
      error = failure.reason;
    }
    revision++;
    _viewport.value = applied;
    notifyListeners();
    return !dirty;
  }

  /// History restores the original absolute domain, never a new relative now.
  void restore(MonitoringTimeWindow window) {
    _applied = window;
    _copyApplied();
    dirty = false;
    error = null;
    preset = MonitoringTimePreset.custom;
    revision++;
    _viewport.value = applied;
    notifyListeners();
  }

  /// A passive observation renderer adopts its newly accepted loaded domain.
  /// This is not Apply and is never used by a workspace draft/query owner.
  /// Full view follows explicit rolling bounds; zoom retains absolute UTC when
  /// possible and clamps at the new loaded edge, without issuing another read.
  void presentAcceptedWindow(MonitoringTimeWindow window) {
    final oldBase = applied, oldView = viewport.value;
    if (_disposed || oldBase == window) return;
    final nextView = MonitoringChartViewport.rebase(oldBase, oldView, window);
    _applied = window;
    _copyApplied();
    dirty = false;
    error = null;
    revision++;
    _viewport.value = nextView;
    notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _viewport.dispose();
    super.dispose();
  }
}

String monitoringTimePresetLabel(AppLocalizations l, MonitoringTimePreset value) => switch (value) {
  MonitoringTimePreset.minutes15 => l.monitoringTime15Minutes,
  MonitoringTimePreset.hour1 => l.monitoringTime1Hour,
  MonitoringTimePreset.hours6 => l.monitoringTime6Hours,
  MonitoringTimePreset.day1 => l.monitoringTime1Day,
  MonitoringTimePreset.days7 => l.monitoringTime7Days,
  MonitoringTimePreset.custom => l.monitoringTimeCustom,
};
String monitoringTimeWindowMessage(AppLocalizations l, MonitoringTimeWindowError error) => switch (error) {
  MonitoringTimeWindowError.invalidStart => l.monitoringTimeInvalidStart,
  MonitoringTimeWindowError.invalidEnd => l.monitoringTimeInvalidEnd,
  MonitoringTimeWindowError.invalidOrder => l.monitoringTimeInvalidOrder,
  MonitoringTimeWindowError.invalidStep => l.monitoringTimeInvalidStep,
  MonitoringTimeWindowError.notApplied => l.monitoringTimeUnapplied,
};

class MonitoringTimeWindowControls extends StatefulWidget {
  const MonitoringTimeWindowControls({
    super.key,
    required this.controller,
    this.enabled = true,
    this.keyPrefix = 'monitoring-time',
  });
  final MonitoringTimeWindowController controller;
  final bool enabled;
  final String keyPrefix;
  @override
  State<MonitoringTimeWindowControls> createState() => _MonitoringTimeWindowControlsState();
}

class _MonitoringTimeWindowControlsState extends State<MonitoringTimeWindowControls> {
  final start = TextEditingController(), end = TextEditingController(), step = TextEditingController();
  @override
  void initState() {
    super.initState();
    widget.controller.addListener(sync);
    sync();
  }

  @override
  void didUpdateWidget(MonitoringTimeWindowControls oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller) {
      oldWidget.controller.removeListener(sync);
      widget.controller.addListener(sync);
      sync();
    }
  }

  void sync() {
    final value = widget.controller;
    for (final field in [(start, value.draftStart), (end, value.draftEnd), (step, value.draftStep)]) {
      if (field.$1.text != field.$2) field.$1.text = field.$2;
    }
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    widget.controller.removeListener(sync);
    start.dispose();
    end.dispose();
    step.dispose();
    super.dispose();
  }

  Widget field(String suffix, String label, TextEditingController text, ValueChanged<String> change) => TextField(
    key: ValueKey('${widget.keyPrefix}-$suffix'),
    controller: text,
    enabled: widget.enabled,
    autocorrect: false,
    enableSuggestions: false,
    onChanged: change,
    decoration: InputDecoration(labelText: label),
  );
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, value = widget.controller;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.monitoringTimeWindow, style: Theme.of(context).textTheme.titleSmall),
        Text(l.monitoringTimeSharedHint),
        const SizedBox(height: 8),
        GlassSelect<MonitoringTimePreset>(
          key: ValueKey('${widget.keyPrefix}-preset'),
          expand: true,
          value: value.preset,
          semanticLabel: l.monitoringTimePreset,
          items: [
            for (final item in MonitoringTimePreset.values)
              GlassSelectItem(value: item, label: monitoringTimePresetLabel(l, item)),
          ],
          onChanged: widget.enabled ? value.select : null,
        ),
        if (value.preset != MonitoringTimePreset.custom) ...[
          const SizedBox(height: 8),
          Align(
            alignment: Alignment.centerLeft,
            child: GlassButton.plain(
              key: ValueKey('${widget.keyPrefix}-now'),
              label: l.monitoringTimeNow,
              size: GlassControlSize.lg,
              onPressed: widget.enabled ? () => value.select(value.preset) : null,
            ),
          ),
        ],
        const SizedBox(height: 8),
        LayoutBuilder(
          builder: (_, size) {
            final fields = [
              field('start', l.monitoringTimeStart, start, (text) => value.edit(start: text)),
              field('end', l.monitoringTimeEnd, end, (text) => value.edit(end: text)),
            ];
            return size.maxWidth >= 650
                ? Row(
                    children: [
                      Expanded(child: fields[0]),
                      const SizedBox(width: 12),
                      Expanded(child: fields[1]),
                    ],
                  )
                : Column(children: [fields[0], const SizedBox(height: 8), fields[1]]);
          },
        ),
        const SizedBox(height: 8),
        field('step', l.monitoringStep, step, (text) => value.edit(step: text)),
        if (value.error case final error?)
          Text(
            monitoringTimeWindowMessage(l, error),
            key: ValueKey('${widget.keyPrefix}-error'),
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        if (value.dirty) Text(l.monitoringTimeUnapplied, key: ValueKey('${widget.keyPrefix}-dirty')),
        const SizedBox(height: 8),
        Align(
          alignment: Alignment.centerLeft,
          child: GlassButton(
            key: ValueKey('${widget.keyPrefix}-apply-window'),
            label: l.monitoringTimeApply,
            size: GlassControlSize.lg,
            onPressed: widget.enabled && value.dirty ? value.apply : null,
          ),
        ),
      ],
    );
  }
}
