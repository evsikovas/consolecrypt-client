import 'package:consolecrypt/automation/automation_project_metadata.dart';
import 'package:consolecrypt/automation/automation_service.dart';
import 'package:flutter/foundation.dart';

enum ProjectMetadataPhase { idle, reading, ready, publishing, published, publicationUnknown, failed, retired }

enum ProjectMetadataIssue { unavailable, changed, localOnly, floorUndeclared, schemaReadOnly, reconciliationRequired }

/// One original project/profile/unlock only. Constructor and timers do no I/O.
/// Native remains responsible for all admission, encryption and commit checks.
final class ProjectMetadataController extends ChangeNotifier {
  ProjectMetadataController({
    required this.service,
    required this.profileId,
    required this.nonce,
    required this.project,
    required this.originalCurrent,
  });
  final AutomationProjectMetadataService service;
  final String profileId, nonce;
  final AutomationProject project;
  final bool Function() originalCurrent;
  ProjectMetadataPhase phase = ProjectMetadataPhase.idle;
  ProjectMetadataIssue? issue;
  ProjectMetadataContext? context;
  ProjectMetadataCatalog? catalog;
  int _generation = 0;
  bool _alive = true;
  bool get busy => phase == ProjectMetadataPhase.reading || phase == ProjectMetadataPhase.publishing;
  bool get current => _alive && phase != ProjectMetadataPhase.retired && originalCurrent();
  bool get canRequestPublish => current && !busy && context?.canRequestPublish == true;
  void _changed() {
    if (_alive) notifyListeners();
  }

  void retire() {
    if (!_alive || phase == ProjectMetadataPhase.retired) return;
    _generation++;
    context = null;
    catalog = null;
    issue = ProjectMetadataIssue.changed;
    phase = ProjectMetadataPhase.retired;
    _changed();
  }

  bool _same(int generation) {
    if (!current) {
      retire();
      return false;
    }
    return generation == _generation;
  }

  ProjectMetadataIssue? _blocked(ProjectMetadataContext value) {
    if (value.catalog.reconciliationRequired) return ProjectMetadataIssue.reconciliationRequired;
    if (!value.catalog.schemaSupported || value.catalog.readOnly) return ProjectMetadataIssue.schemaReadOnly;
    return switch (value.syncState) {
      'local_only' => ProjectMetadataIssue.localOnly,
      'floor_undeclared' => ProjectMetadataIssue.floorUndeclared,
      _ => null,
    };
  }

  Future<void> loadContext() async {
    if (!current) {
      retire();
      return;
    }
    if (busy) return;
    final generation = ++_generation;
    context = null;
    issue = null;
    phase = ProjectMetadataPhase.reading;
    _changed();
    try {
      final raw = await service.projectMetadataContext(project.id);
      if (!_same(generation)) return;
      final parsed = ProjectMetadataContext.parse(raw, profileId: profileId, nonce: nonce, project: project);
      context = parsed;
      catalog = parsed.catalog;
      issue = _blocked(parsed);
      phase = ProjectMetadataPhase.ready;
    } on FormatException {
      if (!_same(generation)) return;
      issue = ProjectMetadataIssue.changed;
      phase = ProjectMetadataPhase.failed;
    } catch (_) {
      if (!_same(generation)) return;
      issue = ProjectMetadataIssue.unavailable;
      phase = ProjectMetadataPhase.failed;
    }
    _changed();
  }

  Future<void> readCatalog() async {
    if (!current) {
      retire();
      return;
    }
    if (busy) return;
    final generation = ++_generation;
    // A separate catalog observation never silently refreshes a publish frame.
    context = null;
    issue = null;
    phase = ProjectMetadataPhase.reading;
    _changed();
    try {
      final raw = await service.projectMetadataCatalog();
      if (!_same(generation)) return;
      catalog = checkedProjectMetadataReply(raw, profileId: profileId, nonce: nonce);
      if (catalog!.reconciliationRequired) issue = ProjectMetadataIssue.reconciliationRequired;
      if (!catalog!.schemaSupported) issue = ProjectMetadataIssue.schemaReadOnly;
      phase = ProjectMetadataPhase.ready;
    } on FormatException {
      if (!_same(generation)) return;
      issue = ProjectMetadataIssue.changed;
      phase = ProjectMetadataPhase.failed;
    } catch (_) {
      if (!_same(generation)) return;
      issue = ProjectMetadataIssue.unavailable;
      phase = ProjectMetadataPhase.failed;
    }
    _changed();
  }

  Future<bool> publish() async {
    if (!current) {
      retire();
      return false;
    }
    final captured = context;
    if (busy || captured == null || !captured.canRequestPublish) return false;
    final generation = ++_generation;
    final previous = captured.catalog.find(project.id)?.revision ?? 0;
    final request = captured.frame.publishRequest(previous);
    // Consume this UI frame before the call. Errors never auto-retry or invent
    // rollback; only a later explicit read may observe the actual catalog.
    context = null;
    issue = null;
    phase = ProjectMetadataPhase.publishing;
    _changed();
    try {
      final raw = await service.publishProjectMetadata(request);
      if (!_same(generation)) return false;
      final actual = checkedProjectMetadataReply(raw, profileId: profileId, nonce: nonce, projectId: project.id);
      if (actual.readOnly ||
          actual.revision != captured.catalog.revision! + 1 ||
          actual.find(project.id)?.revision != previous + 1) {
        throw const FormatException('automation_metadata_publication_changed');
      }
      catalog = actual;
      phase = ProjectMetadataPhase.published;
      _changed();
      return true;
    } catch (_) {
      if (!_same(generation)) return false;
      issue = ProjectMetadataIssue.unavailable;
      phase = ProjectMetadataPhase.publicationUnknown;
      _changed();
      return false;
    }
  }

  @override
  void dispose() {
    _alive = false;
    _generation++;
    context = null;
    catalog = null;
    super.dispose();
  }
}
