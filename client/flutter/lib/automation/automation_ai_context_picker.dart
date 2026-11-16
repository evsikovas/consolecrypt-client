import 'dart:async';

import 'package:consolecrypt/automation/automation_ai_context.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:material_ui/material_ui.dart';

/// A local part of the existing AI review, not an independent reader/poller.
/// All capture IO starts only from its explicit buttons. Values never return to
/// the parent: only Native capture IDs and selected metadata references do.
class AutomationAiContextPicker extends StatefulWidget {
  const AutomationAiContextPicker({
    super.key,
    required this.service,
    required this.ai,
    required this.projectId,
    required this.profileId,
    required this.notificationScopeId,
    required this.initialProjectRevision,
    required this.hosts,
    required this.runners,
    required this.isCurrent,
    required this.enabled,
    required this.onSelection,
    required this.onBusy,
    this.inventory,
  });
  final AutomationAiContextService service;
  final AutomationAiService ai;
  final AutomationInventoryService? inventory;
  final String projectId, profileId, notificationScopeId;
  final int initialProjectRevision;
  final List<Host> hosts;
  final List<AutomationRunnerProfile> runners;
  final bool Function() isCurrent;
  final bool enabled;
  final ValueChanged<(AutomationJson?, bool)> onSelection;
  final ValueChanged<bool> onBusy;
  @override
  State<AutomationAiContextPicker> createState() => _AutomationAiContextPickerState();
}

class _AutomationAiContextPickerState extends State<AutomationAiContextPicker> {
  String? hostId, runnerId, operation;
  int? projectRevision;
  int generation = 0;
  bool busy = false, cancelled = false, failed = false, groupsLoaded = false, retired = false;
  late final AutomationAiContextService originalService;
  late final AutomationAiService originalAi;
  late final AutomationInventoryService? originalInventory;
  Object? inFlight;
  @override
  void initState() {
    super.initState();
    originalService = widget.service;
    originalAi = widget.ai;
    originalInventory = widget.inventory;
    projectRevision = widget.initialProjectRevision;
  }

  final groups = <String>[], selectedGroups = <String>{}, selectedTags = <String>{};
  final captures = <AutomationAiContextKind, AutomationAiCapturedContext>{};
  final chosen = <AutomationAiContextKind, Set<String>>{};
  final expiry = <AutomationAiContextKind, Timer>{};
  final expired = <String>{};
  bool get current => mounted && !retired && widget.isCurrent();

  void discard(String id, AutomationAiService ai) {
    unawaited(ai.discardAi(id).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
  }

  void drop(AutomationAiContextKind kind) {
    expiry.remove(kind)?.cancel();
    final old = captures.remove(kind);
    chosen.remove(kind);
    if (old != null) discard(old.captureId, originalAi);
  }

  @override
  void didUpdateWidget(AutomationAiContextPicker oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(widget.service, oldWidget.service) ||
        !identical(widget.ai, oldWidget.ai) ||
        !identical(widget.inventory, oldWidget.inventory) ||
        widget.projectId != oldWidget.projectId ||
        widget.profileId != oldWidget.profileId ||
        widget.notificationScopeId != oldWidget.notificationScopeId) {
      retired = true;
      generation++;
      cancel();
      for (final kind in [...captures.keys]) {
        drop(kind);
      }
      groups.clear();
      selectedGroups.clear();
      selectedTags.clear();
      projectRevision = null;
    }
    final beforeHost = oldWidget.hosts.where((h) => h.id.value == hostId).firstOrNull;
    final afterHost = widget.hosts.where((h) => h.id.value == hostId).firstOrNull;
    final beforeRunner = oldWidget.runners.where((r) => r.id == runnerId).firstOrNull;
    final afterRunner = widget.runners.where((r) => r.id == runnerId).firstOrNull;
    var changed = false;
    if (hostId != null && (afterHost == null || beforeHost?.updatedAt != afterHost.updatedAt)) {
      final capture = captures[AutomationAiContextKind.hostOs];
      if (capture != null) expired.add(capture.captureId);
      selectedTags.clear();
      changed = true;
      if (afterHost == null) hostId = null;
    }
    if (runnerId != null &&
        (afterRunner == null || afterRunner.readOnly || beforeRunner?.revision != afterRunner.revision)) {
      final capture = captures[AutomationAiContextKind.runnerVersions];
      if (capture != null) expired.add(capture.captureId);
      changed = true;
      if (afterRunner == null || afterRunner.readOnly) runnerId = null;
    }
    if (changed) scheduleMicrotask(emit);
  }

  @override
  void dispose() {
    generation++;
    final id = operation;
    if (id != null) {
      unawaited(originalService.cancelAiContext(id).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
    }
    for (final timer in expiry.values) {
      timer.cancel();
    }
    for (final capture in captures.values) {
      discard(capture.captureId, originalAi);
    }
    super.dispose();
  }

  void emit() {
    if (!current) return;
    final selected = <AutomationJson>[];
    var valid = true;
    for (final entry in chosen.entries) {
      if (entry.value.isEmpty) continue;
      final capture = captures[entry.key];
      if (capture == null || expired.contains(capture.captureId) || capture.projectRevision != projectRevision) {
        valid = false;
      } else {
        selected.add(capture.selection(entry.value));
      }
    }
    final any = selected.isNotEmpty || !valid || selectedGroups.isNotEmpty || selectedTags.isNotEmpty;
    widget.onSelection((
      any
          ? {
              'projectId': widget.projectId,
              'expectedProjectRevision': projectRevision,
              if (hostId != null) 'selectedHostId': hostId,
              'selections': selected,
              'selectedGroups': selectedGroups.toList()..sort(),
              'selectedTags': selectedTags.toList()..sort(),
            }
          : null,
      valid && (!any || projectRevision != null),
    ));
  }

  void cancel() {
    final id = operation;
    if (id == null || cancelled) return;
    cancelled = true;
    // Do not clear busy/operation: the actual pending Future still owns IO.
    unawaited(originalService.cancelAiContext(id).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
    if (mounted) setState(() {});
  }

  Future<void> readGroups() async {
    final reader = originalInventory;
    if (!current || busy || !widget.enabled || reader == null) return;
    final token = ++generation, marker = Object();
    inFlight = marker;
    setState(() {
      busy = true;
      failed = false;
    });
    widget.onBusy(true);
    try {
      final result = await reader.inventory(widget.projectId);
      if (!current || token != generation || !identical(widget.inventory, reader)) return;
      final inventory = result['inventory'], revision = result['project_revision'];
      if (result['version'] != 1 ||
          result['profile_id'] != widget.profileId ||
          result['notification_scope_id'] != widget.notificationScopeId ||
          result['project_id'] != widget.projectId ||
          revision is! int ||
          revision < 1 ||
          revision > 9007199254740991 ||
          inventory is! Map ||
          !const [1, 2].contains(inventory['version']) ||
          inventory['groups'] is! Map) {
        throw const FormatException('ai_context_groups');
      }
      final values = (inventory['groups'] as Map).keys;
      if (values.length > 64 ||
          values.any((g) => g is! String || !RegExp(r'^[A-Za-z_][A-Za-z0-9_-]{0,95}$').hasMatch(g))) {
        throw const FormatException('ai_context_groups');
      }
      for (final kind in [...captures.keys]) {
        if (captures[kind]!.projectRevision != revision) drop(kind);
      }
      setState(() {
        projectRevision = revision;
        groupsLoaded = true;
        groups
          ..clear()
          ..addAll(values.cast<String>());
        selectedGroups.clear();
      });
      emit();
    } catch (_) {
      if (current && token == generation) setState(() => failed = true);
    } finally {
      if (mounted && identical(inFlight, marker)) {
        inFlight = null;
        setState(() => busy = false);
        widget.onBusy(false);
      }
    }
  }

  Future<void> capture(AutomationAiContextKind kind) async {
    final subject = kind == AutomationAiContextKind.hostOs ? hostId : runnerId;
    if (!current || busy || !widget.enabled || subject == null || projectRevision == null) return;
    final service = originalService, ai = originalAi, project = widget.projectId, profile = widget.profileId;
    final revision = projectRevision!, nonce = widget.notificationScopeId;
    final id = ObjectId.generate().value, token = ++generation, marker = Object();
    inFlight = marker;
    drop(kind);
    emit();
    setState(() {
      busy = true;
      failed = false;
      cancelled = false;
      operation = id;
    });
    widget.onBusy(true);
    String? issued;
    try {
      final raw = await service.captureAiContext(
        operationId: id,
        profileId: profile,
        notificationScopeId: nonce,
        projectId: project,
        expectedProjectRevision: revision,
        kind: kind,
        subjectId: subject,
      );
      if (raw['profileId'] == profile &&
          raw['notificationScopeId'] == nonce &&
          raw['projectId'] == project &&
          raw['subjectId'] == subject &&
          raw['kind'] == kind.name &&
          raw['captureId'] is String &&
          RegExp(r'^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$').hasMatch(raw['captureId'] as String)) {
        issued = raw['captureId'] as String;
      }
      final result = AutomationAiCapturedContext.fromJson(
        raw,
        expectedProfileId: profile,
        expectedNotificationScopeId: nonce,
        expectedProjectId: project,
        expectedProjectRevision: revision,
        expectedKind: kind,
        expectedSubjectId: subject,
      );
      if (!current || token != generation || cancelled || !identical(service, widget.service)) {
        discard(result.captureId, ai);
        issued = null;
        return;
      }
      for (final other in [...captures.keys]) {
        if (captures[other]!.projectRevision != result.projectRevision) drop(other);
      }
      if (projectRevision != null && projectRevision != result.projectRevision) {
        groupsLoaded = false;
        groups.clear();
        selectedGroups.clear();
        selectedTags.clear();
      }
      setState(() {
        projectRevision = result.projectRevision;
        captures[kind] = result;
        chosen[kind] = {};
      });
      expiry[kind] = Timer(
        result.expiresUtc.difference(DateTime.now().toUtc()).isNegative
            ? Duration.zero
            : result.expiresUtc.difference(DateTime.now().toUtc()),
        () {
          if (!current || !identical(captures[kind], result)) return;
          setState(() => expired.add(result.captureId));
          emit();
          discard(result.captureId, ai);
        },
      );
      issued = null;
      emit();
    } catch (_) {
      if (issued != null) discard(issued, ai);
      if (current && token == generation && !cancelled) setState(() => failed = true);
    } finally {
      if (mounted && identical(inFlight, marker)) {
        inFlight = null;
        setState(() {
          operation = null;
          busy = false;
        });
        widget.onBusy(false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    if (retired) return const SizedBox.shrink();
    final l = context.l10n, enabled = current && widget.enabled && !busy;
    final host = widget.hosts.where((h) => h.id.value == hostId).firstOrNull;
    String fieldLabel(String field) => switch (field) {
      'osId' => l.automationAiContextOsId,
      'osVersion' => l.automationAiContextOsVersion,
      'architecture' => l.automationAiContextArchitecture,
      'kernelRelease' => l.automationAiContextKernel,
      'ansibleCore' => l.automationAiContextAnsible,
      'python' => l.automationAiContextPython,
      'collections' => l.automationAiContextProjectPins,
      _ => l.automationResultMissing,
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.automationAiContextCaptureHint),
        GlassSelect<String?>(
          key: const ValueKey('automation-ai-context-host'),
          value: hostId,
          expand: true,
          semanticLabel: l.automationAiContextHost,
          onChanged: !enabled
              ? null
              : (value) => setState(() {
                  hostId = value;
                  selectedTags.clear();
                  drop(AutomationAiContextKind.hostOs);
                  emit();
                }),
          items: [
            GlassSelectItem(value: null, label: l.automationAiContextHost),
            for (final row in widget.hosts.where((h) => h.protocol == HostProtocol.ssh))
              GlassSelectItem(value: row.id.value, label: row.name),
          ],
        ),
        TextButton(
          key: const ValueKey('automation-ai-context-capture-host'),
          onPressed: enabled && host != null ? () => capture(AutomationAiContextKind.hostOs) : null,
          child: Text(l.automationAiContextCaptureOs),
        ),
        GlassSelect<String?>(
          key: const ValueKey('automation-ai-context-runner'),
          value: runnerId,
          expand: true,
          semanticLabel: l.automationChooseRunner,
          onChanged: !enabled
              ? null
              : (value) => setState(() {
                  runnerId = value;
                  drop(AutomationAiContextKind.runnerVersions);
                  emit();
                }),
          items: [
            GlassSelectItem(value: null, label: l.automationChooseRunner),
            for (final row in widget.runners.where((r) => !r.readOnly))
              GlassSelectItem(value: row.id, label: row.name ?? row.id),
          ],
        ),
        TextButton(
          key: const ValueKey('automation-ai-context-capture-runner'),
          onPressed: enabled && runnerId != null ? () => capture(AutomationAiContextKind.runnerVersions) : null,
          child: Text(l.automationAiContextCaptureVersions),
        ),
        TextButton(
          key: const ValueKey('automation-ai-context-read-groups'),
          onPressed: enabled && widget.inventory != null ? readGroups : null,
          child: Text(l.automationAiContextReadGroups),
        ),
        if (groupsLoaded)
          for (final group in groups)
            CheckboxListTile(
              key: ValueKey('automation-ai-context-group-$group'),
              value: selectedGroups.contains(group),
              title: Text(group),
              onChanged: !enabled
                  ? null
                  : (value) => setState(() {
                      if (value == true) {
                        selectedGroups.add(group);
                      } else {
                        selectedGroups.remove(group);
                      }
                      emit();
                    }),
            ),
        if (host != null)
          for (final tag in host.tags.take(128))
            CheckboxListTile(
              key: ValueKey('automation-ai-context-tag-$tag'),
              value: selectedTags.contains(tag),
              title: Text(tag),
              onChanged: !enabled || projectRevision == null
                  ? null
                  : (value) => setState(() {
                      if (value == true) {
                        selectedTags.add(tag);
                      } else {
                        selectedTags.remove(tag);
                      }
                      emit();
                    }),
            ),
        for (final entry in captures.entries) ...[
          const Divider(),
          Text(
            entry.key == AutomationAiContextKind.hostOs
                ? l.automationAiContextSshSource
                : l.automationAiContextRunnerSource,
          ),
          Text(
            '${l.automationAiContextCaptured}: ${entry.value.capturedUtc.toIso8601String()}',
          ), // l10n-ignore: localized label plus actual Native UTC
          Text(
            '${l.automationAiContextExpires}: ${entry.value.expiresUtc.toIso8601String()}',
          ), // l10n-ignore: localized label plus actual Native UTC
          Text('${l.automationRevision}: ${entry.value.subjectRevision}'),
          if (entry.key == AutomationAiContextKind.runnerVersions) Text(l.automationAiContextPinsHint),
          if (expired.contains(entry.value.captureId))
            InfoBanner(message: l.automationAiContextExpired, tone: BannerTone.warning),
          for (final field
              in (entry.key == AutomationAiContextKind.hostOs
                  ? const ['osId', 'osVersion', 'architecture', 'kernelRelease']
                  : const ['ansibleCore', 'python', 'collections']))
            CheckboxListTile(
              key: ValueKey('automation-ai-context-field-${entry.key.name}-$field'),
              value: chosen[entry.key]?.contains(field) ?? false,
              title: Text(fieldLabel(field)),
              subtitle: Text(_value(entry.value.fields[field], l.automationResultMissing)),
              onChanged:
                  !enabled ||
                      !entry.value.fields.containsKey(field) ||
                      expired.contains(entry.value.captureId) && chosen[entry.key]?.contains(field) != true
                  ? null
                  : (value) => setState(() {
                      if (value == true) {
                        chosen[entry.key]!.add(field);
                      } else {
                        chosen[entry.key]!.remove(field);
                      }
                      emit();
                    }),
            ),
        ],
        if (busy) ...[
          const LinearProgressIndicator(),
          if (operation != null)
            TextButton(
              key: const ValueKey('automation-ai-context-cancel-capture'),
              onPressed: cancelled ? null : cancel,
              child: Text(cancelled ? l.noticesCancelling : l.commonCancel),
            ),
        ],
        if (failed) InfoBanner(message: l.automationAiContextFailed, tone: BannerTone.danger),
      ],
    );
  }

  String _value(Object? value, String missing) {
    if (value is String) return value;
    if (value is List<Map<String, String>>) return value.map((row) => '${row['name']} · ${row['version']}').join('\n');
    return missing;
  }
}
