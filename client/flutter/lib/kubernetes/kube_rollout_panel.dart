import 'dart:async';
import 'dart:convert';

import 'package:consolecrypt/core/glass/glass.dart';
import 'package:consolecrypt/core/l10n/l10n.dart';
import 'package:consolecrypt/core/widgets/dialogs.dart';
import 'package:consolecrypt/kubernetes/kube_browser_service.dart';
import 'package:consolecrypt/kubernetes/kube_providers.dart';
import 'package:consolecrypt/kubernetes/kube_rollout_service.dart';
import 'package:consolecrypt/kubernetes/kube_service.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:material_ui/material_ui.dart';

class KubeRolloutDialog extends ConsumerStatefulWidget {
  const KubeRolloutDialog({required this.sessionId, required this.resource, required this.service, super.key});
  final String sessionId;
  final KubeResourceRef resource;
  final KubeRolloutService service;
  @override
  ConsumerState<KubeRolloutDialog> createState() => _KubeRolloutDialogState();
}

class _KubeRolloutDialogState extends ConsumerState<KubeRolloutDialog> {
  late final KubeScope _scope = ref.read(kubeScopeProvider);
  List<KubeJson> _history = [];
  KubeJson? _diff;
  int? _before, _after;
  bool _loading = true, _failed = false;
  int _generation = 0;
  bool get _current => mounted && _scope.unlocked && identical(_scope, ref.read(kubeScopeProvider));
  @override
  void initState() {
    super.initState();
    ref.listenManual(kubeScopeProvider, (_, next) {
      if (identical(_scope, next)) return;
      _history.clear();
      _diff = null;
      _generation++;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        final route = ModalRoute.of(context);
        if (route?.isActive == true) Navigator.of(context).removeRoute(route!);
      });
    });
    unawaited(_load());
  }

  @override
  void dispose() {
    _generation++;
    _history.clear();
    _diff = null;
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final history = await widget.service.rolloutHistory(widget.sessionId, widget.resource);
      if (!_current) return;
      setState(() {
        _history = history..sort((a, b) => (b['revision'] as int).compareTo(a['revision'] as int));
        if (_history.isNotEmpty) _after = _history.first['revision'] as int;
        if (_history.length > 1) _before = _history[1]['revision'] as int;
      });
    } catch (_) {
      if (_current) setState(() => _failed = true);
    } finally {
      if (_current) setState(() => _loading = false);
    }
  }

  Future<void> _compare() async {
    if (!_current || _loading || _before == null || _after == null || _before == _after) return;
    final generation = ++_generation;
    setState(() {
      _loading = true;
      _failed = false;
      _diff = null;
    });
    try {
      final diff = await widget.service.rolloutDiff(widget.sessionId, widget.resource, _before!, _after!);
      if (_current && generation == _generation) setState(() => _diff = diff);
    } catch (_) {
      if (_current && generation == _generation) setState(() => _failed = true);
    } finally {
      if (_current && generation == _generation) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l = context.l10n;
    Widget selector(bool before) => Expanded(
      child: DropdownButtonFormField<int>(
        key: ValueKey(before ? 'kube-rollout-before' : 'kube-rollout-after'),
        initialValue: before ? _before : _after,
        isExpanded: true,
        decoration: InputDecoration(labelText: before ? l.kubeDiffBefore : l.kubeDiffAfter),
        items: _history
            .map(
              (row) => DropdownMenuItem(
                value: row['revision'] as int,
                child: Text(/* l10n-ignore: Kubernetes revision number. */ '#${row['revision']}'),
              ),
            )
            .toList(),
        onChanged: _loading
            ? null
            : (value) => setState(() {
                if (before) {
                  _before = value;
                } else {
                  _after = value;
                }
                _diff = null;
              }),
      ),
    );
    return GlassDialog(
      title: '${l.kubeRolloutHistory} · ${widget.resource.name}',
      content: SizedBox(
        width: 800,
        height: MediaQuery.sizeOf(context).height * .65,
        child: !_current
            ? const SizedBox.shrink()
            : Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (_loading) const LinearProgressIndicator(),
                  if (_failed) Text(l.kubeRequestFailed),
                  Row(
                    children: [
                      selector(true),
                      const SizedBox(width: 8),
                      selector(false),
                      GlassIconButton(
                        key: const ValueKey('kube-rollout-compare'),
                        icon: Icons.compare_arrows,
                        tooltip: l.kubeCompare,
                        onPressed: _loading || _before == null || _before == _after ? null : _compare,
                      ),
                    ],
                  ),
                  Expanded(
                    child: _diff != null
                        ? KubeManifestDiffView(diff: _diff!)
                        : ListView.builder(
                            itemCount: _history.length,
                            itemBuilder: (_, index) {
                              final row = _history[index];
                              return ExpansionTile(
                                title: Text('#${row['revision']} · ${row['name']}'), // l10n-ignore: Revision metadata and JSON diff content with mathematical diff markers.
                                subtitle: Text(
                                  [
                                    row['created_at'],
                                    if (row['replicas'] != null) '${l.kubeReplicas}: ${row['replicas']}',
                                  ].whereType<String>().join(' · '),
                                ),
                                children: [
                                  SelectableText(
                                    const JsonEncoder.withIndent('  ').convert(row['template']),
                                    style: GlassTokens.of(context).typography.mono,
                                  ),
                                ],
                              );
                            },
                          ),
                  ),
                ],
              ),
      ),
      primaryAction: GlassButton(label: l.commonClose, onPressed: () => closeDialog<void>(context)),
    );
  }
}

class KubeManifestDiffView extends StatelessWidget {
  const KubeManifestDiffView({required this.diff, super.key});
  final KubeJson diff;
  @override
  Widget build(BuildContext context) {
    final rows = (diff['changes'] as List? ?? const []).cast<KubeJson>();
    final l = context.l10n;
    return ListView.builder(
      key: const ValueKey('kube-path-diff'),
      itemCount: rows.length + 1,
      itemBuilder: (_, index) {
        if (index == 0) {
          return Padding(
            padding: const EdgeInsets.all(8),
            child: Text(
              diff['truncated'] == true
                  ? l.kubeDiffTruncated
                  : rows.isEmpty
                  ? l.kubeNoChanges
                  : l.kubeDiffReview,
            ),
          );
        }
        final row = rows[index - 1];
        return Padding(
          padding: const EdgeInsets.all(8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              SelectableText(row['path'] as String? ?? '', style: Theme.of(context).textTheme.titleSmall),
              SelectableText(
                // l10n-ignore: Revision metadata and JSON diff content with mathematical diff markers.
                /* l10n-ignore: Revision metadata and JSON diff content with mathematical diff markers. */ '− ${const JsonEncoder.withIndent('  ').convert(row['before'])}', // l10n-ignore: Revision metadata and JSON diff content with mathematical diff markers.
                style: GlassTokens.of(context).typography.mono.copyWith(color: Theme.of(context).colorScheme.error),
              ),
              SelectableText(
                // l10n-ignore: Revision metadata and JSON diff content with mathematical diff markers.
                /* l10n-ignore: Revision metadata and JSON diff content with mathematical diff markers. */ '+ ${const JsonEncoder.withIndent('  ').convert(row['after'])}', // l10n-ignore: Revision metadata and JSON diff content with mathematical diff markers.
                style: GlassTokens.of(context).typography.mono,
              ),
            ],
          ),
        );
      },
    );
  }
}
