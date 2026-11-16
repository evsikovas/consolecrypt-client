import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/ai/device_local_ai_service.dart';
import 'package:consolecrypt/automation/automation_ai_context_picker.dart';
import 'package:consolecrypt/automation/automation_ai_draft.dart';
import 'package:consolecrypt/automation/automation_event_view.dart';
import 'package:consolecrypt/automation/automation_job_result.dart';
import 'package:consolecrypt/automation/automation_providers.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/ai_service.dart';
import 'package:consolecrypt/core/services/runtime_notice_events.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_editor.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class _AiCodeFrame extends StatelessWidget {
  const _AiCodeFrame({required this.child, required this.height});
  final Widget child;
  final double height;
  @override
  Widget build(BuildContext context) => Container(
    height: height,
    clipBehavior: Clip.hardEdge,
    decoration: BoxDecoration(
      color: Theme.of(context).colorScheme.surface,
      border: Border.all(color: Theme.of(context).dividerColor),
      borderRadius: BorderRadius.circular(8),
    ),
    child: child,
  );
}

class _AiCodeBlock extends StatelessWidget {
  const _AiCodeBlock(this.text, {this.height});
  final String text;
  final double? height;
  @override
  Widget build(BuildContext context) => _AiCodeFrame(
    height: height ?? (text.split('\n').length.clamp(4, 12) * 18 + 24).toDouble(),
    child: SingleChildScrollView(
      padding: const EdgeInsets.all(12),
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: SelectableText(text, style: GlassTokens.of(context).typography.mono),
      ),
    ),
  );
}

/// Format only the native *sanitized* preview. Never reconstruct the preview
/// from the original editor/task. Exact text remains available independently.
class _AiRequestPreview extends StatelessWidget {
  const _AiRequestPreview(this.preview);
  final AutomationJson preview;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, raw = preview['preview'] as String;
    Map<String, dynamic>? fields;
    final separator = raw.lastIndexOf('\n\n');
    if (separator >= 0) {
      try {
        final value = jsonDecode(raw.substring(separator + 2));
        if (value is Map<String, dynamic> &&
            value['task'] is String &&
            value['path'] is String &&
            (value['selected_file'] == null || value['selected_file'] is String)) {
          fields = value;
        }
      } on FormatException {
        /* Fall back to the exact native preview. */
      }
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ContentSurface(
          child: Row(
            children: [
              Icon(preview['locality'] == 'loopback' ? Icons.computer_outlined : Icons.cloud_outlined, size: 20),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(preview['provider_name'] as String, style: Theme.of(context).textTheme.titleSmall),
                    Text(preview['locality'] == 'loopback' ? l.automationAiOnDevice : l.automationAiNetworkProvider),
                  ],
                ),
              ),
            ],
          ),
        ),
        const SizedBox(height: 12),
        Text(l.automationAiPreviewHint),
        const SizedBox(height: 12),
        if (fields != null) ...[
          Text(l.automationAiTask, style: Theme.of(context).textTheme.labelLarge),
          const SizedBox(height: 6),
          SelectableText(fields['task'] as String),
          if (fields['selected_file'] case final String selected) ...[
            const SizedBox(height: 16),
            Text(fields['path'] as String, style: Theme.of(context).textTheme.labelLarge),
            const SizedBox(height: 8),
            _AiCodeBlock(selected),
          ],
          const SizedBox(height: 12),
          ExpansionTile(
            key: const ValueKey('automation-ai-exact-request'),
            tilePadding: EdgeInsets.zero,
            title: Text(l.automationAiExactRequest),
            children: [_AiCodeBlock(raw)],
          ),
        ] else
          _AiCodeBlock(raw, height: 340),
      ],
    );
  }
}

class _AiProposalComparison extends StatelessWidget {
  const _AiProposalComparison({required this.before, required this.after});
  final String before, after;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    return SizedBox(
      height: 340,
      child: DefaultTabController(
        length: 3,
        child: Column(
          children: [
            TabBar(
              tabs: [
                Tab(text: l.automationAiChanges),
                Tab(text: l.automationAiBefore),
                Tab(text: l.automationAiAfter),
              ],
            ),
            const SizedBox(height: 8),
            Expanded(
              child: TabBarView(
                children: [
                  _AiCodeFrame(
                    height: double.infinity,
                    child: Padding(
                      padding: const EdgeInsets.all(12),
                      child: KubeYamlDiff(
                        before: before,
                        after: after,
                        explanation: '${l.automationAiBefore} → ${l.automationAiAfter}',
                      ),
                    ),
                  ),
                  _AiCodeBlock(before, height: double.infinity),
                  _AiCodeBlock(after, height: double.infinity),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Returns an editable draft only. The caller must separately save, validate,
/// review native targets and confirm execution. No execution service is passed.
class AutomationAiDialog extends ConsumerStatefulWidget {
  const AutomationAiDialog({
    super.key,
    required this.scope,
    required this.service,
    required this.path,
    required this.before,
    this.projectId,
    this.projectRevision,
    this.nativeContextScope,
    this.originalCurrent,
    this.repairOrigin,
    this.repairEvents,
    this.recheckRepair,
  });
  final AutomationScope scope;
  final AutomationAiService service;
  final String path, before;
  final String? projectId;
  final int? projectRevision;
  final RuntimeNoticeScope? nativeContextScope;
  final bool Function()? originalCurrent;
  final AutomationJson? repairOrigin;
  final AutomationEventService? repairEvents;
  final Future<bool> Function()? recheckRepair;
  @override
  ConsumerState<AutomationAiDialog> createState() => _AutomationAiDialogState();
}

class _AutomationAiDialogState extends ConsumerState<AutomationAiDialog> {
  final task = TextEditingController();
  late final selectedContext = TextEditingController(text: widget.before);
  String? provider;
  AutomationJson? contextSelection;
  bool contextValid = true, contextBusy = false;
  List<AutomationJson> repairRows = [];
  final selectedRepairRows = <int>{};
  int repairCursor = 0;
  bool repairMore = true, repairLimited = false, repairTaskInitialized = false;
  AutomationJson? preview, proposal;
  bool includeFile = false, busy = false, scopeValid = true;
  Object? error;
  Completer<void>? localLease;
  bool get current => mounted && identical(ref.read(automationScopeProvider), widget.scope) &&
    widget.originalCurrent?.call() != false;
  @override
  void initState() {
    super.initState();
    includeFile = widget.before.trim().isNotEmpty;

    ref.listenManual(
      automationScopeProvider,
      (_, next) => scopeValid = identical(next, widget.scope),
      fireImmediately: true,
    );
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (!repairTaskInitialized && widget.repairOrigin != null) {
      repairTaskInitialized = true;
      task.text = context.l10n.automationRepairTask;
    }
  }

  void finishLease() {
    final lease = localLease;
    if (lease != null && !lease.isCompleted) lease.complete();
  }

  void editPreview() {
    if (busy || preview == null) return;
    final id = preview!['review_id'] as String;
    finishLease();
    setState(() { preview = null; contextSelection = null; contextValid = true; });
    unawaited(widget.service.discardAi(id).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
  }

  @override
  void dispose() {
    finishLease();
    if (scopeValid) {
      if (preview?['review_id'] case final String id) {
        unawaited(widget.service.discardAi(id).then<void>((_) {}, onError: (Object _, StackTrace _) {}));
      }
    }
    task.dispose();
    selectedContext.dispose();
    super.dispose();
  }

  Future<void> loadRepairPage({required int after}) async {
    final origin = widget.repairOrigin, events = widget.repairEvents;
    if (busy || !current || preview != null || proposal != null || origin == null || events == null) return;
    setState(() { busy = true; error = null; repairRows = []; selectedRepairRows.clear(); });
    try {
      if (widget.recheckRepair == null || !await widget.recheckRepair!() || !current) {
        throw const AutomationAiFailed('context_changed');
      }
      final id = origin['job_id'], digest = origin['manifest_digest'];
      if (id is! String || digest is! String) throw const AutomationAiFailed('context_changed');
      final page = await events.jobEvents(id, after: after, limit: 50);
      if (!current || !identical(events, widget.repairEvents)) return;
      final projected = automationTaskResultPage(page, jobId: id, digest: digest,
        after: after, aliases: automationApprovedAliases(origin));
      setState(() {
        repairRows = projected.rows;
        repairCursor = projected.cursor;
        repairMore = projected.count == 50;
        repairLimited = projected.limited;
      });
    } catch (e) {
      if (current) setState(() => error = e);
    } finally {
      if (current) setState(() => busy = false);
    }
  }

  Future<void> prepare() async {
    if (busy || contextBusy || !contextValid || !current || provider == null || task.text.trim().isEmpty) return;
    setState(() {
      busy = true;
      error = null;
    });
    final selectedDataLabel = context.l10n.automationRepairSelectedData;
    final selectedMetadata = contextSelection;
    final contextPort = widget.service is AutomationAiContextService
      ? widget.service as AutomationAiContextService : null;
    Future<void> prepareRequest() async {
      if (widget.repairOrigin != null &&
          (widget.recheckRepair == null || !await widget.recheckRepair!() || !current)) {
        throw const AutomationAiFailed('context_changed');
      }
      final diagnostics = [for (final row in repairRows)
        if (selectedRepairRows.contains(row['sequence']) && row['censored'] != true)
          automationRepairDiagnostic(row)];
      final requestTask = diagnostics.isEmpty ? task.text
        : '${task.text}\n\n$selectedDataLabel\n${jsonEncode(diagnostics)}';
      if (!current) return;
      final AutomationJson result;
      if (selectedMetadata != null) {
        final revision = selectedMetadata['expectedProjectRevision'];
        if (contextPort == null || widget.nativeContextScope == null || widget.projectId == null || selectedMetadata['projectId'] != widget.projectId ||
            revision is! int || revision < 1) { throw const AutomationAiFailed('context_changed'); }
        result = await contextPort.prepareAiWithContext(
          profileId: widget.nativeContextScope!.profileId, notificationScopeId: widget.nativeContextScope!.nonce,
          projectId: widget.projectId!, expectedProjectRevision: revision,
          selectedHostId: selectedMetadata['selectedHostId'] as String?,
          providerId: provider!, task: requestTask, path: widget.path,
          selectedText: includeFile ? selectedContext.text : null,
          selections: (selectedMetadata['selections'] as List).cast<AutomationJson>(),
          selectedGroups: (selectedMetadata['selectedGroups'] as List).cast<String>(),
          selectedTags: (selectedMetadata['selectedTags'] as List).cast<String>(),
        );
      } else {
        result = await widget.service.prepareAi(
          providerId: provider!, task: requestTask, path: widget.path,
          selectedText: includeFile ? selectedContext.text : null,
        );
      }
      if (!current) {
        if (scopeValid) {
          unawaited(
            widget.service
                .discardAi(result['review_id'] as String)
                .then<void>((_) {}, onError: (Object _, StackTrace _) {}),
          );
        }
        return;
      }
      setState(() {
        preview = result;
        busy = false;
      });
    }

    try {
      final ai = ref.read(aiServiceProvider);
      if (provider == deviceLocalAiProviderId.value && ai is DeviceLocalAiService) {
        localLease = Completer<void>();
        // Keep this exact local endpoint alive while the user reviews context;
        // never restart it behind a frozen preview or switch to an API model.
        await ai.withDeviceLocalProvider(() async {
          await prepareRequest();
          if (current) await localLease!.future;
        });
      } else {
        await prepareRequest();
      }
    } catch (e) {
      if (current) {
        setState(() {
          error = e;
          preview = null;
        });
      }
    } finally {
      if (current && proposal == null) setState(() => busy = false);
    }
  }

  Future<void> generate() async {
    if (busy || !current || preview == null) return;
    setState(() {
      busy = true;
      error = null;
    });
    try {
      if (widget.repairOrigin != null &&
          (widget.recheckRepair == null || !await widget.recheckRepair!() || !current)) {
        throw const AutomationAiFailed('context_changed');
      }
      final result = await widget.service.generateAi(preview!['review_id'] as String);
      if (current) setState(() => proposal = result);
    } catch (e) {
      if (current) {
        setState(() {
          error = e;
          preview = null;
        });
      }
    } finally {
      finishLease();
      if (current) setState(() => busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    final providers = ref.watch(aiProvidersProvider);
    final answer = proposal;
    final proposedText = answer == null
        ? null
        : ((answer['files'] as List).first as AutomationJson)['content'] as String;
    return AlertDialog(
      title: Text(l.automationAiTitle),
      content: SizedBox(
        width: 820,
        height: MediaQuery.sizeOf(context).height * .65,
        child: ListView(
          children: [
            Text(widget.path),
            const SizedBox(height: 12),
            if (widget.repairOrigin case final origin?) ...[
              InfoBanner(message: l.automationRepairPriorChanges, tone: BannerTone.warning),
              ExpansionTile(key: const ValueKey('automation-repair-prior-results'),
                title: Text(l.automationRepairPriorResults), children: [AutomationJobResult(job: origin)]),
            ],
            if (preview == null && answer == null) ...[
              GlassSelect<String?>(
                value: provider,
                expand: true,
                semanticLabel: l.aiProviderDialogProviderLabel,
                onChanged: busy ? null : (value) => setState(() => provider = value),
                items: [
                  GlassSelectItem(value: null, label: l.aiProviderDialogProviderLabel),
                  for (final item in providers.value ?? <AiProviderConfig>[])
                    GlassSelectItem(value: item.id.value, label: '${item.name} · ${item.chatModel}'),
                ],
              ),
              if (providers.hasError) Text(errorMessage(l, providers.error!)),
              const SizedBox(height: 12),
              TextField(
                key: const ValueKey('automation-ai-task'),
                controller: task,
                minLines: 3,
                maxLines: 8,
                enabled: !busy,
                decoration: InputDecoration(labelText: l.automationAiTask),
              ),
              CheckboxListTile(
                value: includeFile,
                title: Text(l.automationAiIncludeFile),
                onChanged: busy ? null : (value) => setState(() => includeFile = value ?? false),
              ),
              if (includeFile)
                TextField(
                  controller: selectedContext,
                  minLines: 3,
                  maxLines: 8,
                  enabled: !busy,
                  decoration: InputDecoration(labelText: l.automationAiSelectedContext),
                ),
              Text(l.automationAiContextHint),
              if (widget.service is AutomationAiContextService &&
                  (widget.projectId == null || widget.scope.profileId is! ObjectId || widget.projectRevision == null ||
                   widget.projectRevision! < 1 || widget.nativeContextScope == null ||
                   widget.nativeContextScope!.profileId != (widget.scope.profileId as ObjectId).value))
                InfoBanner(key: const ValueKey('automation-ai-context-unavailable'),
                  message: l.automationAiContextUnavailable),
              if (widget.projectId != null && widget.scope.profileId is ObjectId && widget.service is AutomationAiContextService &&
                  widget.projectRevision != null && widget.projectRevision! > 0 && widget.nativeContextScope != null &&
                  widget.nativeContextScope!.profileId == (widget.scope.profileId as ObjectId).value)
                ExpansionTile(key: const ValueKey('automation-ai-selected-context'),
                  maintainState: true, title: Text(l.automationAiContextChoose),
                  children: [AutomationAiContextPicker(
                    service: widget.service as AutomationAiContextService, ai: widget.service,
                    inventory: widget.service is AutomationInventoryService ? widget.service as AutomationInventoryService : null,
                    projectId: widget.projectId!, profileId: widget.nativeContextScope!.profileId,
                    notificationScopeId: widget.nativeContextScope!.nonce, initialProjectRevision: widget.projectRevision!,
                    hosts: ref.watch(hostsProvider).value ?? const [],
                    runners: ref.watch(automationRunnersProvider).value?.runners ?? const [],
                    isCurrent: () => current, enabled: !busy,
                    onSelection: (value) { if (mounted) setState(() { contextSelection = value.$1; contextValid = value.$2; }); },
                    onBusy: (value) { if (mounted) setState(() => contextBusy = value); },
                  )]),
              if (widget.repairOrigin != null) ...[
                const SizedBox(height: 12),
                Text(l.automationRepairChooseDiagnostics),
                Text(l.automationRepairDiagnosticsHint),
                Wrap(spacing: 8, runSpacing: 4, children: [
                  TextButton(key: const ValueKey('automation-repair-load-events'),
                    onPressed: busy || widget.repairEvents == null ? null : () => loadRepairPage(after: 0),
                    child: Text(l.automationOpenOutput)),
                  if (repairMore && repairCursor > 0)
                    TextButton(key: const ValueKey('automation-repair-next-events'),
                      onPressed: busy ? null : () => loadRepairPage(after: repairCursor),
                      child: Text(l.automationInventoryNext)),
                ]),
                if (repairLimited) Text(l.automationOutputLimited),
                for (final row in repairRows)
                  CheckboxListTile(key: ValueKey('automation-repair-event-${row['sequence']}'),
                    value: selectedRepairRows.contains(row['sequence']),
                    onChanged: busy || row['censored'] == true ||
                      (!selectedRepairRows.contains(row['sequence']) && selectedRepairRows.length >= 8)
                      ? null : (value) => setState(() {
                        if (value == true) { selectedRepairRows.add(row['sequence'] as int); }
                        else { selectedRepairRows.remove(row['sequence']); }
                      }),
                    title: Text(row['censored'] == true ? l.automationEventCensored
                      : '${row['task']} · ${row['type']}'), // l10n-ignore: bounded Native task/status metadata
                    subtitle: row['censored'] == true ? null : Text /* l10n-ignore: bounded Native timestamp, approved host and explicitly selected diagnostic data */ (
                      '${row['timestamp']} · ${row['host_label']}\n${row['error'] ?? ''}'), // l10n-ignore: explicitly selectable bounded Native event data
                  ),
              ],
            ],
            if (preview != null && answer == null) _AiRequestPreview(preview!),
            if (answer != null) ...[
              Text(answer['description'] as String),
              Text(l.automationAiDraftHint),
              for (final warning in answer['warnings'] as List) Text('• $warning'),
              for (final variable in (answer['variables'] as List).cast<AutomationJson>())
                Text /* l10n-ignore: AI variable metadata */ (
                  '${variable['name']}${variable['required'] == true ? ' *' : ''}: ${variable['description']}',
                ), // l10n-ignore: AI response variable metadata, not UI copy
              for (final dependency in (answer['dependencies'] as List).cast<AutomationJson>())
                Text /* l10n-ignore: AI dependency metadata */ (
                  '${dependency['source']} · ${dependency['version']}',
                ), // l10n-ignore: AI response dependency identity and version
              for (final change in answer['expected_changes'] as List) Text('• $change'),
              for (final check in answer['checks'] as List) Text('• $check'),
              const SizedBox(height: 12),
              _AiProposalComparison(before: widget.before, after: proposedText!),
            ],
            if (busy) const LinearProgressIndicator(),
            if (error != null)
              Text(
                error is AutomationAiFailed
                    ? switch ((error as AutomationAiFailed).reason) {
                        'context_changed' => l.automationRetryUnavailable,
                        'truncated' => l.automationAiTruncated,
                        'invalid_response' => l.automationAiInvalidResponse,
                        'model_not_found' => l.automationAiModelMissing,
                        final reason when RegExp(r'^http_[1-5][0-9]{2}$').hasMatch(reason) => l.automationAiHttpError(
                          reason.substring(5),
                        ),
                        _ => l.automationAiProviderRejected,
                      }
                    : errorMessage(l, error!),
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
          ],
        ),
      ),
      actions: [
        TextButton(
          key: const ValueKey('automation-ai-reject'),
          onPressed: () => Navigator.pop(context),
          child: Text(answer == null ? l.commonCancel : l.automationAiRejectChanges),
        ),
        if (preview != null && answer == null)
          TextButton(onPressed: busy ? null : editPreview, child: Text(l.commonBack)),
        if (preview == null && answer == null)
          FilledButton(
            key: const ValueKey('automation-ai-preview'),
            onPressed: busy || contextBusy || !contextValid ? null : prepare,
            child: Text(l.automationAiPreview),
          ),
        if (preview != null && answer == null)
          FilledButton(
            key: const ValueKey('automation-ai-generate'),
            onPressed: busy ? null : generate,
            child: Text(l.automationAiGenerate),
          ),
        if (answer != null)
          FilledButton(
            key: const ValueKey('automation-ai-use'),
            onPressed: busy
                ? null
                : () {
                    try {
                      Navigator.pop(context, AutomationAiDraft(answer));
                    } catch (e) {
                      setState(() => error = e);
                    }
                  },
            child: Text(widget.before.isEmpty ? l.automationAiUseDraft : l.automationAiAcceptChanges),
          ),
      ],
    );
  }
}

/// Only the existing task-results display projection is admitted, never raw res.
AutomationJson automationRepairDiagnostic(AutomationJson row) => {
  'sequence': row['sequence'],
  'status': automationSafeEventText(row['type']),
  if (row['censored'] == true) 'censored': true,
  if (row['censored'] != true) ...{
    for (final field in ['timestamp', 'task', 'action', 'error'])
      if (row[field] is String) field: automationSafeEventText(row[field]),
    if (row['changed'] is bool) 'changed': row['changed'],
    if (row['rc'] is int) 'rc': row['rc'],
  },
};
