import 'dart:math' as math;

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/job_metric_correlation.dart';
import 'package:consolecrypt/monitoring/job_metric_timeline_view.dart';
import 'package:consolecrypt/monitoring/monitoring_binding_models.dart';
import 'package:consolecrypt/monitoring/monitoring_chart_model.dart';
import 'package:consolecrypt/monitoring/monitoring_errors.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window.dart';
import 'package:consolecrypt/monitoring/monitoring_time_window_controls.dart';
import 'package:consolecrypt/monitoring/monitoring_unit_labels.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

class MonitoringResultView extends StatefulWidget {
  const MonitoringResultView({
    super.key,
    required this.result,
    this.jobTimeline,
    this.requestedWindow,
    this.unit,
    this.timeWindow,
    this.active = true,
  });
  final QueryOutcome result;
  final MonitoringCorrelationTimeline? jobTimeline;
  final MonitoringTimeWindow? requestedWindow;
  final MonitoringTimeWindowController? timeWindow;
  final bool active;

  /// Only an already validated Native panel can attach a unit. PromQL labels
  /// and expression text do not supply unit metadata.
  final MonitoringUnit? unit;
  @override
  State<MonitoringResultView> createState() => _MonitoringResultViewState();
}

class _MonitoringResultViewState extends State<MonitoringResultView> {
  int selected = 0;
  late List<MonitoringChartSeries> rows;
  Set<String> hidden = {};
  late MonitoringChartProjection projection;
  MonitoringChartInspection? inspection;
  bool chartFocused = false;
  String? inspectedJobId;
  AppLocalizations get l => context.l10n;
  String get unitLabel => widget.unit == null ? l.monitoringChartUnknownUnit : monitoringUnitLabel(l, widget.unit!);
  bool get sharedViewport => widget.timeWindow?.canView == true && widget.timeWindow!.applied == widget.requestedWindow;
  MonitoringTimeWindow? get chartWindow => sharedViewport ? widget.timeWindow!.viewport.value : widget.requestedWindow;
  @override
  void initState() {
    super.initState();
    rows = monitoringChartSeries(widget.result.data?.series ?? const []);
    widget.timeWindow?.viewport.addListener(viewportChanged);
    rebuildProjection();
  }

  void rebuildProjection() {
    projection = MonitoringChartProjection(
      series: rows,
      hidden: hidden,
      requestedWindow: chartWindow,
      effectiveStep: widget.result.effectiveStepSeconds,
    );
    inspection = null;
    inspectedJobId = null;
  }

  @override
  void didUpdateWidget(MonitoringResultView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.timeWindow != widget.timeWindow) {
      oldWidget.timeWindow?.viewport.removeListener(viewportChanged);
      widget.timeWindow?.viewport.addListener(viewportChanged);
    }
    if (!identical(oldWidget.jobTimeline?.snapshot, widget.jobTimeline?.snapshot)) inspectedJobId = null;
    if (!identical(oldWidget.result, widget.result) ||
        oldWidget.requestedWindow != widget.requestedWindow ||
        oldWidget.unit != widget.unit ||
        oldWidget.timeWindow != widget.timeWindow) {
      final previous = rows, selectedRow = rows.elementAtOrNull(selected);
      rows = monitoringChartSeries(widget.result.data?.series ?? const []);
      hidden = monitoringRetainedHidden(hidden, previous, rows);
      selected = selectedRow == null || selectedRow.ambiguousIdentity
          ? 0
          : rows.indexWhere((r) => r.id == selectedRow.id);
      if (selected < 0) selected = 0;
      rebuildProjection();
    }
  }

  void viewportChanged() {
    if (mounted) setState(rebuildProjection);
  }

  @override
  void dispose() {
    widget.timeWindow?.viewport.removeListener(viewportChanged);
    super.dispose();
  }

  Widget zoomControls() {
    final controller = widget.timeWindow!, visible = controller.viewport.value;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          l.monitoringChartViewport(visible.startText, visible.endText),
          key: const ValueKey('monitoring-chart-viewport'),
        ),
        Wrap(
          spacing: 6,
          runSpacing: 6,
          children: [
            GlassButton(
              key: const ValueKey('monitoring-chart-zoom-in'),
              icon: Icons.zoom_in,
              label: l.monitoringChartZoomIn,
              onPressed: controller.canZoomIn ? () => controller.zoom(.5) : null,
            ),
            GlassButton(
              key: const ValueKey('monitoring-chart-zoom-out'),
              icon: Icons.zoom_out,
              label: l.monitoringChartZoomOut,
              onPressed: controller.canZoomOut ? () => controller.zoom(2) : null,
            ),
            GlassButton(
              key: const ValueKey('monitoring-chart-pan-earlier'),
              icon: Icons.chevron_left,
              label: l.monitoringChartPanEarlier,
              onPressed: controller.canPanEarlier ? () => controller.pan(-.25) : null,
            ),
            GlassButton(
              key: const ValueKey('monitoring-chart-pan-later'),
              icon: Icons.chevron_right,
              label: l.monitoringChartPanLater,
              onPressed: controller.canPanLater ? () => controller.pan(.25) : null,
            ),
            GlassButton(
              key: const ValueKey('monitoring-chart-reset-zoom'),
              icon: Icons.restart_alt,
              label: l.monitoringChartResetZoom,
              onPressed: controller.canZoomOut ? controller.resetViewport : null,
            ),
          ],
        ),
        Row(
          children: [
            Expanded(child: Text(l.monitoringChartLoadedOnly)),
            Tooltip(message: l.monitoringChartZoomShortcuts, child: const Icon(Icons.info_outline, size: 18)),
          ],
        ),
        const SizedBox(height: 8),
      ],
    );
  }

  String rowLabel(MonitoringChartSeries row) {
    final label = row.source.label.isEmpty ? l.monitoringSeries((rows.indexOf(row) + 1).toString()) : row.source.label;
    return row.ambiguousIdentity ? l.monitoringChartDuplicateSeries(label, row.occurrence.toString()) : label;
  }

  MonitoringCorrelationTimeline? get currentJobTimeline {
    final value = widget.jobTimeline, window = widget.requestedWindow;
    return value != null && identical(value.owner.snapshot, value.snapshot) && window != null &&
      value.window == window
      ? value : null;
  }
  String utc(double timestamp) {
    final micros = timestamp * Duration.microsecondsPerSecond;
    if (!micros.isFinite || micros.abs() > const Duration(days: 100000000).inMicroseconds) {
      return l.monitoringChartInvalidUtc(timestamp.toString());
    }
    return jobMetricDisplayTime(DateTime.fromMicrosecondsSinceEpoch(micros.round(), isUtc: true),
      currentJobTimeline?.localTime ?? false);
  }

  Color rowColor(MonitoringChartSeries row) {
    final colors = Theme.of(context).colorScheme;
    final palette = [colors.primary, colors.secondary, colors.tertiary, colors.error];
    final hash = row.id.codeUnits.fold<int>(0, (sum, code) => (sum * 31 + code) & 0x7fffffff);
    return palette[hash % palette.length];
  }

  void toggle(MonitoringChartSeries row, bool visible) => setState(() {
    hidden = {...hidden};
    if (visible) {
      hidden.remove(row.id);
    } else {
      hidden.add(row.id);
    }
    rebuildProjection();
  });

  Widget chart() {
    final domain = projection.domain, values = projection.valueRange;
    final colors = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.monitoringChartUnit(unitLabel), key: const ValueKey('monitoring-chart-unit')),
        const SizedBox(height: 8),
        if (sharedViewport && widget.active) zoomControls(),
        if (domain != null) ...[
          Semantics(
            label: l.monitoringChartInteractionHelp,
            child: Focus(
              key: const ValueKey('monitoring-chart-focus'),
              canRequestFocus: widget.active,
              skipTraversal: !widget.active,
              onFocusChange: (focused) => setState(() => chartFocused = focused),
              onKeyEvent: (_, event) {
                if (!widget.active) return KeyEventResult.ignored;
                if (event is! KeyDownEvent && event is! KeyRepeatEvent) return KeyEventResult.ignored;
                if (sharedViewport && HardwareKeyboard.instance.isControlPressed) {
                  final key = event.logicalKey, controller = widget.timeWindow!;
                  if (key == LogicalKeyboardKey.equal || key == LogicalKeyboardKey.numpadAdd) {
                    controller.zoom(.5);
                    return KeyEventResult.handled;
                  }
                  if (key == LogicalKeyboardKey.minus || key == LogicalKeyboardKey.numpadSubtract) {
                    controller.zoom(2);
                    return KeyEventResult.handled;
                  }
                  if (key == LogicalKeyboardKey.digit0 || key == LogicalKeyboardKey.numpad0) {
                    controller.resetViewport();
                    return KeyEventResult.handled;
                  }
                  if (key == LogicalKeyboardKey.arrowLeft || key == LogicalKeyboardKey.arrowRight) {
                    controller.pan(key == LogicalKeyboardKey.arrowLeft ? -.25 : .25);
                    return KeyEventResult.handled;
                  }
                  return KeyEventResult.ignored;
                }
                if (event.logicalKey == LogicalKeyboardKey.escape) {
                  setState(() => inspection = null);
                  return KeyEventResult.handled;
                }
                final direction = event.logicalKey == LogicalKeyboardKey.arrowLeft
                    ? -1
                    : event.logicalKey == LogicalKeyboardKey.arrowRight
                    ? 1
                    : 0;
                if (direction == 0) return KeyEventResult.ignored;
                setState(() => inspection = projection.move(inspection?.timestamp, direction));
                return KeyEventResult.handled;
              },
              child: LayoutBuilder(
                builder: (context, constraints) {
                  void inspect(Offset point) {
                    setState(
                      () => inspection = projection.inspect(
                        (point.dx - 8) / (constraints.maxWidth - 16),
                        toleranceFraction: 10 / (constraints.maxWidth - 16),
                      ),
                    );
                  }

                  return Listener(
                    onPointerSignal: (event) {
                      if (event is! PointerScrollEvent ||
                          !widget.active ||
                          !sharedViewport ||
                          !HardwareKeyboard.instance.isControlPressed ||
                          event.scrollDelta.dy == 0) {
                        return;
                      }
                      GestureBinding.instance.pointerSignalResolver.register(event, (_) {
                        if (!mounted || !widget.active || !sharedViewport) return;
                        widget.timeWindow!.zoom(
                          math.exp(event.scrollDelta.dy.clamp(-240, 240) / 240),
                          anchor: (event.localPosition.dx - 8) / (constraints.maxWidth - 16),
                        );
                      });
                    },
                    child: MouseRegion(
                      onHover: widget.active ? (event) => inspect(event.localPosition) : null,
                      child: GestureDetector(
                        key: const ValueKey('monitoring-chart'),
                        behavior: HitTestBehavior.opaque,
                        onTapDown: (event) {
                          if (!widget.active) return;
                          Focus.of(context).requestFocus();
                          inspect(event.localPosition);
                          final timeline = currentJobTimeline;
                          if (timeline == null) return;
                          final hits = timeline.snapshot.jobs.markers.where((marker) {
                            final stamp = marker.admitted;
                            if (stamp == null) return false;
                            final time = stamp.utc.microsecondsSinceEpoch / Duration.microsecondsPerSecond;
                            if (!domain.contains(time)) return false;
                            final x = 8 + domain.fraction(time) * (constraints.maxWidth - 16);
                            return (x - event.localPosition.dx).abs() <= 10;
                          }).toList();
                          // Overlapping jobs stay individually available in the
                          // job list; never silently select an arbitrary one.
                          setState(() => inspectedJobId = hits.length == 1 ? hits.single.jobId : null);
                        },
                        child: DecoratedBox(
                          decoration: BoxDecoration(
                            color: colors.surfaceContainerHighest,
                            border: Border.all(
                              color: chartFocused ? colors.primary : colors.outlineVariant,
                              width: chartFocused ? 1.5 : 1,
                            ),
                            borderRadius: BorderRadius.circular(8),
                          ),
                          child: SizedBox(
                            height: 180,
                            child: CustomPaint(
                              key: const ValueKey('monitoring-chart-paint'),
                              painter: _SeriesPainter(
                                projection,
                                {for (final row in rows) row.id: rowColor(row)},
                                colors.outlineVariant,
                                inspection?.timestamp,
                                [for (final marker in currentJobTimeline?.snapshot.jobs.markers ?? const <JobMetricMarker>[])
                                  if (marker.admitted != null && domain.contains(marker.admitted!.utc.microsecondsSinceEpoch / Duration.microsecondsPerSecond))
                                    marker.admitted!.utc.microsecondsSinceEpoch / Duration.microsecondsPerSecond],
                                colors.tertiary,

                              ),
                            ),
                          ),
                        ),
                      ),
                    ),
                  );
                },
              ),
            ),
          ),
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Expanded(
                child: Tooltip(
                  message: utc(domain.start),
                  child: Text(utc(domain.start), maxLines: 1, overflow: TextOverflow.ellipsis),
                ),
              ),
              Expanded(
                child: Tooltip(
                  message: utc(domain.end),
                  child: Text(utc(domain.end), textAlign: TextAlign.end, maxLines: 1, overflow: TextOverflow.ellipsis),
                ),
              ),
            ],
          ),
          if (values != null)
            Text(
              l.monitoringChartValueRange(values.low.toString(), values.high.toString(), unitLabel),
              key: const ValueKey('monitoring-chart-value-range'),
            ),
        ],
        if (currentJobTimeline case final timeline? when domain != null)
          JobMetricChartAnnotations(timeline: timeline,
            startUtc: DateTime.fromMicrosecondsSinceEpoch((domain.start * Duration.microsecondsPerSecond).round(), isUtc: true),
            endUtc: DateTime.fromMicrosecondsSinceEpoch((domain.end * Duration.microsecondsPerSecond).round(), isUtc: true),
            active: widget.active, selectedJobId: inspectedJobId),
        if (projection.visible.isEmpty)
          Text(l.monitoringChartAllHidden, key: const ValueKey('monitoring-chart-all-hidden'))
        else if (values == null)
          Text(l.monitoringChartNoFinite, key: const ValueKey('monitoring-chart-no-finite')),
        Text(l.monitoringChartInteractionHelp),
        if (inspection case final current?) ...[
          const SizedBox(height: 8),
          ContentSurface(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        currentJobTimeline?.localTime == true
                          ? '${l.correlationLocalTime}: ${utc(current.timestamp)}'
                          : l.monitoringChartInspectionUtc(utc(current.timestamp)),
                        key: const ValueKey('monitoring-chart-tooltip-time'),
                      ),
                    ),
                    IconButton(
                      key: const ValueKey('monitoring-chart-clear-inspection'),
                      tooltip: l.commonClose,
                      onPressed: () => setState(() => inspection = null),
                      icon: const Icon(Icons.close),
                    ),
                  ],
                ),
                if (!current.snapped) Text(l.monitoringChartNoSampleNearCursor),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 160),
                  child: SingleChildScrollView(
                    child: Column(
                      children: [
                        for (final value in current.rows)
                          Padding(
                            padding: const EdgeInsets.symmetric(vertical: 4),
                            child: Row(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Icon(Icons.circle, size: 10, color: rowColor(value.series)),
                                const SizedBox(width: 6),
                                Expanded(child: Text(rowLabel(value.series))),
                                const SizedBox(width: 8),
                                Flexible(
                                  child: Text(
                                    value.samples.isEmpty
                                        ? l.monitoringChartMissingValue
                                        : value.samples.length > 1
                                        ? l.monitoringChartDuplicateSamples(value.samples.length.toString())
                                        : '${value.samples.single.display} $unitLabel',
                                    key: ValueKey('monitoring-chart-tooltip-${value.series.id}'),
                                  ),
                                ),
                              ],
                            ),
                          ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
          ),
        ],
        const SizedBox(height: 8),
        Row(
          children: [
            Expanded(
              child: Text(l.monitoringChartVisible(projection.visible.length.toString(), rows.length.toString())),
            ),
            TextButton(
              key: const ValueKey('monitoring-chart-show-all'),
              onPressed: hidden.isEmpty
                  ? null
                  : () => setState(() {
                      hidden = {};
                      rebuildProjection();
                    }),
              child: Text(l.monitoringChartShowAll),
            ),
          ],
        ),
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 168),
          child: SingleChildScrollView(
            key: const ValueKey('monitoring-chart-legend'),
            child: Column(
              children: [
                for (final row in rows)
                  GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onTap: () => toggle(row, hidden.contains(row.id)),
                    child: MergeSemantics(
                      child: Row(
                        children: [
                          Checkbox(
                            key: ValueKey('monitoring-chart-toggle-${row.id}'),
                            value: !hidden.contains(row.id),
                            onChanged: (visible) => toggle(row, visible!),
                          ),
                          Icon(Icons.circle, size: 10, color: rowColor(row)),
                          const SizedBox(width: 6),
                          Expanded(
                            child: Tooltip(
                              message: rowLabel(row),
                              child: Text(rowLabel(row), maxLines: 1, overflow: TextOverflow.ellipsis),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ),
        Text(l.monitoringGaps),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final result = widget.result, data = widget.result.data;
    final series = data?.series.elementAtOrNull(selected);
    return ContentSurface(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            l.monitoringResultStatus(
              result.status == 'success' ? l.monitoringSuccess : l.monitoringError,
              result.httpStatus.toString(),
            ),
            key: const ValueKey('monitoring-result-status'),
            style: Theme.of(context).textTheme.titleMedium,
          ),
          if (result.partial) Text(l.monitoringPartialResult, key: const ValueKey('monitoring-partial')),
          if (result.error != null) SelectableText(monitoringFailureText(context, result.error!)),
          for (final warning in result.warnings) Text(l.monitoringWarning(warning)),
          for (final info in result.infos) Text(l.monitoringInfo(info)),
          if (result.effectiveStepSeconds != null)
            Text(l.monitoringEffectiveStep(result.effectiveStepSeconds.toString())),
          if (widget.requestedWindow case final window?)
            Text(
              l.monitoringTimeAxisRange(window.startText, window.endText),
              key: const ValueKey('monitoring-requested-axis'),
            ),
          if (data?.empty == true) Text(l.monitoringNoData, key: const ValueKey('monitoring-no-data')),
          if (data?.kind == 'string') SelectableText('${data!.timestamp}: ${data.text}'),
          if (data?.sample case final sample?) SelectableText('${sample.timestamp}: ${sample.display}'),
          if (series != null) ...[
            const SizedBox(height: 12),
            if (data!.kind == 'matrix') chart(),
            const SizedBox(height: 12),
            Text(l.monitoringChartSampleTable),
            GlassSelect<int>(
              key: const ValueKey('monitoring-table-series'),
              value: selected,
              expand: true,
              items: [for (var i = 0; i < data.series.length; i++) GlassSelectItem(value: i, label: rowLabel(rows[i]))],
              onChanged: (i) => setState(() => selected = i),
            ),
            const SizedBox(height: 12),
            Text(l.monitoringSamples(series.samples.length.toString())),
            SizedBox(
              height: 220,
              child: ListView.builder(
                key: const ValueKey('monitoring-samples'),
                itemCount: series.samples.length,
                itemBuilder: (_, index) {
                  final sample = series.samples[index];
                  return ListTile(dense: true, title: Text(sample.display), subtitle: Text(utc(sample.timestamp)));
                },
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class _SeriesPainter extends CustomPainter {
  _SeriesPainter(this.projection, this.colors, this.gridColor, this.inspectedTimestamp, this.admissions, this.jobColor);
  final MonitoringChartProjection projection;
  final Map<String, Color> colors;
  final Color gridColor;
  final double? inspectedTimestamp;
  final List<double> admissions;
  final Color jobColor;
  @override
  void paint(Canvas canvas, Size size) {
    final domain = projection.domain, values = projection.valueRange;
    if (domain == null) return;
    canvas.save();
    canvas.clipRRect(RRect.fromRectAndRadius(Offset.zero & size, const Radius.circular(8)));
    final grid = Paint()
      ..color = gridColor
      ..strokeWidth = 0.5;
    for (final fraction in [0.0, 0.5, 1.0]) {
      final y = 8 + fraction * (size.height - 16);
      canvas.drawLine(Offset(8, y), Offset(size.width - 8, y), grid);
    }
    Offset position(MonitoringSample p) => Offset(
      8 + domain.fraction(p.timestamp) * (size.width - 16),
      size.height - 8 - values!.fraction(p.value!) * (size.height - 16),
    );
    if (values != null) {
      for (final row in projection.visible) {
        final color = colors[row.id]!;
        final paint = Paint()
          ..color = color
          ..strokeWidth = 2
          ..style = PaintingStyle.stroke;
        for (final segment in projection.segments(row)) {
          final path = Path();
          for (var i = 0; i < segment.length; i++) {
            final point = position(segment[i]);
            if (i == 0) {
              path.moveTo(point.dx, point.dy);
            } else {
              path.lineTo(point.dx, point.dy);
            }
            canvas.drawCircle(point, 2, Paint()..color = color);
          }
          canvas.drawPath(path, paint);
        }
      }
    }
    // Nominal client-wall admission, not a clock-corrected execution interval.
    // Keep this distinct from finite metric samples and from the hover cursor.
    for (final time in admissions) {
      if (time < domain.start || time > domain.end) continue;
      final x = 8 + domain.fraction(time) * (size.width - 16);
      final paint = Paint()..color = jobColor..strokeWidth = 1.5;
      for (double y = 8; y < size.height - 8; y += 8) {
        canvas.drawLine(Offset(x, y), Offset(x, math.min(y + 4, size.height - 8)), paint);
      }
      canvas.drawCircle(Offset(x, 8), 3, paint);
    }
    if (inspectedTimestamp case final time?) {
      final x = 8 + domain.fraction(time) * (size.width - 16);
      canvas.drawLine(
        Offset(x, 8),
        Offset(x, size.height - 8),
        Paint()
          ..color = gridColor
          ..strokeWidth = 1,
      );
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(_SeriesPainter oldDelegate) =>
      !identical(projection, oldDelegate.projection) ||
      jobColor != oldDelegate.jobColor || admissions.length != oldDelegate.admissions.length ||
      admissions.indexed.any((entry) => oldDelegate.admissions[entry.$1] != entry.$2) ||
      gridColor != oldDelegate.gridColor ||
      colors.entries.any((entry) => oldDelegate.colors[entry.key] != entry.value) ||
      inspectedTimestamp != oldDelegate.inspectedTimestamp;
}

String monitoringFailureText(BuildContext context, MonitoringFailure failure) =>
    monitoringFailureMessage(context.l10n, failure);
