import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/security/secret_text.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_service.dart';
import 'package:consolecrypt/kubernetes/kube_manifest_snippets.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_schema_fields.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_editor.dart';
import 'package:consolecrypt/kubernetes/kube_yaml_syntax.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeCreateDialog extends ConsumerStatefulWidget {
  const KubeCreateDialog({
    required this.session,
    required this.contextName,
    required this.kind,
    required this.service,
    this.namespace,
    this.initialYaml,
    this.onOutput,
    this.initialName,
    super.key,
  });
  final KubeSession session;
  final String contextName;
  final KubeResourceKind kind;
  final KubeManifestService service;
  final String? namespace;
  final String? initialYaml, initialName;
  final void Function(String title, String text)? onOutput;
  @override
  ConsumerState<KubeCreateDialog> createState() => _KubeCreateDialogState();
}

class _KubeCreateDialogState extends ConsumerState<KubeCreateDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  final _name = TextEditingController(), _namespace = TextEditingController(), _yaml = KubeYamlController();
  String? _validated, _preview, _schema;
  bool _busy = false, _failed = false;
  int _generation = 0;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  bool get _canWrite => _current && (ref.read(kubeWriteAccessProvider(widget.session)).value?.canWrite ?? false);
  bool get _valid =>
      !_yaml.text.contains('[redacted]') &&
      _name.text.isNotEmpty &&
      _name.text.length <= 253 &&
      RegExp(r'^[a-z0-9]([-a-z0-9.]*[a-z0-9])?$').hasMatch(_name.text) &&
      (!widget.kind.namespaced ||
          (_namespace.text.isNotEmpty &&
              _namespace.text.length <= 63 &&
              RegExp(r'^[a-z0-9]([-a-z0-9]*[a-z0-9])?$').hasMatch(_namespace.text)));
  KubeResourceRef get _reference =>
      KubeResourceRef(api: widget.kind, name: _name.text, namespace: widget.kind.namespaced ? _namespace.text : null);
  @override
  void initState() {
    super.initState();
    _namespace.text = widget.namespace ?? 'default';
    _name.text = widget.initialName ?? '';
    _yaml.text = widget.initialYaml ?? '';
    unawaited(_loadSchema());
  }

  @override
  void dispose() {
    _generation++;
    _validated = null;
    _preview = null;
    for (final c in [_name, _namespace, _yaml]) {
      c.clear();
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _loadSchema() async {
    try {
      final schema = await widget.service.resourceSchema(widget.session.id, widget.kind);
      if (_current) setState(() => _schema = const JsonEncoder.withIndent('  ').convert(schema));
    } catch (_) {}
  }

  void _changed() {
    _generation++;
    setState(() {
      _validated = null;
      _preview = null;
      _failed = false;
    });
  }

  void _template() {
    if (!_valid || _busy) return;
    _yaml.text = const JsonEncoder.withIndent('  ').convert({
      'apiVersion': widget.kind.apiVersion,
      'kind': widget.kind.kind,
      'metadata': {'name': _name.text, if (widget.kind.namespaced) 'namespace': _namespace.text},
    });
    _changed();
  }

  Future<void> _snippet() async {
    if (!_current || _busy) return;
    final manifest = await showAppDialog<String>(context, secure: true, builder: (_) => const KubeManifestSnippets());
    if (_current && manifest != null) {
      _yaml.text = manifest;
      _changed();
    }
  }

  Future<void> _validate() async {
    if (!_canWrite || !_valid || _busy || _yaml.text.trim().isEmpty) return;
    final generation = ++_generation, source = _yaml.text;
    final secret = SecretText(source);
    setState(() {
      _busy = true;
      _failed = false;
      _validated = null;
      _preview = null;
    });
    try {
      final result = await widget.service.createResource(widget.session.id, _reference, secret, dryRun: true);
      if (_current && generation == _generation) {
        setState(() {
          _validated = source;
          _preview = result.yaml;
        });
        if (mounted) widget.onOutput?.call(context.l10n.kubeDryRun, result.yaml);
      }
    } catch (_) {
      if (_current && generation == _generation) setState(() => _failed = true);
    } finally {
      secret.wipe();
      if (_current) setState(() => _busy = false);
    }
  }

  Future<void> _create() async {
    final source = _validated;
    if (!_canWrite || _busy || source == null || source != _yaml.text) return;
    final reference = _reference;
    final approved = await showKubePassphraseConfirmation(
      context,
      subject: reference.name,
      title: context.l10n.kubeCreateResource,
      message: context.l10n.kubeMutationConfirmHelp,
      confirmLabel: context.l10n.kubeCreateResource,
    );
    if (!_canWrite || !approved || source != _validated || source != _yaml.text) return;
    final secret = SecretText(source);
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      final result = await widget.service.createResource(widget.session.id, reference, secret, dryRun: false);
      if (mounted && _current) widget.onOutput?.call(context.l10n.kubeCreateResource, result.yaml);
      if (mounted && _current) closeDialog(context, true);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      secret.wipe();
      if (_current) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      _generation++;
      _yaml.clear();
      _validated = null;
      _preview = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    ref.watch(kubeWriteAccessProvider(widget.session));
    final l = context.l10n;
    return GlassDialog(
      title: '${l.kubeCreateResource} · ${widget.kind.kind}',
      width: 1000,
      content: SizedBox(
        height: 580,
        child: !_current
            ? const SizedBox.shrink()
            : ListView(
                children: [
                  KubeWriteBanner(session: widget.session, contextName: widget.contextName),
                  TextField(
                    key: const ValueKey('kube-create-name'),
                    controller: _name,
                    enabled: !_busy,
                    decoration: InputDecoration(labelText: l.kubeName),
                    onChanged: (_) => _changed(),
                  ),
                  if (widget.kind.namespaced)
                    TextField(
                      key: const ValueKey('kube-create-namespace'),
                      controller: _namespace,
                      enabled: !_busy,
                      decoration: InputDecoration(labelText: l.kubeNamespace),
                      onChanged: (_) => _changed(),
                    ),
                  Align(
                    alignment: Alignment.centerLeft,
                    child: GlassButton(
                      key: const ValueKey('kube-create-template'),
                      label: l.kubeManifestTemplate,
                      onPressed: _valid && !_busy ? _template : null,
                    ),
                  ),
                  Align(
                    alignment: Alignment.centerLeft,
                    child: GlassButton(
                      key: const ValueKey('kube-create-snippet'),
                      label: l.kubeManifestSnippets,
                      icon: Icons.code,
                      onPressed: _busy ? null : _snippet,
                    ),
                  ),
                  TextField(
                    key: const ValueKey('kube-create-yaml'),
                    controller: _yaml,
                    enabled: !_busy,
                    minLines: 6,
                    maxLines: 12,
                    maxLength: 2 * 1024 * 1024,
                    autocorrect: false,
                    enableSuggestions: false,
                    enableIMEPersonalizedLearning: false,
                    style: GlassTokens.of(context).typography.mono,
                    decoration: InputDecoration(labelText: l.kubeYaml, counterText: ''),
                    onChanged: (_) => _changed(),
                  ),
                  if (_schema != null)
                    Align(
                      alignment: Alignment.centerLeft,
                      child: GlassButton(
                        key: const ValueKey('kube-create-schema'),
                        label: l.kubeSchemaFields,
                        icon: Icons.help_outline,
                        onPressed: !_canWrite || _busy
                            ? null
                            : () => showKubeSchemaFields(
                                context,
                                session: widget.session,
                                kind: widget.kind,
                                service: widget.service,
                                editor: _yaml,
                                onChanged: _changed,
                              ),
                      ),
                    ),
                  if (_schema != null)
                    ExpansionTile(
                      title: Text(l.kubeSchema),
                      children: [SizedBox(height: 200, child: SingleChildScrollView(child: SelectableText(_schema!)))],
                    ),
                  if (_busy) const LinearProgressIndicator(),
                  if (_yaml.text.contains('[redacted]')) Text(l.kubeMaskedEditHelp),
                  if (_failed) Text(l.kubeApplyFailed),
                  if (_preview != null)
                    SizedBox(
                      height: 200,
                      child: KubeYamlDiff(before: '', after: _preview!),
                    ),
                ],
              ),
      ),
      primaryAction: GlassButton.prominent(
        key: const ValueKey('kube-create-apply'),
        label: l.kubeCreateResource,
        onPressed: _canWrite && !_busy && _validated != null ? _create : null,
      ),
      secondaryActions: [
        GlassButton(
          key: const ValueKey('kube-create-validate'),
          label: l.kubeDryRun,
          onPressed: _canWrite && !_busy && _valid && _yaml.text.trim().isNotEmpty ? _validate : null,
        ),
        GlassButton(label: l.commonCancel, onPressed: _busy ? null : () => closeDialog(context, false)),
      ],
    );
  }
}
