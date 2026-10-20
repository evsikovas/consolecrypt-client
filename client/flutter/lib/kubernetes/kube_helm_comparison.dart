import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_helm_comparison_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeHelmComparisonPanel extends ConsumerStatefulWidget {
  const KubeHelmComparisonPanel({
    required this.session,
    required this.namespace,
    required this.secretName,
    required this.service,
    this.autoload = false,
    super.key,
  });
  final KubeSession session;
  final String namespace, secretName;
  final KubeHelmComparisonService service;
  final bool autoload;
  @override
  ConsumerState<KubeHelmComparisonPanel> createState() => _KubeHelmComparisonPanelState();
}

class _KubeHelmComparisonPanelState extends ConsumerState<KubeHelmComparisonPanel> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  KubeHelmComparison? _data;
  bool _busy = false, _failed = false;
  int _generation = 0;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    if (widget.autoload) unawaited(_load());
  }

  @override
  void didUpdateWidget(covariant KubeHelmComparisonPanel oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.secretName != widget.secretName ||
        oldWidget.namespace != widget.namespace ||
        oldWidget.session.id != widget.session.id ||
        !identical(oldWidget.service, widget.service)) {
      _generation++;
      _data = null;
      _busy = false;
      _failed = false;
    }
  }

  @override
  void dispose() {
    _generation++;
    _data = null;
    super.dispose();
  }

  Future<void> _load() async {
    if (!_current || _busy) return;
    final generation = ++_generation;
    setState(() {
      _busy = true;
      _failed = false;
      _data = null;
    });
    try {
      final data = await widget.service.compareHelmLive(widget.session.id, widget.namespace, widget.secretName);
      if (_current && generation == _generation) setState(() => _data = data);
    } catch (_) {
      if (_current && generation == _generation) setState(() => _failed = true);
    } finally {
      if (_current && generation == _generation) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.watch(kubeScopeProvider);
    if (!_current) return const SizedBox.shrink();
    final l = context.l10n;
    final data = _data;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(l.kubeHelmCompareHelp),
        Align(
          alignment: Alignment.centerLeft,
          child: TextButton.icon(
            key: const ValueKey('kube-helm-live-load'),
            onPressed: _busy ? null : _load,
            icon: const Icon(Icons.compare_arrows),
            label: Text(l.kubeHelmCompareLive),
          ),
        ),
        if (_busy) const LinearProgressIndicator(),
        if (_failed) Text(l.kubeRequestFailed),
        if (data?.partial == true) Text(l.kubeInsightsPartial),
        if (data?.masked == true) Text(l.kubeHelmCompareMasked),
        if (data != null) Text(l.kubeHelmRevision(data.revision)),
        Expanded(
          child: ListView.builder(
            itemCount: data?.items.length ?? 0,
            itemBuilder: (context, index) {
              final item = data!.items[index];
              final status = switch (item.state) {
                'equal' => l.kubeHelmCompareEqual,
                'changed' => l.kubeHelmCompareChanged,
                'missing' => l.kubeHelmCompareMissing,
                _ => l.kubeRequestFailed,
              };
              return ExpansionTile(
                title: Text('${item.reference.api.kind} · ${item.reference.namespace ?? ''}/${item.reference.name}'), // l10n-ignore: Kubernetes API kind and exact resource identity.
                subtitle: Text(status),
                children: [
                  if ((item.diff['changes'] as List? ?? const []).isNotEmpty)
                    Padding(
                      padding: const EdgeInsets.all(8),
                      child: SelectableText(
                        const JsonEncoder.withIndent('  ').convert(item.diff),
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(fontFamily: 'monospace'),
                      ),
                    ),
                ],
              );
            },
          ),
        ),
      ],
    );
  }
}
