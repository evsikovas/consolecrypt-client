import 'dart:async';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/services/errors.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_rollout_panel.dart';
import 'package:consolecrypt/kubernetes/kube_schema_fields.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_syntax.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeYamlEditor extends ConsumerStatefulWidget {
  const KubeYamlEditor({
    required this.session,
    required this.contextName,
    required this.resource,
    required this.original,
    required this.service,
    this.initialYaml,
    this.onOutput,
    super.key,
  });
  final KubeSession session;
  final String contextName;
  final KubeResourceRef resource;
  final KubeResourceDetail original;
  final KubeMutationService service;
  final String? initialYaml;
  final void Function(String title, String text)? onOutput;
  @override
  ConsumerState<KubeYamlEditor> createState() => _KubeYamlEditorState();
}

class _KubeYamlEditorState extends ConsumerState<KubeYamlEditor> with SingleTickerProviderStateMixin {
  late final TextEditingController _yaml;
  late final TabController _tabs;
  late final KubeScope _scope;
  KubeResourceDetail? _preview;
  KubeJson? _fieldDiff;
  String? _validatedSource;
  bool _busy = false, _failed = false, _force = false;
  String? _failure;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  bool get _masked => widget.original.yaml.contains('[redacted]');
  bool get _canWrite => !_masked && (ref.read(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false);

  @override
  void initState() {
    super.initState();
    _scope = ref.read(kubeScopeProvider);
    _yaml = KubeYamlController(text: widget.initialYaml ?? widget.original.yaml);
    _tabs = TabController(length: 2, vsync: this);
  }

  @override
  void dispose() {
    _validatedSource = null;
    _preview = null;
    _fieldDiff = null;
    _yaml.clear();
    _yaml.dispose();
    _tabs.dispose();
    super.dispose();
  }

  void _invalidatePreview() {
    setState(() {
      _validatedSource = null;
      _preview = null;
      _fieldDiff = null;
    });
  }

  Future<void> _validate() async {
    if (!_current || _busy || !_canWrite || widget.resource.api.isSecret || _yaml.text.trim().isEmpty) return;
    final source = _yaml.text;
    final manifest = SecretText(source);
    setState(() {
      _busy = true;
      _failed = false;
      _validatedSource = null;
      _preview = null;
      _fieldDiff = null;
    });
    try {
      final preview = await widget.service.applyYaml(
        widget.session.id,
        widget.resource,
        manifest,
        dryRun: true,
        force: _force,
      );
      if (!_current || _yaml.text != source) return;
      KubeJson? fieldDiff;
      final manifestService = widget.service;
      if (manifestService is KubeManifestService) {
        final reviewed = SecretText(preview.yaml);
        try {
          fieldDiff = await (manifestService as KubeManifestService).compareManifest(
            widget.session.id,
            widget.resource,
            reviewed,
          );
        } finally {
          reviewed.wipe();
        }
      }
      if (!_current || _yaml.text != source) return;
      setState(() {
        _fieldDiff = fieldDiff;
        _validatedSource = source;
        _preview = preview;
      });
      if (mounted) widget.onOutput?.call(context.l10n.kubeDryRun, preview.yaml);
      _tabs.animateTo(1);
    } catch (error) {
      if (_current) {
        setState(() {
          _failed = true;
          _failure = _rule(error);
        });
      }
    } finally {
      manifest.wipe();
      if (_current) setState(() => _busy = false);
    }
  }

  static String? _rule(Object error) => error is AppException ? error.args['rule'] : null;

  Future<void> _apply() async {
    // Apply always goes through a server-side dry run first; an unchecked
    // edit is validated here and the review tab is shown before confirming.
    if (_validatedSource != _yaml.text || _preview == null) {
      await _validate();
      if (!mounted || !_current || _validatedSource != _yaml.text || _preview == null) return;
    }
    final source = _validatedSource;
    if (!_current || _busy || !_canWrite || source == null || source != _yaml.text || _preview == null) return;
    final approved = await showTypedKubeConfirmation(
      context,
      expected: widget.resource.name,
      title: context.l10n.kubeApplyYaml,
      message: context.l10n.kubeApplyYamlHelp,
      confirmLabel: context.l10n.kubeApplyYaml,
    );
    if (!_current || !approved || !_canWrite || source != _yaml.text || source != _validatedSource) return;
    final manifest = SecretText(source);
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final result = await widget.service.applyYaml(
        widget.session.id,
        widget.resource,
        manifest,
        dryRun: false,
        force: _force,
      );
      if (mounted && _current) widget.onOutput?.call(context.l10n.kubeApplyYaml, result.yaml);
      if (mounted && _current) closeDialog(context, true);
    } catch (error) {
      if (_current) {
        setState(() {
          _failed = true;
          _failure = _rule(error);
          _preview = null;
          _fieldDiff = null;
          _validatedSource = null;
        });
      }
    } finally {
      manifest.wipe();
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(next, _scope)) return;
      _yaml.clear();
      _validatedSource = null;
      _preview = null;
      _fieldDiff = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    final l = context.l10n;
    final canWrite = !_masked && (ref.watch(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false);
    final preview = _preview;
    return PopScope(
      canPop: !_busy,
      child: GlassDialog(
        key: const ValueKey('kube-yaml-editor'),
        width: 1080,
        title: '${l.kubeEditYaml} · ${widget.resource.name}',
        content: SizedBox(
          height: 620,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              KubeWriteBanner(session: widget.session, contextName: widget.contextName),
              if (_masked) Text(l.kubeMaskedEditHelp),
              if (_busy) const LinearProgressIndicator(),
              if (_failed)
                Text(switch (_failure) {
                  'conflict' => l.kubeApplyConflict,
                  'forbidden' || 'unauthorized' => l.kubeApplyForbidden,
                  'invalid_request' => l.kubeApplyInvalid,
                  _ => l.kubeApplyFailed,
                }, style: TextStyle(color: Theme.of(context).colorScheme.error)),
              TabBar(
                controller: _tabs,
                tabs: [
                  Tab(text: l.kubeObjectYaml),
                  Tab(key: const ValueKey('kube-yaml-review-tab'), text: l.kubeReviewChanges),
                ],
              ),
              Expanded(
                child: TabBarView(
                  controller: _tabs,
                  children: [
                    Column(
                      children: [
                        if (widget.service is KubeManifestService)
                          Align(
                            alignment: Alignment.centerLeft,
                            child: GlassButton(
                              key: const ValueKey('kube-yaml-schema'),
                              label: l.kubeSchemaFields,
                              icon: Icons.help_outline,
                              onPressed: _busy || !canWrite
                                  ? null
                                  : () => showKubeSchemaFields(
                                      context,
                                      session: widget.session,
                                      kind: widget.resource.api,
                                      service: widget.service as KubeManifestService,
                                      editor: _yaml,
                                      onChanged: _invalidatePreview,
                                    ),
                            ),
                          ),
                        Expanded(
                          child: TextField(
                            key: const ValueKey('kube-yaml-input'),
                            controller: _yaml,
                            readOnly: _busy || !canWrite,
                            maxLines: null,
                            expands: true,
                            maxLength: 2 * 1024 * 1024,
                            textAlignVertical: TextAlignVertical.top,
                            style: GlassTokens.of(context).typography.mono,
                            autocorrect: false,
                            enableSuggestions: false,
                            enableIMEPersonalizedLearning: false,
                            decoration: InputDecoration(
                              border: InputBorder.none,
                              counterText: '',
                              hintText: l.kubeObjectYaml,
                            ),
                            onChanged: (_) => _invalidatePreview(),
                          ),
                        ),
                        CheckboxListTile(
                          key: const ValueKey('kube-yaml-force'),
                          contentPadding: EdgeInsets.zero,
                          value: _force,
                          title: Text(l.kubeForceApply),
                          subtitle: Text(l.kubeForceApplyHelp),
                          onChanged: _busy || !canWrite
                              ? null
                              : (value) {
                                  _force = value ?? false;
                                  _invalidatePreview();
                                },
                        ),
                      ],
                    ),
                    preview == null
                        ? Padding(padding: const EdgeInsets.all(12), child: Text(l.kubeDryRunRequired))
                        : _fieldDiff != null
                        ? KubeManifestDiffView(diff: _fieldDiff!)
                        : KubeYamlDiff(before: widget.original.yaml, after: preview.yaml),
                  ],
                ),
              ),
            ],
          ),
        ),
        primaryAction: GlassButton.destructive(
          key: const ValueKey('kube-yaml-apply'),
          label: l.kubeApplyYaml,
          onPressed: canWrite && !_busy && (preview != null || _yaml.text != widget.original.yaml) ? _apply : null,
        ),
        secondaryActions: [
          GlassButton(
            key: const ValueKey('kube-yaml-validate'),
            label: l.kubeDryRun,
            onPressed: canWrite && !_busy ? _validate : null,
          ),
          GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog(context, false)),
        ],
      ),
    );
  }
}

/// A bounded line comparison avoids a quadratic diff over large manifests.
/// Common leading/trailing lines are collapsed; every changed line is shown.
class KubeYamlDiff extends StatelessWidget {
  const KubeYamlDiff({required this.before, required this.after, this.explanation, super.key});
  final String before, after;
  final String? explanation;
  @override
  Widget build(BuildContext context) {
    final a = before.split('\n'), b = after.split('\n');
    var start = 0, endA = a.length, endB = b.length;
    while (start < endA && start < endB && a[start] == b[start]) {
      start++;
    }
    while (endA > start && endB > start && a[endA - 1] == b[endB - 1]) {
      endA--;
      endB--;
    }
    if (start == endA && start == endB) return Text(context.l10n.kubeNoYamlChanges);
    final removed = a.sublist(start, endA);
    final added = b.sublist(start, endB);
    final style = GlassTokens.of(context).typography.mono;
    return SelectionArea(
      child: ListView.builder(
        key: const ValueKey('kube-yaml-diff'),
        itemCount: 1 + removed.length + added.length,
        itemBuilder: (context, index) {
          if (index == 0) {
            return Padding(
              padding: const EdgeInsets.symmetric(vertical: 8),
              child: Text(explanation ?? context.l10n.kubeDryRunHelp),
            );
          }
          final offset = index - 1;
          final isRemoved = offset < removed.length;
          final line = isRemoved ? removed[offset] : added[offset - removed.length];
          return Text(
            // l10n-ignore: Literal diff marker and user manifest line.
            /* l10n-ignore: Literal diff marker and user manifest line. */ '${isRemoved ? '−' : '+'} $line',
            style: style.copyWith(
              color: isRemoved
                  ? Theme.of(context).colorScheme.error
                  : GlassTokens.of(context).palette.tone(GlassTone.success),
            ),
          );
        },
      ),
    );
  }
}
