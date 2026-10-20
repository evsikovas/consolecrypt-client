import 'dart:async';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_comparison.dart';
import 'package:consolecrypt/kubernetes/kube_helm_comparison_service.dart';
import 'package:consolecrypt/kubernetes/kube_history_service.dart';
import 'package:consolecrypt/kubernetes/kube_insights_service.dart';
import 'package:consolecrypt/kubernetes/kube_mutation_service.dart';
import 'package:consolecrypt/kubernetes/kube_operations_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:consolecrypt/kubernetes/kube_write_access.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';
import 'package:url_launcher/url_launcher.dart';

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
    this.onLogsAt,
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
  final void Function(KubeResourceRef controller, String objectFilter, DateTime since)? onLogsAt;
  final void Function(String namespace, String releaseName)? onHelm;
  final VoidCallback? onChanged;
  @override
  ConsumerState<KubeGitOpsDetails> createState() => KubeGitOpsDetailsState();
}

class KubeGitOpsDetailsState extends ConsumerState<KubeGitOpsDetails> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  List<KubeResourceKind> _kinds = const [];
  bool _failed = false, _busy = false, _controllerFailed = false;
  bool _timelinePartial = false;
  List<_TimelineEvent> _events = const [];
  String _latestRevision = '', _sourceRepository = '';
  bool _sourceUnavailable = false;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  bool get _flux => widget.resource.api.group.endsWith('.toolkit.fluxcd.io');
  KubeJson get _spec => _map(widget.detail.object['spec']);
  KubeJson get _status => _map(widget.detail.object['status']);
  @override
  void initState() {
    super.initState();
    unawaited(_discover());
    unawaited(_loadEvents());
  }

  @override
  void didUpdateWidget(covariant KubeGitOpsDetails oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.detail, widget.detail)) {
      unawaited(_loadEvents());
      unawaited(_loadSource());
    }
  }

  int _sourceGeneration = 0;
  Future<void> _loadSource() async {
    final generation = ++_sourceGeneration;
    if (!_current) return;
    setState(() {
      _latestRevision = '';
      _sourceRepository = '';
      _sourceUnavailable = false;
    });
    if (widget.resource.api.group != 'kustomize.toolkit.fluxcd.io' || _kinds.isEmpty) return;
    final source = _map(_spec['sourceRef']);
    final reference = _reference(
      'source.toolkit.fluxcd.io',
      _text(source['kind']),
      _text(source['name']),
      _text(source['namespace']).isEmpty ? widget.resource.namespace : _text(source['namespace']),
    );
    if (reference == null) {
      setState(() => _sourceUnavailable = true);
      return;
    }
    try {
      final detail = await widget.service.getResource(widget.session.id, reference);
      if (!_current || generation != _sourceGeneration) return;
      setState(() {
        _latestRevision = _text(_map(_map(detail.object['status'])['artifact'])['revision']);
        _sourceRepository = gitOpsDisplayUrl(_text(_map(detail.object['spec'])['url']));
      });
    } catch (_) {
      if (_current && generation == _sourceGeneration) setState(() => _sourceUnavailable = true);
    }
  }

  int _eventsGeneration = 0;
  Future<void> _loadEvents() async {
    final generation = ++_eventsGeneration;
    final uid = _text(_map(widget.detail.object['metadata'])['uid']);
    final service = widget.service;
    if (!_current || uid.isEmpty || service is! KubeOperationsService) return;
    final events = <String, _TimelineEvent>{};
    var partial = false;
    if (service is KubeHistoryService) {
      final historyService = service as KubeHistoryService;
      try {
        final status = await historyService.historyStatus(widget.session.id);
        if (!_current || generation != _eventsGeneration) return;
        if (status['enabled'] == true) {
          final history = await historyService.eventHistory(
            widget.session.contextId,
            namespace: widget.resource.namespace,
            limit: 200,
          );
          if (!_current || generation != _eventsGeneration) return;
          partial |= history.length >= 200;
          for (final event in history) {
            if (event['object_uid'] != uid) continue;
            final parsed = _TimelineEvent.fromHistory(event);
            if (parsed != null) events[parsed.uid] = parsed;
          }
        }
      } catch (_) {
        partial = true;
      }
    }
    if (!_current || generation != _eventsGeneration) return;
    try {
      final live = await (service as KubeOperationsService).objectEvents(widget.session.id, widget.resource, uid);
      if (!_current || generation != _eventsGeneration) return;
      partial |= live.length >= 200;
      for (final row in live.take(200)) {
        if (_map(row.object['involvedObject'])['uid'] != uid) continue;
        final parsed = _TimelineEvent.fromLive(row.object);
        if (parsed != null) events[parsed.uid] = parsed;
      }
    } catch (_) {
      partial = true;
    }
    if (!_current || generation != _eventsGeneration) return;
    final sorted = events.values.toList()..sort((a, b) => b.time.compareTo(a.time));
    setState(() {
      _events = sorted.take(20).toList();
      _timelinePartial = partial || sorted.length > 20;
    });
  }

  Future<void> _discover() async {
    if (!_current) return;
    try {
      final kinds = await widget.service.discover(widget.session.id);
      if (_current) {
        setState(() => _kinds = kinds);
        await _loadSource();
      }
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

  Future<void> openControllerLogs({DateTime? since}) async {
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
      final controller = KubeResourceRef(api: api, namespace: rows.single.namespace, name: rows.single.name);
      if (since != null && widget.onLogsAt != null) {
        widget.onLogsAt!(controller, widget.resource.name, since);
      } else {
        widget.onLogs(controller, widget.resource.name);
      }
    } catch (_) {
      if (_current) setState(() => _controllerFailed = true);
    } finally {
      if (_current) setState(() => _busy = false);
    }
  }

  ({String namespace, String name, int revision})? _helmStorage() {
    final history = _list(_status['history']);
    if (history.isEmpty) return null;
    final latest = _map(history.first);
    final revision = latest['version'];
    final name = _text(latest['name']);
    if (revision is! int || revision < 1 || name.isEmpty) return null;
    final storage = _text(_status['storageNamespace']).isNotEmpty
        ? _text(_status['storageNamespace'])
        : _text(_spec['storageNamespace']);
    final namespace = storage.isNotEmpty
        ? storage
        : (_text(latest['namespace']).isNotEmpty ? _text(latest['namespace']) : widget.resource.namespace ?? 'default');
    return (namespace: namespace, name: name, revision: revision);
  }

  Future<void> _compareHelm() async {
    final storage = _helmStorage();
    if (!_current || storage == null || widget.service is! KubeHelmComparisonService) return;
    await showAppDialog<void>(
      context,
      secure: true,
      builder: (context) => AlertDialog(
        title: Text(context.l10n.kubeHelmCompareLive),
        content: SizedBox(
          width: 780,
          height: (MediaQuery.sizeOf(context).height * 0.7).clamp(200, 650),
          child: KubeHelmComparisonPanel(
            session: widget.session,
            namespace: storage.namespace,
            secretName: 'sh.helm.release.v1.${storage.name}.v${storage.revision}',
            service: widget.service as KubeHelmComparisonService,
            autoload: true,
          ),
        ),
        actions: [TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(context.l10n.commonClose))],
      ),
    );
  }

  Widget _field(String label, Object? value, {bool url = false}) {
    final text = _text(value);
    if (text.isEmpty) return const SizedBox.shrink();
    final safe = url ? gitOpsDisplayUrl(text) : text;
    return Padding(padding: const EdgeInsets.symmetric(vertical: 4), child: Text('$label: $safe', softWrap: true));
  }

  Widget _revision(Object? value, {Object? repository, String? label}) {
    final text = _text(value);
    if (text.isEmpty) return const SizedBox.shrink();
    final uri = gitOpsCommitUri(_text(repository), text);
    final l = context.l10n;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Wrap(
        crossAxisAlignment: WrapCrossAlignment.center,
        spacing: 6,
        children: [
          Text('${label ?? l.kubeGitOpsRevision}: $text', softWrap: true),
          IconButton(
            key: ValueKey('gitops-copy-revision-$text'),
            tooltip: l.commonCopy,
            onPressed: () {
              if (_current) unawaited(copyPlainWithNotice(context, ref, text, what: l.copyWhatText));
            },
            icon: const Icon(Icons.copy, size: 16),
          ),
          if (uri != null)
            IconButton(
              key: ValueKey('gitops-open-commit-$text'),
              tooltip: l.commonOpen,
              onPressed: () async {
                if (!_current) return;
                try {
                  if (!await launchUrl(uri, mode: LaunchMode.externalApplication) && _current) {
                    setState(() => _failed = true);
                  }
                } catch (_) {
                  if (_current) setState(() => _failed = true);
                }
              },
              icon: const Icon(Icons.open_in_new, size: 16),
            ),
        ],
      ),
    );
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
                onPressed: _busy ? null : openControllerLogs,
                icon: const Icon(Icons.article_outlined),
                label: Text(l.kubeGitOpsControllerLogs),
              ),
            if (helm && widget.onHelm != null)
              TextButton.icon(
                onPressed: () {
                  if (!_current) return;
                  final latest = history.isEmpty ? const <String, dynamic>{} : _map(history.first);
                  final name = _text(latest['name']).isNotEmpty ? _text(latest['name']) : _text(spec['releaseName']);
                  final namespace = _text(status['storageNamespace']).isNotEmpty
                      ? _text(status['storageNamespace'])
                      : _text(spec['storageNamespace']).isNotEmpty
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
            if (helm && widget.service is KubeHelmComparisonService)
              TextButton.icon(
                onPressed: _helmStorage() == null ? null : _compareHelm,
                icon: const Icon(Icons.compare_arrows),
                label: Text(l.kubeHelmCompareLive),
              ),
          ],
        ),
        _field(l.kubeGitOpsInterval, spec['interval']),
        if (source)
          _section(l.kubeGitOpsSource, [
            _field('URL', spec['url'], url: true),
            _field(l.kubeGitOpsBranch, gitRef['branch']),
            _field(l.kubeGitOpsTag, gitRef['tag'] ?? gitRef['semver']),
            _revision(gitRef['commit'], repository: spec['url']),
            _field(
              l.kubeGitOpsSignature,
              signature == null
                  ? l.kubeGitOpsSignatureUnknown
                  : '${_text(signature['status'])} · ${_text(signature['reason'])}',
            ),
            _field(l.kubeGitOpsArtifact, artifact['url'], url: true),
            _revision(artifact['revision'], repository: spec['url']),
            _field('SHA', artifact['digest']),
          ]),
        if (sourceLinks.isNotEmpty) _section(l.kubeGitOpsSource, sourceLinks),
        if (widget.resource.api.group == 'argoproj.io')
          for (final item in [_map(spec['source']), ..._list(spec['sources']).take(20).map(_map)])
            if (item.isNotEmpty)
              _section(l.kubeGitOpsSource, [
                _field('URL', item['repoURL'], url: true),
                _revision(item['targetRevision'], repository: item['repoURL']),
                _field(l.kubeGitOpsPath, item['path']),
                _field(l.kubeGitOpsChart, item['chart']),
              ]),
        _field(l.kubeGitOpsPath, spec['path']),
        if (widget.resource.api.group == 'kustomize.toolkit.fluxcd.io') ...[
          _revision(status['lastAppliedRevision'], repository: _sourceRepository, label: l.kubeGitOpsAppliedRevision),
          _revision(_latestRevision, repository: _sourceRepository, label: l.kubeGitOpsLatestRevision),
          if (_sourceUnavailable) Text(l.kubeInsightsPartial),
        ] else
          _revision(status['lastAppliedRevision'] ?? status['lastAttemptedRevision']),
        if (helm) ...[
          _field(l.kubeGitOpsChart, _map(_map(spec['chart'])['spec'])['chart']),
          _field(l.kubeGitOpsTag, _map(_map(spec['chart'])['spec'])['version']),
          _field(l.kubeGitOpsDrift, _map(spec['driftDetection'])['mode']),
          for (final test in conditions.where((condition) => condition['type'] == 'TestSuccess'))
            _field(l.kubeHelmTests, '${_text(test['status'])} · ${_text(test['reason'])}'),
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
        if (_events.isNotEmpty || _timelinePartial)
          _section(l.kubeEvents, [
            for (final event in _events)
              ListTile(
                contentPadding: EdgeInsets.zero,
                leading: Icon(event.warning ? Icons.warning_amber : Icons.history, size: 18),
                title: Text(
                  '${MaterialLocalizations.of(context).formatMediumDate(event.time.toLocal())} '
                  '${MaterialLocalizations.of(context).formatTimeOfDay(TimeOfDay.fromDateTime(event.time.toLocal()))}'
                  ' · ${event.reason} · ×${event.count}',
                ),
                subtitle: event.revision.isEmpty ? null : Text(event.revision),
                trailing: !_flux
                    ? null
                    : IconButton(
                        tooltip: l.kubeGitOpsControllerLogs,
                        onPressed: _busy ? null : () => openControllerLogs(since: event.time),
                        icon: const Icon(Icons.article_outlined),
                      ),
              ),
            if (_timelinePartial) Text(l.kubeInsightsPartial),
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
    final since = DateTime.tryParse(_text(entry['lastDeployed'] ?? entry['deployedAt']));
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Wrap(
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Text(text, softWrap: true),
              if (_flux && since != null)
                IconButton(
                  tooltip: context.l10n.kubeGitOpsControllerLogs,
                  onPressed: _busy ? null : () => openControllerLogs(since: since),
                  icon: const Icon(Icons.article_outlined, size: 16),
                ),
            ],
          ),
          if (_map(entry['testHooks']).isNotEmpty) ...[
            Text(context.l10n.kubeHelmTests),
            for (final hook in _map(entry['testHooks']).entries.take(20))
              Text(
                /* l10n-ignore: Helm hook identifier and observed status metadata. */ '${_text(hook.key)} · ${_text(_map(hook.value)['phase'])}' // l10n-ignore: Helm hook identifier and observed status metadata.
                ' · ${_text(_map(hook.value)['lastCompleted'] ?? _map(hook.value)['lastStarted'])}',
              ),
            if (_map(entry['testHooks']).length > 20) Text(context.l10n.kubeInsightsPartial),
          ],
        ],
      ),
    );
  }
}

final class _TimelineEvent {
  const _TimelineEvent(this.uid, this.time, this.reason, this.count, this.warning, this.revision);
  final String uid, reason, revision;
  final DateTime time;
  final int count;
  final bool warning;
  static _TimelineEvent? fromLive(KubeJson event) {
    final metadata = _map(event['metadata']);
    final uid = _text(metadata['uid']);
    final time = DateTime.tryParse(
      _text(
        _map(event['series'])['lastObservedTime'] ??
            event['lastTimestamp'] ??
            event['eventTime'] ??
            metadata['creationTimestamp'],
      ),
    );
    if (uid.isEmpty || time == null) return null;
    final annotations = _map(metadata['annotations']);
    final revision = _text(
      annotations['kustomize.toolkit.fluxcd.io/revision'] ??
          annotations['helm.toolkit.fluxcd.io/revision'] ??
          annotations['source.toolkit.fluxcd.io/revision'],
    );
    final count = event['count'] as num? ?? 1;
    return _TimelineEvent(
      uid,
      time,
      _text(event['reason']),
      count.toInt().clamp(1, 1000000000),
      event['type'] == 'Warning',
      revision,
    );
  }

  static _TimelineEvent? fromHistory(KubeJson event) {
    final uid = _text(event['uid']);
    final time = event['last_seen_ms'];
    if (uid.isEmpty || time is! int) return null;
    final count = event['count'] as num? ?? 1;
    return _TimelineEvent(
      uid,
      DateTime.fromMillisecondsSinceEpoch(time, isUtc: true),
      _text(event['reason']),
      count.toInt().clamp(1, 1000000000),
      event['event_type'] == 'Warning',
      '',
    );
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

/// Build a commit URL from a recognized Git hosting layout. Never propagate
/// credentials, query parameters, fragments, arbitrary schemes or path escapes.
Uri? gitOpsCommitUri(String repository, String revision) {
  if (repository.length > 2048 || revision.length > 512) return null;
  final hash = RegExp(r'(?:^|@)(?:sha1:|sha256:)?([0-9a-fA-F]{7,64})$').firstMatch(revision)?.group(1);
  if (hash == null) return null;
  var source = repository;
  final scp = RegExp(r'^[a-zA-Z0-9_-]+@([a-zA-Z0-9.-]+):(.+)$').firstMatch(source);
  if (scp != null) source = 'ssh://${scp.group(1)}/${scp.group(2)}';
  final uri = Uri.tryParse(source);
  if (uri == null || !const ['https', 'ssh'].contains(uri.scheme) || uri.host.isEmpty) return null;
  final host = uri.host.toLowerCase();
  final parts = uri.pathSegments.where((part) => part.isNotEmpty).toList();
  if (parts.isEmpty) return null;
  if (parts.last.endsWith('.git')) parts[parts.length - 1] = parts.last.substring(0, parts.last.length - 4);
  if (parts.any((part) => part == '.' || part == '..' || !RegExp(r'^[a-zA-Z0-9_.-]+$').hasMatch(part))) return null;
  final List<String> suffix;
  if ((host == 'github.com' || host.startsWith('github.') || host.startsWith('gitea.') || host == 'codeberg.org') &&
      parts.length == 2) {
    suffix = ['commit', hash];
  } else if ((host == 'gitlab.com' || host.startsWith('gitlab.')) && parts.length >= 2) {
    suffix = ['-', 'commit', hash];
  } else if (host == 'bitbucket.org' && parts.length == 2) {
    suffix = ['commits', hash];
  } else {
    return null;
  }
  return Uri(
    scheme: 'https',
    host: host,
    port: uri.scheme == 'https' && uri.hasPort ? uri.port : null,
    pathSegments: [...parts, ...suffix],
  );
}
