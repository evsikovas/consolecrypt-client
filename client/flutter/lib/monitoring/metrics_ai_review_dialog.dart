import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/metrics_ai_context.dart';
import 'package:consolecrypt/monitoring/metrics_ai_review_flow.dart';
import 'package:consolecrypt/monitoring/monitoring_alert_models.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:material_ui/material_ui.dart';

/// Local accepted data plus a typed projection factory owned by the current
/// workspace. No reader, endpoint, router, file writer or execution callback.
final class MetricsAiMetricInput {
  const MetricsAiMetricInput({required this.outcome, required this.select});
  final QueryOutcome outcome;
  final MetricsAiMetricSelection Function(int seriesIndex, List<int> sampleIndices) select;
}

class MetricsAiReviewDialog extends StatefulWidget {
  const MetricsAiReviewDialog({
    super.key,
    required this.fence,
    required this.transport,
    required this.isCurrent,
    required this.providerLabel,
    required this.onClose,
    this.metric,
    this.alerts,
  });
  final MetricsAiFence fence;
  final MetricsAiReviewTransport transport;
  final bool Function() isCurrent;
  final String providerLabel;
  final VoidCallback onClose;
  final MetricsAiMetricInput? metric;
  final MonitoringAlertSnapshot? alerts;
  @override
  State<MetricsAiReviewDialog> createState() => _MetricsAiReviewDialogState();
}

class _MetricsAiReviewDialogState extends State<MetricsAiReviewDialog> {
  final question = TextEditingController();
  final samples = <int, Set<int>>{};
  final selectedAlerts = <int>{};
  int series = 0;
  MetricsAiAction action = MetricsAiAction.explain;
  bool consent = false;
  MetricsAiProjectionIssue? selectionIssue;
  late MetricsAiReviewFlow flow;

  bool get current => mounted && widget.isCurrent();
  String utc(double timestamp) {
    final micros = timestamp * Duration.microsecondsPerSecond;
    if (!micros.isFinite || micros.abs() > const Duration(days: 100000000).inMicroseconds) {
      return context.l10n.monitoringChartInvalidUtc(timestamp.toString());
    }
    return DateTime.fromMicrosecondsSinceEpoch(micros.round(), isUtc: true).toIso8601String();
  }

  @override
  void initState() {
    super.initState();
    flow = MetricsAiReviewFlow(
      transport: widget.transport,
      original: widget.fence,
      isCurrent: (fence) => current && widget.fence.matches(fence),
      now: DateTime.now,
    );
    question.addListener(edited);
  }

  @override
  void didUpdateWidget(covariant MetricsAiReviewDialog oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(widget.fence, oldWidget.fence) ||
        !identical(widget.metric, oldWidget.metric) ||
        !identical(widget.alerts, oldWidget.alerts) ||
        !identical(widget.transport, oldWidget.transport)) {
      flow.invalidate();
      samples.clear();
      selectedAlerts.clear();
      consent = false;
    }
  }

  void edited() {
    if (!mounted) return;
    unawaited(flow.reset());
    setState(() {
      consent = false;
      selectionIssue = null;
    });
  }

  @override
  void dispose() {
    question.removeListener(edited);
    question.dispose();
    unawaited(flow.reset());
    flow.dispose();
    super.dispose();
  }

  Future<void> prepare() async {
    if (!current) {
      if (mounted) setState(flow.invalidate);
      return;
    }
    try {
      final metric = widget.metric;
      final projection = MetricsAiProjection.selected(
        widget.fence,
        metrics: [
          if (metric != null)
            for (final entry in samples.entries)
              if (entry.value.isNotEmpty) metric.select(entry.key, entry.value.toList()..sort()),
        ],
        alerts: [
          if (widget.alerts != null)
            for (final index in selectedAlerts)
              MetricsAiAlertSelection.fromAccepted(widget.alerts!, widget.fence, alertIndex: index),
        ],
        createdUtc: DateTime.now().toUtc(),
      );
      final future = flow.prepare(MetricsAiRequest(action: action, question: question.text, projection: projection));
      setState(() {
        selectionIssue = null;
      });
      await future;
      if (mounted) setState(() {});
    } on MetricsAiProjectionException catch (error) {
      if (mounted) setState(() => selectionIssue = error.issue);
    } on MetricsAiFlowException {
      if (mounted) setState(() => selectionIssue = MetricsAiProjectionIssue.invalidSelection);
    }
  }

  Future<void> send() async {
    final preview = flow.state.preview;
    if (!current) {
      if (mounted) setState(flow.invalidate);
      return;
    }
    if (preview == null) return;
    try {
      final approval = flow.approve(preview, approveExternal: consent);
      final future = flow.send(approval);
      setState(() {});
      await future;
      if (mounted) setState(() {});
    } on MetricsAiFlowException {
      if (mounted) setState(() {});
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, state = flow.state, data = widget.metric?.outcome.data;
    final busy = state.phase == MetricsAiPhase.preparing || state.phase == MetricsAiPhase.sending;
    final editable = current && !busy;
    final count = data == null
        ? 0
        : data.kind == 'scalar' && data.sample != null
        ? 1
        : data.series.length;
    final points = count == 0
        ? <MonitoringSample>[]
        : data!.kind == 'scalar'
        ? [data.sample!]
        : data.series[series].samples;
    final preview = state.preview, proposal = state.proposal;
    return GlassDialog(
      title: l.metricsAiTitle,
      width: 860,
      content: SizedBox(
        height: 540,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(widget.providerLabel),
              const SizedBox(height: 8),
              Text(l.metricsAiSelectionHint),
              const SizedBox(height: 8),
              Text(l.metricsAiSelectionLimit),
              if (count > 0) ...[
                const SizedBox(height: 12),
                DropdownButtonFormField<int>(
                  key: const ValueKey('metrics-ai-series'),
                  initialValue: series,
                  items: [for (var i = 0; i < count; i++) DropdownMenuItem(value: i, child: Text('${i + 1}'))],
                  onChanged: editable
                      ? (value) {
                          if (value != null) setState(() => series = value);
                        }
                      : null,
                ),
                SizedBox(
                  height: 180,
                  child: ListView.builder(
                    itemCount: points.length,
                    itemBuilder: (context, index) {
                      final point = points[index], selected = samples[series]?.contains(index) == true;
                      return CheckboxListTile(
                        dense: true,
                        key: ValueKey('metrics-ai-sample-$series-$index'),
                        title: Text('${utc(point.timestamp)} · ${point.display}'),
                        subtitle: Text(l.metricsAiSelectSample),
                        value: selected,
                        onChanged:
                            editable &&
                                (selected ||
                                    (samples[series]?.length ?? 0) < 32 &&
                                        (samples[series]?.isNotEmpty == true ||
                                            samples.values.where((values) => values.isNotEmpty).length < 8))
                            ? (value) {
                                edited();
                                setState(() {
                                  if (value == true) {
                                    samples.putIfAbsent(series, () => {}).add(index);
                                  } else {
                                    samples[series]?.remove(index);
                                  }
                                });
                              }
                            : null,
                      );
                    },
                  ),
                ),
              ],
              if (widget.alerts != null &&
                  widget.alerts!.kind == MonitoringAlertKind.alerts &&
                  widget.alerts!.alerts.isNotEmpty)
                SizedBox(
                  height: 180,
                  child: ListView.builder(
                    itemCount: widget.alerts!.alerts.length,
                    itemBuilder: (context, i) => CheckboxListTile(
                      dense: true,
                      key: ValueKey('metrics-ai-alert-$i'),
                      title: Text('${l.metricsAiSelectAlert} ${i + 1}'),
                      value: selectedAlerts.contains(i),
                      onChanged: editable && (selectedAlerts.contains(i) || selectedAlerts.length < 8)
                          ? (value) {
                              edited();
                              setState(() {
                                if (value == true) {
                                  selectedAlerts.add(i);
                                } else {
                                  selectedAlerts.remove(i);
                                }
                              });
                            }
                          : null,
                    ),
                  ),
                ),
              const SizedBox(height: 12),
              DropdownButtonFormField<MetricsAiAction>(
                key: const ValueKey('metrics-ai-action'),
                initialValue: action,
                items: [
                  DropdownMenuItem(value: MetricsAiAction.explain, child: Text(l.metricsAiExplain)),
                  DropdownMenuItem(value: MetricsAiAction.proposePlaybook, child: Text(l.metricsAiPropose)),
                ],
                onChanged: editable
                    ? (value) {
                        if (value != null) {
                          edited();
                          setState(() => action = value);
                        }
                      }
                    : null,
              ),
              TextField(
                key: const ValueKey('metrics-ai-question'),
                controller: question,
                enabled: editable,
                maxLength: 4096,
                maxLines: 3,
                decoration: InputDecoration(labelText: l.metricsAiQuestion),
              ),
              if (selectionIssue != null)
                InfoBanner(
                  key: const ValueKey('metrics-ai-selection-error'),
                  message: selectionIssue == MetricsAiProjectionIssue.noSelection
                      ? l.metricsAiNoSelection
                      : selectionIssue == MetricsAiProjectionIssue.budget ||
                            selectionIssue == MetricsAiProjectionIssue.invalidSelection
                      ? l.metricsAiSelectionLimit
                      : l.metricsAiContextChanged,
                  tone: BannerTone.danger,
                ),
              if (!current || state.phase == MetricsAiPhase.invalidated)
                InfoBanner(
                  key: const ValueKey('metrics-ai-context-error'),
                  message: l.metricsAiContextChanged,
                  tone: BannerTone.danger,
                ),
              if (state.phase == MetricsAiPhase.failed)
                InfoBanner(
                  key: const ValueKey('metrics-ai-error'),
                  message: l.metricsAiFailed,
                  tone: BannerTone.danger,
                ),
              if (busy) const LinearProgressIndicator(key: ValueKey('metrics-ai-progress')),
              if (preview != null) ...[
                Text(l.metricsAiPreview),
                ContentSurface(
                  child: SizedBox(
                    height: 200,
                    child: SingleChildScrollView(
                      child: SelectableText(preview.previewText, key: const ValueKey('metrics-ai-exact-preview')),
                    ),
                  ),
                ),
                if (preview.external)
                  CheckboxListTile(
                    key: const ValueKey('metrics-ai-consent'),
                    title: Text(l.metricsAiExternalConsent),
                    value: consent,
                    onChanged: current && state.phase == MetricsAiPhase.preview
                        ? (value) => setState(() => consent = value == true)
                        : null,
                  ),
              ],
              if (proposal != null)
                ContentSurface(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      SelectableText(proposal.explanation, key: const ValueKey('metrics-ai-explanation')),
                      if (proposal.proposedPlaybook != null)
                        SelectableText(proposal.proposedPlaybook!, key: const ValueKey('metrics-ai-playbook')),
                      for (final limitation in proposal.limitations) Text(limitation),
                    ],
                  ),
                ),
              const SizedBox(height: 12),
              Text(l.metricsAiReadonly),
            ],
          ),
        ),
      ),
      primaryAction: GlassButton(
        key: const ValueKey('metrics-ai-primary'),
        label: state.phase == MetricsAiPhase.preview ? l.aiChatSend : l.metricsAiPrepare,
        onPressed: !editable || state.phase == MetricsAiPhase.proposed
            ? null
            : state.phase == MetricsAiPhase.preview
            ? preview!.external && !consent
                  ? null
                  : send
            : prepare,
      ),
      secondaryActions: [GlassButton(label: l.commonClose, onPressed: widget.onClose)],
    );
  }
}
