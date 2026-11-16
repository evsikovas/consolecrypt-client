import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/monitoring/grafana_constant_review.dart';
import 'package:consolecrypt/monitoring/grafana_import.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_buffer.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_constants.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_input.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_prefill.dart';
import 'package:consolecrypt/monitoring/monitoring_grafana_worker.dart';
import 'package:consolecrypt/monitoring/monitoring_models.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

String grafanaImportProblemLabel(AppLocalizations l, GrafanaImportProblem p) => switch (p) {
  GrafanaImportProblem.invalidJson => l.grafanaImportInvalidJson,
  GrafanaImportProblem.inputTooLarge => l.grafanaImportTooLarge,
  GrafanaImportProblem.tooDeep => l.grafanaImportTooDeep,
  GrafanaImportProblem.duplicateKey => l.grafanaImportDuplicateKey,
  GrafanaImportProblem.invalidDashboard => l.grafanaImportInvalidDashboard,
  GrafanaImportProblem.unsupportedSchema => l.grafanaImportUnsupportedSchema,
  GrafanaImportProblem.invalidPanel => l.grafanaImportInvalidPanel,
  GrafanaImportProblem.tooManyPanels => l.grafanaImportTooManyPanels,
  GrafanaImportProblem.unsupportedPanel => l.grafanaImportUnsupportedPanel,
  GrafanaImportProblem.unsupportedDatasource => l.grafanaImportUnsupportedDatasource,
  GrafanaImportProblem.missingExpression => l.grafanaImportMissingExpression,
  GrafanaImportProblem.multipleTargets => l.grafanaImportMultipleTargets,
  GrafanaImportProblem.hiddenTarget => l.grafanaImportHiddenTarget,
  GrafanaImportProblem.variables => l.grafanaImportVariables,
  GrafanaImportProblem.transformations => l.grafanaImportTransformations,
  GrafanaImportProblem.repeat => l.grafanaImportRepeat,
  GrafanaImportProblem.timeOverride => l.grafanaImportTimeOverride,
  GrafanaImportProblem.legacyAlert => l.grafanaImportLegacyAlert,
  GrafanaImportProblem.unitHintOnly => l.grafanaImportUnitHintOnly,
  GrafanaImportProblem.presentationNotImported => l.grafanaImportPresentation,
};

/// The framework runs this before committing the value to EditableText. IME
/// and platform paste paths therefore have the same bounded projection guard.
class _GrafanaProjectionFormatter extends TextInputFormatter {
  _GrafanaProjectionFormatter(this.accept);
  final GrafanaInputBuffer Function(String) accept;
  @override
  TextEditingValue formatEditUpdate(TextEditingValue oldValue, TextEditingValue newValue) {
    final buffer = accept(newValue.text);
    if (buffer.display == newValue.text) return newValue;
    return TextEditingValue(
      text: buffer.display,
      selection: TextSelection.collapsed(offset: buffer.display.length),
    );
  }
}

/// Offline review only. No service, store, URL launcher or query callback is
/// available here. Continue returns a single draft to the existing editor.
class MonitoringGrafanaImport extends StatefulWidget {
  const MonitoringGrafanaImport({
    super.key,
    required this.sources,
    required this.isCurrent,
    required this.loadJson,
    required this.onContinue,
    required this.onCancel,
    this.inspectorFactory,
    this.readClipboard,
  });
  final List<MonitoringSource> sources;
  final bool Function() isCurrent;
  final Future<String?> Function() loadJson;
  final ValueChanged<GrafanaPanelPrefill> onContinue;
  final VoidCallback onCancel;
  final GrafanaDashboardInspector Function()? inspectorFactory;
  final Future<String?> Function()? readClipboard;
  @override
  State<MonitoringGrafanaImport> createState() => _MonitoringGrafanaImportState();
}

class _MonitoringGrafanaImportState extends State<MonitoringGrafanaImport> {
  final input = TextEditingController();
  GrafanaImportReport? report;
  GrafanaInputBuffer buffer = GrafanaInputBuffer.empty;
  GrafanaInspection? inspection;
  GrafanaConstantReview? constantReview;
  GrafanaConstantProblem? constantProblem;
  Map<String, String> constantValues = const {};
  String? reviewedSourceFingerprint;
  bool reviewingConstants = false;
  late final GrafanaDashboardInspector inspector;
  late final TextInputFormatter projectionFormatter;
  bool projecting = false, inspecting = false, pasting = false;
  String? panelPath, sourceId;
  GrafanaImportProblem? problem;
  GrafanaInputFailure? inputFailure;
  bool loading = false;
  int generation = 0;
  bool get current => mounted && widget.isCurrent();
  MonitoringSource? get source => widget.sources.where((s) => s.id == sourceId).firstOrNull;
  GrafanaPanelReview? get panel => report?.panels.where((p) => p.path == panelPath).firstOrNull;
  GrafanaConstantReview? get reviewedConstants {
    final value = constantReview;
    if (value == null ||
        value.original.originalJson != buffer.original ||
        value.panelPath != panelPath ||
        source?.fingerprint != reviewedSourceFingerprint) {
      return null;
    }
    return value;
  }

  bool canReviewConstants(GrafanaPanelReview row) =>
      inspector is GrafanaManualConstantInspector &&
      (inspection?.constantNames.isNotEmpty ?? false) &&
      row.problems.contains(GrafanaImportProblem.variables) &&
      row.problems.every(
        (problem) => problem == GrafanaImportProblem.variables || problem == GrafanaImportProblem.unitHintOnly,
      );

  void clearConstantReview({bool clearValues = false}) {
    constantReview = null;
    constantProblem = null;
    reviewedSourceFingerprint = null;
    reviewingConstants = false;
    if (clearValues) constantValues = const {};
  }

  void selectPanel(String path, GrafanaImportReport expected) {
    if (!current || !identical(report, expected) || panelPath == path) return;
    generation++;
    inspector.cancel();
    setState(() {
      panelPath = path;
      clearConstantReview(clearValues: true);
    });
  }

  void selectSource(String? id) {
    if (!current || report == null) return;
    generation++;
    inspector.cancel();
    setState(() {
      sourceId = id;
      clearConstantReview();
    });
  }

  void editConstantValues(Map<String, String> values) {
    if (!current || report == null) return;
    generation++;
    inspector.cancel();
    setState(() {
      constantValues = Map.unmodifiable(values);
      clearConstantReview();
    });
  }

  Future<void> reviewConstants() async {
    final worker = inspector, target = source, path = panelPath, original = buffer;
    if (!current ||
        worker is! GrafanaManualConstantInspector ||
        target == null ||
        path == null ||
        constantValues.isEmpty ||
        loading ||
        inspecting ||
        pasting ||
        reviewingConstants) {
      return;
    }
    final token = ++generation, values = Map<String, String>.of(constantValues), fingerprint = target.fingerprint;
    setState(() {
      clearConstantReview();
      reviewingConstants = true;
    });
    try {
      final value = await (worker as GrafanaManualConstantInspector).reviewConstants(original.original, path, values);
      if (!current ||
          token != generation ||
          !identical(original, buffer) ||
          source?.fingerprint != fingerprint ||
          value.original.originalJson != original.original ||
          value.panelPath != path) {
        return;
      }
      setState(() {
        constantReview = value;
        reviewedSourceFingerprint = fingerprint;
      });
    } on GrafanaInspectionCancelled {
      // An explicit edit, cancellation or scope change owns the new state.
    } on GrafanaConstantException catch (failure) {
      if (current && token == generation) setState(() => constantProblem = failure.problem);
    } on GrafanaImportException catch (failure) {
      if (current && token == generation) setState(() => problem = failure.problem);
    } catch (_) {
      if (current && token == generation) setState(() => inputFailure = GrafanaInputFailure.readFailed);
    } finally {
      if (mounted && token == generation) setState(() => reviewingConstants = false);
    }
  }

  @override
  void initState() {
    super.initState();
    inspector = widget.inspectorFactory?.call() ?? IsolatedGrafanaDashboardInspector();
    projectionFormatter = _GrafanaProjectionFormatter((text) => acceptInput(text, fromFormatter: true));
    input.addListener(changed);
  }

  void changed() {
    if (projecting || input.text == buffer.display) return;
    acceptInput(input.text);
  }

  GrafanaInputBuffer acceptInput(String text, {bool fromFormatter = false}) {
    if (!current) return GrafanaInputBuffer.empty;
    GrafanaInputFailure? failure;
    GrafanaInputBuffer accepted;
    try {
      accepted = GrafanaInputBuffer.accept(text);
    } on GrafanaInputException catch (error) {
      accepted = GrafanaInputBuffer.empty;
      failure = error.reason;
    }
    generation++;
    inspector.cancel();
    setState(() {
      buffer = accepted;
      report = null;
      inspection = null;
      panelPath = null;
      sourceId = null;
      clearConstantReview(clearValues: true);
      problem = null;
      inputFailure = failure;
      inspecting = false;
      pasting = false;
    });
    if (!fromFormatter) {
      projecting = true;
      input.value = TextEditingValue(
        text: accepted.display,
        selection: TextSelection.collapsed(offset: accepted.display.length),
      );
      projecting = false;
    }
    return accepted;
  }

  Future<void> paste({bool replace = false}) async {
    if (!current || loading || pasting) return;
    final token = ++generation, original = buffer, selection = input.selection;
    inspector.cancel();
    setState(() {
      pasting = true;
      inspecting = false;
      report = null;
      inspection = null;
      panelPath = null;
      sourceId = null;
      clearConstantReview(clearValues: true);
    });
    try {
      final text =
          await (widget.readClipboard?.call() ?? Clipboard.getData(Clipboard.kTextPlain).then((data) => data?.text));
      if (!current ||
          token != generation ||
          !identical(original, buffer) ||
          selection != input.selection ||
          text == null) {
        return;
      }
      if (replace || buffer.previewOnly || !selection.isValid) {
        acceptInput(text);
      } else {
        acceptInput(buffer.original.replaceRange(selection.start, selection.end, text));
      }
    } catch (_) {
      if (current && token == generation) setState(() => inputFailure = GrafanaInputFailure.readFailed);
    } finally {
      if (mounted && token == generation) setState(() => pasting = false);
    }
  }

  Widget toolbar(BuildContext context, EditableTextState state) => AdaptiveTextSelectionToolbar.buttonItems(
    anchors: state.contextMenuAnchors,
    buttonItems: [
      for (final item in state.contextMenuButtonItems)
        if (item.type == ContextMenuButtonType.paste)
          ContextMenuButtonItem(
            type: item.type,
            label: item.label,
            onPressed: () {
              state.hideToolbar();
              unawaited(paste());
            },
          )
        else
          item,
    ],
  );

  void cancel() {
    if (!current) return;
    generation++;
    inspector.cancel();
    setState(() {
      inspecting = false;
      loading = false;
      pasting = false;
      clearConstantReview(clearValues: true);
    });
    widget.onCancel();
  }

  @override
  void dispose() {
    generation++;
    report = null;
    inspection = null;
    clearConstantReview(clearValues: true);
    buffer = GrafanaInputBuffer.empty;
    inspector.dispose();
    input.removeListener(changed);
    input.clear();
    input.dispose();
    super.dispose();
  }

  Future<void> load() async {
    if (!current || loading) return;
    final token = ++generation;
    setState(() {
      loading = true;
      report = null;
      inspection = null;
      panelPath = null;
      sourceId = null;
      clearConstantReview(clearValues: true);
      inspector.cancel();
      inspecting = false;
      problem = null;
      inputFailure = null;
    });
    try {
      final text = await widget.loadJson();
      if (!current || token != generation || text == null) return;
      acceptInput(text);
      if (mounted) setState(() => loading = false);
    } on GrafanaInputException catch (e) {
      if (current && token == generation) setState(() => inputFailure = e.reason);
    } catch (_) {
      if (current && token == generation) setState(() => inputFailure = GrafanaInputFailure.readFailed);
    } finally {
      if (mounted && (token == generation || !loading)) setState(() => loading = false);
    }
  }

  Future<void> inspect() async {
    if (!current || loading || inspecting || pasting || buffer.original.isEmpty) return;
    final token = ++generation;
    setState(() {
      inspecting = true;
      report = null;
      inspection = null;
      panelPath = null;
      sourceId = null;
      clearConstantReview(clearValues: true);
      problem = null;
      inputFailure = null;
    });
    try {
      final value = await inspector.inspect(buffer.original);
      if (!current || token != generation) return;
      setState(() {
        report = value.report;
        inspection = value;
      });
    } on GrafanaInspectionCancelled {
      // Explicit edit/cancel/scope change already owns the resulting UI state.
    } on GrafanaImportException catch (error) {
      if (current && token == generation) setState(() => problem = error.problem);
    } catch (_) {
      if (current && token == generation) setState(() => inputFailure = GrafanaInputFailure.readFailed);
    } finally {
      if (mounted && token == generation) setState(() => inspecting = false);
    }
  }

  void continueDraft() {
    final value = report, target = source, path = panelPath;
    if (!current ||
        loading ||
        inspecting ||
        pasting ||
        reviewingConstants ||
        value == null ||
        target == null ||
        path == null) {
      return;
    }
    final reviewed = reviewedConstants;
    final prefill = reviewed == null
        ? GrafanaPanelPrefill.choose(value, path, target)
        : GrafanaPanelPrefill.fromConstants(reviewed, target);
    if (prefill != null) widget.onContinue(prefill);
  }

  Widget inputSection(BuildContext context, String? error, {required bool compact}) {
    final l = context.l10n;
    final size = compact ? GlassControlSize.sm : GlassControlSize.md;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(compact ? l.grafanaImportCompactHint : l.grafanaImportIntro),
        const SizedBox(height: 6),
        if (buffer.previewOnly)
          InfoBanner(
            key: const ValueKey('grafana-import-preview-notice'),
            message: l.grafanaImportPreview,
            tone: BannerTone.info,
          ),
        Actions(
          actions: {
            PasteTextIntent: CallbackAction<PasteTextIntent>(
              onInvoke: (_) {
                unawaited(paste());
                return null;
              },
            ),
          },
          child: TextField(
            key: const ValueKey('grafana-import-json'),
            controller: input,
            enabled: !loading,
            readOnly: buffer.previewOnly,
            minLines: compact ? 1 : 3,
            maxLines: compact ? 2 : 6,
            autocorrect: false,
            enableSuggestions: false,
            smartDashesType: SmartDashesType.disabled,
            smartQuotesType: SmartQuotesType.disabled,
            inputFormatters: [projectionFormatter],
            contextMenuBuilder: toolbar,
            decoration: InputDecoration(labelText: l.grafanaImportJson, isDense: compact),
          ),
        ),
        Text(
          l.grafanaImportLength(buffer.original.length),
          key: const ValueKey('grafana-import-input-length'),
          style: compact ? Theme.of(context).textTheme.bodySmall : null,
        ),
        Wrap(
          spacing: 6,
          runSpacing: 4,
          children: [
            GlassButton(
              key: const ValueKey('grafana-import-paste'),
              label: l.grafanaImportPaste,
              size: size,
              onPressed: loading || pasting ? null : () => paste(replace: true),
            ),
            GlassButton(
              key: const ValueKey('grafana-import-clear'),
              label: l.grafanaImportClear,
              size: size,
              onPressed: loading ? null : () => acceptInput(''),
            ),
            GlassButton(
              key: const ValueKey('grafana-import-file'),
              label: l.grafanaImportFile,
              size: size,
              onPressed: loading ? null : load,
            ),
            GlassButton(
              key: const ValueKey('grafana-import-inspect'),
              label: l.grafanaImportInspect,
              size: size,
              onPressed: loading || pasting || inspecting || buffer.original.isEmpty ? null : inspect,
            ),
          ],
        ),
        if (loading || pasting || inspecting || reviewingConstants) const LinearProgressIndicator(),
        if (inspecting || reviewingConstants)
          Text(l.grafanaImportInspecting, key: const ValueKey('grafana-import-progress')),
        if (error != null) ...[
          const SizedBox(height: 6),
          InfoBanner(key: const ValueKey('grafana-import-error'), message: error, tone: BannerTone.danger),
        ],
      ],
    );
  }

  Widget reviewHeader(BuildContext context, GrafanaImportReport value) {
    final l = context.l10n;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(value.title, maxLines: 1, overflow: TextOverflow.ellipsis, style: Theme.of(context).textTheme.titleMedium),
        if (!value.problems.contains(GrafanaImportProblem.unsupportedSchema))
          Text(l.grafanaImportCounts(value.supported.length, value.unsupported.length))
        else
          Text(l.grafanaImportNotInspected, key: const ValueKey('grafana-import-not-inspected')),
        ExpansionTile(
          key: ValueKey((value, 'grafana-import-notes')),
          dense: true,
          tilePadding: EdgeInsets.zero,
          childrenPadding: const EdgeInsets.only(bottom: 6),
          title: Text(
            l.grafanaImportReviewDetails(value.problems.length + (value.variables.isNotEmpty ? 1 : 0)),
            key: const ValueKey('grafana-import-notes-toggle'),
          ),
          children: [
            Align(alignment: AlignmentDirectional.centerStart, child: Text(l.grafanaImportIntro)),
            Align(alignment: AlignmentDirectional.centerStart, child: Text(l.grafanaImportSourceHint)),
            if (value.problems.contains(GrafanaImportProblem.unsupportedSchema) && inspection != null)
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: Text(
                  l.grafanaImportEnvelope(
                    inspection!.apiVersion ?? l.grafanaImportNotDeclared,
                    inspection!.kind ?? l.grafanaImportNotDeclared,
                    inspection!.schemaVersion ?? l.grafanaImportNotDeclared,
                  ),
                ),
              ),
            for (final issue in value.problems)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 2),
                child: InfoBanner(message: grafanaImportProblemLabel(l, issue), tone: BannerTone.warning),
              ),
            if (value.variables.isNotEmpty)
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: Text(l.grafanaImportVariableNames(value.variables.take(16).join(', '))),
              ),
            if (value.variables.length > 16)
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: Text(l.grafanaImportMoreVariables(value.variables.length - 16)),
              ),
          ],
        ),
      ],
    );
  }

  Widget panelRows(BuildContext context, GrafanaImportReport value) {
    final l = context.l10n;
    return ListView.builder(
      key: const ValueKey('grafana-import-panels'),
      primary: false,
      itemCount: value.panels.length,
      itemBuilder: (context, index) {
        final row = value.panels[index];
        return Padding(
          padding: const EdgeInsets.symmetric(vertical: 2),
          child: ContentSurface(
            padding: const EdgeInsets.symmetric(horizontal: 4),
            child: Tooltip(
              message: l.grafanaImportPanelDetails,
              child: ExpansionTile(
                key: ValueKey((value, row.path)),
                dense: true,
                tilePadding: const EdgeInsets.symmetric(horizontal: 8),
                childrenPadding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
                title: Row(
                  key: ValueKey('grafana-import-row-${row.path}'),
                  children: [
                    Expanded(
                      flex: 3,
                      child: Text(
                        row.title.isEmpty ? l.grafanaImportUntitled : row.title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                    const SizedBox(width: 6),
                    Flexible(
                      flex: 2,
                      child: row.supported || canReviewConstants(row)
                          ? GlassButton(
                              key: ValueKey('grafana-import-choose-${row.path}'),
                              label: panelPath == row.path ? l.grafanaImportSelected : l.grafanaImportChoose,
                              tooltip: panelPath == row.path ? l.grafanaImportSelected : l.grafanaImportChoose,
                              size: GlassControlSize.sm,
                              onPressed: () => selectPanel(row.path, value),
                            )
                          : Text(
                              l.grafanaImportUnsupportedBadge,
                              style: Theme.of(context).textTheme.bodySmall,
                              maxLines: 2,
                              overflow: TextOverflow.ellipsis,
                            ),
                    ),
                  ],
                ),
                children: [
                  Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: SelectableText(row.title.isEmpty ? l.grafanaImportUntitled : row.title),
                  ),
                  Align(alignment: AlignmentDirectional.centerStart, child: Text(row.path)),
                  for (final issue in row.problems)
                    Align(
                      alignment: AlignmentDirectional.centerStart,
                      child: Text(grafanaImportProblemLabel(l, issue)),
                    ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }

  Widget constantsSection(BuildContext context, GrafanaImportReport value) {
    final l = context.l10n, selectedPath = panelPath;
    final reviewed = reviewedConstants;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        MonitoringGrafanaConstants(
          key: ValueKey((value, panelPath)),
          names: inspection!.constantNames,
          onChanged: (values) {
            if (current && identical(report, value) && panelPath == selectedPath) {
              editConstantValues(values);
            }
          },
        ),
        const SizedBox(height: 6),
        Align(
          alignment: AlignmentDirectional.centerStart,
          child: GlassButton(
            key: const ValueKey('grafana-constants-review'),
            label: l.grafanaConstantsReview,
            size: GlassControlSize.sm,
            onPressed: source == null || constantValues.isEmpty || reviewingConstants ? null : reviewConstants,
          ),
        ),
        if (constantProblem != null)
          InfoBanner(
            key: const ValueKey('grafana-constants-error'),
            message: switch (constantProblem!) {
              GrafanaConstantProblem.missingValue => l.grafanaConstantMissing,
              GrafanaConstantProblem.invalidBindings => l.grafanaConstantInvalidValues,
              GrafanaConstantProblem.budget => l.grafanaConstantBudget,
              _ => l.grafanaConstantUnsupported,
            },
            tone: BannerTone.danger,
          ),
        if (reviewed != null) ...[
          ContentSurface(
            padding: const EdgeInsets.all(8),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 120),
              child: SingleChildScrollView(
                child: SelectableText(reviewed.draft.expression, key: const ValueKey('grafana-constants-query')),
              ),
            ),
          ),
          Text(l.grafanaConstantsReviewed(reviewed.names.length), key: const ValueKey('grafana-constants-reviewed')),
          Text(l.grafanaConstantModeHint),
        ],
      ],
    );
  }

  Widget sourceSection(BuildContext context, GrafanaImportReport value, GrafanaPanelDraft? selected) {
    final l = context.l10n;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        DropdownButtonFormField<String>(
          key: ObjectKey(value),
          initialValue: sourceId,
          isExpanded: true,
          itemHeight: null,
          menuMaxHeight: 280,
          decoration: InputDecoration(labelText: l.grafanaImportSource, isDense: true),
          items: [
            for (final source in widget.sources)
              if (!source.readOnly && source.revision > 0)
                DropdownMenuItem(
                  value: source.id,
                  child: Text('${source.name} · ${l.monitoringBindingRevision}: ${source.revision}'),
                ),
          ],
          onChanged: (id) {
            if (current && identical(report, value)) selectSource(id);
          },
        ),
        if (selected != null)
          ExpansionTile(
            key: ValueKey((value, panelPath, 'grafana-import-selection')),
            dense: true,
            tilePadding: EdgeInsets.zero,
            title: Text(l.grafanaImportSelectionDetails, key: const ValueKey('grafana-import-selection-details')),
            children: [
              if (selected.datasourceHint != null)
                Align(
                  alignment: AlignmentDirectional.centerStart,
                  child: Text(l.grafanaImportDatasourceHint(selected.datasourceHint!)),
                ),
              Align(
                alignment: AlignmentDirectional.centerStart,
                child: Text(
                  l.grafanaImportModeHint(selected.instant ? l.monitoringQueryCurrent : l.monitoringQueryRange),
                ),
              ),
              if (selected.unitHint != null)
                Align(
                  alignment: AlignmentDirectional.centerStart,
                  child: Text(l.grafanaImportUnitHint(selected.unitHint!)),
                ),
              ContentSurface(
                padding: const EdgeInsets.all(8),
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 120),
                  child: SingleChildScrollView(child: SelectableText(selected.expression)),
                ),
              ),
            ],
          ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n, value = report;
    final selected = reviewedConstants?.draft ?? panel?.draft;
    final error = problem != null
        ? grafanaImportProblemLabel(l, problem!)
        : switch (inputFailure) {
            GrafanaInputFailure.tooLarge => l.grafanaImportTooLarge,
            GrafanaInputFailure.invalidUtf8 => l.grafanaImportInvalidUtf8,
            GrafanaInputFailure.readFailed => l.grafanaImportReadFailed,
            null => null,
          };
    return GlassDialog(
      title: l.grafanaImportTitle,
      icon: Icons.file_open_outlined,
      width: 880,
      content: LayoutBuilder(
        builder: (context, constraints) {
          if (value == null) return SingleChildScrollView(child: inputSection(context, error, compact: false));
          final height = constraints.hasBoundedHeight ? constraints.maxHeight : MediaQuery.sizeOf(context).height * .65;
          final gap = height < 80 ? 0.0 : 8.0;
          return SizedBox(
            height: height,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                ConstrainedBox(
                  constraints: BoxConstraints(maxHeight: (height * .32).clamp(0.0, 232.0).toDouble()),
                  child: SingleChildScrollView(
                    key: const ValueKey('grafana-import-review-header'),
                    primary: false,
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [reviewHeader(context, value), inputSection(context, error, compact: true)],
                    ),
                  ),
                ),
                SizedBox(height: gap),
                Expanded(child: panelRows(context, value)),
                if (panel != null && canReviewConstants(panel!)) ...[
                  SizedBox(height: gap),
                  Expanded(
                    flex: 2,
                    child: SingleChildScrollView(
                      key: const ValueKey('grafana-import-constant-section'),
                      primary: false,
                      child: constantsSection(context, value),
                    ),
                  ),
                ],
                SizedBox(height: gap),
                ConstrainedBox(
                  constraints: BoxConstraints(maxHeight: (height * .22).clamp(0.0, 120.0).toDouble()),
                  child: SingleChildScrollView(
                    key: const ValueKey('grafana-import-source-section'),
                    primary: false,
                    child: sourceSection(context, value, selected),
                  ),
                ),
              ],
            ),
          );
        },
      ),
      primaryAction: GlassButton(
        key: const ValueKey('grafana-import-continue'),
        label: l.grafanaImportContinue,
        onPressed:
            !loading && !inspecting && !pasting && !reviewingConstants && selected != null && source != null && current
            ? continueDraft
            : null,
      ),
      secondaryActions: [
        GlassButton(key: const ValueKey('grafana-import-cancel'), label: l.commonCancel, onPressed: cancel),
      ],
    );
  }
}
