import 'package:consolecrypt/monitoring/grafana_constant_review.dart';
import 'package:consolecrypt/monitoring/grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:consolecrypt/monitoring/monitoring_saved_panel_models.dart';

/// An editor draft, never an approval or persistence/read capability. The host
/// must validate its captured profile/store/catalog context before accepting it.
class GrafanaPanelPrefill {
  const GrafanaPanelPrefill._(this.definition, this.sourceFingerprint);
  final SavedPanelDefinition definition;
  final String sourceFingerprint;

  static GrafanaPanelPrefill? choose(GrafanaImportReport report, String panelPath, MonitoringSource source) {
    if (source.readOnly || source.revision < 1) return null;
    final panel = report.panels.where((p) => p.path == panelPath).firstOrNull;
    final draft = panel?.draft;
    if (draft == null) return null;
    final definition = SavedPanelDefinition(
      title: draft.title,
      query: draft.expression,
      sourceId: source.id,
      sourceRevision: source.revision,
      // Grafana unit strings never establish validated metric units.
      unitHint: SavedPanelUnitHint.unknown,
    );
    definition.validate();
    return GrafanaPanelPrefill._(definition, source.fingerprint);
  }

  /// Only the private-constructor reviewed resolver output may enter this
  /// path. A caller-created panel draft or a raw variable map is insufficient.
  static GrafanaPanelPrefill? fromConstants(GrafanaConstantReview review, MonitoringSource source) {
    if (source.readOnly || source.revision < 1) return null;
    final definition = SavedPanelDefinition(
      title: review.draft.title,
      query: review.draft.expression,
      sourceId: source.id,
      sourceRevision: source.revision,
      unitHint: SavedPanelUnitHint.unknown,
    );
    definition.validate();
    return GrafanaPanelPrefill._(definition, source.fingerprint);
  }

  bool matches(MonitoringSource source) =>
      !source.readOnly &&
      source.fingerprint == sourceFingerprint &&
      source.id == definition.sourceId &&
      source.revision == definition.sourceRevision;
}
