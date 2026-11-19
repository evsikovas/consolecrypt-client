import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/automation/automation_project_metadata_controller.dart';
import 'package:consolecrypt/automation/automation_project_settings_sync_dialog.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:flutter/foundation.dart';
import 'package:material_ui/material_ui.dart';

/// Shows inert references and publishes only after an explicit separate action.
class AutomationProjectMetadataDialog extends StatefulWidget {
  const AutomationProjectMetadataDialog({
    super.key,
    required this.service,
    required this.project,
    required this.profileId,
    required this.nonce,
    required this.originalCurrent,
    required this.originalLive,
  });
  final AutomationProjectMetadataService service;
  final AutomationProject project;
  final String profileId, nonce;
  final bool Function() originalCurrent;
  final ValueListenable<bool> originalLive;
  @override
  State<AutomationProjectMetadataDialog> createState() => _MetadataDialogState();
}

class _MetadataDialogState extends State<AutomationProjectMetadataDialog> {
  late final ProjectMetadataController controller;
  @override
  void initState() {
    super.initState();
    controller = ProjectMetadataController(
      service: widget.service,
      profileId: widget.profileId,
      nonce: widget.nonce,
      project: widget.project,
      originalCurrent: widget.originalCurrent,
    )..addListener(_changed);
    widget.originalLive.addListener(_observeOriginal);
    controller.loadContext();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  void _observeOriginal() {
    if (!widget.originalLive.value) controller.retire();
  }

  @override
  void dispose() {
    widget.originalLive.removeListener(_observeOriginal);
    controller.removeListener(_changed);
    controller.dispose();
    super.dispose();
  }

  Future<void> _syncSetup(AutomationProjectSettingsSyncService service) async {
    await showGlassDialog<void>(
      context,
      barrierDismissible: false,
      builder: (_) => AutomationProjectSettingsSyncDialog(
        service: service,
        profileId: widget.profileId,
        nonce: widget.nonce,
        originalLive: widget.originalLive,
      ),
    );
    if (mounted && controller.current) await controller.loadContext();
  }

  String? _issue(BuildContext context) {
    final l = context.l10n;
    if (controller.phase == ProjectMetadataPhase.publicationUnknown) return l.automationMetadataUnknown;
    if (controller.phase == ProjectMetadataPhase.published) return l.automationMetadataPublished;
    return switch (controller.issue) {
      ProjectMetadataIssue.localOnly => l.automationMetadataLocal,
      ProjectMetadataIssue.floorUndeclared => l.automationMetadataCompatibility,
      ProjectMetadataIssue.schemaReadOnly => l.automationMetadataReadOnly,
      ProjectMetadataIssue.reconciliationRequired => l.automationMetadataConflict,
      ProjectMetadataIssue.changed => l.automationMetadataChanged,
      ProjectMetadataIssue.unavailable => l.automationMetadataUnavailable,
      null => null,
    };
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, rows = controller.catalog?.records ?? const <ProjectMetadataRecord>[];
    final message = _issue(context), retired = controller.phase == ProjectMetadataPhase.retired;
    final service = widget.service;
    return GlassDialog(
      title: l.automationMetadataTitle,
      icon: Icons.sync_outlined,
      width: 720,
      content: SizedBox(
        height: (MediaQuery.sizeOf(context).height * .55).clamp(160.0, 420.0),
        child: CustomScrollView(
          key: const ValueKey('project-metadata-records'),
          slivers: [
            SliverToBoxAdapter(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(widget.project.name ?? '', maxLines: 1, overflow: TextOverflow.ellipsis),
                  const SizedBox(height: 8),
                  InfoBanner(message: l.automationMetadataHint),
                  if (service is AutomationProjectSettingsSyncService &&
                      controller.catalog?.releasedFloor != null &&
                      controller.issue != ProjectMetadataIssue.localOnly) ...[
                    const SizedBox(height: 8),
                    Align(
                      alignment: AlignmentDirectional.centerStart,
                      child: GlassButton(
                        label: l.automationSettingsSyncSetup,
                        icon: Icons.settings_outlined,
                        onPressed: controller.busy || retired
                            ? null
                            : () => _syncSetup(service as AutomationProjectSettingsSyncService),
                      ),
                    ),
                  ],
                  if (message != null) ...[
                    const SizedBox(height: 8),
                    InfoBanner(
                      message: message,
                      tone: controller.phase == ProjectMetadataPhase.published
                          ? BannerTone.success
                          : BannerTone.warning,
                    ),
                  ],
                  const SizedBox(height: 8),
                  if (controller.busy) const LinearProgressIndicator(minHeight: 2),
                ],
              ),
            ),
            if (rows.isEmpty)
              SliverToBoxAdapter(
                child: Padding(
                  padding: const EdgeInsets.symmetric(vertical: 24),
                  child: Center(child: Text(l.automationMetadataEmpty)),
                ),
              )
            else
              SliverList.builder(
                itemCount: rows.length,
                itemBuilder: (context, index) {
                  final row = rows[index];
                  return Padding(
                    padding: const EdgeInsets.only(bottom: 6),
                    child: ContentSurface(
                      child: ListTile(
                        key: ValueKey('project-metadata-record-${row.id}'),
                        dense: true,
                        leading: const Icon(Icons.description_outlined, size: 20),
                        title: Text(row.name, maxLines: 1, overflow: TextOverflow.ellipsis),
                        subtitle: Text(
                          '${l.automationMetadataRevision}: ${row.revision} · ${l.automationMetadataFiles}: ${row.files}'
                          '${row.unresolvedReferences.isEmpty ? '' : '\n${l.automationMetadataUnresolved}: ${row.unresolvedReferences.length}'}',
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                        ),
                        trailing: Tooltip(
                          message: l.automationMetadataRecordHint,
                          child: const Icon(Icons.lock_outline, size: 18),
                        ),
                      ),
                    ),
                  );
                },
              ),
          ],
        ),
      ),
      leadingAction: GlassButton(
        key: const ValueKey('project-metadata-refresh'),
        label: l.automationMetadataRefresh,
        icon: Icons.refresh,
        onPressed: controller.busy || retired
            ? null
            : () {
                if (controller.phase == ProjectMetadataPhase.publicationUnknown) {
                  controller.readCatalog();
                } else {
                  controller.loadContext();
                }
              },
      ),
      secondaryActions: [
        GlassButton(
          label: l.commonClose,
          onPressed: controller.phase == ProjectMetadataPhase.publishing ? null : () => Navigator.pop(context),
        ),
      ],
      primaryAction: GlassButton(
        key: const ValueKey('project-metadata-publish'),
        label: l.automationMetadataPublish,
        icon: Icons.cloud_upload_outlined,
        busy: controller.phase == ProjectMetadataPhase.publishing,
        onPressed: controller.canRequestPublish
            ? () {
                controller.publish();
              }
            : null,
      ),
    );
  }
}
