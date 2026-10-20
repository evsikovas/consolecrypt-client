import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

/// Controller-specific metadata and links use served discovery descriptors.
/// Secret/configuration references never resolve their values in this card.
class KubeGitOpsDetails extends ConsumerStatefulWidget {
  const KubeGitOpsDetails({
    required this.session,
    required this.resource,
    required this.detail,
    required this.service,
    required this.onOpen,
    required this.onLogs,
    this.onHelm,
    this.onChanged,
    super.key,
  });
  final KubeSession session;
  final KubeResourceRef resource;
  final KubeResourceDetail detail;
  final KubeBrowserService service;
  final ValueChanged<KubeResourceRef> onOpen;
  final void Function(KubeResourceRef controller, String objectFilter) onLogs;
  final void Function(String namespace, String releaseName)? onHelm;
  final VoidCallback? onChanged;
  @override
  ConsumerState<KubeGitOpsDetails> createState() => _KubeGitOpsDetailsState();
}

class _KubeGitOpsDetailsState extends ConsumerState<KubeGitOpsDetails> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  List<KubeResourceKind> _kinds = const [];
  bool _failed = false, _busy = false, _controllerFailed = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  bool get _flux => widget.resource.api.group.endsWith('.toolkit.fluxcd.io');
  KubeJson get _spec => _map(widget.detail.object['spec']);
  KubeJson get _status => _map(widget.detail.object['status']);
  @override
  void initState() {
    super.initState();
    unawaited(_discover());
  }

  Future<void> _discover() async {
    if (!_current) return;
    try {
      final kinds = await widget.service.discover(widget.session.id);
      if (_current) setState(() => _kinds = kinds);
    } catch (_) {
      if (_current) setState(() => _failed = true);
    }
  }

  KubeResourceRef? _reference(String group, String kind, String name, String? namespace, [String? version]) {
    if (name.isEmpty || name.length > 253 || name.contains('/')) return null;
    final candidates = _kinds.where((api) => api.group == group && api.kind == kind && api.canRead).toList()
      ..sort((a, b) {
        if (a.version == b.version) return 0;
        if (a.version == version) return -1;
        if (b.version == version) return 1;
        final aStable = !a.version.contains('alpha') && !a.version.contains('beta');
        final bStable = !b.version.contains('alpha') && !b.version.contains('beta');
        return aStable == bStable ? b.version.compareTo(a.version) : (aStable ? -1 : 1);
      });
    if (candidates.isEmpty) return null;
    final api = candidates.first;
    if (api.namespaced && (namespace == null || namespace.isEmpty)) return null;
    return KubeResourceRef(api: api, name: name, namespace: api.namespaced ? namespace : null);
  }

  Widget _link(String group, String kind, String name, String? namespace, [String? version]) {
    final reference = _reference(group, kind, name, namespace, version);
    final label = '$kind · ${namespace == null || namespace.isEmpty ? '' : '$namespace/'}$name';
    return TextButton.icon(
      key: ValueKey('gitops-link-$group-$kind-$namespace-$name'),
      onPressed: reference == null
          ? null
          : () {
              if (_current) widget.onOpen(reference);
            },
      icon: const Icon(Icons.open_in_new, size: 16),
      label: Text(label, softWrap: true),
    );
  }

  Future<void> _reconcile() async {
    final service = widget.service;
    if (!_current || _busy || service is! KubeInsightsService || service is! KubeMutationService) return;
    final approved = await showTypedKubeConfirmation(
      context,
      expected: widget.resource.name,
      title: context.l10n.kubeGitOpsReconcile,
      message: context.l10n.kubeGitOpsSourceReconcileHelp,
      confirmLabel: context.l10n.kubeGitOpsReconcile,
    );
    if (!_current || !approved) return;
    setState(() {
      _busy = true;
      _failed = false;
    });
    try {
      // A dialog may outlive a production write window. Re-read native access
      // immediately before the action; ProdGuard also validates at dispatch.
      final access = await (service as KubeMutationService).writeAccess(widget.session.id);
      if (!_current || !access.canWrite) return;
      await (service as KubeInsightsService).fluxAction(widget.session.id, widget.resource, KubeFluxAction.reconcile);
      if (_current) widget.onChanged?.call();
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  Future<void> _controllerLogs() async {
    if (!_current || _busy) return;
    final component = switch (widget.resource.api.group) {
      'source.toolkit.fluxcd.io' => 'source-controller',
      'kustomize.toolkit.fluxcd.io' => 'kustomize-controller',
      'helm.toolkit.fluxcd.io' => 'helm-controller',
      'notification.toolkit.fluxcd.io' => 'notification-controller',
      'image.toolkit.fluxcd.io' =>
        widget.resource.api.kind == 'ImageUpdateAutomation'
            ? 'image-automation-controller'
            : 'image-reflector-controller',
      _ => null,
    };
    if (component == null) return;
    final api = _kinds.where((api) => api.group == 'apps' && api.kind == 'Deployment' && api.canList).firstOrNull;
    if (api == null) {
      setState(() => _controllerFailed = true);
      return;
    }
    setState(() {
      _busy = true;
      _controllerFailed = false;
    });
    try {
      final page = await widget.service.listResources(
        widget.session.id,
        api,
        labelSelector: 'app=$component',
        limit: 200,
      );
      if (!_current) return;
      final rows = page.items.where((row) => row.namespace != null).toList();
      // Multiple installations must be chosen explicitly; never send the user
      // to an arbitrary controller in another namespace.
      if (rows.length != 1 || page.continueToken != null) {
        setState(() => _controllerFailed = true);
        return;
      }
      widget.onLogs(
        KubeResourceRef(api: api, namespace: rows.single.namespace, name: rows.single.name),
        widget.resource.name,
      );
    } catch (_) {
      if (_current) setState(() => _controllerFailed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  Widget _field(String label, Object? value, {bool url = false}) {
    final text = _text(value);
    if (text.isEmpty) return const SizedBox.shrink();
    final safe = url ? gitOpsDisplayUrl(text) : text;
    return Padding(padding: const EdgeInsets.symmetric(vertical: 4), child: Text('$label: $safe', softWrap: true));
  }

  Widget _section(String title, List<Widget> children) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 8),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(title, style: Theme.of(context).textTheme.titleSmall),
        ...children,
      ],
    ),
  );

  List<Widget> _sourceReferences() {
    final result = <Widget>[];
    for (final source in [
      _map(_spec['sourceRef']),
      _map(_map(_map(_spec['chart'])['spec'])['sourceRef']),
      _map(_spec['chartRef']),
    ]) {
      final name = _text(source['name']);
      if (name.isEmpty) continue;
      result.add(
        _link(
          'source.toolkit.fluxcd.io',
          _text(source['kind']),
          name,
          _text(source['namespace']).isEmpty ? widget.resource.namespace : _text(source['namespace']),
        ),
      );
    }
    final generated = _text(_status['helmChart']).split('/');
    if (generated.length == 2 && generated.every((part) => part.isNotEmpty)) {
      result.add(_link('source.toolkit.fluxcd.io', 'HelmChart', generated[1], generated[0]));
    }
    for (final dependency in _list(_spec['dependsOn']).take(200)) {
      final value = _map(dependency);
      if (_text(value['name']).isEmpty) continue;
      result.add(
        _link(
          widget.resource.api.group,
          widget.resource.api.kind,
          _text(value['name']),
          _text(value['namespace']).isEmpty ? widget.resource.namespace : _text(value['namespace']),
        ),
      );
    }
    return result;
  }

  @override
  Widget build(BuildContext context) {
    ref.watch(kubeScopeProvider);
    if (!_current || (!_flux && widget.resource.api.group != 'argoproj.io')) return const SizedBox.shrink();
    final l = context.l10n;
    final spec = _spec, status = _status;
    final artifact = _map(status['artifact']), gitRef = _map(spec['ref']);
    final conditions = _list(status['conditions']).map(_map);
    final source = widget.resource.api.group == 'source.toolkit.fluxcd.io';
    final helm = widget.resource.api.kind == 'HelmRelease';
    final signature = conditions.where((c) => c['type'] == 'SourceVerified').firstOrNull;
    final access = ref.watch(kubeWriteAccessProvider(widget.session)).value;
    final inventory = _list(_map(status['inventory'])['entries']);
    final argoResources = _list(status['resources']);
    final history = _list(status['history']);
    final sourceLinks = _sourceReferences();
    final valuesFrom = _list(spec['valuesFrom']);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (_failed) Text(l.kubeRequestFailed),
        if (_controllerFailed) Text(l.kubeGitOpsControllerUnavailable),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            if (source && widget.service is KubeInsightsService && widget.service is KubeMutationService)
              OutlinedButton.icon(
                key: const ValueKey('gitops-reconcile-source'),
                onPressed: !_busy && access?.canWrite == true ? _reconcile : null,
                icon: const Icon(Icons.sync),
                label: Text(l.kubeGitOpsReconcile),
              ),
            if (_flux)
              TextButton.icon(
                key: const ValueKey('gitops-controller-logs'),
                onPressed: _busy ? null : _controllerLogs,
                icon: const Icon(Icons.article_outlined),
                label: Text(l.kubeGitOpsControllerLogs),
              ),
            if (helm && widget.onHelm != null)
              TextButton.icon(
                onPressed: () {
                  if (!_current) return;
                  final latest = history.isEmpty ? const <String, dynamic>{} : _map(history.first);
                  final name = _text(latest['name']).isNotEmpty ? _text(latest['name']) : _text(spec['releaseName']);
                  final namespace = _text(spec['storageNamespace']).isNotEmpty
                      ? _text(spec['storageNamespace'])
                      : (_text(latest['namespace']).isNotEmpty
                            ? _text(latest['namespace'])
                            : (_text(spec['targetNamespace']).isNotEmpty
                                  ? _text(spec['targetNamespace'])
                                  : widget.resource.namespace ?? 'default'));
                  widget.onHelm!(namespace, name.isNotEmpty ? name : widget.resource.name);
                },
                icon: const Icon(Icons.history),
                label: Text(l.kubeHelmHistory),
              ),
          ],
        ),
        _field(l.kubeGitOpsInterval, spec['interval']),
        if (source)
          _section(l.kubeGitOpsSource, [
            _field('URL', spec['url'], url: true),
            _field(l.kubeGitOpsBranch, gitRef['branch']),
            _field(l.kubeGitOpsTag, gitRef['tag'] ?? gitRef['semver']),
            _field(l.kubeGitOpsRevision, gitRef['commit']),
            _field(
              l.kubeGitOpsSignature,
              signature == null
                  ? l.kubeGitOpsSignatureUnknown
                  : '${_text(signature['status'])} · ${_text(signature['reason'])}',
            ),
            _field(l.kubeGitOpsArtifact, artifact['url'], url: true),
            _field(l.kubeGitOpsRevision, artifact['revision']),
            _field('SHA', artifact['digest']),
          ]),
        if (sourceLinks.isNotEmpty) _section(l.kubeGitOpsSource, sourceLinks),
        if (widget.resource.api.group == 'argoproj.io')
          for (final item in [_map(spec['source']), ..._list(spec['sources']).take(20).map(_map)])
            if (item.isNotEmpty)
              _section(l.kubeGitOpsSource, [
                _field('URL', item['repoURL'], url: true),
                _field(l.kubeGitOpsRevision, item['targetRevision']),
                _field(l.kubeGitOpsPath, item['path']),
                _field(l.kubeGitOpsChart, item['chart']),
              ]),
        _field(l.kubeGitOpsPath, spec['path']),
        _field(l.kubeGitOpsRevision, status['lastAppliedRevision'] ?? status['lastAttemptedRevision']),
        if (helm) ...[
          _field(l.kubeGitOpsChart, _map(_map(spec['chart'])['spec'])['chart']),
          _field(l.kubeGitOpsTag, _map(_map(spec['chart'])['spec'])['version']),
          _field(l.kubeGitOpsDrift, _map(spec['driftDetection'])['mode']),
        ],
        if (valuesFrom.isNotEmpty)
          _section(l.kubeGitOpsValuesFrom, [
            Text(l.kubeGitOpsMetadataOnly),
            for (final entry in valuesFrom.take(200))
              _link(
                '',
                _text(_map(entry)['kind']).isEmpty ? 'Secret' : _text(_map(entry)['kind']),
                _text(_map(entry)['name']),
                widget.resource.namespace,
              ),
          ]),
        if (inventory.isNotEmpty)
          _section(l.kubeGitOpsInventory, [
            for (final entry in inventory.take(200)) _inventoryLink(_map(entry)),
            if (inventory.length > 200) Text(l.kubeInsightsPartial),
          ]),
        if (argoResources.isNotEmpty)
          _section(l.kubeGitOpsInventory, [
            for (final entry in argoResources.take(200))
              _link(
                _text(_map(entry)['group']),
                _text(_map(entry)['kind']),
                _text(_map(entry)['name']),
                _text(_map(entry)['namespace']),
                _text(_map(entry)['version']),
              ),
            if (argoResources.length > 200) Text(l.kubeInsightsPartial),
          ]),
        if (helm || widget.resource.api.group == 'argoproj.io')
          _section(l.kubeHelmHistory, [
            if (history.isEmpty) Text(l.kubeGitOpsHistoryEmpty),
            for (final item in history.take(20)) _historyEntry(_map(item)),
            if (history.length > 20) Text(l.kubeInsightsPartial),
          ]),
      ],
    );
  }

  Widget _inventoryLink(KubeJson entry) {
    final parts = _text(entry['id']).split('_');
    if (parts.length != 4) return const SizedBox.shrink();
    return _link(parts[2], parts[3], parts[1], parts[0], _text(entry['v']));
  }

  Widget _historyEntry(KubeJson entry) {
    final text = [
      entry['version'] ?? entry['id'],
      entry['status'],
      entry['chartName'],
      entry['chartVersion'],
      entry['lastDeployed'] ?? entry['deployedAt'],
      entry['revision'],
    ].map(_text).where((value) => value.isNotEmpty).join(' · ');
    return Padding(padding: const EdgeInsets.symmetric(vertical: 4), child: Text(text, softWrap: true));
  }
}

KubeJson _map(Object? value) => value is KubeJson ? value : const {};
List<dynamic> _list(Object? value) => value is List ? value : const [];
String _text(Object? value) {
  final result = value is String ? value : (value is num || value is bool ? value.toString() : '');
  final clean = result.replaceAll(RegExp(r'[\u0000-\u001f\u007f]'), '');
  return clean.substring(0, clean.length > 1024 ? 1024 : clean.length);
}

/// URLs may embed Git credentials. Display only location/path metadata.
String gitOpsDisplayUrl(String value) {
  final uri = Uri.tryParse(value);
  if (uri == null || !uri.hasScheme || uri.host.isEmpty) return '[redacted]';
  return uri.replace(userInfo: '', query: '', fragment: '').toString().replaceFirst(RegExp(r'[?#]+$'), '');
}
