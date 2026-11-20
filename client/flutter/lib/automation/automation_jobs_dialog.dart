import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/automation/automation_event_view.dart';
import 'package:consolecrypt/automation/automation_job_admission_audit.dart';
import 'package:consolecrypt/automation/automation_job_result.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_run_dialog.dart';
import 'package:consolecrypt/automation/automation_scope_guard.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/automation/automation_workspace_panel.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class AutomationJobsDialog extends ConsumerStatefulWidget {
  const AutomationJobsDialog({
    super.key,
    required this.scope,
    required this.service,
    this.embedded = false,
    this.onClose,
    this.active = true,
    this.reviewEpoch = 0,
    this.onJobSelected,
    this.onJobSubmitted,
    this.onRepairRequested,
  });
  final bool embedded, active;
  final int reviewEpoch;
  final ValueChanged<String>? onJobSelected, onJobSubmitted;
  final VoidCallback? onClose;
  final ValueChanged<AutomationJson>? onRepairRequested;
  final AutomationScope scope;
  final AutomationJobService service;
  @override
  ConsumerState<AutomationJobsDialog> createState() => _AutomationJobsDialogState();
}

class _AutomationJobsDialogState extends ConsumerState<AutomationJobsDialog> {
  List<AutomationJson> jobs = [];
  final expandedJobs = <String>{};
  final resultStates = <String, AutomationJobResultViewState>{};
  final historyScroll = ScrollController();
  Widget? retryComposer;
  Object? error;
  bool busy = false;
  bool retryBusy = false;
  Timer? timer;
  String? eventJob;
  List<AutomationJson> events = [];
  Object? eventError;
  bool eventBusy = false, follow = true, eventTruncated = false;
  int cursor = 0;
  bool visible = true;
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope);
  @override
  void initState() {
    super.initState();
    reload();
    timer = Timer.periodic(const Duration(seconds: 2), (_) {
      if (widget.active && visible) reload();
      if (widget.active && visible && follow && eventJob != null) {
        loadEvents(after: cursor, append: true);
      }
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    visible = TickerMode.valuesOf(context).enabled;
  }

  @override
  void didUpdateWidget(AutomationJobsDialog oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.reviewEpoch != oldWidget.reviewEpoch) retryComposer = null;
    if (!oldWidget.active && widget.active) reload();
  }

  @override
  void dispose() {
    timer?.cancel();
    historyScroll.dispose();
    super.dispose();
  }

  Future<void> reload() async {
    if (busy || !current || !widget.active || !visible) return;
    busy = true;
    try {
      final result = await widget.service.jobs();
      if (current) {
        setState(() {
          jobs = [for (final value in result['jobs'] as List) value as AutomationJson]
            ..sort((a, b) => (b['accepted_at'] as int).compareTo(a['accepted_at'] as int));
          error = null;
        });
      }
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      busy = false;
    }
  }

  Future<void> showEvents(String id) async {
    if (eventBusy) return;
    setState(() {
      eventJob = id;
      events = [];
      cursor = 0;
      follow = true;
      eventError = null;
    });
    await loadEvents(after: 0);
  }

  Future<void> loadEvents({required int after, bool append = false}) async {
    final service = widget.service;
    final id = eventJob;
    if (service is! AutomationEventService || id == null || eventBusy || !current) return;
    setState(() => eventBusy = true);
    try {
      final page = await (service as AutomationEventService).jobEvents(id, after: after);
      if (!current || id != eventJob) return;
      final incoming = (page['events'] as List).cast<AutomationJson>();
      final combined = [if (append) ...events, ...incoming];
      var bytes = combined.fold<int>(0, (total, event) => total + utf8.encode(jsonEncode(event)).length);
      while (combined.length > 50 || (bytes > 1024 * 1024 && combined.length > 1)) {
        bytes -= utf8.encode(jsonEncode(combined.removeAt(0))).length;
      }
      setState(() {
        events = combined;
        cursor = page['cursor'] as int;
        eventTruncated = page['output_truncated'] as bool;
        eventError = null;
      });
    } catch (e) {
      if (current) setState(() => eventError = e);
    } finally {
      if (current) setState(() => eventBusy = false);
    }
  }

  Future<void> cancel(String id) async {
    try {
      await widget.service.cancelJob(id);
      await reload();
    } catch (e) {
      if (current) setState(() => error = e);
    }
  }

  Future<void> retry(AutomationJson previous) async {
    if (retryBusy || !current) return;
    final projects = ref.read(automationServiceProvider);
    if (projects == null) return;
    setState(() => retryBusy = true);
    try {
      final catalog = await projects.projects();
      if (!mounted || !current) return;
      final project = catalog.projects.where((item) => item.id == previous['project_id'] && !item.readOnly).firstOrNull;
      if (catalog.readOnly || project == null) throw FormatException(context.l10n.automationRetryUnavailable);
      final revision = await projects.files(project.id);
      if (!mounted || !current) return;
      final paths = [
        for (final entry in revision.entries)
          if (!entry.directory && (entry.path.endsWith('.yml') || entry.path.endsWith('.yaml'))) entry.path,
      ];
      final playbook = (previous['manifest'] as AutomationJson?)?['playbook'] as String?;
      if (!paths.contains(playbook)) throw FormatException(context.l10n.automationRetryUnavailable);
      if (widget.embedded) {
        setState(
          () => retryComposer = AutomationRunDialog(
            scope: widget.scope,
            service: widget.service,
            project: project,
            playbooks: [playbook!],
            initialPlaybook: playbook,
            retryOf: previous,
            embedded: true,
            onClose: () => setState(() => retryComposer = null),
            onSubmitted: (id) {
              setState(() => retryComposer = null);
              widget.onJobSubmitted?.call(id);
              reload();
            },
          ),
        );
      } else {
        await showGlassDialog<String>(
          context,
          builder: (_) => AutomationScopeGuard(
            scope: widget.scope,
            builder: (_) => AutomationRunDialog(
              scope: widget.scope,
              service: widget.service,
              project: project,
              playbooks: [playbook!],
              initialPlaybook: playbook,
              retryOf: previous,
            ),
          ),
        );
      }
      if (current) await reload();
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => retryBusy = false);
    }
  }

  Future<void> acknowledge(String id) async {
    final yes = await showGlassDialog<bool>(
      context,
      builder: (_) => AutomationScopeGuard(
        scope: widget.scope,
        builder: (context) => AlertDialog(
          title: Text(context.l10n.automationStateUnknown),
          content: Text(context.l10n.automationUnknownWarning),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context, false), child: Text(context.l10n.commonCancel)),
            TextButton(
              key: const ValueKey('automation-acknowledge-unknown'),
              onPressed: () => Navigator.pop(context, true),
              child: Text(context.l10n.automationAcknowledgeConfirm),
            ),
          ],
        ),
      ),
    );
    if (yes != true || !current) return;
    try {
      await widget.service.acknowledgeUnknown(id, acceptPartialChanges: true);
      await reload();
    } catch (e) {
      if (current) setState(() => error = e);
    }
  }

  Widget admissionAudit(AutomationJson job) {
    final id = job['job_id'] as String;
    final state = resultStates.putIfAbsent(id, AutomationJobResultViewState.new);
    return AutomationJobAdmissionAuditView(
      job: job,
      expanded: state.auditOpen,
      page: state.auditPage,
      onExpanded: (open) => setState(() => state.auditOpen = open),
      onPage: (page) => setState(() => state.auditPage = page),
    );
  }

  String label(String state) {
    final l = context.l10n;
    return switch (state) {
      'queued' => l.automationStateQueued,
      'dispatching' => l.automationStateDispatching,
      'running' => l.automationStateRunning,
      'succeeded' => l.automationStateSucceeded,
      'failed' => l.automationStateFailed,
      'cancelled' => l.automationStateCancelled,
      'interrupted' => l.automationStateInterrupted,
      'unknown' => l.automationStateUnknown,
      'acknowledged' => l.automationStateAcknowledged,
      _ => state,
    };
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    if (retryComposer != null) return retryComposer!;
    return AutomationWorkspacePanel(
      embedded: widget.embedded,
      title: Text(l.automationJobs),
      content: ListView(
        key: const ValueKey('automation-jobs-scroll'),
        controller: historyScroll,
        children: [
          Text(l.automationHistoryHint),
          const SizedBox(height: 12),
          if (jobs.isEmpty && error == null) Text(l.automationNoJobs),
          if (error != null)
            Text(errorMessage(l, error!), style: TextStyle(color: Theme.of(context).colorScheme.error)),
          if (eventJob != null) ...[
            const Divider(),
            Text(l.automationEvents),
            SelectableText(eventJob!),
            CheckboxListTile(
              value: follow,
              title: Text(l.automationFollowEvents),
              onChanged: (value) => setState(() => follow = value ?? false),
            ),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                TextButton(
                  onPressed: eventBusy || events.isEmpty || (events.first['sequence'] as int) <= 1
                      ? null
                      : () {
                          setState(() => follow = false);
                          loadEvents(after: ((events.first['sequence'] as int) - 51).clamp(0, 1 << 62).toInt());
                        },
                  child: Text(l.automationPreviousEvents),
                ),
                TextButton(
                  onPressed: eventBusy ? null : () => loadEvents(after: cursor),
                  child: Text(l.automationMoreEvents),
                ),
              ],
            ),
            if (eventBusy) const LinearProgressIndicator(),
            if (eventError != null) Text(errorMessage(l, eventError!)),
            if (eventTruncated) Text(l.automationOutputLimited),
            if (events.isEmpty && !eventBusy && eventError == null) Text(l.automationNoEvents),
            if (widget.service is AutomationEventService)
              for (final selectedJob in jobs.where((j) => j['job_id'] == eventJob))
                AutomationTaskResults(
                  key: ValueKey('history-task-results-$eventJob'),
                  service: widget.service as AutomationEventService,
                  job: selectedJob,
                  scopeIdentity: widget.scope,
                  active: widget.active && visible,
                  isCurrent: () => current,
                ),
            AutomationHumanEvents(events: events, job: jobs.where((j) => j['job_id'] == eventJob).firstOrNull),
            const Divider(),
          ],
          for (final job in jobs)
            Padding(
              key: ValueKey('automation-history-job-${job['job_id']}'),
              padding: const EdgeInsets.symmetric(vertical: 6),
              child: ContentSurface(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Expanded(
                          child: Text /* l10n-ignore: manifest filename identity */ (
                            // l10n-ignore: manifest filename or project identity
                            '${(job['manifest'] as AutomationJson?)?['playbook'] ?? job['project_id']}', // l10n-ignore: manifest filename or project identity
                            style: Theme.of(context).textTheme.titleSmall,
                          ),
                        ),
                        const SizedBox(width: 12),
                        GlassBadge(
                          label: label(job['state'] as String),
                          tone: switch (job['state']) {
                            'succeeded' => GlassTone.success,
                            'failed' => GlassTone.danger,
                            'running' || 'queued' || 'dispatching' => GlassTone.info,
                            _ => GlassTone.neutral,
                          },
                        ),
                      ],
                    ),
                    const SizedBox(height: 6),
                    Text(
                      [
                        (job['manifest'] as AutomationJson?)?['mode'] == 'apply'
                            ? l.automationApplyMode
                            : l.automationCheckMode,
                        l.automationReviewTargetCount((job['host_ids'] as List).length.toString()),
                        if (((job['runner_result'] as AutomationJson?)?['result'] as AutomationJson?)?['exit_code']
                            case final int code)
                          '${l.automationResultExitCode}: $code',
                      ].join(' · '),
                    ),
                    Wrap(
                      spacing: 8,
                      runSpacing: 4,
                      children: [
                        if (widget.service is AutomationEventService)
                          TextButton.icon(
                            key: ValueKey('automation-events-${job['job_id']}'),
                            icon: const Icon(Icons.terminal, size: 18),
                            onPressed: eventBusy
                                ? null
                                : () {
                                    if (widget.onJobSelected case final callback?) {
                                      callback(job['job_id'] as String);
                                    } else {
                                      showEvents(job['job_id'] as String);
                                    }
                                  },
                            label: Text(l.automationOpenOutput),
                          ),
                        if (job['state'] == 'failed' && widget.onRepairRequested != null)
                          TextButton.icon(
                            key: ValueKey('automation-repair-${job['job_id']}'),
                            icon: const Icon(Icons.auto_fix_high_outlined, size: 18),
                            onPressed: retryBusy ? null : () => widget.onRepairRequested!(job),
                            label: Text(l.automationRepairSuggest),
                          ),
                        if (!['queued', 'dispatching', 'running', 'unknown'].contains(job['state']))
                          TextButton(
                            key: ValueKey('automation-retry-${job['job_id']}'),
                            onPressed: retryBusy ? null : () => retry(job),
                            child: Text(l.automationRetryJob),
                          ),
                        TextButton(
                          key: ValueKey('automation-job-details-${job['job_id']}'),
                          onPressed: () => setState(() {
                            final id = job['job_id'] as String;
                            if (!expandedJobs.remove(id)) expandedJobs.add(id);
                          }),
                          child: Text(l.automationJobDetails),
                        ),
                      ],
                    ),
                    if (job['state'] == 'unknown') ...[
                      Text(l.automationUnknownWarning),
                      TextButton(
                        onPressed: () => acknowledge(job['job_id'] as String),
                        child: Text(l.automationAcknowledgeUnknown),
                      ),
                    ],
                    if (['queued', 'dispatching', 'running'].contains(job['state']))
                      TextButton(onPressed: () => cancel(job['job_id'] as String), child: Text(l.automationCancelJob)),
                    if (expandedJobs.contains(job['job_id'])) ...[
                      const Divider(),
                      SelectableText(job['job_id'] as String),
                      if (job['retry_origin'] case final Map<String, dynamic> previous)
                        Text /* l10n-ignore: localized retry label and job identity */ (
                          '${l.automationRetryOrigin}: ${previous['job_id']}',
                        ), // l10n-ignore: localized retry label plus native job identity
                      if (job['inventory_groups'] case final Map<String, dynamic> groups)
                        for (final group in groups.entries)
                          Text /* l10n-ignore: localized group label and count */ (
                            // l10n-ignore: localized group label plus group identity and count
                            '${l.automationGroups} · ${group.key} (${(group.value as Map)['hosts'] is List ? ((group.value as Map)['hosts'] as List).length : (group.value as Map)['selected_count']})', // l10n-ignore: localized group label plus group identity and count
                          ),
                      if (job.containsKey('admission_audit')) admissionAudit(job),
                      if (job['runner_result'] != null)
                        AutomationJobResult(
                          key: ValueKey('automation-result-${job['job_id']}'),
                          job: job,
                          viewState: resultStates.putIfAbsent(
                            job['job_id'] as String,
                            AutomationJobResultViewState.new,
                          ),
                        ),
                    ],
                  ],
                ),
              ),
            ),
        ],
      ),
      actions: [
        TextButton(onPressed: () => reload(), child: Text(l.commonRetry)),
        TextButton(
          onPressed: () {
            if (widget.embedded) {
              widget.onClose?.call();
            } else {
              Navigator.pop(context);
            }
          },
          child: Text(l.commonClose),
        ),
      ],
    );
  }
}
