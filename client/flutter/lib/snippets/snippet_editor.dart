import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/error_messages.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/l10n/labels.dart';
import 'package:consolecrypt/core/models/models.dart';
import 'package:consolecrypt/core/providers.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/common.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/core/widgets/risk_badge.dart';
import 'package:consolecrypt/core/widgets/security_widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Create/edit a snippet; [draft] pre-fills from an AI suggestion
/// ("Save as snippet").
Future<Snippet?> showSnippetEditor(
  BuildContext context, {
  Snippet? snippet,
  SnippetDraft? draft,
  String? packageName,
  String? initialTemplate,
}) => showAppDialog<Snippet>(
  context,
  builder: (_) =>
      SnippetEditorDialog(snippet: snippet, draft: draft, packageName: packageName, initialTemplate: initialTemplate),
);

class SnippetEditorDialog extends ConsumerStatefulWidget {
  const SnippetEditorDialog({super.key, this.snippet, this.draft, this.packageName, this.initialTemplate});

  final Snippet? snippet;
  final SnippetDraft? draft;
  final String? packageName;
  final String? initialTemplate;

  @override
  ConsumerState<SnippetEditorDialog> createState() => _SnippetEditorDialogState();
}

class _SnippetEditorDialogState extends ConsumerState<SnippetEditorDialog> {
  late final TextEditingController _name;
  late final TextEditingController _description;
  late final TextEditingController _template;
  late final TextEditingController _shell;
  late final TextEditingController _package;
  late SnippetType _type;
  late RiskLevel _risk;
  late List<String> _tags;
  late SnippetSource _source;
  final Map<String, (TextEditingController, TextEditingController)> _vars = {};
  RiskAssessment? _assessment;
  Timer? _debounce;
  AppException? _error;
  bool _saving = false;
  ProfileId? _profileId;

  @override
  void initState() {
    super.initState();
    _profileId = ref.read(activeProfileProvider)?.id;
    final s = widget.snippet;
    final d = widget.draft;
    _name = TextEditingController(text: s?.name ?? d?.name ?? '');
    _description = TextEditingController(text: s?.description ?? d?.description ?? '');
    _template = TextEditingController(text: s?.template ?? d?.template ?? widget.initialTemplate ?? '');
    _shell = TextEditingController(text: s?.shell ?? '');
    _package = TextEditingController(text: s?.packageName ?? widget.packageName ?? '');
    _type = s?.snippetType ?? d?.snippetType ?? SnippetType.bash;
    _risk = s?.riskLevel ?? d?.suggestedRisk ?? RiskLevel.unknown;
    _tags = [...?s?.tags ?? d?.tags];
    _source =
        s?.source ??
        (d != null
            ? SnippetSource.ai
            : widget.initialTemplate != null
            ? SnippetSource.history
            : SnippetSource.user);
    for (final v in s?.variables ?? d?.variables ?? const <SnippetVariable>[]) {
      _vars[v.name] = (TextEditingController(text: v.defaultValue ?? ''), TextEditingController(text: v.description));
    }
    _template.addListener(_onTemplateChanged);
    _onTemplateChanged();
  }

  @override
  void dispose() {
    _debounce?.cancel();
    for (final c in [_name, _description, _template, _shell, _package]) {
      c.dispose();
    }
    for (final (a, b) in _vars.values) {
      a.dispose();
      b.dispose();
    }
    super.dispose();
  }

  void _onTemplateChanged() {
    for (final name in templateVariables(_template.text)) {
      _vars.putIfAbsent(name, () => (TextEditingController(), TextEditingController()));
    }
    if (mounted) setState(() {});
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 300), () async {
      final text = _template.text;
      if (text.trim().isEmpty) return;
      final a = await ref.read(snippetServiceProvider).assessRisk(text, declared: _risk, source: _source);
      if (mounted) setState(() => _assessment = a);
    });
  }

  Future<void> _save() async {
    if (_profileId != ref.read(activeProfileProvider)?.id || ref.read(vaultStatusProvider).value?.isUnlocked != true) {
      closeDialog<Snippet>(context);
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    final now = DateTime.now().toUtc();
    final names = templateVariables(_template.text);
    final s = widget.snippet;
    final snippet = Snippet(
      id: s?.id ?? ObjectId.generate(),
      name: _name.text.trim(),
      description: _description.text.trim(),
      packageName: _package.text.trim().isEmpty ? null : _package.text.trim(),
      catalogId: s?.catalogId,
      snippetType: _type,
      shell: _shell.text.trim().isEmpty ? null : _shell.text.trim(),
      template: _template.text,
      variables: [
        for (final n in names)
          SnippetVariable(
            name: n,
            defaultValue: _vars[n]!.$1.text.isEmpty ? null : _vars[n]!.$1.text,
            description: _vars[n]!.$2.text,
          ),
      ],
      tags: _tags,
      riskLevel: _risk,
      source: _source,
      createdBy: s?.createdBy,
      createdAt: s?.createdAt ?? now,
      updatedAt: now,
      lastUsedAt: s?.lastUsedAt,
      usageCount: s?.usageCount ?? 0,
    );
    try {
      final saved = await ref.read(snippetServiceProvider).saveSnippet(snippet);
      if (mounted) closeDialog(context, saved);
    } on AppException catch (e) {
      if (mounted) setState(() => _error = e);
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final names = templateVariables(_template.text);
    final a = _assessment;
    final tokens = GlassTokens.of(context);
    final t = tokens.typography;
    final heading = t.bodyEmph.copyWith(color: tokens.palette.label);
    final mono = t.mono.copyWith(color: tokens.palette.label);
    return GlassDialog(
      key: const ValueKey('snippet-editor'),
      title: widget.snippet == null ? l10n.snippetEditorTitleNew : l10n.snippetEditorTitleEdit,
      width: 700,
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (widget.draft != null)
              Padding(
                padding: const EdgeInsets.only(bottom: GlassSpacing.s12),
                child: InfoBanner(message: l10n.snippetEditorAiDraftBanner),
              ),
            Row(
              children: [
                Expanded(
                  flex: 2,
                  child: TextField(
                    key: const ValueKey('snippet-name'),
                    controller: _name,
                    decoration: InputDecoration(labelText: l10n.commonName),
                  ),
                ),
                const SizedBox(width: GlassSpacing.s12),
                Expanded(
                  child: DropdownButtonFormField<SnippetType>(
                    isExpanded: true,
                    borderRadius: BorderRadius.circular(tokens.radii.menu),
                    initialValue: _type,
                    decoration: InputDecoration(labelText: l10n.snippetsTypeLabel),
                    items: [
                      for (final type in SnippetType.values)
                        DropdownMenuItem(value: type, child: Text(type.localized(l10n))),
                    ],
                    onChanged: (v) => setState(() => _type = v ?? _type),
                  ),
                ),
              ],
            ),
            const SizedBox(height: GlassSpacing.s12),
            TextField(
              controller: _description,
              decoration: InputDecoration(labelText: l10n.commonDescription),
            ),
            const SizedBox(height: GlassSpacing.s12),
            TextField(
              key: const ValueKey('snippet-package-name'),
              controller: _package,
              decoration: InputDecoration(labelText: l10n.snippetPackage, helperText: l10n.snippetPackageHint),
            ),
            if (ref.watch(snippetsProvider).value case final snippets?)
              Wrap(
                spacing: 6,
                children: [
                  for (final name in ({
                    for (final s in snippets)
                      if (s.packageName != null) s.packageName!,
                  }.toList()..sort()))
                    ActionChip(label: Text(name), onPressed: () => _package.text = name),
                ],
              ),
            const SizedBox(height: GlassSpacing.s12),
            TextField(
              key: const ValueKey('snippet-template'),
              controller: _template,
              minLines: 2,
              maxLines: 6,
              style: mono,
              decoration: InputDecoration(
                labelText: l10n.snippetEditorTemplateLabel,
                hintText: 'kubectl logs -n {{namespace}} {{pod}} --tail={{lines}}', // l10n-ignore: command example
              ),
            ),
            const SizedBox(height: GlassSpacing.s12),
            TextField(
              controller: _shell,
              decoration: InputDecoration(
                labelText: l10n.snippetEditorShellLabel,
                hintText: 'bash, pwsh7, psql', // l10n-ignore: technical tokens
              ),
            ),
            if (names.isNotEmpty) ...[
              const SizedBox(height: GlassSpacing.s16),
              Text(l10n.snippetEditorVariables, style: heading),
              const SizedBox(height: GlassSpacing.s8),
              for (final n in names)
                Padding(
                  padding: const EdgeInsets.only(bottom: GlassSpacing.s8),
                  child: Row(
                    children: [
                      SizedBox(width: 120, child: Text('{{$n}}', style: mono.copyWith(fontSize: 12))),
                      Expanded(
                        child: TextField(
                          controller: _vars[n]!.$1,
                          decoration: InputDecoration(labelText: l10n.snippetEditorVariableDefault),
                        ),
                      ),
                      const SizedBox(width: GlassSpacing.s8),
                      Expanded(
                        flex: 2,
                        child: TextField(
                          controller: _vars[n]!.$2,
                          decoration: InputDecoration(labelText: l10n.commonDescription),
                        ),
                      ),
                    ],
                  ),
                ),
            ],
            const SizedBox(height: GlassSpacing.s12),
            Text(l10n.snippetEditorRisk, style: heading),
            const SizedBox(height: GlassSpacing.s6),
            GlassSegmented<RiskLevel>(
              key: const ValueKey('snippet-risk'),
              inChrome: false,
              expand: true,
              segments: [
                for (final r in RiskLevel.values)
                  GlassSegment(value: r, icon: GlassRiskBadge.describe(r).$2, label: r.localized(l10n)),
              ],
              selected: _risk,
              onChanged: (r) {
                setState(() => _risk = r);
                _onTemplateChanged();
              },
            ),
            if (a != null) ...[
              const SizedBox(height: GlassSpacing.s8),
              Row(
                children: [
                  Text(l10n.snippetEditorEffective),
                  const SizedBox(width: GlassSpacing.s6),
                  RiskBadge(risk: a.effective),
                  const SizedBox(width: GlassSpacing.s8),
                  Expanded(
                    child: Text(
                      l10n.snippetEditorLocalRules(a.reasons.isEmpty ? a.local.localized(l10n) : a.reasons.join('; ')),
                      style: t.callout.copyWith(color: tokens.secondaryLabel),
                    ),
                  ),
                ],
              ),
            ],
            const SizedBox(height: GlassSpacing.s12),
            TagEditor(tags: _tags, onChanged: (tags) => setState(() => _tags = tags)),
            if (_error != null) ...[
              const SizedBox(height: GlassSpacing.s12),
              GateErrorText(text: errorMessage(l10n, _error!)),
            ],
          ],
        ),
      ),
      secondaryActions: [GlassButton(label: l10n.commonCancel, onPressed: () => closeDialog<Snippet>(context))],
      primaryAction: GlassButton.prominent(
        key: const ValueKey('save-snippet'),
        busy: _saving,
        onPressed: _saving ? null : _save,
        label: l10n.commonSave,
      ),
    );
  }
}
