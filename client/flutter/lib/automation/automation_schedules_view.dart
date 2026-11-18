import 'package:consolecrypt/app/critical_notice_providers.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';
import 'package:material_ui/material_ui.dart';

final _runtimeFrames = StreamProvider.autoDispose<RuntimeNoticeFrame>((ref) {
  return ref.watch(criticalRuntimeEventsProvider)?.watchRuntimeNotices() ?? const Stream<RuntimeNoticeFrame>.empty();
});

typedef _CatalogRequest = ({AutomationScheduleCatalogService service, String profile, String scope, int epoch});

final _catalog = FutureProvider.autoDispose.family<List<AutomationJson>, _CatalogRequest>((ref, request) async {
  final result = await request.service.scheduleCatalog({
    'version': 1,
    'profileId': request.profile,
    'notificationScopeId': request.scope,
  });
  final rows = result['records'];
  if (result['version'] != 1 ||
      result['profileId'] != request.profile ||
      result['notificationScopeId'] != request.scope ||
      result['metadataOnly'] != true ||
      rows is! List ||
      rows.length > 32) {
    throw const FormatException('schedule catalog');
  }
  return rows.cast<AutomationJson>().toList(growable: false);
});

class AutomationSchedulesView extends ConsumerStatefulWidget {
  const AutomationSchedulesView({
    super.key,
    required this.scope,
    required this.service,
    required this.active,
    required this.onCreate,
  });

  final AutomationScope scope;
  final AutomationService service;
  final bool active;
  final Future<void> Function(AutomationProject) onCreate;

  @override
  ConsumerState<AutomationSchedulesView> createState() => _AutomationSchedulesViewState();
}

class _AutomationSchedulesViewState extends ConsumerState<AutomationSchedulesView> {
  String? _projectId;
  bool _opening = false;

  Widget _scheduleCard(
    BuildContext context,
    AutomationJson row,
    Map<String, String> projects,
    Map<String, String> runners,
  ) {
    final l = context.l10n;
    final schedule = row['schedule'];
    final expires = DateTime.tryParse('${row['expiresUtc']}')?.toLocal();
    final format = DateFormat.yMMMd(Localizations.localeOf(context).toString()).add_Hm();
    final details = <String>[];
    if (schedule is AutomationJson &&
        schedule['hour'] is int &&
        schedule['minute'] is int &&
        schedule['zone'] is String &&
        schedule['weekdays'] is List) {
      final time = TimeOfDay(hour: schedule['hour'] as int, minute: schedule['minute'] as int).format(context);
      final weekdays = MaterialLocalizations.of(context).narrowWeekdays;
      final days = (schedule['weekdays'] as List).whereType<int>().where((day) => day >= 0 && day <= 6);
      details.add('$time · ${schedule['zone']} · ${days.map((day) => weekdays[(day + 1) % 7]).join(' · ')}');
    } else {
      details.add(l.automationSchedulesDetailsUnavailable);
    }
    if (expires != null) details.add('${l.automationSchedulesExpiry} · ${format.format(expires)}');
    final status = switch (row['delivery']) {
      'localBlocked' => l.automationSchedulesBlocked,
      _ when expires != null && !expires.isAfter(DateTime.now()) => l.automationSchedulesExpired,
      'acknowledged' => l.automationSchedulesAcknowledged,
      'pendingEnrollment' => l.automationSchedulesPending,
      _ => l.automationSchedulesUnknown,
    };
    final project = projects[row['projectId']];
    final runner = runners[row['runnerId']];
    final playbook = row['playbook'] as String?;
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Icon(Icons.schedule_rounded, size: 18),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    project ?? playbook ?? runner ?? l.automationSchedulesTitle,
                    style: Theme.of(context).textTheme.titleSmall,
                  ),
                ),
                const SizedBox(width: 8),
                Flexible(child: Text(status, textAlign: TextAlign.end)),
              ],
            ),
            const SizedBox(height: 6),
            if (project != null && playbook != null) Text(playbook),
            if (runner != null) Text(runner),
            for (final line in details) Text(line),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.active || !widget.scope.unlocked) return const SizedBox.shrink();
    final l = context.l10n;
    final profile = ref.watch(criticalNoticeScopeProvider);
    final events = ref.watch(criticalRuntimeEventsProvider);
    ref.watch(_runtimeFrames);
    final frame = events?.currentScope;
    final scope = frame?.scope;
    final service = widget.service;
    final request =
        service is AutomationScheduleCatalogService &&
            profile.unlocked &&
            frame != null &&
            !frame.streamUnavailable &&
            !frame.unsupported &&
            scope != null &&
            identical(ref.watch(automationScopeProvider), widget.scope) &&
            scope.profileId == profile.profileId
        ? (
            service: service as AutomationScheduleCatalogService,
            profile: scope.profileId,
            scope: scope.nonce,
            epoch: frame.epoch,
          )
        : null;
    final records = request == null ? null : ref.watch(_catalog(request));
    final projects = ref.watch(automationProjectsProvider).value?.projects ?? const <AutomationProject>[];
    final runners = ref.watch(automationRunnersProvider).value?.runners ?? const <AutomationRunnerProfile>[];
    final projectNames = {
      for (final project in projects)
        if (project.name case final name?) project.id: name,
    };
    final runnerNames = {
      for (final runner in runners)
        if (runner.name case final name?) runner.id: name,
    };
    final editable = projects.where((project) => !project.readOnly).toList(growable: false);
    final selected = editable.where((project) => project.id == _projectId).firstOrNull ?? editable.firstOrNull;
    final serverAvailable = runners.any(
      (runner) => !runner.readOnly && runner.configuration?['kind'] == 'service_remote',
    );
    return ListView(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 20),
      children: [
        Text(l.automationSchedulesTitle, style: Theme.of(context).textTheme.titleLarge),
        const SizedBox(height: 4),
        Text(l.automationSchedulesIntro),
        if (!serverAvailable) ...[const SizedBox(height: 8), Text(l.automationSchedulesRunnerRequired)],
        const SizedBox(height: 12),
        if (selected != null && serverAvailable) ...[
          GlassSelect<String>(
            value: selected.id,
            semanticLabel: l.automationLibrary,
            expand: true,
            onChanged: _opening ? null : (id) => setState(() => _projectId = id),
            items: [
              for (final project in editable) GlassSelectItem(value: project.id, label: project.name ?? project.id),
            ],
          ),
          const SizedBox(height: 8),
        ],
        Wrap(
          spacing: 8,
          runSpacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            GlassButton(
              label: l.automationSchedulesNew,
              icon: Icons.add_rounded,
              onPressed: !serverAvailable || selected == null || request == null || _opening
                  ? null
                  : () async {
                      setState(() => _opening = true);
                      try {
                        await widget.onCreate(selected);
                      } finally {
                        if (mounted) setState(() => _opening = false);
                      }
                    },
            ),
            GlassIconButton(
              icon: Icons.refresh_rounded,
              size: GlassControlSize.md.height,
              iconSize: 16,
              tooltip: l.commonRetry,
              onPressed: request == null || records?.isLoading == true ? null : () => ref.invalidate(_catalog(request)),
            ),
          ],
        ),
        const SizedBox(height: 16),
        if (records?.isLoading == true)
          const LinearProgressIndicator(minHeight: 2)
        else if (records == null || records.hasError)
          Text(l.automationSchedulesUnavailable)
        else if (records.value!.isEmpty)
          Text(l.automationSchedulesEmpty)
        else
          for (final row in records.value!) _scheduleCard(context, row, projectNames, runnerNames),
      ],
    );
  }
}
