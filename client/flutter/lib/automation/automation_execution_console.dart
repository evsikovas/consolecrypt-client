import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_event_view.dart';
import 'package:consolecrypt/automation/automation_job_admission_audit.dart';
import 'package:consolecrypt/automation/automation_job_result.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/monitoring/job_metrics_chooser.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Read-only, job-scoped journal. It never attaches to a shell or sends stdin.
class AutomationExecutionConsole extends ConsumerStatefulWidget {
  const AutomationExecutionConsole({
    super.key,
    required this.scope,
    required this.service,
    required this.jobId,
    required this.collapsed,
    required this.onToggle,
    required this.onHistory,
    this.onClose,
  });
  static const double collapsedHeight = 36;
  final AutomationScope scope;
  final AutomationJobService service;
  final String jobId;
  final bool collapsed;
  final VoidCallback onToggle, onHistory;
  final VoidCallback? onClose;
  @override
  ConsumerState<AutomationExecutionConsole> createState() => _AutomationExecutionConsoleState();
}

class _AutomationExecutionConsoleState extends ConsumerState<AutomationExecutionConsole> {
  AutomationJson? job;
  List<AutomationJson> events = [];
  AutomationJobResultViewState resultState = AutomationJobResultViewState();
  final outputScroll = ScrollController(keepScrollOffset: false);
  final technicalScroll = ScrollController(keepScrollOffset: false);
  AutomationEventViewState technicalState = AutomationEventViewState();
  final savedOffsets = <double>[0, 0];
  int selectedTab = 0;
  ScrollController get scroll => outputScroll;
  Timer? timer;
  Object? error;
  bool fetching = false, initial = true, follow = true, truncated = false, visible = false;
  int cursor = 0, requestGeneration = 0;
  bool sourceArtifactsRetired = false;
  bool terminalDrained = false;
  bool get terminal => const {
    'succeeded',
    'successful',
    'failed',
    'cancelled',
    'interrupted',
    'acknowledged',
    'unknown',
  }.contains(job?['state']);
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (current && visible && !widget.collapsed) reload();
    });
    timer = Timer.periodic(const Duration(seconds: 2), (_) {
      if (visible && !widget.collapsed) reload();
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final wasVisible = visible;
    visible = TickerMode.valuesOf(context).enabled;
    if (!wasVisible && visible && !widget.collapsed) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (current) reload();
      });
    }
  }

  @override
  void didUpdateWidget(AutomationExecutionConsole oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.scope, widget.scope) ||
        !identical(oldWidget.service, widget.service) ||
        oldWidget.jobId != widget.jobId) {
      requestGeneration++;
      fetching = false;
      initial = true;
      terminalDrained = false;
      sourceArtifactsRetired = false;
      job = null;
      events = [];
      cursor = 0;
      error = null;
      truncated = false;
      follow = true;
      selectedTab = 0;
      resultState = AutomationJobResultViewState();
      technicalState = AutomationEventViewState();
      savedOffsets.fillRange(0, 2, 0);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (current && visible && !widget.collapsed) reload();
      });
    }
    if (!oldWidget.collapsed && widget.collapsed) {
      if (outputScroll.hasClients) savedOffsets[0] = outputScroll.position.pixels;
      if (technicalScroll.hasClients) savedOffsets[1] = technicalScroll.position.pixels;
    }
    if (oldWidget.collapsed && !widget.collapsed) {
      reload();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!current) return;
        if (outputScroll.hasClients) {
          outputScroll.jumpTo(savedOffsets[0].clamp(0, outputScroll.position.maxScrollExtent));
        }
        if (technicalScroll.hasClients) {
          technicalScroll.jumpTo(savedOffsets[1].clamp(0, technicalScroll.position.maxScrollExtent));
        }
      });
    }
  }

  @override
  void dispose() {
    timer?.cancel();
    outputScroll.dispose();
    technicalScroll.dispose();
    super.dispose();
  }

  Future<void> reload() async {
    if (fetching || !current || !visible || widget.collapsed || terminalDrained) return;
    fetching = true;
    final originalScope = widget.scope,
        originalService = widget.service,
        originalJobId = widget.jobId,
        request = requestGeneration;
    bool original() =>
        current &&
        request == requestGeneration &&
        identical(widget.scope, originalScope) &&
        identical(widget.service, originalService) &&
        widget.jobId == originalJobId;
    try {
      final catalog = await originalService.jobs();
      if (!original()) return;
      final found = (catalog['jobs'] as List)
          .cast<AutomationJson>()
          .where((j) => j['job_id'] == originalJobId)
          .firstOrNull;
      List<AutomationJson> incoming = [];
      var nextCursor = cursor, limited = truncated, retired = sourceArtifactsRetired;
      if (originalService case final AutomationEventService service) {
        final after = cursor;
        final page = await service.jobEvents(originalJobId, after: after);
        if (!original()) return;
        if (page.containsKey('artifact_retirement')) {
          validateRetirement(page, found ?? job, originalJobId, after);
          retired = true;
        }
        incoming = (page['events'] as List).cast<AutomationJson>();
        nextCursor = page['cursor'] as int;
        limited = page['output_truncated'] == true;
      }
      if (!original()) return;
      final combined = [...events, ...incoming];
      var bytes = combined.fold<int>(0, (count, event) => count + utf8.encode(jsonEncode(event)).length);
      while (combined.length > 50 || bytes > 1024 * 1024 && combined.length > 1) {
        bytes -= utf8.encode(jsonEncode(combined.removeAt(0))).length;
        limited = true;
      }
      final wasAtBottom = !scroll.hasClients || scroll.position.maxScrollExtent - scroll.position.pixels <= 24;
      setState(() {
        job = found ?? job;
        final stableTerminal = const {
          'succeeded',
          'successful',
          'failed',
          'cancelled',
          'interrupted',
          'acknowledged',
        }.contains(job?['state']);
        final finalEvidence =
            job?['runner_result'] != null || const {'cancelled', 'interrupted', 'acknowledged'}.contains(job?['state']);
        terminalDrained = stableTerminal && finalEvidence && incoming.isEmpty;
        events = combined;
        cursor = nextCursor;
        truncated = limited;
        sourceArtifactsRetired = retired;
        error = null;
        initial = false;
      });
      if (selectedTab == 0 && follow && wasAtBottom && incoming.isNotEmpty) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (current && selectedTab == 0 && scroll.hasClients) scroll.jumpTo(scroll.position.maxScrollExtent);
        });
      }
    } catch (e) {
      if (original()) {
        setState(() {
          error = e;
          initial = false;
        });
      }
    } finally {
      if (original()) fetching = false;
    }
  }

  // Source fact only. Never mint an event, advance its cursor, or imply cache erasure.
  void validateRetirement(AutomationJson page, AutomationJson? approved, String id, int after) {
    final raw = page['artifact_retirement'];
    bool bounded(Object? value, int min, int max) => value is int && value >= min && value <= max;
    final digest = approved?['manifest_digest'];
    if (raw is! Map<String, dynamic> ||
        raw.length != 4 ||
        !raw.keys.toSet().containsAll({'version', 'retired_at', 'last_sequence', 'output_bytes'}) ||
        raw['version'] is! int ||
        raw['version'] != 1 ||
        !bounded(raw['retired_at'], 1, 253402300799) ||
        !bounded(raw['last_sequence'], 0, 1000000000) ||
        !bounded(raw['output_bytes'], 0, 50 * 1024 * 1024) ||
        page['job_id'] != id ||
        approved?['job_id'] != id ||
        digest is! String ||
        !RegExp(r'^[0-9a-f]{64}$').hasMatch(digest) ||
        page['manifest_digest'] != digest ||
        page['events'] is! List ||
        (page['events'] as List).isNotEmpty ||
        page['cursor'] is! int ||
        page['cursor'] != after ||
        !{'succeeded', 'failed', 'cancelled', 'interrupted', 'unknown'}.contains(page['state'])) {
      throw const FormatException('Invalid source artifact retirement metadata');
    }
    if (after > (raw['last_sequence'] as int)) throw const FormatException('Invalid source artifact retirement cursor');
  }

  Widget log({required bool technical}) {
    final l = context.l10n;
    final metricsScope = widget.scope, metricsService = widget.service, metricsJob = job;
    final controller = technical ? technicalScroll : outputScroll;
    final body = NotificationListener<UserScrollNotification>(
      onNotification: (notice) {
        if (!technical &&
            controller.hasClients &&
            controller.position.maxScrollExtent - controller.position.pixels > 24 &&
            follow) {
          setState(() => follow = false);
        }
        return false;
      },
      child: ListView(
        key: ValueKey(
          technical
              ? 'automation-console-technical-scroll-${widget.jobId}'
              : 'automation-console-scroll-${widget.jobId}',
        ),
        controller: controller,
        padding: const EdgeInsets.all(12),
        children: [
          if (initial) const LinearProgressIndicator(),
          if (error != null) Text(errorMessage(l, error!)),
          if (truncated) Text(l.automationOutputLimited),
          if (sourceArtifactsRetired)
            Text(
              l.automationOutputRetired,
              key: ValueKey(technical ? 'automation-retired-technical' : 'automation-retired-output'),
            ),
          if (technical && metricsJob?['state'] == 'failed' && jobMetricsAvailable(ref))
            Align(
              alignment: AlignmentDirectional.centerStart,
              child: GlassButton(
                key: const ValueKey('automation-console-job-metrics'),
                icon: Icons.insights_outlined,
                label: l.correlationJobMetricsTitle,
                onPressed: () => showJobMetricsChooser(
                  context,
                  ref,
                  scope: metricsScope,
                  jobs: metricsService,
                  job: metricsJob!,
                  producerCurrent: () =>
                      current &&
                      visible &&
                      !widget.collapsed &&
                      selectedTab == 1 &&
                      identical(widget.scope, metricsScope) &&
                      identical(widget.service, metricsService) &&
                      widget.jobId == metricsJob['job_id'],
                ),
              ),
            ),
          if (technical && job != null && widget.service is AutomationEventService)
            AutomationTaskResults(
              key: ValueKey('console-task-results-${widget.jobId}'),
              service: widget.service as AutomationEventService,
              job: job!,
              scopeIdentity: widget.scope,
              active: visible && !widget.collapsed && selectedTab == 1,
              isCurrent: () => current,
            ),
          AutomationHumanEvents(events: events, job: job, technical: technical, viewState: technicalState),
          if (technical && job?.containsKey('admission_audit') == true)
            AutomationJobAdmissionAuditView(
              job: job!,
              expanded: resultState.auditOpen,
              page: resultState.auditPage,
              onExpanded: (open) => setState(() => resultState.auditOpen = open),
              onPage: (page) => setState(() => resultState.auditPage = page),
            ),
          if (technical && job?['runner_result'] != null)
            AutomationJobResult(
              key: ValueKey('automation-console-result-${widget.jobId}'),
              job: job!,
              viewState: resultState,
            ),
        ],
      ),
    );
    if (technical) return body;
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 4, 12, 12),
      child: Container(
        key: const ValueKey('automation-console-output-frame'),
        clipBehavior: Clip.hardEdge,
        decoration: BoxDecoration(
          color: Theme.of(context).colorScheme.surface,
          border: Border.all(color: Theme.of(context).dividerColor),
          borderRadius: BorderRadius.circular(8),
        ),
        child: body,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, manifest = job?['manifest'] as AutomationJson?;
    final state = automationJobStateLabel(l, job?['state'] as String? ?? 'queued');
    // l10n-ignore: localized console heading and Native manifest filename
    final title = '${l.automationExecutionConsole} · ${manifest?['playbook'] ?? widget.jobId}';
    Widget control(String key, String tooltip, IconData icon, VoidCallback? callback, {Color? color}) => IconButton(
      key: ValueKey(key),
      tooltip: tooltip,
      onPressed: callback,
      padding: EdgeInsets.zero,
      constraints: const BoxConstraints.tightFor(width: 28, height: 28),
      icon: Icon(icon, size: 16, color: color),
    );
    Widget controls() => Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (!terminal)
          control(
            'automation-console-follow',
            l.automationExecutionFollowLog,
            Icons.vertical_align_bottom,
            () => setState(() => follow = !follow),
            color: follow ? Theme.of(context).colorScheme.primary : Theme.of(context).colorScheme.onSurfaceVariant,
          ),
        control('automation-console-history', l.automationJobs, Icons.history, widget.onHistory),
        if (terminal && widget.onClose != null)
          control('automation-console-close', l.commonClose, Icons.close, widget.onClose),
        control(
          'automation-console-collapse',
          widget.collapsed ? l.automationExecutionExpand : l.automationExecutionCollapse,
          widget.collapsed ? Icons.keyboard_arrow_up : Icons.keyboard_arrow_down,
          widget.onToggle,
        ),
      ],
    );
    Widget tab(int index, String label) => Semantics(
      selected: selectedTab == index,
      child: GlassButton(
        key: ValueKey(index == 0 ? 'automation-console-output-tab' : 'automation-console-technical-tab'),
        label: label,
        tooltip: label,
        size: GlassControlSize.sm,
        style: selectedTab == index ? GlassButtonStyle.prominent : GlassButtonStyle.plain,
        onPressed: () => setState(() => selectedTab = index),
      ),
    );
    Widget tabs(double maxWidth) => ConstrainedBox(
      constraints: BoxConstraints(maxWidth: maxWidth),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          tab(0, l.automationExecutionOutput),
          const SizedBox(width: 4),
          Flexible(child: tab(1, l.automationExecutionTechnicalInformation)),
        ],
      ),
    );
    Widget identity() => Tooltip(
      message: '$title\n${l.automationExecutionReadOnly}',
      child: Row(
        children: [
          const Icon(Icons.terminal, size: 16),
          const SizedBox(width: 6),
          Flexible(
            child: Text(
              title,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ),
          const SizedBox(width: 8),
          Text(
            state,
            key: const ValueKey('automation-console-job-state'),
            style: Theme.of(context).textTheme.bodySmall?.copyWith(
              color: job?['state'] == 'failed'
                  ? Theme.of(context).colorScheme.error
                  : Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
        ],
      ),
    );
    return ContentSurface(
      padding: EdgeInsets.zero,
      kind: ContentSurfaceKind.solid,
      child: Column(
        children: [
          LayoutBuilder(
            builder: (context, box) {
              final wide = box.maxWidth >= 760;
              return SizedBox(
                key: const ValueKey('automation-console-header'),
                height: widget.collapsed || wide ? AutomationExecutionConsole.collapsedHeight : 64,
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 8),
                  child: Column(
                    children: [
                      SizedBox(
                        height: widget.collapsed || wide ? 36 : 32,
                        child: Row(
                          children: [
                            Expanded(child: identity()),
                            if (!widget.collapsed && wide) ...[
                              const SizedBox(width: 12),
                              tabs(box.maxWidth * .5),
                              const SizedBox(width: 8),
                            ],
                            const SizedBox(width: 4),
                            controls(),
                          ],
                        ),
                      ),
                      if (!widget.collapsed && !wide)
                        SizedBox(
                          height: 32,
                          child: Align(alignment: Alignment.centerLeft, child: tabs(box.maxWidth - 16)),
                        ),
                    ],
                  ),
                ),
              );
            },
          ),
          if (!widget.collapsed)
            Expanded(
              child: IndexedStack(index: selectedTab, children: [log(technical: false), log(technical: true)]),
            ),
        ],
      ),
    );
  }
}
