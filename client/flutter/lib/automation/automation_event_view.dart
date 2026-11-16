import 'dart:convert';

import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:material_ui/material_ui.dart';

String automationJobStateLabel(AppLocalizations l, String state) => switch (state) {
  'queued' => l.automationStateQueued,
  'dispatching' => l.automationStateDispatching,
  'running' => l.automationStateRunning,
  'succeeded' => l.automationStateSucceeded,
  'failed' => l.automationStateFailed,
  'cancelled' => l.automationStateCancelled,
  'interrupted' => l.automationStateInterrupted,
  'acknowledged' => l.automationStateAcknowledged,
  _ => l.automationStateUnknown,
};
String automationSafeEventText(Object? value) {
  if (value is! String) return '';
  final bounded = value.length > 4096 ? value.substring(0, 4096) : value;
  final plain = bounded
      .replaceAll(RegExp(r'(?:\x1b\]|\x9d)[\s\S]*?(?:\x07|\x1b\\|\x9c|$)'), '')
      .replaceAll(RegExp(r'(?:\x1b\[|\x9b)[0-?]*[ -/]*[@-~]'), '')
      .replaceAll(RegExp(r'[\x00-\x1f\x7f-\x9f\u202a-\u202e\u2066-\u2069]'), ' ');
  return plain.length > 256 ? '${plain.substring(0, 256)}…' : plain;
}

Map<String, String> automationApprovedAliases(AutomationJson? job) {
  final manifest = job?['manifest'];
  if (manifest is! Map || manifest['targets'] is! List) return {};
  return {
    for (final target in (manifest['targets'] as List).whereType<Map<Object?, Object?>>().take(10000))
      if (target['alias'] is String)
        target['alias'] as String: automationSafeEventText(target['address']).isEmpty
            ? automationSafeEventText(target['alias'])
            : automationSafeEventText(target['address']),
  };
}

Map<String, dynamic> _payload(AutomationJson event) =>
    event['payload'] is Map ? Map<String, dynamic>.from(event['payload'] as Map) : {};

/// A strict display projection of native journal DTOs. Raw module stdout,
/// stderr, msg, invocation, diff and arbitrary result fields never enter this view.
AutomationJson automationEventProjection(AutomationJson event, Map<String, String> aliases) {
  final payload = _payload(event), safe = <String, dynamic>{};
  for (final key in ['task', 'play', 'task_uuid', 'play_uuid', 'task_action']) {
    if (payload[key] is String) safe[key] = automationSafeEventText(payload[key]);
  }
  if (payload['host'] is String && aliases.containsKey(payload['host'])) safe['host'] = payload['host'];
  if (payload['res'] is Map) {
    final result = payload['res'] as Map;
    safe['res'] = {
      for (final key in ['changed', 'failed', 'skipped', 'censored', 'truncated'])
        if (result[key] is bool) key: result[key],
    };
  }
  if (event['type'] == 'runner_summary') {
    if (payload['exit_code'] is int) safe['exit_code'] = payload['exit_code'];
    if (payload['stats'] is Map) {
      safe['stats'] = {
        for (final metric in ['ok', 'changed', 'failures', 'dark', 'skipped', 'rescued', 'ignored'])
          if ((payload['stats'] as Map)[metric] is Map)
            metric: {
              for (final alias in aliases.keys.take(100))
                if (((payload['stats'] as Map)[metric] as Map)[alias] is int &&
                    (((payload['stats'] as Map)[metric] as Map)[alias] as int) >= 0)
                  alias: ((payload['stats'] as Map)[metric] as Map)[alias],
            },
      };
    }
  }
  return {
    'sequence': event['sequence'],
    'type': automationSafeEventText(event['type']),
    'timestamp': automationSafeEventText(event['timestamp']),
    'payload': safe,
  };
}

List<String> automationHumanEventLines(AutomationJson event, Map<String, String> aliases) {
  final safe = automationEventProjection(event, aliases), payload = safe['payload'] as AutomationJson;
  final task = payload['task'] as String? ?? '', play = payload['play'] as String? ?? '';
  final host = aliases[payload['host']];
  final res = payload['res'] as AutomationJson? ?? {};
  final suffix = '${host == null ? '' : ' [$host]'}${task.isEmpty ? '' : ' — $task'}';
  switch (safe['type']) {
    case 'playbook_on_play_start':
      return ['PLAY [$play]'];
    case 'playbook_on_task_start':
    case 'runner_on_start':
      return ['TASK [$task]'];
    case 'runner_on_ok':
      return ['${res['changed'] == true ? 'changed' : 'ok'}$suffix'];
    case 'runner_on_failed':
      return ['failed$suffix'];
    case 'runner_on_unreachable':
      return ['unreachable$suffix'];
    case 'runner_on_skipped':
      return ['skipped$suffix'];
    case 'runner_summary':
      final stats = payload['stats'] as AutomationJson? ?? {};
      return [
        'PLAY RECAP', // l10n-ignore: Ansible-compatible output heading token
        for (final alias in aliases.keys.take(100))
          '${aliases[alias]} : ${[
            for (final metric in {'ok': 'ok', 'changed': 'changed', 'failures': 'failed', 'dark': 'unreachable', 'skipped': 'skipped'}.entries) '${metric.value}=${stats[metric.key] is Map ? ((stats[metric.key] as Map)[alias] ?? '—') : '—'}', // l10n-ignore: Ansible recap counter names and values
          ].join('  ')}',
      ];
    default:
      return [];
  }
}

/// Explicit diagnostics preserve the native journal's existing data, as plain
/// bounded text. NoLog/censored subtrees are never traversed or exposed.
Object? automationEventDiagnostics(AutomationJson event) {
  final result = _payload(event)['res'];
  var budget = 16384;
  var nodes = 0;
  Object? clean(Object? value, int depth) {
    if (++nodes > 512 || budget <= 0 || depth > 8) return '…';
    if (value is Map) {
      if (value['_ansible_no_log'] == true || value['censored'] == true) return {'censored': true};
      return {
        for (final entry in value.entries.take(100)) automationSafeEventText(entry.key): clean(entry.value, depth + 1),
      };
    }
    if (value is List) return value.take(100).map((v) => clean(v, depth + 1)).toList();
    if (value is String) {
      final limit = budget < 4096 ? budget : 4096;
      final text = value.length > limit ? value.substring(0, limit) : value;
      final plain = text
          .replaceAll(RegExp(r'(?:\x1b\]|\x9d)[\s\S]*?(?:\x07|\x1b\\|\x9c|$)'), '')
          .replaceAll(RegExp(r'(?:\x1b\[|\x9b)[0-?]*[ -/]*[@-~]'), '')
          .replaceAll(RegExp(r'[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f\u202a-\u202e\u2066-\u2069]'), ' ');
      budget -= plain.length;
      return plain;
    }
    return value is num || value is bool || value == null ? value : null;
  }

  return clean(result, 0);
}

({String text, bool limited}) automationDiagnosticText(Object? details) {
  final text = details is Map
      ? details.entries
            .map(
              (e) => '${e.key}:\n${e.value is String ? e.value : const JsonEncoder.withIndent('  ').convert(e.value)}', // l10n-ignore: serialized native diagnostic data
            )
            .join('\n\n')
      : const JsonEncoder.withIndent('  ').convert(details);
  return (text: text.length > 16384 ? '${text.substring(0, 16384)}…' : text, limited: text.length > 16384);
}

enum AutomationOutputKind { play, task, ok, changed, failed, unreachable, skipped, recap, recapHost, other }

/// Display tone comes from the native event kind and structured changed flag,
/// never from untrusted task text or terminal escape sequences.
AutomationOutputKind automationEventOutputKind(AutomationJson event) => switch (event['type']) {
  'playbook_on_play_start' => AutomationOutputKind.play,
  'playbook_on_task_start' || 'runner_on_start' => AutomationOutputKind.task,
  'runner_on_ok' =>
    _payload(event)['res'] is Map && (_payload(event)['res'] as Map)['changed'] == true
        ? AutomationOutputKind.changed
        : AutomationOutputKind.ok,
  'runner_on_failed' => AutomationOutputKind.failed,
  'runner_on_unreachable' => AutomationOutputKind.unreachable,
  'runner_on_skipped' => AutomationOutputKind.skipped,
  'runner_summary' => AutomationOutputKind.recap,
  _ => AutomationOutputKind.other,
};

class AutomationOutputLine extends StatelessWidget {
  const AutomationOutputLine({super.key, required this.text, required this.kind});
  final String text;
  final AutomationOutputKind kind;
  @override
  Widget build(BuildContext context) {
    final tokens = GlassTokens.of(context), palette = GlassTokens.of(context).palette;
    final heading = {AutomationOutputKind.play, AutomationOutputKind.task, AutomationOutputKind.recap}.contains(kind);
    final color = switch (kind) {
      AutomationOutputKind.play || AutomationOutputKind.task => palette.accent,
      AutomationOutputKind.ok => palette.success,
      AutomationOutputKind.changed => palette.warning,
      AutomationOutputKind.failed || AutomationOutputKind.unreachable => palette.danger,
      AutomationOutputKind.skipped => palette.secondary,
      AutomationOutputKind.recap => palette.info,
      AutomationOutputKind.recapHost || AutomationOutputKind.other => palette.label,
    };
    return Container(
      padding: EdgeInsets.only(top: kind == AutomationOutputKind.recap ? 10 : 3, bottom: 3),
      decoration: kind == AutomationOutputKind.recap
          ? BoxDecoration(
              border: Border(top: BorderSide(color: Theme.of(context).dividerColor)),
            )
          : null,
      child: SelectableText(
        text,
        style: tokens.typography.mono.copyWith(
          color: color,
          fontWeight: heading ? FontWeight.w600 : FontWeight.w400,
          height: 1.5,
        ),
      ),
    );
  }
}

class AutomationEventViewState {
  final diagnosticsOpen = <int>{};
  bool jsonOpen = false;
}

List<String> automationJobRecapLines(AutomationJson? job) {
  final evidence = job?['runner_result'];
  if (evidence is! Map || evidence['result'] is! Map) return [];
  final summary = (evidence['result'] as Map)['summary'];
  if (summary is! Map || summary['stats'] is! Map) return [];
  return automationHumanEventLines({
    'type': 'runner_summary',
    'payload': {'stats': summary['stats']},
  }, automationApprovedAliases(job));
}

class AutomationHumanEvents extends StatelessWidget {
  const AutomationHumanEvents({
    super.key,
    required this.events,
    required this.job,
    this.technical = true,
    this.viewState,
  });
  final List<AutomationJson> events;
  final AutomationJson? job;
  final bool technical;
  final AutomationEventViewState? viewState;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, aliases = automationApprovedAliases(job);
    if (!technical) {
      final lines = <({String id, String text, AutomationOutputKind kind})>[
        for (final event in events)
          for (final (index, line) in automationHumanEventLines(event, aliases).indexed)
            (
              id: 'event-${event['sequence']}-$index',
              text: line,
              kind: event['type'] == 'runner_summary' && index > 0
                  ? AutomationOutputKind.recapHost
                  : automationEventOutputKind(event),
            ),
        if (!events.any((event) => event['type'] == 'runner_summary'))
          for (final (index, line) in automationJobRecapLines(job).indexed)
            (
              id: 'result-recap-$index',
              text: line,
              kind: index == 0 ? AutomationOutputKind.recap : AutomationOutputKind.recapHost,
            ),
      ];
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (lines.isEmpty) Text(l.automationNoEvents),
          if (aliases.length > 100 &&
              (events.any((event) => event['type'] == 'runner_summary') || automationJobRecapLines(job).isNotEmpty))
            Text(l.automationOutputLimited),
          for (final line in lines)
            AutomationOutputLine(key: ValueKey('automation-output-${line.id}'), text: line.text, kind: line.kind),
        ],
      );
    }

    final technicalJson = const JsonEncoder.withIndent('  ')
        .convert(events.map((event) => automationEventProjection(event, aliases)).toList());
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (events.isEmpty) Text(l.automationNoEvents),
        for (final event in events)
          _EventEntry(
            key: ValueKey('event-${event['sequence']}'),
            event: event,
            aliases: aliases,
            viewState: viewState,
          ),
        ExpansionTile(
          key: const ValueKey('automation-event-technical'),
          initiallyExpanded: viewState?.jsonOpen ?? false,
          onExpansionChanged: (open) {
            if (viewState != null) viewState!.jsonOpen = open;
          },
          title: Text(l.automationExecutionTechnical),
          children: [
            if (technicalJson.length > 16384) Text(l.automationOutputLimited),
            SelectableText(technicalJson.length > 16384 ? '${technicalJson.substring(0, 16384)}…' : technicalJson),
          ],
        ),
      ],
    );
  }
}

class _EventEntry extends StatefulWidget {
  const _EventEntry({super.key, required this.event, required this.aliases, this.viewState});
  final AutomationJson event;
  final Map<String, String> aliases;
  final AutomationEventViewState? viewState;
  @override
  State<_EventEntry> createState() => _EventEntryState();
}

class _EventEntryState extends State<_EventEntry> {
  bool localOpen = false;
  int get sequence => widget.event['sequence'] as int? ?? 0;
  bool get open => widget.viewState?.diagnosticsOpen.contains(sequence) ?? localOpen;
  void toggle() => setState(() {
    final view = widget.viewState;
    if (view == null) {
      localOpen = !localOpen;
    } else if (!view.diagnosticsOpen.add(sequence)) {
      view.diagnosticsOpen.remove(sequence);
    }
  });
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, details = automationEventDiagnostics(widget.event);
    final censored = details is Map && details['censored'] == true;
    final diagnostic = open && !censored ? automationDiagnosticText(details) : (text: '', limited: false);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final line in automationHumanEventLines(widget.event, widget.aliases))
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 3),
            child: SelectableText(line, style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
          ),
        if (censored)
          Text(l.automationEventCensored)
        else if (details != null) ...[
          Align(
            alignment: Alignment.centerLeft,
            child: TextButton.icon(
              key: ValueKey('automation-event-diagnostics-${widget.event['sequence']}'),
              onPressed: toggle,
              icon: Icon(open ? Icons.expand_less : Icons.expand_more, size: 16),
              label: Text(l.automationEventDiagnostics),
            ),
          ),
          if (open)
            ContentSurface(
              kind: ContentSurfaceKind.inset,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [if (diagnostic.limited) Text(l.automationOutputLimited), SelectableText(diagnostic.text)],
              ),
            ),
        ],
      ],
    );
  }
}


/// A fixed projection of one Native-verified journal page. It retains neither
/// arbitrary module payloads nor task-name-based identity/aggregate guesses.
({List<AutomationJson> rows, int cursor, bool limited, int count})
automationTaskResultPage(
  AutomationJson page, {
  required String jobId,
  required String digest,
  required int after,
  required Map<String, String> aliases,
}) {
  final events = page['events'], cursor = page['cursor'];
  if (page['job_id'] != jobId ||
      page['manifest_digest'] != digest ||
      events is! List ||
      events.length > 50 ||
      cursor is! int ||
      cursor < after ||
      cursor > 9007199254740991 ||
      page['output_truncated'] is! bool) {
    throw const FormatException('task_page');
  }
  var sequence = after;
  final rows = <AutomationJson>[];
  for (final value in events) {
    if (value is! Map ||
        value['version'] is! int ||
        value['version'] != 1 ||
        value['job_id'] != jobId ||
        value['sequence'] is! int ||
        value['sequence'] != sequence + 1) {
      throw const FormatException('task_sequence');
    }
    sequence++;
    final kind = value['type'];
    if (!const {
      'runner_on_ok',
      'runner_on_failed',
      'runner_on_unreachable',
      'runner_on_skipped',
    }.contains(kind)) {
      continue;
    }
    final payload = value['payload'];
    if (payload is! Map ||
        payload['host'] is! String ||
        !aliases.containsKey(payload['host'])) {
      continue;
    }
    final result = payload['res'] is Map
        ? payload['res'] as Map
        : const <String, Object?>{};
    final censored =
        payload['_ansible_no_log'] == true ||
        payload['no_log'] == true ||
        result['_ansible_no_log'] == true ||
        result['censored'] != null && result['censored'] != false;
    final taskId = payload['task_uuid'];
    // Legacy/malformed UUIDs remain independent sequence rows; never merge
    // identically named tasks or treat a display name as a task identifier.
    final validId =
        taskId is String &&
        RegExp(r'^[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$')
            .hasMatch(taskId);
    rows.add({
      'sequence': sequence,
      'timestamp': automationSafeEventText(value['timestamp']),
      'type': kind,
      'task': automationSafeEventText(payload['task']),
      'task_id': validId ? taskId : null,
      'action': automationSafeEventText(payload['task_action']),
      'host': payload['host'],
      'host_label': aliases[payload['host']],
      'censored': censored,
      if (!censored && result['truncated'] == true) 'truncated': true,
      if (!censored && result['changed'] is bool) 'changed': result['changed'],
      if (!censored &&
          result['rc'] is int &&
          (result['rc'] as int) >= -2147483648 &&
          (result['rc'] as int) <= 2147483647)
        'rc': result['rc'],
      if (!censored &&
          const {'runner_on_failed', 'runner_on_unreachable'}.contains(kind) &&
          result['msg'] is String)
        'error': automationSafeEventText(result['msg']),
    });
  }
  if (cursor != sequence) throw const FormatException('task_cursor');
  return (
    rows: rows,
    cursor: cursor,
    limited: page['output_truncated'] as bool,
    count: events.length,
  );
}

/// Explicit, bounded reads through the existing event service. Opening the
/// surrounding screen or filtering/polling does not request another page.
class AutomationTaskResults extends StatefulWidget {
  const AutomationTaskResults({
    super.key,
    required this.service,
    required this.job,
    required this.scopeIdentity,
    required this.isCurrent,
    this.active = true,
  });
  final AutomationEventService service;
  final AutomationJson job;
  final Object scopeIdentity;
  final bool Function() isCurrent;
  final bool active;
  @override
  State<AutomationTaskResults> createState() => _AutomationTaskResultsState();
}

class _AutomationTaskResultsState extends State<AutomationTaskResults> {
  final filter = TextEditingController();
  final previous = <int>[];
  final expanded = <int>{};
  List<AutomationJson> rows = [];
  int after = 0, cursor = 0, count = 0, generation = 0;
  bool open = false,
      busy = false,
      loaded = false,
      failed = false,
      limited = false;
  Object? inFlight;
  String get jobId =>
      widget.job['job_id'] is String ? widget.job['job_id'] as String : '';
  String get digest => widget.job['manifest_digest'] is String
      ? widget.job['manifest_digest'] as String
      : '';
  bool get current => mounted && widget.active && widget.isCurrent();

  @override
  void didUpdateWidget(AutomationTaskResults oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.service, widget.service) ||
        !identical(oldWidget.scopeIdentity, widget.scopeIdentity) ||
        oldWidget.job['job_id'] != jobId ||
        oldWidget.job['manifest_digest'] != digest ||
        jsonEncode(automationApprovedAliases(oldWidget.job)) !=
            jsonEncode(automationApprovedAliases(widget.job))) {
      generation++;
      rows = [];
      previous.clear();
      expanded.clear();
      filter.clear();
      after = 0;
      cursor = 0;
      count = 0;
      loaded = false;
      busy = false;
      failed = false;
      limited = false;
    } else if (oldWidget.active && !widget.active) {
      generation++;
      busy = false;
    }
  }

  @override
  void dispose() {
    generation++;
    filter.dispose();
    super.dispose();
  }

  Future<void> read() async {
    if (inFlight != null ||
        !current ||
        !open ||
        jobId.isEmpty ||
        !RegExp(r'^[0-9a-f]{64}$').hasMatch(digest)) {
      return;
    }
    final originalService = widget.service,
        originalScope = widget.scopeIdentity;
    final originalJob = jobId, originalDigest = digest, requested = after;
    final aliases = automationApprovedAliases(widget.job), token = ++generation;
    final flight = Object();
    inFlight = flight;
    bool original() =>
        current &&
        token == generation &&
        open &&
        identical(widget.service, originalService) &&
        identical(widget.scopeIdentity, originalScope) &&
        jobId == originalJob &&
        digest == originalDigest;
    setState(() {
      busy = true;
      failed = false;
    });
    try {
      final page = await originalService.jobEvents(
        originalJob,
        after: requested,
        limit: 50,
      );
      if (!original()) return;
      final view = automationTaskResultPage(
        page,
        jobId: originalJob,
        digest: originalDigest,
        after: requested,
        aliases: aliases,
      );
      setState(() {
        rows = view.rows;
        cursor = view.cursor;
        count = view.count;
        limited = view.limited;
        loaded = true;
      });
    } catch (_) {
      if (original()) {
        setState(() {
          failed = true;
          loaded = false;
        });
      }
    } finally {
      // Stopping the UI wait does not manufacture Native request retirement.
      // A new page cannot overlap this actual Future, including after hiding.
      if (identical(inFlight, flight)) {
        inFlight = null;
        if (mounted) setState(() => busy = false);
      }
    }
  }

  void move(bool forward) {
    if (inFlight != null || !current) return;
    if (forward) {
      if (!loaded || cursor <= after) return;
      previous.add(after);
      if (previous.length > 128) previous.removeAt(0);
      after = cursor;
    } else {
      if (previous.isEmpty) return;
      after = previous.removeLast();
    }
    setState(() {
      rows = [];
      cursor = after;
      loaded = false;
      expanded.clear();
    });
    read();
  }

  void firstPage() {
    if (inFlight != null || !current || after == 0) return;
    setState(() {
      previous.clear();
      after = 0;
      cursor = 0;
      rows = [];
      loaded = false;
      expanded.clear();
    });
    read();
  }

  Widget row(AutomationJson value) {
    final l = context.l10n, seq = value['sequence'] as int;
    final state = switch (value['type']) {
      'runner_on_failed' => l.automationResultFailed,
      'runner_on_unreachable' => l.automationResultUnreachable,
      'runner_on_skipped' => l.automationResultSkipped,
      _ =>
        value['changed'] == true
            ? l.automationResultChanged
            : l.automationResultOk,
    };
    final task = (value['task'] as String).isEmpty
        ? l.automationTaskUnnamed
        : value['task'] as String;
    final stamp = (value['timestamp'] as String).isEmpty
        ? l.automationResultMissing
        : value['timestamp'] as String;
    final changedLabel = value['changed'] == null
        ? l.automationResultMissing
        : value['changed'] == true
        ? l.automationResultChanged
        : l.automationTaskUnchanged;
    final rc = value['rc'];
    final exitLabel = rc is int ? rc.toString() : l.automationResultMissing;
    return ExpansionTile(
      key: ValueKey('automation-task-result-$seq'),
      dense: true,
      minTileHeight: 40,
      visualDensity: VisualDensity.compact,
      initiallyExpanded: expanded.contains(seq),
      onExpansionChanged: (value) {
        if (value) {
          expanded.add(seq);
        } else {
          expanded.remove(seq);
        }
      },
      title: Text(task, maxLines: 2, overflow: TextOverflow.ellipsis),
      subtitle: Text(
        l.automationTaskHostState(value['host_label'] as String, state),
      ),
      children: [
        Padding(
          padding: const EdgeInsets.all(8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              SelectableText(task),
              Text(l.automationTaskEventReference(seq.toString(), stamp)),
              if (value['task_id'] != null)
                SelectableText(
                  l.automationTaskIdentity(value['task_id'] as String),
                ),
              if ((value['action'] as String).isNotEmpty)
                Text(l.automationTaskModule(value['action'] as String)),
              Text('${l.automationResultChanged}: $changedLabel'),
              Text('${l.automationResultExitCode}: $exitLabel'),
              if (value['truncated'] == true) Text(l.automationOutputLimited),
              if (value['censored'] == true)
                Text(l.automationEventCensored)
              else if (value['error'] is String &&
                  (value['error'] as String).isNotEmpty)
                SelectableText(
                  value['error'] as String,
                  key: ValueKey('automation-task-error-$seq'),
                ),
            ],
          ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.isCurrent()) return const SizedBox.shrink();
    final l = context.l10n, query = filter.text.toLowerCase();
    final shown = rows.where(
      (value) =>
          (value['host'] as String).toLowerCase().contains(query) ||
          (value['host_label'] as String).toLowerCase().contains(query),
    );
    return ExpansionTile(
      key: ValueKey('automation-task-results-$jobId'),
      dense: true,
      minTileHeight: 36,
      title: Text(l.automationTaskResults),
      initiallyExpanded: open,
      onExpansionChanged: (value) {
        if (!mounted || !widget.isCurrent()) return;
        setState(() {
          open = value;
          if (!value) {
            generation++;
            busy = false;
          }
        });
        if (value && !loaded) read();
      },
      children: [
        Text(l.automationTaskPageHelp),
        if (digest.isEmpty) Text(l.automationResultNoHosts),
        TextField(
          key: const ValueKey('automation-task-host-filter'),
          controller: filter,
          maxLength: 128,
          decoration: InputDecoration(labelText: l.automationTaskHostFilter),
          onChanged: (_) {
            if (current) setState(() {});
          },
        ),
        Wrap(
          spacing: 8,
          runSpacing: 4,
          children: [
            TextButton(
              key: const ValueKey('automation-task-first'),
              onPressed: inFlight != null || after == 0 || !widget.active
                  ? null
                  : firstPage,
              child: Text(l.automationTaskFirstPage),
            ),
            TextButton(
              key: const ValueKey('automation-task-previous'),
              onPressed: inFlight != null || previous.isEmpty || !widget.active
                  ? null
                  : () => move(false),
              child: Text(l.automationPreviousEvents),
            ),
            TextButton(
              key: const ValueKey('automation-task-next'),
              onPressed:
                  inFlight != null ||
                      !loaded ||
                      cursor <= after ||
                      !widget.active
                  ? null
                  : () => move(true),
              child: Text(l.automationMoreEvents),
            ),
            TextButton(
              key: const ValueKey('automation-task-refresh'),
              onPressed: inFlight != null || !widget.active ? null : read,
              child: Text(l.automationTaskRefresh),
            ),
            if (busy)
              TextButton(
                key: const ValueKey('automation-task-cancel-read'),
                onPressed: () {
                  if (current) {
                    setState(() {
                      generation++;
                      busy = false;
                    });
                  }
                },
                child: Text(l.commonCancel),
              ),
          ],
        ),
        if (busy) const LinearProgressIndicator(),
        if (!busy && inFlight != null) Text(l.automationTaskReadFinishing),
        if (failed)
          InfoBanner(
            key: const ValueKey('automation-task-read-error'),
            message: l.automationTaskReadFailed,
            tone: BannerTone.danger,
          ),
        if (loaded)
          Text(
            l.automationTaskEventPage(
              (count == 0 ? after : after + 1).toString(),
              cursor.toString(),
            ),
          ),
        if (limited) Text(l.automationOutputLimited),
        if (loaded && shown.isEmpty) Text(l.automationTaskNoMatchingResults),
        for (final value in shown) row(value),
      ],
    );
  }
}
