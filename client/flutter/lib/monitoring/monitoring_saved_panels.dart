import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_prefill.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';
import 'package:material_ui/material_ui.dart';

String? savedPanelAdmissionMessage(AppLocalizations l, Object error) =>
    error is AppException &&
        error.code == AppErrorCode.validation &&
        error.reason == AppErrorReason.invalidField &&
        error.args['field'] == 'monitoring_panels' &&
        error.args['rule'] == 'panel_sync_admission_required'
    ? l.monitoringSavedPanelSyncAdmission
    : null;

String savedPanelUnitLabel(AppLocalizations l, SavedPanelUnitHint hint) => switch (hint) {
  SavedPanelUnitHint.unknown => l.monitoringSavedPanelHintUnknown,
  SavedPanelUnitHint.percent => l.monitoringSavedPanelHintPercent,
  SavedPanelUnitHint.bytes => l.monitoringSavedPanelHintBytes,
  SavedPanelUnitHint.bytesPerSecond => l.monitoringSavedPanelHintBytesPerSecond,
  SavedPanelUnitHint.seconds => l.monitoringSavedPanelHintSeconds,
  SavedPanelUnitHint.count => l.monitoringSavedPanelHintCount,
  SavedPanelUnitHint.perSecond => l.monitoringSavedPanelHintPerSecond,
};

/// Pure editor/list: the workspace owns every request, lifecycle generation and
/// cancellation. This child never creates/disposes a service or cancels siblings.
class MonitoringSavedPanels extends StatefulWidget {
  const MonitoringSavedPanels({
    super.key,
    required this.panels,
    required this.source,
    required this.enabled,
    required this.canRead,
    required this.sourceNames,
    required this.busy,
    required this.onSave,
    required this.onDelete,
    required this.onRead,
    this.onPreview,
    this.previewControls,
    this.previewEnabled = true,
    this.onImport,
    this.onOpenGrafana,
    this.prefill,
    this.onPrefillHandled,
    this.syncSnapshot,
    this.syncChecking = false,
    this.syncFailed = false,
    this.canSave = true,
    this.onCheckSync,
    this.onReviewSyncConsent,
  });
  final VoidCallback? onImport, onOpenGrafana, onCheckSync, onReviewSyncConsent;
  final MonitoringPanelSyncSnapshot? syncSnapshot;
  final bool syncChecking, syncFailed, canSave;
  final ValueChanged<SavedPanelDefinition>? onPreview;
  final Widget? previewControls;
  final bool previewEnabled;
  final GrafanaPanelPrefill? prefill;
  final ValueChanged<GrafanaPanelPrefill>? onPrefillHandled;
  final List<SavedMonitoringPanel> panels;
  final MonitoringSource? source;
  final bool enabled, busy, canRead;
  final Map<String, String> sourceNames;
  final Future<bool> Function(SavedPanelDefinition, SavedMonitoringPanel?) onSave;
  final Future<bool> Function(SavedMonitoringPanel) onDelete;
  final void Function(SavedMonitoringPanel, bool range) onRead;
  @override
  State<MonitoringSavedPanels> createState() => _MonitoringSavedPanelsState();
}

class _MonitoringSavedPanelsState extends State<MonitoringSavedPanels> {
  final title = TextEditingController(), query = TextEditingController();
  SavedPanelUnitHint hint = SavedPanelUnitHint.unknown;
  SavedMonitoringPanel? previous;
  bool editing = false, confirmImportReplacement = false;
  String? deleting;
  GrafanaPanelPrefill? appliedPrefill;
  @override
  void initState() {
    super.initState();
    acceptPrefill();
  }

  void acceptPrefill() {
    final draft = widget.prefill, target = widget.source;
    if (draft == null || identical(draft, appliedPrefill) || target == null || !draft.matches(target)) return;
    appliedPrefill = draft;
    previous = null;
    title.text = draft.definition.title;
    query.text = draft.definition.query;
    hint = draft.definition.unitHint;
    editing = true;
    deleting = null;
    confirmImportReplacement = false;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) widget.onPrefillHandled?.call(draft);
    });
  }

  @override
  void dispose() {
    title.dispose();
    query.dispose();
    super.dispose();
  }

  @override
  void didUpdateWidget(MonitoringSavedPanels oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.source?.fingerprint != widget.source?.fingerprint) {
      editing = false;
      previous = null;
      deleting = null;
      confirmImportReplacement = false;
    }
    acceptPrefill();
  }

  void edit(SavedMonitoringPanel? panel) => setState(() {
    previous = panel;
    title.text = panel?.definition.title ?? '';
    query.text = panel?.definition.query ?? '';
    hint = panel?.definition.unitHint ?? SavedPanelUnitHint.unknown;
    editing = true;
    deleting = null;
    confirmImportReplacement = false;
  });
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, source = widget.source;
    final canEdit = widget.enabled && !widget.busy && source != null && !source.readOnly;
    String stateLabel(SavedPanelState state) => switch (state) {
      SavedPanelState.current => l.monitoringSavedPanelCurrent,
      SavedPanelState.staleSource => l.monitoringSavedPanelStale,
      SavedPanelState.missingSource => l.monitoringSavedPanelMissing,
    };
    return ContentSurface(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(l.monitoringSavedPanels, style: Theme.of(context).textTheme.titleMedium),
          Text(l.monitoringSavedPanelIntro),
          MonitoringPanelSyncStatus(snapshot:widget.syncSnapshot,busy:widget.syncChecking,failed:widget.syncFailed,onCheck:widget.onCheckSync),
          if(widget.onReviewSyncConsent!=null)Align(alignment:Alignment.centerLeft,child:TextButton.icon(
            key:const ValueKey('panel-policy-open'),onPressed:widget.busy?null:widget.onReviewSyncConsent,
            icon:const Icon(Icons.verified_user_outlined),label:Text(l.monitoringPanelPolicyOpen))),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              OutlinedButton(
                key: const ValueKey('saved-panel-new'),
                onPressed: canEdit && widget.panels.length < 64 ? () => edit(null) : null,
                child: Text(l.monitoringSavedPanelNew),
              ),
              if (widget.onImport != null)
                OutlinedButton(
                  key: const ValueKey('saved-panel-import-grafana'),
                  onPressed: widget.enabled && !widget.busy && widget.panels.length < 64
                      ? () {
                          if (editing && (title.text.isNotEmpty || query.text.isNotEmpty)) {
                            setState(() => confirmImportReplacement = true);
                          } else {
                            widget.onImport?.call();
                          }
                        }
                      : null,
                  child: Text(l.grafanaImportTitle),
                ),
              if (widget.onOpenGrafana != null)
                OutlinedButton(
                  key: const ValueKey('saved-panel-open-grafana'),
                  onPressed: !widget.busy ? widget.onOpenGrafana : null,
                  child: Text(l.grafanaOpenTitle),
                ),
            ],
          ),
          if (confirmImportReplacement) ...[
            InfoBanner(
              key: const ValueKey('saved-panel-import-replacement'),
              message: l.grafanaImportReplaceWarning,
              tone: BannerTone.warning,
            ),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                TextButton(
                  key: const ValueKey('saved-panel-import-replace'),
                  onPressed: widget.enabled && !widget.busy
                      ? () {
                          setState(() => confirmImportReplacement = false);
                          widget.onImport?.call();
                        }
                      : null,
                  child: Text(l.grafanaImportReplace),
                ),
                TextButton(
                  key: const ValueKey('saved-panel-import-keep'),
                  onPressed: () => setState(() => confirmImportReplacement = false),
                  child: Text(l.grafanaImportKeepDraft),
                ),
              ],
            ),
          ],
          if (widget.panels.isEmpty) Text(l.monitoringSavedPanelEmpty),
          for (final panel in widget.panels)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 6),
              child: Column(
                key: ValueKey('saved-panel-${panel.id}'),
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(panel.definition.title, style: Theme.of(context).textTheme.titleSmall),
                  Text(stateLabel(panel.state)),
                  Text(
                    '${widget.sourceNames[panel.definition.sourceId] ?? l.monitoringSavedPanelMissing} · ${l.monitoringBindingPinnedRevision}: ${panel.definition.sourceRevision}',
                  ),
                  Text('${l.monitoringSavedPanelHint}: ${savedPanelUnitLabel(l, panel.definition.unitHint)}'),
                  Wrap(
                    spacing: 8,
                    runSpacing: 4,
                    children: [
                      TextButton(
                        onPressed: canEdit ? () => edit(panel) : null,
                        child: Text(l.monitoringSavedPanelEdit),
                      ),
                      TextButton(
                        key: ValueKey('saved-panel-read-${panel.id}'),
                        onPressed:
                            !widget.busy &&
                                widget.enabled &&
                                widget.canRead &&
                                source != null &&
                                !source.readOnly &&
                                panel.currentFor(source)
                            ? () => widget.onRead(panel, false)
                            : null,
                        child: Text(l.monitoringQueryCurrent),
                      ),
                      TextButton(
                        key: ValueKey('saved-panel-range-${panel.id}'),
                        onPressed:
                            !widget.busy &&
                                widget.enabled &&
                                widget.canRead &&
                                source != null &&
                                !source.readOnly &&
                                panel.currentFor(source)
                            ? () => widget.onRead(panel, true)
                            : null,
                        child: Text(l.monitoringQueryRange),
                      ),
                      TextButton(
                        onPressed: widget.enabled && !widget.busy ? () => setState(() => deleting = panel.id) : null,
                        child: Text(l.monitoringSavedPanelDelete),
                      ),
                    ],
                  ),
                  if (deleting == panel.id)
                    Wrap(
                      spacing: 8,
                      runSpacing: 4,
                      children: [
                        Text(l.monitoringSavedPanelDeleteConfirm),
                        TextButton(
                          onPressed: widget.enabled && !widget.busy
                              ? () async {
                                  if (await widget.onDelete(panel) && mounted) {
                                    setState(() {
                                      deleting = null;
                                      editing = false;
                                    });
                                  }
                                }
                              : null,
                          child: Text(l.monitoringConfirmDelete),
                        ),
                        TextButton(
                          onPressed: widget.busy ? null : () => setState(() => deleting = null),
                          child: Text(l.commonCancel),
                        ),
                      ],
                    ),
                ],
              ),
            ),
          if (editing) ...[
            const SizedBox(height: 12),
            if (source != null) Text('${source.name} · ${l.monitoringBindingRevision}: ${source.revision}'),
            TextField(
              key: const ValueKey('saved-panel-title'),
              controller: title,
              enabled: canEdit,
              maxLength: 120,
              decoration: InputDecoration(labelText: l.monitoringSavedPanelTitle),
            ),
            TextField(
              key: const ValueKey('saved-panel-query'),
              controller: query,
              enabled: canEdit,
              minLines: 2,
              maxLines: 8,
              maxLength: 16384,
              decoration: InputDecoration(labelText: l.monitoringExpression),
            ),
            DropdownButtonFormField<SavedPanelUnitHint>(
              key: ValueKey('saved-panel-unit-${hint.name}'),
              initialValue: hint,
              decoration: InputDecoration(labelText: l.monitoringSavedPanelHint),
              items: SavedPanelUnitHint.values
                  .map((h) => DropdownMenuItem(value: h, child: Text(savedPanelUnitLabel(l, h))))
                  .toList(),
              onChanged: canEdit ? (value) => setState(() => hint = value!) : null,
            ),
            const SizedBox(height: 8),
            if (widget.onPreview != null) ...[
              Text(l.monitoringSavedPanelPreviewHelp),
              if (widget.previewControls != null) widget.previewControls!,
              const SizedBox(height: 8),
            ],
            Text(l.monitoringSavedPanelReview),
            Wrap(
              spacing: 8,
              children: [
                if (widget.onPreview != null)
                  FilledButton.icon(
                    key: const ValueKey('saved-panel-preview'),
                    onPressed: !widget.previewEnabled || !canEdit
                        ? null
                        : () => widget.onPreview!(
                            SavedPanelDefinition(
                              title: title.text.trim(),
                              sourceId: source.id,
                              sourceRevision: source.revision,
                              query: query.text,
                              unitHint: hint,
                            ),
                          ),
                    icon: const Icon(Icons.show_chart),
                    label: Text(l.monitoringSavedPanelPreview),
                  ),
                FilledButton(
                  key: const ValueKey('saved-panel-save'),
                  onPressed: !canEdit || !widget.canSave
                      ? null
                      : () async {
                          final target = widget.source;
                          if (target == null) return;
                          final definition = SavedPanelDefinition(
                            title: title.text.trim(),
                            sourceId: target.id,
                            sourceRevision: target.revision,
                            query: query.text,
                            unitHint: hint,
                          );
                          if (await widget.onSave(definition, previous) && mounted) {
                            setState(() {
                              editing = false;
                              previous = null;
                            });
                          }
                        },
                  child: Text(l.monitoringSavedPanelSave),
                ),
                TextButton(
                  onPressed: widget.busy ? null : () => setState(() => editing = false),
                  child: Text(l.commonCancel),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }
}


/// Fixed diagnostic observations only. There is no Ready/enable switch here.
class MonitoringPanelSyncStatus extends StatelessWidget {
  const MonitoringPanelSyncStatus({super.key, this.snapshot, this.busy = false, this.failed = false, this.onCheck});
  final MonitoringPanelSyncSnapshot? snapshot;
  final bool busy, failed;
  final VoidCallback? onCheck;
  @override
  Widget build(BuildContext context) {
    final l = context.l10n, value = snapshot;
    if (value?.storage == PanelSyncStorage.local) {
      return InfoBanner(key: const ValueKey('panel-sync-local'), message: l.monitoringPanelSyncLocal);
    }
    final serverText = switch (value?.server) {
      null => l.monitoringPanelSyncUnavailable,
      PanelSyncServer.notChecked => l.monitoringPanelSyncNotChecked,
      PanelSyncServer.unsupported => l.monitoringPanelSyncUnsupported,
      PanelSyncServer.unavailable => l.monitoringPanelSyncServiceUnavailable,
      PanelSyncServer.timedOut => l.monitoringPanelSyncTimedOut,
      PanelSyncServer.authenticationRequired || PanelSyncServer.deviceRevoked || PanelSyncServer.proofRequired => l.monitoringPanelSyncAccess,
      PanelSyncServer.upgradeRequired => l.monitoringPanelSyncUpgrade,
      PanelSyncServer.quarantined => l.monitoringPanelSyncQuarantined,
      PanelSyncServer.invalid || PanelSyncServer.refused => l.monitoringPanelSyncInvalid,
      PanelSyncServer.available => switch (value?.account) {
        PanelSyncAccount.off => l.monitoringPanelSyncAccountUnset,
        PanelSyncAccount.quarantined => l.monitoringPanelSyncQuarantined,
        _ => l.monitoringPanelSyncPending,
      },
    };
    final readiness = switch (value?.readiness) {
      PanelSyncReadiness.floorUndeclared => l.monitoringPanelSyncFloorMissing,
      PanelSyncReadiness.rosterUnavailable || PanelSyncReadiness.rosterInvalid => l.monitoringPanelSyncDevicesUnavailable,
      PanelSyncReadiness.devicesUnsupported => l.monitoringPanelSyncBlocked(value!.blockedDevices.toString()),
      _ => l.monitoringPanelSyncPending,
    };
    return Column(key: const ValueKey('panel-sync-diagnostics'), crossAxisAlignment: CrossAxisAlignment.stretch, children: [
      InfoBanner(message: value == null ? l.monitoringPanelSyncUnavailable : l.monitoringSavedPanelSyncAdmission,
        tone: BannerTone.warning),
      if (value != null) Text(readiness, key: const ValueKey('panel-sync-readiness')),
      if(value!=null&&value.deviceBlockers.isNotEmpty)ExpansionTile(key:const ValueKey('panel-sync-device-blockers'),
        title:Text(l.monitoringPanelSyncDeviceReasons),
        children:[Text(l.monitoringPanelSyncCheckedDevices(value.checkedDevices.toString())),
          SizedBox(height:(value.deviceBlockers.length*88.0).clamp(88.0,264.0),child:ListView.builder(
            itemCount:value.deviceBlockers.length,itemBuilder:(context,index){final blocker=value.deviceBlockers[index];
              return Padding(padding:const EdgeInsets.symmetric(vertical:8),child:Column(crossAxisAlignment:CrossAxisAlignment.stretch,children:[
                SelectableText(blocker.deviceId,key:ValueKey('panel-sync-device-${blocker.deviceId}')),
                Text(switch(blocker.reason){PanelSyncBlockerReason.unknownVersion=>l.monitoringPanelSyncUnknownDeviceVersion,
                  PanelSyncBlockerReason.invalidVersion=>l.monitoringPanelSyncInvalidDeviceVersion,
                  PanelSyncBlockerReason.prereleaseVersion=>l.monitoringPanelSyncPrereleaseDeviceVersion,
                  PanelSyncBlockerReason.olderVersion=>l.monitoringPanelSyncOlderDeviceVersion}),
              ]));
            }))]),
      if (value?.releasedFloor case final version?) Text(l.monitoringPanelSyncReleasedFloor(version)),
      if (value != null && value.server != PanelSyncServer.notChecked) Text(l.monitoringPanelSyncObserved),
      if (value?.accountFloor case final version?) Text(l.monitoringPanelSyncAccountFloor(version)),
      Text(serverText, key: const ValueKey('panel-sync-server')),
      if (failed) InfoBanner(key: const ValueKey('panel-sync-failed'), tone: BannerTone.danger, message:l.monitoringPanelSyncCheckFailed),
      if (busy) const LinearProgressIndicator(key: ValueKey('panel-sync-checking')),
      Align(alignment:Alignment.centerLeft, child:TextButton.icon(key: const ValueKey('panel-sync-check'),
        onPressed:busy ? null : onCheck, icon:const Icon(Icons.fact_check_outlined),
        label:Text(busy ? l.monitoringPanelSyncChecking : l.monitoringPanelSyncCheck))),
    ]);
  }
}
